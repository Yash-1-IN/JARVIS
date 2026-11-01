//! `cpal` capture and playback for the spike.
//!
//! Spike shortcuts the real implementation must not copy: the input callback allocates a
//! `Vec` and uses a std channel. Product code needs a pre-allocated lock-free ring buffer
//! (CLAUDE.md: nothing allocates or locks inside the audio callback). Audio stays in RAM;
//! nothing here writes a file.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use sherpa_onnx::LinearResampler;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const STT_RATE: i32 = 16_000;

pub fn list_devices() -> Result<(), String> {
    let host = cpal::default_host();
    println!("host: {}", host.id().name());
    let show = |kind: &str, devs: Result<Vec<cpal::Device>, _>, default: Option<String>| {
        println!("{kind}:");
        match devs {
            Ok(devs) => {
                for d in devs {
                    let name = d.name().unwrap_or_else(|_| "?".into());
                    let mark = if default.as_deref() == Some(name.as_str()) {
                        " (default)"
                    } else {
                        ""
                    };
                    println!("  {name}{mark}");
                }
            }
            Err(e) => println!("  cannot list: {e}"),
        }
    };
    let din = host.default_input_device().and_then(|d| d.name().ok());
    let dout = host.default_output_device().and_then(|d| d.name().ok());
    show("inputs", host.input_devices().map(|i| i.collect()), din);
    show("outputs", host.output_devices().map(|i| i.collect()), dout);
    Ok(())
}

fn pick_input(name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();
    if let Some(want) = name {
        let devs = host.input_devices().map_err(|e| e.to_string())?;
        for d in devs {
            if d.name().is_ok_and(|n| n.contains(want)) {
                return Ok(d);
            }
        }
        return Err(format!("no input device matching {want:?}; run `devices`"));
    }
    host.default_input_device()
        .ok_or_else(|| "no default input device (microphone missing or access denied)".into())
}

/// A running microphone. Chunks arrive already mixed to mono and resampled to 16 kHz.
pub struct Capture {
    _stream: cpal::Stream,
    pub rx: Receiver<Vec<f32>>,
    pub device_name: String,
    pub device_rate: u32,
    pub device_channels: u16,
    pub callbacks: Arc<AtomicU64>,
    pub errors: Arc<AtomicU64>,
}

impl Capture {
    pub fn start(device: Option<&str>) -> Result<Self, String> {
        let dev = pick_input(device)?;
        let device_name = dev.name().unwrap_or_else(|_| "?".into());
        let cfg = dev
            .default_input_config()
            .map_err(|e| format!("cannot read input config (privacy setting?): {e}"))?;
        let device_rate = cfg.sample_rate().0;
        let device_channels = cfg.channels();
        let (tx, rx) = mpsc::channel::<Vec<f32>>();
        let callbacks = Arc::new(AtomicU64::new(0));
        let errors = Arc::new(AtomicU64::new(0));
        let resampler = LinearResampler::create(device_rate as i32, STT_RATE)
            .ok_or("could not create the resampler")?;
        let resampler = Arc::new(Mutex::new(resampler));
        let stream_cfg: cpal::StreamConfig = cfg.clone().into();
        let ch = device_channels as usize;

        fn build<T>(
            dev: &cpal::Device,
            cfg: &cpal::StreamConfig,
            ch: usize,
            tx: mpsc::Sender<Vec<f32>>,
            rs: Arc<Mutex<LinearResampler>>,
            callbacks: Arc<AtomicU64>,
            errors: Arc<AtomicU64>,
        ) -> Result<cpal::Stream, cpal::BuildStreamError>
        where
            T: SizedSample,
            f32: FromSample<T>,
        {
            let errs = errors.clone();
            dev.build_input_stream(
                cfg,
                move |data: &[T], _| {
                    callbacks.fetch_add(1, Ordering::Relaxed);
                    let mono: Vec<f32> = data
                        .chunks(ch)
                        .map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / ch as f32)
                        .collect();
                    let out = match rs.lock() {
                        Ok(r) => r.resample(&mono, false),
                        Err(_) => return,
                    };
                    if !out.is_empty() {
                        let _ = tx.send(out);
                    }
                },
                move |_| {
                    errs.fetch_add(1, Ordering::Relaxed);
                },
                None,
            )
        }

