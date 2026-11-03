//! Commands for openWakeWord's "hey jarvis" model (see `oww.rs`): `oww-bench` (record once, try
//! many thresholds), `oww` (listen live) and `oww-selftest` (speak the phrase with the voice model
//! and check the detector hears it, no microphone needed).

use crate::{audio, models, oww, wakebench, Args};
use sherpa_onnx::{GenerationConfig, LinearResampler};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const MODEL: &str = "hey_jarvis_v0.1.onnx";
/// Two seconds, in 80 ms hops: one spoken phrase is one event.
const REFRACTORY_HOPS: usize = 25;

fn parse_list(s: &str) -> Result<Vec<f32>, String> {
    s.split(',')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(|x| {
            x.parse::<f32>()
                .map_err(|_| format!("{x:?} is not a number"))
        })
        .collect()
}

fn detector(a: &Args) -> Result<oww::Detector, String> {
    oww::Detector::load(
        &PathBuf::from(a.need("models-dir")?),
        a.get("model").unwrap_or(MODEL),
    )
}

pub fn bench(a: &Args) -> Result<(), String> {
    let mut det = detector(a)?;
    let thresholds = parse_list(
        a.get("thresholds")
            .unwrap_or("0.7,0.5,0.4,0.3,0.2,0.1,0.05"),
    )?;
    let (positives, negative) = wakebench::collect(a)?;
    let neg_secs = negative.len() as f64 / 16_000.0;
    let (n_plain, n_hey) = (
        positives.iter().filter(|t| t.label != "hey jarvis").count(),
        positives.iter().filter(|t| t.label == "hey jarvis").count(),
    );
    println!(
        "\n{} positive takes ({n_plain} plain, {n_hey} \"hey\"), {neg_secs:.0} s of background. Scoring...\n",
        positives.len()
    );
    let started = Instant::now();
    let mut peaks: Vec<(&'static str, f32)> = Vec::new();
    for t in &positives {
        peaks.push((
            t.label,
            oww::peak(&oww::scores(&mut det, &t.samples, true)?),
        ));
    }
    let bg = oww::scores(&mut det, &negative, false)?;
    let busy = started.elapsed().as_secs_f64();
    println!("peak score of each take (0 to 1; the model thinks it heard the phrase near 1):");
    for (i, (label, p)) in peaks.iter().enumerate() {
        println!("  take {:>2} {:<10} {p:.3}", i + 1, label);
    }
    println!("highest score in the background: {:.3}\n", oww::peak(&bg));
    println!(
        "{:>9}  {:>11} {:>11}  {:>12}",
        "threshold", "hits plain", "hits hey", "false trig."
    );
    for &th in &thresholds {
        let hit = |hey: bool| {
            peaks
                .iter()
                .filter(|(l, p)| (*l == "hey jarvis") == hey && *p >= th)
                .count()
        };
        println!(
            "{th:>9}  {:>5}/{n_plain:<5} {:>5}/{n_hey:<5}  {:>12}",
            hit(false),
            hit(true),
            oww::events(&bg, th, REFRACTORY_HOPS)
        );
    }
    println!(
        "\nfalse trig. = separate times the score reached the threshold in {neg_secs:.0} s of background (the spec limit is\none per 4 hours). Scoring all audio took {busy:.1} s on this computer."
    );
    Ok(())
}

pub fn live(a: &Args) -> Result<(), String> {
    let mut det = detector(a)?;
    let threshold: f32 = a.num("threshold", 0.5)?;
    let secs: f32 = a.num("seconds", 120.0)?;
    let cap = audio::Capture::start(a.get("device"))?;
    println!(
        "listening on {:?} for {secs} s; fires at score {threshold} or more. Say \"hey Jarvis\".",
        cap.device_name
    );
    let started = Instant::now();
    let end = started + Duration::from_secs_f32(secs);
    let (mut quiet, mut busy) = (REFRACTORY_HOPS, Duration::ZERO);
    while Instant::now() < end {
        let Ok(chunk) = cap.rx.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let t = Instant::now();
        let scores = det.feed(&chunk)?;
        busy += t.elapsed();
        for s in scores {
            if s >= threshold && quiet >= REFRACTORY_HOPS {
                println!(
                    "{:7.1}s  WAKE  (score {s:.2})",
                    started.elapsed().as_secs_f64()
                );
            }
            quiet = if s >= threshold {
                0
            } else {
                quiet.saturating_add(1)
            };
        }
    }
    println!(
        "busy {:.1}% of wall time on this thread",
        100.0 * busy.as_secs_f64() / started.elapsed().as_secs_f64()
    );
    Ok(())
}

/// Speaks a few phrases with the voice model and prints the peak wake-word score for each, so the
/// detector can be checked without a microphone.
pub fn selftest(a: &Args) -> Result<(), String> {
    let mut det = detector(a)?;
    let tts = models::tts(&PathBuf::from(a.need("tts-dir")?), 2, "cpu")?;
    let cfg = GenerationConfig {
        sid: a.num("sid", 26)?,
        ..Default::default()
    };
    let rs = LinearResampler::create(tts.sample_rate(), audio::STT_RATE).ok_or("resampler")?;
    println!("{:<22} peak score", "spoken phrase");
    let phrases: Vec<String> = match a.get("phrases") {
        Some(p) => p.split('|').map(str::to_string).collect(),
        None => [
            "Hey Jarvis.",
            "Hey Siri.",
            "Good morning, sir.",
            "What time is it?",
        ]
        .map(String::from)
        .to_vec(),
    };
    for phrase in phrases.iter().map(String::as_str) {
        let out = tts
            .generate_with_config(phrase, &cfg, None::<fn(&[f32], f32) -> bool>)
            .ok_or("the voice produced no audio")?;
        let samples = rs.resample(out.samples(), true);
        let p = oww::peak(&oww::scores(&mut det, &samples, true)?);
        println!("{phrase:<22} {p:.3}");
    }
    Ok(())
}
