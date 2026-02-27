use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::encoder::{
    config::{OutputFormat, QualityMode},
    ffmpeg_paths::resolve_ffmpeg_bin,
    output_paths::move_temp_to_final,
};
use ffmpeg_the_third::{ffi, format as ffmpeg_format, media};

use super::{
    dsp::build_mix_filter, dsp::build_single_track_filter, AudioTrackInput, AudioTrackSource,
};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const WAV_HEADER_BYTES: u64 = 44;

pub(super) fn audio_file_has_payload(path: &Path) -> bool {
    fs::metadata(path)
        .map(|m| m.is_file() && m.len() > WAV_HEADER_BYTES)
        .unwrap_or(false)
}

pub(super) fn mux_audio_into_video(
    format: &OutputFormat,
    quality_mode: &QualityMode,
    video_path: &Path,
    final_output_path: &Path,
    audio_tracks: &[AudioTrackInput],
    microphone_gain_percent: u16,
) -> Result<(), String> {
    let ffmpeg_bin = resolve_ffmpeg_bin();
    let original_output = video_path.to_path_buf();
    let temp_video = make_video_only_path(&original_output);
    let output_audio_delay_ms =
        detect_video_start_delay_ms(video_path).saturating_add(read_audio_sync_offset_ms());

    if !original_output.exists() {
        return Err(format!(
            "No existe el video base para mezclar audio: {}",
            original_output.display()
        ));
    }

    if temp_video.exists() {
        let _ = fs::remove_file(&temp_video);
    }

    fs::rename(&original_output, &temp_video)
        .map_err(|e| format!("No se pudo preparar el video para mux de audio: {}", e))?;

    let mut cmd = Command::new(&ffmpeg_bin);
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-threads")
        .arg("0")
        .arg("-i")
        .arg(&temp_video);

    if audio_tracks.len() == 1 {
        let adjusted_track = with_added_delay(&audio_tracks[0], output_audio_delay_ms);
        cmd.arg("-i").arg(&adjusted_track.path);
        if should_bypass_single_track_filter(&adjusted_track, microphone_gain_percent, quality_mode)
        {
            cmd.arg("-map").arg("0:v:0").arg("-map").arg("1:a:0");
        } else {
            if let Some(filter) =
                build_single_track_filter(&adjusted_track, microphone_gain_percent, quality_mode)
            {
                cmd.arg("-af").arg(filter);
            }
            cmd.arg("-map").arg("0:v:0").arg("-map").arg("1:a:0");
        }
    } else {
        let adjusted_tracks: Vec<AudioTrackInput> = audio_tracks
            .iter()
            .map(|track| with_added_delay(track, output_audio_delay_ms))
            .collect();

        for track in audio_tracks {
            cmd.arg("-i").arg(&track.path);
        }

        let filter_graph =
            build_mix_filter(&adjusted_tracks, microphone_gain_percent, quality_mode);
        cmd.arg("-filter_complex")
            .arg(filter_graph)
            .arg("-filter_threads")
            .arg("0")
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("[aout]");
    }

    cmd.arg("-c:v").arg("copy").arg("-shortest");

    match format {
        OutputFormat::WebM => {
            cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("128k");
        }
        OutputFormat::Mp4 => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("160k");
            if should_enable_mp4_faststart() {
                cmd.arg("-movflags").arg("+faststart");
            }
        }
        OutputFormat::Mkv => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("160k");
        }
    }

    cmd.arg(&final_output_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let output = cmd.output().map_err(|e| {
        restore_video_only_file(&temp_video, &original_output);
        let _ = move_temp_to_final(&original_output, final_output_path);
        if e.kind() == io::ErrorKind::NotFound {
            "No se encontró FFmpeg CLI para mux de audio. Define CAPTURIST_FFMPEG_BIN o agrega ffmpeg.exe al PATH."
                .to_string()
        } else {
            format!("No se pudo ejecutar FFmpeg para mux de audio: {}", e)
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        restore_video_only_file(&temp_video, &original_output);
        let _ = move_temp_to_final(&original_output, final_output_path);
        return Err(format!(
            "FFmpeg falló al combinar video+audio: {}",
            if stderr.is_empty() {
                "sin salida de error".to_string()
            } else {
                stderr
            }
        ));
    }

    let _ = fs::remove_file(&temp_video);
    Ok(())
}

fn make_video_only_path(output_path: &Path) -> PathBuf {
    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("recording");
    let ext = output_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("mp4");
    output_path.with_file_name(format!("{stem}.video_only.{ext}"))
}

fn restore_video_only_file(video_only: &Path, target_output: &Path) {
    if target_output.exists() {
        let _ = fs::remove_file(target_output);
    }
    let _ = fs::rename(video_only, target_output);
}

fn should_enable_mp4_faststart() -> bool {
    match env::var("CAPTURIST_MP4_FASTSTART") {
        Ok(value) => {
            let normalized = value.trim().to_ascii_lowercase();
            normalized == "1" || normalized == "true" || normalized == "yes"
        }
        Err(_) => false,
    }
}

fn read_audio_sync_offset_ms() -> u64 {
    match env::var("CAPTURIST_AUDIO_SYNC_OFFSET_MS") {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .ok()
            .map(|parsed| parsed.min(1_000))
            .unwrap_or(0),
        Err(_) => 0,
    }
}

fn with_added_delay(track: &AudioTrackInput, extra_delay_ms: u64) -> AudioTrackInput {
    AudioTrackInput {
        path: track.path.clone(),
        delay_ms: track.delay_ms.saturating_add(extra_delay_ms),
        source: track.source,
    }
}

fn detect_video_start_delay_ms(video_path: &Path) -> u64 {
    let Some(path) = video_path.to_str() else {
        return 0;
    };

    let _ = ffmpeg_the_third::init();
    let Ok(mut input_ctx) = ffmpeg_format::input(path) else {
        return 0;
    };
    let Some(video_stream) = input_ctx.streams().best(media::Type::Video) else {
        return 0;
    };

    let stream_index = video_stream.index();
    let time_base = video_stream.time_base();
    if let Some(start_ms) = timestamp_to_ms(video_stream.start_time(), time_base) {
        return start_ms.min(1_000);
    }

    const MAX_PACKETS_TO_PROBE: usize = 512;
    for packet_result in input_ctx.packets().take(MAX_PACKETS_TO_PROBE) {
        let Ok((stream, packet)) = packet_result else {
            continue;
        };
        if stream.index() != stream_index {
            continue;
        }

        if let Some(ts) = packet.dts().or_else(|| packet.pts()) {
            if let Some(start_ms) = timestamp_to_ms(ts, time_base) {
                return start_ms.min(1_000);
            }
        }
    }

    0
}

fn timestamp_to_ms(timestamp: i64, time_base: ffmpeg_the_third::Rational) -> Option<u64> {
    if timestamp <= 0 || timestamp == ffi::AV_NOPTS_VALUE {
        return None;
    }

    let den = i128::from(time_base.denominator());
    let num = i128::from(time_base.numerator());
    if den <= 0 || num <= 0 {
        return None;
    }

    let ts_ms = (i128::from(timestamp) * num * 1_000) / den;
    if ts_ms <= 0 {
        None
    } else {
        Some(u64::try_from(ts_ms).unwrap_or(0))
    }
}

fn should_bypass_single_track_filter(
    track: &AudioTrackInput,
    microphone_gain_percent: u16,
    quality_mode: &QualityMode,
) -> bool {
    if track.source != AudioTrackSource::System {
        return false;
    }

    if track.delay_ms > 0 {
        return false;
    }

    if microphone_gain_percent != 100 {
        return false;
    }

    matches!(
        quality_mode,
        QualityMode::Performance | QualityMode::Balanced
    )
}

#[cfg(test)]
mod tests {
    use super::{
        should_bypass_single_track_filter, AudioTrackInput, AudioTrackSource, QualityMode,
    };
    use std::path::PathBuf;

    fn system_track(delay_ms: u64) -> AudioTrackInput {
        AudioTrackInput {
            path: PathBuf::from("system.wav"),
            delay_ms,
            source: AudioTrackSource::System,
        }
    }

    #[test]
    fn bypass_single_track_filter_para_sistema_sin_delay_en_modos_rapidos() {
        let track = system_track(0);
        assert!(should_bypass_single_track_filter(
            &track,
            100,
            &QualityMode::Performance
        ));
        assert!(should_bypass_single_track_filter(
            &track,
            100,
            &QualityMode::Balanced
        ));
    }

    #[test]
    fn no_bypass_single_track_filter_con_delay_o_modo_quality() {
        let delayed = system_track(120);
        assert!(!should_bypass_single_track_filter(
            &delayed,
            100,
            &QualityMode::Balanced
        ));

        let no_delay = system_track(0);
        assert!(!should_bypass_single_track_filter(
            &no_delay,
            100,
            &QualityMode::Quality
        ));
    }
}
