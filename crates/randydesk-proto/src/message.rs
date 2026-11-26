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
    /// Compresión de video H.264 inter-frame (OpenH264 Annex-B, ideal para alta fluidez y bajo bitrate).
    H264,
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
            jpeg_quality: 72,
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
    /// Botón lateral Atrás (`XBUTTON1`).
    Extra1,
    /// Botón lateral Adelante (`XBUTTON2`).
    Extra2,
}

/// Información de un monitor disponible en el host remoto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    /// Índice del monitor compatible con el capturador DXGI/GDI.
    pub index: u8,
    /// Nombre legible (`Monitor 1`, `Monitor 2`, ...).
    pub name: String,
    /// Ancho nativo en píxeles.
    pub width: u32,
    /// Alto nativo en píxeles.
    pub height: u32,
    /// Indica si es el monitor principal en coordenadas `(0, 0)`.
    pub primary: bool,
}

/// Forma visual estándar del cursor del sistema remoto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CursorKind {
    #[default]
    Default,
    Text,
    PointingHand,
    Crosshair,
    Move,
    ResizeHorizontal,
    ResizeVertical,
    ResizeNeSw,
    ResizeNwSe,
    NotAllowed,
    Wait,
    Progress,
    Hidden,
}

/// Códec aplicado a un paquete de audio remoto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioCodec {
    /// IMA-ADPCM estéreo 4:1 sobre PCM `16-bit`.
    ImaAdpcm,
}

/// Bloque de audio estéreo capturado y comprimido en el host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFrame {
    /// Frecuencia de muestreo en Hz (típicamente `48_000`).
    pub sample_rate: u32,
    /// Número de canales (`2` estéreo).
    pub channels: u8,
    /// Códec utilizado en `data`.
    pub codec: AudioCodec,
    /// Payload comprimido del bloque de audio.
    pub data: Vec<u8>,
}

