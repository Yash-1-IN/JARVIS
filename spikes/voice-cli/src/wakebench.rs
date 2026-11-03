//! `wake-bench`: record "Jarvis" / "hey Jarvis" and some background noise once, then try many
//! sensitivity settings of the wake-word model on those same recordings and print one table.
//! Audio is kept in memory only (never written). Without a microphone, `--pos-wav` and
//! `--neg-wav` take WAV files instead.
//!
//! Two knobs matter (sherpa-onnx keyword spotter): `threshold` (lower = easier to trigger) and
//! `score` (a boost for the phrase; higher = easier to trigger). More sensitive finds more real
//! "Jarvis" and also more false triggers; the table shows both so a setting can be chosen.

use crate::{audio, bench, models, Args};
use sherpa_onnx::KeywordSpotter;
use std::io::BufRead;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) struct Take {
    pub(crate) label: &'static str,
    pub(crate) samples: Vec<f32>,
}

fn parse_list(s: &str) -> Result<Vec<f32>, String> {
    s.split(',')
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(|x| {
            x.parse::<f32>()
                .map_err(|_| format!("{x:?} is not a number"))
        })
        .collect()
}

/// How many times the model fires on `samples`, and how long the decoding took. A short tail
/// of silence lets it finish the last word.
fn detections(kws: &KeywordSpotter, samples: &[f32]) -> (usize, Duration) {
    let stream = kws.create_stream();
    let (mut hits, mut busy) = (0usize, Duration::ZERO);
    let tail = vec![0.0f32; 8_000];
    for chunk in samples.chunks(1600).chain(std::iter::once(tail.as_slice())) {
        let t = Instant::now();
        stream.accept_waveform(audio::STT_RATE, chunk);
        while kws.is_ready(&stream) {
            kws.decode(&stream);
        }
        let fired = kws
            .get_result(&stream)
            .is_some_and(|r| !r.keyword.is_empty());
        if fired {
            hits += 1;
            kws.reset(&stream);
        }
        busy += t.elapsed();
    }
    (hits, busy)
}

fn record_for(cap: &audio::Capture, secs: f32) -> Vec<f32> {
    for _ in cap.rx.try_iter() {}
    let mut out = Vec::new();
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < end {
        if let Ok(c) = cap.rx.recv_timeout(Duration::from_millis(50)) {
            out.extend_from_slice(&c);
        }
    }
    out
}

fn wait_enter(prompt: &str) -> Result<(), String> {
    print!("{prompt}");
    std::io::Write::flush(&mut std::io::stdout()).ok();
    std::io::stdin()
        .lock()
        .read_line(&mut String::new())
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Records (or loads) the positive takes and the background used by both wake-word benchmarks.
pub(crate) fn collect(a: &Args) -> Result<(Vec<Take>, Vec<f32>), String> {
    let mut positives: Vec<Take> = Vec::new();
    let mut negative: Vec<f32> = Vec::new();
    if a.get("pos-wav").is_some() || a.get("neg-wav").is_some() {
        for p in a
            .get("pos-wav")
            .unwrap_or("")
            .split(',')
            .filter(|p| !p.is_empty())
        {
            positives.push(Take {
                label: "wav",
                samples: bench::wav_16k(p)?,
            });
        }
        for p in a
            .get("neg-wav")
            .unwrap_or("")
            .split(',')
            .filter(|p| !p.is_empty())
        {
            negative.extend(bench::wav_16k(p)?);
        }
    } else {
        let cap = audio::Capture::start(a.get("device"))?;
        let n: usize = a.num("takes", 10)?;
        let window: f32 = a.num("window", 3.0)?;
        let neg_secs: f32 = a.num("neg-seconds", 60.0)?;
        println!(
            "microphone: {:?}. Audio stays in memory only.\n",
            cap.device_name
        );
        println!("PART 1: {n} short recordings of {window} s. Press Enter, then say the phrase");
        println!("clearly at your normal volume, at your normal desk distance.\n");
        for i in 0..n {
            let (label, phrase) = if i % 2 == 0 {
                ("jarvis", "Jarvis")
            } else {
                ("hey jarvis", "hey Jarvis")
            };
            wait_enter(&format!("[{}/{n}] Enter, then say:  \"{phrase}\"  ", i + 1))?;
            let samples = record_for(&cap, window);
            println!("   recorded");
            positives.push(Take { label, samples });
        }
        println!(
            "\nPART 2: {neg_secs} s of ordinary background. Talk about something else, or play"
        );
        println!("a video or music, as you normally would. Do NOT say the wake word.");
        wait_enter("Press Enter to start  ")?;
        negative = record_for(&cap, neg_secs);
        println!("   recorded\n");
    }
    Ok((positives, negative))
}

pub fn run(a: &Args) -> Result<(), String> {
    let dir = PathBuf::from(a.need("kws-dir")?);
    let keywords = a.need("keywords")?;
    let thresholds = parse_list(a.get("thresholds").unwrap_or("0.25,0.15,0.1,0.05,0.02"))?;
    let scores = parse_list(a.get("scores").unwrap_or("1,2,3,4"))?;
    if thresholds.is_empty() || scores.is_empty() {
        return Err("--thresholds and --scores need at least one number".into());
    }

    let (positives, negative) = collect(a)?;
    let neg_secs = negative.len() as f64 / 16_000.0;
    let n_jarvis = positives.iter().filter(|t| t.label != "hey jarvis").count();
    let n_hey = positives.iter().filter(|t| t.label == "hey jarvis").count();
    println!(
        "{} positive takes ({n_jarvis} plain, {n_hey} \"hey\"), {neg_secs:.0} s of background.\n",
        positives.len()
    );
    println!(
        "{:>9} {:>6}  {:>11} {:>11}  {:>12} {:>8}",
        "threshold", "score", "hits plain", "hits hey", "false trig.", "busy"
    );
    for &th in &thresholds {
        for &sc in &scores {
            let kws = models::kws(&dir, keywords, th, sc)?;
            let (mut plain, mut hey) = (0, 0);
            for t in &positives {
                let (h, _) = detections(&kws, &t.samples);
                if h > 0 {
                    if t.label == "hey jarvis" {
                        hey += 1;
                    } else {
                        plain += 1;
                    }
                }
            }
            let (false_hits, busy) = if negative.is_empty() {
                (0, Duration::ZERO)
            } else {
                detections(&kws, &negative)
            };
            println!(
                "{th:>9} {sc:>6}  {plain:>5}/{n_jarvis:<5} {hey:>5}/{n_hey:<5}  {false_hits:>12} {:>7.1}%",
                100.0 * busy.as_secs_f64() / neg_secs.max(0.001)
            );
        }
    }
    println!(
        "\nhits = takes where the phrase was found at least once. false trig. = times it fired in the\nbackground audio ({neg_secs:.0} s; the spec limit is one per 4 hours, so a few per minute here is far too many)."
    );
    println!("busy = decode time as a share of the audio length, on one thread.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_lists_parse_and_reject_junk() {
        assert_eq!(parse_list("0.25, 0.1,2").unwrap(), vec![0.25, 0.1, 2.0]);
        assert!(parse_list("").unwrap().is_empty());
        assert!(parse_list("0.1,abc").is_err());
    }
}
