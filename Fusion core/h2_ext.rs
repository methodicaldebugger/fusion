//! HTTP/2 (RFC 9113): framing, stream state, flow control, a client exchange and a server loop.
//!
//! Frames, flow control and stream handling are implemented here with `std` only. HPACK header
//! compression (RFC 7541, including its Huffman table) comes from the `hpack` crate, enabled with
//! the `http2` Cargo feature; without it `Codec` returns an error naming the feature. Use HTTP/2
//! over TLS (ALPN `h2`, needs the `tls` feature) or in cleartext with prior knowledge (`h2c`).
//!
//! The client session can keep multiple streams in flight and collect responses by stream id.
//! `serve_with_push` emits PUSH_PROMISE plus the pushed response. The server callback remains
//! synchronous, stream priorities are not implemented, and this is not yet a full RFC conformance
//! implementation.

use crate::tls_ext::Io;
use std::collections::{HashMap, HashSet, VecDeque};

pub const PREFACE: &[u8] = b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n";

const DATA: u8 = 0;
const HEADERS: u8 = 1;
const PRIORITY: u8 = 2;
const RST_STREAM: u8 = 3;
const SETTINGS: u8 = 4;
const PUSH_PROMISE: u8 = 5;
const PING: u8 = 6;
const GOAWAY: u8 = 7;
const WINDOW_UPDATE: u8 = 8;
const CONTINUATION: u8 = 9;

const END_STREAM: u8 = 0x1;
const ACK: u8 = 0x1;
const END_HEADERS: u8 = 0x4;
const PADDED: u8 = 0x8;
const PRIORITY_FLAG: u8 = 0x20;

const DEFAULT_WINDOW: i64 = 65_535;
const DEFAULT_MAX_FRAME: usize = 16_384;
const MAX_INCOMING_FRAME: usize = 1 << 20;
const MAX_BODY: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub kind: u8,
    pub flags: u8,
    pub stream: u32,
    pub payload: Vec<u8>,
}

pub fn write_frame(io: &mut dyn Io, kind: u8, flags: u8, stream: u32, payload: &[u8]) -> Result<(), String> {
    let len = payload.len();
    if len > 0x00ff_ffff {
        return Err("HTTP/2 frame too large".to_string());
    }
    let mut buf = Vec::with_capacity(9 + len);
    buf.extend_from_slice(&[(len >> 16) as u8, (len >> 8) as u8, len as u8, kind, flags]);
    buf.extend_from_slice(&(stream & 0x7fff_ffff).to_be_bytes());
    buf.extend_from_slice(payload);
    io.write_all(&buf).and_then(|_| io.flush()).map_err(|e| format!("HTTP/2 write: {}", e))
}

/// Message used when the peer closes the connection (so callers can tell it from a protocol error).
pub const CONNECTION_CLOSED: &str = "HTTP/2 connection closed";

fn read_error(e: std::io::Error) -> String {
    use std::io::ErrorKind::*;
    match e.kind() {
        UnexpectedEof | ConnectionReset | ConnectionAborted | BrokenPipe => CONNECTION_CLOSED.to_string(),
        _ => format!("HTTP/2 read: {}", e),
    }
}

pub fn read_frame(io: &mut dyn Io) -> Result<Frame, String> {
    let mut head = [0u8; 9];
    io.read_exact(&mut head).map_err(read_error)?;
    let len = ((head[0] as usize) << 16) | ((head[1] as usize) << 8) | head[2] as usize;
    if len > MAX_INCOMING_FRAME {
        return Err(format!("HTTP/2 frame of {} bytes exceeds the limit", len));
    }
    let mut payload = vec![0u8; len];
    io.read_exact(&mut payload).map_err(read_error)?;
    Ok(Frame { kind: head[3], flags: head[4], stream: u32::from_be_bytes([head[5], head[6], head[7], head[8]]) & 0x7fff_ffff, payload })
}

// ---------------------------------------------------------------------------
// HPACK
// ---------------------------------------------------------------------------

pub type HeaderList = Vec<(String, String)>;

pub struct Codec {
    #[cfg(feature = "http2")]
    encoder: hpack::Encoder<'static>,
    #[cfg(feature = "http2")]
    decoder: hpack::Decoder<'static>,
}

impl Codec {
    #[cfg(feature = "http2")]
    pub fn new() -> Codec {
        Codec { encoder: hpack::Encoder::new(), decoder: hpack::Decoder::new() }
    }

    #[cfg(not(feature = "http2"))]
    pub fn new() -> Codec {
        Codec {}
    }

