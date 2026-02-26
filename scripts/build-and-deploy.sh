#!/usr/bin/env bash
set -euo pipefail

phase() {
  local title="$1"
  echo
  echo "############################################################"
  echo "# ${title}"
  echo "############################################################"
}

info() {
  echo "[INFO] $*"
}

warn() {
  echo "[WARN] $*"
}

error() {
  echo "[ERROR] $*" >&2
}

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="${APP_NAME:-capturist}"
APP_EXE_NAME="${APP_EXE_NAME:-${APP_NAME}.exe}"
TARGET="${TARGET:-x86_64-pc-windows-msvc}"
PACKAGE_MODE="${PACKAGE_MODE:-installer}" # installer | portable | both
ARTIFACTS_DIR="${ARTIFACTS_DIR:-$ROOT_DIR/artifacts}"
FFMPEG_DIR="${FFMPEG_DIR:-$ROOT_DIR/ffmpeg-windows}"
FFMPEG_LICENSE_FILE="$FFMPEG_DIR/LICENSE.txt"
RUN_XWIN_CHECK="${RUN_XWIN_CHECK:-1}"
RUN_FRONTEND_BUILD="${RUN_FRONTEND_BUILD:-1}"

REQUIRED_LIBS=(avcodec avformat avutil swscale swresample avdevice avfilter)
REQUIRED_DLLS=(avcodec avformat avutil swscale swresample avdevice avfilter)

phase "FASE 0: VALIDACIONES INICIALES"

case "$PACKAGE_MODE" in
  installer|portable|both) ;;
  *)
    error "PACKAGE_MODE invalido: '$PACKAGE_MODE'"
    error "Valores validos: installer | portable | both"
    exit 1
    ;;
esac

for cmd in pnpm cargo cargo-xwin realpath; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    error "No se encontro el comando requerido: $cmd"
    exit 1
  fi
done

if [[ "$PACKAGE_MODE" == "installer" || "$PACKAGE_MODE" == "both" ]]; then
  if ! command -v makensis >/dev/null 2>&1; then
    error "No se encontro 'makensis' (NSIS) en el host Linux/WSL."
    error "Instala NSIS con: sudo apt update && sudo apt install -y nsis"
    exit 1
  fi
fi

if [[ ! -d "$FFMPEG_DIR" ]]; then
  error "No existe FFMPEG_DIR: $FFMPEG_DIR"
  exit 1
fi

phase "FASE 1: VERIFICACION DE ARTEFACTOS FFMPEG"

MISSING=0

if [[ ! -d "$FFMPEG_DIR/include" ]]; then
  error "Falta carpeta include/ en: $FFMPEG_DIR"
  MISSING=1
fi
if [[ ! -d "$FFMPEG_DIR/lib" ]]; then
  error "Falta carpeta lib/ en: $FFMPEG_DIR"
  MISSING=1
fi
if [[ ! -d "$FFMPEG_DIR/bin" ]]; then
  error "Falta carpeta bin/ en: $FFMPEG_DIR"
  MISSING=1
fi

for lib in "${REQUIRED_LIBS[@]}"; do
  if [[ ! -f "$FFMPEG_DIR/lib/${lib}.lib" ]]; then
    error "Falta archivo de link: lib/${lib}.lib"
    MISSING=1
  fi
done

for dll in "${REQUIRED_DLLS[@]}"; do
  if ! ls "$FFMPEG_DIR/bin/${dll}-"*.dll >/dev/null 2>&1; then
    error "Falta DLL requerida: bin/${dll}-*.dll"
    MISSING=1
  fi
done

if [[ ! -f "$FFMPEG_DIR/bin/ffmpeg.exe" ]]; then
  error "Falta archivo: bin/ffmpeg.exe"
  MISSING=1
fi

if [[ ! -f "$FFMPEG_LICENSE_FILE" ]]; then
  error "Falta licencia FFmpeg: LICENSE.txt"
  MISSING=1
fi

if [[ "$MISSING" -eq 1 ]]; then
  error "Validacion FFmpeg fallida."
  exit 1
fi

info "Artefactos FFmpeg validados en: $FFMPEG_DIR"

phase "FASE 2: ENTORNO DE BUILD"

export CARGO_TERM_COLOR=always
export FORCE_COLOR=1
export CLICOLOR=1
export CLICOLOR_FORCE=1
export npm_config_color=always
export FFMPEG_DIR
export FFMPEG_INCLUDE_DIR="$FFMPEG_DIR/include"
export FFMPEG_LIB_DIR="$FFMPEG_DIR/lib"

info "TARGET=$TARGET"
info "PACKAGE_MODE=$PACKAGE_MODE"
info "FFMPEG_DIR=$FFMPEG_DIR"

create_tauri_overlay_config() {
  local overlay_file="$1"
  local ffmpeg_bin_rel
  local ffmpeg_license_rel
  ffmpeg_bin_rel="$(realpath --relative-to "$ROOT_DIR/src-tauri" "$FFMPEG_DIR/bin")"
  ffmpeg_license_rel="$(realpath --relative-to "$ROOT_DIR/src-tauri" "$FFMPEG_LICENSE_FILE")"

  cat >"$overlay_file" <<JSON
{
  "bundle": {
    "targets": ["nsis"],
    "resources": {
      "${ffmpeg_bin_rel}/ffmpeg.exe": "ffmpeg.exe",
      "${ffmpeg_bin_rel}/avcodec-*.dll": "",
      "${ffmpeg_bin_rel}/avdevice-*.dll": "",
      "${ffmpeg_bin_rel}/avfilter-*.dll": "",
      "${ffmpeg_bin_rel}/avformat-*.dll": "",
      "${ffmpeg_bin_rel}/avutil-*.dll": "",
      "${ffmpeg_bin_rel}/swresample-*.dll": "",
      "${ffmpeg_bin_rel}/swscale-*.dll": "",
      "${ffmpeg_license_rel}": "THIRD_PARTY_LICENSES/FFMPEG-GPLv3.txt"
    }
  }
}
JSON
}

