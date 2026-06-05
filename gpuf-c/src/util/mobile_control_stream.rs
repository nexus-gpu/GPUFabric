use std::fmt;
use std::fs::File;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use tokio_rustls::rustls::{
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
    ClientConfig, ClientConnection, DigitallySignedStruct, Error as TlsError, RootCertStore,
    SignatureScheme, StreamOwned,
};

use super::mobile_tls_policy::{validate_mobile_tls_policy, MobileTlsPolicy};

#[derive(Debug, Clone, Default)]
pub struct MobileControlTlsConfig {
    pub enabled: bool,
    pub ca_cert_path: Option<PathBuf>,
    pub server_name: Option<String>,
    pub cert_sha256_pin: Option<[u8; 32]>,
}

impl MobileControlTlsConfig {
    pub fn plaintext() -> Self {
        Self::default()
    }

    pub fn from_inputs(
        enabled: bool,
        ca_cert_path: Option<&str>,
        server_name: Option<&str>,
        cert_sha256_pin: Option<&str>,
    ) -> Result<Self> {
        if !enabled {
            return Ok(Self::plaintext());
        }

        let policy = validate_mobile_tls_policy(
            ca_cert_path,
            server_name.unwrap_or_default(),
            cert_sha256_pin,
        )
        .map_err(|e| anyhow!("invalid mobile control TLS policy: {e}"))?;

        Ok(Self {
            enabled: true,
            ca_cert_path: policy.ca_cert_path,
            server_name: Some(policy.server_name),
            cert_sha256_pin: policy
                .cert_sha256_pin
                .as_deref()
                .map(parse_sha256_pin)
                .transpose()?,
        })
    }
}

pub enum MobileControlStream {
    Plain(TcpStream),
    Tls(Box<StreamOwned<ClientConnection, TcpStream>>),
}

impl fmt::Debug for MobileControlStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain(_) => f.debug_tuple("Plain").finish_non_exhaustive(),
            Self::Tls(_) => f.debug_tuple("Tls").finish_non_exhaustive(),
        }
    }
}

impl MobileControlStream {
    pub fn set_read_timeout(&self, timeout: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.set_read_timeout(timeout),
            Self::Tls(stream) => stream.get_ref().set_read_timeout(timeout),
        }
    }

    pub fn set_write_timeout(&self, timeout: Option<std::time::Duration>) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.set_write_timeout(timeout),
            Self::Tls(stream) => stream.get_ref().set_write_timeout(timeout),
        }
    }

    pub fn shutdown(&self, how: Shutdown) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.shutdown(how),
            Self::Tls(stream) => stream.get_ref().shutdown(how),
        }
    }
}

impl Read for MobileControlStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.read(buf),
            Self::Tls(stream) => stream.read(buf),
        }
    }
}

impl Write for MobileControlStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(stream) => stream.write(buf),
            Self::Tls(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(stream) => stream.flush(),
            Self::Tls(stream) => stream.flush(),
        }
    }
}

pub fn connect_mobile_control_stream(
    server_addr: &str,
    control_port: u16,
    tls: &MobileControlTlsConfig,
) -> Result<MobileControlStream> {
    let tcp = TcpStream::connect(format!("{}:{}", server_addr, control_port))
        .map_err(|e| anyhow!("failed to connect to configured control endpoint: {e}"))?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();
    tcp.set_write_timeout(Some(std::time::Duration::from_secs(30)))
        .ok();

    if !tls.enabled {
        return Ok(MobileControlStream::Plain(tcp));
    }

    install_mobile_rustls_crypto_provider_once();
    let server_name_raw = tls
        .server_name
        .as_deref()
        .ok_or_else(|| anyhow!("mobile control TLS server name is required"))?
        .to_string();
    let server_name = ServerName::try_from(server_name_raw.clone())
        .map_err(|_| anyhow!("invalid mobile control TLS server name: {server_name_raw}"))?;

    let config = build_client_config(tls)?;
    let conn = ClientConnection::new(Arc::new(config), server_name)?;
    let mut stream = StreamOwned::new(conn, tcp);
    while stream.conn.is_handshaking() {
        stream.conn.complete_io(&mut stream.sock)?;
    }

    if let Some(expected_pin) = tls.cert_sha256_pin {
        verify_peer_leaf_pin(&stream, expected_pin)?;
    }

    Ok(MobileControlStream::Tls(Box::new(stream)))
}