    #[cfg(feature = "http2")]
    pub fn encode(&mut self, headers: &[(String, String)]) -> Result<Vec<u8>, String> {
        Ok(self.encoder.encode(headers.iter().map(|(k, v)| (k.as_bytes(), v.as_bytes()))))
    }

    #[cfg(feature = "http2")]
    pub fn decode(&mut self, block: &[u8]) -> Result<HeaderList, String> {
        let pairs = self.decoder.decode(block).map_err(|e| format!("HPACK decoding failed: {:?}", e))?;
        Ok(pairs.into_iter().map(|(k, v)| (String::from_utf8_lossy(&k).into_owned(), String::from_utf8_lossy(&v).into_owned())).collect())
    }

    #[cfg(not(feature = "http2"))]
    pub fn encode(&mut self, _headers: &[(String, String)]) -> Result<Vec<u8>, String> {
        Err("HTTP/2 needs header compression: rebuild Fusion with `cargo build --features http2`".to_string())
    }

    #[cfg(not(feature = "http2"))]
    pub fn decode(&mut self, _block: &[u8]) -> Result<HeaderList, String> {
        Err("HTTP/2 needs header compression: rebuild Fusion with `cargo build --features http2`".to_string())
    }
}

impl Default for Codec {
    fn default() -> Self {
        Codec::new()
    }
}

// ---------------------------------------------------------------------------
// Connection state shared by client and server
// ---------------------------------------------------------------------------

/// A complete message received on one stream: a request (server) or a response (client).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub stream: u32,
    pub headers: HeaderList,
    pub body: Vec<u8>,
}

struct Block {
    stream: u32,
    data: Vec<u8>,
    end_stream: bool,
    promised_stream: Option<u32>,
}

#[derive(Default)]
struct Partial {
    headers: HeaderList,
    body: Vec<u8>,
    seen_headers: bool,
}

pub struct Conn {
    codec: Codec,
    conn_send: i64,
    initial_send: i64,
    max_frame: usize,
    stream_send: HashMap<u32, i64>,
    block: Option<Block>,
    streams: HashMap<u32, Partial>,
    /// Complete messages, oldest first.
    pub ready: VecDeque<Message>,
    /// Streams the peer reset, with the error code.
    pub reset: HashMap<u32, u32>,
    /// Set when the peer sent GOAWAY (error code).
    pub goaway: Option<u32>,
    /// Whether the peer permits server push (HTTP/2 SETTINGS_ENABLE_PUSH).
    peer_push_enabled: bool,
    next_push_stream: u32,
    /// Promised even-numbered streams received from the peer.
    pub promised_streams: HashSet<u32>,
}

impl Default for Conn {
    fn default() -> Self {
        Conn::new()
    }
}

impl Conn {
    pub fn new() -> Conn {
        Conn {
            codec: Codec::new(),
            conn_send: DEFAULT_WINDOW,
            initial_send: DEFAULT_WINDOW,
            max_frame: DEFAULT_MAX_FRAME,
            stream_send: HashMap::new(),
            block: None,
            streams: HashMap::new(),
            ready: VecDeque::new(),
            reset: HashMap::new(),
            goaway: None,
            peer_push_enabled: true,
            next_push_stream: 2,
            promised_streams: HashSet::new(),
        }
    }

    fn window_for(&mut self, stream: u32) -> &mut i64 {
        let initial = self.initial_send;
        self.stream_send.entry(stream).or_insert(initial)
    }

