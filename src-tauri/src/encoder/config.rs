use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputFormat {
    Mp4,
    Mkv,
    WebM,
}

impl OutputFormat {
    pub fn ffmpeg_format_name(&self) -> &str {
        match self {
            OutputFormat::Mp4 => "mp4",
            OutputFormat::Mkv => "matroska",
            OutputFormat::WebM => "webm",
        }
    }

    pub fn default_codec(&self) -> VideoCodec {
        match self {
            OutputFormat::Mp4 | OutputFormat::Mkv => VideoCodec::H264,
            OutputFormat::WebM => VideoCodec::Vp9,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
}

impl VideoCodec {
    pub fn ffmpeg_encoder_name(&self) -> &str {
        match self {
            VideoCodec::H264 => "libx264",
            VideoCodec::H265 => "libx265",
            VideoCodec::Vp9 => "libvpx-vp9",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VideoEncoderPreference {
    #[default]
    Auto,
    Nvenc,
    Amf,
    Qsv,
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum QualityMode {
    Performance,
    #[default]
    Balanced,
    Quality,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputResolution {
    Native,
    FullHd,
    Hd,
    Sd,
    #[serde(rename = "p1440")]
    P1440,
    #[serde(rename = "p2160")]
    P2160,
    Custom {
        width: u32,
        height: u32,
    },
}

impl OutputResolution {
    pub fn dimensions(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        match self {
            OutputResolution::Native => (source_width, source_height),
            OutputResolution::FullHd => (1920, 1080),
            OutputResolution::Hd => (1280, 720),
            OutputResolution::Sd => (854, 480),
            OutputResolution::P1440 => (2560, 1440),
            OutputResolution::P2160 => (3840, 2160),
            OutputResolution::Custom { width, height } => (*width, *height),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EncoderPreset {
    UltraFast,
    Fast,
    Medium,
}

impl EncoderPreset {
    pub fn as_str(&self) -> &str {
        match self {
            EncoderPreset::UltraFast => "ultrafast",
            EncoderPreset::Fast => "fast",
            EncoderPreset::Medium => "medium",
        }
    }
}

fn default_microphone_gain_percent() -> u16 {
    100
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioCaptureConfig {
    #[serde(default)]
    pub capture_system_audio: bool,
    #[serde(default)]
    pub capture_microphone_audio: bool,
    #[serde(default)]
    pub system_audio_device: Option<String>,
    #[serde(default)]
    pub microphone_device: Option<String>,
    #[serde(default = "default_microphone_gain_percent")]
    pub microphone_gain_percent: u16,
}

impl Default for AudioCaptureConfig {
    fn default() -> Self {
        Self {
            capture_system_audio: false,
            capture_microphone_audio: false,
            system_audio_device: None,
            microphone_device: None,
            microphone_gain_percent: default_microphone_gain_percent(),
        }
    }
}

impl AudioCaptureConfig {
    pub fn is_enabled(&self) -> bool {
        self.capture_system_audio || self.capture_microphone_audio
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncoderConfig {
    pub output_path: PathBuf,
    pub format: OutputFormat,
    pub codec: Option<VideoCodec>,
    #[serde(default)]
    pub video_encoder_preference: VideoEncoderPreference,
    pub resolution: OutputResolution,
    pub crf: u32,
    pub preset: EncoderPreset,
    #[serde(default)]
    pub quality_mode: QualityMode,
    pub fps: u32,
    #[serde(default)]
    pub audio: AudioCaptureConfig,
}

impl EncoderConfig {
    pub fn effective_codec(&self) -> VideoCodec {
        self.codec
            .clone()
            .unwrap_or_else(|| self.format.default_codec())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.fps == 0 || self.fps > 120 {
            return Err(format!(
                "FPS inválido: {}. Debe estar entre 1 y 120",
                self.fps
            ));
        }

        if self.crf > 51 {
            return Err(format!(
                "CRF inválido: {}. Debe estar entre 0 y 51",
                self.crf
            ));
        }

        if let OutputResolution::Custom { width, height } = &self.resolution {
            if *width == 0 || *height == 0 {
                return Err("La resolución personalizada debe tener ancho y alto > 0".to_string());
            }
        }

        if let Some(device) = &self.audio.system_audio_device {
            if device.trim().is_empty() {
                return Err(
                    "El nombre del dispositivo de audio del sistema no puede estar vacío"
                        .to_string(),
                );
            }
        }

        if let Some(device) = &self.audio.microphone_device {
            if device.trim().is_empty() {
                return Err(
                    "El nombre del dispositivo de micrófono no puede estar vacío".to_string(),
                );
            }
        }

        if self.audio.microphone_gain_percent > 400 {
            return Err(format!(
                "Ganancia de micrófono inválida: {}%. Debe estar entre 0% y 400%",
                self.audio.microphone_gain_percent
            ));
        }

        if self.format == OutputFormat::WebM {
            let codec = self.effective_codec();
            if codec != VideoCodec::Vp9 {
                return Err("WebM solo es compatible con el codec VP9".to_string());
            }
        }

        Ok(())
    }
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("recording.mp4"),
            format: OutputFormat::Mp4,
            codec: None,
            video_encoder_preference: VideoEncoderPreference::Auto,
            resolution: OutputResolution::Native,
            crf: 23,
            preset: EncoderPreset::UltraFast,
            quality_mode: QualityMode::Balanced,
            fps: 30,
            audio: AudioCaptureConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AudioCaptureConfig, EncoderConfig, OutputFormat, OutputResolution, VideoCodec,
        VideoEncoderPreference,
    };

    #[test]
    fn audio_config_is_enabled_si_hay_fuente_activa() {
        let system_enabled = AudioCaptureConfig {
            capture_system_audio: true,
            ..AudioCaptureConfig::default()
        };
        assert!(system_enabled.is_enabled());

        let mic_enabled = AudioCaptureConfig {
            capture_microphone_audio: true,
            ..AudioCaptureConfig::default()
        };
        assert!(mic_enabled.is_enabled());

        let disabled = AudioCaptureConfig::default();
        assert!(!disabled.is_enabled());
    }

    #[test]
    fn validate_rechaza_fps_fuera_de_rango() {
        let config = EncoderConfig {
            fps: 0,
            ..EncoderConfig::default()
        };

        let err = config
            .validate()
            .expect_err("debio fallar por fps invalido");
        assert!(err.contains("FPS inválido"));
    }

    #[test]
    fn validate_rechaza_ganancia_de_microfono_fuera_de_rango() {
        let config = EncoderConfig {
            audio: AudioCaptureConfig {
                microphone_gain_percent: 401,
                ..AudioCaptureConfig::default()
            },
            ..EncoderConfig::default()
        };

        let err = config
            .validate()
            .expect_err("debio fallar por ganancia de microfono invalida");
        assert!(err.contains("Ganancia de micrófono inválida"));
    }

    #[test]
    fn validate_rechaza_webm_con_codec_no_vp9() {
        let config = EncoderConfig {
            format: OutputFormat::WebM,
            codec: Some(VideoCodec::H264),
            video_encoder_preference: VideoEncoderPreference::Auto,
            ..EncoderConfig::default()
        };

        let err = config
            .validate()
            .expect_err("debio fallar por codec incompatible en webm");
        assert!(err.contains("WebM solo es compatible"));
    }

    #[test]
    fn validate_acepta_configuracion_valida() {
        let config = EncoderConfig {
            format: OutputFormat::Mp4,
            resolution: OutputResolution::Custom {
                width: 1920,
                height: 1080,
            },
            ..EncoderConfig::default()
        };

        assert!(config.validate().is_ok());
    }
}
