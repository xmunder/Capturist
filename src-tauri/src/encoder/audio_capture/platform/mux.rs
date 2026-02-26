use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::encoder::{
    config::OutputFormat, ffmpeg_paths::resolve_ffmpeg_bin, output_paths::move_temp_to_final,
};

use super::{dsp::build_mix_filter, dsp::build_single_track_filter, AudioTrackInput};

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
    video_path: &Path,
    final_output_path: &Path,
    audio_tracks: &[AudioTrackInput],
    microphone_gain_percent: u16,
) -> Result<(), String> {
    let ffmpeg_bin = resolve_ffmpeg_bin();
    let original_output = video_path.to_path_buf();
    let temp_video = make_video_only_path(&original_output);

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
        let track = &audio_tracks[0];
        cmd.arg("-i").arg(&track.path);
        let filter = build_single_track_filter(track, microphone_gain_percent);
        cmd.arg("-af")
            .arg(filter)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("1:a:0");
    } else {
        for track in audio_tracks {
            cmd.arg("-i").arg(&track.path);
        }

        let filter_graph = build_mix_filter(audio_tracks, microphone_gain_percent);
        cmd.arg("-filter_complex")
            .arg(filter_graph)
            .arg("-map")
            .arg("0:v:0")
            .arg("-map")
            .arg("[aout]");
    }

    cmd.arg("-c:v").arg("copy");

    match format {
        OutputFormat::WebM => {
            cmd.arg("-c:a").arg("libopus").arg("-b:a").arg("128k");
        }
        OutputFormat::Mp4 => {
            cmd.arg("-c:a")
                .arg("aac")
                .arg("-b:a")
                .arg("192k")
                .arg("-movflags")
                .arg("+faststart");
        }
        OutputFormat::Mkv => {
            cmd.arg("-c:a").arg("aac").arg("-b:a").arg("192k");
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
