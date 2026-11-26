//! Decodificación incremental de bloques JPEG/LZ4 y stream H.264 (OpenH264) para textura GPU.

use anyhow::{anyhow, Context, Result};
use openh264::decoder::Decoder as OpenH264Decoder;
use openh264::formats::YUVSource;
use openh264::nal_units;
use randydesk_proto::{CodecKind, EncodedTile, HostMsg};
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;
use zune_jpeg::JpegDecoder;

/// Bloque decodificado en RGBA8 listo para subirse con `TextureHandle::set_partial`.
#[derive(Debug, Clone)]
pub struct DecodedTilePatch {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Actualización de video decodificada para aplicarse incrementalmente sobre la textura GPU.
#[derive(Debug, Clone)]
pub struct FrameUpdate {
    pub width: u32,
    pub height: u32,
    pub full_refresh: bool,
    pub tiles: Vec<DecodedTilePatch>,
}

/// Decodificador con estado de sesión que soporta JPEG, LZ4 y H.264 inter-frame.
pub struct FrameDecoder {
    h264: Option<OpenH264Decoder>,
}

impl Default for FrameDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameDecoder {
    /// Crea un decodificador nuevo inicializando OpenH264 bajo demanda.
    pub fn new() -> Self {
        Self { h264: None }
    }

    /// Decodifica cualquier `HostMsg::VideoFrame` (JPEG, LZ4 o H.264 Annex-B).
    pub fn decode(&mut self, msg: &HostMsg) -> Result<FrameUpdate> {
        decode_frame_with(msg, Some(&mut self.h264))
    }
}

/// Decodifica únicamente los tiles incluidos en `HostMsg::VideoFrame` validando sus límites.
#[allow(dead_code)]
pub fn decode_frame_update(msg: &HostMsg) -> Result<FrameUpdate> {
    let mut temp_h264 = None;
    decode_frame_with(msg, Some(&mut temp_h264))
}

fn decode_frame_with(
    msg: &HostMsg,
    mut h264_state: Option<&mut Option<OpenH264Decoder>>,
) -> Result<FrameUpdate> {
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

    let mut decoded_tiles = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if let Some(patch) = decode_tile(tile, *width, *height, h264_state.as_deref_mut())? {
            decoded_tiles.push(patch);
        }
    }

    Ok(FrameUpdate {
        width: *width,
        height: *height,
        full_refresh: *full_refresh,
        tiles: decoded_tiles,
    })
}