    /// Processes one received frame (acknowledging SETTINGS and PING, tracking windows, assembling messages).
    pub fn handle(&mut self, io: &mut dyn Io, frame: Frame) -> Result<(), String> {
        if let Some(block) = &self.block {
            if frame.kind != CONTINUATION || frame.stream != block.stream {
                return Err("HTTP/2 protocol error: header block interrupted".to_string());
            }
        }
        match frame.kind {
            SETTINGS => {
                if frame.flags & ACK != 0 {
                    return Ok(());
                }
                if frame.payload.len() % 6 != 0 {
                    return Err("HTTP/2 protocol error: malformed SETTINGS".to_string());
                }
                for entry in frame.payload.chunks(6) {
                    let id = u16::from_be_bytes([entry[0], entry[1]]);
                    let value = u32::from_be_bytes([entry[2], entry[3], entry[4], entry[5]]);
                    match id {
                        0x2 => {
                            if value > 1 {
                                return Err("HTTP/2 protocol error: ENABLE_PUSH must be 0 or 1".to_string());
                            }
                            self.peer_push_enabled = value == 1;
                        }
                        0x4 => {
                            if value > 0x7fff_ffff {
                                return Err("HTTP/2 protocol error: window size too large".to_string());
                            }
                            let delta = value as i64 - self.initial_send;
                            self.initial_send = value as i64;
                            for w in self.stream_send.values_mut() {
                                *w += delta;
                            }
                        }
                        0x5 => {
                            if !(16_384..=16_777_215).contains(&value) {
                                return Err("HTTP/2 protocol error: invalid MAX_FRAME_SIZE".to_string());
                            }
                            self.max_frame = value as usize;
                        }
                        _ => {}
                    }
                }
                write_frame(io, SETTINGS, ACK, 0, &[])
            }
            PING => {
                if frame.flags & ACK == 0 {
                    write_frame(io, PING, ACK, 0, &frame.payload)?;
                }
                Ok(())
            }
            WINDOW_UPDATE => {
                if frame.payload.len() != 4 {
                    return Err("HTTP/2 protocol error: malformed WINDOW_UPDATE".to_string());
                }
                let inc = (u32::from_be_bytes([frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3]]) & 0x7fff_ffff) as i64;
                if frame.stream == 0 {
                    self.conn_send += inc;
                } else {
                    *self.window_for(frame.stream) += inc;
                }
                Ok(())
            }
            GOAWAY => {
                let code = if frame.payload.len() >= 8 { u32::from_be_bytes([frame.payload[4], frame.payload[5], frame.payload[6], frame.payload[7]]) } else { 0 };
                self.goaway = Some(code);
                Ok(())
            }
            RST_STREAM => {
                let code = if frame.payload.len() >= 4 { u32::from_be_bytes([frame.payload[0], frame.payload[1], frame.payload[2], frame.payload[3]]) } else { 0 };
                self.reset.insert(frame.stream, code);
                self.streams.remove(&frame.stream);
                Ok(())
            }
            PRIORITY => Ok(()),
            PUSH_PROMISE => {
                if frame.stream == 0 || frame.payload.len() < 4 {
                    return Err("HTTP/2 protocol error: malformed PUSH_PROMISE".to_string());
                }
                if !self.peer_push_enabled {
                    return Err("HTTP/2 protocol error: peer sent PUSH_PROMISE after push was disabled".to_string());
                }
                let mut data: &[u8] = &frame.payload;
                let mut pad = 0usize;
                if frame.flags & PADDED != 0 {
                    pad = *data.first().ok_or("HTTP/2 protocol error: truncated PUSH_PROMISE")? as usize;
                    data = &data[1..];
                }
                if data.len() < 4 || pad > data.len() - 4 {
                    return Err("HTTP/2 protocol error: malformed PUSH_PROMISE padding".to_string());
                }
                let promised = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) & 0x7fff_ffff;
                if promised == 0 || promised % 2 != 0 || self.promised_streams.contains(&promised) {
                    return Err("HTTP/2 protocol error: invalid promised stream id".to_string());
                }
                let fragment = data[4..data.len() - pad].to_vec();
                if frame.flags & END_HEADERS != 0 {
                    self.finish_push_promise(promised, fragment)
                } else {
                    self.block = Some(Block { stream: frame.stream, data: fragment, end_stream: false, promised_stream: Some(promised) });
                    Ok(())
                }
            },
            HEADERS => {
                if frame.stream == 0 {
                    return Err("HTTP/2 protocol error: HEADERS on stream 0".to_string());
                }
                let mut data: &[u8] = &frame.payload;
                let mut pad = 0usize;
                if frame.flags & PADDED != 0 {
                    pad = *data.first().ok_or("HTTP/2 protocol error: truncated HEADERS")? as usize;
                    data = &data[1..];
                }
                if frame.flags & PRIORITY_FLAG != 0 {
                    if data.len() < 5 {
                        return Err("HTTP/2 protocol error: truncated HEADERS".to_string());
                    }
                    data = &data[5..];
                }
                if pad > data.len() {
                    return Err("HTTP/2 protocol error: bad padding".to_string());
                }
                let fragment = data[..data.len() - pad].to_vec();
                let end_stream = frame.flags & END_STREAM != 0;
                if frame.flags & END_HEADERS != 0 {
                    self.finish_block(frame.stream, fragment, end_stream)
                } else {
                    self.block = Some(Block { stream: frame.stream, data: fragment, end_stream, promised_stream: None });
                    Ok(())
                }
            }
            CONTINUATION => {
                let mut block = self.block.take().ok_or("HTTP/2 protocol error: unexpected CONTINUATION")?;
                block.data.extend_from_slice(&frame.payload);
                if block.data.len() > MAX_INCOMING_FRAME {
                    return Err("HTTP/2 header block too large".to_string());
                }
                if frame.flags & END_HEADERS != 0 {
                    if let Some(promised) = block.promised_stream {
                        self.finish_push_promise(promised, block.data)
                    } else {
                        self.finish_block(block.stream, block.data, block.end_stream)
                    }
                } else {
                    self.block = Some(block);
                    Ok(())
                }
            }
            DATA => {
                if frame.stream == 0 {
                    return Err("HTTP/2 protocol error: DATA on stream 0".to_string());
                }
                let total = frame.payload.len();
                let mut data: &[u8] = &frame.payload;
                if frame.flags & PADDED != 0 {
                    let pad = *data.first().ok_or("HTTP/2 protocol error: truncated DATA")? as usize;
                    data = &data[1..];
                    if pad > data.len() {
                        return Err("HTTP/2 protocol error: bad padding".to_string());
                    }
                    data = &data[..data.len() - pad];
                }
                let end_stream = frame.flags & END_STREAM != 0;
                if let Some(p) = self.streams.get_mut(&frame.stream) {
                    if p.body.len() + data.len() > MAX_BODY {
                        return Err("HTTP/2 body too large".to_string());
                    }
                    p.body.extend_from_slice(data);
                }
                if total > 0 {
                    let inc = (total as u32).to_be_bytes();
                    write_frame(io, WINDOW_UPDATE, 0, 0, &inc)?;
                    if !end_stream {
                        write_frame(io, WINDOW_UPDATE, 0, frame.stream, &inc)?;
                    }
                }
                if end_stream {
                    self.complete(frame.stream);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn finish_block(&mut self, stream: u32, fragment: Vec<u8>, end_stream: bool) -> Result<(), String> {
        let headers = self.codec.decode(&fragment)?;
        let partial = self.streams.entry(stream).or_default();
        partial.headers.extend(headers);
        partial.seen_headers = true;
        if end_stream {
            self.complete(stream);
        }
        Ok(())
    }

    fn finish_push_promise(&mut self, promised: u32, fragment: Vec<u8>) -> Result<(), String> {
        if fragment.len() > MAX_INCOMING_FRAME {
            return Err("HTTP/2 PUSH_PROMISE header block too large".to_string());
        }
        let _promised_request = self.codec.decode(&fragment)?;
        self.promised_streams.insert(promised);
        Ok(())
    }

    fn complete(&mut self, stream: u32) {
        if let Some(p) = self.streams.remove(&stream) {
            if p.seen_headers {
                self.ready.push_back(Message { stream, headers: p.headers, body: p.body });
            }
        }
    }

    /// Reads and handles one frame.
    pub fn pump(&mut self, io: &mut dyn Io) -> Result<(), String> {
        let frame = read_frame(io)?;
        self.handle(io, frame)
    }

    /// Sends a PUSH_PROMISE for a promised request and allocates an even stream id.
    /// Large HPACK blocks are split across PUSH_PROMISE and CONTINUATION frames.
    pub fn send_push_promise(&mut self, io: &mut dyn Io, parent_stream: u32, request_headers: &[(String, String)]) -> Result<u32, String> {
        if !self.peer_push_enabled {
            return Err("HTTP/2 server push was disabled by the client (SETTINGS_ENABLE_PUSH=0)".to_string());
        }
        if parent_stream == 0 || parent_stream % 2 == 0 {
            return Err("HTTP/2 PUSH_PROMISE requires an active client-initiated parent stream".to_string());
        }
        let promised = self.next_push_stream;
        self.next_push_stream = self.next_push_stream.checked_add(2).ok_or("HTTP/2 promised stream id exhausted")?;
        let block = self.codec.encode(request_headers)?;
        if block.len() > MAX_INCOMING_FRAME {
            return Err("HTTP/2 PUSH_PROMISE header block too large".to_string());
        }
        let first_capacity = self.max_frame.saturating_sub(4).max(1);
        let first_len = block.len().min(first_capacity);
        let mut payload = Vec::with_capacity(4 + first_len);
        payload.extend_from_slice(&promised.to_be_bytes());
        payload.extend_from_slice(&block[..first_len]);
        let remaining = &block[first_len..];
        write_frame(io, PUSH_PROMISE, if remaining.is_empty() { END_HEADERS } else { 0 }, parent_stream, &payload)?;
        for (index, chunk) in remaining.chunks(self.max_frame.max(1)).enumerate() {
            let is_last = (index + 1) * self.max_frame >= remaining.len();
            write_frame(io, CONTINUATION, if is_last { END_HEADERS } else { 0 }, parent_stream, chunk)?;
        }
        Ok(promised)
    }

    /// Sends a header block, splitting it into HEADERS + CONTINUATION frames if needed.
    pub fn send_headers(&mut self, io: &mut dyn Io, stream: u32, headers: &[(String, String)], end_stream: bool) -> Result<(), String> {
        let block = self.codec.encode(headers)?;
        let mut chunks = block.chunks(self.max_frame.max(1)).peekable();
        let mut first = true;
        if block.is_empty() {
            return write_frame(io, HEADERS, END_HEADERS | if end_stream { END_STREAM } else { 0 }, stream, &[]);
        }
        while let Some(chunk) = chunks.next() {
            let last = chunks.peek().is_none();
            if first {
                let mut flags = if end_stream { END_STREAM } else { 0 };
                if last {
                    flags |= END_HEADERS;
                }
                write_frame(io, HEADERS, flags, stream, chunk)?;
                first = false;
            } else {
                write_frame(io, CONTINUATION, if last { END_HEADERS } else { 0 }, stream, chunk)?;
            }
        }
        Ok(())
    }

    /// Sends a body, respecting the peer's connection and stream flow-control windows. While
    /// both windows are exhausted it keeps reading frames (to receive WINDOW_UPDATE).
    pub fn send_data(&mut self, io: &mut dyn Io, stream: u32, data: &[u8], end_stream: bool) -> Result<(), String> {
        if data.is_empty() {
            if end_stream {
                write_frame(io, DATA, END_STREAM, stream, &[])?;
            }
            return Ok(());
        }
        let mut sent = 0usize;
        while sent < data.len() {
            if self.goaway.is_some() || self.reset.contains_key(&stream) {
                return Err("peer closed the stream before the body was sent".to_string());
            }
            let stream_window = *self.window_for(stream);
            let allowed = self.conn_send.min(stream_window).min(self.max_frame as i64);
            if allowed <= 0 {
                self.pump(io)?;
                continue;
            }
            let n = (allowed as usize).min(data.len() - sent);
            let last = sent + n == data.len();
            write_frame(io, DATA, if last && end_stream { END_STREAM } else { 0 }, stream, &data[sent..sent + n])?;
            self.conn_send -= n as i64;
            *self.window_for(stream) -= n as i64;
            sent += n;
        }
        Ok(())
    }
}

fn is_connection_specific(name: &str) -> bool {
    matches!(name, "connection" | "keep-alive" | "proxy-connection" | "transfer-encoding" | "upgrade" | "host" | "content-length")
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    /// HTTP/2 stream id. Even ids identify server-pushed responses.
    pub stream: u32,
    pub status: i64,
    pub headers: HeaderList,
    pub body: Vec<u8>,
}

/// Reusable HTTP/2 client session. Multiple requests may be sent before collecting responses;
/// each request receives an independent stream id and responses may arrive in any order.
pub struct ClientSession<'a> {
    io: &'a mut dyn Io,
    conn: Conn,
    next_stream: u32,
    scheme: String,
    authority: String,
}

impl<'a> ClientSession<'a> {
    pub fn new(io: &'a mut dyn Io, scheme: &str, authority: &str, enable_push: bool) -> Result<ClientSession<'a>, String> {
        io.write_all(PREFACE).map_err(|e| format!("HTTP/2 write: {}", e))?;
        let mut settings = Vec::with_capacity(6);
        settings.extend_from_slice(&[0, 2]); // SETTINGS_ENABLE_PUSH
        settings.extend_from_slice(&(if enable_push { 1u32 } else { 0u32 }).to_be_bytes());
        write_frame(io, SETTINGS, 0, 0, &settings)?;
        let mut conn = Conn::new();
        conn.peer_push_enabled = enable_push;
        Ok(ClientSession { io, conn, next_stream: 1, scheme: scheme.to_string(), authority: authority.to_string() })
    }

    pub fn send_request(&mut self, method: &str, path: &str, headers: &[(String, String)], body: &[u8]) -> Result<u32, String> {
        if self.next_stream > 0x7fff_fffd {
            return Err("HTTP/2 client stream id space exhausted".to_string());
        }
        let stream = self.next_stream;
        self.next_stream += 2;
        let mut list: HeaderList = vec![
            (":method".into(), method.to_ascii_uppercase()),
            (":scheme".into(), self.scheme.clone()),
            (":authority".into(), self.authority.clone()),
            (":path".into(), if path.is_empty() { "/".to_string() } else { path.to_string() }),
        ];
        let mut has_agent = false;
        for (k, v) in headers {
            let name = k.to_ascii_lowercase();
            if is_connection_specific(&name) || name == "te" { continue; }
            if name == "user-agent" { has_agent = true; }
            list.push((name, v.clone()));
        }
        if !has_agent { list.push(("user-agent".into(), "fusion/0.3".into())); }
        if !body.is_empty() || matches!(method.to_ascii_uppercase().as_str(), "POST" | "PUT" | "PATCH") {
            list.push(("content-length".into(), body.len().to_string()));
        }
        self.conn.send_headers(self.io, stream, &list, body.is_empty())?;
        if !body.is_empty() { self.conn.send_data(self.io, stream, body, true)?; }
        Ok(stream)
    }

    fn into_response(message: Message) -> Result<Response, String> {
        let status: i64 = header(&message.headers, ":status").and_then(|s| s.parse().ok()).ok_or("HTTP/2 response has no :status")?;
        let headers = message.headers.into_iter().filter(|(k, _)| !k.starts_with(':')).collect();
        Ok(Response { stream: message.stream, status, headers, body: message.body })
    }

    /// Waits for one specific stream. Other completed responses remain queued.
    pub fn wait_response(&mut self, stream: u32) -> Result<Response, String> {
        loop {
            if let Some(pos) = self.conn.ready.iter().position(|m| m.stream == stream) {
                let message = self.conn.ready.remove(pos).ok_or("HTTP/2 internal error")?;
                return Self::into_response(message);
            }
            if let Some(code) = self.conn.reset.get(&stream) {
                return Err(format!("HTTP/2 stream reset by peer (error code {})", code));
            }
            if let Some(code) = self.conn.goaway {
                return Err(format!("HTTP/2 connection closed by peer (GOAWAY, error code {})", code));
            }
            self.conn.pump(self.io)?;
        }
    }

    /// Returns the next completed response, regardless of stream id. This also yields pushed
    /// responses on even-numbered streams when server push was enabled in `new`.
    pub fn next_response(&mut self) -> Result<Response, String> {
        loop {
            if let Some(message) = self.conn.ready.pop_front() {
                return Self::into_response(message);
            }
            if let Some(code) = self.conn.goaway {
                return Err(format!("HTTP/2 connection closed by peer (GOAWAY, error code {})", code));
            }
            self.conn.pump(self.io)?;
        }
    }
}

/// Performs one request/response exchange on a fresh connection (stream 1).
pub fn client_request(
    io: &mut dyn Io,
    scheme: &str,
    authority: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Response, String> {
    let mut session = ClientSession::new(io, scheme, authority, false)?;
    let stream = session.send_request(method, path, headers, body)?;
    session.wait_response(stream)
}

// ---------------------------------------------------------------------------
// Server
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub scheme: String,
    pub authority: String,
    pub headers: HeaderList,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: i64,
    pub headers: HeaderList,
    pub body: Vec<u8>,
}

/// A server-pushed resource associated with a parent request stream.
#[derive(Debug, Clone, PartialEq)]
pub struct Push {
    /// Pseudo-headers describing the promised request, e.g. `:method`, `:scheme`, `:authority`, `:path`.
    pub request_headers: HeaderList,
    pub reply: Reply,
}

/// Normal response plus optional HTTP/2 server pushes.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerResponse {
    pub reply: Reply,
    pub pushes: Vec<Push>,
}

fn to_request(message: &Message) -> Result<Request, String> {
    let method = header(&message.headers, ":method").ok_or("request has no :method")?.to_string();
    let target = header(&message.headers, ":path").ok_or("request has no :path")?;
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (target.to_string(), String::new()),
    };
    Ok(Request {
        method,
        path,
        query,
        scheme: header(&message.headers, ":scheme").unwrap_or("http").to_string(),
        authority: header(&message.headers, ":authority").unwrap_or("").to_string(),
        headers: message.headers.iter().filter(|(k, _)| !k.starts_with(':')).cloned().collect(),
        body: message.body.clone(),
    })
}