/// Entrada de archivo o carpeta reportada por el navegador de archivos remoto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Nombre visible de la entrada (o letra de unidad si está en raíz de unidades).
    pub name: String,
    /// `true` si corresponde a una carpeta o unidad.
    pub is_dir: bool,
    /// Tamaño en bytes (0 para carpetas).
    pub size: u64,
    /// Última modificación en segundos Unix.
    pub modified_unix: u64,
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
    /// Porcentaje de escala de transmisión en el host (`50..=100`).
    SetStreamScale(u8),
    /// Bloquea la sesión de Windows en el equipo remoto (`LockWorkStation`).
    LockWorkStation,
    /// Envía la secuencia de atención segura (`Ctrl+Alt+Del`) mediante `SendSAS` del servicio `SYSTEM`.
    SendSas,
    /// Selecciona en vivo el índice de monitor a capturar en el host.
    SelectMonitor(u8),
    /// Activa o pausa la captura y envío de audio WASAPI desde el host.
    SetAudioEnabled(bool),
    /// Solicita listar el contenido de un directorio remoto (vacío = escritorio por defecto).
    FileListReq { path: String },
    /// Inicia una transferencia de subida hacia una carpeta remota.
    FileUploadStart {
        transfer_id: u64,
        remote_dir: String,
        file_name: String,
        total_size: u64,
    },
    /// Bloque secuencial de archivo subido hacia el host.
    FileUploadChunk {
        transfer_id: u64,
        offset: u64,
        data: Vec<u8>,
    },
    /// Confirma fin de subida para renombrar `.rdpart` a destino definitivo.
    FileUploadFinish { transfer_id: u64 },
    /// Solicita descargar un archivo remoto en bloques hacia el cliente.
    FileDownloadReq { transfer_id: u64, remote_path: String },
    /// Cancela una transferencia de archivo activa (subida o descarga).
    FileTransferCancel { transfer_id: u64 },
    /// Crea una nueva carpeta dentro de `parent_dir`.
    FileMkdir { parent_dir: String, name: String },
    /// Coordina UDP hole punching P2P enviando al host la IP:puerto STUN del cliente.
    P2pPunch { client_public_addr: String },
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
    /// Lista de endpoints directos LAN/Tailscale (`ip:puerto`) del host para promover sesión Relay a QUIC directo.
    DirectEndpoints(Vec<String>),
    /// Lista de monitores disponibles en el host e índice activo.
    MonitorList {
        monitors: Vec<MonitorInfo>,
        active_index: u8,
    },
    /// Cambio en la forma actual del cursor remoto.
    CursorShape(CursorKind),
    /// Bloque comprimido de audio del sistema remoto.
    AudioData(AudioFrame),
    /// Respuesta con entradas de carpeta o error al listar archivos remotos.
    FileListResp {
        path: String,
        parent: Option<String>,
        entries: Vec<FileEntry>,
        error: Option<String>,
    },
    /// Metadatos iniciales de una descarga iniciada desde el host.
    FileDownloadMeta {
        transfer_id: u64,
        file_name: String,
        total_size: u64,
    },
    /// Bloque secuencial de archivo descargado desde el host.
    FileDownloadChunk {
        transfer_id: u64,
        offset: u64,
        data: Vec<u8>,
    },
    /// Confirmación de transferencia completada correctamente.
    FileTransferDone { transfer_id: u64, saved_path: String },
    /// Error o cancelación de una transferencia de archivos.
    FileTransferError { transfer_id: u64, message: String },
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

        for extra in [
            ClientMsg::SetStreamScale(75),
            ClientMsg::LockWorkStation,
            ClientMsg::SendSas,
            ClientMsg::SelectMonitor(1),
            ClientMsg::Input(InputEvent::MouseButton {
                button: MouseButton::Extra1,
                down: true,
            }),
            ClientMsg::Input(InputEvent::MouseButton {
                button: MouseButton::Extra2,
                down: false,
            }),
        ] {
            let bytes = bincode::serialize(&extra).expect("serialización comando");
            let back: ClientMsg = bincode::deserialize(&bytes).expect("deserialización comando");
            assert_eq!(extra, back);
        }

        let h264_msg = HostMsg::VideoFrame {
            frame_id: 100,
            width: 1280,
            height: 720,
            full_refresh: true,
            tiles: vec![EncodedTile {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
                codec: CodecKind::H264,
                data: vec![0, 0, 0, 1, 0x67],
            }],
        };
        let encoded_h264 = bincode::serialize(&h264_msg).expect("serialización h264");
        let decoded_h264: HostMsg = bincode::deserialize(&encoded_h264).expect("deserialización h264");
        assert_eq!(h264_msg, decoded_h264);

        let endpoints = HostMsg::DirectEndpoints(vec!["192.168.1.50:21118".to_string()]);
        let enc_ep = bincode::serialize(&endpoints).expect("serialización endpoints");
        let dec_ep: HostMsg = bincode::deserialize(&enc_ep).expect("deserialización endpoints");
        assert_eq!(endpoints, dec_ep);

        let mon_list = HostMsg::MonitorList {
            monitors: vec![MonitorInfo {
                index: 0,
                name: "Monitor 1".to_string(),
                width: 1920,
                height: 1080,
                primary: true,
            }],
            active_index: 0,
        };
        let enc_m = bincode::serialize(&mon_list).expect("serialización monitor_list");
        let dec_m: HostMsg = bincode::deserialize(&enc_m).expect("deserialización monitor_list");
        assert_eq!(mon_list, dec_m);

        let cur = HostMsg::CursorShape(CursorKind::PointingHand);
        let enc_c = bincode::serialize(&cur).expect("serialización cursor");
        let dec_c: HostMsg = bincode::deserialize(&enc_c).expect("deserialización cursor");
        assert_eq!(cur, dec_c);

        let audio = HostMsg::AudioData(AudioFrame {
            sample_rate: 48_000,
            channels: 2,
            codec: AudioCodec::ImaAdpcm,
            data: vec![0, 0, 0, 0, 1, 2, 3, 4, 0xAB],
        });
        let enc_a = bincode::serialize(&audio).expect("serialización audio");
        let dec_a: HostMsg = bincode::deserialize(&enc_a).expect("deserialización audio");
        assert_eq!(audio, dec_a);

        for fmsg in [
            ClientMsg::SetAudioEnabled(true),
            ClientMsg::FileListReq {
                path: String::new(),
            },
            ClientMsg::FileUploadStart {
                transfer_id: 9,
                remote_dir: "C:\\Users".to_string(),
                file_name: "demo.zip".to_string(),
                total_size: 4096,
            },
            ClientMsg::FileUploadChunk {
                transfer_id: 9,
                offset: 0,
                data: vec![1, 2, 3],
            },
            ClientMsg::FileUploadFinish { transfer_id: 9 },
            ClientMsg::FileDownloadReq {
                transfer_id: 10,
                remote_path: "C:\\a.txt".to_string(),
            },
            ClientMsg::FileTransferCancel { transfer_id: 10 },
            ClientMsg::FileMkdir {
                parent_dir: "C:\\".to_string(),
                name: "Nueva".to_string(),
            },
            ClientMsg::P2pPunch {
                client_public_addr: "203.0.113.42:54321".to_string(),
            },
        ] {
            let enc = bincode::serialize(&fmsg).expect("serialización archivo cliente");
            let dec: ClientMsg = bincode::deserialize(&enc).expect("deserialización archivo cliente");
            assert_eq!(fmsg, dec);
        }

        let flist = HostMsg::FileListResp {
            path: "C:\\".to_string(),
            parent: Some("__DRIVES__".to_string()),
            entries: vec![FileEntry {
                name: "readme.txt".to_string(),
                is_dir: false,
                size: 42,
                modified_unix: 1_700_000_000,
            }],
            error: None,
        };
        let enc_fl = bincode::serialize(&flist).expect("serialización file list");
        let dec_fl: HostMsg = bincode::deserialize(&enc_fl).expect("deserialización file list");
        assert_eq!(flist, dec_fl);
    }
}
