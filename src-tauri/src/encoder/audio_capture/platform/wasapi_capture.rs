use std::{
    fs::File,
    io::{self, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::RPC_E_CHANGED_MODE,
        Media::Audio::{
            IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
            WAVEFORMATEX,
        },
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_MULTITHREADED,
        },
    },
};

use super::device_discovery::{to_utf16_null, DeviceDescriptor};

const FIRST_ENABLE_UNSET: u64 = u64::MAX;

pub(super) struct ActiveCapture {
    pub(super) kind: &'static str,
    pub(super) wav_path: PathBuf,
    pub(super) device_name: String,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) enabled: Arc<AtomicBool>,
    pub(super) ever_enabled: Arc<AtomicBool>,
    pub(super) first_enabled_at_ms: Arc<AtomicU64>,
    pub(super) handle: Option<JoinHandle<Result<(), String>>>,
}

pub(super) fn normalized_track_delay(raw_delay: u64) -> u64 {
    if raw_delay == FIRST_ENABLE_UNSET {
        0
    } else {
        raw_delay
    }
}

pub(super) fn stop_capture_worker(worker: &mut Option<ActiveCapture>, errors: &mut Vec<String>) {
    if let Some(active) = worker.as_mut() {
        active.stop.store(true, Ordering::SeqCst);

        if let Some(handle) = active.handle.take() {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => errors.push(err),
                Err(_) => errors.push(format!(
                    "El hilo de {} finalizó inesperadamente.",
                    active.kind
                )),
            }
        }
    }
}

pub(super) fn spawn_capture_worker(
    kind: &'static str,
    wav_path: PathBuf,
    device: DeviceDescriptor,
    loopback: bool,
    initial_enabled: bool,
    recording_started_at: Instant,
) -> Result<ActiveCapture, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let enabled = Arc::new(AtomicBool::new(initial_enabled));
    let ever_enabled = Arc::new(AtomicBool::new(initial_enabled));
    let first_enabled_at_ms = Arc::new(AtomicU64::new(if initial_enabled {
        0
    } else {
        FIRST_ENABLE_UNSET
    }));

    let stop_clone = Arc::clone(&stop);
    let enabled_clone = Arc::clone(&enabled);
    let ever_enabled_clone = Arc::clone(&ever_enabled);
    let first_enabled_at_ms_clone = Arc::clone(&first_enabled_at_ms);
    let id = device.id.clone();
    let name = device.name.clone();
    let name_for_error = name.clone();
    let worker_path = wav_path.clone();

    let thread_name = if loopback {
        "capturist-audio-system"
    } else {
        "capturist-audio-mic"
    };

    let handle = thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            capture_device_loop(
                &id,
                &worker_path,
                stop_clone,
                enabled_clone,
                ever_enabled_clone,
                first_enabled_at_ms_clone,
                recording_started_at,
                loopback,
            )
        })
        .map_err(|e| {
            format!(
                "No se pudo iniciar captura WASAPI para {} ({}): {}",
                kind, name_for_error, e
            )
        })?;

    Ok(ActiveCapture {
        kind,
        wav_path,
        device_name: name,
        stop,
        enabled,
        ever_enabled,
        first_enabled_at_ms,
        handle: Some(handle),
    })
}

