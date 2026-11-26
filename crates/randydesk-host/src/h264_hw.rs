//! Codificador H.264 acelerado por GPU mediante Windows Media Foundation (MFT).
//! Si no existe codificador hardware o falla en ejecucion, el invocador cae a OpenH264 CPU.

use anyhow::{anyhow, Result};
use std::ffi::c_void;
use tracing::{debug, info, warn};
use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::{
    eAVEncH264VProfile_Base, IMFActivate, IMFMediaBuffer, IMFMediaEventGenerator, IMFMediaType,
    IMFSample, IMFTransform, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample, MFShutdown,
    MFStartup, MFTEnumEx, MFVideoFormat_H264, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    METransformHaveOutput, METransformNeedInput, MFT_CATEGORY_VIDEO_ENCODER,
    MFT_ENUM_FLAG_ASYNCMFT, MFT_ENUM_FLAG_HARDWARE, MFT_ENUM_FLAG_SORTANDFILTER,
    MFT_MESSAGE_COMMAND_FLUSH, MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
    MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_OUTPUT_DATA_BUFFER, MFT_OUTPUT_STREAM_INFO,
    MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO, MFSTARTUP_NOSOCKET, MF_VERSION,
    MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE, MF_EVENT_FLAG_NONE,
    MF_LOW_LATENCY, MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_SUBTYPE,
    MF_TRANSFORM_ASYNC, MF_TRANSFORM_ASYNC_UNLOCK, MFMediaType_Video,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED,
};

struct MfSessionGuard {
    mf_started: bool,
    com_owned: bool,
}

impl Drop for MfSessionGuard {
    fn drop(&mut self) {
        if self.mf_started {
            unsafe {
                let _ = MFShutdown();
            }
        }
        if self.com_owned {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Codificador H.264 por hardware usando Media Foundation (`IMFTransform`).
/// El campo `transform` se declara antes de `_guard` para liberarse antes de `MFShutdown`/`CoUninitialize`.
pub struct MfH264Encoder {
    transform: IMFTransform,
    event_gen: Option<IMFMediaEventGenerator>,
    is_async: bool,
    width: u32,
    height: u32,
    fps: u8,
    bitrate_bps: u32,
    nv12_buf: Vec<u8>,
    frame_idx: u64,
    pending_input_tokens: u32,
    out_stream_id: u32,
    out_provides_samples: bool,
    out_buf_size: u32,
    _guard: MfSessionGuard,
}

// SAFETY: El transform MFT se inicializa bajo `COINIT_MULTITHREADED` (MTA) y solo se usa con `&mut self`.
unsafe impl Send for MfH264Encoder {}

impl MfH264Encoder {
    /// Intenta inicializar un codificador H.264 hardware para las dimensiones y bitrate indicados.
    /// Devuelve `None` silenciosamente si no hay GPU MFT compatible.
    pub fn try_new(width: u32, height: u32, fps: u8, bitrate_bps: u32) -> Option<Self> {
        if width < 128 || height < 128 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return None;
        }
        match unsafe { Self::init_inner(width, height, fps, bitrate_bps) } {
            Ok(enc) => {
                info!("Codificador H.264 GPU Media Foundation activo ({width}x{height} @ {fps} fps)");
                Some(enc)
            }
            Err(err) => {
                debug!("H.264 GPU Media Foundation no disponible ({width}x{height}): {err:#}");
                None
            }
        }
    }

    unsafe fn init_inner(width: u32, height: u32, fps: u8, bitrate_bps: u32) -> Result<Self> {
        let com_owned = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let mf_res = MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET);
        if let Err(err) = mf_res {
            if com_owned {
                CoUninitialize();
            }
            return Err(anyhow!("MFStartup fallo: {err}"));
        }
        let guard = MfSessionGuard {
            mf_started: true,
            com_owned,
        };

        let output_info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut activates_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        let flags = (MFT_ENUM_FLAG_HARDWARE.0
            | MFT_ENUM_FLAG_SORTANDFILTER.0
            | MFT_ENUM_FLAG_ASYNCMFT.0) as u32;
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            windows::Win32::Media::MediaFoundation::MFT_ENUM_FLAG(flags as i32),
            None,
            Some(&output_info),
            &mut activates_ptr,
            &mut count,
        )
        .map_err(|e| anyhow!("MFTEnumEx fallo: {e}"))?;

        let mut activates = Vec::with_capacity(count as usize);
        if !activates_ptr.is_null() {
            for i in 0..(count as usize) {
                if let Some(act) = std::ptr::read(activates_ptr.add(i)) {
                    activates.push(act);
                }
            }
            CoTaskMemFree(Some(activates_ptr as *const c_void));
        }

        if activates.is_empty() {
            return Err(anyhow!("no se encontraron codificadores H.264 hardware"));
        }

        let mut last_err = anyhow!("ningun MFT hardware acepto formato NV12->H264");
        let mut guard = Some(guard);
        for activate in activates {
            let Ok(transform) = activate.ActivateObject::<IMFTransform>() else {
                continue;
            };
            let current_guard = guard.take().expect("MfSessionGuard presente");
            match Self::configure_transform(transform, width, height, fps, bitrate_bps, current_guard)
            {
                Ok(ready) => return Ok(ready),
                Err((err, recovered_guard)) => {
                    last_err = err;
                    guard = Some(recovered_guard);
                }
            }
        }
        Err(last_err)
    }