phase "FASE 3: VALIDACION CROSS-TARGET"
if [[ "$RUN_XWIN_CHECK" == "1" ]]; then
  cargo xwin check --manifest-path src-tauri/Cargo.toml --target "$TARGET"
else
  warn "Se omite cargo xwin check porque RUN_XWIN_CHECK=$RUN_XWIN_CHECK"
fi

phase "FASE 4: BUILD FRONTEND"
if [[ "$RUN_FRONTEND_BUILD" == "1" ]]; then
  pnpm run build
else
  warn "Se omite pnpm run build porque RUN_FRONTEND_BUILD=$RUN_FRONTEND_BUILD"
fi

phase "FASE 5: BUILD TAURI WINDOWS"

overlay_config=""
cleanup() {
  if [[ -n "$overlay_config" && -f "$overlay_config" ]]; then
    rm -f "$overlay_config"
  fi
}
trap cleanup EXIT

tauri_build_cmd=(pnpm tauri build --runner cargo-xwin --target "$TARGET")

if [[ "$PACKAGE_MODE" == "portable" ]]; then
  tauri_build_cmd+=(--no-bundle)
else
  overlay_config="$(mktemp)"
  create_tauri_overlay_config "$overlay_config"
  tauri_build_cmd+=(-c "$overlay_config")
fi

"${tauri_build_cmd[@]}"

release_dir="$ROOT_DIR/src-tauri/target/${TARGET}/release"
if [[ ! -d "$release_dir" ]]; then
  error "No se encontro directorio release para target: $release_dir"
  exit 1
fi

resolve_release_exe() {
  local expected="$release_dir/$APP_EXE_NAME"
  if [[ -f "$expected" ]]; then
    echo "$expected"
    return 0
  fi

  local fallback
  fallback="$(find "$release_dir" -maxdepth 1 -type f -name '*.exe' ! -name 'ffmpeg.exe' | sort | head -n1 || true)"
  if [[ -n "$fallback" ]]; then
    echo "$fallback"
    return 0
  fi

  return 1
}

copy_portable_artifacts() {
  local exe_path="$1"
  local portable_dir="$2/$APP_NAME"
  local portable_licenses_dir="$portable_dir/THIRD_PARTY_LICENSES"

  mkdir -p "$portable_dir"
  mkdir -p "$portable_licenses_dir"
  cp "$exe_path" "$portable_dir/$APP_EXE_NAME"
  cp "$FFMPEG_DIR/bin/ffmpeg.exe" "$portable_dir/"
  cp "$FFMPEG_LICENSE_FILE" "$portable_licenses_dir/FFMPEG-GPLv3.txt"

  shopt -s nullglob
  for dll in "${REQUIRED_DLLS[@]}"; do
    local matches=("$FFMPEG_DIR/bin/${dll}-"*.dll)
    if [[ "${#matches[@]}" -eq 0 ]]; then
      error "No se encontro DLL para patron: ${dll}-*.dll"
      exit 1
    fi
    cp "${matches[0]}" "$portable_dir/"
  done
  shopt -u nullglob

  info "Portable generado en: $portable_dir"
}

copy_installer_artifact() {
  local installer_output_dir="$1"
  local nsis_dir="$release_dir/bundle/nsis"

  if [[ ! -d "$nsis_dir" ]]; then
    error "No se encontro carpeta NSIS: $nsis_dir"
    exit 1
  fi

  local installer_path
  installer_path="$(ls -1t "$nsis_dir"/*.exe 2>/dev/null | head -n1 || true)"
  if [[ -z "$installer_path" ]]; then
    error "No se encontro instalador NSIS (.exe) en: $nsis_dir"
    exit 1
  fi

  mkdir -p "$installer_output_dir"
  cp "$installer_path" "$installer_output_dir/"
  info "Instalador copiado en: $installer_output_dir/$(basename "$installer_path")"
}

phase "FASE 6: EMPAQUETADO Y COPIA DE ARTEFACTOS"

build_stamp="$(date +%Y%m%d_%H%M%S)"
output_dir="$ARTIFACTS_DIR/$build_stamp"
mkdir -p "$output_dir"

if [[ "$PACKAGE_MODE" == "portable" || "$PACKAGE_MODE" == "both" ]]; then
  exe_path="$(resolve_release_exe || true)"
  if [[ -z "${exe_path:-}" ]]; then
    error "No se encontro ejecutable release para armar portable."
    exit 1
  fi
  copy_portable_artifacts "$exe_path" "$output_dir/portable"
fi

if [[ "$PACKAGE_MODE" == "installer" || "$PACKAGE_MODE" == "both" ]]; then
  copy_installer_artifact "$output_dir/installer"
fi

phase "RESUMEN"
info "Build completado."
info "Artefactos en: $output_dir"
if [[ "$PACKAGE_MODE" == "installer" || "$PACKAGE_MODE" == "both" ]]; then
  info "Installer: $output_dir/installer"
fi
if [[ "$PACKAGE_MODE" == "portable" || "$PACKAGE_MODE" == "both" ]]; then
  info "Portable:  $output_dir/portable/$APP_NAME"
fi
