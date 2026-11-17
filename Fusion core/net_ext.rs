//! Networking: TCP and UDP sockets exposed through numeric handles, plus an HTTP/1.1 client
//! and a minimal HTTP/1.1 server toolkit (accept a request, respond to it).
//!
//! HTTP/1.1 server connections support sequential keep-alive requests. `http_request` is a
//! one-shot convenience API; `http_client_*` exposes reusable persistent client sessions.
//! HTTP/2 framing, client sessions and server-push primitives live in `h2_ext`.

use crate::data_ext::Data;
use crate::h2_ext;
use crate::tls_ext::{self, BoxIo, Io, ServerTls};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

enum Resource {
    Listener(TcpListener),
    Stream(TcpStream),
    Udp(UdpSocket),
    /// Protocol-sniffing HTTP server (HTTP/1.1 and HTTP/2, optionally TLS) fed by an acceptor thread.
    HttpServer(Arc<HttpServer>),
    /// Pending reply for one accepted request (see `http_respond`).
    Reply(Mutex<Option<mpsc::Sender<ResponseMsg>>>),
    /// Reusable HTTP/1.1 client connection.
    HttpClient(Arc<Http1Client>),
}

pub struct ResponseMsg {
    status: i64,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

pub struct HttpServer {
    queue: Mutex<VecDeque<Data>>,
    ready: Condvar,
    stop: AtomicBool,
    port: u16,
}

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);

fn registry() -> &'static Mutex<HashMap<i64, Arc<Resource>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<i64, Arc<Resource>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register(resource: Resource) -> i64 {
    let id = NEXT_HANDLE.fetch_add(1, Ordering::SeqCst);
    registry().lock().unwrap().insert(id, Arc::new(resource));
    id
}

fn lookup(handle: i64) -> Result<Arc<Resource>, String> {
    registry()
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| format!("invalid or closed network handle {}", handle))
}

pub fn close(handle: i64) -> Result<(), String> {
    let removed = registry().lock().unwrap().remove(&handle);
    if let Some(res) = removed {
        match &*res {
            Resource::Stream(s) => {
                let _ = s.shutdown(Shutdown::Both);
            }
            Resource::HttpServer(server) => {
                server.stop.store(true, Ordering::SeqCst);
                server.ready.notify_all();
            }
            _ => {}
        }
    }
    Ok(())
}

fn address(host: &str, port: i64) -> Result<std::net::SocketAddr, String> {
    if !(0..=65535).contains(&port) {
        return Err(format!("port {} out of range", port));
    }
    (host, port as u16)
        .to_socket_addrs()
        .map_err(|e| format!("cannot resolve '{}': {}", host, e))?
        .next()
        .ok_or_else(|| format!("no address found for '{}'", host))
}

// ---------------------------------------------------------------------------
// TCP
// ---------------------------------------------------------------------------

pub fn tcp_listen(host: &str, port: i64) -> Result<i64, String> {
    let listener = TcpListener::bind(address(host, port)?)
        .map_err(|e| format!("tcp_listen {}:{}: {}", host, port, e))?;
    Ok(register(Resource::Listener(listener)))
}

pub fn tcp_connect(host: &str, port: i64) -> Result<i64, String> {
    let stream = TcpStream::connect(address(host, port)?)
        .map_err(|e| format!("tcp_connect {}:{}: {}", host, port, e))?;
    Ok(register(Resource::Stream(stream)))
}

pub fn tcp_accept(listener: i64) -> Result<i64, String> {
    let res = lookup(listener)?;
    match &*res {
        Resource::Listener(l) => {
            let (stream, _) = l.accept().map_err(|e| format!("tcp_accept: {}", e))?;
            Ok(register(Resource::Stream(stream)))
        }
        _ => Err("tcp_accept expects a listener handle".to_string()),
    }
}

/// Reads up to `max` bytes. Returns an empty string at end of stream.
pub fn tcp_read(handle: i64, max: i64) -> Result<String, String> {
    let res = lookup(handle)?;
    match &*res {
        Resource::Stream(s) => {
            let mut buf = vec![0u8; max.clamp(1, 1 << 24) as usize];
            let n = (&*s)
                .read(&mut buf)
                .map_err(|e| format!("tcp_read: {}", e))?;
            Ok(String::from_utf8_lossy(&buf[..n]).into_owned())
        }
        _ => Err("tcp_read expects a stream handle".to_string()),
    }
}

pub fn tcp_write(handle: i64, text: &str) -> Result<i64, String> {
    let res = lookup(handle)?;
    match &*res {
        Resource::Stream(s) => {
            (&*s)
                .write_all(text.as_bytes())
                .map_err(|e| format!("tcp_write: {}", e))?;
            Ok(text.len() as i64)
        }
        _ => Err("tcp_write expects a stream handle".to_string()),
    }
}

pub fn set_timeout(handle: i64, millis: i64) -> Result<(), String> {
    let res = lookup(handle)?;
    let t = if millis <= 0 {
        None
    } else {
        Some(Duration::from_millis(millis as u64))
    };
    match &*res {
        Resource::Stream(s) => {
            s.set_read_timeout(t).map_err(|e| e.to_string())?;
            s.set_write_timeout(t).map_err(|e| e.to_string())
        }
        Resource::Udp(u) => {
            u.set_read_timeout(t).map_err(|e| e.to_string())?;
            u.set_write_timeout(t).map_err(|e| e.to_string())
        }
        _ => Err("timeouts apply to streams and UDP sockets".to_string()),
    }
}

pub fn local_port(handle: i64) -> Result<i64, String> {
    let res = lookup(handle)?;
    let addr = match &*res {
        Resource::Listener(l) => l.local_addr(),
        Resource::Stream(s) => s.local_addr(),
        Resource::Udp(u) => u.local_addr(),
        Resource::HttpServer(server) => return Ok(server.port as i64),
        Resource::Reply(_) => return Err("a pending reply has no local port".to_string()),
        Resource::HttpClient(_) => {
            return Err("an HTTP client session has no listening port".to_string())
        }
    };
    addr.map(|a| a.port() as i64).map_err(|e| e.to_string())
}

