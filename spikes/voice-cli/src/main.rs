//! Spike A-08: `cpal` capture -> `sherpa-onnx` wake word, VAD, STT and TTS from the terminal,
//! with per-stage timings. Throwaway: no app integration, nothing is written to disk, and
//! the numbers it prints are the point (spec §15, S1 report).

mod audio;
mod models;

use sherpa_onnx::{GenerationConfig, LinearResampler, Wave};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const USAGE: &str = "\
voice-cli <command> [--flag value ...]

  devices                                  list microphones and speakers
  mic        [--seconds 5] [--device NAME]
  transcribe --wav FILE --vad SILERO.onnx --asr moonshine|whisper --asr-dir DIR
  listen     --vad SILERO.onnx --asr moonshine|whisper --asr-dir DIR
             [--seconds 60] [--device NAME] [--silence 0.7]
  wake       --kws-dir DIR --keywords FILE [--seconds 120] [--threshold 0.25]
             [--score 1.0] [--device NAME]    (or --wav FILE instead of the microphone)
  say        --tts-dir DIR --text TEXT [--sid 24,25,26,27] [--speed 1.0] [--no-play]
             (speaker ids differ between Kokoro versions: listen and pick)
";

struct Args(HashMap<String, String>);

impl Args {
    fn parse(rest: &[String]) -> Result<Self, String> {
        let mut m = HashMap::new();
        let mut i = 0;
        while i < rest.len() {
            let k = rest[i]
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected {:?}\n{USAGE}", rest[i]))?;
            if k == "no-play" {
                m.insert(k.into(), "1".into());
                i += 1;
                continue;
            }
            let v = rest
                .get(i + 1)
                .ok_or_else(|| format!("--{k} needs a value"))?;
            m.insert(k.into(), v.clone());
            i += 2;
        }
        Ok(Self(m))
    }
    fn get(&self, k: &str) -> Option<&str> {
        self.0.get(k).map(String::as_str)
    }
    fn need(&self, k: &str) -> Result<&str, String> {
        self.get(k).ok_or_else(|| format!("missing --{k}\n{USAGE}"))
    }
    fn num<T: std::str::FromStr>(&self, k: &str, default: T) -> Result<T, String> {
        match self.get(k) {
            None => Ok(default),
            Some(v) => v
                .parse()
                .map_err(|_| format!("--{k} {v:?} is not a number")),
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Wall time spent inside processing calls, to report a standby "CPU" proxy. It counts one
/// thread only; read the OS process monitor for the real figure (spec §10).
#[derive(Default)]
struct Busy(Duration);

impl Busy {
    fn time<T>(&mut self, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        self.0 += t.elapsed();
        r
    }
}

fn transcribe_samples(rec: &sherpa_onnx::OfflineRecognizer, samples: &[f32]) -> (String, Duration) {
    let t = Instant::now();
    let stream = rec.create_stream();
    stream.accept_waveform(audio::STT_RATE, samples);
    rec.decode(&stream);
    let text = stream
        .get_result()
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default();
    (text, t.elapsed())
}

fn cmd_mic(a: &Args) -> Result<(), String> {
    let secs: f32 = a.num("seconds", 5.0)?;
    let cap = audio::Capture::start(a.get("device"))?;
    println!(
        "mic: {:?}, {} Hz, {} channel(s) -> mono 16 kHz",
        cap.device_name, cap.device_rate, cap.device_channels
    );
    let end = Instant::now() + Duration::from_secs_f32(secs);
    let started = Instant::now();
    let (mut n, mut peak, mut sq, mut chunks) = (0usize, 0f32, 0f64, 0usize);
    while Instant::now() < end {
        if let Ok(c) = cap.rx.recv_timeout(Duration::from_millis(100)) {
            chunks += 1;
            n += c.len();
            for s in &c {
                peak = peak.max(s.abs());
                sq += f64::from(*s) * f64::from(*s);
            }
        }
    }
    let wall = started.elapsed().as_secs_f64();
    let expected = wall * 16_000.0;
    let rms = if n > 0 { (sq / n as f64).sqrt() } else { 0.0 };
    println!(
        "got {n} samples in {wall:.2}s ({:.1}% of the expected 16 kHz count)",
        100.0 * n as f64 / expected
    );
    println!(
        "callbacks {}, chunks {chunks}, stream errors {}",
        cap.callbacks.load(std::sync::atomic::Ordering::Relaxed),
        cap.errors.load(std::sync::atomic::Ordering::Relaxed)
    );
    println!("level: rms {rms:.4}, peak {peak:.3}");
    if peak == 0.0 {
        println!("WARNING: all zeros. Windows: Settings > Privacy > Microphone must allow desktop apps. macOS: the terminal needs microphone permission.");
    } else if peak < 0.01 {
        println!("note: very quiet. Speak while it runs, or raise the input level.");
    }
    Ok(())
}

fn cmd_transcribe(a: &Args) -> Result<(), String> {
    let t0 = Instant::now();
    let vad = models::vad(a.need("vad")?, a.num("silence", 0.7)?)?;
    let rec = models::asr(a.need("asr")?, &PathBuf::from(a.need("asr-dir")?), 2)?;
    println!("models loaded in {:.0} ms", ms(t0.elapsed()));
    let wav = Wave::read(a.need("wav")?).ok_or("cannot read the WAV file")?;
    let mut samples = wav.samples().to_vec();
    if wav.sample_rate() != audio::STT_RATE {
        let rs = LinearResampler::create(wav.sample_rate(), audio::STT_RATE).ok_or("resampler")?;
        samples = rs.resample(&samples, true);
    }
    let total = samples.len() as f64 / 16_000.0;
    println!("audio: {total:.1} s");
    let (mut stt_total, mut segs) = (Duration::ZERO, 0);
    let mut drain = |vad: &sherpa_onnx::VoiceActivityDetector| {
        while !vad.is_empty() {
            if let Some(seg) = vad.front() {
                let dur = seg.samples().len() as f64 / 16_000.0;
                let at = seg.start() as f64 / 16_000.0;
                let (text, took) = transcribe_samples(&rec, seg.samples());
                stt_total += took;
                segs += 1;
                println!("[{at:6.2}s +{dur:4.1}s] {:5.0} ms  {text}", ms(took));
            }
            vad.pop();
        }
    };
    for chunk in samples.chunks(1600) {
        vad.accept_waveform(chunk);
        drain(&vad);
    }
    vad.flush();
    drain(&vad);
    println!(
        "{segs} segment(s); STT {:.0} ms total, real-time factor {:.2} (lower is faster)",
        ms(stt_total),
        stt_total.as_secs_f64() / total.max(0.001)
    );
    Ok(())
}

fn cmd_listen(a: &Args) -> Result<(), String> {
    let silence: f32 = a.num("silence", 0.7)?;
    let vad = models::vad(a.need("vad")?, silence)?;
    let rec = models::asr(a.need("asr")?, &PathBuf::from(a.need("asr-dir")?), 2)?;
    let cap = audio::Capture::start(a.get("device"))?;
    let secs: f32 = a.num("seconds", 60.0)?;
    println!(
        "listening on {:?} for {secs} s. Speak; pause {silence} s to finish a request.",
        cap.device_name
    );
    let started = Instant::now();
    let end = started + Duration::from_secs_f32(secs);
    let mut busy = Busy::default();
    while Instant::now() < end {
        let Ok(chunk) = cap.rx.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        busy.time(|| vad.accept_waveform(&chunk));
        while !vad.is_empty() {
            let ready = Instant::now();
            if let Some(seg) = vad.front() {
                let dur = seg.samples().len() as f64 / 16_000.0;
                let (text, took) = transcribe_samples(&rec, seg.samples());
                busy.0 += took;
                println!(
                    "{dur:4.1} s of speech -> STT {:4.0} ms (after the {silence} s pause, text ready {:4.0} ms after end of speech)  {text}",
                    ms(took),
                    f64::from(silence) * 1000.0 + ms(ready.elapsed())
                );
            }
            vad.pop();
        }
    }
    let wall = started.elapsed();
    println!(
        "busy {:.1}% of wall time on this thread (VAD + STT)",
        100.0 * busy.0.as_secs_f64() / wall.as_secs_f64()
    );
    Ok(())
}

fn cmd_wake(a: &Args) -> Result<(), String> {
    let kws = models::kws(
        &PathBuf::from(a.need("kws-dir")?),
        a.need("keywords")?,
        a.num("threshold", 0.25)?,
        a.num("score", 1.0)?,
    )?;
    let stream = kws.create_stream();
    let mut busy = Busy::default();
    let mut hits = 0u32;
    let mut feed = |chunk: &[f32], at: f64, busy: &mut Busy| {
        let arrived = Instant::now();
        let hit = busy.time(|| {
            stream.accept_waveform(audio::STT_RATE, chunk);
            while kws.is_ready(&stream) {
                kws.decode(&stream);
            }
            kws.get_result(&stream).filter(|r| !r.keyword.is_empty())
        });
        if let Some(r) = hit {
            hits += 1;
            println!(
                "{at:7.1}s  WAKE {:?}  (decoded {:.0} ms after the audio chunk arrived)",
                r.keyword,
                ms(arrived.elapsed())
            );
            kws.reset(&stream);
        }
    };
    if let Some(path) = a.get("wav") {
        let wav = Wave::read(path).ok_or("cannot read the WAV file")?;
        let mut samples = wav.samples().to_vec();
        if wav.sample_rate() != audio::STT_RATE {
            let rs =
                LinearResampler::create(wav.sample_rate(), audio::STT_RATE).ok_or("resampler")?;
            samples = rs.resample(&samples, true);
        }
        samples.extend(std::iter::repeat_n(0.0, 8_000)); // let the model finish the last word
        for (i, chunk) in samples.chunks(1600).enumerate() {
            feed(chunk, i as f64 * 0.1, &mut busy);
        }
        println!(
            "{hits} detection(s) in {:.1} s of audio; decoding took {:.0} ms",
            samples.len() as f64 / 16_000.0,
            ms(busy.0)
        );
        return Ok(());
    }
    let cap = audio::Capture::start(a.get("device"))?;
    let secs: f32 = a.num("seconds", 120.0)?;
    println!("wake-word standby on {:?} for {secs} s. Say the phrase; stay quiet otherwise to count false triggers.", cap.device_name);
    let started = Instant::now();
    let end = started + Duration::from_secs_f32(secs);
    while Instant::now() < end {
        let Ok(chunk) = cap.rx.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        feed(&chunk, started.elapsed().as_secs_f64(), &mut busy);
    }
    let wall = started.elapsed();
    println!("{hits} detection(s) in {:.0} s", wall.as_secs_f64());
    println!(
        "busy {:.2}% of wall time on this thread (spec §10 budget: standby CPU <= 5%, target 2%)",
        100.0 * busy.0.as_secs_f64() / wall.as_secs_f64()
    );
    Ok(())
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            out.push(std::mem::take(&mut cur).trim().to_string());
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.retain(|s| !s.is_empty());
    out
}

fn cmd_say(a: &Args) -> Result<(), String> {
    let t0 = Instant::now();
    let tts = models::tts(&PathBuf::from(a.need("tts-dir")?))?;
    println!(
        "voice loaded in {:.0} ms ({} speaker id(s), {} Hz)",
        ms(t0.elapsed()),
        tts.num_speakers(),
        tts.sample_rate()
    );
    let sids: Vec<i32> = a
        .get("sid")
        .unwrap_or("26")
        .split(',')
        .map(|v| {
            v.trim()
                .parse()
                .map_err(|_| format!("--sid {v:?} is not a number"))
        })
        .collect::<Result<_, _>>()?;
    let speed: f32 = a.num("speed", 1.0)?;
    let rate = tts.sample_rate() as u32;
    let play = a.get("no-play").is_none();
    for sid in sids {
        println!("--- speaker id {sid}");
        let cfg = GenerationConfig {
            sid,
            speed,
            ..Default::default()
        };
        for (i, sentence) in split_sentences(a.need("text")?).iter().enumerate() {
            let t = Instant::now();
            let out = tts
                .generate_with_config(sentence, &cfg, None::<fn(&[f32], f32) -> bool>)
                .ok_or("the voice produced no audio")?;
            let took = t.elapsed();
            let secs = out.samples().len() as f64 / f64::from(rate);
            println!(
                "sentence {}: {:.1} s of speech in {:.0} ms (real-time factor {:.2}){}  {sentence:?}",
                i + 1,
                secs,
                ms(took),
                took.as_secs_f64() / secs.max(0.001),
                if i == 0 { "  <- time to first sound, before the speaker delay" } else { "" }
            );
            if play {
                let first = audio::play(out.samples(), rate)?;
                println!(
                    "           speaker started {:.0} ms after play()",
                    ms(first)
                );
            }
        }
    }
    Ok(())
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let result = match argv.first().map(String::as_str) {
        Some("devices") => audio::list_devices(),
        Some(cmd @ ("mic" | "transcribe" | "listen" | "wake" | "say")) => Args::parse(&argv[1..])
            .and_then(|a| match cmd {
                "mic" => cmd_mic(&a),
                "transcribe" => cmd_transcribe(&a),
                "listen" => cmd_listen(&a),
                "wake" => cmd_wake(&a),
                _ => cmd_say(&a),
            }),
        _ => Err(USAGE.into()),
    };
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_split_on_end_marks_and_keep_the_tail() {
        assert_eq!(
            split_sentences("Good evening, sir. Shall I open Chrome? Yes"),
            vec!["Good evening, sir.", "Shall I open Chrome?", "Yes"]
        );
        assert!(split_sentences("  ").is_empty());
    }

    #[test]
    fn flags_parse_and_numbers_are_checked() {
        let args: Vec<String> = ["--seconds", "7", "--no-play", "--vad", "v.onnx"]
            .map(String::from)
            .to_vec();
        let a = Args::parse(&args).unwrap();
        assert_eq!(a.num("seconds", 1.0f32).unwrap(), 7.0);
        assert_eq!(a.get("no-play"), Some("1"));
        assert_eq!(a.need("vad").unwrap(), "v.onnx");
        assert!(a.need("asr").is_err());
        assert!(Args::parse(&["bare".to_string()]).is_err());
        let bad = Args::parse(&["--seconds".to_string(), "x".to_string()]).unwrap();
        assert!(bad.num("seconds", 1.0f32).is_err());
    }
}
