//! The controller ties the hotkey listener to the dictation pipeline
//! (audio → speech-to-text → text injection).
//!
//! It owns a background thread that receives [`HotkeyEvent`]s and drives the pipeline,
//! updating [`SharedState`] status as it goes:
//! - press:   start microphone capture and open the STT stream.
//! - stream:  forward audio frames; surface interim/final transcripts to the UI.
//! - release: stop capture, await the final transcript, and inject it (M5).

use std::sync::mpsc::{channel, Receiver};

use anyhow::Result;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::unbounded_channel;
use tokio::task::JoinHandle;

use crate::app::{SharedState, Status};
use crate::audio::{AudioCapture, CaptureSession};
use crate::hotkey::{self, HotkeyEvent, ListenerHandle};
use crate::inject;
use crate::secrets;
use crate::stt::{self, StreamParams, TranscriptEvent};

/// Keeps the controller's resources alive for the lifetime of the app.
pub struct Controller {
    _listener: ListenerHandle,
}

impl Controller {
    /// Starts the hotkey listener and the event-processing thread.
    pub fn start(state: SharedState) -> Result<Controller> {
        let (tx, rx) = channel::<HotkeyEvent>();

        let cfg = state.config();
        let listener = hotkey::spawn_listener(&cfg.hotkey, tx)?;

        std::thread::Builder::new()
            .name("voiceforge-controller".into())
            .spawn(move || match Session::new(state) {
                Ok(session) => session.run(rx),
                Err(e) => tracing::error!("controller failed to start: {e}"),
            })?;

        Ok(Controller {
            _listener: listener,
        })
    }
}

/// A dictation in progress: the running capture plus the STT task producing its
/// transcript.
struct ActiveCapture {
    capture: CaptureSession,
    task: JoinHandle<Result<String>>,
}

/// Per-thread controller state. The active `CaptureSession` lives here because
/// `cpal::Stream` is not `Send` and must stay on this one thread. A dedicated tokio
/// runtime drives the async STT stream.
struct Session {
    state: SharedState,
    rt: Runtime,
    active: Option<ActiveCapture>,
}

impl Session {
    fn new(state: SharedState) -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(Session {
            state,
            rt,
            active: None,
        })
    }

    fn run(mut self, rx: Receiver<HotkeyEvent>) {
        tracing::info!("controller started");
        while let Ok(event) = rx.recv() {
            match event {
                HotkeyEvent::Pressed => self.on_pressed(),
                HotkeyEvent::Released => self.on_released(),
            }
        }
        tracing::info!("controller stopped");
    }

    fn on_pressed(&mut self) {
        let cfg = self.state.config();
        if !cfg.enabled {
            tracing::debug!("hotkey pressed but dictation is disabled");
            return;
        }
        if self.active.is_some() {
            return; // already listening
        }

        // Channel from the (sync) audio callback to the (async) STT task.
        let (sink, audio_rx) = unbounded_channel::<Vec<i16>>();

        let capture = match AudioCapture::start(cfg.microphone.as_deref(), sink) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("failed to start capture: {e}");
                self.state.set_status(Status::Error(format!("Mic error: {e}")));
                return;
            }
        };

        let params = StreamParams {
            sample_rate: capture.sample_rate,
            language: cfg.language.clone(),
        };
        let api_key = secrets::get_api_key().ok().flatten();

        // Live transcript updates flow back to the UI via SharedState.
        let ev_state = self.state.clone();
        let on_event = move |ev: TranscriptEvent| match ev {
            TranscriptEvent::Interim(t) => ev_state.set_last_transcript(t),
            TranscriptEvent::Final(t) => ev_state.set_last_transcript(t),
        };

        let cfg_for_task = cfg.clone();
        let task = self
            .rt
            .spawn(async move { stt::run(&cfg_for_task, api_key, params, audio_rx, on_event).await });

        tracing::info!("listening ({} Hz)", capture.sample_rate);
        self.state.set_status(Status::Listening);
        self.active = Some(ActiveCapture { capture, task });
    }

    fn on_released(&mut self) {
        let Some(active) = self.active.take() else {
            return;
        };

        // Dropping the capture stops the mic and closes the audio channel, which tells
        // the STT task that input has ended so it can finalize.
        drop(active.capture);

        // Wait for the provider to return the final transcript.
        let result = self.rt.block_on(active.task);
        let transcript = match result {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                tracing::error!("transcription failed: {e}");
                self.state.set_status(Status::Error(short_error(&e)));
                return;
            }
            Err(e) => {
                tracing::error!("transcription task panicked: {e}");
                self.state.set_status(Status::Error("internal error".into()));
                return;
            }
        };

        let transcript = transcript.trim().to_string();
        if transcript.is_empty() {
            tracing::info!("no speech recognized");
            self.state.set_status(Status::Idle);
            return;
        }

        self.state.set_last_transcript(transcript.clone());
        self.state.set_status(Status::Injecting);
        if let Err(e) = inject::type_text(&transcript) {
            tracing::error!("text injection failed: {e}");
            self.state.set_status(Status::Error(short_error(&e)));
            return;
        }
        tracing::info!("injected {} chars", transcript.len());
        self.state.set_status(Status::Idle);
    }
}

/// Trims an error to a short, user-facing message.
fn short_error(e: &anyhow::Error) -> String {
    let s = e.to_string();
    s.lines().next().unwrap_or("error").chars().take(80).collect()
}