    unsafe fn configure_transform(
        transform: IMFTransform,
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
        guard: MfSessionGuard,
    ) -> std::result::Result<Self, (anyhow::Error, MfSessionGuard)> {
        let mut is_async = false;
        if let Ok(attrs) = transform.GetAttributes() {
            is_async = attrs.GetUINT32(&MF_TRANSFORM_ASYNC).unwrap_or(0) != 0;
            if is_async && attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1).is_err() {
                return Err((anyhow!("no se pudo desbloquear MFT asincrono"), guard));
            }
            // Solicita modo baja latencia sin B-frames para escritorio remoto en tiempo real.
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
        }

        let event_gen = if is_async {
            match transform.cast::<IMFMediaEventGenerator>() {
                Ok(gen) => Some(gen),
                Err(e) => {
                    return Err((anyhow!("MFT asincrono sin IMFMediaEventGenerator: {e}"), guard));
                }
            }
        } else {
            None
        };

        let out_type = match Self::build_h264_type(width, height, fps, bitrate_bps) {
            Ok(t) => t,
            Err(e) => return Err((e, guard)),
        };
        if let Err(e) = transform.SetOutputType(0, &out_type, 0) {
            return Err((anyhow!("SetOutputType H264 fallo: {e}"), guard));
        }

        let in_type = match Self::build_nv12_type(width, height, fps) {
            Ok(t) => t,
            Err(e) => return Err((e, guard)),
        };
        if let Err(e) = transform.SetInputType(0, &in_type, 0) {
            return Err((anyhow!("SetInputType NV12 fallo: {e}"), guard));
        }

