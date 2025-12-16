pub mod db;
pub mod cmd;
pub mod pack;
pub mod protoc;
pub mod msg;
use tracing::{debug,info, Level};
use anyhow::Result;
use tokio_rustls::{
    rustls::{
        pki_types::{CertificateDer, PrivateKeyDer},
    },
};
use std::fs::File;
use std::io::BufReader; 

pub fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);
    let certs = rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>()?; // Vec<Vec<u8>>
    if certs.is_empty() {
        anyhow::bail!("no certificates found in {}", path);
    }
    // convert to CertificateDer<'static>
    let certs = certs.into_iter().map(|v| v.into()).collect();
    Ok(certs)
}

pub fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let f = File::open(path)?;
    let mut reader = BufReader::new(f);

    // Prefer PKCS#8 keys, fallback to RSA keys
    let pkcs8 = rustls_pemfile::pkcs8_private_keys(&mut reader).collect::<Result<Vec<_>, _>>()?;
    if !pkcs8.is_empty() {
        return Ok(pkcs8[0].clone_key().into());
    }

    // If not found, reopen and try rsa_private_keys
    let f2 = File::open(path)?;
    info!("Private key path exists: {}", path);
    let mut reader2 = BufReader::new(f2);
    let rsa = rustls_pemfile::rsa_private_keys(&mut reader2).collect::<Result<Vec<_>, _>>()?;
    if !rsa.is_empty() {
        return Ok(rsa[0].clone_key().into());
    }

    anyhow::bail!("no private keys found in {}", path);
}


pub fn init_logging() {
    // Use DEBUG level for debug builds, INFO for release builds

    #[cfg(not(debug_assertions))]
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_ansi(!cfg!(windows))
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .compact()
        .init();

    #[cfg(debug_assertions)]
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .with_ansi(!cfg!(windows))
        .with_target(false)
        .with_thread_ids(true)
        .with_thread_names(true)
        .with_file(true)
        .with_line_number(true)
        .compact()
        .init();
    
    debug!("Logging initialized");
}
