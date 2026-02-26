export type TargetKind = "monitor" | "window";

export interface CaptureTarget {
  id: number;
  name: string;
  width: number;
  height: number;
  originX: number;
  originY: number;
  screenWidth: number;
  screenHeight: number;
  isPrimary: boolean;
  kind: TargetKind;
}

export type CaptureState = "idle" | "running" | "paused" | "stopped";

export interface CaptureManagerSnapshot {
  state: CaptureState;
  elapsedMs: number;
  lastError?: string | null;
  videoEncoderLabel?: string | null;
  isProcessing: boolean;
}

export interface RecordingAudioStatus {
  captureSystemAudio: boolean;
  captureMicrophoneAudio: boolean;
  systemAudioDeviceName?: string | null;
  microphoneAudioDeviceName?: string | null;
}

export type OutputFormat = "mp4" | "mkv" | "webM";

export type VideoCodec = "h264" | "h265" | "vp9";
export type VideoEncoderPreference = "auto" | "nvenc" | "amf" | "qsv" | "software";

export type OutputResolution =
  | "native"
  | "fullHd"
  | "hd"
  | "sd"
  | "p1440"
  | "p2160"
  | { custom: { width: number; height: number } };

export type EncoderPreset = "ultraFast" | "fast" | "medium";
export type RecordingQualityMode = "performance" | "balanced" | "quality";

export interface CropRegion {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RecordingSessionConfig {
  targetId: number;
  fps: number;
  cropRegion?: CropRegion | null;
  outputPath: string;
  format: OutputFormat;
  codec?: VideoCodec | null;
  videoEncoderPreference?: VideoEncoderPreference;
  resolution: OutputResolution;
  crf: number;
  preset: EncoderPreset;
  qualityMode?: RecordingQualityMode;
  captureSystemAudio?: boolean;
  captureMicrophoneAudio?: boolean;
  systemAudioDevice?: string | null;
  microphoneDevice?: string | null;
  microphoneGainPercent?: number;
}
