//! Reconstrucción del framebuffer RGBA8 a partir de bloques incrementales JPEG o LZ4.

use anyhow::{anyhow, Context, Result};
use randydesk_proto::{CodecKind, EncodedTile, HostMsg};
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// Buffer de imagen completo listo para subirse a textura GPU en `egui`.
#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Estado persistente de reconstrucción de pantalla en el cliente.
pub struct FrameBuffer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl FrameBuffer {
    /// Crea un buffer vacío que se dimensionará con el primer cuadro recibido.
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            rgba: Vec::new(),
        }
    }

    /// Aplica un `HostMsg::VideoFrame` sobre el framebuffer y devuelve una copia lista para renderizar.
    pub fn apply_frame(&mut self, msg: &HostMsg) -> Result<DecodedFrame> {
        let HostMsg::VideoFrame {
            width,
            height,
            full_refresh,
            tiles,
            ..
        } = msg
        else {
            return Err(anyhow!("mensaje recibido no es VideoFrame"));
        };

        if *width == 0 || *height == 0 {
            return Err(anyhow!("dimensiones de video inválidas ({width}x{height})"));
        }

        let expected_len = (*width as usize)
            .checked_mul(*height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| anyhow!("desbordamiento calculando tamaño de framebuffer"))?;

        if self.width != *width || self.height != *height || self.rgba.len() != expected_len {
            self.width = *width;
            self.height = *height;
            self.rgba.resize(expected_len, 0);
        } else if *full_refresh && tiles.is_empty() {
            self.rgba.fill(0);
        }

        for tile in tiles {
            self.apply_tile(tile)?;
        }

        Ok(DecodedFrame {
            width: self.width,
            height: self.height,
            rgba: self.rgba.clone(),
        })
    }

    fn apply_tile(&mut self, tile: &EncodedTile) -> Result<()> {
        if tile.width == 0 || tile.height == 0 {
            return Ok(());
        }
        if tile.x >= self.width || tile.y >= self.height {
            return Err(anyhow!(
                "tile fuera de límites ({},{} en {}x{})",
                tile.x,
                tile.y,
                self.width,
                self.height
            ));
        }

        let tile_rgba = match tile.codec {
            CodecKind::Jpeg => {
                let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
                let mut decoder = JpegDecoder::new_with_options(&tile.data, options);
                decoder
                    .decode()
                    .context("falló decodificación JPEG de tile")?
            }
            CodecKind::Lz4Rgba => lz4_flex::decompress_size_prepended(&tile.data)
                .context("falló descompresión LZ4 de tile")?,
        };

        let expected_tile_bytes = (tile.width as usize) * (tile.height as usize) * 4;
        if tile_rgba.len() < expected_tile_bytes {
            return Err(anyhow!(
                "tile decodificado incompleto ({} < {expected_tile_bytes})",
                tile_rgba.len()
            ));
        }

        let copy_w = tile.width.min(self.width - tile.x) as usize;
        let copy_h = tile.height.min(self.height - tile.y) as usize;
        let dst_stride = (self.width as usize) * 4;
        let src_stride = (tile.width as usize) * 4;

        for row in 0..copy_h {
            let src_start = row * src_stride;
            let dst_start = ((tile.y as usize) + row) * dst_stride + (tile.x as usize) * 4;
            let bytes = copy_w * 4;
            self.rgba[dst_start..dst_start + bytes]
                .copy_from_slice(&tile_rgba[src_start..src_start + bytes]);
        }

        Ok(())
    }
}

impl Default for FrameBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_lz4_and_resizes_safely() {
        let mut fb = FrameBuffer::new();
        let raw_tile = vec![42u8; 16 * 16 * 4];
        let compressed = lz4_flex::compress_prepend_size(&raw_tile);

        let frame = HostMsg::VideoFrame {
            frame_id: 1,
            width: 32,
            height: 32,
            full_refresh: true,
            tiles: vec![EncodedTile {
                x: 16,
                y: 16,
                width: 16,
                height: 16,
                codec: CodecKind::Lz4Rgba,
                data: compressed,
            }],
        };

        let out = fb.apply_frame(&frame).expect("apply_frame lz4");
        assert_eq!((out.width, out.height), (32, 32));
        let px_offset = (16 * 32 + 16) * 4;
        assert_eq!(&out.rgba[px_offset..px_offset + 4], &[42, 42, 42, 42]);

        // Cambio de resolución debe redimensionar sin error
        let frame2 = HostMsg::VideoFrame {
            frame_id: 2,
            width: 64,
            height: 48,
            full_refresh: true,
            tiles: vec![],
        };
        let out2 = fb.apply_frame(&frame2).expect("resize");
        assert_eq!(out2.rgba.len(), 64 * 48 * 4);
    }

    #[test]
    fn rejects_out_of_bounds_tile() {
        let mut fb = FrameBuffer::new();
        let raw_tile = vec![10u8; 8 * 8 * 4];
        let compressed = lz4_flex::compress_prepend_size(&raw_tile);
        let bad = HostMsg::VideoFrame {
            frame_id: 1,
            width: 32,
            height: 32,
            full_refresh: true,
            tiles: vec![EncodedTile {
                x: 40,
                y: 0,
                width: 8,
                height: 8,
                codec: CodecKind::Lz4Rgba,
                data: compressed,
            }],
        };
        assert!(fb.apply_frame(&bad).is_err());
    }

    #[test]
    fn decodes_jpeg_tile_correctly() {
        let mut jpeg_bytes = Vec::new();
        let rgb = vec![128u8; 16 * 16 * 3];
        let enc = jpeg_encoder::Encoder::new(&mut jpeg_bytes, 90);
        enc.encode(&rgb, 16, 16, jpeg_encoder::ColorType::Rgb)
            .expect("encode jpeg");

        let mut fb = FrameBuffer::new();
        let frame = HostMsg::VideoFrame {
            frame_id: 1,
            width: 16,
            height: 16,
            full_refresh: true,
            tiles: vec![EncodedTile {
                x: 0,
                y: 0,
                width: 16,
                height: 16,
                codec: CodecKind::Jpeg,
                data: jpeg_bytes,
            }],
        };
        let out = fb.apply_frame(&frame).expect("apply jpeg frame");
        assert_eq!((out.width, out.height), (16, 16));
        assert_eq!(out.rgba.len(), 16 * 16 * 4);
        assert!(out.rgba[0].abs_diff(128) <= 4);
    }
}
