# Matriz de contrato Tauri (congelada)

Este documento fija el contrato entre frontend y backend para la etapa inicial en `Capturist`.

## Comandos

| Comando | Request | Response | Notas |
|---|---|---|---|
| `is_capture_supported` | `{}` | `boolean` | `true` cuando backend de captura está disponible. |
| `get_targets` | `{}` | `CaptureTarget[]` | Lista de monitores/ventanas capturables. |
| `get_audio_input_devices` | `{}` | `string[]` | Lista de entradas de micrófono. |
| `get_recording_audio_status` | `{}` | `RecordingAudioStatus` | Estado de audio en vivo de sesión activa. |
| `set_global_shortcuts` | `{ config: ShortcutBindings }` | `void` | Valida combinaciones no vacías y no duplicadas. |
| `start_recording` | `{ config: RecordingSessionConfig }` | `void` | Valida config y arranca sesión. |
| `update_recording_audio_capture` | `{ config: { captureSystemAudio, captureMicrophoneAudio } }` | `void` | Solo permitido con sesión activa. |
| `pause_recording` | `{}` | `void` | `running -> paused`. |
| `resume_recording` | `{}` | `void` | `paused -> running`. |
| `stop_recording` | `{}` | `void` | Finaliza sesión y vuelve a `idle`. |
| `cancel_recording` | `{}` | `void` | Alias de `stop_recording`. |
| `get_recording_status` | `{}` | `CaptureManagerSnapshot` | Snapshot para polling UI. |
| `select_region_native` | `{}` | `CropRegion \| null` | En Windows abre overlay nativo y retorna region o `null` si se cancela. En no-Windows devuelve error de plataforma. |

## Modelos principales

- `CaptureTarget`
  - `id`, `name`, `width`, `height`, `originX`, `originY`, `screenWidth`, `screenHeight`, `isPrimary`, `kind`.
- `CaptureManagerSnapshot`
  - `state`, `elapsedMs`, `lastError`, `videoEncoderLabel`, `isProcessing`.
- `RecordingSessionConfig`
  - `targetId`, `fps`, `cropRegion`, `outputPath`, `format`, `codec`, `videoEncoderPreference`, `resolution`, `crf`, `preset`, `qualityMode`, `captureSystemAudio`, `captureMicrophoneAudio`, `systemAudioDevice`, `microphoneDevice`, `microphoneGainPercent`.

## Estados y transiciones

Estados de grabación:
- `idle`
- `running`
- `paused`
- `stopped`

Transiciones válidas:
- `idle -> running` (`start_recording`)
- `running -> paused` (`pause_recording`)
- `paused -> running` (`resume_recording`)
- `running/paused -> idle` (`stop_recording` o `cancel_recording`)

## Alcance de esta etapa

- El contrato está operativo y compatible en nombres de comandos/payloads.
- Descubrimiento de targets (`get_targets`) ya usa backend `windows-capture` en Windows.
- Captura continua de frames ya corre con `windows-capture` en thread dedicado.
- Encoder real (`ffmpeg-the-third`) ya integrado con salida final de archivo.
- Audio WASAPI (sistema/microfono) y mux final con FFmpeg CLI ya integrados.
