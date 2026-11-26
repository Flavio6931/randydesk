//! Reproducción de audio remoto de baja latencia mediante WASAPI (`48 kHz` estéreo).

use randydesk_proto::{decode_stereo_ima_adpcm, AudioCodec, AudioFrame};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioClient, IAudioRenderClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u16 = 2;

struct ComApartmentGuard {
    needs_uninit: bool,
}

impl ComApartmentGuard {
    fn enter() -> Self {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self {
            needs_uninit: hr.is_ok(),
        }
    }
}

impl Drop for ComApartmentGuard {
    fn drop(&mut self) {
        if self.needs_uninit {
            unsafe { CoUninitialize() };
        }
    }
}

/// Reproductor WASAPI independiente del hilo de UI (`Arc<AudioPlayer>` seguro entre hilos).
pub struct AudioPlayer {
    muted: Arc<AtomicBool>,
    volume: Arc<AtomicU8>,
    stop: Arc<AtomicBool>,
    tx: Mutex<Option<mpsc::SyncSender<Vec<i16>>>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Default for AudioPlayer {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPlayer {
    /// Inicia el hilo de salida de audio WASAPI en modo compartido.
    pub fn new() -> Self {
        let muted = Arc::new(AtomicBool::new(false));
        let volume = Arc::new(AtomicU8::new(100));
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel::<Vec<i16>>(16);

        let stop_w = Arc::clone(&stop);
        let muted_w = Arc::clone(&muted);
        let vol_w = Arc::clone(&volume);
        let worker = std::thread::Builder::new()
            .name("randydesk-audio-play".into())
            .spawn(move || {
                let _com = ComApartmentGuard::enter();
                run_render_loop(&stop_w, &muted_w, &vol_w, rx);
            })
            .ok();

        Self {
            muted,
            volume,
            stop,
            tx: Mutex::new(Some(tx)),
            worker: Mutex::new(worker),
        }
    }

    /// Silencia o reactiva la reproducción local de audio remoto.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    /// Ajusta el volumen de reproducción (`0..=100`).
    pub fn set_volume(&self, volume: u8) {
        self.volume.store(volume.min(100), Ordering::Relaxed);
    }

    /// Decodifica y encola un paquete de audio recibido desde el host sin bloquear la red.
    pub fn push_frame(&self, frame: &AudioFrame) {
        if self.muted.load(Ordering::Relaxed) || self.volume.load(Ordering::Relaxed) == 0 {
            return;
        }
        if frame.sample_rate != AUDIO_SAMPLE_RATE || frame.channels != 2 {
            return;
        }
        let samples = match frame.codec {
            AudioCodec::ImaAdpcm => decode_stereo_ima_adpcm(&frame.data),
        };
        if samples.is_empty() {
            return;
        }
        if let Ok(guard) = self.tx.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.try_send(samples);
            }
        }
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Ok(mut tx) = self.tx.lock() {
            *tx = None;
        }
        if let Ok(mut w) = self.worker.lock() {
            if let Some(handle) = w.take() {
                let _ = handle.join();
            }
        }
    }
}

fn run_render_loop(
    stop: &AtomicBool,
    muted: &AtomicBool,
    volume: &AtomicU8,
    rx: mpsc::Receiver<Vec<i16>>,
) {
    while !stop.load(Ordering::Relaxed) {
        if let Err(err) = run_wasapi_session(stop, muted, volume, &rx) {
            warn!("Salida WASAPI interrumpida ({err}); reintentando en 2 s");
            for _ in 0..20 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                while rx.try_recv().is_ok() {}
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn run_wasapi_session(
    stop: &AtomicBool,
    muted: &AtomicBool,
    volume: &AtomicU8,
    rx: &mpsc::Receiver<Vec<i16>>,
) -> anyhow::Result<()> {
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eConsole)? };
    let audio_client: IAudioClient = unsafe { device.Activate(CLSCTX_ALL, None)? };

    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: AUDIO_CHANNELS,
        nSamplesPerSec: AUDIO_SAMPLE_RATE,
        nAvgBytesPerSec: AUDIO_SAMPLE_RATE * (AUDIO_CHANNELS as u32) * 2,
        nBlockAlign: AUDIO_CHANNELS * 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };

    let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            1_200_000, // 120 ms de jitter buffer
            0,
            &format,
            None,
        )?;
    }

    let buffer_frames = unsafe { audio_client.GetBufferSize()? };
    let render_client: IAudioRenderClient = unsafe { audio_client.GetService()? };
    unsafe { audio_client.Start()? };
    info!("Salida WASAPI lista (48 kHz estéreo, buffer={buffer_frames} frames)");

    while !stop.load(Ordering::Relaxed) {
        let mut chunk = match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if muted.load(Ordering::Relaxed) {
            continue;
        }
        let vol = volume.load(Ordering::Relaxed).min(100) as i32;
        if vol == 0 {
            continue;
        }
        if vol < 100 {
            for s in &mut chunk {
                *s = ((*s as i32) * vol / 100) as i16;
            }
        }

        let mut frame_offset = 0usize;
        let total_frames = chunk.len() / 2;
        while frame_offset < total_frames && !stop.load(Ordering::Relaxed) {
            let padding = unsafe { audio_client.GetCurrentPadding()? };
            let avail = buffer_frames.saturating_sub(padding) as usize;
            if avail == 0 {
                std::thread::sleep(Duration::from_millis(4));
                continue;
            }
            let write_frames = (total_frames - frame_offset).min(avail);
            let dst_ptr = unsafe { render_client.GetBuffer(write_frames as u32)? };
            let sample_start = frame_offset * 2;
            let sample_count = write_frames * 2;
            let dst_slice = unsafe {
                std::slice::from_raw_parts_mut(dst_ptr as *mut i16, sample_count)
            };
            dst_slice.copy_from_slice(&chunk[sample_start..sample_start + sample_count]);
            unsafe { render_client.ReleaseBuffer(write_frames as u32, 0)? };
            frame_offset += write_frames;
        }
    }

    let _ = unsafe { audio_client.Stop() };
    Ok(())
}
