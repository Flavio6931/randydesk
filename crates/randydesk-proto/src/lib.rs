//! Protocolo binario, framing seguro, cifrado E2E y autenticación de RandyDesk.

pub mod auth;
pub mod e2e;
pub mod framing;
pub mod message;

pub use auth::{
    compute_proof, format_host_id, generate_host_id, generate_nonce, generate_session_pin,
    parse_numeric_id, verify_proof,
};
pub use e2e::{E2eCipher, E2eError, E2eKeypair};
pub use framing::{read_msg, write_msg, FramingError};
pub use message::{
    ClientMsg, CodecKind, EncodedTile, HostMsg, InputEvent, MouseButton, StreamSettings,
};

/// Puerto UDP/QUIC por defecto para conexiones directas del agente host.
pub const DEFAULT_PORT: u16 = 21118;

/// Puerto TCP/WebSocket por defecto para el servidor Relay.
pub const DEFAULT_RELAY_PORT: u16 = 21119;

/// Servidor Relay público gratuito por defecto del proyecto.
pub const DEFAULT_RELAY_URL: &str = "wss://randydesk-relay.onrender.com";

/// Versión actual del protocolo de comunicación.
pub const PROTOCOL_VERSION: u16 = 1;

/// Límite máximo permitido por mensaje serializado (32 MiB) para evitar ataques OOM.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// Tamaño cuadrado en píxeles de cada bloque (tile) de pantalla.
pub const TILE_SIZE: u32 = 128;

/// Normaliza una URL de servidor Relay convirtiendo `http(s)` a `ws(s)` y quitando `/` final.
pub fn normalize_relay_ws_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let converted = if let Some(rest) = trimmed.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("http://") {
        format!("ws://{rest}")
    } else if trimmed.starts_with("wss://") || trimmed.starts_with("ws://") {
        trimmed.to_string()
    } else {
        format!("wss://{trimmed}")
    };
    Some(converted)
}