fn decode_tile(
    tile: &EncodedTile,
    frame_width: u32,
    frame_height: u32,
    h264_state: Option<&mut Option<OpenH264Decoder>>,
) -> Result<Option<DecodedTilePatch>> {
    if tile.width == 0 || tile.height == 0 {
        return Ok(None);
    }
    if tile.x >= frame_width || tile.y >= frame_height {
        return Err(anyhow!(
            "tile fuera de límites ({},{} en {}x{})",
            tile.x,
            tile.y,
            frame_width,
            frame_height
        ));
    }

    if tile.codec == CodecKind::H264 {
        let holder = h264_state.ok_or_else(|| anyhow!("decodificador H.264 no disponible"))?;
        if holder.is_none() {
            let dec = OpenH264Decoder::new()
                .map_err(|e| anyhow!("no se pudo inicializar OpenH264 decoder: {e}"))?;
            *holder = Some(dec);
        }
        let dec = holder.as_mut().expect("decoder H.264 inicializado");
        let mut last_img: Option<(u32, u32, Vec<u8>)> = None;
        for nal in nal_units(&tile.data) {
            if let Ok(Some(yuv)) = dec.decode(nal) {
                let (w, h) = yuv.dimensions();
                let mut rgba = vec![255u8; yuv.rgba8_len()];
                yuv.write_rgba8(&mut rgba);
                last_img = Some((w as u32, h as u32, rgba));
            }
        }
        return Ok(last_img.map(|(w, h, rgba)| DecodedTilePatch {
            x: tile.x,
            y: tile.y,
            width: w,
            height: h,
            rgba,
        }));
    }

    let mut rgba = match tile.codec {
        CodecKind::Jpeg => {
            let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA);
            let mut decoder = JpegDecoder::new_with_options(&tile.data, options);
            decoder
                .decode()
                .context("falló decodificación JPEG de tile")?
        }
        CodecKind::Lz4Rgba => lz4_flex::decompress_size_prepended(&tile.data)
            .context("falló descompresión LZ4 de tile")?,
        CodecKind::H264 => unreachable!(),
    };

    let expected_tile_bytes = (tile.width as usize) * (tile.height as usize) * 4;
    if rgba.len() < expected_tile_bytes {
        return Err(anyhow!(
            "tile decodificado incompleto ({} < {expected_tile_bytes})",
            rgba.len()
        ));
    }

    let copy_w = tile.width.min(frame_width - tile.x);
    let copy_h = tile.height.min(frame_height - tile.y);
    if copy_w != tile.width || copy_h != tile.height {
        let mut cropped = vec![0u8; (copy_w as usize) * (copy_h as usize) * 4];
        let src_stride = (tile.width as usize) * 4;
        let dst_stride = (copy_w as usize) * 4;
        for row in 0..(copy_h as usize) {
            let src_start = row * src_stride;
            let dst_start = row * dst_stride;
            cropped[dst_start..dst_start + dst_stride]
                .copy_from_slice(&rgba[src_start..src_start + dst_stride]);
        }
        rgba = cropped;
    } else {
        rgba.truncate(expected_tile_bytes);
    }

    Ok(Some(DecodedTilePatch {
        x: tile.x,
        y: tile.y,
        width: copy_w,
        height: copy_h,
        rgba,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use randydesk_host::h264_codec::H264FrameEncoder;
    use randydesk_proto::StreamSettings;

    #[test]
    fn decodes_lz4_patch_without_full_framebuffer_copy() {
        let raw_tile = vec![42u8; 16 * 16 * 4];
        let compressed = lz4_flex::compress_prepend_size(&raw_tile);

        let frame = HostMsg::VideoFrame {
            frame_id: 1,
            width: 1920,
            height: 1080,
            full_refresh: false,
            tiles: vec![EncodedTile {
                x: 128,
                y: 256,
                width: 16,
                height: 16,
                codec: CodecKind::Lz4Rgba,
                data: compressed,
            }],
        };

        let update = decode_frame_update(&frame).expect("decode_frame_update lz4");
        assert_eq!((update.width, update.height), (1920, 1080));
        assert!(!update.full_refresh);
        assert_eq!(update.tiles.len(), 1);
        assert_eq!((update.tiles[0].x, update.tiles[0].y), (128, 256));
        assert_eq!(update.tiles[0].rgba.len(), 16 * 16 * 4);
        assert_eq!(&update.tiles[0].rgba[0..4], &[42, 42, 42, 42]);
    }

    #[test]
    fn rejects_out_of_bounds_tile() {
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
        assert!(decode_frame_update(&bad).is_err());
    }

    #[test]
    fn decodes_jpeg_tile_correctly() {
        let mut jpeg_bytes = Vec::new();
        let rgb = vec![128u8; 16 * 16 * 3];
        let enc = jpeg_encoder::Encoder::new(&mut jpeg_bytes, 90);
        enc.encode(&rgb, 16, 16, jpeg_encoder::ColorType::Rgb)
            .expect("encode jpeg");

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
        let update = decode_frame_update(&frame).expect("decode jpeg");
        assert_eq!(update.tiles.len(), 1);
        assert_eq!(update.tiles[0].rgba.len(), 16 * 16 * 4);
        assert!(update.tiles[0].rgba[0].abs_diff(128) <= 4);
    }

    #[test]
    fn roundtrip_h264_encode_and_decode() {
        let mut enc = H264FrameEncoder::new();
        let mut dec = FrameDecoder::new();
        let w = 128u32;
        let h = 128u32;
        let bgra = vec![200u8; (w * h * 4) as usize];
        let settings = StreamSettings {
            codec: CodecKind::H264,
            jpeg_quality: 75,
            target_fps: 30,
        };
        let msg = enc
            .encode(&bgra, w, h, settings, 100, true)
            .expect("encode")
            .expect("frame");
        let update = dec.decode(&msg).expect("decode H264");
        assert_eq!((update.width, update.height), (128, 128));
        assert_eq!(update.tiles.len(), 1);
        assert_eq!(update.tiles[0].rgba.len(), (w * h * 4) as usize);
    }
}
