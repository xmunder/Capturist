import { invoke } from "@tauri-apps/api/core";
import { homeDir, join } from "@tauri-apps/api/path";
import type {
  CaptureManagerSnapshot,
  CaptureTarget,
  OutputFormat,
  RecordingAudioStatus,
  RecordingSessionConfig,
} from "./types";

export class Grabador {
  static async isCaptureSupported(): Promise<boolean> {
    return invoke("is_capture_supported");
  }

  static async getTargets(): Promise<CaptureTarget[]> {
    return invoke("get_targets");
  }

  static async getAudioInputDevices(): Promise<string[]> {
    return invoke("get_audio_input_devices");
  }

  static async start(config: RecordingSessionConfig): Promise<void> {
    await invoke("start_recording", { config });
  }

  static async updateRecordingAudioCapture(
    captureSystemAudio: boolean,
    captureMicrophoneAudio: boolean,
  ): Promise<void> {
    await invoke("update_recording_audio_capture", {
      config: { captureSystemAudio, captureMicrophoneAudio },
    });
  }

  static async pause(): Promise<void> {
    await invoke("pause_recording");
  }

  static async resume(): Promise<void> {
    await invoke("resume_recording");
  }

  static async stop(): Promise<void> {
    await invoke("stop_recording");
  }

  static async cancel(): Promise<void> {
    await invoke("cancel_recording");
  }

  static async status(): Promise<CaptureManagerSnapshot> {
    return invoke("get_recording_status");
  }

  static async recordingAudioStatus(): Promise<RecordingAudioStatus> {
    return invoke("get_recording_audio_status");
  }

  static async selectRegionNative(): Promise<import("./types").CropRegion | null> {
    return invoke("select_region_native");
  }

  static async defaultOutputPath(format: OutputFormat) {
    const base = await homeDir();
    const timestamp = new Date()
      .toISOString()
      .replace(/[:.]/g, "-")
      .replace("T", "_")
      .slice(0, 19);
    const filename = `capturist_${timestamp}`;
    return join(base, `${filename}.${format}`);
  }
}
