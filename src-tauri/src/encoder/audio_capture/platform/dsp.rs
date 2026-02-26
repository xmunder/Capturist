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

fn dsp_filter_chain() -> String {
    format!("highpass=f={SYSTEM_HIGHPASS_HZ},lowpass=f={SYSTEM_LOWPASS_HZ}")
}

fn microphone_noise_filter_chain() -> String {
    format!(
        "highpass=f={MIC_HIGHPASS_HZ},lowpass=f={MIC_LOWPASS_HZ},afftdn=nf={MIC_NOISE_FLOOR_DB}:nr={MIC_NOISE_REDUCTION_DB}:tn=1,agate=threshold={MIC_GATE_THRESHOLD}:ratio={MIC_GATE_RATIO}:attack={MIC_GATE_ATTACK_MS}:release={MIC_GATE_RELEASE_MS}"
    )
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

fn build_track_chain(
    input_idx: usize,
    track: &AudioTrackInput,
    microphone_gain_percent: u16,
    output_label: &str,
) -> String {
    let mut chain = format!("[{input_idx}:a]aresample=async=1:first_pts=0,asetpts=PTS-STARTPTS");
    if track.delay_ms > 0 {
        chain.push_str(&format!(",adelay={}|{}", track.delay_ms, track.delay_ms));
    }
    if track.source == AudioTrackSource::Microphone {
        chain.push_str(&format!(",{}", microphone_noise_filter_chain()));
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

pub(super) fn build_mix_filter(tracks: &[AudioTrackInput], microphone_gain_percent: u16) -> String {
    let dsp = dsp_filter_chain();
    match tracks.len() {
        0 => format!("[0:a]anull,{dsp}[aout]"),
        1 => {
            let mut chain = build_track_chain(1, &tracks[0], microphone_gain_percent, "");
            chain.push_str(&format!(",{dsp}[aout]"));
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
                    &format!("[{}]", label),
                );
                parts.push(chain);
            }

            parts.push(format!(
                "{}amix=inputs={}:normalize=0:dropout_transition=2[mix]",
                labels.join(""),
                tracks.len()
            ));
            parts.push(format!("[mix]{dsp}[aout]"));

            parts.join(";")
        }
    }
}

pub(super) fn build_single_track_filter(
    track: &AudioTrackInput,
    microphone_gain_percent: u16,
) -> String {
    let mut chain = "aresample=async=1:first_pts=0,asetpts=PTS-STARTPTS".to_string();
    if track.delay_ms > 0 {
        chain.push_str(&format!(",adelay={}|{}", track.delay_ms, track.delay_ms));
    }
    if track.source == AudioTrackSource::Microphone {
        chain.push_str(&format!(",{}", microphone_noise_filter_chain()));
        if microphone_gain_percent != 100 {
            chain.push_str(&format!(
                ",volume={}",
                format_mic_gain(microphone_gain_percent)
            ));
        }
    }
    chain.push_str(&format!(",{}", dsp_filter_chain()));
    chain
}
