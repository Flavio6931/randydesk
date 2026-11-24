//! Decodificación incremental de bloques JPEG/LZ4 para actualización parcial de textura GPU.

use anyhow::{anyhow, Context, Result};
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

/// Decodifica únicamente los tiles incluidos en `HostMsg::VideoFrame` validando sus límites.
pub fn decode_frame_update(msg: &HostMsg) -> Result<FrameUpdate> {
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
        if let Some(patch) = decode_tile(tile, *width, *height)? {
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
}
