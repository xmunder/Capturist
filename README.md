# Capturist

Aplicación de grabación de pantalla para Windows (Tauri + React + Rust).

## Estado del proyecto

Hito actual: **contrato frontend/backend congelado + capa de captura base migrada a windows-capture**.

Se dejó un baseline compatible con el proyecto de referencia en:
- comandos Tauri,
- modelos de request/response,
- semántica de estados de grabación.
- `ScreenProvider` conectado a `windows-capture` para descubrimiento real de monitores y ventanas en Windows.
- Loop de captura real en thread dedicado usando `windows-capture` (frames crudos en memoria).

Documento de contrato:
- [docs/contract-matrix.md](docs/contract-matrix.md)

## Setup

### Requisitos

- Node.js + pnpm
- Rust toolchain
- Tauri CLI

### Desarrollo

```bash
pnpm install
pnpm tauri dev
```

## Build (objetivo Windows)

Para cross-build desde Linux/WSL, usar `cargo-xwin`.

Comandos de referencia:

```bash
cargo xwin check --target x86_64-pc-windows-msvc
pnpm tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc
```

## Arquitectura base

- UI (React): capa de presentación y control.
- Bridge Tauri: comandos invocables desde frontend.
- Aplicación/captura: manager de sesión con estados `idle/running/paused/stopped`.
- Infraestructura: en transición hacia `windows-capture` + `ffmpeg-the-third`.

## Próximos hitos

1. Reemplazar `FrameConsumer` por backend `ffmpeg-the-third`.
2. Conectar salida de video/audio end-to-end con archivo válido.
3. Validar flujo completo y remover dependencia de `vendor/`.
