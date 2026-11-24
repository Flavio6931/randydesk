//! Protocolo binario, framing seguro y autenticación challenge-response de RandyDesk.

pub mod auth;
pub mod framing;
pub mod message;

pub use auth::{compute_proof, generate_nonce, generate_session_pin, verify_proof};
pub use framing::{read_msg, write_msg, FramingError};
pub use message::{
    ClientMsg, CodecKind, EncodedTile, HostMsg, InputEvent, MouseButton, StreamSettings,
};

/// Puerto UDP/QUIC por defecto para conexiones entrantes del agente host.
pub const DEFAULT_PORT: u16 = 21118;

/// Versión actual del protocolo de comunicación.
pub const PROTOCOL_VERSION: u16 = 1;

/// Límite máximo permitido por mensaje serializado (32 MiB) para evitar ataques OOM.
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// Tamaño cuadrado en píxeles de cada bloque (tile) de pantalla.
pub const TILE_SIZE: u32 = 128;
