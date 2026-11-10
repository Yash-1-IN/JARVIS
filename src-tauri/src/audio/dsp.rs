//! The small pure pieces of capture: mixing to mono (callback-safe), resampling to 16 kHz and the
//! pre-roll. No device, no threads, so each is tested directly.

use std::collections::VecDeque;

/// What the speech models take: 16 kHz, mono.
pub const STT_RATE: u32 = 16_000;
/// How much audio before the wake or key press is kept, so the start of a request is not lost.
pub const PRE_ROLL_SECONDS: u32 = 2;

/// Mixes interleaved frames to mono and hands each sample to `push`. **Called from the audio
/// callback: no allocation, no locking.** `push` returns `false` when the ring is full, which is
/// counted by the caller as an overrun and never blocks.
pub fn mix_to_mono<T: Copy>(
    data: &[T],
    channels: usize,
    to_f32: impl Fn(T) -> f32,
    mut push: impl FnMut(f32) -> bool,
) -> u32 {
    let channels = channels.max(1);
    let mut dropped = 0;
    for frame in data.chunks_exact(channels) {
        let sum: f32 = frame.iter().map(|s| to_f32(*s)).sum();
        if !push(sum / channels as f32) {
            dropped += 1;
        }
    }
    dropped
}

/// Zero crossings of the low-pass on each side of an output sample. More is a sharper filter and a
/// longer delay; 12 gives a transition of about 3 kHz at 48 kHz and over 60 dB of rejection.
const SINC_CROSSINGS: f64 = 12.0;
/// How finely the position between two input samples is tabulated (an error of 1/512 of a sample).
const PHASES: usize = 512;

/// Resamples to 16 kHz **with a low-pass in front**, and keeps its place between chunks, so cutting
/// the input anywhere gives the same output as one long call.
///
/// Why the filter: from 48 kHz the step is exactly 3, so a plain linear resampler just keeps every
/// third sample. Everything above 8 kHz (a microphone array's hiss, a fan, the "s" and "f" of
/// speech) then folds down into the 0 to 8 kHz band the speech model listens to. A windowed-sinc
/// low-pass at 90% of the new Nyquist removes it first. (Measured on the model's sample recording
/// with high-frequency noise added: 3 extra word errors without the filter, none with it.)
///
/// The output is delayed by `half` input samples (about 0.8 ms at 48 kHz); the same rate passes
/// through unchanged with no delay.
pub struct Resampler {
    passthrough: bool,
    /// The two rates. Positions are whole numbers over `to`, so the result never depends on how the
    /// input was cut into chunks.
    from: u64,
    to: u64,
    /// Filter taps on each side of an output position, in input samples.
    half: usize,
    /// `PHASES` rows of `2 * half` weights, each row summing to 1 (unity gain at DC).
    table: Vec<f32>,
    /// Input history: `half` zeros, then what has arrived and is still needed.
    buf: Vec<f32>,
    /// How many samples have been dropped from the front of `buf`.
    base: u64,
    /// How many output samples have been produced.
    produced: u64,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        let step = f64::from(from.max(1)) / f64::from(to.max(1));
        // Cutoff in cycles per input sample: 90% of the lower of the two Nyquist rates.
        let fc = 0.45 * (1.0 / step).min(1.0);
        let half = ((SINC_CROSSINGS / (2.0 * fc)).ceil() as usize).max(2);
        let taps = 2 * half;
        let mut table = Vec::with_capacity(PHASES * taps);
        for ph in 0..PHASES {
            let frac = ph as f64 / PHASES as f64;
            let row: Vec<f64> = (0..taps)
                .map(|k| {
                    let t = (k as f64 - half as f64 + 1.0) - frac; // distance in input samples
                    let x = 2.0 * fc * t;
                    let sinc = if x.abs() < 1e-12 {
                        1.0
                    } else {
                        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
                    };
                    let w = (t / half as f64).clamp(-1.0, 1.0);
                    let window = 0.42
                        + 0.5 * (std::f64::consts::PI * w).cos()
                        + 0.08 * (2.0 * std::f64::consts::PI * w).cos();
                    2.0 * fc * sinc * window
                })
                .collect();
            let sum: f64 = row.iter().sum();
            table.extend(row.iter().map(|v| (v / sum) as f32));
        }
        Self {
            passthrough: from == to,
            from: u64::from(from.max(1)),
            to: u64::from(to.max(1)),
            half,
            table,
            buf: vec![0.0; half],
            base: 0,
            produced: 0,
        }
    }

    /// The delay the filter adds, in input samples.
    #[cfg(test)]
    fn delay(&self) -> usize {
        self.half
    }

    /// Appends the resampled `input` to `out`. This runs on the voice thread, so it may allocate.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        let taps = 2 * self.half;
        loop {
            // Output `n` sits at input position `n * from / to`: a whole part and a remainder.
            let num = self.produced * self.from;
            let whole = num / self.to;
            let rem = num % self.to;
            // Its taps start at buffer index `whole + 1 - base` (the buffer begins with `half` zeros).
            let start = (whole + 1 - self.base) as usize;
            if start + taps > self.buf.len() {
                break;
            }
            let phase = ((rem * PHASES as u64) / self.to) as usize;
            let row = &self.table[phase * taps..(phase + 1) * taps];
            let acc: f32 = row
                .iter()
                .zip(&self.buf[start..start + taps])
                .map(|(w, x)| w * x)
                .sum();
            out.push(acc);
            self.produced += 1;
        }
        // Forget input that no later output can reach.
        let next = (self.produced * self.from) / self.to + 1;
        let drop = ((next - self.base) as usize).min(self.buf.len());
        if drop > 0 {
            self.buf.drain(..drop);
            self.base += drop as u64;
        }
    }
}