pub fn peer_address(handle: i64) -> Result<String, String> {
    let res = lookup(handle)?;
    match &*res {
        Resource::Stream(s) => s
            .peer_addr()
            .map(|a| a.to_string())
            .map_err(|e| e.to_string()),
        _ => Err("peer_address expects a stream handle".to_string()),
    }
}

// ---------------------------------------------------------------------------
// UDP
// ---------------------------------------------------------------------------

pub fn udp_bind(host: &str, port: i64) -> Result<i64, String> {
    let socket = UdpSocket::bind(address(host, port)?)
        .map_err(|e| format!("udp_bind {}:{}: {}", host, port, e))?;
    Ok(register(Resource::Udp(socket)))
}

pub fn udp_send_to(handle: i64, host: &str, port: i64, text: &str) -> Result<i64, String> {
    let res = lookup(handle)?;
    match &*res {
        Resource::Udp(u) => u
            .send_to(text.as_bytes(), address(host, port)?)
            .map(|n| n as i64)
            .map_err(|e| format!("udp_send_to: {}", e)),
        _ => Err("udp_send_to expects a UDP handle".to_string()),
    }
}

/// Returns `(datagram text, "ip:port" of the sender)`.
pub fn udp_recv_from(handle: i64, max: i64) -> Result<(String, String), String> {
    let res = lookup(handle)?;
    match &*res {
        Resource::Udp(u) => {
            let mut buf = vec![0u8; max.clamp(1, 65535) as usize];
            let (n, from) = u
                .recv_from(&mut buf)
                .map_err(|e| format!("udp_recv_from: {}", e))?;
            Ok((
                String::from_utf8_lossy(&buf[..n]).into_owned(),
                from.to_string(),
            ))
        }
        _ => Err("udp_recv_from expects a UDP handle".to_string()),
    }
}

// ---------------------------------------------------------------------------
// HTTP/1.1 message parsing shared by client and server
// ---------------------------------------------------------------------------

const MAX_HEAD: usize = 64 * 1024;
const MAX_BODY: usize = 64 * 1024 * 1024;

