//! Framing binario asíncrono con prefijo de longitud de 32 bits y cota máxima anti-OOM.

use crate::MAX_FRAME_BYTES;
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Errores posibles durante lectura o escritura de mensajes en el stream.
#[derive(Debug, Error)]
pub enum FramingError {
    /// Fallo de entrada/salida en el stream subyacente.
    #[error("error de E/S en stream: {0}")]
    Io(#[from] std::io::Error),
    /// Error de serialización o deserialización binaria.
    #[error("error de codificación bincode: {0}")]
    Codec(#[from] bincode::Error),
    /// El frame supera el límite máximo permitido por seguridad.
    #[error("mensaje excede el límite máximo ({size} bytes > {MAX_FRAME_BYTES} bytes)")]
    FrameTooLarge { size: usize },
}

/// Serializa y escribe un mensaje precedido por su longitud `u32` en little-endian.
pub async fn write_msg<W, T>(writer: &mut W, msg: &T) -> Result<(), FramingError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let payload = bincode::serialize(msg)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(FramingError::FrameTooLarge {
            size: payload.len(),
        });
    }
    let len = payload.len() as u32;
    writer.write_all(&len.to_le_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Lee un mensaje con prefijo de longitud `u32` little-endian validando `MAX_FRAME_BYTES`.
pub async fn read_msg<R, T>(reader: &mut R) -> Result<T, FramingError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let size = u32::from_le_bytes(len_buf) as usize;
    if size > MAX_FRAME_BYTES {
        return Err(FramingError::FrameTooLarge { size });
    }
    let mut buf = vec![0u8; size];
    reader.read_exact(&mut buf).await?;
    let msg = bincode::deserialize(&buf)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientMsg, PROTOCOL_VERSION};
    use tokio::io::duplex;

    #[tokio::test]
    async fn framing_roundtrip_async() {
        let (mut tx, mut rx) = duplex(4096);
        let original = ClientMsg::Hello {
            version: PROTOCOL_VERSION,
        };
        write_msg(&mut tx, &original).await.expect("write_msg");
        let parsed: ClientMsg = read_msg(&mut rx).await.expect("read_msg");
        assert_eq!(original, parsed);
    }

    #[tokio::test]
    async fn framing_rejects_oversized_frame() {
        let (mut tx, mut rx) = duplex(64);
        let invalid_len = (MAX_FRAME_BYTES as u32) + 1;
        tx.write_all(&invalid_len.to_le_bytes())
            .await
            .expect("write length");
        let err = read_msg::<_, ClientMsg>(&mut rx)
            .await
            .expect_err("debe rechazar tamaño excesivo");
        assert!(matches!(err, FramingError::FrameTooLarge { .. }));
    }
}
