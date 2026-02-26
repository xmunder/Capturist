import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { TopBar } from "./components/TopBar";
import { CaptureControls } from "./components/CaptureControls";
import { StatusPanel } from "./components/StatusPanel";
import { Alerts } from "./components/Alerts";
import { AdvancedSettingsModal } from "./components/AdvancedSettingsModal";
import { useRecorderController } from "./hooks/useRecorderController";
import styles from "./App.module.css";

const IDLE_FAVICON_PATH = "/favicon.svg";
const RECORDING_FAVICON_PATH = "/favicon-record.svg";

function dataUrlToBytes(dataUrl: string): Uint8Array {
  const base64 = dataUrl.split(",", 2)[1];
  if (!base64) {
    throw new Error("No se pudo leer el PNG generado del icono.");
  }
  const binary = window.atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function buildRecordingOverlayPngBytes(size = 64): Uint8Array {
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;

  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("No se pudo inicializar canvas para crear el overlay.");
  }

  context.clearRect(0, 0, size, size);
  const center = size / 2;
  const radius = size * 0.28;

  context.beginPath();
  context.arc(center, center, radius, 0, Math.PI * 2);
  context.fillStyle = "#f44336";
  context.fill();

  context.lineWidth = Math.max(1.5, size * 0.05);
  context.strokeStyle = "rgba(255, 255, 255, 0.9)";
  context.stroke();

  return dataUrlToBytes(canvas.toDataURL("image/png"));
}

async function svgPathToPngBytes(svgPath: string, size = 128): Promise<Uint8Array> {
  const response = await window.fetch(svgPath, { cache: "no-store" });
  if (!response.ok) {
    throw new Error(`No se pudo leer el SVG (${response.status}): ${svgPath}`);
  }
  const svgText = await response.text();
  const svgBlobUrl = URL.createObjectURL(
    new Blob([svgText], { type: "image/svg+xml;charset=utf-8" }),
  );

  return new Promise<Uint8Array>((resolve, reject) => {
    const image = new window.Image();
    image.onload = () => {
      const canvas = document.createElement("canvas");
      canvas.width = size;
      canvas.height = size;
      const context = canvas.getContext("2d");
      if (!context) {
        URL.revokeObjectURL(svgBlobUrl);
        reject(new Error("No se pudo inicializar canvas para convertir el SVG."));
        return;
      }

      context.clearRect(0, 0, size, size);
      context.drawImage(image, 0, 0, size, size);
      try {
        const dataUrl = canvas.toDataURL("image/png");
        resolve(dataUrlToBytes(dataUrl));
      } catch (error) {
        reject(error);
      } finally {
        URL.revokeObjectURL(svgBlobUrl);
      }
    };
    image.onerror = () => {
      URL.revokeObjectURL(svgBlobUrl);
      reject(new Error(`No se pudo cargar el asset SVG: ${svgPath}`));
    };
    image.src = svgBlobUrl;
  });
}