struct Head {
    first_line: String,
    headers: Vec<(String, String)>,
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn parse_head(raw: &[u8]) -> Result<Head, String> {
    let text = String::from_utf8_lossy(raw);
    let mut lines = text.split("\r\n");
    let first_line = lines.next().unwrap_or("").to_string();
    if first_line.is_empty() {
        return Err("empty HTTP message".to_string());
    }
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| format!("malformed header line '{}'", line))?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok(Head {
        first_line,
        headers,
    })
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn decode_chunked(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = find_subslice(data, b"\r\n").ok_or("truncated chunked body")?;
        let size_text = String::from_utf8_lossy(&data[..end]).to_string();
        let size_hex = size_text.split(';').next().unwrap_or("").trim().to_string();
        let size =
            usize::from_str_radix(&size_hex, 16).map_err(|_| "invalid chunk size".to_string())?;
        data = &data[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 {
            return Err("truncated chunk".to_string());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
        if out.len() > MAX_BODY {
            return Err("HTTP body too large".to_string());
        }
    }
}

/// Reads one complete HTTP message (head + body). With `until_eof`, a body without a length
/// is read until the peer closes the connection (responses); otherwise it is empty (requests).
fn read_message<R: Read + ?Sized>(
    stream: &mut R,
    until_eof: bool,
) -> Result<(Head, Vec<u8>), String> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let head_end = loop {
        if let Some(i) = find_subslice(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err("HTTP header section too large".to_string());
        }
        let n = stream
            .read(&mut chunk)
            .map_err(|e| format!("read: {}", e))?;
        if n == 0 {
            return Err("connection closed before the HTTP headers were complete".to_string());
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = parse_head(&buf[..head_end])?;
    let mut body: Vec<u8> = buf[head_end + 4..].to_vec();
    let chunked = header(&head.headers, "transfer-encoding")
        .map_or(false, |v| v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        loop {
            match decode_chunked(&body) {
                Ok(decoded) => return Ok((head, decoded)),
                Err(e) => {
                    let n = stream
                        .read(&mut chunk)
                        .map_err(|e| format!("read: {}", e))?;
                    if n == 0 {
                        return Err(e);
                    }
                    body.extend_from_slice(&chunk[..n]);
                    if body.len() > MAX_BODY {
                        return Err("HTTP body too large".to_string());
                    }
                }
            }
        }
    }
    if let Some(len) = header(&head.headers, "content-length") {
        let len: usize = len
            .parse()
            .map_err(|_| "invalid Content-Length".to_string())?;
        if len > MAX_BODY {
            return Err("HTTP body too large".to_string());
        }
        while body.len() < len {
            let n = stream
                .read(&mut chunk)
                .map_err(|e| format!("read: {}", e))?;
            if n == 0 {
                return Err("connection closed before the full HTTP body arrived".to_string());
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(len);
        return Ok((head, body));
    }
    if until_eof {
        loop {
            let n = stream
                .read(&mut chunk)
                .map_err(|e| format!("read: {}", e))?;
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
            if body.len() > MAX_BODY {
                return Err("HTTP body too large".to_string());
            }
        }
        return Ok((head, body));
    }
    Ok((head, Vec::new()))
}

fn headers_to_data(headers: &[(String, String)]) -> Data {
    Data::Map(
        headers
            .iter()
            .map(|(k, v)| (k.clone(), Data::Str(v.clone())))
            .collect(),
    )
}

fn reason_phrase(status: i64) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn valid_header_text(s: &str) -> bool {
    !s.contains('\r') && !s.contains('\n')
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

struct Url {
    scheme: String,
    host: String,
    port: i64,
    target: String,
}

struct Http1Client {
    url: Url,
    io: Mutex<Http1Stream>,
}

struct Http1Stream {
    io: BoxIo,
    buffered: Vec<u8>,
    reusable: bool,
}

impl Http1Stream {
    fn read_more(&mut self) -> Result<bool, String> {
        let mut chunk = [0u8; 8192];
        let n = self
            .io
            .read(&mut chunk)
            .map_err(|e| format!("HTTP read: {}", e))?;
        if n == 0 {
            return Ok(false);
        }
        self.buffered.extend_from_slice(&chunk[..n]);
        if self.buffered.len() > MAX_BODY + MAX_HEAD {
            return Err("HTTP response exceeds configured size limits".to_string());
        }
        Ok(true)
    }

    fn read_response(&mut self, method: &str) -> Result<Data, String> {
        let head_end = loop {
            if let Some(end) = find_subslice(&self.buffered, b"\r\n\r\n") {
                break end;
            }
            if self.buffered.len() > MAX_HEAD {
                return Err("HTTP response headers too large".to_string());
            }
            if !self.read_more()? {
                self.reusable = false;
                return Err("connection closed before HTTP response headers".to_string());
            }
        };
        let head = parse_head(&self.buffered[..head_end])?;
        self.buffered.drain(..head_end + 4);
        let mut parts = head.first_line.splitn(3, ' ');
        let version = parts.next().unwrap_or("HTTP/1.1").to_string();
        let status: i64 = parts
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| format!("malformed status line '{}'", head.first_line))?;
        let reason = parts.next().unwrap_or("").to_string();
        // Interim responses (100 Continue, 103 Early Hints) are not the final result.
        if (100..200).contains(&status) && status != 101 {
            return self.read_response(method);
        }
        if status == 101 {
            self.reusable = false;
        }
        let no_body = method.eq_ignore_ascii_case("HEAD")
            || (100..200).contains(&status)
            || status == 204
            || status == 304;
        let body = if no_body {
            Vec::new()
        } else if header(&head.headers, "transfer-encoding")
            .map_or(false, |v| v.to_ascii_lowercase().contains("chunked"))
        {
            loop {
                if let Some((decoded, consumed)) = decode_chunked_complete(&self.buffered)? {
                    self.buffered.drain(..consumed);
                    break decoded;
                }
                if !self.read_more()? {
                    self.reusable = false;
                    return Err("truncated chunked HTTP response".to_string());
                }
            }
        } else if let Some(length) = header(&head.headers, "content-length") {
            let length: usize = length
                .parse()
                .map_err(|_| "invalid response Content-Length".to_string())?;
            if length > MAX_BODY {
                return Err("HTTP response body too large".to_string());
            }
            while self.buffered.len() < length {
                if !self.read_more()? {
                    self.reusable = false;
                    return Err("connection closed before complete HTTP response body".to_string());
                }
            }
            self.buffered.drain(..length).collect()
        } else {
            // A response without explicit framing is close-delimited and cannot be reused.
            self.reusable = false;
            while self.read_more()? {}
            std::mem::take(&mut self.buffered)
        };
        if header_has_token(&head.headers, "connection", "close")
            || (version.eq_ignore_ascii_case("HTTP/1.0")
                && !header_has_token(&head.headers, "connection", "keep-alive"))
        {
            self.reusable = false;
        }
        Ok(response_map(&version, status, reason, &head.headers, &body))
    }
}

/// Parses one complete chunked body and returns its decoded bytes plus the number of wire bytes
/// consumed. `None` means that more bytes are required.
fn decode_chunked_complete(data: &[u8]) -> Result<Option<(Vec<u8>, usize)>, String> {
    let mut pos = 0usize;
    let mut out = Vec::new();
    loop {
        let Some(line_end_rel) = find_subslice(&data[pos..], b"\r\n") else {
            return Ok(None);
        };
        let line_end = pos + line_end_rel;
        let size_text = String::from_utf8_lossy(&data[pos..line_end]);
        let size_hex = size_text.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size_hex, 16).map_err(|_| "invalid chunk size".to_string())?;
        pos = line_end + 2;
        if size == 0 {
            if data.len() >= pos + 2 && &data[pos..pos + 2] == b"\r\n" {
                return Ok(Some((out, pos + 2)));
            }
            let Some(trailer_end_rel) = find_subslice(&data[pos..], b"\r\n\r\n") else {
                return Ok(None);
            };
            return Ok(Some((out, pos + trailer_end_rel + 4)));
        }
        if out.len().saturating_add(size) > MAX_BODY {
            return Err("HTTP body too large".to_string());
        }
        if data.len() < pos.saturating_add(size).saturating_add(2) {
            return Ok(None);
        }
        out.extend_from_slice(&data[pos..pos + size]);
        pos += size;
        if &data[pos..pos + 2] != b"\r\n" {
            return Err("malformed chunk terminator".to_string());
        }
        pos += 2;
    }
}

fn parse_url(url: &str) -> Result<Url, String> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else if let Some(r) = url.strip_prefix("h2c://") {
        ("h2c", r)
    } else {
        return Err(format!(
            "unsupported URL '{}': expected http://, https:// or h2c:// (HTTP/2 without TLS)",
            url
        ));
    };
    let (authority, target) = match rest.find(|c: char| c == '/' || c == '?') {
        Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
        Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
        None => (rest, "/".to_string()),
    };
    if authority.is_empty() {
        return Err("URL has no host".to_string());
    }
    let default_port = if scheme == "https" { 443 } else { 80 };
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        // IPv6 literal: [::1] or [::1]:8080
        let (h, after) = inner
            .split_once(']')
            .ok_or_else(|| "unterminated IPv6 address in URL".to_string())?;
        match after.strip_prefix(':') {
            Some(p) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (
                h.to_string(),
                p.parse::<i64>().map_err(|_| "invalid port".to_string())?,
            ),
            Some(_) => return Err("invalid port in URL".to_string()),
            None => (h.to_string(), default_port),
        }
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (
                h.to_string(),
                p.parse::<i64>().map_err(|_| "invalid port".to_string())?,
            ),
            _ => (authority.to_string(), default_port),
        }
    };
    if !valid_header_text(&host) || !valid_header_text(&target) || target.contains(' ') {
        return Err("URL contains illegal characters".to_string());
    }
    Ok(Url {
        scheme: scheme.to_string(),
        host,
        port,
        target,
    })
}

/// Opens a persistent HTTP/1.1 client connection. HTTPS uses rustls with HTTP/1.1 ALPN.
pub fn http_client_open(url: &str, timeout_ms: i64) -> Result<i64, String> {
    let u = parse_url(url)?;
    if u.scheme == "h2c" {
        return Err("persistent HTTP/1.1 sessions do not support h2c URLs".to_string());
    }
    let timeout = Duration::from_millis(if timeout_ms <= 0 {
        30_000
    } else {
        timeout_ms as u64
    });
    let tcp = TcpStream::connect_timeout(&address(&u.host, u.port)?, timeout)
        .map_err(|e| format!("connect {}:{}: {}", u.host, u.port, e))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let io: BoxIo = if u.scheme == "https" {
        tls_ext::connect_tls(&u.host, tcp, false)?.stream
    } else {
        Box::new(tcp)
    };
    Ok(register(Resource::HttpClient(Arc::new(Http1Client {
        url: u,
        io: Mutex::new(Http1Stream {
            io,
            buffered: Vec::new(),
            reusable: true,
        }),
    }))))
}

/// Sends a request over an existing persistent HTTP/1.1 connection.
pub fn http_client_request(
    handle: i64,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &str,
) -> Result<Data, String> {
    if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("invalid HTTP method '{}'", method));
    }
    if path.is_empty() || !path.starts_with('/') || path.chars().any(|c| c.is_whitespace()) {
        return Err("HTTP request path must be an absolute path beginning with '/' and contain no whitespace or line breaks".to_string());
    }
    check_headers(headers)?;
    if headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err("http_client_request manages message framing; Transfer-Encoding cannot be supplied manually".to_string());
    }
    if let Some((_, value)) = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        let declared: usize = value
            .parse()
            .map_err(|_| "invalid request Content-Length".to_string())?;
        if declared != body.as_bytes().len() {
            return Err("request Content-Length does not match the supplied body".to_string());
        }
    }
    let resource = lookup(handle)?;
    let Resource::HttpClient(client) = &*resource else {
        return Err("http_client_request expects an HTTP client handle".to_string());
    };
    let mut session = client
        .io
        .lock()
        .map_err(|_| "HTTP client connection lock poisoned".to_string())?;
    if !session.reusable {
        return Err("HTTP client connection is no longer reusable; open a new session".to_string());
    }
    let authority = if (client.url.scheme == "https" && client.url.port == 443)
        || (client.url.scheme == "http" && client.url.port == 80)
    {
        client.url.host.clone()
    } else {
        format!("{}:{}", client.url.host, client.url.port)
    };
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: keep-alive\r\nUser-Agent: fusion/0.3\r\n",
        method.to_ascii_uppercase(),
        path,
        authority
    );
    let mut has_length = false;
    for (key, value) in headers {
        if key.eq_ignore_ascii_case("host") || key.eq_ignore_ascii_case("connection") {
            continue;
        }
        if key.eq_ignore_ascii_case("content-length") {
            has_length = true;
        }
        request.push_str(&format!("{}: {}\r\n", key, value));
    }
    if !has_length
        && (!body.is_empty()
            || matches!(
                method.to_ascii_uppercase().as_str(),
                "POST" | "PUT" | "PATCH"
            ))
    {
        request.push_str(&format!("Content-Length: {}\r\n", body.as_bytes().len()));
    }
    request.push_str("\r\n");
    session
        .io
        .write_all(request.as_bytes())
        .map_err(|e| format!("HTTP request write: {}", e))?;
    session
        .io
        .write_all(body.as_bytes())
        .map_err(|e| format!("HTTP request body write: {}", e))?;
    session
        .io
        .flush()
        .map_err(|e| format!("HTTP request flush: {}", e))?;
    session.read_response(method)
}

