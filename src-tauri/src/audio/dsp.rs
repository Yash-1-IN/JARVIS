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

/// Linear resampler that keeps its place between chunks, so cutting the input anywhere gives the
/// same output as one long call. Linear is enough for speech at these rates (the spike used the
/// same kind through sherpa-onnx and the models were scored on it).
pub struct Resampler {
    /// Input samples consumed per output sample.
    step: f64,
    /// Position of the next output sample, in input samples, relative to `prev`.
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            step: f64::from(from.max(1)) / f64::from(to.max(1)),
            pos: 0.0,
            prev: 0.0,
        }
    }

    /// Appends the resampled `input` to `out`. This runs on the voice thread, so it may allocate.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        // The conceptual stream is [prev, input...]; `pos` indexes into it.
        let n = input.len();
        if n == 0 {
            return;
        }
        let at = |i: usize| if i == 0 { self.prev } else { input[i - 1] };
        while self.pos < n as f64 {
            let i = self.pos.floor() as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = at(i);
            let b = at(i + 1);
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= n as f64;
        self.prev = input[n - 1];
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
            let mut out = Vec::new();
            r.process(&vec![0.5; from as usize], &mut out);
            let want = STT_RATE as i64;
            assert!(
                (out.len() as i64 - want).abs() <= 2,
                "{from}: {}",
                out.len()
            );
            // After the first sample (which blends in the starting silence) the level is steady.
            assert!(out[2..].iter().all(|v| (v - 0.5).abs() < 1e-5), "{from}");
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
    fn the_same_rate_passes_through_unchanged_after_the_first_sample() {
        let input = sine(16_000, 100);
        let mut out = Vec::new();
        Resampler::new(16_000, 16_000).process(&input, &mut out);
        assert_eq!(out.len(), 100);
        assert!(
            out[1..]
                .iter()
                .zip(&input)
                .all(|(a, b)| (a - b).abs() < 1e-5)
        );
        Resampler::new(16_000, 16_000).process(&[], &mut out); // empty input is fine
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
