//! Codificador de video H.264 (OpenH264 Annex-B) optimizado para escritorio en tiempo real.

use crate::encoder::downscale_bgra;
use crate::h264_hw::MfH264Encoder;
use anyhow::{anyhow, ensure, Result};
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, RateControlMode,
    SpsPpsStrategy, UsageType,
};
use openh264::formats::{BgraSliceU8, YUVBuffer};
use openh264::OpenH264API;
use randydesk_proto::{CodecKind, EncodedTile, HostMsg, StreamSettings};

/// Codificador H.264 persistente por sesión con detección rápida de cuadros estáticos y ajuste de bitrate.
/// Prioriza codificacion hardware via Windows Media Foundation (NVENC/QuickSync/AMF) y cae a OpenH264 CPU.
pub struct H264FrameEncoder {
    hw_encoder: Option<MfH264Encoder>,
    hw_disabled: bool,
    hw_empty_streak: u8,
    encoder: Option<Encoder>,
    width: u32,
    height: u32,
    bitrate_bps: u32,
    fps: u8,
    last_hash: Option<u64>,
    frame_id: u64,
    yuv: Option<YUVBuffer>,
    scaled_bgra: Vec<u8>,
    even_bgra: Vec<u8>,
}

impl Default for H264FrameEncoder {
    fn default() -> Self {
        Self::new()
    }
}

impl H264FrameEncoder {
    /// Crea un nuevo codificador H.264 sin estado previo.
    pub fn new() -> Self {
        Self {
            hw_encoder: None,
            hw_disabled: false,
            hw_empty_streak: 0,
            encoder: None,
            width: 0,
            height: 0,
            bitrate_bps: 0,
            fps: 30,
            last_hash: None,
            frame_id: 0,
            yuv: None,
            scaled_bgra: Vec::new(),
            even_bgra: Vec::new(),
        }
    }

    /// Reinicia el detector de cuadro estático y fuerza un keyframe IDR en el próximo frame.
    pub fn invalidate(&mut self) {
        self.last_hash = None;
        if let Some(enc) = self.encoder.as_mut() {
            enc.force_intra_frame();
        }
    }

