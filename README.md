# Capturist

Aplicación de grabación de pantalla para Windows construida con Tauri + React + Rust.

## Estado actual

Actualmente el proyecto incluye:

- Captura de pantalla con `windows-capture` (monitores y ventanas).
- Encoder de video con `ffmpeg-the-third`.
- Captura de audio WASAPI (sistema y micrófono) + mux final con FFmpeg CLI.
- Selección de región nativa en Windows (`select_region_native`).
- Atajos globales con emisión de evento al frontend (`global-shortcut-triggered`).
- Frontend completo del grabador (controles, estado, settings, modal avanzado).
- Pipeline único de build/deploy en `scripts/build-and-deploy.sh`.

Documento de contrato:

- [docs/contract-matrix.md](docs/contract-matrix.md)

## Arquitectura

- UI (React): componentes, hooks y estado de grabación.
- Bridge Tauri: comandos invocables desde frontend.
- Aplicación/captura: manager de sesión con estados `idle/running/paused/stopped`.
- Infraestructura:
  - `ScreenProvider` con `windows-capture`.
  - `FrameConsumer` con `ffmpeg-the-third`.
  - audio WASAPI + mux final con FFmpeg.

## Requisitos

- Node.js + pnpm
- Rust toolchain
- Tauri CLI
- `cargo-xwin` (cross-build Windows desde Linux/WSL)
- `makensis` (solo si el modo de paquete incluye instalador)
- Artefactos FFmpeg en `ffmpeg-windows/`:
  - `include/`
  - `lib/*.lib`
  - `bin/*.dll` y `bin/ffmpeg.exe`

## Desarrollo local

```bash
pnpm install
pnpm tauri dev
```

## Puertas de calidad

```bash
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --all-targets --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
cargo xwin check --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc
pnpm run build
```

## Build Windows

### Build manual (rápido)

```bash
cargo xwin check --target x86_64-pc-windows-msvc --manifest-path src-tauri/Cargo.toml
pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

### Build/deploy recomendado (script único)

Usar:

```bash
scripts/build-and-deploy.sh
```

Variables importantes:

- `PACKAGE_MODE=installer|portable|both` (default: `installer`)
- `TARGET` (default: `x86_64-pc-windows-msvc`)
- `FFMPEG_DIR` (default: `./ffmpeg-windows`)
- `ARTIFACTS_DIR` (default: `./artifacts`)
- `RUN_XWIN_CHECK=1|0`
- `RUN_FRONTEND_BUILD=1|0`

Ejemplos:

```bash
PACKAGE_MODE=portable scripts/build-and-deploy.sh
PACKAGE_MODE=both TARGET=x86_64-pc-windows-msvc scripts/build-and-deploy.sh
```

Salida esperada:

- `artifacts/<timestamp>/installer/` (si aplica)
- `artifacts/<timestamp>/portable/<app>/` (si aplica)

## Notas operativas

- Plataforma objetivo: Windows (x86_64).
- Atajos globales nativos: disponibles en Windows.
- En Linux/WSL el comando `set_global_shortcuts` devuelve error esperado de plataforma.

## Pendientes de cierre

- Validación manual completa en Windows 10/11 (con y sin GPU).
- Matriz final de encoder en hardware real: NVENC, AMF, QSV, CPU fallback.
- Más cobertura de tests en módulos de audio/region/shortcuts.
