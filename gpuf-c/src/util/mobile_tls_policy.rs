use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use tokio_rustls::rustls::pki_types::ServerName;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MobileTlsPolicy {
    pub ca_cert_path: Option<PathBuf>,
    pub server_name: String,
    pub cert_sha256_pin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobileTlsPolicyError {
    MissingServerName,
    InvalidServerName(String),
    MissingTrustMaterial,
    InvalidCaBundle(String),
    InvalidSha256Pin(String),
}

impl fmt::Display for MobileTlsPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingServerName => write!(f, "server name is required"),
            Self::InvalidServerName(name) => write!(f, "invalid TLS server name: {name}"),
            Self::MissingTrustMaterial => {
                write!(f, "CA bundle or SHA256 certificate pin is required")
            }
            Self::InvalidCaBundle(msg) => write!(f, "invalid CA bundle: {msg}"),
            Self::InvalidSha256Pin(msg) => write!(f, "invalid SHA256 certificate pin: {msg}"),
        }
    }
}

impl std::error::Error for MobileTlsPolicyError {}

pub fn validate_mobile_tls_policy(
    ca_cert_path: Option<&str>,
    server_name: &str,
    cert_sha256_pin: Option<&str>,
) -> Result<MobileTlsPolicy, MobileTlsPolicyError> {
    let server_name = server_name.trim();
    if server_name.is_empty() {
        return Err(MobileTlsPolicyError::MissingServerName);
    }
    ServerName::try_from(server_name.to_string())
        .map_err(|_| MobileTlsPolicyError::InvalidServerName(server_name.to_string()))?;

    let ca_cert_path = ca_cert_path.and_then(non_empty_trimmed).map(PathBuf::from);
    let cert_sha256_pin = cert_sha256_pin
        .and_then(non_empty_trimmed)
        .map(normalize_sha256_pin)
        .transpose()?;

    if ca_cert_path.is_none() && cert_sha256_pin.is_none() {
        return Err(MobileTlsPolicyError::MissingTrustMaterial);
    }

    if let Some(path) = &ca_cert_path {
        validate_ca_bundle(path)?;
    }

    Ok(MobileTlsPolicy {
        ca_cert_path,
        server_name: server_name.to_string(),
        cert_sha256_pin,
    })
}

fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn validate_ca_bundle(path: &PathBuf) -> Result<(), MobileTlsPolicyError> {
    let file = File::open(path)
        .map_err(|e| MobileTlsPolicyError::InvalidCaBundle(format!("{}: {e}", path.display())))?;
    let mut reader = BufReader::new(file);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| MobileTlsPolicyError::InvalidCaBundle(e.to_string()))?;
    if certs.is_empty() {
        return Err(MobileTlsPolicyError::InvalidCaBundle(format!(
            "{} contains no PEM certificates",
            path.display()
        )));
    }
    Ok(())
}

fn normalize_sha256_pin(raw: &str) -> Result<String, MobileTlsPolicyError> {
    let without_prefix = raw
        .trim()
        .strip_prefix("sha256:")
        .or_else(|| raw.trim().strip_prefix("SHA256:"))
        .unwrap_or(raw.trim());
    let normalized = without_prefix.replace(':', "").to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(MobileTlsPolicyError::InvalidSha256Pin(
            "expected 64 hex characters, optionally prefixed by sha256:".to_string(),
        ));
    }
    Ok(normalized)
}

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
    fn accepts_ca_bundle_and_sha256_pin() {
        let policy = validate_mobile_tls_policy(
            Some(&fixture("control-rotated-cert.pem")),
            "gpuf.example.internal",
            Some("sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        )
        .expect("valid policy");
        assert_eq!(policy.server_name, "gpuf.example.internal");
        assert_eq!(
            policy.cert_sha256_pin.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert!(policy.ca_cert_path.is_some());
    }

    #[test]
    fn accepts_colon_separated_pin_without_ca() {
        let policy = validate_mobile_tls_policy(
            None,
            "gpuf.example.internal",
            Some("AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA:AA"),
        )
        .expect("pin-only policy");
        assert!(policy.ca_cert_path.is_none());
        assert_eq!(
            policy.cert_sha256_pin.as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }

    #[test]
    fn rejects_missing_trust_material() {
        let err = validate_mobile_tls_policy(None, "gpuf.example.internal", None).unwrap_err();
        assert!(matches!(err, MobileTlsPolicyError::MissingTrustMaterial));
    }

    #[test]
    fn rejects_bad_pin() {
        let err =
            validate_mobile_tls_policy(None, "gpuf.example.internal", Some("abcd")).unwrap_err();
        assert!(matches!(err, MobileTlsPolicyError::InvalidSha256Pin(_)));
    }

    #[test]
    fn rejects_invalid_server_name() {
        let err =
            validate_mobile_tls_policy(None, "not a host", Some(&"a".repeat(64))).unwrap_err();
        assert!(matches!(err, MobileTlsPolicyError::InvalidServerName(_)));
    }

    #[test]
    fn ffi_returns_stable_error_codes() {
        let ca = std::ffi::CString::new(fixture("control-rotated-cert.pem")).unwrap();
        let server = std::ffi::CString::new("gpuf.example.internal").unwrap();
        let pin = std::ffi::CString::new("a".repeat(64)).unwrap();
        let empty = std::ffi::CString::new("").unwrap();
        let bad_pin = std::ffi::CString::new("abcd").unwrap();

        assert_eq!(
            crate::gpuf_validate_mobile_tls_policy(ca.as_ptr(), server.as_ptr(), pin.as_ptr()),
            0
        );
        assert_eq!(
            crate::gpuf_validate_mobile_tls_policy(
                std::ptr::null(),
                server.as_ptr(),
                empty.as_ptr()
            ),
            -2
        );
        assert_eq!(
            crate::gpuf_validate_mobile_tls_policy(
                empty.as_ptr(),
                server.as_ptr(),
                bad_pin.as_ptr()
            ),
            -4
        );
        assert_eq!(
            crate::gpuf_validate_mobile_tls_policy(empty.as_ptr(), std::ptr::null(), pin.as_ptr()),
            -1
        );
    }

    #[test]
    fn rejects_missing_ca_file() {
        let err =
            validate_mobile_tls_policy(Some("/no/such/gpuf-ca.pem"), "gpuf.example.internal", None)
                .unwrap_err();
        assert!(matches!(err, MobileTlsPolicyError::InvalidCaBundle(_)));
    }
}
