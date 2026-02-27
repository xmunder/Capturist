use crate::encoder::config::QualityMode;

use super::{AudioTrackInput, AudioTrackSource};

const SYSTEM_HIGHPASS_HZ: u32 = 80;
const SYSTEM_LOWPASS_HZ: u32 = 14_000;
const MIC_HIGHPASS_HZ: u32 = 120;
const MIC_LOWPASS_HZ: u32 = 9_000;
const MIC_NOISE_FLOOR_DB: i32 = -32;
const MIC_NOISE_REDUCTION_DB: u32 = 18;
const MIC_GATE_THRESHOLD: f32 = 0.015;
const MIC_GATE_RATIO: u32 = 3;
const MIC_GATE_ATTACK_MS: u32 = 20;
const MIC_GATE_RELEASE_MS: u32 = 250;
const MAX_GAIN_MULTIPLIER: f64 = 16.0;

fn dsp_filter_chain(quality_mode: &QualityMode) -> Option<String> {
    if matches!(quality_mode, QualityMode::Performance) {
        return None;
    }

    Some(format!(
        "highpass=f={SYSTEM_HIGHPASS_HZ},lowpass=f={SYSTEM_LOWPASS_HZ}"
    ))
}

fn microphone_noise_filter_chain(quality_mode: &QualityMode) -> Option<String> {
    if !matches!(quality_mode, QualityMode::Quality) {
        return None;
    }

    Some(format!(
        "highpass=f={MIC_HIGHPASS_HZ},lowpass=f={MIC_LOWPASS_HZ},afftdn=nf={MIC_NOISE_FLOOR_DB}:nr={MIC_NOISE_REDUCTION_DB}:tn=1,agate=threshold={MIC_GATE_THRESHOLD}:ratio={MIC_GATE_RATIO}:attack={MIC_GATE_ATTACK_MS}:release={MIC_GATE_RELEASE_MS}"
    ))
}

fn microphone_light_filter_chain(quality_mode: &QualityMode) -> Option<String> {
    if matches!(quality_mode, QualityMode::Balanced) {
        return Some(format!(
            "highpass=f={MIC_HIGHPASS_HZ},lowpass=f={MIC_LOWPASS_HZ}"
        ));
    }

    None
}

fn microphone_filter_chain(quality_mode: &QualityMode) -> Option<String> {
    if let Some(chain) = microphone_noise_filter_chain(quality_mode) {
        return Some(chain);
    }

    microphone_light_filter_chain(quality_mode)
}

fn format_mic_gain(microphone_gain_percent: u16) -> String {
    let gain = (microphone_gain_percent as f64 / 100.0).clamp(0.0, MAX_GAIN_MULTIPLIER);
    let mut gain_str = format!("{gain:.3}");
    while gain_str.contains('.') && gain_str.ends_with('0') {
        gain_str.pop();
    }
    if gain_str.ends_with('.') {
        gain_str.pop();
    }
    gain_str
}

fn requires_resync(quality_mode: &QualityMode, track: &AudioTrackInput) -> bool {
    track.delay_ms > 0
        || track.source == AudioTrackSource::Microphone
        || !matches!(quality_mode, QualityMode::Performance)
}

fn build_track_prefix(quality_mode: &QualityMode, track: &AudioTrackInput) -> String {
    if requires_resync(quality_mode, track) {
        "aresample=async=1:first_pts=0,asetpts=PTS-STARTPTS".to_string()
    } else {
        "anull".to_string()
    }
}

fn build_track_chain(
    input_idx: usize,
    track: &AudioTrackInput,
    microphone_gain_percent: u16,
    quality_mode: &QualityMode,
    output_label: &str,
) -> String {
    let mut chain = format!("[{input_idx}:a]{}", build_track_prefix(quality_mode, track));
    if track.delay_ms > 0 {
        chain.push_str(&format!(",adelay={}|{}", track.delay_ms, track.delay_ms));
    }
    if track.source == AudioTrackSource::Microphone {
        if let Some(mic_filter) = microphone_filter_chain(quality_mode) {
            chain.push_str(&format!(",{mic_filter}"));
        }
        if microphone_gain_percent != 100 {
            chain.push_str(&format!(
                ",volume={}",
                format_mic_gain(microphone_gain_percent)
            ));
        }
    }
    chain.push_str(output_label);
    chain
}

pub(super) fn build_mix_filter(
    tracks: &[AudioTrackInput],
    microphone_gain_percent: u16,
    quality_mode: &QualityMode,
) -> String {
    let dsp = dsp_filter_chain(quality_mode);
    match tracks.len() {
        0 => match dsp {
            Some(chain) => format!("[0:a]anull,{chain}[aout]"),
            None => "[0:a]anull[aout]".to_string(),
        },
        1 => {
            let mut chain =
                build_track_chain(1, &tracks[0], microphone_gain_percent, quality_mode, "");
            if let Some(dsp_chain) = dsp {
                chain.push_str(&format!(",{dsp_chain}"));
            }
            chain.push_str("[aout]");
            chain
        }
        _ => {
            let mut parts = Vec::with_capacity(tracks.len() + 2);
            let mut labels = Vec::with_capacity(tracks.len());

            for (idx, track) in tracks.iter().enumerate() {
                let input_idx = idx + 1;
                let label = format!("a{}", input_idx);
                labels.push(format!("[{}]", label));
                let chain = build_track_chain(
                    input_idx,
                    track,
                    microphone_gain_percent,
                    quality_mode,
                    &format!("[{}]", label),
                );
                parts.push(chain);
            }

            parts.push(format!(
                "{}amix=inputs={}:normalize=0:dropout_transition=2[mix]",
                labels.join(""),
                tracks.len()
            ));
            if let Some(dsp_chain) = dsp {
                parts.push(format!("[mix]{dsp_chain}[aout]"));
            } else {
                parts.push("[mix]anull[aout]".to_string());
            }

            parts.join(";")
        }
    }
}

pub(super) fn build_single_track_filter(
    track: &AudioTrackInput,
    microphone_gain_percent: u16,
    quality_mode: &QualityMode,
) -> Option<String> {
    let mut segments = Vec::<String>::new();
    let prefix = build_track_prefix(quality_mode, track);
    if prefix != "anull" {
        segments.push(prefix);
    }

    if track.delay_ms > 0 {
        segments.push(format!("adelay={}|{}", track.delay_ms, track.delay_ms));
    }
    if track.source == AudioTrackSource::Microphone {
        if let Some(mic_filter) = microphone_filter_chain(quality_mode) {
            segments.push(mic_filter);
        }
        if microphone_gain_percent != 100 {
            segments.push(format!(
                "volume={}",
                format_mic_gain(microphone_gain_percent)
            ));
        }
    }
    if let Some(dsp_chain) = dsp_filter_chain(quality_mode) {
        segments.push(dsp_chain);
    }

    if segments.is_empty() {
        None
    } else {
        Some(segments.join(","))
    }
}