    /// Codifica un cuadro BGRA a H.264 Annex-B, devolviendo `Ok(None)` si no hubo cambios y no es keyframe.
    pub fn encode(
        &mut self,
        bgra: &[u8],
        width: u32,
        height: u32,
        settings: StreamSettings,
        scale_percent: u8,
        force_keyframe: bool,
    ) -> Result<Option<HostMsg>> {
        let src_stride = (width as usize) * 4;
        ensure!(
            bgra.len() >= src_stride * (height as usize),
            "buffer BGRA insuficiente ({} < {})",
            bgra.len(),
            src_stride * (height as usize)
        );

        let pct = scale_percent.clamp(50, 100);
        let (work_w, work_h, work_bgra): (u32, u32, &[u8]) =
            if pct < 100 && width >= 128 && height >= 128 {
                let dst_w = ((width * (pct as u32)) / 100).max(64) & !1;
                let dst_h = ((height * (pct as u32)) / 100).max(64) & !1;
                downscale_bgra(
                    bgra,
                    width,
                    height,
                    dst_w,
                    dst_h,
                    pct == 50,
                    &mut self.scaled_bgra,
                );
                (dst_w, dst_h, self.scaled_bgra.as_slice())
            } else {
                (width, height, bgra)
            };

        let even_w = (work_w & !1).max(64);
        let even_h = (work_h & !1).max(64);
        ensure!(
            even_w <= work_w && even_h <= work_h,
            "resolución demasiado pequeña para H.264 ({work_w}x{work_h})"
        );

        let tight_bgra: &[u8] = if even_w == work_w && even_h == work_h {
            work_bgra
        } else {
            let src_row_bytes = (work_w as usize) * 4;
            let dst_row_bytes = (even_w as usize) * 4;
            self.even_bgra.resize(dst_row_bytes * (even_h as usize), 0);
            for y in 0..(even_h as usize) {
                let src_start = y * src_row_bytes;
                let dst_start = y * dst_row_bytes;
                self.even_bgra[dst_start..dst_start + dst_row_bytes]
                    .copy_from_slice(&work_bgra[src_start..src_start + dst_row_bytes]);
            }
            self.even_bgra.as_slice()
        };

        let hash = fast_sample_hash(tight_bgra);
        let has_active_encoder = self.hw_encoder.is_some() || self.encoder.is_some();
        if !force_keyframe && self.last_hash == Some(hash) && has_active_encoder {
            return Ok(None);
        }

        let target_bps = bitrate_for_quality(settings.jpeg_quality);
        let target_fps = settings.target_fps.clamp(5, 60);

        if !self.hw_disabled && even_w >= 128 && even_h >= 128 {
            let need_hw_recreate = self.hw_encoder.as_ref().is_none_or(|hw| {
                force_keyframe || !hw.matches_params(even_w, even_h, target_fps, target_bps)
            });
            if need_hw_recreate {
                self.hw_encoder = MfH264Encoder::try_new(even_w, even_h, target_fps, target_bps);
                self.hw_empty_streak = 0;
                if self.hw_encoder.is_none() {
                    self.hw_disabled = true;
                }
            }
            if let Some(hw) = self.hw_encoder.as_mut() {
                match hw.encode_bgra(tight_bgra) {
                    Ok(Some((data, is_idr))) => {
                        self.hw_empty_streak = 0;
                        self.last_hash = Some(hash);
                        self.frame_id += 1;
                        return Ok(Some(HostMsg::VideoFrame {
                            frame_id: self.frame_id,
                            width: even_w,
                            height: even_h,
                            full_refresh: is_idr || need_hw_recreate,
                            tiles: vec![EncodedTile {
                                x: 0,
                                y: 0,
                                width: even_w,
                                height: even_h,
                                codec: CodecKind::H264,
                                data,
                            }],
                        }));
                    }
                    Ok(None) => {
                        self.hw_empty_streak = self.hw_empty_streak.saturating_add(1);
                        if self.hw_empty_streak >= 8 || (force_keyframe && self.last_hash.is_none()) {
                            tracing::warn!(
                                "H.264 GPU MFT no emitio cuadro inmediato, cambiando a OpenH264 CPU"
                            );
                            self.hw_encoder = None;
                            self.hw_disabled = true;
                        } else {
                            return Ok(None);
                        }
                    }
                    Err(err) => {
                        tracing::warn!("H.264 GPU MFT fallo en ejecucion, cambiando a OpenH264 CPU: {err:#}");
                        self.hw_encoder = None;
                        self.hw_disabled = true;
                    }
                }
            }
        }

        let bitrate_diff = target_bps.abs_diff(self.bitrate_bps);
        let needs_recreate = self.encoder.is_none()
            || self.width != even_w
            || self.height != even_h
            || self.fps != target_fps
            || bitrate_diff * 4 > self.bitrate_bps.max(1);

        let mut is_new_encoder = false;
        if needs_recreate {
            let config = EncoderConfig::new()
                .usage_type(UsageType::ScreenContentRealTime)
                .rate_control_mode(RateControlMode::Bitrate)
                .bitrate(BitRate::from_bps(target_bps))
                .max_frame_rate(FrameRate::from_hz(target_fps as f32))
                .skip_frames(false)
                .sps_pps_strategy(SpsPpsStrategy::ConstantId)
                .intra_frame_period(IntraFramePeriod::from_num_frames((target_fps as u32) * 3));

            let enc = Encoder::with_api_config(OpenH264API::from_source(), config)
                .map_err(|e| anyhow!("no se pudo crear encoder OpenH264: {e}"))?;
            self.encoder = Some(enc);
            self.width = even_w;
            self.height = even_h;
            self.bitrate_bps = target_bps;
            self.fps = target_fps;
            self.yuv = Some(YUVBuffer::new(even_w as usize, even_h as usize));
            is_new_encoder = true;
        }

        let slice = BgraSliceU8::new(tight_bgra, (even_w as usize, even_h as usize));
        let yuv = self
            .yuv
            .as_mut()
            .expect("YUVBuffer inicializado junto al encoder");
        yuv.read_bgra8(slice);

        let enc = self
            .encoder
            .as_mut()
            .expect("Encoder OpenH264 inicializado");
        if force_keyframe && !is_new_encoder {
            enc.force_intra_frame();
        }

        let bitstream = enc
            .encode(yuv)
            .map_err(|e| anyhow!("error codificando cuadro H.264: {e}"))?;
        let data = bitstream.to_vec();
        if data.is_empty() {
            return Ok(None);
        }

        let is_idr = matches!(bitstream.frame_type(), FrameType::IDR | FrameType::I) || is_new_encoder;
        self.last_hash = Some(hash);
        self.frame_id += 1;

        Ok(Some(HostMsg::VideoFrame {
            frame_id: self.frame_id,
            width: even_w,
            height: even_h,
            full_refresh: is_idr,
            tiles: vec![EncodedTile {
                x: 0,
                y: 0,
                width: even_w,
                height: even_h,
                codec: CodecKind::H264,
                data,
            }],
        }))
    }
}