/// The last [`PRE_ROLL_SECONDS`] of 16 kHz audio, in RAM only. Fixed size: it never grows and is
/// never written anywhere.
pub struct PreRoll {
    buf: VecDeque<f32>,
    cap: usize,
}

impl PreRoll {
    pub fn new() -> Self {
        let cap = (STT_RATE * PRE_ROLL_SECONDS) as usize;
        Self {
            buf: VecDeque::with_capacity(cap),
            cap,
        }
    }

    pub fn push(&mut self, samples: &[f32]) {
        let samples = &samples[samples.len().saturating_sub(self.cap)..];
        let overflow = (self.buf.len() + samples.len()).saturating_sub(self.cap);
        self.buf.drain(..overflow);
        self.buf.extend(samples);
    }

    /// A copy of what is held, oldest first.
    pub fn snapshot(&self) -> Vec<f32> {
        self.buf.iter().copied().collect()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_averaged_to_mono_and_a_full_ring_is_counted_not_waited_on() {
        let mut out = Vec::new();
        let dropped = mix_to_mono(
            &[1.0f32, 0.0, 0.5, 0.5, -1.0, 1.0],
            2,
            |s| s,
            |v| {
                out.push(v);
                true
            },
        );
        assert_eq!((dropped, out), (0, vec![0.5, 0.5, 0.0]));
        // A full ring: each refused sample is counted and the loop carries on.
        let dropped = mix_to_mono(&[0.1f32; 8], 1, |s| s, |_| false);
        assert_eq!(dropped, 8);
        // A trailing partial frame is ignored, and zero channels cannot divide by zero.
        let mut n = 0;
        mix_to_mono(
            &[1.0f32; 5],
            2,
            |s| s,
            |_| {
                n += 1;
                true
            },
        );
        assert_eq!(n, 2);
        mix_to_mono(&[1.0f32; 3], 0, |s| s, |_| true);
    }

    #[test]
    fn integer_samples_convert_through_the_given_function() {
        let mut out = Vec::new();
        mix_to_mono(
            &[i16::MAX, i16::MIN],
            1,
            |s| f32::from(s) / 32768.0,
            |v| {
                out.push(v);
                true
            },
        );
        assert!(out[0] > 0.999 && out[1] == -1.0);
    }

    fn sine(rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    #[test]
    fn resampling_gives_the_right_length_and_keeps_a_constant_level() {
        for from in [48_000u32, 44_100, 32_000, 16_000, 8_000] {
            let mut r = Resampler::new(from, STT_RATE);
            let delay = r.delay() as f64 / (f64::from(from) / f64::from(STT_RATE));
            let mut out = Vec::new();
            r.process(&vec![0.5; from as usize], &mut out);
            // One second in gives one second out, less the filter's delay.
            let want = STT_RATE as f64 - if from == STT_RATE { 0.0 } else { delay };
            assert!(
                (out.len() as f64 - want).abs() <= 2.0,
                "{from}: {}",
                out.len()
            );
            // Once the filter has filled (twice its delay) the level is steady.
            let settle = (2.0 * delay).ceil() as usize + 2;
            assert!(
                out[settle.min(out.len() - 1)..]
                    .iter()
                    .all(|v| (v - 0.5).abs() < 1e-3),
                "{from}"
            );
        }
    }

    #[test]
    fn cutting_the_input_anywhere_gives_the_same_output() {
        let input = sine(44_100, 10_000);
        let mut whole = Vec::new();
        Resampler::new(44_100, STT_RATE).process(&input, &mut whole);
        for chunk in [1usize, 7, 480, 1023] {
            let mut r = Resampler::new(44_100, STT_RATE);
            let mut cut = Vec::new();
            for c in input.chunks(chunk) {
                r.process(c, &mut cut);
            }
            assert_eq!(cut.len(), whole.len(), "chunk {chunk}");
            assert!(
                cut.iter().zip(&whole).all(|(a, b)| (a - b).abs() < 1e-5),
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn the_same_rate_passes_through_unchanged() {
        let input = sine(16_000, 100);
        let mut out = Vec::new();
        Resampler::new(16_000, 16_000).process(&input, &mut out);
        assert_eq!(out, input);
        Resampler::new(16_000, 16_000).process(&[], &mut out); // empty input is fine
    }

    /// RMS of the output after the filter has filled.
    fn settled_rms(from: u32, tone_hz: f32, amp: f32) -> f32 {
        let n = from as usize;
        let input: Vec<f32> = (0..n)
            .map(|i| amp * (i as f32 * tone_hz * std::f32::consts::TAU / from as f32).sin())
            .collect();
        let mut out = Vec::new();
        Resampler::new(from, STT_RATE).process(&input, &mut out);
        let tail = &out[out.len() / 4..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
    }

    #[test]
    fn speech_frequencies_pass_and_what_would_fold_down_is_removed() {
        let full = 1.0 / std::f32::consts::SQRT_2; // RMS of a unit sine
        for from in [48_000u32, 44_100] {
            for hz in [300.0, 1_000.0, 3_000.0, 6_000.0] {
                let r = settled_rms(from, hz, 1.0);
                assert!((r / full - 1.0).abs() < 0.03, "{from} {hz} Hz: {r}");
            }
            // These would land inside 0 to 8 kHz without the filter (12 kHz folds to 4 kHz, and so on).
            for hz in [10_000.0, 12_000.0, 16_000.0, 20_000.0] {
                let r = settled_rms(from, hz, 1.0);
                assert!(r < 0.003, "{from} {hz} Hz leaks through: {r}");
            }
        }
    }

    #[test]
    fn the_pre_roll_keeps_only_the_last_two_seconds_in_order_and_never_grows() {
        let mut p = PreRoll::new();
        let cap = (STT_RATE * PRE_ROLL_SECONDS) as usize;
        let a: Vec<f32> = (0..cap as u32 + 500).map(|i| i as f32).collect();
        p.push(&a);
        assert_eq!(p.len(), cap);
        let s = p.snapshot();
        assert_eq!((s[0], s[cap - 1]), (500.0, (cap + 499) as f32));
        // Pushing more drops the oldest first and keeps order.
        p.push(&[-1.0, -2.0]);
        let s = p.snapshot();
        assert_eq!(s.len(), cap);
        assert_eq!((s[cap - 2], s[cap - 1]), (-1.0, -2.0));
        assert_eq!(s[0], 502.0);
        // A single push larger than the whole buffer keeps its tail.
        p.push(&vec![9.0; cap * 3]);
        assert_eq!(p.len(), cap);
        p.clear();
        assert_eq!(p.len(), 0);
    }
}
