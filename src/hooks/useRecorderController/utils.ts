import { homeDir, join, videoDir } from "@tauri-apps/api/path";
import type { CaptureState, VideoCodec } from "../../recorder/types";

export type CodecChoice = "auto" | VideoCodec | "nvenc" | "amf" | "qsv";

export const DEFAULT_CRF = 23;
export const DEFAULT_OUTPUT_NAME = "Video";

export const STATUS_LABELS: Record<CaptureState, string> = {
  idle: "En espera",
  running: "Grabando",
  paused: "Pausado",
  stopped: "Detenido",
};

export function formatElapsed(ms: number) {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = String(Math.floor(total / 3600)).padStart(2, "0");
  const minutes = String(Math.floor((total % 3600) / 60)).padStart(2, "0");
  const seconds = String(total % 60).padStart(2, "0");
  return `${hours}:${minutes}:${seconds}`;
}

export function withExtension(path: string, ext: string) {
  const safe = path.trim();
  if (!safe) return safe;

  const known = ["mp4", "mkv", "webm"];
  const match = safe.match(/\.([a-z0-9]+)$/i);
  if (match && known.includes(match[1].toLowerCase())) {
    return safe.replace(/\.[a-z0-9]+$/i, `.${ext}`);
  }

  if (!match) {
    return `${safe}.${ext}`;
  }

  return safe;
}

export function stripExtension(filename: string) {
  return filename.replace(/\.[a-z0-9]+$/i, "");
}

export function formatTimestamp(date = new Date()) {
  return date
    .toISOString()
    .replace(/[:.]/g, "-")
    .replace("T", "_")
    .slice(0, 19);
}

export function injectOutputNameTokens(filename: string, date = new Date()) {
  const yyyy = date.getFullYear();
  const mm = String(date.getMonth() + 1).padStart(2, "0");
  const dd = String(date.getDate()).padStart(2, "0");
  const hh = String(date.getHours()).padStart(2, "0");
  const min = String(date.getMinutes()).padStart(2, "0");
  const ss = String(date.getSeconds()).padStart(2, "0");

  return filename
    .replace(/\{date\}/g, `${yyyy}-${mm}-${dd}`)
    .replace(/\{time\}/g, `${hh}-${min}-${ss}`);
}

export async function defaultVideosDir(homePath?: string) {
  try {
    const base = await videoDir();
    if (base?.trim()) {
      return join(base, "Recordings");
    }
  } catch {
    // fallback below
  }

  const userHome = homePath ?? (await homeDir());
  return join(userHome, "Videos", "Recordings");
}

export function toCompactPath(path: string, homePath: string) {
  if (!path) return "";
  if (!homePath) return path;

  const normalizedPath = path.replace(/\\/g, "/");
  const normalizedHome = homePath.replace(/\\/g, "/");

  if (normalizedPath === normalizedHome) {
    return "~";
  }

  if (normalizedPath.startsWith(`${normalizedHome}/`)) {
    return `~/${normalizedPath.slice(normalizedHome.length + 1)}`;
  }

  return path;
}

export function formatCodec(codec: CodecChoice) {
  if (codec === "auto") return "Auto";
  if (codec === "nvenc") return "NVENC (NVIDIA)";
  if (codec === "amf") return "AMF (AMD)";
  if (codec === "qsv") return "QSV (Intel)";
  if (codec === "h264") return "H.264";
  if (codec === "h265") return "H.265";
  return "VP9";
}
