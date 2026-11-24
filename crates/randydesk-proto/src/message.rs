//! Definición de mensajes serializables entre cliente visor y host remoto.

use serde::{Deserialize, Serialize};

/// Códec utilizado para comprimir cada bloque de pantalla.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CodecKind {
    /// Compresión JPEG con pérdida controlada (ideal para movimiento fluido).
    #[default]
    Jpeg,
    /// Compresión LZ4 sobre RGBA sin pérdida (ideal para texto y código nítido).
    Lz4Rgba,
}

/// Preferencias dinámicas de transmisión de video solicitadas por el cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamSettings {
    /// Códec de compresión activo.
    pub codec: CodecKind,
    /// Calidad JPEG en rango 35..=95.
    pub jpeg_quality: u8,
    /// Tasa objetivo de cuadros por segundo (5..=60).
    pub target_fps: u8,
}

impl Default for StreamSettings {
    fn default() -> Self {
        Self {
            codec: CodecKind::Jpeg,
            jpeg_quality: 75,
            target_fps: 30,
        }
    }
}

/// Bloque rectangular codificado de una región modificada de la pantalla.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodedTile {
    /// Coordenada X de origen en píxeles.
    pub x: u32,
    /// Coordenada Y de origen en píxeles.
    pub y: u32,
    /// Ancho real del bloque en píxeles.
    pub width: u32,
    /// Alto real del bloque en píxeles.
    pub height: u32,
    /// Códec con el que fue comprimido este bloque.
    pub codec: CodecKind,
    /// Payload comprimido del bloque.
    pub data: Vec<u8>,
}

/// Botón del ratón soportado para inyección remota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    /// Botón principal (izquierdo).
    Left,
    /// Botón secundario (derecho).
    Right,
    /// Botón central / rueda.
    Middle,
}

/// Eventos de entrada remota enviados del cliente hacia el host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    /// Posición normalizada del puntero en rango [0.0, 1.0] respecto al monitor remoto.
    MouseMove { x_norm: f32, y_norm: f32 },
    /// Pulsación o liberación de un botón del ratón.
    MouseButton { button: MouseButton, down: bool },
    /// Desplazamiento de rueda vertical/horizontal.
    MouseWheel { delta_x: i16, delta_y: i16 },
    /// Tecla física usando Virtual-Key (`vk`) y scan code de Windows.
    Key {
        vk: u16,
        scan: u16,
        extended: bool,
        down: bool,
    },
    /// Carácter Unicode directo (tildes, ñ, símbolos) mediante `KEYEVENTF_UNICODE`.
    UnicodeChar { ch: char, down: bool },
}

/// Mensajes enviados por el cliente visor hacia el host remoto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Saludo inicial indicando versión de protocolo.
    Hello { version: u16 },
    /// Respuesta criptográfica al reto enviado por el host.
    AuthResponse { proof: [u8; 32] },
    /// Evento de teclado o ratón a inyectar.
    Input(InputEvent),
    /// Actualiza parámetros de compresión/FPS en caliente.
    UpdateSettings(StreamSettings),
    /// Solicita un cuadro completo actualizado de todos los bloques.
    RequestKeyframe,
    /// Envía texto UTF-8 al portapapeles remoto.
    ClipboardSet(String),
    /// Solicita leer el texto actual del portapapeles remoto.
    ClipboardGet,
    /// Latido de medición de latencia (timestamp cliente en ms).
    Ping(u64),
    /// Cierre ordenado de sesión.
    Disconnect,
}

/// Mensajes enviados por el host remoto hacia el cliente visor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HostMsg {
    /// Reto de autenticación y metadatos iniciales de pantalla del host.
    AuthChallenge {
        version: u16,
        nonce: [u8; 32],
        host_name: String,
        width: u32,
        height: u32,
    },
    /// Resultado de la verificación de credenciales.
    AuthResult {
        ok: bool,
        reason: Option<String>,
    },
    /// Cuadro incremental o completo compuesto por bloques modificados.
    VideoFrame {
        frame_id: u64,
        width: u32,
        height: u32,
        full_refresh: bool,
        tiles: Vec<EncodedTile>,
    },
    /// Contenido textual leído del portapapeles del host.
    ClipboardData(String),
    /// Respuesta al latido `Ping` para calcular RTT.
    Pong(u64),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_client_and_host_messages() {
        let client_msg = ClientMsg::Input(InputEvent::MouseMove {
            x_norm: 0.42,
            y_norm: 0.77,
        });
        let encoded = bincode::serialize(&client_msg).expect("serialización cliente");
        let decoded: ClientMsg = bincode::deserialize(&encoded).expect("deserialización cliente");
        assert_eq!(client_msg, decoded);

        let host_msg = HostMsg::VideoFrame {
            frame_id: 99,
            width: 1920,
            height: 1080,
            full_refresh: true,
            tiles: vec![EncodedTile {
                x: 0,
                y: 0,
                width: 128,
                height: 128,
                codec: CodecKind::Lz4Rgba,
                data: vec![1, 2, 3, 4],
            }],
        };
        let encoded_h = bincode::serialize(&host_msg).expect("serialización host");
        let decoded_h: HostMsg = bincode::deserialize(&encoded_h).expect("deserialización host");
        assert_eq!(host_msg, decoded_h);
    }
}
