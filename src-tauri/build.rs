use std::{env, path::PathBuf};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let ffmpeg_dir = env::var("FFMPEG_DIR").unwrap_or_else(|_| {
            let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
            manifest_dir
                .parent()
                .map(|path| path.join("ffmpeg-windows"))
                .unwrap_or_else(|| manifest_dir.join("ffmpeg-windows"))
                .to_string_lossy()
                .to_string()
        });

        let ffmpeg_path = PathBuf::from(ffmpeg_dir);
        let include_dir = ffmpeg_path.join("include");
        let lib_dir = ffmpeg_path.join("lib");

        if !include_dir.exists() || !lib_dir.exists() {
            panic!(
                "No se encontraron artefactos de FFmpeg para Windows. Se esperaba include/ y lib/ en {}",
                ffmpeg_path.display()
            );
        }
    }

    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    println!("cargo:rerun-if-changed=build.rs");

    tauri_build::build();
}
