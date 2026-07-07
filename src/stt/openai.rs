//! OpenAI speech-to-text via the audio transcription endpoint.
//!
//! Unlike Deepgram, OpenAI's transcription API is a single multipart HTTP request rather
//! than a streaming socket, so this provider buffers the captured PCM while the hotkey is
//! held, wraps it into an in-memory WAV on release, and POSTs it to
//! `https://api.openai.com/v1/audio/transcriptions` with the `gpt-4o-transcribe` model.
//! This fits VoiceForge's push-to-talk UX, where text is only inserted after release.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedReceiver;

use super::{StreamParams, TranscriptEvent};

const ENDPOINT: &str = "https://api.openai.com/v1/audio/transcriptions";
const MODEL: &str = "gpt-4o-transcribe";

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// Buffers all audio, transcribes it in one request, and returns the text.
pub async fn run<F>(
    api_key: String,
    params: StreamParams,
    mut audio: UnboundedReceiver<Vec<i16>>,
    mut on_event: F,
) -> Result<String>
where
    F: FnMut(TranscriptEvent) + Send + 'static,
{
    // Collect PCM until capture is dropped (hotkey released).
    let mut samples: Vec<i16> = Vec::new();
    while let Some(frame) = audio.recv().await {
        samples.extend_from_slice(&frame);
    }

    if samples.is_empty() {
        return Ok(String::new());
    }

    let secs = samples.len() as f32 / params.sample_rate.max(1) as f32;
    tracing::info!("transcribing {secs:.2}s of audio with OpenAI ({MODEL})");

    let wav = encode_wav(&samples, params.sample_rate);

    let file_part = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .context("building audio part")?;

    let mut form = reqwest::multipart::Form::new()
        .text("model", MODEL)
        .part("file", file_part);

    // OpenAI expects an ISO-639-1 code (e.g. "en"), so drop any region suffix.
    let lang = params.language.split('-').next().unwrap_or("").to_string();
    if !lang.is_empty() {
        form = form.text("language", lang);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(ENDPOINT)
        .bearer_auth(&api_key)
        .multipart(form)
        .send()
        .await
        .context("sending request to OpenAI (check network)")?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // Surface a concise, user-meaningful error (e.g. invalid key → 401).
        let detail = extract_error_message(&body).unwrap_or_else(|| body.clone());
        return Err(anyhow!("OpenAI error {}: {}", status.as_u16(), detail));
    }

    let parsed: TranscriptionResponse =
        serde_json::from_str(&body).context("parsing OpenAI response")?;
    let transcript = parsed.text.trim().to_string();
    tracing::info!("OpenAI transcript: {:?}", transcript);
    on_event(TranscriptEvent::Final(transcript.clone()));
    Ok(transcript)
}

/// Pulls a human-readable message out of an OpenAI error JSON body, if present.
fn extract_error_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrEnvelope {
        error: ErrBody,
    }
    #[derive(Deserialize)]
    struct ErrBody {
        message: String,
    }
    serde_json::from_str::<ErrEnvelope>(body)
        .ok()
        .map(|e| e.error.message)
}

/// Encodes mono 16-bit PCM samples as a little-endian WAV byte buffer.
fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let channels: u16 = 1;
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * (bits_per_sample as u32 / 8);
    let block_align = channels * (bits_per_sample / 8);
    let data_len = (samples.len() * 2) as u32;
    let riff_len = 36 + data_len;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&riff_len.to_le_bytes());
    buf.extend_from_slice(b"WAVE");

    // fmt chunk
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    buf
}
