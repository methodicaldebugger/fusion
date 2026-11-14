//! Optional HTTP build-artifact cache. Executables are stored at /<key> and
//! their integrity/provenance sidecars at /<key>.provenance.toml.
use std::{fs, path::Path, process::Command};

pub fn restore(base: &str, key: &str, dest: &Path) -> Result<bool, String> {
    let url = format!("{}/{}", base.trim_end_matches('/'), key);
    let parent = dest.parent().ok_or("artifact destination has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let status = Command::new("curl").args(["--fail", "--silent", "--show-error", "--location", "--output"])
        .arg(dest).arg(&url).status().map_err(|e| format!("start curl for remote cache: {e}"))?;
    if status.success() {
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(dest).map_err(|e| e.to_string())?.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(dest, permissions).map_err(|e| e.to_string())?;
        }
        Ok(true)
    } else { let _ = fs::remove_file(dest); Ok(false) }
}

/// Download the provenance sidecar associated with an executable cache key.
/// A missing sidecar is a cache miss: callers must not trust the executable.
pub fn restore_metadata(base: &str, key: &str, dest: &Path) -> Result<bool, String> {
    let url = format!("{}/{}.provenance.toml", base.trim_end_matches('/'), key);
    let parent = dest.parent().ok_or("metadata destination has no parent")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let status = Command::new("curl").args(["--fail", "--silent", "--show-error", "--location", "--output"])
        .arg(dest).arg(&url).status().map_err(|e| format!("start curl for remote cache provenance: {e}"))?;
    if status.success() { Ok(true) } else { let _ = fs::remove_file(dest); Ok(false) }
}

/// Upload provenance before uploading the executable. The executable object is
/// the commit marker, so readers never accept an artifact without its sidecar.
pub fn publish_metadata(base: &str, key: &str, contents: &str) -> Result<(), String> {
    let url = format!("{}/{}.provenance.toml", base.trim_end_matches('/'), key);
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos()).unwrap_or_default();
    let temporary = std::env::temp_dir().join(format!("foundry-cache-provenance-{}-{nonce}.toml", std::process::id()));
    fs::write(&temporary, contents).map_err(|e| format!("stage remote cache provenance: {e}"))?;
    let result = Command::new("curl").args(["--fail", "--silent", "--show-error", "--request", "PUT", "--upload-file"])
        .arg(&temporary).arg(&url).status()
        .map_err(|e| format!("start curl for remote cache provenance: {e}"));
    let _ = fs::remove_file(&temporary);
    let status = result?;
    if status.success() { Ok(()) } else { Err(format!("remote cache provenance upload failed for {url}")) }
}

pub fn publish(base: &str, key: &str, src: &Path) -> Result<(), String> {
    let url = format!("{}/{}", base.trim_end_matches('/'), key);
    let status = Command::new("curl").args(["--fail", "--silent", "--show-error", "--request", "PUT", "--upload-file"])
        .arg(src).arg(&url).status().map_err(|e| format!("start curl for remote cache: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("remote cache upload failed for {url}")) }
}

pub fn copy_local(src: &Path, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() { fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    fs::copy(src, dest).map(|_| ()).map_err(|e| e.to_string())
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        path::PathBuf,
        sync::{Arc, Mutex},
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    type Objects = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;
    type Requests = Arc<Mutex<Vec<String>>>;

    fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        while !header.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).expect("read HTTP request headers");
            header.push(byte[0]);
            assert!(header.len() < 64 * 1024, "HTTP request headers too large");
        }
        let text = String::from_utf8(header).expect("HTTP headers are UTF-8");
        let mut lines = text.split("\r\n");
        let request_line = lines.next().unwrap_or_default().to_string();
        let content_length = lines.filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok()).unwrap_or(0);
        let mut body = vec![0u8; content_length];
        if content_length > 0 { stream.read_exact(&mut body).expect("read HTTP request body"); }
        (request_line, body)
    }

    fn respond(stream: &mut TcpStream, status: &str, body: &[u8]) {
        write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
    }

    fn start_cache_server(request_count: usize) -> (String, Objects, Requests, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind local cache server");
        let address = listener.local_addr().unwrap();
        let objects: Objects = Arc::new(Mutex::new(BTreeMap::new()));
        let server_objects = objects.clone();
        let request_log: Requests = Arc::new(Mutex::new(Vec::new()));
        let server_log = request_log.clone();
        let handle = thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().expect("accept cache request");
                let (request, body) = read_request(&mut stream);
                let mut parts = request.split_whitespace();
                let method = parts.next().unwrap_or_default();
                let path = parts.next().unwrap_or_default().to_string();
                server_log.lock().unwrap().push(format!("{method} {path}"));
                match method {
                    "PUT" => {
                        server_objects.lock().unwrap().insert(path, body);
                        respond(&mut stream, "200 OK", b"stored");
                    }
                    "GET" => {
                        let value = server_objects.lock().unwrap().get(&path).cloned();
                        if let Some(value) = value { respond(&mut stream, "200 OK", &value); }
                        else { respond(&mut stream, "404 Not Found", b"missing"); }
                    }
                    _ => respond(&mut stream, "405 Method Not Allowed", b"unsupported"),
                }
            }
        });
        (format!("http://{address}"), objects, request_log, handle)
    }

    fn temporary_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("foundry-cache-http-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn remote_cache_round_trips_executable_and_provenance_sidecar() {
        let (base, objects, requests, server) = start_cache_server(4);
        let dir = temporary_directory("roundtrip");
        fs::create_dir_all(&dir).unwrap();
        let executable = dir.join("app");
        let restored_executable = dir.join("restored-app");
        let restored_metadata = dir.join("restored-provenance.toml");
        fs::write(&executable, b"native executable bytes").unwrap();
        let provenance = "format_version = 5\nbuild_fingerprint = \"test-key\"\nexecutable_sha256 = \"abc\"\n";

        publish_metadata(&base, "test-key", provenance).unwrap();
        publish(&base, "test-key", &executable).unwrap();
        assert!(restore_metadata(&base, "test-key", &restored_metadata).unwrap());
        assert_eq!(fs::read_to_string(&restored_metadata).unwrap(), provenance);
        assert!(restore(&base, "test-key", &restored_executable).unwrap());
        assert_eq!(fs::read(&restored_executable).unwrap().as_slice(), b"native executable bytes");
        server.join().unwrap();
        assert_eq!(requests.lock().unwrap().clone(), vec![
            "PUT /test-key.provenance.toml".to_string(), "PUT /test-key".to_string(),
            "GET /test-key.provenance.toml".to_string(), "GET /test-key".to_string(),
        ]);

        let stored = objects.lock().unwrap();
        assert_eq!(stored.get("/test-key.provenance.toml").unwrap().as_slice(), provenance.as_bytes());
        assert_eq!(stored.get("/test-key").unwrap().as_slice(), b"native executable bytes");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn missing_remote_provenance_sidecar_is_a_cache_miss() {
        let (base, _objects, _requests, server) = start_cache_server(1);
        let dir = temporary_directory("missing-sidecar");
        fs::create_dir_all(&dir).unwrap();
        let metadata = dir.join("missing.toml");
        assert!(!restore_metadata(&base, "absent-key", &metadata).unwrap());
        assert!(!metadata.exists(), "failed sidecar download must remove partial output");
        server.join().unwrap();
        let _ = fs::remove_dir_all(dir);
    }
}
