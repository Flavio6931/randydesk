//! Codificador incremental por bloques (tiles) con detección de cambios y soporte JPEG/LZ4.

use crate::capture::CapturedFrame;
use anyhow::Result;
use jpeg_encoder::{ColorType, Encoder};
use randydesk_proto::{CodecKind, EncodedTile, HostMsg, StreamSettings, TILE_SIZE};

/// Codificador con memoria del cuadro previo para enviar únicamente bloques modificados.
pub struct TileEncoder {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    tile_hashes: Vec<u64>,
    frame_id: u64,
    rgb_scratch: Vec<u8>,
    rgba_scratch: Vec<u8>,
    jpeg_scratch: Vec<u8>,
}

impl TileEncoder {
    /// Crea un nuevo codificador sin estado previo.
    pub fn new() -> Self {
        Self {
            width: 0,
            height: 0,
            cols: 0,
            rows: 0,
            tile_hashes: Vec::new(),
            frame_id: 0,
            rgb_scratch: Vec::new(),
            rgba_scratch: Vec::new(),
            jpeg_scratch: Vec::new(),
        }
    }

    /// Reinicia el caché interno de bloques para forzar un cuadro completo en la siguiente llamada.
    pub fn invalidate(&mut self) {
        self.tile_hashes.clear();
    }