        let stream_info: MFT_OUTPUT_STREAM_INFO = match transform.GetOutputStreamInfo(0) {
            Ok(info) => info,
            Err(e) => return Err((anyhow!("GetOutputStreamInfo fallo: {e}"), guard)),
        };
        let out_provides_samples =
            (stream_info.dwFlags & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32)) != 0;
        let out_buf_size = stream_info.cbSize.max(width * height);

        let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
        let _ = transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);

        let nv12_len = (width as usize) * (height as usize) * 3 / 2;
        Ok(Self {
            transform,
            event_gen,
            is_async,
            width,
            height,
            fps,
            bitrate_bps,
            nv12_buf: vec![0u8; nv12_len],
            frame_idx: 0,
            pending_input_tokens: 0,
            out_stream_id: 0,
            out_provides_samples,
            out_buf_size,
            _guard: guard,
        })
    }

    unsafe fn build_h264_type(
        width: u32,
        height: u32,
        fps: u8,
        bitrate_bps: u32,
    ) -> Result<IMFMediaType> {
        let mt = MFCreateMediaType().map_err(|e| anyhow!("MFCreateMediaType out: {e}"))?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
        mt.SetUINT32(&MF_MT_AVG_BITRATE, bitrate_bps)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | (height as u64))?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        mt.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
        Ok(mt)
    }

    unsafe fn build_nv12_type(width: u32, height: u32, fps: u8) -> Result<IMFMediaType> {
        let mt = MFCreateMediaType().map_err(|e| anyhow!("MFCreateMediaType in: {e}"))?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | (height as u64))?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, ((fps as u64) << 32) | 1)?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        Ok(mt)
    }

    pub fn matches_params(&self, width: u32, height: u32, fps: u8, bitrate_bps: u32) -> bool {
        self.width == width
            && self.height == height
            && self.fps == fps
            && self.bitrate_bps.abs_diff(bitrate_bps) * 4 <= self.bitrate_bps.max(1)
    }

    /// Codifica un cuadro BGRA tight (`width * height * 4`) en H.264 Annex-B y reporta si es IDR.
    pub fn encode_bgra(&mut self, bgra: &[u8]) -> Result<Option<(Vec<u8>, bool)>> {
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if bgra.len() < expected {
            return Err(anyhow!("buffer BGRA insuficiente para MFT"));
        }
        bgra_to_nv12(
            &bgra[..expected],
            self.width as usize,
            self.height as usize,
            &mut self.nv12_buf,
        );

        let dur_hns = 10_000_000i64 / (self.fps.max(1) as i64);
        let pts_hns = (self.frame_idx as i64) * dur_hns;
        self.frame_idx += 1;

        unsafe {
            let sample = self.create_input_sample(&self.nv12_buf, pts_hns, dur_hns)?;
            if self.is_async {
                self.encode_async(&sample)
            } else {
                self.encode_sync(&sample)
            }
        }
    }

    unsafe fn create_input_sample(&self, nv12: &[u8], pts: i64, dur: i64) -> Result<IMFSample> {
        let buffer: IMFMediaBuffer = MFCreateMemoryBuffer(nv12.len() as u32)?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        buffer.Lock(&mut ptr, None, None)?;
        std::ptr::copy_nonoverlapping(nv12.as_ptr(), ptr, nv12.len());
        let _ = buffer.Unlock();
        buffer.SetCurrentLength(nv12.len() as u32)?;

        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime(pts)?;
        sample.SetSampleDuration(dur)?;
        Ok(sample)
    }

    unsafe fn encode_sync(&self, sample: &IMFSample) -> Result<Option<(Vec<u8>, bool)>> {
        self.transform
            .ProcessInput(0, sample, 0)
            .map_err(|e| anyhow!("MFT ProcessInput fallo: {e}"))?;
        let mut out_bytes = Vec::new();
        for _ in 0..8 {
            match self.pull_output(&mut out_bytes)? {
                PullState::NeedMoreInput => break,
                PullState::StreamChanged => continue,
                PullState::GotSample => {}
            }
        }
        if out_bytes.is_empty() {
            Ok(None)
        } else {
            let idr = is_annex_b_idr(&out_bytes);
            Ok(Some((out_bytes, idr)))
        }
    }

    unsafe fn encode_async(&mut self, sample: &IMFSample) -> Result<Option<(Vec<u8>, bool)>> {
        let Some(gen) = self.event_gen.as_ref().cloned() else {
            return Err(anyhow!("MFT asincrono sin event_gen"));
        };
        let mut input_sent = false;
        if self.pending_input_tokens > 0 {
            self.transform
                .ProcessInput(0, sample, 0)
                .map_err(|e| anyhow!("MFT async ProcessInput fallo: {e}"))?;
            self.pending_input_tokens -= 1;
            input_sent = true;
        }

        let mut out_bytes = Vec::new();
        for _ in 0..32 {
            let event = gen
                .GetEvent(MF_EVENT_FLAG_NONE)
                .map_err(|e| anyhow!("MFT GetEvent fallo: {e}"))?;
            let ev_type = event.GetType().map_err(|e| anyhow!("GetType fallo: {e}"))?;
            if ev_type == METransformNeedInput.0 as u32 {
                if !input_sent {
                    self.transform
                        .ProcessInput(0, sample, 0)
                        .map_err(|e| anyhow!("MFT async ProcessInput fallo: {e}"))?;
                    input_sent = true;
                } else {
                    self.pending_input_tokens += 1;
                    break;
                }
            } else if ev_type == METransformHaveOutput.0 as u32 {
                match self.pull_output(&mut out_bytes)? {
                    PullState::GotSample | PullState::StreamChanged => {
                        if input_sent && !out_bytes.is_empty() {
                            break;
                        }
                    }
                    PullState::NeedMoreInput => break,
                }
            }
        }
        if out_bytes.is_empty() {
            Ok(None)
        } else {
            let idr = is_annex_b_idr(&out_bytes);
            Ok(Some((out_bytes, idr)))
        }
    }

    unsafe fn pull_output(&self, out_bytes: &mut Vec<u8>) -> Result<PullState> {
        let provided_sample = if self.out_provides_samples {
            None
        } else {
            let s = MFCreateSample()?;
            let b = MFCreateMemoryBuffer(self.out_buf_size)?;
            s.AddBuffer(&b)?;
            Some(s)
        };

        let mut out_buf = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: self.out_stream_id,
            pSample: std::mem::ManuallyDrop::new(provided_sample),
            dwStatus: 0,
            pEvents: std::mem::ManuallyDrop::new(None),
        };
        let mut status = 0u32;
        let res = self
            .transform
            .ProcessOutput(0, std::slice::from_mut(&mut out_buf), &mut status);

        let taken_sample = std::mem::ManuallyDrop::take(&mut out_buf.pSample);
        let _taken_events = std::mem::ManuallyDrop::take(&mut out_buf.pEvents);

        match res {
            Ok(()) => {
                if let Some(sample) = taken_sample {
                    if let Ok(buf) = sample.ConvertToContiguousBuffer() {
                        let mut ptr: *mut u8 = std::ptr::null_mut();
                        let mut cur_len = 0u32;
                        if buf.Lock(&mut ptr, None, Some(&mut cur_len)).is_ok() && cur_len > 0 {
                            let slice = std::slice::from_raw_parts(ptr, cur_len as usize);
                            out_bytes.extend_from_slice(slice);
                            let _ = buf.Unlock();
                        }
                    }
                }
                Ok(PullState::GotSample)
            }
            Err(err) if err.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => Ok(PullState::NeedMoreInput),
            Err(err) if err.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                warn!("MFT H.264 solicito cambio de formato de salida");
                if let Ok(new_type) = self.transform.GetOutputAvailableType(0, 0) {
                    let _ = self.transform.SetOutputType(0, &new_type, 0);
                }
                Ok(PullState::StreamChanged)
            }
            Err(err) => Err(anyhow!("MFT ProcessOutput fallo: {err}")),
        }
    }
}