pub fn http_client_close(handle: i64) -> Result<(), String> {
    close(handle)
}

fn response_map(
    version: &str,
    status: i64,
    reason: String,
    headers: &[(String, String)],
    body: &[u8],
) -> Data {
    Data::Map(vec![
        ("status".to_string(), Data::Int(status)),
        ("reason".to_string(), Data::Str(reason)),
        ("version".to_string(), Data::Str(version.to_string())),
        ("headers".to_string(), headers_to_data(headers)),
        (
            "body".to_string(),
            Data::Str(String::from_utf8_lossy(body).into_owned()),
        ),
    ])
}

fn check_headers(headers: &[(String, String)]) -> Result<(), String> {
    for (k, v) in headers {
        if k.is_empty() || !valid_header_text(k) || !valid_header_text(v) || k.contains(':') {
            return Err("illegal characters in HTTP header".to_string());
        }
    }
    Ok(())
}

fn http1_exchange(
    io: &mut dyn Io,
    u: &Url,
    method: &str,
    headers: &[(String, String)],
    body: &str,
) -> Result<Data, String> {
    let authority =
        if (u.scheme == "https" && u.port == 443) || (u.scheme != "https" && u.port == 80) {
            u.host.clone()
        } else {
            format!("{}:{}", u.host, u.port)
        };
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: fusion/0.3\r\n",
        method.to_ascii_uppercase(),
        u.target,
        authority
    );
    let mut has_length = false;
    for (k, v) in headers {
        if k.eq_ignore_ascii_case("content-length") {
            has_length = true;
        }
        if k.eq_ignore_ascii_case("host") || k.eq_ignore_ascii_case("connection") {
            continue;
        }
        request.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !has_length
        && (!body.is_empty()
            || matches!(
                method.to_ascii_uppercase().as_str(),
                "POST" | "PUT" | "PATCH"
            ))
    {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    io.write_all(request.as_bytes())
        .map_err(|e| format!("write: {}", e))?;
    io.write_all(body.as_bytes())
        .map_err(|e| format!("write: {}", e))?;
    io.flush().map_err(|e| format!("write: {}", e))?;
    let (head, body_bytes) = read_message(io, true)?;
    let mut parts = head.first_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    let status: i64 = parts
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| format!("malformed status line '{}'", head.first_line))?;
    let reason = parts.next().unwrap_or("").to_string();
    Ok(response_map(
        &version,
        status,
        reason,
        &head.headers,
        &body_bytes,
    ))
}

