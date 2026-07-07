//! cpal-based microphone capture.

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, StreamConfig};
use tokio::sync::mpsc::UnboundedSender;

/// Sink for captured mono 16-bit PCM frames. A tokio unbounded sender is used so the
/// (synchronous, real-time) cpal callback can hand frames to the async STT task without
/// blocking.
pub type FrameSink = UnboundedSender<Vec<i16>>;

/// Lists the names of available input (microphone) devices.
pub fn enumerate_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.input_devices() {
        Ok(devices) => devices.filter_map(|d| d.name().ok()).collect(),
        Err(e) => {
            tracing::warn!("could not enumerate input devices: {e}");
            Vec::new()
        }
    }
}

/// A live capture session. Dropping it stops the microphone stream (which drops the
/// sink, signalling end-of-input to the consumer).
///
/// Note: `cpal::Stream` is not `Send`, so a `CaptureSession` must be created and dropped
/// on the same thread (the controller thread).
pub struct CaptureSession {
    /// The sample rate of the captured audio, in Hz.
    pub sample_rate: u32,
    _stream: Stream,
}

/// Entry point for starting microphone capture.
pub struct AudioCapture;

impl AudioCapture {
    /// Starts capturing from the named device (or the system default if `None`),
    /// forwarding mono 16-bit PCM frames to `sink`.
    pub fn start(device_name: Option<&str>, sink: FrameSink) -> Result<CaptureSession> {
        let host = cpal::default_host();
        let device = pick_device(&host, device_name)?;
        let name = device.name().unwrap_or_else(|_| "unknown".into());

        let default_cfg = device
            .default_input_config()
            .context("querying default input config")?;
        let sample_format = default_cfg.sample_format();
        let channels = default_cfg.channels() as usize;
        let sample_rate = default_cfg.sample_rate().0;
        let config: StreamConfig = default_cfg.into();

        tracing::info!(
            "starting capture on \"{name}\": {sample_rate} Hz, {channels} ch, {:?}",
            sample_format
        );

        let err_fn = |e| tracing::error!("audio stream error: {e}");

        // Build a stream whose callback down-mixes to mono i16 and forwards frames.
        let stream = match sample_format {
            SampleFormat::F32 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[f32], _| {
                        let _ = sink.send(downmix_f32(data, channels));
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::I16 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[i16], _| {
                        let _ = sink.send(downmix_i16(data, channels));
                    },
                    err_fn,
                    None,
                )?
            }
            SampleFormat::U16 => {
                let sink = sink.clone();
                device.build_input_stream(
                    &config,
                    move |data: &[u16], _| {
                        let _ = sink.send(downmix_u16(data, channels));
                    },
                    err_fn,
                    None,
                )?
            }
            other => return Err(anyhow!("unsupported sample format: {other:?}")),
        };

        stream.play().context("starting the input stream")?;

        Ok(CaptureSession {
            sample_rate,
            _stream: stream,
        })
    }
}

fn pick_device(host: &cpal::Host, name: Option<&str>) -> Result<Device> {
    if let Some(name) = name {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(dev) = devices.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                return Ok(dev);
            }
        }
        tracing::warn!("microphone \"{name}\" not found; falling back to default");
    }
    host.default_input_device()
        .ok_or_else(|| anyhow!("no input (microphone) device available"))
}

// --- Down-mixing helpers: interleaved multi-channel → mono i16 ---

fn downmix_f32(data: &[f32], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.iter().map(|&s| f32_to_i16(s)).collect();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: f32 = frame.iter().copied().sum();
            f32_to_i16(sum / channels as f32)
        })
        .collect()
}

fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.to_vec();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| s as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

fn downmix_u16(data: &[u16], channels: usize) -> Vec<i16> {
    let to_i16 = |s: u16| (s as i32 - 32768) as i16;
    if channels <= 1 {
        return data.iter().map(|&s| to_i16(s)).collect();
    }
    data.chunks(channels)
        .map(|frame| {
            let sum: i32 = frame.iter().map(|&s| to_i16(s) as i32).sum();
            (sum / channels as i32) as i16
        })
        .collect()
}

fn f32_to_i16(s: f32) -> i16 {
    (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}
