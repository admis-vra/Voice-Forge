//! Local Whisper speech-to-text via `whisper.cpp` (the `whisper-rs` bindings).
//!
//! This is the default, fully-offline provider. It buffers the captured audio while the
//! hotkey is held, resamples it to the 16 kHz mono float format Whisper expects, and runs
//! inference locally on release — no API key, no network (after the one-time model
//! download handled by [`super::model`]).
//!
//! The loaded model context is cached across utterances (loading a model costs hundreds
//! of milliseconds) so repeated dictation stays low-latency. Inference runs on a blocking
//! thread pool so it never stalls the async runtime.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{anyhow, Context, Result};
use tokio::sync::mpsc::UnboundedReceiver;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::model::{self, ModelInfo};
use super::{StreamParams, TranscriptEvent};

const TARGET_RATE: u32 = 16_000;

/// A loaded model, cached by its file path so we only reload when the model changes.
struct Cached {
    path: PathBuf,
    ctx: Arc<WhisperContext>,
}

fn cache() -> &'static Mutex<Option<Cached>> {
    static CACHE: OnceLock<Mutex<Option<Cached>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Loads (or returns the cached) Whisper context for `path`. On a corrupt-model load
/// failure the file is deleted so a fresh copy can be fetched next time.
fn get_context(path: &PathBuf) -> Result<Arc<WhisperContext>> {
    let mut guard = cache().lock().unwrap();
    if let Some(c) = guard.as_ref() {
        if &c.path == path {
            return Ok(c.ctx.clone());
        }
    }

    let path_str = path.to_string_lossy().to_string();
    let ctx = WhisperContext::new_with_params(&path_str, WhisperContextParameters::default())
        .map_err(|e| {
            // A load failure usually means a truncated/corrupt file — remove it.
            tracing::error!("failed to load Whisper model ({e}); deleting it for re-download");
            model::delete(path);
            anyhow!("could not load model (it may be corrupt; it will be re-downloaded): {e}")
        })?;

    let ctx = Arc::new(ctx);
    *guard = Some(Cached {
        path: path.clone(),
        ctx: ctx.clone(),
    });
    tracing::info!("Whisper model loaded and cached");
    Ok(ctx)
}

/// Downloads (if needed) and preloads the model so the first dictation is instant.
/// Best-effort: errors are logged, not propagated, so startup never fails on this.
pub async fn prefetch(model_name: &str, mut on_status: impl FnMut(String)) {
    let info = model::resolve(model_name);
    match model::ensure(info, |s| on_status(s)).await {
        Ok(path) => {
            // Warm the context on a blocking thread.
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = get_context(&path) {
                    tracing::warn!("prefetch: model warm-up failed: {e}");
                }
            })
            .await;
        }
        Err(e) => tracing::warn!("prefetch: model not ready: {e}"),
    }
}

/// Provider entry point: buffer audio, transcribe locally, return the text.
pub async fn run<F>(
    model_name: String,
    params: StreamParams,
    mut audio: UnboundedReceiver<Vec<i16>>,
    mut on_event: F,
) -> Result<String>
where
    F: FnMut(TranscriptEvent) + Send + 'static,
{
    // Collect PCM until capture ends (hotkey released).
    let mut samples: Vec<i16> = Vec::new();
    while let Some(frame) = audio.recv().await {
        samples.extend_from_slice(&frame);
    }
    if samples.is_empty() {
        return Ok(String::new());
    }

    // Ensure the model is available (may download on first ever use).
    let info: &ModelInfo = model::resolve(&model_name);
    let path = model::ensure(info, |s| on_event(TranscriptEvent::Interim(s)))
        .await
        .context("preparing Whisper model")?;

    let audio_f32 = resample_to_16k(&samples, params.sample_rate);
    let secs = audio_f32.len() as f32 / TARGET_RATE as f32;
    tracing::info!("transcribing {secs:.2}s locally with Whisper '{}'", info.name);

    let language = normalize_lang(&params.language);

    // Run CPU-bound inference off the async runtime.
    let text = tokio::task::spawn_blocking(move || transcribe(&path, &audio_f32, language))
        .await
        .context("transcription task panicked")??;

    let text = text.trim().to_string();
    on_event(TranscriptEvent::Final(text.clone()));
    Ok(text)
}

/// Synchronous local inference. Returns the concatenated segment text.
fn transcribe(path: &PathBuf, audio: &[f32], language: Option<String>) -> Result<String> {
    let ctx = get_context(path)?;
    let mut state = ctx.create_state().context("creating Whisper state")?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    // Use a sensible number of threads without starving the rest of the system.
    let threads = std::thread::available_parallelism()
        .map(|n| (n.get().saturating_sub(1)).clamp(1, 8))
        .unwrap_or(4) as i32;
    params.set_n_threads(threads);
    params.set_translate(false);
    if let Some(lang) = language.as_deref() {
        params.set_language(Some(lang));
    }
    // Quiet: we don't want whisper.cpp printing to stdout/stderr.
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Dictation is a single short clip; suppressing blank/initial context helps latency.
    params.set_no_context(true);
    params.set_suppress_blank(true);

    state
        .full(params, audio)
        .context("running Whisper inference")?;

    let n = state.full_n_segments();
    let mut out = String::new();
    for i in 0..n {
        if let Some(segment) = state.get_segment(i) {
            if let Ok(text) = segment.to_str_lossy() {
                out.push_str(&text);
            }
        }
    }
    Ok(out)
}

/// Whisper wants a bare ISO-639-1 code (e.g. "en"); drop any region suffix. An empty or
/// "auto" language lets Whisper auto-detect.
fn normalize_lang(language: &str) -> Option<String> {
    let base = language.split('-').next().unwrap_or("").trim().to_lowercase();
    if base.is_empty() || base == "auto" {
        None
    } else {
        Some(base)
    }
}

/// Resamples mono i16 PCM at `src_rate` to mono f32 at 16 kHz using linear interpolation,
/// which is more than adequate for speech recognition. Pads very short clips to ~1 s so
/// Whisper always has enough audio to work with.
fn resample_to_16k(samples: &[i16], src_rate: u32) -> Vec<f32> {
    let mut out: Vec<f32> = if src_rate == TARGET_RATE {
        samples.iter().map(|&s| s as f32 / 32768.0).collect()
    } else {
        let ratio = TARGET_RATE as f32 / src_rate as f32;
        let out_len = ((samples.len() as f32) * ratio).ceil() as usize;
        let mut v = Vec::with_capacity(out_len);
        for i in 0..out_len {
            let src_pos = i as f32 / ratio;
            let idx = src_pos.floor() as usize;
            let frac = src_pos - idx as f32;
            let a = samples.get(idx).copied().unwrap_or(0) as f32;
            let b = samples.get(idx + 1).copied().unwrap_or(a as i16) as f32;
            v.push((a + (b - a) * frac) / 32768.0);
        }
        v
    };

    // Whisper expects a minimum amount of audio; pad short utterances with silence.
    let min_len = TARGET_RATE as usize; // 1 second
    if out.len() < min_len {
        out.resize(min_len, 0.0);
    }
    out
}