        let stream = match cfg.sample_format() {
            SampleFormat::F32 => build::<f32>(
                &dev,
                &stream_cfg,
                ch,
                tx,
                resampler,
                callbacks.clone(),
                errors.clone(),
            ),
            SampleFormat::I16 => build::<i16>(
                &dev,
                &stream_cfg,
                ch,
                tx,
                resampler,
                callbacks.clone(),
                errors.clone(),
            ),
            SampleFormat::U16 => build::<u16>(
                &dev,
                &stream_cfg,
                ch,
                tx,
                resampler,
                callbacks.clone(),
                errors.clone(),
            ),
            other => return Err(format!("unsupported sample format {other:?}")),
        }
        .map_err(|e| format!("cannot open the microphone: {e}"))?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Self {
            _stream: stream,
            rx,
            device_name,
            device_rate,
            device_channels,
            callbacks,
            errors,
        })
    }
}

/// Plays mono samples on the default output. Returns the delay from this call to the first
/// audio callback (a floor for "time to first sound": the OS may buffer more after it).
pub fn play(samples: &[f32], rate: u32) -> Result<Duration, String> {
    let host = cpal::default_host();
    let dev = host
        .default_output_device()
        .ok_or("no default output device")?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    let out_rate = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    // Linear resample to the device rate (spike quality is fine for a listening check).
    let ratio = rate as f64 / out_rate as f64;
    let n_out = (samples.len() as f64 / ratio) as usize;
    let data: Vec<f32> = (0..n_out)
        .map(|i| {
            let pos = i as f64 * ratio;
            let i0 = pos as usize;
            let frac = (pos - i0 as f64) as f32;
            let a = samples.get(i0).copied().unwrap_or(0.0);
            let b = samples.get(i0 + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect();
    let data = Arc::new(data);
    let pos = Arc::new(AtomicU64::new(0));
    let started = Instant::now();
    let first = Arc::new(Mutex::new(None::<Duration>));
    let done = Arc::new(AtomicBool::new(false));
    let stream_cfg: cpal::StreamConfig = cfg.clone().into();

    #[allow(clippy::too_many_arguments)] // spike: a one-off helper, not worth a struct
    fn build<T>(
        dev: &cpal::Device,
        cfg: &cpal::StreamConfig,
        ch: usize,
        data: Arc<Vec<f32>>,
        pos: Arc<AtomicU64>,
        started: Instant,
        first: Arc<Mutex<Option<Duration>>>,
        done: Arc<AtomicBool>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError>
    where
        T: SizedSample + FromSample<f32>,
    {
        dev.build_output_stream(
            cfg,
            move |out: &mut [T], _| {
                if let Ok(mut f) = first.lock() {
                    f.get_or_insert_with(|| started.elapsed());
                }
                for frame in out.chunks_mut(ch) {
                    let i = pos.fetch_add(1, Ordering::Relaxed) as usize;
                    let v = data.get(i).copied().unwrap_or(0.0);
                    if i >= data.len() {
                        done.store(true, Ordering::Relaxed);
                    }
                    for s in frame {
                        *s = T::from_sample(v);
                    }
                }
            },
            |e| eprintln!("output error: {e}"),
            None,
        )
    }

    let (d, c, p, s, f, dn) = (
        data.clone(),
        cfg.clone(),
        pos,
        started,
        first.clone(),
        done.clone(),
    );
    let _ = c;
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => build::<f32>(&dev, &stream_cfg, ch, d, p, s, f, dn),
        SampleFormat::I16 => build::<i16>(&dev, &stream_cfg, ch, d, p, s, f, dn),
        SampleFormat::U16 => build::<u16>(&dev, &stream_cfg, ch, d, p, s, f, dn),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| format!("cannot open the speakers: {e}"))?;
    stream.play().map_err(|e| e.to_string())?;
    let limit = Instant::now() + Duration::from_secs_f64(n_out as f64 / out_rate as f64 + 3.0);
    while !done.load(Ordering::Relaxed) && Instant::now() < limit {
        std::thread::sleep(Duration::from_millis(20));
    }
    let first = first.lock().ok().and_then(|g| *g).unwrap_or_default();
    Ok(first)
}