/// Performs one HTTP request. `http://` uses HTTP/1.1; `https://` uses TLS and negotiates HTTP/2
/// or HTTP/1.1 with ALPN; `h2c://` speaks HTTP/2 over plain TCP (prior knowledge).
/// Returns a map with `status`, `reason`, `version`, `headers` and `body`.
pub fn http_request(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: &str,
    timeout_ms: i64,
) -> Result<Data, String> {
    let u = parse_url(url)?;
    if method.is_empty() || !method.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(format!("invalid HTTP method '{}'", method));
    }
    check_headers(headers)?;
    let timeout = Duration::from_millis(if timeout_ms <= 0 {
        30_000
    } else {
        timeout_ms as u64
    });
    let tcp = TcpStream::connect_timeout(&address(&u.host, u.port)?, timeout)
        .map_err(|e| format!("connect {}:{}: {}", u.host, u.port, e))?;
    tcp.set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let mut io: BoxIo;
    let mut use_h2 = false;
    match u.scheme.as_str() {
        "https" => {
            let connected = tls_ext::connect_tls(&u.host, tcp, true)?;
            use_h2 = connected.alpn.as_deref() == Some("h2");
            io = connected.stream;
        }
        "h2c" => {
            use_h2 = true;
            io = Box::new(tcp);
        }
        _ => io = Box::new(tcp),
    }
    if use_h2 {
        let authority =
            if (u.scheme == "https" && u.port == 443) || (u.scheme != "https" && u.port == 80) {
                u.host.clone()
            } else {
                format!("{}:{}", u.host, u.port)
            };
        let scheme = if u.scheme == "https" { "https" } else { "http" };
        let r = h2_ext::client_request(
            &mut *io,
            scheme,
            &authority,
            method,
            &u.target,
            headers,
            body.as_bytes(),
        )?;
        return Ok(response_map(
            "HTTP/2",
            r.status,
            String::new(),
            &r.headers,
            &r.body,
        ));
    }
    http1_exchange(&mut *io, &u, method, headers, body)
}

// ---------------------------------------------------------------------------
// HTTP server toolkit
// ---------------------------------------------------------------------------
//
// Two ways to serve:
//  * `tcp_listen` + `http_accept`: plain HTTP/1.1, requests are read on the calling thread.
//  * `http_listen` / `https_listen` + `http_accept`: an acceptor thread handles connections
//    concurrently, detects HTTP/1.1 vs HTTP/2 (ALPN under TLS, the connection preface on plain
//    TCP) and queues requests for `http_accept`. `http_respond` answers either kind.

fn request_map(
    method: String,
    path: String,
    query: String,
    version: &str,
    headers: &[(String, String)],
    body: &[u8],
    extra: Vec<(String, Data)>,
) -> Data {
    let mut entries = vec![
        ("method".to_string(), Data::Str(method)),
        ("path".to_string(), Data::Str(path)),
        ("query".to_string(), Data::Str(query)),
        ("version".to_string(), Data::Str(version.to_string())),
        ("headers".to_string(), headers_to_data(headers)),
        (
            "body".to_string(),
            Data::Str(String::from_utf8_lossy(body).into_owned()),
        ),
    ];
    entries.extend(extra);
    Data::Map(entries)
}

fn split_target(target: &str) -> (String, String) {
    match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    }
}

/// Starts a protocol-sniffing HTTP server. With `tls`, connections are TLS and ALPN selects h2 or http/1.1.
pub fn http_listen(host: &str, port: i64, tls: Option<ServerTls>) -> Result<i64, String> {
    let listener = TcpListener::bind(address(host, port)?)
        .map_err(|e| format!("http_listen {}:{}: {}", host, port, e))?;
    let bound = listener.local_addr().map_err(|e| e.to_string())?.port();
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let server = Arc::new(HttpServer {
        queue: Mutex::new(VecDeque::new()),
        ready: Condvar::new(),
        stop: AtomicBool::new(false),
        port: bound,
    });
    let worker = server.clone();
    thread::Builder::new()
        .name("fusion-http-acceptor".to_string())
        .spawn(move || accept_loop(listener, worker, tls))
        .map_err(|e| format!("cannot start acceptor thread: {}", e))?;
    Ok(register(Resource::HttpServer(server)))
}

pub fn https_listen(host: &str, port: i64, cert_pem: &str, key_pem: &str) -> Result<i64, String> {
    let config = tls_ext::server_config(cert_pem, key_pem)?;
    http_listen(host, port, Some(config))
}

