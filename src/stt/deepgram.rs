//! Deepgram streaming speech-to-text over a WebSocket.
//!
//! Protocol summary:
//! - Connect to `wss://api.deepgram.com/v1/listen` with query params describing the
//!   audio (linear16, sample rate, mono) and options (language, interim results,
//!   punctuation, endpointing), authenticating with an `Authorization: Token <key>`
//!   header.
//! - Send raw little-endian PCM as binary WebSocket frames.
//! - Receive JSON `Results` messages containing interim and final transcripts.
//! - When audio ends, send `{"type":"CloseStream"}` and drain remaining results until
//!   Deepgram closes the socket.

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

use super::{frame_to_le_bytes, StreamParams, TranscriptEvent};

/// Minimal shape of a Deepgram streaming response we care about.
#[derive(Debug, Deserialize)]
struct DgResponse {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    is_final: bool,
    channel: Option<DgChannel>,
}

#[derive(Debug, Deserialize)]
struct DgChannel {
    alternatives: Vec<DgAlternative>,
}

#[derive(Debug, Deserialize)]
struct DgAlternative {
    #[serde(default)]
    transcript: String,
}

/// Streams `audio` to Deepgram and returns the aggregated final transcript.
pub async fn run<F>(
    api_key: String,
    params: StreamParams,
    mut audio: UnboundedReceiver<Vec<i16>>,
    mut on_event: F,
) -> Result<String>
where
    F: FnMut(TranscriptEvent) + Send + 'static,
{
    let url = format!(
        "wss://api.deepgram.com/v1/listen\
         ?encoding=linear16&sample_rate={sr}&channels=1\
         &language={lang}&model=nova-2&interim_results=true\
         &punctuate=true&smart_format=true&endpointing=300",
        sr = params.sample_rate,
        lang = params.language,
    );

    let mut request = url
        .into_client_request()
        .context("building Deepgram request")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Token {api_key}")
            .parse()
            .context("invalid API key characters")?,
    );

    let (ws, _resp) = tokio_tungstenite::connect_async(request)
        .await
        .context("connecting to Deepgram (check network and API key)")?;
    tracing::info!("Deepgram stream connected ({} Hz)", params.sample_rate);

    let (mut write, mut read) = ws.split();

    let mut finals: Vec<String> = Vec::new();
    let mut input_done = false;
    let mut close_sent = false;

    loop {
        tokio::select! {
            // Forward audio frames until capture is dropped.
            frame = audio.recv(), if !input_done => {
                match frame {
                    Some(frame) => {
                        let bytes = frame_to_le_bytes(&frame);
                        if let Err(e) = write.send(Message::Binary(bytes)).await {
                            tracing::warn!("Deepgram send error: {e}");
                            input_done = true;
                        }
                    }
                    None => {
                        // Capture ended: ask Deepgram to finish up.
                        input_done = true;
                        if !close_sent {
                            let _ = write
                                .send(Message::Text("{\"type\":\"CloseStream\"}".into()))
                                .await;
                            close_sent = true;
                        }
                    }
                }
            }

            // Receive transcription results.
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(done) = handle_message(&text, &mut finals, &mut on_event) {
                            if done { break; }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {} // ignore ping/pong/binary
                    Some(Err(e)) => {
                        tracing::warn!("Deepgram read error: {e}");
                        break;
                    }
                }
            }
        }
    }

    let transcript = finals.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    tracing::info!("Deepgram final transcript: {:?}", transcript);
    Ok(transcript)
}

/// Parses one Deepgram message. Returns `Some(true)` when this signals end of stream.
fn handle_message<F>(
    text: &str,
    finals: &mut Vec<String>,
    on_event: &mut F,
) -> Option<bool>
where
    F: FnMut(TranscriptEvent),
{
    let resp: DgResponse = match serde_json::from_str(text) {
        Ok(r) => r,
        Err(_) => return None,
    };

    // Metadata marks the end after CloseStream.
    if resp.kind.as_deref() == Some("Metadata") {
        return Some(true);
    }

    let transcript = resp
        .channel
        .as_ref()
        .and_then(|c| c.alternatives.first())
        .map(|a| a.transcript.clone())
        .unwrap_or_default();

    if transcript.trim().is_empty() {
        return None;
    }

    if resp.is_final {
        finals.push(transcript.clone());
        on_event(TranscriptEvent::Final(transcript));
    } else {
        on_event(TranscriptEvent::Interim(transcript));
    }
    None
}
