//! Codificador incremental por bloques (tiles) con detección de cambios y soporte JPEG/LZ4.

use crate::capture::CapturedFrame;
use anyhow::{anyhow, ensure, Result};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use randydesk_proto::{CodecKind, EncodedTile, HostMsg, StreamSettings, TILE_SIZE};

/// Umbral porcentual de tiles modificados para activar calidad adaptativa en movimiento.
const MOTION_TILE_THRESHOLD_PCT: usize = 35;
/// Reducción temporal de calidad JPEG durante movimiento intenso.
const MOTION_QUALITY_DROP: u8 = 16;
/// Piso mínimo de calidad JPEG al aplicar reducción adaptativa.
const MOTION_MIN_QUALITY: u8 = 42;

/// Codificador con memoria del cuadro previo para enviar únicamente bloques modificados.
pub struct TileEncoder {
    width: u32,
    height: u32,
    cols: u32,
    rows: u32,
    tile_hashes: Vec<u64>,
    degraded_tiles: Vec<bool>,
    frame_id: u64,
    last_jpeg_quality: Option<u8>,
    rgb_scratch: Vec<u8>,
    rgba_scratch: Vec<u8>,
    jpeg_scratch: Vec<u8>,
}

struct TileJob {
    idx: usize,
    x: u32,
    y: u32,
    tw: u32,
    th: u32,
    hash: u64,
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
            degraded_tiles: Vec::new(),
            frame_id: 0,
            last_jpeg_quality: None,
            rgb_scratch: Vec::new(),
            rgba_scratch: Vec::new(),
            jpeg_scratch: Vec::new(),
        }
    }

    /// Reinicia el caché interno de bloques para forzar un cuadro completo en la siguiente llamada.
    pub fn invalidate(&mut self) {
        self.tile_hashes.clear();
        self.degraded_tiles.clear();
    }

    /// Devuelve la calidad JPEG efectiva empleada en el último cuadro JPEG emitido.
    pub fn last_jpeg_quality(&self) -> Option<u8> {
        self.last_jpeg_quality
    }

    /// Codifica un cuadro capturado devolviendo `Ok(None)` si ningún bloque cambió respecto al anterior.
    pub fn encode(
        &mut self,
        frame: &CapturedFrame,
        settings: StreamSettings,
        force_keyframe: bool,
    ) -> Result<Option<HostMsg>> {
        let stride = (frame.width as usize) * 4;
        ensure!(
            frame.bgra.len() >= stride * (frame.height as usize),
            "buffer BGRA insuficiente ({} < {})",
            frame.bgra.len(),
            stride * (frame.height as usize)
        );

        let cols = frame.width.div_ceil(TILE_SIZE);
        let rows = frame.height.div_ceil(TILE_SIZE);
        let total_tiles = (cols * rows) as usize;
        let resolution_changed = self.width != frame.width || self.height != frame.height;
        let full_refresh =
            force_keyframe || resolution_changed || self.tile_hashes.len() != total_tiles;

        if full_refresh {
            self.width = frame.width;
            self.height = frame.height;
            self.cols = cols;
            self.rows = rows;
            self.tile_hashes.resize(total_tiles, u64::MAX);
            self.degraded_tiles.resize(total_tiles, false);
            self.degraded_tiles.fill(false);
        }

        let mut changed_flags = vec![false; total_tiles];
        let mut new_hashes = vec![0u64; total_tiles];
        let mut hash_changed_count = 0usize;

        for ty in 0..rows {
            for tx in 0..cols {
                let x = tx * TILE_SIZE;
                let y = ty * TILE_SIZE;
                let tw = (frame.width - x).min(TILE_SIZE);
                let th = (frame.height - y).min(TILE_SIZE);
                let idx = (ty * cols + tx) as usize;

                let hash = hash_bgra_tile(&frame.bgra, stride, x, y, tw, th);
                new_hashes[idx] = hash;
                if full_refresh || self.tile_hashes[idx] != hash {
                    changed_flags[idx] = true;
                    hash_changed_count += 1;
                }
            }
        }

        let base_quality = settings.jpeg_quality.clamp(35, 95);
        let high_motion = settings.codec == CodecKind::Jpeg
            && !full_refresh
            && total_tiles > 0
            && hash_changed_count * 100 > total_tiles * MOTION_TILE_THRESHOLD_PCT;
        let effective_quality = if high_motion {
            base_quality
                .saturating_sub(MOTION_QUALITY_DROP)
                .max(MOTION_MIN_QUALITY.min(base_quality))
        } else {
            base_quality
        };

        let mut jobs = Vec::new();
        for ty in 0..rows {
            for tx in 0..cols {
                let idx = (ty * cols + tx) as usize;
                let needs_crisp_refresh = !high_motion && self.degraded_tiles[idx];
                if changed_flags[idx] || needs_crisp_refresh {
                    let x = tx * TILE_SIZE;
                    let y = ty * TILE_SIZE;
                    let tw = (frame.width - x).min(TILE_SIZE);
                    let th = (frame.height - y).min(TILE_SIZE);
                    jobs.push(TileJob {
                        idx,
                        x,
                        y,
                        tw,
                        th,
                        hash: new_hashes[idx],
                    });
                }
            }
        }

        if jobs.is_empty() {
            return Ok(None);
        }

        let encoded_tiles = if jobs.len() == 1 {
            let job = &jobs[0];
            let data = encode_tile_payload(
                &frame.bgra,
                stride,
                job,
                settings.codec,
                effective_quality,
                &mut self.rgb_scratch,
                &mut self.rgba_scratch,
                &mut self.jpeg_scratch,
            )?;
            vec![EncodedTile {
                x: job.x,
                y: job.y,
                width: job.tw,
                height: job.th,
                codec: settings.codec,
                data,
            }]
        } else {
            let workers = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, jobs.len());
            let chunk_size = jobs.len().div_ceil(workers);
            let mut results: Vec<Option<EncodedTile>> = vec![None; jobs.len()];
            let bgra = &frame.bgra;
            let codec = settings.codec;

            std::thread::scope(|scope| -> Result<()> {
                let mut handles = Vec::with_capacity(workers);
                for (job_chunk, res_chunk) in
                    jobs.chunks(chunk_size).zip(results.chunks_mut(chunk_size))
                {
                    handles.push(scope.spawn(move || -> Result<()> {
                        let mut rgb_scratch = Vec::new();
                        let mut rgba_scratch = Vec::new();
                        let mut jpeg_scratch = Vec::new();
                        for (job, slot) in job_chunk.iter().zip(res_chunk.iter_mut()) {
                            let data = encode_tile_payload(
                                bgra,
                                stride,
                                job,
                                codec,
                                effective_quality,
                                &mut rgb_scratch,
                                &mut rgba_scratch,
                                &mut jpeg_scratch,
                            )?;
                            *slot = Some(EncodedTile {
                                x: job.x,
                                y: job.y,
                                width: job.tw,
                                height: job.th,
                                codec,
                                data,
                            });
                        }
                        Ok(())
                    }));
                }

                for handle in handles {
                    handle
                        .join()
                        .map_err(|_| anyhow!("pánico en hilo de codificación de tiles"))??;
                }
                Ok(())
            })?;

            results.into_iter().flatten().collect()
        };

        let is_degraded = settings.codec == CodecKind::Jpeg && effective_quality < base_quality;
        for job in &jobs {
            self.tile_hashes[job.idx] = job.hash;
            self.degraded_tiles[job.idx] = is_degraded;
        }
        self.last_jpeg_quality = (settings.codec == CodecKind::Jpeg).then_some(effective_quality);

        self.frame_id += 1;
        Ok(Some(HostMsg::VideoFrame {
            frame_id: self.frame_id,
            width: frame.width,
            height: frame.height,
            full_refresh,
            tiles: encoded_tiles,
        }))
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_tile_payload(
    bgra: &[u8],
    stride: usize,
    job: &TileJob,
    codec: CodecKind,
    quality: u8,
    rgb_scratch: &mut Vec<u8>,
    rgba_scratch: &mut Vec<u8>,
    jpeg_scratch: &mut Vec<u8>,
) -> Result<Vec<u8>> {
    match codec {
        CodecKind::Jpeg => encode_jpeg_tile_into(
            bgra,
            stride,
            job.x,
            job.y,
            job.tw,
            job.th,
            quality,
            rgb_scratch,
            jpeg_scratch,
        ),
        CodecKind::Lz4Rgba => {
            Ok(encode_lz4_tile_into(bgra, stride, job.x, job.y, job.tw, job.th, rgba_scratch))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_jpeg_tile_into(
    bgra: &[u8],
    stride: usize,
    x: u32,
    y: u32,
    tw: u32,
    th: u32,
    quality: u8,
    rgb_scratch: &mut Vec<u8>,
    jpeg_scratch: &mut Vec<u8>,
) -> Result<Vec<u8>> {
    let pixel_count = (tw as usize) * (th as usize);
    rgb_scratch.clear();
    rgb_scratch.reserve(pixel_count * 3);

    for row in 0..(th as usize) {
        let start = ((y as usize) + row) * stride + (x as usize) * 4;
        let row_slice = &bgra[start..start + (tw as usize) * 4];
        for px in row_slice.as_chunks::<4>().0 {
            // BGRA -> RGB
            rgb_scratch.push(px[2]);
            rgb_scratch.push(px[1]);
            rgb_scratch.push(px[0]);
        }
    }

    jpeg_scratch.clear();
    let mut encoder = Encoder::new(&mut *jpeg_scratch, quality);
    let sampling = if quality < 85 {
        SamplingFactor::F_2_2
    } else {
        SamplingFactor::R_4_4_4
    };
    encoder.set_sampling_factor(sampling);
    encoder.encode(rgb_scratch, tw as u16, th as u16, ColorType::Rgb)?;
    Ok(jpeg_scratch.clone())
}

fn encode_lz4_tile_into(
    bgra: &[u8],
    stride: usize,
    x: u32,
    y: u32,
    tw: u32,
    th: u32,
    rgba_scratch: &mut Vec<u8>,
) -> Vec<u8> {
    let pixel_count = (tw as usize) * (th as usize);
    rgba_scratch.clear();
    rgba_scratch.reserve(pixel_count * 4);

    for row in 0..(th as usize) {
        let start = ((y as usize) + row) * stride + (x as usize) * 4;
        let row_slice = &bgra[start..start + (tw as usize) * 4];
        for px in row_slice.as_chunks::<4>().0 {
            // BGRA -> RGBA opaco para renderizado directo en textura GPU
            rgba_scratch.push(px[2]);
            rgba_scratch.push(px[1]);
            rgba_scratch.push(px[0]);
            rgba_scratch.push(255);
        }
    }

    lz4_flex::compress_prepend_size(rgba_scratch)
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

    fn paint_tile(frame: &mut CapturedFrame, tx: u32, ty: u32, seed: u8) {
        let stride = (frame.width as usize) * 4;
        let x0 = tx * TILE_SIZE;
        let y0 = ty * TILE_SIZE;
        let tw = (frame.width - x0).min(TILE_SIZE);
        let th = (frame.height - y0).min(TILE_SIZE);
        for row in 0..th {
            for col in 0..tw {
                let idx = ((y0 + row) as usize) * stride + ((x0 + col) as usize) * 4;
                frame.bgra[idx] = seed.wrapping_add((col as u8).wrapping_mul(3));
                frame.bgra[idx + 1] = seed.wrapping_add((row as u8).wrapping_mul(5));
                frame.bgra[idx + 2] = seed ^ ((col ^ row) as u8);
                frame.bgra[idx + 3] = 255;
            }
        }
    }

    /// Extrae el byte de submuestreo `(H << 4) | V` de luminancia (Y) desde el marcador SOF0 (`0xFFC0`).
    fn jpeg_y_sampling_factor(jpeg: &[u8]) -> Option<u8> {
        let mut i = 0usize;
        while i + 1 < jpeg.len() {
            if jpeg[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = jpeg[i + 1];
            if marker == 0xD8 || marker == 0xD9 || marker == 0x00 || (0xD0..=0xD7).contains(&marker) {
                i += 2;
                continue;
            }
            if marker == 0xC0 && i + 11 < jpeg.len() {
                // SOF0: FF C0 [len:2] [prec:1] [h:2] [w:2] [ncomp:1] [comp_id:1] [sampling:1]
                return Some(jpeg[i + 11]);
            }
            if i + 3 >= jpeg.len() {
                break;
            }
            let seg_len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
            i += 2 + seg_len;
        }
        None
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

        // Cambia 1 píxel en el bloque inferior derecho (x=200, y=200) -> 1/4 = 25% <= 35%
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

    #[test]
    fn jpeg_sampling_uses_420_below_85_and_444_at_or_above_85() {
        let frame = synthetic_frame(128, 128, 90);

        let mut enc_low = TileEncoder::new();
        let msg_84 = enc_low
            .encode(
                &frame,
                StreamSettings {
                    codec: CodecKind::Jpeg,
                    jpeg_quality: 84,
                    target_fps: 30,
                },
                false,
            )
            .expect("encode q84")
            .expect("frame q84");
        if let HostMsg::VideoFrame { tiles, .. } = msg_84 {
            assert_eq!(jpeg_y_sampling_factor(&tiles[0].data), Some(0x22));
        } else {
            panic!("mensaje inesperado");
        }

        let mut enc_high = TileEncoder::new();
        let msg_85 = enc_high
            .encode(
                &frame,
                StreamSettings {
                    codec: CodecKind::Jpeg,
                    jpeg_quality: 85,
                    target_fps: 30,
                },
                false,
            )
            .expect("encode q85")
            .expect("frame q85");
        if let HostMsg::VideoFrame { tiles, .. } = msg_85 {
            assert_eq!(jpeg_y_sampling_factor(&tiles[0].data), Some(0x11));
        } else {
            panic!("mensaje inesperado");
        }
    }

    #[test]
    fn adaptive_motion_reduces_quality_and_emits_crisp_refresh_when_motion_stops() {
        let mut enc = TileEncoder::new();
        let settings = StreamSettings {
            codec: CodecKind::Jpeg,
            jpeg_quality: 85,
            target_fps: 30,
        };

        // 2x2 tiles (4 tiles). Frame inicial siempre a calidad base (85 -> 4:4:4)
        let mut frame = synthetic_frame(256, 256, 30);
        let _ = enc.encode(&frame, settings, false).unwrap().unwrap();
        assert_eq!(enc.last_jpeg_quality(), Some(85));

        // Movimiento: cambian 2 de 4 tiles (50% > 35%) -> baja calidad a 85 - 16 = 69 (4:2:0)
        paint_tile(&mut frame, 0, 0, 10);
        paint_tile(&mut frame, 1, 0, 20);
        let motion_msg = enc
            .encode(&frame, settings, false)
            .expect("encode motion")
            .expect("debe emitir tiles en movimiento");
        assert_eq!(enc.last_jpeg_quality(), Some(69));
        match motion_msg {
            HostMsg::VideoFrame {
                full_refresh,
                tiles,
                ..
            } => {
                assert!(!full_refresh);
                assert_eq!(tiles.len(), 2);
                assert!(tiles
                    .iter()
                    .all(|t| jpeg_y_sampling_factor(&t.data) == Some(0x22)));
            }
            _ => panic!("mensaje inesperado"),
        }

        // Cesa el movimiento (frame idéntico) -> debe emitir refresco nítido de los 2 tiles degradados a calidad 85 (4:4:4)
        let crisp_msg = enc
            .encode(&frame, settings, false)
            .expect("encode crisp")
            .expect("debe emitir refresco nítido al cesar el movimiento");
        assert_eq!(enc.last_jpeg_quality(), Some(85));
        match crisp_msg {
            HostMsg::VideoFrame {
                full_refresh,
                tiles,
                ..
            } => {
                assert!(!full_refresh);
                assert_eq!(tiles.len(), 2);
                assert_eq!((tiles[0].x, tiles[0].y), (0, 0));
                assert_eq!((tiles[1].x, tiles[1].y), (128, 0));
                assert!(tiles
                    .iter()
                    .all(|t| jpeg_y_sampling_factor(&t.data) == Some(0x11)));
            }
            _ => panic!("mensaje inesperado"),
        }

        // Siguiente frame sin cambios -> ya no hay tiles degradados, devuelve None
        assert!(enc.encode(&frame, settings, false).unwrap().is_none());
    }

    #[test]
    fn adaptive_motion_respects_minimum_quality_42_and_parallel_order() {
        let mut enc = TileEncoder::new();
        let settings = StreamSettings {
            codec: CodecKind::Jpeg,
            jpeg_quality: 50,
            target_fps: 30,
        };

        // 3x2 tiles (6 tiles)
        let mut frame = synthetic_frame(384, 256, 40);
        let _ = enc.encode(&frame, settings, false).unwrap().unwrap();
        assert_eq!(enc.last_jpeg_quality(), Some(50));

        // Cambian 4 de 6 tiles (66.7% > 35%) -> 50 - 16 = 34, clamped a mínimo 42
        paint_tile(&mut frame, 1, 0, 11);
        paint_tile(&mut frame, 2, 0, 22);
        paint_tile(&mut frame, 0, 1, 33);
        paint_tile(&mut frame, 2, 1, 44);
        let msg = enc.encode(&frame, settings, false).unwrap().unwrap();
        assert_eq!(enc.last_jpeg_quality(), Some(42));
        if let HostMsg::VideoFrame { tiles, .. } = msg {
            let coords: Vec<(u32, u32)> = tiles.iter().map(|t| (t.x, t.y)).collect();
            assert_eq!(coords, vec![(128, 0), (256, 0), (0, 128), (256, 128)]);
        } else {
            panic!("mensaje inesperado");
        }

        // Al cesar el movimiento vuelve a 50 y limpia estado
        let crisp = enc.encode(&frame, settings, false).unwrap().unwrap();
        assert_eq!(enc.last_jpeg_quality(), Some(50));
        if let HostMsg::VideoFrame { tiles, .. } = crisp {
            assert_eq!(tiles.len(), 4);
        } else {
            panic!("mensaje inesperado");
        }
        assert!(enc.encode(&frame, settings, false).unwrap().is_none());
    }
}
