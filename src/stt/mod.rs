//! Speech-to-text providers.
//!
//! A provider consumes a stream of mono 16-bit PCM frames and produces transcript
//! events (live interim text plus final segments), returning the complete aggregated
//! transcript when the audio input ends.
//!
//! Providers share the [`run`] entry point and the [`TranscriptEvent`] type; this is the
//! abstraction boundary that lets new providers be added without touching the
//! controller. Currently: [`deepgram`] (streaming API) and [`mock`] (offline testing).

pub mod deepgram;
pub mod mock;
pub mod model;
pub mod openai;
pub mod whisper;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::config::{Config, Provider};

/// An incremental transcription update.
#[derive(Debug, Clone)]
pub enum TranscriptEvent {
    /// A live, not-yet-final hypothesis (may change).
    Interim(String),
    /// A finalized segment that will not change.
    Final(String),
    /// Non-transcript progress (e.g. downloading a local model on first use). Kept
    /// separate from `Interim`/`Final` so callers don't mistake it for recognized speech.
    Progress { message: String, percent: Option<f32> },
}

/// Parameters describing the audio being streamed.
#[derive(Debug, Clone)]
pub struct StreamParams {
    pub sample_rate: u32,
    pub language: String,
}

/// Runs the provider selected in `cfg` against the incoming audio frames.
///
/// - `api_key` is required by network providers (ignored by the mock).
/// - `audio` yields PCM frames until the capture is dropped, at which point the stream
///   is closed and the provider finalizes.
/// - `on_event` is invoked for each interim/final update, e.g. to update the UI.
///
/// Returns the full aggregated transcript.
pub async fn run<F>(
    cfg: &Config,
    api_key: Option<String>,
    params: StreamParams,
    audio: UnboundedReceiver<Vec<i16>>,
    on_event: F,
) -> anyhow::Result<String>
where
    F: FnMut(TranscriptEvent) + Send + 'static,
{
    match cfg.provider {
        Provider::Whisper => {
            whisper::run(cfg.whisper_model.clone(), params, audio, on_event).await
        }
        Provider::Openai => {
            let key = api_key.ok_or_else(|| {
                anyhow::anyhow!("no OpenAI API key configured (set one in Settings)")
            })?;
            openai::run(key, params, audio, on_event).await
        }
        Provider::Deepgram => {
            let key = api_key.ok_or_else(|| {
                anyhow::anyhow!("no Deepgram API key configured (set one in Settings)")
            })?;
            deepgram::run(key, params, audio, on_event).await
        }
        Provider::Mock => mock::run(params, audio, on_event).await,
    }
}

/// Converts a mono i16 frame to little-endian bytes for linear16 transport.
pub(crate) fn frame_to_le_bytes(frame: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(frame.len() * 2);
    for &s in frame {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}
