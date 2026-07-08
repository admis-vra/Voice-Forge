//! Central application state shared between the UI thread, the tray, and (in later
//! milestones) the audio/STT/injection controller running on background threads.
//!
//! The state is deliberately small and lock-guarded so any component can read the
//! current status or update config without tight coupling.

use std::sync::{Arc, Mutex};

use crate::config::Config;

/// High-level runtime status, surfaced in the tray glyph and dashboard.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Idle in the background, waiting for the hotkey. No mic, no network.
    Idle,
    /// Hotkey held: capturing audio and streaming to the provider.
    Listening,
    /// Injecting the recognized text into the focused app.
    Injecting,
    /// Fetching a local model (e.g. first-ever Whisper use). `percent` is `None` while
    /// size is still unknown or the step has no numeric progress (e.g. "ready").
    Downloading { message: String, percent: Option<f32> },
    /// A recoverable error occurred; the message is shown to the user.
    Error(String),
}

impl Status {
    /// Short label for display.
    pub fn label(&self) -> String {
        match self {
            Status::Idle => "Idle".into(),
            Status::Listening => "Listening…".into(),
            Status::Injecting => "Typing…".into(),
            Status::Downloading { message, .. } => message.clone(),
            Status::Error(m) => format!("Error: {m}"),
        }
    }
}

/// Thread-safe handle to the application's shared state.
#[derive(Clone)]
pub struct SharedState {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    config: Config,
    status: Status,
    /// Whether an API key is present in the credential vault (cached to avoid
    /// hitting the vault on every UI frame).
    api_key_present: bool,
    /// Last transcript produced, for display on the dashboard.
    last_transcript: String,
}

impl SharedState {
    pub fn new(config: Config, api_key_present: bool) -> Self {
        SharedState {
            inner: Arc::new(Mutex::new(Inner {
                config,
                status: Status::Idle,
                api_key_present,
                last_transcript: String::new(),
            })),
        }
    }

    /// Returns a clone of the current config.
    pub fn config(&self) -> Config {
        self.inner.lock().unwrap().config.clone()
    }

    /// Replaces the config and persists it to disk.
    pub fn update_config(&self, cfg: Config) -> anyhow::Result<()> {
        cfg.save()?;
        self.inner.lock().unwrap().config = cfg;
        Ok(())
    }

    pub fn status(&self) -> Status {
        self.inner.lock().unwrap().status.clone()
    }

    pub fn set_status(&self, status: Status) {
        self.inner.lock().unwrap().status = status;
    }

    pub fn api_key_present(&self) -> bool {
        self.inner.lock().unwrap().api_key_present
    }

    pub fn set_api_key_present(&self, present: bool) {
        self.inner.lock().unwrap().api_key_present = present;
    }

    pub fn last_transcript(&self) -> String {
        self.inner.lock().unwrap().last_transcript.clone()
    }

    pub fn set_last_transcript(&self, text: String) {
        self.inner.lock().unwrap().last_transcript = text;
    }
}