fn capture_device_loop(
    device_id: &str,
    wav_path: &Path,
    stop: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    ever_enabled: Arc<AtomicBool>,
    first_enabled_at_ms: Arc<AtomicU64>,
    recording_started_at: Instant,
    loopback: bool,
) -> Result<(), String> {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let should_uninitialize = hr.is_ok();
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        return Err(format!(
            "No se pudo inicializar COM para captura de audio: 0x{:08X}",
            hr.0 as u32
        ));
    }

    let result = (|| -> Result<(), String> {
        let enumerator = create_device_enumerator()?;
        let device_id_utf16 = to_utf16_null(device_id);
        let device = unsafe {
            enumerator
                .GetDevice(PCWSTR(device_id_utf16.as_ptr()))
                .map_err(|e| format!("No se pudo abrir el endpoint de audio WASAPI: {}", e))?
        };

        let audio_client: IAudioClient = unsafe {
            device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| format!("No se pudo activar IAudioClient en WASAPI: {}", e))?
        };

        let mix_format_ptr = unsafe {
            audio_client
                .GetMixFormat()
                .map_err(|e| format!("No se pudo obtener el formato de mezcla de WASAPI: {}", e))?
        };

        let format_guard = CoTaskMemPtr(mix_format_ptr as *mut _);
        let (format_blob, block_align) = parse_wave_format_blob(mix_format_ptr)?;

        let mut stream_flags = 0u32;
        if loopback {
            stream_flags |= AUDCLNT_STREAMFLAGS_LOOPBACK;
        }

        unsafe {
            audio_client
                .Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    stream_flags,
                    10_000_000,
                    0,
                    mix_format_ptr,
                    None,
                )
                .map_err(|e| format!("No se pudo inicializar stream WASAPI: {}", e))?;
        }

        let capture_client: IAudioCaptureClient = unsafe {
            audio_client
                .GetService()
                .map_err(|e| format!("No se pudo inicializar IAudioCaptureClient: {}", e))?
        };

        let mut writer = WavFileWriter::create(wav_path, &format_blob)
            .map_err(|e| format!("No se pudo abrir archivo temporal WAV: {}", e))?;

        unsafe {
            audio_client
                .Start()
                .map_err(|e| format!("No se pudo iniciar stream WASAPI: {}", e))?;
        }

        while !stop.load(Ordering::Relaxed) {
            let mut frames_in_packet = unsafe {
                capture_client
                    .GetNextPacketSize()
                    .map_err(|e| format!("Error leyendo tamaño de paquete de audio: {}", e))?
            };

            if frames_in_packet == 0 {
                thread::sleep(Duration::from_millis(5));
                continue;
            }

            while frames_in_packet > 0 {
                let mut data_ptr = std::ptr::null_mut();
                let mut frame_count = 0u32;
                let mut flags = 0u32;

                unsafe {
                    capture_client
                        .GetBuffer(&mut data_ptr, &mut frame_count, &mut flags, None, None)
                        .map_err(|e| format!("Error obteniendo buffer de captura WASAPI: {}", e))?;
                }

                let bytes_to_write = (frame_count as usize).saturating_mul(block_align);
                let is_enabled = enabled.load(Ordering::Relaxed);
                if is_enabled {
                    let was_enabled_before = ever_enabled.swap(true, Ordering::SeqCst);
                    if !was_enabled_before {
                        let elapsed_ms = recording_started_at.elapsed().as_millis() as u64;
                        let _ = first_enabled_at_ms.compare_exchange(
                            FIRST_ENABLE_UNSET,
                            elapsed_ms,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        );
                    }
                }

                let started_track = ever_enabled.load(Ordering::Relaxed);
                let write_result = if bytes_to_write == 0 {
                    Ok(())
                } else if !started_track {
                    Ok(())
                } else if !is_enabled
                    || (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0
                    || data_ptr.is_null()
                {
                    writer.write_silence(bytes_to_write)
                } else {
                    let data = unsafe {
                        std::slice::from_raw_parts(data_ptr as *const u8, bytes_to_write)
                    };
                    writer.write_samples(data)
                };

                let release_result = unsafe { capture_client.ReleaseBuffer(frame_count) };
                if let Err(e) = release_result {
                    return Err(format!("Error liberando buffer de captura WASAPI: {}", e));
                }

                if let Err(e) = write_result {
                    return Err(format!("Error escribiendo audio temporal: {}", e));
                }

                frames_in_packet = unsafe {
                    capture_client.GetNextPacketSize().map_err(|e| {
                        format!("Error consultando siguiente paquete de audio: {}", e)
                    })?
                };
            }
        }

        let _ = unsafe { audio_client.Stop() };
        writer
            .finalize()
            .map_err(|e| format!("No se pudo cerrar archivo WAV temporal: {}", e))?;
        drop(format_guard);
        Ok(())
    })();

    if should_uninitialize {
        unsafe { CoUninitialize() };
    }

    result
}

