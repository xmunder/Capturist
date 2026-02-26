# Licencias y Compliance (FFmpeg)

Este documento resume el estado tecnico de licencias para la distribucion Windows de Capturist.

No constituye asesoria legal. Antes de distribucion publica/comercial se recomienda revision legal formal.

## 1) Evidencia tecnica observada en el repositorio

- Artefactos FFmpeg usados por el proyecto: `ffmpeg-windows/`
- Archivo de licencia presente: `ffmpeg-windows/LICENSE.txt` (GPLv3)
- Inspeccion local del binario (`strings ffmpeg-windows/bin/ffmpeg.exe`) detecta flags de build:
  - `--enable-gpl`
  - `--enable-version3`
  - `--enable-libx264`
  - `--enable-libx265`
- En la inspeccion local no aparece `--enable-nonfree`.

## 2) Impacto de licencia esperado

Con la evidencia anterior, esta build de FFmpeg debe tratarse como distribucion GPLv3.

Adicionalmente, codecs como H.264/H.265 pueden tener implicaciones de patentes segun pais/uso.

## 3) Medidas implementadas en el proyecto

- `scripts/build-and-deploy.sh` valida la presencia de `ffmpeg-windows/LICENSE.txt`.
- `scripts/build-and-deploy.sh` empaqueta esa licencia en:
  - installer (resource de Tauri)
  - portable (`THIRD_PARTY_LICENSES/FFMPEG-GPLv3.txt`)

## 4) Checklist de release (obligatorio)

1. Confirmar binario real a distribuir:
   - `ffmpeg.exe -version`
   - `ffmpeg.exe -buildconf`
2. Guardar evidencia del build de FFmpeg junto a artefactos de release.
3. Incluir texto de licencia GPLv3 y avisos de terceros en el paquete final.
4. Revisar impacto de patentes/codecs para el mercado objetivo.
5. Validar que lo declarado en documentacion coincide con lo empaquetado.
