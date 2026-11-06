//! Finds where speech ends (spec §6.2: 700 ms of silence, 15 s hard cap). A plain energy detector
//! with a noise floor learned from the first 200 ms. It sits behind the same shape the Silero
//! detector from the S1 spike will have, so a better detector can replace it without touching the
//! controller. Pure: it sees samples and nothing else.

/// 20 ms of audio at 16 kHz.
pub const FRAME: usize = 320;

#[derive(Debug, Clone, Copy)]
pub struct EndpointConfig {
    /// Loud frames in a row that count as the start of speech (a key click is shorter).
    pub min_speech_frames: u32,
    /// Quiet frames in a row after speech that end it: 35 frames are 700 ms.
    pub end_silence_frames: u32,
    /// The hard cap on one request: 750 frames are 15 s.
    pub max_frames: u32,
    /// Give up if no speech starts within this many frames: 300 frames are 6 s.
    pub no_speech_frames: u32,
    /// The first frames used to learn the background level: 10 frames are 200 ms.
    pub floor_frames: u32,
    /// Speech must be this much louder than the background.
    pub floor_multiplier: f32,
    /// The quietest level that can count as speech (RMS, full scale is 1.0).
    pub min_threshold: f32,
    /// The loudest the threshold may rise to, so speaking from the first moment still counts.
    pub max_threshold: f32,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            min_speech_frames: 3,
            end_silence_frames: 35,
            max_frames: 750,
            no_speech_frames: 300,
            floor_frames: 10,
            floor_multiplier: 3.0,
            min_threshold: 0.015,
            max_threshold: 0.05,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Continue,
    /// Speech started and then 700 ms of silence followed.
    SpeechEnded,
    /// The 15 s cap was reached while speech was going on.
    TooLong,
    /// Nothing that sounds like speech arrived within the time allowed.
    NoSpeech,
}

pub struct Endpointer {
    cfg: EndpointConfig,
    carry: Vec<f32>,
    frames: u32,
    floor_sum: f32,
    floor_n: u32,
    threshold: f32,
    loud_run: u32,
    quiet_run: u32,
    speech: bool,
    done: Option<Decision>,
    /// Held: the person is holding the key, so a pause never ends the capture (#143). Only the cap,
    /// the "no speech" limit and the caller's own stop do. `release_hold` switches to auto.
    held: bool,
    /// Sum and count of the levels of frames louder than the threshold since speech began: the
    /// average level of what was counted as speech, for the log line.
    speech_sum: f32,
    speech_n: u32,
}

/// What the detector learned, for the `voice: capture ended` log line (numbers only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndpointStats {
    /// The background level learned from the first frames; `None` before they have all arrived.
    pub floor: Option<f32>,
    /// The level a frame must exceed to count as speech.
    pub threshold: f32,
    /// The average level of the frames counted as speech; `None` if there were none.
    pub speech_level: Option<f32>,
}