fn create_device_enumerator() -> Result<IMMDeviceEnumerator, String> {
    unsafe {
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("No se pudo crear IMMDeviceEnumerator: {}", e))
    }
}

fn parse_wave_format_blob(format_ptr: *mut WAVEFORMATEX) -> Result<(Vec<u8>, usize), String> {
    if format_ptr.is_null() {
        return Err("WASAPI devolvió un formato de audio nulo.".to_string());
    }

    let base_len = std::mem::size_of::<WAVEFORMATEX>();
    let base_slice = unsafe { std::slice::from_raw_parts(format_ptr as *const u8, base_len) };

    let cb_size = u16::from_le_bytes([base_slice[16], base_slice[17]]) as usize;
    let block_align = u16::from_le_bytes([base_slice[12], base_slice[13]]) as usize;
    if block_align == 0 {
        return Err("Formato WASAPI inválido: block_align = 0.".to_string());
    }

    let total_len = base_len + cb_size;
    if total_len > 4096 {
        return Err(format!(
            "Formato WASAPI inválido: tamaño de estructura demasiado grande ({total_len})."
        ));
    }

    let full_blob = unsafe { std::slice::from_raw_parts(format_ptr as *const u8, total_len) };
    Ok((full_blob.to_vec(), block_align))
}

struct CoTaskMemPtr<T>(*mut T);

impl<T> Drop for CoTaskMemPtr<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CoTaskMemFree(Some(self.0 as _)) };
        }
    }
}

struct WavFileWriter {
    file: File,
    data_size_offset: u64,
    written_audio_bytes: u64,
}

impl WavFileWriter {
    fn create(path: &Path, format_blob: &[u8]) -> io::Result<Self> {
        let mut file = File::create(path)?;
        let fmt_size = format_blob.len() as u32;

        file.write_all(b"RIFF")?;
        file.write_all(&0u32.to_le_bytes())?;
        file.write_all(b"WAVE")?;

        file.write_all(b"fmt ")?;
        file.write_all(&fmt_size.to_le_bytes())?;
        file.write_all(format_blob)?;

        file.write_all(b"data")?;
        let data_size_offset = file.stream_position()?;
        file.write_all(&0u32.to_le_bytes())?;

        Ok(Self {
            file,
            data_size_offset,
            written_audio_bytes: 0,
        })
    }

    fn write_samples(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)?;
        self.written_audio_bytes = self.written_audio_bytes.saturating_add(data.len() as u64);
        Ok(())
    }

    fn write_silence(&mut self, len: usize) -> io::Result<()> {
        const CHUNK: usize = 4096;
        let zeros = [0u8; CHUNK];
        let mut remaining = len;
        while remaining > 0 {
            let write_now = remaining.min(CHUNK);
            self.file.write_all(&zeros[..write_now])?;
            self.written_audio_bytes = self.written_audio_bytes.saturating_add(write_now as u64);
            remaining -= write_now;
        }
        Ok(())
    }

    fn finalize(&mut self) -> io::Result<()> {
        let file_size = self.file.seek(SeekFrom::End(0))?;
        let riff_size = file_size.saturating_sub(8) as u32;
        let data_size = self.written_audio_bytes.min(u32::MAX as u64) as u32;

        self.file.seek(SeekFrom::Start(4))?;
        self.file.write_all(&riff_size.to_le_bytes())?;

        self.file.seek(SeekFrom::Start(self.data_size_offset))?;
        self.file.write_all(&data_size.to_le_bytes())?;

        self.file.flush()?;
        Ok(())
    }
}
