#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::{Builder as TempBuilder, TempDir};

use crate::encoder::ffmpeg_paths::resolve_ffmpeg_dir;

pub struct PreparedOutputPaths {
    pub temp_dir: TempDir,
    pub temp_output_path: PathBuf,
}

pub fn prepare_output_paths(final_output_path: PathBuf) -> Result<PreparedOutputPaths, String> {
    let file_name = final_output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("recording.mp4");

    let mut temp_dir = None;
    if let Some(ffmpeg_dir) = resolve_ffmpeg_dir() {
        let base = ffmpeg_dir.join("capturist-temp");
        if fs::create_dir_all(&base).is_ok() {
            if let Ok(dir) = TempBuilder::new().prefix("session-").tempdir_in(&base) {
                temp_dir = Some(dir);
            }
        }
    }

    let temp_dir = match temp_dir {
        Some(value) => value,
        None => TempBuilder::new()
            .prefix("capturist-temp-")
            .tempdir()
            .map_err(|err| format!("No se pudo crear carpeta temporal para grabación: {err}"))?,
    };

    let temp_output_path = temp_dir.path().join(file_name);

    Ok(PreparedOutputPaths {
        temp_dir,
        temp_output_path,
    })
}

pub fn move_temp_to_final(temp_path: &Path, final_path: &Path) -> Result<(), String> {
    if !temp_path.exists() {
        return Err(format!(
            "No existe el archivo temporal para mover: {}",
            temp_path.display()
        ));
    }

    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "No se pudo crear carpeta de salida '{}': {err}",
                parent.display()
            )
        })?;
    }

    if final_path.exists() {
        let _ = fs::remove_file(final_path);
    }

    if fs::rename(temp_path, final_path).is_ok() {
        return Ok(());
    }

    fs::copy(temp_path, final_path)
        .map_err(|err| format!("No se pudo copiar archivo final: {err}"))?;

    if let Err(err) = fs::remove_file(temp_path) {
        eprintln!(
            "[output] No se pudo limpiar temporal '{}': {}",
            temp_path.display(),
            err
        );
    }

    Ok(())
}