fn rms(frame: &[f32]) -> f32 {
    (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
}

impl Endpointer {
    pub fn new(cfg: EndpointConfig) -> Self {
        Self {
            threshold: cfg.min_threshold,
            cfg,
            carry: Vec::with_capacity(FRAME),
            frames: 0,
            floor_sum: 0.0,
            floor_n: 0,
            loud_run: 0,
            quiet_run: 0,
            speech: false,
            done: None,
            held: false,
            speech_sum: 0.0,
            speech_n: 0,
        }
    }

    /// Starts in the held state (a push-to-talk capture): a pause does not end it.
    pub fn with_held(mut self, held: bool) -> Self {
        self.held = held;
        self
    }

    pub fn is_held(&self) -> bool {
        self.held
    }

    /// The key was released after a tap: from now on a pause may end the capture. Silence that was
    /// already heard counts, so if 700 ms or more of quiet has passed since the last speech, this
    /// returns `SpeechEnded` at once. Otherwise `Continue` (or the decision already reached).
    pub fn release_hold(&mut self) -> Decision {
        if let Some(d) = self.done {
            return d;
        }
        self.held = false;
        if self.speech && self.quiet_run >= self.cfg.end_silence_frames {
            self.done = Some(Decision::SpeechEnded);
            self.carry.clear();
            return Decision::SpeechEnded;
        }
        Decision::Continue
    }

    pub fn stats(&self) -> EndpointStats {
        EndpointStats {
            floor: (self.floor_n >= self.cfg.floor_frames && self.floor_n > 0)
                .then(|| self.floor_sum / self.floor_n as f32),
            threshold: self.threshold,
            speech_level: (self.speech_n > 0).then(|| self.speech_sum / self.speech_n as f32),
        }
    }

    /// Whether speech has started (so a key release can still use what was said).
    pub fn has_speech(&self) -> bool {
        self.speech
    }

    /// Feeds samples in any chunk size. Once a final decision is reached it keeps returning it.
    pub fn push(&mut self, samples: &[f32]) -> Decision {
        if let Some(d) = self.done {
            return d;
        }
        self.carry.extend_from_slice(samples);
        let mut start = 0;
        while self.carry.len() - start >= FRAME {
            let level = rms(&self.carry[start..start + FRAME]);
            start += FRAME;
            if let Some(d) = self.frame(level) {
                self.done = Some(d);
                self.carry.clear();
                return d;
            }
        }
        self.carry.drain(..start);
        Decision::Continue
    }

    fn frame(&mut self, level: f32) -> Option<Decision> {
        self.frames += 1;
        if self.floor_n < self.cfg.floor_frames {
            self.floor_sum += level;
            self.floor_n += 1;
            if self.floor_n == self.cfg.floor_frames {
                let floor = self.floor_sum / self.floor_n as f32;
                self.threshold = (floor * self.cfg.floor_multiplier)
                    .clamp(self.cfg.min_threshold, self.cfg.max_threshold);
            }
        }
        let loud = level > self.threshold;
        if self.speech {
            self.quiet_run = if loud { 0 } else { self.quiet_run + 1 };
            if loud {
                self.speech_sum += level;
                self.speech_n += 1;
            }
            if self.quiet_run >= self.cfg.end_silence_frames && !self.held {
                return Some(Decision::SpeechEnded);
            }
            if self.frames >= self.cfg.max_frames {
                return Some(Decision::TooLong);
            }
        } else {
            self.loud_run = if loud { self.loud_run + 1 } else { 0 };
            if self.loud_run >= self.cfg.min_speech_frames {
                self.speech = true;
                self.quiet_run = 0;
            } else if self.frames >= self.cfg.no_speech_frames {
                return Some(Decision::NoSpeech);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ms` milliseconds of a 220 Hz tone at the given peak level.
    fn tone(ms: usize, amp: f32) -> Vec<f32> {
        (0..ms * 16)
            .map(|i| amp * (i as f32 * 220.0 * std::f32::consts::TAU / 16_000.0).sin())
            .collect()
    }

    fn feed(e: &mut Endpointer, audio: &[f32], chunk: usize) -> Decision {
        let mut last = Decision::Continue;
        for c in audio.chunks(chunk) {
            last = e.push(c);
            if last != Decision::Continue {
                break;
            }
        }
        last
    }

    #[test]
    fn speech_followed_by_700_ms_of_quiet_ends_and_not_before() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut a = tone(300, 0.002); // background
        a.extend(tone(800, 0.3)); // speech
        a.extend(tone(660, 0.002)); // not yet 700 ms of quiet
        assert_eq!(feed(&mut e, &a, 480), Decision::Continue);
        assert!(e.has_speech());
        assert_eq!(e.push(&tone(100, 0.002)), Decision::SpeechEnded);
        // A decision, once reached, stays.
        assert_eq!(e.push(&tone(2000, 0.3)), Decision::SpeechEnded);
    }

    #[test]
    fn the_result_does_not_depend_on_how_the_audio_is_cut_into_chunks() {
        let mut a = tone(300, 0.002);
        a.extend(tone(500, 0.3));
        a.extend(tone(900, 0.002));
        for chunk in [1usize, 7, 320, 321, 1000, 16_000] {
            let mut e = Endpointer::new(EndpointConfig::default());
            assert_eq!(
                feed(&mut e, &a, chunk),
                Decision::SpeechEnded,
                "chunk {chunk}"
            );
        }
    }

    #[test]
    fn a_short_click_is_not_speech_and_silence_times_out_after_six_seconds() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut a = tone(200, 0.002);
        a.extend(tone(20, 0.5)); // one loud frame: a key click
        a.extend(tone(5_700, 0.002)); // 5.92 s in all: not yet six seconds
        assert_eq!(feed(&mut e, &a, 640), Decision::Continue);
        assert!(!e.has_speech());
        assert_eq!(e.push(&tone(200, 0.002)), Decision::NoSpeech);
    }

    #[test]
    fn talking_without_a_break_is_cut_at_fifteen_seconds() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut a = tone(200, 0.002);
        a.extend(tone(15_000, 0.3));
        assert_eq!(feed(&mut e, &a, 800), Decision::TooLong);
    }

    #[test]
    fn a_noisy_room_raises_the_bar_so_noise_alone_is_not_speech() {
        let mut e = Endpointer::new(EndpointConfig::default());
        // Steady hum at 0.012 RMS-ish for the whole time: the floor is learned, nothing starts.
        let hum = tone(7_000, 0.017);
        assert_eq!(feed(&mut e, &hum, 640), Decision::NoSpeech);
        assert!(!e.has_speech());
    }

    #[test]
    fn speaking_from_the_very_first_moment_still_counts() {
        let mut e = Endpointer::new(EndpointConfig::default());
        let mut a = tone(600, 0.3); // the floor is learned from speech, so the threshold is capped
        a.extend(tone(900, 0.001));
        assert_eq!(feed(&mut e, &a, 320), Decision::SpeechEnded);
    }

    #[test]
    fn a_held_capture_is_not_ended_by_a_pause_but_keeps_the_audio_and_the_cap() {
        let mut e = Endpointer::new(EndpointConfig::default()).with_held(true);
        let mut a = tone(300, 0.002); // background
        a.extend(tone(800, 0.3)); // speech
        a.extend(tone(2_000, 0.002)); // a long pause: 2 s, well past 700 ms
        assert_eq!(feed(&mut e, &a, 480), Decision::Continue);
        assert!(e.has_speech() && e.is_held());
        // Speech resumes, and then the 15 s cap still ends it.
        assert_eq!(e.push(&tone(500, 0.3)), Decision::Continue);
        assert_eq!(e.push(&tone(12_000, 0.3)), Decision::TooLong);
    }

    #[test]
    fn a_held_capture_with_no_speech_still_times_out() {
        let mut e = Endpointer::new(EndpointConfig::default()).with_held(true);
        assert_eq!(e.push(&tone(6_500, 0.002)), Decision::NoSpeech);
    }

    #[test]
    fn a_tap_after_a_long_pause_ends_at_once_and_after_a_short_one_waits_for_the_rest() {
        // Long pause already heard: the release ends it with no further delay.
        let mut e = Endpointer::new(EndpointConfig::default()).with_held(true);
        let mut a = tone(300, 0.002);
        a.extend(tone(800, 0.3));
        a.extend(tone(900, 0.002));
        assert_eq!(feed(&mut e, &a, 480), Decision::Continue);
        assert_eq!(e.release_hold(), Decision::SpeechEnded);
        assert!(!e.is_held());
        // A decision, once reached, stays.
        assert_eq!(e.push(&tone(1_000, 0.3)), Decision::SpeechEnded);
        // Short pause: it carries on as an auto capture and ends when the 700 ms is complete.
        let mut e = Endpointer::new(EndpointConfig::default()).with_held(true);
        let mut a = tone(300, 0.002);
        a.extend(tone(800, 0.3));
        a.extend(tone(300, 0.002));
        assert_eq!(feed(&mut e, &a, 480), Decision::Continue);
        assert_eq!(e.release_hold(), Decision::Continue);
        assert_eq!(e.push(&tone(300, 0.002)), Decision::Continue);
        assert_eq!(e.push(&tone(200, 0.002)), Decision::SpeechEnded);
    }

    #[test]
    fn a_release_before_any_speech_changes_nothing_but_the_mode() {
        let mut e = Endpointer::new(EndpointConfig::default()).with_held(true);
        e.push(&tone(300, 0.002));
        assert_eq!(e.release_hold(), Decision::Continue);
        assert!(!e.is_held());
    }

    #[test]
    fn the_stats_say_what_the_detector_learned() {
        let mut e = Endpointer::new(EndpointConfig::default());
        assert_eq!(e.stats().floor, None);
        let mut a = tone(300, 0.004); // background learned from the first 200 ms
        a.extend(tone(600, 0.3)); // speech
        e.push(&a);
        let s = e.stats();
        assert!(s.floor.is_some_and(|f| f > 0.0 && f < 0.01), "{s:?}");
        assert!(s.threshold >= 0.015 && s.threshold <= 0.05);
        assert!(s.speech_level.is_some_and(|l| l > 0.15), "{s:?}");
    }
}