impl Drop for MfH264Encoder {
    fn drop(&mut self) {
        unsafe {
            let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
        }
    }
}

enum PullState {
    GotSample,
    NeedMoreInput,
    StreamChanged,
}

/// Convierte un framebuffer BGRA (`width * height * 4`) a plano NV12 (`Y` + `UV` intercalado, BT.601).
pub fn bgra_to_nv12(bgra: &[u8], width: usize, height: usize, nv12_out: &mut [u8]) {
    let y_plane_len = width * height;
    let uv_plane_len = y_plane_len / 2;
    debug_assert!(bgra.len() >= y_plane_len * 4);
    debug_assert!(nv12_out.len() >= y_plane_len + uv_plane_len);

    let (y_plane, uv_plane) = nv12_out.split_at_mut(y_plane_len);
    for row in 0..height {
        let src_row = &bgra[row * width * 4..(row + 1) * width * 4];
        let y_row = &mut y_plane[row * width..(row + 1) * width];
        for col in 0..width {
            let px = &src_row[col * 4..col * 4 + 4];
            let b = px[0] as i32;
            let g = px[1] as i32;
            let r = px[2] as i32;
            let y = ((66 * r + 129 * g + 25 * b + 128) >> 8) + 16;
            y_row[col] = y.clamp(0, 255) as u8;
        }
    }

    for row_pair in 0..(height / 2) {
        let row0 = &bgra[(row_pair * 2) * width * 4..(row_pair * 2 + 1) * width * 4];
        let row1 = &bgra[(row_pair * 2 + 1) * width * 4..(row_pair * 2 + 2) * width * 4];
        let uv_row = &mut uv_plane[row_pair * width..(row_pair + 1) * width];
        for col_pair in 0..(width / 2) {
            let i0 = col_pair * 8;
            let b = (row0[i0] as i32 + row0[i0 + 4] as i32 + row1[i0] as i32 + row1[i0 + 4] as i32)
                >> 2;
            let g = (row0[i0 + 1] as i32
                + row0[i0 + 5] as i32
                + row1[i0 + 1] as i32
                + row1[i0 + 5] as i32)
                >> 2;
            let r = (row0[i0 + 2] as i32
                + row0[i0 + 6] as i32
                + row1[i0 + 2] as i32
                + row1[i0 + 6] as i32)
                >> 2;
            let u = ((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128;
            let v = ((112 * r - 94 * g - 18 * b + 128) >> 8) + 128;
            uv_row[col_pair * 2] = u.clamp(0, 255) as u8;
            uv_row[col_pair * 2 + 1] = v.clamp(0, 255) as u8;
        }
    }
}

/// Detecta si un bitstream H.264 Annex-B contiene al menos un NAL IDR (tipo 5) o SPS (tipo 7).
pub fn is_annex_b_idr(bitstream: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 3 < bitstream.len() {
        let start_len = if bitstream[i] == 0 && bitstream[i + 1] == 0 && bitstream[i + 2] == 1 {
            3
        } else if i + 4 < bitstream.len()
            && bitstream[i] == 0
            && bitstream[i + 1] == 0
            && bitstream[i + 2] == 0
            && bitstream[i + 3] == 1
        {
            4
        } else {
            i += 1;
            continue;
        };
        let nal_hdr = bitstream[i + start_len];
        let nal_type = nal_hdr & 0x1F;
        if nal_type == 5 || nal_type == 7 {
            return true;
        }
        i += start_len + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_to_nv12_converts_uniform_frame_correctly() {
        // Azul puro en BGRA = (255, 0, 0, 255) -> Y≈41, U≈240, V≈110
        let w = 4usize;
        let h = 4usize;
        let mut bgra = Vec::with_capacity(w * h * 4);
        for _ in 0..(w * h) {
            bgra.extend_from_slice(&[255, 0, 0, 255]);
        }
        let mut nv12 = vec![0u8; w * h * 3 / 2];
        bgra_to_nv12(&bgra, w, h, &mut nv12);
        assert!(nv12[..w * h].iter().all(|&y| (38..=44).contains(&y)));
        for pair in nv12[w * h..].as_chunks::<2>().0 {
            assert!(pair[0] > 230, "U esperado alto para azul, fue {}", pair[0]);
            assert!((105..=115).contains(&pair[1]), "V fue {}", pair[1]);
        }
    }

    #[test]
    fn annex_b_idr_detection_works() {
        assert!(is_annex_b_idr(&[0, 0, 0, 1, 0x67, 0x42, 0, 0, 1, 0x65]));
        assert!(is_annex_b_idr(&[0, 0, 1, 0x65, 0x88]));
        assert!(!is_annex_b_idr(&[0, 0, 0, 1, 0x41, 0x9A]));
    }
}
