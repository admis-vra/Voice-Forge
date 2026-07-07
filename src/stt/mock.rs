//! Offline mock speech provider for testing the audio → inject path without a network
//! or API key.
//!
//! It consumes (and discards) the incoming audio so the pipeline behaves realistically,
//! emits a couple of interim updates, and returns a fixed transcript whose length hints
//! at how much audio was captured.

use tokio::sync::mpsc::UnboundedReceiver;

use super::{StreamParams, TranscriptEvent};

/// Drains audio and returns a deterministic placeholder transcript.
pub async fn run<F>(
    params: StreamParams,
    mut audio: UnboundedReceiver<Vec<i16>>,
    mut on_event: F,
) -> anyhow::Result<String>
where
    F: FnMut(TranscriptEvent) + Send + 'static,
{
    let mut samples: usize = 0;
    on_event(TranscriptEvent::Interim("listening…".into()));
    while let Some(frame) = audio.recv().await {
        samples += frame.len();
    }
    let secs = samples as f32 / params.sample_rate.max(1) as f32;
    let transcript = format!("this is a mock transcript of about {secs:.1} seconds of audio");
    on_event(TranscriptEvent::Final(transcript.clone()));
    Ok(transcript)
}
