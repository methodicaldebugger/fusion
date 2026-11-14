//! TLS transport for HTTPS (client and server), built on `rustls`.
//!
//! TLS is intentionally **not** implemented by hand: a home-grown TLS stack would be insecure.
//! Enable the optional `tls` Cargo feature (`cargo build --features tls`) to pull in `rustls`,
//! `webpki-roots` (the Mozilla root certificates) and `rustls-pemfile`. Without the feature every
//! function here returns a clear error and the rest of Fusion is unaffected.

use std::io::{Read, Write};
use std::net::TcpStream;

/// A bidirectional byte stream (plain TCP or TLS).
pub trait Io: Read + Write + Send {}
impl<T: Read + Write + Send> Io for T {}

pub type BoxIo = Box<dyn Io>;

/// An established transport and the application protocol negotiated with ALPN (`h2`, `http/1.1`).
pub struct Connected {
    pub stream: BoxIo,
    pub alpn: Option<String>,
}

#[cfg(not(feature = "tls"))]
mod imp {
    use super::*;

    #[derive(Clone)]
    pub struct ServerTls;

    const MISSING: &str = "HTTPS needs TLS support: rebuild Fusion with `cargo build --features tls`";

    pub fn available() -> bool {
        false
    }

    pub fn connect(_host: &str, _tcp: TcpStream, _offer_h2: bool) -> Result<Connected, String> {
        Err(MISSING.to_string())
    }

    pub fn server_config(_cert_pem: &str, _key_pem: &str) -> Result<ServerTls, String> {
        Err(MISSING.to_string())
    }

    pub fn accept(_config: &ServerTls, _tcp: TcpStream) -> Result<Connected, String> {
        Err(MISSING.to_string())
    }
}

#[cfg(feature = "tls")]
mod imp {
    use super::*;
    use std::io::BufReader;
    use std::sync::Arc;

    pub type ServerTls = Arc<rustls::ServerConfig>;

    fn provider() -> Arc<rustls::crypto::CryptoProvider> {
        Arc::new(rustls::crypto::ring::default_provider())
    }

    pub fn available() -> bool {
        true
    }

    pub fn connect(host: &str, mut tcp: TcpStream, offer_h2: bool) -> Result<Connected, String> {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let mut config = rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS configuration: {}", e))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = if offer_h2 { vec![b"h2".to_vec(), b"http/1.1".to_vec()] } else { vec![b"http/1.1".to_vec()] };
        let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| format!("invalid server name '{}': {}", host, e))?;
        let mut conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| format!("TLS handshake setup: {}", e))?;
        while conn.is_handshaking() {
            conn.complete_io(&mut tcp).map_err(|e| format!("TLS handshake with '{}' failed: {}", host, e))?;
        }
        let alpn = conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned());
        Ok(Connected { stream: Box::new(rustls::StreamOwned::new(conn, tcp)), alpn })
    }

    pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<ServerTls, String> {
        let cert_file = std::fs::File::open(cert_pem).map_err(|e| format!("open certificate '{}': {}", cert_pem, e))?;
        let certs = rustls_pemfile::certs(&mut BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("read certificate '{}': {}", cert_pem, e))?;
        if certs.is_empty() {
            return Err(format!("no certificates found in '{}'", cert_pem));
        }
        let key_file = std::fs::File::open(key_pem).map_err(|e| format!("open private key '{}': {}", key_pem, e))?;
        let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))
            .map_err(|e| format!("read private key '{}': {}", key_pem, e))?
            .ok_or_else(|| format!("no private key found in '{}'", key_pem))?;
        let mut config = rustls::ServerConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS configuration: {}", e))?
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| format!("certificate/key mismatch: {}", e))?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }

    pub fn accept(config: &ServerTls, mut tcp: TcpStream) -> Result<Connected, String> {
        let mut conn = rustls::ServerConnection::new(config.clone()).map_err(|e| format!("TLS accept: {}", e))?;
        while conn.is_handshaking() {
            conn.complete_io(&mut tcp).map_err(|e| format!("TLS handshake failed: {}", e))?;
        }
        let alpn = conn.alpn_protocol().map(|p| String::from_utf8_lossy(p).into_owned());
        Ok(Connected { stream: Box::new(rustls::StreamOwned::new(conn, tcp)), alpn })
    }
}

pub use imp::ServerTls;

/// True when Fusion was built with TLS support.
pub fn available() -> bool {
    imp::available()
}

/// Performs a client handshake over `tcp` for server name `host`, offering `h2` when `offer_h2`.
pub fn connect_tls(host: &str, tcp: TcpStream, offer_h2: bool) -> Result<Connected, String> {
    imp::connect(host, tcp, offer_h2)
}

/// Loads a PEM certificate chain and PEM private key into a server configuration.
pub fn server_config(cert_pem: &str, key_pem: &str) -> Result<ServerTls, String> {
    imp::server_config(cert_pem, key_pem)
}

/// Performs a server handshake over an accepted connection.
pub fn accept_tls(config: &ServerTls, tcp: TcpStream) -> Result<Connected, String> {
    imp::accept(config, tcp)
}

#[cfg(all(test, not(feature = "tls")))]
mod tests {
    use super::*;

    #[test]
    fn without_the_feature_https_reports_how_to_enable_it() {
        assert!(!available());
        let err = server_config("cert.pem", "key.pem").err().unwrap();
        assert!(err.contains("--features tls"), "{}", err);
    }
}
