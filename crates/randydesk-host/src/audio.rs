//! Captura de audio de reproducción del sistema Windows en vivo mediante WASAPI Loopback.

use randydesk_proto::{encode_stereo_ima_adpcm, AudioCodec, AudioFrame, HostMsg};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_LOOPBACK,
    AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const AUDIO_SAMPLE_RATE: u32 = 48_000;
const AUDIO_CHANNELS: u16 = 2;
const FRAMES_PER_PACKET: usize = 960; // 20 ms a 48 kHz
const AUDCLNT_BUFFERFLAGS_SILENT_U32: u32 = 0x2;
const SILENCE_PEAK_THRESHOLD: i16 = 64;

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

/// Inicia un hilo dedicado que captura WASAPI Loopback (`48 kHz` estéreo) y envía bloques IMA-ADPCM.
pub fn spawn_audio_capture_worker(
    enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<HostMsg>,
) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("randydesk-audio-cap".into())
        .spawn(move || {
            let _com = ComApartmentGuard::enter();
            while !stop.load(Ordering::Relaxed) {
                if !enabled.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(80));
                    continue;
                }
                if let Err(err) = run_capture_session(&enabled, &stop, &tx) {
                    warn!("Captura WASAPI loopback interrumpida ({err}); reintentando en 2 s");
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        })
        .expect("spawn audio capture worker")
}

fn run_capture_session(
    enabled: &AtomicBool,
    stop: &AtomicBool,
    tx: &mpsc::Sender<HostMsg>,
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

    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
    unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            flags,
            1_000_000, // 100 ms en unidades de 100 ns
            0,
            &format,
            None,
        )?;
    }

    let capture_client: IAudioCaptureClient = unsafe { audio_client.GetService()? };
    unsafe { audio_client.Start()? };
    info!("Captura WASAPI Loopback iniciada (48 kHz estéreo IMA-ADPCM)");

    let mut accum: Vec<i16> = Vec::with_capacity(FRAMES_PER_PACKET * 4);
    let target_samples = FRAMES_PER_PACKET * (AUDIO_CHANNELS as usize);

    while !stop.load(Ordering::Relaxed) {
        if !enabled.load(Ordering::Relaxed) {
            accum.clear();
            let _ = unsafe { audio_client.Stop() };
            while !stop.load(Ordering::Relaxed) && !enabled.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(60));
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let _ = unsafe { audio_client.Start() };
        }

        let mut got_packet = false;
        loop {
            let next_frames = unsafe { capture_client.GetNextPacketSize()? };
            if next_frames == 0 {
                break;
            }
            got_packet = true;

            let mut data_ptr: *mut u8 = std::ptr::null_mut();
            let mut num_frames: u32 = 0;
            let mut buf_flags: u32 = 0;
            unsafe {
                capture_client.GetBuffer(
                    &mut data_ptr,
                    &mut num_frames,
                    &mut buf_flags,
                    None,
                    None,
                )?;
            }

            let silent = (buf_flags & AUDCLNT_BUFFERFLAGS_SILENT_U32) != 0 || data_ptr.is_null();
            if !silent && num_frames > 0 {
                let sample_count = (num_frames as usize) * (AUDIO_CHANNELS as usize);
                let slice =
                    unsafe { std::slice::from_raw_parts(data_ptr as *const i16, sample_count) };
                accum.extend_from_slice(slice);
            } else {
                accum.clear();
            }

            unsafe { capture_client.ReleaseBuffer(num_frames)? };

            while accum.len() >= target_samples {
                let block: Vec<i16> = accum.drain(..target_samples).collect();
                let peak = block.iter().map(|s| s.saturating_abs()).max().unwrap_or(0);
                if peak >= SILENCE_PEAK_THRESHOLD {
                    let encoded = encode_stereo_ima_adpcm(&block);
                    let _ = tx.try_send(HostMsg::AudioData(AudioFrame {
                        sample_rate: AUDIO_SAMPLE_RATE,
                        channels: AUDIO_CHANNELS as u8,
                        codec: AudioCodec::ImaAdpcm,
                        data: encoded,
                    }));
                }
            }
        }

        if !got_packet {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    let _ = unsafe { audio_client.Stop() };
    Ok(())
}
