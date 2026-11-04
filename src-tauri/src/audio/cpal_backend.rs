//! The real microphone, through `cpal` (WASAPI on Windows, CoreAudio on macOS, ALSA on Linux).
//!
//! Not run in CI or in the cloud (no microphone there): it needs a manual test on real hardware.
//! Everything around it (ring, resampler, pre-roll, recovery) is tested with a fake in `capture.rs`.

use super::capture::{AudioError, Backend, Opened};
use super::dsp::mix_to_mono;
use crate::ipc::types::AudioDevices;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use rtrb::{Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// How much audio the ring holds between the callback and the voice thread, in seconds.
const RING_SECONDS: usize = 2;

fn names(devices: impl Iterator<Item = cpal::Device>) -> Vec<String> {
    let mut v: Vec<String> = devices.filter_map(|d| d.name().ok()).collect();
    v.sort();
    v.dedup();
    v
}

/// The microphones and speakers the system has, by name, and which microphone the system default
/// points at now. Can block for a moment while the OS answers, so call it off the main thread.
pub fn list_devices() -> AudioDevices {
    let host = cpal::default_host();
    AudioDevices {
        inputs: host.input_devices().map(names).unwrap_or_default(),
        outputs: host.output_devices().map(names).unwrap_or_default(),
        default_input: host.default_input_device().and_then(|d| d.name().ok()),
    }
}

pub struct CpalBackend;

fn failed(e: impl std::fmt::Display) -> AudioError {
    AudioError::Failed(e.to_string())
}

impl Backend for CpalBackend {
    fn default_input_name(&mut self) -> Option<String> {
        cpal::default_host()
            .default_input_device()
            .and_then(|d| d.name().ok())
    }

    fn open(&mut self, device: Option<&str>) -> Result<Opened, AudioError> {
        let host = cpal::default_host();
        // A saved name that is no longer there falls back to the system default, as the contract
        // says; the window shows `defaultInput` so the person can see which one is in use.
        let named = device.and_then(|want| {
            host.input_devices()
                .ok()?
                .find(|d| d.name().is_ok_and(|n| n == want))
        });
        if device.is_some() && named.is_none() {
            tracing::warn!("the chosen microphone was not found, using the system default");
        }
        let dev = named
            .or_else(|| host.default_input_device())
            .ok_or(AudioError::NoDevice)?;
        let device_name = dev.name().unwrap_or_else(|_| "?".into());
        let cfg = dev.default_input_config().map_err(|e| match e {
            cpal::DefaultStreamConfigError::DeviceNotAvailable => AudioError::NoDevice,
            other => failed(other),
        })?;
        let rate = cfg.sample_rate().0;
        let channels = usize::from(cfg.channels());
        let (producer, ring) = RingBuffer::<f32>::new(rate as usize * RING_SECONDS);
        let lost = Arc::new(AtomicBool::new(false));
        let overruns = Arc::new(AtomicU64::new(0));
        let stream_cfg: cpal::StreamConfig = cfg.clone().into();
        let (l, o) = (lost.clone(), overruns.clone());
        let stream = match cfg.sample_format() {
            SampleFormat::F32 => build::<f32>(&dev, &stream_cfg, channels, producer, o, l),
            SampleFormat::I16 => build::<i16>(&dev, &stream_cfg, channels, producer, o, l),
            SampleFormat::U16 => build::<u16>(&dev, &stream_cfg, channels, producer, o, l),
            other => return Err(failed(format!("unsupported sample format {other:?}"))),
        }
        .map_err(|e| match e {
            cpal::BuildStreamError::DeviceNotAvailable => AudioError::NoDevice,
            other => failed(other),
        })?;
        stream.play().map_err(failed)?;
        Ok(Opened {
            guard: Box::new(stream),
            rate,
            device_name,
            ring,
            lost,
            overruns,
        })
    }
}

/// Builds the input stream. The data callback runs on the audio thread: it captures only a
/// pre-allocated ring producer and two atomics, and it never allocates, locks or logs.
fn build<T>(
    dev: &cpal::Device,
    cfg: &cpal::StreamConfig,
    channels: usize,
    mut producer: Producer<f32>,
    overruns: Arc<AtomicU64>,
    lost: Arc<AtomicBool>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    dev.build_input_stream(
        cfg,
        move |data: &[T], _| {
            let dropped = mix_to_mono(
                data,
                channels,
                |s| s.to_sample::<f32>(),
                |v| producer.push(v).is_ok(),
            );
            if dropped > 0 {
                overruns.fetch_add(u64::from(dropped), Ordering::Relaxed);
            }
        },
        // Any stream error means the device cannot be trusted any more: the capture thread closes
        // this stream and opens a new one. No logging here; the capture thread logs the loss.
        move |_| lost.store(true, Ordering::Release),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_devices_never_panics_even_with_no_audio_hardware() {
        // CI runners and cloud sessions have no microphone: empty lists are a correct answer.
        let d = list_devices();
        if let Some(def) = &d.default_input {
            assert!(!def.is_empty());
        }
        assert!(
            d.inputs.windows(2).all(|w| w[0] < w[1]),
            "sorted and unique"
        );
    }
}
