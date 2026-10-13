//! Audio signal helpers: resampling to 16 kHz, energy-based voice activity
//! trimming and gain normalization. Pure std so it is unit-testable.

pub const TARGET_RATE: u32 = 16_000;

/// Root-mean-square level of a block of samples.
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

/// Resample mono audio. When downsampling, each output sample averages the
/// input window it covers (a cheap anti-aliasing filter); when upsampling it
/// interpolates linearly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    if ratio > 1.0 {
        for k in 0..out_len {
            let start = (k as f64 * ratio) as usize;
            let end = (((k + 1) as f64 * ratio) as usize).min(input.len()).max(start + 1);
            let window = &input[start..end];
            out.push(window.iter().sum::<f32>() / window.len() as f32);
        }
    } else {
        for k in 0..out_len {
            let pos = k as f64 * ratio;
            let i = pos.floor() as usize;
            let frac = (pos - i as f64) as f32;
            let a = input[i];
            let b = *input.get(i + 1).unwrap_or(&a);
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Result of trimming silence.
#[derive(Debug, Clone)]
pub struct Trimmed {
    pub samples: Vec<f32>,
    pub speech_ms: u32,
}

/// Trim leading/trailing silence from 16 kHz audio using frame energy.
/// Returns None if there is no speech at all.
pub fn trim_silence(samples: &[f32], rate: u32, sensitive: bool) -> Option<Trimmed> {
    let frame = (rate as usize * 30) / 1000; // 30 ms
    if samples.len() < frame * 3 {
        return None;
    }
    let energies: Vec<f32> = samples.chunks(frame).map(rms).collect();
    let mut sorted = energies.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_floor = sorted[sorted.len() / 10];
    let (min_abs, factor) = if sensitive { (0.0015, 2.0) } else { (0.006, 3.0) };
    let threshold = (noise_floor * factor).max(min_abs);

    let voiced: Vec<bool> = energies.iter().map(|&e| e > threshold).collect();
    let voiced_count = voiced.iter().filter(|&&v| v).count();
    // Need at least ~120 ms of voiced frames.
    if voiced_count < 4 {
        return None;
    }
    let first = voiced.iter().position(|&v| v)?;
    let last = voiced.iter().rposition(|&v| v)?;
    let pad = 8; // ~240 ms of context on each side
    let start = first.saturating_sub(pad) * frame;
    let end = ((last + 1 + pad) * frame).min(samples.len());
    Some(Trimmed { samples: samples[start..end].to_vec(), speech_ms: (voiced_count * 30) as u32 })
}

/// Scale audio so its peak sits at ~0.9, never amplifying by more than `max_gain`.
pub fn normalize(samples: &mut [f32], max_gain: f32) {
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak <= 1e-6 {
        return;
    }
    let gain = (0.9 / peak).min(max_gain);
    if (gain - 1.0).abs() < 0.01 {
        return;
    }
    for s in samples.iter_mut() {
        *s = (*s * gain).clamp(-1.0, 1.0);
    }
}

/// Whisper.cpp rejects very short inputs; pad to at least `min_ms` with silence.
pub fn pad_to(samples: &mut Vec<f32>, rate: u32, min_ms: u32) {
    let min_len = (rate as usize * min_ms as usize) / 1000;
    if samples.len() < min_len {
        let missing = min_len - samples.len();
        let head = missing / 2;
        let mut padded = vec![0.0; head];
        padded.extend_from_slice(samples);
        padded.resize(min_len, 0.0);
        *samples = padded;
    }
}

/// Downmix interleaved multi-channel audio to mono.
pub fn downmix(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, ms: u32, amp: f32) -> Vec<f32> {
        let n = (rate * ms / 1000) as usize;
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin()).collect()
    }

    #[test]
    fn resample_lengths() {
        let x = vec![0.5f32; 48_000];
        assert_eq!(resample(&x, 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&x[..44_100], 44_100, 16_000).len(), 16_000);
        assert_eq!(resample(&x[..8_000], 8_000, 16_000).len(), 16_000);
        assert!(resample(&x, 48_000, 16_000).iter().all(|v| (v - 0.5).abs() < 1e-5));
    }

    #[test]
    fn resample_keeps_tone_energy() {
        let x = sine(440.0, 48_000, 500, 0.5);
        let y = resample(&x, 48_000, 16_000);
        let (a, b) = (rms(&x), rms(&y));
        assert!((a - b).abs() / a < 0.05, "rms {a} vs {b}");
    }

    #[test]
    fn trims_silence() {
        let mut audio = vec![0.0005f32; 16_000]; // 1 s near-silence
        audio.extend(sine(300.0, 16_000, 800, 0.3)); // 0.8 s "speech"
        audio.extend(vec![0.0005f32; 16_000]);
        let t = trim_silence(&audio, 16_000, false).unwrap();
        assert!(t.samples.len() < audio.len());
        assert!(t.samples.len() >= 12_800, "kept {}", t.samples.len());
        assert!(t.speech_ms >= 700);
    }

    #[test]
    fn silence_is_rejected() {
        let audio = vec![0.0004f32; 32_000];
        assert!(trim_silence(&audio, 16_000, false).is_none());
    }

    #[test]
    fn whisper_quiet_speech_needs_sensitive_mode() {
        let mut audio = vec![0.0003f32; 16_000];
        audio.extend(sine(300.0, 16_000, 800, 0.004));
        audio.extend(vec![0.0003f32; 8_000]);
        assert!(trim_silence(&audio, 16_000, false).is_none());
        assert!(trim_silence(&audio, 16_000, true).is_some());
    }

    #[test]
    fn normalizes_with_cap() {
        let mut x = vec![0.1f32, -0.2, 0.05];
        normalize(&mut x, 10.0);
        assert!((x[1] + 0.9).abs() < 1e-5);
        let mut quiet = vec![0.001f32, -0.001];
        normalize(&mut quiet, 8.0);
        assert!((quiet[0] - 0.008).abs() < 1e-6);
    }

    #[test]
    fn pads_short_audio() {
        let mut x = vec![0.1f32; 4_000];
        pad_to(&mut x, 16_000, 1_000);
        assert_eq!(x.len(), 16_000);
    }

    #[test]
    fn downmixes() {
        assert_eq!(downmix(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }
}
