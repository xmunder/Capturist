#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use std::path::PathBuf;

pub fn resolve_ffmpeg_bin() -> PathBuf {
    if let Ok(explicit) = std::env::var("CAPTURIST_FFMPEG_BIN") {
        let explicit = PathBuf::from(explicit);
        if explicit.exists() {
            return explicit;
        }
    }

    if let Ok(ffmpeg_dir) = std::env::var("FFMPEG_DIR") {
        let dir = PathBuf::from(ffmpeg_dir);
        let candidate_a = dir.join("bin").join("ffmpeg.exe");
        if candidate_a.exists() {
            return candidate_a;
        }

        let candidate_b = dir.join("ffmpeg.exe");
        if candidate_b.exists() {
            return candidate_b;
        }
    }

    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ffmpeg.exe"));
            candidates.push(dir.join("resources").join("ffmpeg.exe"));
            candidates.push(dir.join("ffmpeg-windows").join("bin").join("ffmpeg.exe"));
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("ffmpeg.exe"));
        candidates.push(cwd.join("ffmpeg-windows").join("bin").join("ffmpeg.exe"));
        candidates.push(
            cwd.join("..")
                .join("ffmpeg-windows")
                .join("bin")
                .join("ffmpeg.exe"),
        );
        candidates.push(
            cwd.join("..")
                .join("..")
                .join("ffmpeg-windows")
                .join("bin")
                .join("ffmpeg.exe"),
        );
    }

    for candidate in candidates {
        if candidate.exists() {
            return candidate;
        }
    }

    PathBuf::from("ffmpeg")
}

pub fn resolve_ffmpeg_dir() -> Option<PathBuf> {
    let bin = resolve_ffmpeg_bin();
    let parent = bin.parent()?.to_path_buf();
    if parent.as_os_str().is_empty() {
        None
    } else {
        Some(parent)
    }
}