fn reply_headers(reply: &Reply) -> HeaderList {
    let mut list: HeaderList = vec![(":status".into(), reply.status.to_string())];
    let mut has_type = false;
    for (k, v) in &reply.headers {
        let name = k.to_ascii_lowercase();
        if is_connection_specific(&name) {
            continue;
        }
        if name == "content-type" {
            has_type = true;
        }
        list.push((name, v.clone()));
    }
    if !has_type {
        list.push(("content-type".into(), "text/plain; charset=utf-8".into()));
    }
    list.push(("content-length".into(), reply.body.len().to_string()));
    list
}

/// Serves one HTTP/2 connection without server push.
pub fn serve(io: &mut dyn Io, on_request: &mut dyn FnMut(Request) -> Reply) -> Result<(), String> {
    serve_with_push(io, &mut |request| ServerResponse { reply: on_request(request), pushes: Vec::new() })
}

/// Serves one HTTP/2 connection and emits explicitly requested server pushes.
/// The application must only push resources that are safe and useful for the requesting client.
pub fn serve_with_push(io: &mut dyn Io, on_request: &mut dyn FnMut(Request) -> ServerResponse) -> Result<(), String> {
    let mut preface = [0u8; 24];
    io.read_exact(&mut preface).map_err(read_error)?;
    if preface != PREFACE {
        return Err("invalid HTTP/2 connection preface".to_string());
    }
    let mut conn = Conn::new();
    write_frame(io, SETTINGS, 0, 0, &[])?;
    loop {
        while let Some(message) = conn.ready.pop_front() {
            let response = match to_request(&message) {
                Ok(request) => on_request(request),
                Err(e) => ServerResponse {
                    reply: Reply { status: 400, headers: Vec::new(), body: e.into_bytes() },
                    pushes: Vec::new(),
                },
            };
            for push in response.pushes {
                let promised = conn.send_push_promise(io, message.stream, &push.request_headers)?;
                let push_no_body = push.reply.status == 204 || push.reply.status == 304 || push.reply.body.is_empty();
                conn.send_headers(io, promised, &reply_headers(&push.reply), push_no_body)?;
                if !push_no_body {
                    conn.send_data(io, promised, &push.reply.body, true)?;
                }
            }
            let reply = response.reply;
            let no_body = reply.status == 204 || reply.status == 304 || reply.body.is_empty();
            conn.send_headers(io, message.stream, &reply_headers(&reply), no_body)?;
            if !no_body {
                conn.send_data(io, message.stream, &reply.body, true)?;
            }
        }
        if conn.goaway.is_some() {
            return Ok(());
        }
        match conn.pump(io) {
            Ok(()) => {}
            Err(e) if e == CONNECTION_CLOSED => return Ok(()),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use std::io::Cursor;

    struct Pipe {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl std::io::Read for Pipe {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl std::io::Write for Pipe {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.output.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn frame_round_trip() {
        let mut pipe = Pipe { input: Cursor::new(Vec::new()), output: Vec::new() };
        write_frame(&mut pipe, PING, 0, 0, b"12345678").unwrap();
        let mut reader = Pipe { input: Cursor::new(pipe.output.clone()), output: Vec::new() };
        let frame = read_frame(&mut reader).unwrap();
        assert_eq!(frame, Frame { kind: PING, flags: 0, stream: 0, payload: b"12345678".to_vec() });
        assert_eq!(&pipe.output[..9], &[0, 0, 8, 6, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn settings_are_acknowledged_and_windows_adjusted() {
        let mut conn = Conn::new();
        let mut pipe = Pipe { input: Cursor::new(Vec::new()), output: Vec::new() };
        // INITIAL_WINDOW_SIZE = 100, MAX_FRAME_SIZE = 32768.
        let payload = [0, 4, 0, 0, 0, 100, 0, 5, 0, 0, 0x80, 0];
        conn.handle(&mut pipe, Frame { kind: SETTINGS, flags: 0, stream: 0, payload: payload.to_vec() }).unwrap();
        assert_eq!(conn.initial_send, 100);
        assert_eq!(conn.max_frame, 32768);
        assert_eq!(&pipe.output[3..5], &[SETTINGS, ACK]);
        // A WINDOW_UPDATE of 50 on a new stream adds to the new initial window.
        conn.handle(&mut pipe, Frame { kind: WINDOW_UPDATE, flags: 0, stream: 3, payload: 50u32.to_be_bytes().to_vec() }).unwrap();
        assert_eq!(*conn.window_for(3), 150);
    }

    #[test]
    fn ping_is_echoed_with_ack() {
        let mut conn = Conn::new();
        let mut pipe = Pipe { input: Cursor::new(Vec::new()), output: Vec::new() };
        conn.handle(&mut pipe, Frame { kind: PING, flags: 0, stream: 0, payload: b"abcdefgh".to_vec() }).unwrap();
        assert_eq!(pipe.output[4], ACK);
        assert_eq!(&pipe.output[9..], b"abcdefgh");
    }

    #[test]
    fn data_flow_control_blocks_until_window_update() {
        // Window of 10 bytes, body of 25: after 10 bytes the sender must read a WINDOW_UPDATE.
        let mut conn = Conn::new();
        conn.conn_send = 10;
        let mut updates = Vec::new();
        let mut wire = Pipe { input: Cursor::new(Vec::new()), output: Vec::new() };
        write_frame(&mut wire, WINDOW_UPDATE, 0, 0, &20u32.to_be_bytes()).unwrap();
        updates.extend_from_slice(&wire.output);
        let mut io = Pipe { input: Cursor::new(updates), output: Vec::new() };
        conn.send_data(&mut io, 1, &[7u8; 25], true).unwrap();
        // Two DATA frames: 10 bytes, then 15 bytes with END_STREAM.
        let first_len = ((io.output[0] as usize) << 16) | ((io.output[1] as usize) << 8) | io.output[2] as usize;
        assert_eq!(first_len, 10);
        let second = &io.output[9 + 10..];
        assert_eq!(second[4], END_STREAM);
        assert_eq!(second.len(), 9 + 15);
    }

    #[cfg(not(feature = "http2"))]
    #[test]
    fn without_the_feature_header_compression_reports_how_to_enable_it() {
        let mut codec = Codec::new();
        assert!(codec.encode(&[(":method".into(), "GET".into())]).unwrap_err().contains("--features http2"));
    }
}

#[cfg(all(test, feature = "http2"))]
mod loopback_tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn one_session_can_have_multiple_requests_in_flight() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            let mut handler = |req: Request| Reply {
                status: 200,
                headers: Vec::new(),
                body: req.path.into_bytes(),
            };
            serve(&mut tcp, &mut handler).unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut session = ClientSession::new(&mut tcp, "http", "localhost", false).unwrap();
        let first = session.send_request("GET", "/one", &[], &[]).unwrap();
        let second = session.send_request("GET", "/two", &[], &[]).unwrap();
        assert_eq!((first, second), (1, 3));
        // Waiting for stream 3 must preserve the earlier completed stream 1 response.
        let r2 = session.wait_response(second).unwrap();
        let r1 = session.wait_response(first).unwrap();
        assert_eq!(String::from_utf8(r2.body).unwrap(), "/two");
        assert_eq!(String::from_utf8(r1.body).unwrap(), "/one");
        drop(session);
        drop(tcp);
        server.join().unwrap();
    }

    #[test]
    fn server_push_uses_even_stream_and_is_delivered_to_client() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            let mut handler = |_req: Request| ServerResponse {
                reply: Reply { status: 200, headers: Vec::new(), body: b"html".to_vec() },
                pushes: vec![Push {
                    request_headers: vec![
                        (":method".into(), "GET".into()),
                        (":scheme".into(), "http".into()),
                        (":authority".into(), "localhost".into()),
                        (":path".into(), "/app.css".into()),
                    ],
                    reply: Reply { status: 200, headers: vec![("content-type".into(), "text/css".into())], body: b"body{}".to_vec() },
                }],
            };
            serve_with_push(&mut tcp, &mut handler).unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut session = ClientSession::new(&mut tcp, "http", "localhost", true).unwrap();
        session.send_request("GET", "/", &[], &[]).unwrap();
        let pushed = session.next_response().unwrap();
        let parent = session.next_response().unwrap();
        assert_eq!(pushed.stream, 2);
        assert_eq!(pushed.status, 200);
        assert_eq!(String::from_utf8(pushed.body).unwrap(), "body{}");
        assert_eq!(parent.stream, 1);
        assert_eq!(String::from_utf8(parent.body).unwrap(), "html");
        drop(session);
        drop(tcp);
        server.join().unwrap();
    }

    #[test]
    fn client_and_server_exchange_a_large_body_with_flow_control() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut tcp, _) = listener.accept().unwrap();
            let mut handler = |req: Request| Reply {
                status: 200,
                headers: vec![("x-method".to_string(), req.method.clone())],
                body: format!("{}:{}:{}", req.path, req.query, req.body.len()).into_bytes(),
            };
            serve(&mut tcp, &mut handler).unwrap();
        });
        let mut tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = vec![b'x'; 200_000];
        let response = client_request(&mut tcp, "http", "localhost", "POST", "/upload?a=1", &[], &body).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), "/upload:a=1:200000");
        assert_eq!(header(&response.headers, "x-method"), Some("POST"));
        drop(tcp);
        server.join().unwrap();
    }
}