fn install_mobile_rustls_crypto_provider_once() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    });
}

fn build_client_config(tls: &MobileControlTlsConfig) -> Result<ClientConfig> {
    if let Some(ca_path) = &tls.ca_cert_path {
        let mut roots = RootCertStore::empty();
        for cert in load_root_certs(ca_path)? {
            roots.add(cert)?;
        }
        return Ok(ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth());
    }

    let expected_pin = tls
        .cert_sha256_pin
        .ok_or_else(|| anyhow!("mobile control TLS requires CA trust or SHA256 pin"))?;
    let provider = tokio_rustls::rustls::crypto::ring::default_provider();
    let verifier = Arc::new(PinnedServerVerifier {
        expected_pin,
        supported: provider.signature_verification_algorithms,
    });
    Ok(ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

fn load_root_certs(path: &PathBuf) -> Result<Vec<CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if certs.is_empty() {
        anyhow::bail!("{} contains no PEM certificates", path.display());
    }
    Ok(certs)
}

fn verify_peer_leaf_pin(
    stream: &StreamOwned<ClientConnection, TcpStream>,
    expected_pin: [u8; 32],
) -> Result<()> {
    let leaf = stream
        .conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .ok_or_else(|| anyhow!("mobile control TLS peer did not present a certificate"))?;
    if sha256_bytes(leaf.as_ref()) == expected_pin {
        Ok(())
    } else {
        Err(anyhow!(
            "mobile control TLS certificate SHA256 pin mismatch"
        ))
    }
}

fn parse_sha256_pin(pin: &str) -> Result<[u8; 32]> {
    let normalized = pin
        .trim()
        .strip_prefix("sha256:")
        .or_else(|| pin.trim().strip_prefix("SHA256:"))
        .unwrap_or(pin.trim())
        .replace(':', "")
        .to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("invalid SHA256 pin");
    }
    let mut out = [0u8; 32];
    for (idx, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&normalized[idx * 2..idx * 2 + 2], 16)?;
    }
    Ok(out)
}

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(bytes);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

#[derive(Debug)]
struct PinnedServerVerifier {
    expected_pin: [u8; 32],
    supported: WebPkiSupportedAlgorithms,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, TlsError> {
        if sha256_bytes(end_entity.as_ref()) == self.expected_pin {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(TlsError::InvalidCertificate(
                tokio_rustls::rustls::CertificateError::ApplicationVerificationFailure,
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.supported)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.supported)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported.supported_schemes()
    }
}

#[allow(dead_code)]
fn _assert_policy_is_send_sync(_: &MobileTlsPolicy) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
            .display()
            .to_string()
    }

    #[test]
    fn plaintext_config_disables_tls_even_with_empty_inputs() {
        let config = MobileControlTlsConfig::from_inputs(false, None, None, None).unwrap();
        assert!(!config.enabled);
        assert!(config.ca_cert_path.is_none());
        assert!(config.server_name.is_none());
        assert!(config.cert_sha256_pin.is_none());
    }

    #[test]
    fn tls_config_accepts_ca_and_pin() {
        let config = MobileControlTlsConfig::from_inputs(
            true,
            Some(&fixture("control-rotated-cert.pem")),
            Some("gpuf.example.internal"),
            Some("sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        )
        .unwrap();
        assert!(config.enabled);
        assert!(config.ca_cert_path.is_some());
        assert_eq!(config.server_name.as_deref(), Some("gpuf.example.internal"));
        assert_eq!(config.cert_sha256_pin, Some([0xaa; 32]));
    }

    #[test]
    fn tls_config_rejects_missing_trust_material() {
        let err =
            MobileControlTlsConfig::from_inputs(true, None, Some("gpuf.example.internal"), None)
                .unwrap_err();
        assert!(err.to_string().contains("CA bundle or SHA256"));
    }
}