    /// Codifica un cuadro capturado devolviendo `Ok(None)` si ningún bloque cambió respecto al anterior.
    pub fn encode(
        &mut self,
        frame: &CapturedFrame,
        settings: StreamSettings,
        force_keyframe: bool,
    ) -> Result<Option<HostMsg>> {
        let cols = frame.width.div_ceil(TILE_SIZE);
        let rows = frame.height.div_ceil(TILE_SIZE);
        let total_tiles = (cols * rows) as usize;
        let resolution_changed = self.width != frame.width || self.height != frame.height;
        let full_refresh = force_keyframe || resolution_changed || self.tile_hashes.len() != total_tiles;

        if full_refresh {
            self.width = frame.width;
            self.height = frame.height;
            self.cols = cols;
            self.rows = rows;
            self.tile_hashes.resize(total_tiles, u64::MAX);
        }

        let mut encoded_tiles = Vec::new();
        let stride = (frame.width as usize) * 4;

        for ty in 0..rows {
            for tx in 0..cols {
                let x = tx * TILE_SIZE;
                let y = ty * TILE_SIZE;
                let tw = (frame.width - x).min(TILE_SIZE);
                let th = (frame.height - y).min(TILE_SIZE);
                let idx = (ty * cols + tx) as usize;

                let hash = hash_bgra_tile(&frame.bgra, stride, x, y, tw, th);
                if !full_refresh && self.tile_hashes[idx] == hash {
                    continue;
                }

                let data = match settings.codec {
                    CodecKind::Jpeg => self.encode_jpeg_tile(
                        &frame.bgra,
                        stride,
                        x,
                        y,
                        tw,
                        th,
                        settings.jpeg_quality.clamp(35, 95),
                    )?,
                    CodecKind::Lz4Rgba => {
                        self.encode_lz4_tile(&frame.bgra, stride, x, y, tw, th)
                    }
                };

                self.tile_hashes[idx] = hash;
                encoded_tiles.push(EncodedTile {
                    x,
                    y,
                    width: tw,
                    height: th,
                    codec: settings.codec,
                    data,
                });
            }
        }

        if encoded_tiles.is_empty() {
            return Ok(None);
        }

        self.frame_id += 1;
        Ok(Some(HostMsg::VideoFrame {
            frame_id: self.frame_id,
            width: frame.width,
            height: frame.height,
            full_refresh,
            tiles: encoded_tiles,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn encode_jpeg_tile(
        &mut self,
        bgra: &[u8],
        stride: usize,
        x: u32,
        y: u32,
        tw: u32,
        th: u32,
        quality: u8,
    ) -> Result<Vec<u8>> {
        let pixel_count = (tw as usize) * (th as usize);
        self.rgb_scratch.clear();
        self.rgb_scratch.reserve(pixel_count * 3);

        for row in 0..(th as usize) {
            let start = ((y as usize) + row) * stride + (x as usize) * 4;
            let row_slice = &bgra[start..start + (tw as usize) * 4];
            for px in row_slice.as_chunks::<4>().0 {
                // BGRA -> RGB
                self.rgb_scratch.push(px[2]);
                self.rgb_scratch.push(px[1]);
                self.rgb_scratch.push(px[0]);
            }
        }

        self.jpeg_scratch.clear();
        let encoder = Encoder::new(&mut self.jpeg_scratch, quality);
        encoder.encode(&self.rgb_scratch, tw as u16, th as u16, ColorType::Rgb)?;
        Ok(self.jpeg_scratch.clone())
    }

    fn encode_lz4_tile(
        &mut self,
        bgra: &[u8],
        stride: usize,
        x: u32,
        y: u32,
        tw: u32,
        th: u32,
    ) -> Vec<u8> {
        let pixel_count = (tw as usize) * (th as usize);
        self.rgba_scratch.clear();
        self.rgba_scratch.reserve(pixel_count * 4);

        for row in 0..(th as usize) {
            let start = ((y as usize) + row) * stride + (x as usize) * 4;
            let row_slice = &bgra[start..start + (tw as usize) * 4];
            for px in row_slice.as_chunks::<4>().0 {
                // BGRA -> RGBA opaco para renderizado directo en textura GPU
                self.rgba_scratch.push(px[2]);
                self.rgba_scratch.push(px[1]);
                self.rgba_scratch.push(px[0]);
                self.rgba_scratch.push(255);
            }
        }

        lz4_flex::compress_prepend_size(&self.rgba_scratch)
    }
}

impl Default for TileEncoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash FNV-1a rápido de 64 bits sobre bloques de 8 bytes del tile BGRA.
fn hash_bgra_tile(bgra: &[u8], stride: usize, x: u32, y: u32, tw: u32, th: u32) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    let row_bytes = (tw as usize) * 4;

    for row in 0..(th as usize) {
        let start = ((y as usize) + row) * stride + (x as usize) * 4;
        let slice = &bgra[start..start + row_bytes];
        let (chunks, rem) = slice.as_chunks::<8>();
        for &chunk in chunks {
            let val = u64::from_le_bytes(chunk);
            hash ^= val;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        for &b in rem {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_frame(width: u32, height: u32, fill_b: u8) -> CapturedFrame {
        let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
        for px in bgra.as_chunks_mut::<4>().0 {
            px[0] = fill_b;
            px[1] = 100;
            px[2] = 200;
            px[3] = 255;
        }
        CapturedFrame { width, height, bgra }
    }

    #[test]
    fn encoder_emits_only_changed_tiles_and_detects_resolution_change() {
        let mut enc = TileEncoder::new();
        let settings = StreamSettings {
            codec: CodecKind::Jpeg,
            jpeg_quality: 80,
            target_fps: 30,
        };

        let mut frame = synthetic_frame(256, 256, 50); // 2x2 tiles de 128x128
        let first = enc
            .encode(&frame, settings, false)
            .expect("encode 1")
            .expect("debe emitir frame completo");
        match first {
            HostMsg::VideoFrame {
                full_refresh,
                tiles,
                ..
            } => {
                assert!(full_refresh);
                assert_eq!(tiles.len(), 4);
            }
            _ => panic!("mensaje inesperado"),
        }

        // Sin cambios -> None
        let unchanged = enc.encode(&frame, settings, false).expect("encode 2");
        assert!(unchanged.is_none());

        // Cambia 1 píxel en el bloque inferior derecho (x=200, y=200)
        let idx = (200 * 256 + 200) * 4;
        frame.bgra[idx] = 250;
        let diff = enc
            .encode(&frame, settings, false)
            .expect("encode 3")
            .expect("debe emitir solo 1 tile");
        match diff {
            HostMsg::VideoFrame {
                full_refresh,
                tiles,
                ..
            } => {
                assert!(!full_refresh);
                assert_eq!(tiles.len(), 1);
                assert_eq!((tiles[0].x, tiles[0].y), (128, 128));
                assert_eq!(tiles[0].codec, CodecKind::Jpeg);
            }
            _ => panic!("mensaje inesperado"),
        }

        // Cambio de resolución -> full_refresh y prueba LZ4
        let frame_lz4 = synthetic_frame(300, 150, 77);
        let lz4_settings = StreamSettings {
            codec: CodecKind::Lz4Rgba,
            ..settings
        };
        let res_msg = enc
            .encode(&frame_lz4, lz4_settings, false)
            .expect("encode 4")
            .expect("debe emitir full refresh");
        match res_msg {
            HostMsg::VideoFrame {
                full_refresh,
                tiles,
                width,
                height,
                ..
            } => {
                assert!(full_refresh);
                assert_eq!((width, height), (300, 150));
                assert_eq!(tiles.len(), 6); // 3 cols x 2 rows
                assert!(tiles.iter().all(|t| t.codec == CodecKind::Lz4Rgba));
            }
            _ => panic!("mensaje inesperado"),
        }
    }
}