fn bitrate_for_quality(quality: u8) -> u32 {
    let q = quality.clamp(35, 95) as u32;
    400_000 + ((q - 35) * 4_100_000) / 60
}

fn fast_sample_hash(bgra: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    // Muestrea palabras de 32 bits cada 16 píxeles (64 bytes) para detectar cambios en <0.2 ms.
    let mut i = 0usize;
    while i + 4 <= bgra.len() {
        let px = u32::from_le_bytes([bgra[i], bgra[i + 1], bgra[i + 2], bgra[i + 3]]);
        hash ^= px as u64;
        hash = hash.wrapping_mul(0x100000001b3);
        i += 64;
    }
    hash ^= bgra.len() as u64;
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h264_roundtrip_encodes_keyframe_and_skips_static_frame() {
        let mut enc = H264FrameEncoder::new();
        let w = 128u32;
        let h = 128u32;
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                bgra[idx] = (x * 2) as u8;
                bgra[idx + 1] = (y * 2) as u8;
                bgra[idx + 2] = 180;
                bgra[idx + 3] = 255;
            }
        }

        let settings = StreamSettings {
            codec: CodecKind::H264,
            jpeg_quality: 72,
            target_fps: 30,
        };

        let first = enc
            .encode(&bgra, w, h, settings, 100, true)
            .expect("encode H264 1")
            .expect("debe emitir primer IDR");
        match first {
            HostMsg::VideoFrame {
                width,
                height,
                full_refresh,
                tiles,
                ..
            } => {
                assert_eq!((width, height), (128, 128));
                assert!(full_refresh);
                assert_eq!(tiles.len(), 1);
                assert_eq!(tiles[0].codec, CodecKind::H264);
                assert!(!tiles[0].data.is_empty());
            }
            other => panic!("mensaje inesperado: {other:?}"),
        }

        // Cuadro estático sin cambios -> None
        let static_res = enc
            .encode(&bgra, w, h, settings, 100, false)
            .expect("encode H264 static");
        assert!(static_res.is_none());

        // Cuadro modificado -> P-frame
        for y in 16..48 {
            for x in 16..48 {
                let idx = ((y * w + x) * 4) as usize;
                bgra[idx] = 250;
                bgra[idx + 1] = 20;
            }
        }
        let delta = enc
            .encode(&bgra, w, h, settings, 100, false)
            .expect("encode H264 delta")
            .expect("debe emitir delta H264");
        if let HostMsg::VideoFrame { tiles, .. } = delta {
            assert_eq!(tiles[0].codec, CodecKind::H264);
            assert!(!tiles[0].data.is_empty());
        } else {
            panic!("mensaje inesperado");
        }
    }
}