fn accept_loop(listener: TcpListener, server: Arc<HttpServer>, tls: Option<ServerTls>) {
    while !server.stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((tcp, _)) => {
                let _ = tcp.set_nonblocking(false);
                let (s, t) = (server.clone(), tls.clone());
                let _ = thread::Builder::new()
                    .name("fusion-http-connection".to_string())
                    .spawn(move || handle_connection(tcp, s, t));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// True if the first bytes on a plain connection are the HTTP/2 preface (without consuming them).
fn sniff_h2(tcp: &TcpStream) -> bool {
    let mut buf = [0u8; 24];
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match tcp.peek(&mut buf) {
            Ok(0) | Err(_) => return false,
            Ok(n) => {
                let m = n.min(24);
                if buf[..m] != h2_ext::PREFACE[..m] {
                    return false;
                }
                if n >= 24 {
                    return true;
                }
            }
        }
        if Instant::now() > deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Queues a request and waits for the application to answer it through `http_respond`.
fn dispatch(server: &HttpServer, mut request: Data) -> ResponseMsg {
    let (tx, rx) = mpsc::channel();
    let id = register(Resource::Reply(Mutex::new(Some(tx))));
    if let Data::Map(entries) = &mut request {
        entries.push(("conn".to_string(), Data::Int(id)));
    }
    server.queue.lock().unwrap().push_back(request);
    server.ready.notify_one();
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(message) => message,
        Err(_) => {
            let _ = close(id);
            ResponseMsg {
                status: 504,
                headers: Vec::new(),
                body: b"the handler did not respond in time".to_vec(),
            }
        }
    }
}

fn handle_connection(tcp: TcpStream, server: Arc<HttpServer>, tls: Option<ServerTls>) {
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = tcp.set_write_timeout(Some(Duration::from_secs(30)));
    let scheme = if tls.is_some() { "https" } else { "http" };
    let (mut io, h2): (BoxIo, bool) = match &tls {
        Some(config) => match tls_ext::accept_tls(config, tcp) {
            Ok(connected) => {
                let h2 = connected.alpn.as_deref() == Some("h2");
                (connected.stream, h2)
            }
            Err(_) => return,
        },
        None => {
            let h2 = sniff_h2(&tcp);
            (Box::new(tcp), h2)
        }
    };
    if h2 {
        let mut handler = |req: h2_ext::Request| -> h2_ext::Reply {
            let data = request_map(
                req.method.clone(),
                req.path.clone(),
                req.query.clone(),
                "HTTP/2",
                &req.headers,
                &req.body,
                vec![
                    ("scheme".to_string(), Data::Str(req.scheme.clone())),
                    ("authority".to_string(), Data::Str(req.authority.clone())),
                ],
            );
            let reply = dispatch(&server, data);
            h2_ext::Reply {
                status: reply.status,
                headers: reply.headers,
                body: reply.body,
            }
        };
        let _ = h2_ext::serve(&mut *io, &mut handler);
        return;
    }
    // HTTP/1.1 persistence is sequential: a response is completed before the next
    // request is dispatched. Pipelining is intentionally not advertised because the
    // current application-facing request API has one response token per request.
    for request_number in 0..1000 {
        let (head, body) = match read_message(&mut *io, false) {
            Ok(m) => m,
            Err(_) => return, // EOF, idle timeout, or malformed/truncated request.
        };
        let mut it = head.first_line.splitn(3, ' ');
        let method = it.next().unwrap_or("").to_string();
        let target = it.next().unwrap_or("/").to_string();
        let version = it.next().unwrap_or("HTTP/1.1").to_string();
        if method.is_empty() || !(version == "HTTP/1.1" || version == "HTTP/1.0") {
            let _ = io.write_all(
                b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
            let _ = io.flush();
            return;
        }
        let request_keep_alive = request_wants_keep_alive(&version, &head.headers);
        let (path, query) = split_target(&target);
        let data = request_map(
            method,
            path,
            query,
            &version,
            &head.headers,
            &body,
            vec![("scheme".to_string(), Data::Str(scheme.to_string()))],
        );
        let reply = dispatch(&server, data);
        let response_keep_alive = request_keep_alive
            && request_number < 999
            && !header_has_token(&reply.headers, "connection", "close");
        let bytes = format_h1_response(
            reply.status,
            &reply.headers,
            &reply.body,
            response_keep_alive,
        );
        if io.write_all(&bytes).and_then(|_| io.flush()).is_err() {
            return;
        }
        if !response_keep_alive {
            return;
        }
    }
}

fn accept_queued(server: &HttpServer) -> Result<Data, String> {
    let mut queue = server.queue.lock().unwrap();
    loop {
        if let Some(request) = queue.pop_front() {
            return Ok(request);
        }
        if server.stop.load(Ordering::SeqCst) {
            return Err("http_accept: the server was closed".to_string());
        }
        let (guard, _) = server
            .ready
            .wait_timeout(queue, Duration::from_millis(200))
            .unwrap();
        queue = guard;
    }
}

/// Accepts one request. The returned map has `method`, `path`, `query`, `version`, `headers`,
/// `body` and `conn` (pass `conn` to `http_respond`); servers started with `http_listen` also
/// give `scheme`, and HTTP/2 requests give `authority`.
pub fn http_accept(listener: i64) -> Result<Data, String> {
    let res = lookup(listener)?;
    let l = match &*res {
        Resource::HttpServer(server) => return accept_queued(server),
        Resource::Listener(l) => l,
        _ => {
            return Err(
                "http_accept expects a listener from tcp_listen, http_listen or https_listen"
                    .to_string(),
            )
        }
    };
    loop {
        let (mut stream, _) = l.accept().map_err(|e| format!("http_accept: {}", e))?;
        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
        let (head, body) = match read_message(&mut stream, false) {
            Ok(m) => m,
            Err(_) => {
                // Malformed or aborted request: answer 400 if we still can and keep serving.
                let _ = stream.write_all(
                    b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                continue;
            }
        };
        let mut it = head.first_line.splitn(3, ' ');
        let method = it.next().unwrap_or("").to_string();
        let target = it.next().unwrap_or("/").to_string();
        let version = it.next().unwrap_or("HTTP/1.1").to_string();
        let (path, query) = split_target(&target);
        let conn = register(Resource::Stream(stream));
        return Ok(request_map(
            method,
            path,
            query,
            &version,
            &head.headers,
            &body,
            vec![("conn".to_string(), Data::Int(conn))],
        ));
    }
}

fn header_has_token(headers: &[(String, String)], name: &str, token: &str) -> bool {
    headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .any(|(_, value)| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
}

fn request_wants_keep_alive(version: &str, headers: &[(String, String)]) -> bool {
    if header_has_token(headers, "connection", "close") {
        return false;
    }
    if version.eq_ignore_ascii_case("HTTP/1.1") {
        true
    } else {
        version.eq_ignore_ascii_case("HTTP/1.0")
            && header_has_token(headers, "connection", "keep-alive")
    }
}

fn format_h1_response(
    status: i64,
    headers: &[(String, String)],
    body: &[u8],
    keep_alive: bool,
) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {} {}\r\n", status, reason_phrase(status));
    let mut has_type = false;
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if lower == "content-length"
            || lower == "connection"
            || lower == "keep-alive"
            || lower == "transfer-encoding"
        {
            continue;
        }
        if lower == "content-type" {
            has_type = true;
        }
        out.push_str(&format!("{}: {}\r\n", k, v));
    }
    if !has_type {
        out.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    }
    if keep_alive {
        out.push_str(&format!("Content-Length: {}\r\nConnection: keep-alive\r\nKeep-Alive: timeout=30, max=1000\r\n\r\n", body.len()));
    } else {
        out.push_str(&format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        ));
    }
    let mut bytes = out.into_bytes();
    if status != 204 && status != 304 {
        bytes.extend_from_slice(body);
    }
    bytes
}

/// Sends a response for the request identified by `conn` and finishes that exchange.
pub fn http_respond(
    conn: i64,
    status: i64,
    headers: &[(String, String)],
    body: &str,
) -> Result<(), String> {
    if !(100..=599).contains(&status) {
        return Err(format!("invalid HTTP status {}", status));
    }
    check_headers(headers)?;
    let res = lookup(conn)?;
    match &*res {
        Resource::Reply(slot) => {
            let sender = slot
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| "a response was already sent for this request".to_string())?;
            let delivered = sender.send(ResponseMsg {
                status,
                headers: headers.to_vec(),
                body: body.as_bytes().to_vec(),
            });
            close(conn)?;
            delivered.map_err(|_| "http_respond: the client connection is gone".to_string())
        }
        Resource::Stream(stream) => {
            let bytes = format_h1_response(status, headers, body.as_bytes(), false);
            let mut writer = stream;
            let result = writer.write_all(&bytes);
            let _ = writer.flush();
            close(conn)?;
            result.map_err(|e| format!("http_respond: {}", e))
        }
        _ => Err("http_respond expects a connection from http_accept".to_string()),
    }
}

/// Decodes a URL query string (`a=1&b=x%20y`) into a map.
pub fn parse_query(query: &str) -> Data {
    let mut entries: Vec<(String, Data)> = Vec::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let (k, v) = (percent_decode(k), percent_decode(v));
        match entries.iter_mut().find(|(ek, _)| *ek == k) {
            Some(slot) => slot.1 = Data::Str(v),
            None => entries.push((k, Data::Str(v))),
        }
    }
    Data::Map(entries)
}

pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let decoded = std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match decoded {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod keep_alive_tests {
    use super::*;

    #[test]
    fn http11_is_persistent_unless_close_is_requested() {
        assert!(request_wants_keep_alive("HTTP/1.1", &[]));
        assert!(!request_wants_keep_alive(
            "HTTP/1.1",
            &[("Connection".into(), "keep-alive, close".into())]
        ));
    }

    #[test]
    fn http10_requires_explicit_keep_alive() {
        assert!(!request_wants_keep_alive("HTTP/1.0", &[]));
        assert!(request_wants_keep_alive(
            "HTTP/1.0",
            &[("connection".into(), "keep-alive".into())]
        ));
    }

    #[test]
    fn response_framing_keeps_connection_open_only_when_requested() {
        let keep = String::from_utf8(format_h1_response(200, &[], b"ok", true)).unwrap();
        assert!(keep.contains("Connection: keep-alive\r\n"));
        assert!(keep.contains("Content-Length: 2\r\n"));
        let close = String::from_utf8(format_h1_response(200, &[], b"ok", false)).unwrap();
        assert!(close.contains("Connection: close\r\n"));
    }

    #[test]
    fn chunked_response_parser_preserves_bytes_after_message() {
        let wire = b"5\r\nhello\r\n0\r\n\r\nnext";
        let (body, consumed) = decode_chunked_complete(wire).unwrap().unwrap();
        assert_eq!(body, b"hello");
        assert_eq!(&wire[consumed..], b"next");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn url_parsing() {
        let u = parse_url("http://example.com:8080/a/b?x=1").unwrap();
        assert_eq!(
            (u.host.as_str(), u.port, u.target.as_str()),
            ("example.com", 8080, "/a/b?x=1")
        );
        let u = parse_url("http://example.com").unwrap();
        assert_eq!((u.port, u.target.as_str()), (80, "/"));
        let u = parse_url("https://example.com/x").unwrap();
        assert_eq!((u.scheme.as_str(), u.port), ("https", 443));
        let u = parse_url("h2c://[::1]:9000/p").unwrap();
        assert_eq!(
            (u.scheme.as_str(), u.host.as_str(), u.port),
            ("h2c", "::1", 9000)
        );
        assert!(parse_url("ftp://example.com").is_err());
    }

    #[test]
    fn percent_coding() {
        assert_eq!(percent_decode("a%20b+c%2"), "a b c%2");
        assert_eq!(percent_encode("a b/é"), "a%20b%2F%C3%A9");
        assert_eq!(
            parse_query("a=1&b=x%20y&a=2").get("a"),
            Some(&Data::Str("2".into()))
        );
    }

    #[test]
    fn chunked_decoding() {
        assert_eq!(
            decode_chunked(b"5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap(),
            b"hello world"
        );
        assert!(decode_chunked(b"zz\r\n").is_err());
    }

    #[test]
    fn tcp_echo() {
        let listener = tcp_listen("127.0.0.1", 0).unwrap();
        let port = local_port(listener).unwrap();
        let server = thread::spawn(move || {
            let conn = tcp_accept(listener).unwrap();
            let text = tcp_read(conn, 1024).unwrap();
            tcp_write(conn, &text.to_uppercase()).unwrap();
            close(conn).unwrap();
        });
        let c = tcp_connect("127.0.0.1", port).unwrap();
        tcp_write(c, "hello").unwrap();
        assert_eq!(tcp_read(c, 1024).unwrap(), "HELLO");
        server.join().unwrap();
        close(c).unwrap();
        close(listener).unwrap();
        assert!(tcp_read(c, 10).is_err());
    }

    #[test]
    fn udp_round_trip() {
        let a = udp_bind("127.0.0.1", 0).unwrap();
        let b = udp_bind("127.0.0.1", 0).unwrap();
        set_timeout(b, 2000).unwrap();
        udp_send_to(a, "127.0.0.1", local_port(b).unwrap(), "ping").unwrap();
        let (text, from) = udp_recv_from(b, 64).unwrap();
        assert_eq!(text, "ping");
        assert!(from.starts_with("127.0.0.1:"));
    }

    #[test]
    fn http_client_and_server() {
        let listener = tcp_listen("127.0.0.1", 0).unwrap();
        let port = local_port(listener).unwrap();
        let server = thread::spawn(move || {
            let req = http_accept(listener).unwrap();
            assert_eq!(req.get("method"), Some(&Data::Str("POST".into())));
            assert_eq!(req.get("path"), Some(&Data::Str("/echo".into())));
            let body = match req.get("body") {
                Some(Data::Str(s)) => s.clone(),
                _ => String::new(),
            };
            let Some(Data::Int(conn)) = req.get("conn") else {
                panic!("no conn")
            };
            http_respond(
                *conn,
                201,
                &[("X-Test".to_string(), "yes".to_string())],
                &format!("got:{}", body),
            )
            .unwrap();
        });
        let url = format!("http://127.0.0.1:{}/echo?x=1", port);
        let resp = http_request("POST", &url, &[], "payload", 5000).unwrap();
        assert_eq!(resp.get("status"), Some(&Data::Int(201)));
        assert_eq!(resp.get("body"), Some(&Data::Str("got:payload".into())));
        assert_eq!(
            resp.get("headers").unwrap().get("x-test"),
            Some(&Data::Str("yes".into()))
        );
        server.join().unwrap();
        close(listener).unwrap();
    }

    #[test]
    fn persistent_http_client_reuses_one_http11_connection() {
        let server = http_listen("127.0.0.1", 0, None).unwrap();
        let port = local_port(server).unwrap();
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let req = http_accept(server).unwrap();
                let Some(Data::Str(path)) = req.get("path").cloned() else {
                    panic!("missing path")
                };
                let Some(Data::Int(conn)) = req.get("conn").cloned() else {
                    panic!("missing response token")
                };
                http_respond(conn, 200, &[], &format!("served:{}", path)).unwrap();
            }
        });
        let client = http_client_open(&format!("http://127.0.0.1:{}/", port), 5000).unwrap();
        let first = http_client_request(client, "GET", "/first", &[], "").unwrap();
        let second = http_client_request(client, "GET", "/second", &[], "").unwrap();
        assert_eq!(first.get("body"), Some(&Data::Str("served:/first".into())));
        assert_eq!(
            second.get("body"),
            Some(&Data::Str("served:/second".into()))
        );
        http_client_close(client).unwrap();
        worker.join().unwrap();
        close(server).unwrap();
    }

    #[test]
    fn http_listen_serves_http1_concurrently_and_detects_the_protocol() {
        let server = http_listen("127.0.0.1", 0, None).unwrap();
        let port = local_port(server).unwrap();
        let worker = thread::spawn(move || {
            for _ in 0..2 {
                let req = http_accept(server).unwrap();
                let Some(Data::Str(path)) = req.get("path").cloned() else {
                    panic!("no path")
                };
                let Some(Data::Int(conn)) = req.get("conn").cloned() else {
                    panic!("no conn")
                };
                http_respond(conn, 200, &[], &format!("hi {}", path)).unwrap();
            }
        });
        // Two clients at once: the acceptor thread must not serialise connection handling.
        let a = thread::spawn(move || {
            http_request(
                "GET",
                &format!("http://127.0.0.1:{}/a", port),
                &[],
                "",
                5000,
            )
            .unwrap()
        });
        let b = thread::spawn(move || {
            http_request(
                "GET",
                &format!("http://127.0.0.1:{}/b", port),
                &[],
                "",
                5000,
            )
            .unwrap()
        });
        let (ra, rb) = (a.join().unwrap(), b.join().unwrap());
        assert_eq!(ra.get("body"), Some(&Data::Str("hi /a".into())));
        assert_eq!(rb.get("body"), Some(&Data::Str("hi /b".into())));
        assert_eq!(ra.get("version"), Some(&Data::Str("HTTP/1.1".into())));
        worker.join().unwrap();
        close(server).unwrap();
    }

    #[cfg(not(feature = "tls"))]
    #[test]
    fn https_without_tls_support_explains_how_to_enable_it() {
        let err = https_listen("127.0.0.1", 0, "cert.pem", "key.pem").unwrap_err();
        assert!(err.contains("--features tls"), "{}", err);
    }

    #[cfg(feature = "http2")]
    #[test]
    fn http2_cleartext_round_trip_through_the_server_toolkit() {
        let server = http_listen("127.0.0.1", 0, None).unwrap();
        let port = local_port(server).unwrap();
        let worker = thread::spawn(move || {
            let req = http_accept(server).unwrap();
            assert_eq!(req.get("version"), Some(&Data::Str("HTTP/2".into())));
            let Some(Data::Int(conn)) = req.get("conn").cloned() else {
                panic!("no conn")
            };
            let Some(Data::Str(body)) = req.get("body").cloned() else {
                panic!("no body")
            };
            http_respond(
                conn,
                201,
                &[("x-seen".to_string(), "yes".to_string())],
                &format!("echo:{}", body),
            )
            .unwrap();
        });
        let resp = http_request(
            "POST",
            &format!("h2c://127.0.0.1:{}/h2", port),
            &[],
            "payload",
            5000,
        )
        .unwrap();
        assert_eq!(resp.get("status"), Some(&Data::Int(201)));
        assert_eq!(resp.get("version"), Some(&Data::Str("HTTP/2".into())));
        assert_eq!(resp.get("body"), Some(&Data::Str("echo:payload".into())));
        assert_eq!(
            resp.get("headers").unwrap().get("x-seen"),
            Some(&Data::Str("yes".into()))
        );
        worker.join().unwrap();
        close(server).unwrap();
    }
}
