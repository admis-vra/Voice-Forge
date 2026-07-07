//! Application configuration: persisted as TOML in the per-user config directory
//! (`%APPDATA%\VoiceForge\config.toml` on Windows).
//!
//! The config holds only non-secret preferences. The Deepgram API key is stored
//! separately in the OS credential vault — see [`crate::secrets`].

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

/// Which speech-to-text backend to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Local Whisper (whisper.cpp) — fully offline, no API key or internet (after the
    /// model is downloaded once). The default.
    Whisper,
    /// OpenAI audio transcription (`gpt-4o-transcribe`); requires an API key.
    Openai,
    /// Deepgram streaming API (requires an API key in the credential vault).
    Deepgram,
    /// Offline mock provider that emits fixed text — for testing without network.
    Mock,
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Whisper
    }
}

/// A push-to-talk hotkey: a set of modifiers plus a main key, described by name.
///
/// Stored as human-readable strings so the config file is easy to hand-edit and so
/// the representation is not tied to any OS-specific virtual-key numbering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hotkey {
    /// Modifier names, e.g. `["alt"]` or `["ctrl", "shift"]`.
    pub modifiers: Vec<String>,
    /// Main key name, e.g. `"space"`.
    pub key: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Hotkey {
            modifiers: vec!["alt".to_string()],
            key: "space".to_string(),
        }
    }
}

impl std::fmt::Display for Hotkey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for m in &self.modifiers {
            write!(f, "{} + ", title_case(m))?;
        }
        write!(f, "{}", title_case(&self.key))
    }
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Top-level user configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Whether dictation is globally enabled (the tray toggle).
    pub enabled: bool,
    /// Selected microphone device name, or `None` for the system default.
    pub microphone: Option<String>,
    /// BCP-47 language code passed to the STT provider, e.g. `"en"`, `"en-US"`.
    pub language: String,
    /// Push-to-talk hotkey.
    pub hotkey: Hotkey,
    /// Launch VoiceForge automatically at Windows sign-in.
    pub autostart: bool,
    /// Active speech provider.
    pub provider: Provider,
    /// Local Whisper model name (see [`crate::stt::whisper`] model registry), e.g.
    /// `"base"`, `"base.en"`, `"small"`. Used only when `provider == Whisper`.
    pub whisper_model: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            microphone: None,
            language: "en".to_string(),
            hotkey: Hotkey::default(),
            autostart: false,
            provider: Provider::default(),
            whisper_model: "base".to_string(),
        }
    }
}

impl Config {
    /// Returns the directory VoiceForge uses for config, logs, etc., creating it if
    /// necessary.
    pub fn data_dir() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "VoiceForge", "VoiceForge")
            .context("could not determine a config directory for this platform")?;
        let dir = dirs.config_dir().to_path_buf();
        fs::create_dir_all(&dir)
            .with_context(|| format!("creating config dir {}", dir.display()))?;
        Ok(dir)
    }

    /// Full path to the config file.
    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::data_dir()?.join("config.toml"))
    }

    /// Loads config from disk, falling back to defaults (and writing them) if the file
    /// does not exist yet. A corrupt file is logged and replaced with defaults rather
    /// than crashing the background process.
    pub fn load() -> Result<Config> {
        let path = Self::config_path()?;
        if !path.exists() {
            let cfg = Config::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        match toml::from_str::<Config>(&text) {
            Ok(cfg) => Ok(cfg),
            Err(e) => {
                tracing::warn!("config is corrupt ({e}); resetting to defaults");
                let cfg = Config::default();
                cfg.save()?;
                Ok(cfg)
            }
        }
    }

    /// Writes the current config to disk atomically-ish (write temp, then rename).
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let text = toml::to_string_pretty(self).context("serializing config")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))?;
        Ok(())
    }
}
