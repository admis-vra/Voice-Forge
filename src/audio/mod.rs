//! Microphone capture built on `cpal`.
//!
//! Responsibilities:
//! - Enumerate input devices for the settings dropdown.
//! - Capture audio on demand between hotkey press and release.
//! - Deliver mono 16-bit PCM frames over a channel, tagged with the sample rate, so the
//!   speech provider can stream them without the controller knowing device details.
//!
//! We capture at the device's native sample rate and let the STT provider be told what
//! that rate is, rather than resampling in-process — this keeps the audio path simple
//! and lossless. Multi-channel input is down-mixed to mono by averaging.

pub mod capture;

pub use capture::{enumerate_input_devices, AudioCapture, CaptureSession};
