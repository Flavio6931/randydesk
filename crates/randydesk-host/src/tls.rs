//! Gestión de identidad TLS 1.3 del host y configuración QUIC optimizada para sesiones largas.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::{ServerConfig, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Identificador ALPN del protocolo RandyDesk sobre QUIC.
pub const ALPN_PROTOCOL: &[u8] = b"randydesk-v1";

/// Certificado y huella SHA-256 cargados o generados para el host.
pub struct HostIdentity {
    pub server_config: ServerConfig,
    pub fingerprint_sha256: String,
}

/// Carga (o genera en el primer arranque) el certificado TLS del host y construye `ServerConfig`.
pub fn load_or_create_server_identity() -> Result<HostIdentity> {
    let dir = identity_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
    let cert_path = dir.join("host_cert.der");
    let key_path = dir.join("host_key.der");

    let (cert_bytes, key_bytes) = if cert_path.exists() && key_path.exists() {
        (
            fs::read(&cert_path).context("error leyendo host_cert.der")?,
            fs::read(&key_path).context("error leyendo host_key.der")?,
        )
    } else {
        let certified = rcgen::generate_simple_self_signed(vec![
            "randydesk.local".to_string(),
            "localhost".to_string(),
        ])
        .context("error generando certificado TLS autofirmado")?;
        let cert_der = certified.cert.der().to_vec();
        let key_der = certified.key_pair.serialize_der();
        fs::write(&cert_path, &cert_der).context("error guardando host_cert.der")?;
        fs::write(&key_path, &key_der).context("error guardando host_key.der")?;
        (cert_der, key_der)
    };

    build_server_identity(cert_bytes, key_bytes)
}

/// Construye una identidad efímera en memoria (útil para pruebas automatizadas).
pub fn create_ephemeral_server_identity() -> Result<HostIdentity> {
    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    build_server_identity(
        certified.cert.der().to_vec(),
        certified.key_pair.serialize_der(),
    )
}

fn build_server_identity(cert_bytes: Vec<u8>, key_bytes: Vec<u8>) -> Result<HostIdentity> {
    let fingerprint_sha256 = format_sha256_fingerprint(&cert_bytes);
    let cert_der = CertificateDer::from(cert_bytes);
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_bytes));

    let mut tls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("certificado o llave privada TLS inválidos")?;
    tls_config.alpn_protocols = vec![ALPN_PROTOCOL.to_vec()];

    let quic_crypto = QuicServerConfig::try_from(tls_config)
        .context("no se pudo inicializar configuración QUIC TLS")?;
    let mut server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));

    let mut transport = TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(5)));
    transport.max_idle_timeout(Some(VarInt::from_u32(60_000).into()));
    transport.receive_window(VarInt::from_u32(8 * 1024 * 1024));
    transport.stream_receive_window(VarInt::from_u32(4 * 1024 * 1024));
    server_config.transport_config(Arc::new(transport));

    Ok(HostIdentity {
        server_config,
        fingerprint_sha256,
    })
}

/// Formatea la huella SHA-256 de un certificado DER como pares hexadecimales separados por `:`.
pub fn format_sha256_fingerprint(cert_der: &[u8]) -> String {
    let digest = Sha256::digest(cert_der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

fn identity_dir() -> Result<PathBuf> {
    let proj = ProjectDirs::from("com", "RandyDesk", "RandyDesk")
        .context("no se pudo resolver el directorio de datos de usuario")?;
    Ok(proj.config_dir().to_path_buf())
}