function App() {
  const [showQuality, setShowQuality] = useState(false);
  const { state, derived, actions } = useRecorderController();
  const taskbarIconCacheRef = useRef<{
    idle?: Uint8Array;
    recording?: Uint8Array;
    overlay?: Uint8Array;
  }>({});
  const iconRequestIdRef = useRef(0);

  useEffect(() => {
    const requestId = ++iconRequestIdRef.current;
    const isActiveRecording = state.status === "running" || state.status === "paused";
    const iconPath = isActiveRecording ? RECORDING_FAVICON_PATH : IDLE_FAVICON_PATH;
    const isWindowsPlatform =
      typeof navigator !== "undefined" && /windows/i.test(navigator.userAgent);
    const pageIconPath = isWindowsPlatform ? IDLE_FAVICON_PATH : iconPath;
    const baseIconKey = isWindowsPlatform
      ? "idle"
      : isActiveRecording
        ? "recording"
        : "idle";
    const baseIconPath = isWindowsPlatform ? IDLE_FAVICON_PATH : iconPath;

    const syncIcons = async () => {
      let iconLink = document.querySelector<HTMLLinkElement>("link[rel~='icon']");
      if (!iconLink) {
        iconLink = document.createElement("link");
        iconLink.rel = "icon";
        iconLink.href = IDLE_FAVICON_PATH;
        iconLink.type = "image/svg+xml";
        document.head.appendChild(iconLink);
      }

      iconLink.href = pageIconPath;
      iconLink.type = "image/svg+xml";

      try {
        let baseIconBytes = taskbarIconCacheRef.current[baseIconKey];
        if (!baseIconBytes) {
          baseIconBytes = await svgPathToPngBytes(baseIconPath, 128);
          taskbarIconCacheRef.current[baseIconKey] = baseIconBytes;
        }

        if (iconRequestIdRef.current !== requestId) {
          return;
        }

        const currentWindow = getCurrentWindow();
        await currentWindow.setIcon(baseIconBytes);

        let overlayBytes: Uint8Array | undefined;
        if (isWindowsPlatform) {
          if (isActiveRecording) {
            overlayBytes = taskbarIconCacheRef.current.overlay;
            if (!overlayBytes) {
              overlayBytes = buildRecordingOverlayPngBytes(64);
              taskbarIconCacheRef.current.overlay = overlayBytes;
            }
            await currentWindow.setOverlayIcon(overlayBytes);
          } else {
            await currentWindow.setOverlayIcon();
          }
        }

        if (isActiveRecording) {
          window.setTimeout(() => {
            void currentWindow.setIcon(baseIconBytes).catch((err) => {
              console.warn("[icon] no se pudo reintentar icono de taskbar", err);
            });
            if (isWindowsPlatform && overlayBytes) {
              void currentWindow.setOverlayIcon(overlayBytes).catch((err) => {
                console.warn("[icon] no se pudo reintentar overlay del taskbar", err);
              });
            }
          }, 120);
        } else if (isWindowsPlatform) {
          void currentWindow.setOverlayIcon().catch((err) => {
            console.warn("[icon] no se pudo limpiar overlay del taskbar", err);
          });
        }
      } catch (err) {
        console.warn("[icon] no se pudo actualizar icono de taskbar", err);
      }
    };

    void syncIcons();
  }, [state.status]);

  return (
    <main className={styles.app}>
      <div className={styles.shell}>
        <TopBar onOpenSettings={() => setShowQuality(true)} />

        <section className={styles.workspace}>
          <CaptureControls
            targets={state.targets}
            selectedTargetId={state.selectedTargetId}
            supported={state.supported}
            status={state.status}
            busy={state.busy}
            isProcessing={derived.isProcessing}
            isRecording={derived.isRecording}
            showCropControls={derived.showCropControls}
            cropEnabled={state.cropEnabled}
            captureSystemAudio={state.captureSystemAudio}
            captureMicrophoneAudio={state.captureMicrophoneAudio}
            microphoneGainPercent={state.microphoneGainPercent}
            audioInputDevices={state.audioInputDevices}
            selectedMicrophoneDevice={state.selectedMicrophoneDevice}
            onSelectTarget={(targetId) => actions.setSelectedTargetId(targetId)}
            onRefreshTargets={actions.refreshTargets}
            onToggleCropEnabled={(enabled) => actions.setCropEnabled(enabled)}
            onOpenRegionOverlay={actions.openRegionOverlay}
            onCaptureSystemAudioChange={actions.setCaptureSystemAudio}
            onCaptureMicrophoneAudioChange={actions.setCaptureMicrophoneAudio}
            onMicrophoneGainPercentChange={actions.setMicrophoneGainPercent}
            onSelectMicrophoneDevice={actions.setSelectedMicrophoneDevice}
            onRefreshMicrophoneDevices={actions.refreshAudioInputDevices}
            onStart={actions.startRecording}
            onPause={actions.pauseRecording}
            onResume={actions.resumeRecording}
            onStop={actions.stopRecording}
          />

          <StatusPanel
            status={state.status}
            statusLabel={derived.statusLabel}
            elapsedLabel={derived.elapsedLabel}
            resolutionLabel={derived.outputResolutionLabel}
            codecLabel={derived.codecLabel}
            captureModeLabel={derived.captureModeLabel}
            systemCaptureLabel={derived.systemCaptureLabel}
            microphoneCaptureLabel={derived.microphoneCaptureLabel}
            outputDirLabel={derived.compactOutputDir}
          />
        </section>

        <Alerts
          lastError={state.lastError}
          errorMsg={state.errorMsg}
          isProcessing={derived.isProcessing}
        />
      </div>

      <AdvancedSettingsModal
        open={showQuality}
        onClose={() => setShowQuality(false)}
        isRecording={derived.isRecording}
        fps={state.fps}
        format={state.format}
        codec={state.codec}
        videoEncoderCapabilities={derived.videoEncoderCapabilities}
        preset={state.preset}
        qualityMode={state.qualityMode}
        resolutionChoice={state.resolutionChoice}
        supports4kOutput={derived.supports4kOutput}
        crf={state.crf}
        customWidth={state.customWidth}
        customHeight={state.customHeight}
        outputDir={state.outputDir}
        outputName={state.outputName}
        onFpsChange={actions.setFps}
        onFormatChange={actions.setFormat}
        onCodecChange={actions.setCodec}
        onPresetChange={actions.setPreset}
        onQualityModeChange={actions.setQualityMode}
        onResolutionChoiceChange={actions.setResolutionChoice}
        onCrfChange={actions.setCrf}
        onCustomWidthInput={actions.setCustomWidthFromInput}
        onCustomHeightInput={actions.setCustomHeightFromInput}
        onOutputDirChange={actions.setOutputDir}
        onOutputNameChange={actions.setOutputName}
        onPickOutputDir={actions.pickOutputDir}
        shortcuts={state.keyboardShortcuts}
        onShortcutChange={actions.setKeyboardShortcut}
      />
    </main>
  );
}

export default App;
