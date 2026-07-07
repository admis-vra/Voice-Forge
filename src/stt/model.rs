//! Local Whisper model management.
//!
//! Models are GGML files stored under `<data_dir>/models`. This module knows the small
//! catalog of supported models, detects whether one is present, and downloads it once
//! (from the official whisper.cpp model repository on Hugging Face) if missing — the user
//! never downloads anything manually. After the one-time fetch, transcription is fully
//! offline.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;

use crate::config::Config;

/// A model in the catalog: its config name, on-disk file, download URL, and an
/// approximate size used as a sanity check against truncated downloads.
pub struct ModelInfo {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    /// Approximate file size in bytes (for a corruption/truncation sanity check).
    pub approx_bytes: u64,
}

/// Supported models. `base` (multilingual) is the default: a good accuracy/speed balance
/// for real-time dictation at ~142 MB. Larger/smaller entries can be added freely.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        name: "tiny",
        file: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        approx_bytes: 75_000_000,
    },
    ModelInfo {
        name: "base",
        file: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        approx_bytes: 142_000_000,
    },
    ModelInfo {
        name: "base.en",
        file: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        approx_bytes: 142_000_000,
    },
    ModelInfo {
        name: "small",
        file: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        approx_bytes: 466_000_000,
    },
];

/// Looks up a model by name, falling back to `base` for unknown names.
pub fn resolve(name: &str) -> &'static ModelInfo {
    MODELS
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name))
        .unwrap_or_else(|| MODELS.iter().find(|m| m.name == "base").unwrap())
}

/// Directory where models are stored (created if missing).
pub fn models_dir() -> Result<PathBuf> {
    let dir = Config::data_dir()?.join("models");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Full path to a model's file (whether or not it exists yet).
pub fn model_path(info: &ModelInfo) -> Result<PathBuf> {
    Ok(models_dir()?.join(info.file))
}

/// Returns true if the model file exists and is at least plausibly complete.
pub fn is_present(info: &ModelInfo) -> bool {
    match model_path(info) {
        Ok(path) => match fs::metadata(&path) {
            // Require at least ~80% of the expected size to reject truncated files.
            Ok(m) => m.len() >= info.approx_bytes / 5 * 4,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Ensures the model is present, downloading it if necessary. `on_progress` is called
/// with a human-readable status line (e.g. for the UI/log). Returns the file path.
///
/// The download streams to a temporary file and is renamed into place only on success,
/// so an interrupted download never leaves a corrupt model at the real path.
pub async fn ensure<F: FnMut(String)>(info: &ModelInfo, mut on_progress: F) -> Result<PathBuf> {
    let path = model_path(info)?;
    if is_present(info) {
        return Ok(path);
    }

    on_progress(format!("Downloading Whisper model '{}'…", info.name));
    tracing::info!("downloading model '{}' from {}", info.name, info.url);

    let tmp = path.with_extension("part");
    // Clean up any leftover partial file.
    let _ = fs::remove_file(&tmp);

    let client = reqwest::Client::new();
    let resp = client
        .get(info.url)
        .send()
        .await
        .with_context(|| format!("requesting model '{}' (internet required for first-time download)", info.name))?
        .error_for_status()
        .context("model download returned an error status")?;

    let total = resp.content_length().unwrap_or(info.approx_bytes);
    let mut file = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
    let mut downloaded: u64 = 0;
    let mut last_pct: u64 = u64::MAX;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("network error during model download")?;
        file.write_all(&chunk).context("writing model to disk")?;
        downloaded += chunk.len() as u64;
        let pct = (downloaded.saturating_mul(100) / total.max(1)).min(100);
        if pct != last_pct && pct % 5 == 0 {
            last_pct = pct;
            on_progress(format!("Downloading model '{}'… {pct}%", info.name));
            tracing::debug!("model download {pct}%");
        }
    }
    file.flush().ok();
    drop(file);

    // Sanity-check size before committing.
    let got = fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
    if got < info.approx_bytes / 5 * 4 {
        let _ = fs::remove_file(&tmp);
        return Err(anyhow!(
            "downloaded model looks incomplete ({got} bytes); please try again"
        ));
    }

    fs::rename(&tmp, &path).with_context(|| format!("finalizing {}", path.display()))?;
    on_progress(format!("Model '{}' ready", info.name));
    tracing::info!("model '{}' downloaded to {}", info.name, path.display());
    Ok(path)
}

/// Deletes a model file (used to recover from a corrupt model so it can be re-fetched).
pub fn delete(path: &Path) {
    if let Err(e) = fs::remove_file(path) {
        tracing::warn!("could not delete corrupt model {}: {e}", path.display());
    }
}
