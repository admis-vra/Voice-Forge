//! VoiceForge — universal voice-typing utility.
//!
//! Speech → Text → Type. Runs in the system tray, listens while a global hotkey is
//! held, streams audio to a speech-to-text provider, and injects the recognized text
//! into whatever application currently has keyboard focus.
//!
//! M1 milestone: starts the tray icon and a hidden settings window (egui). The window
//! is shown from the tray and hidden (not quit) on close. Audio/STT/injection are added
//! in later milestones.

// Hide the console window on Windows release builds (keep it in debug for logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod audio;
mod autostart;
mod config;
mod controller;
mod hotkey;
mod icon;
mod inject;
mod logging;
mod secrets;
mod stt;
mod tray;
mod ui;

use anyhow::Result;

use app::SharedState;
use config::Config;

fn main() -> Result<()> {
    // Keep the log writer guard alive for the whole program.
    let _log_guard = logging::init()?;

    tracing::info!("VoiceForge starting (v{})", env!("CARGO_PKG_VERSION"));

    // One-shot helper: `voiceforge set-key <API_KEY>` stores the key in the OS
    // credential vault (the same entry the running app reads) and exits. Useful for
    // scripted setup; day-to-day, paste the key into the Settings window instead.
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "set-key" {
        secrets::set_api_key(&args[2])?;
        println!("API key stored in Windows Credential Manager.");
        return Ok(());
    }

    let cfg = Config::load()?;
    let api_key_present = secrets::has_api_key();
    tracing::info!(
        "config: enabled={}, language={}, hotkey=\"{}\", provider={:?}, autostart={}, api_key={}",
        cfg.enabled,
        cfg.language,
        cfg.hotkey,
        cfg.provider,
        cfg.autostart,
        api_key_present
    );

    let state = SharedState::new(cfg.clone(), api_key_present);

    // For the local Whisper provider, prepare the model in the background at startup
    // (download-if-missing + warm the context) so the first dictation is instant. This
    // is best-effort and never blocks or fails startup.
    if cfg.provider == config::Provider::Whisper {
        let st = state.clone();
        let model = cfg.whisper_model.clone();
        std::thread::Builder::new()
            .name("voiceforge-model-prefetch".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!("prefetch runtime failed: {e}");
                        return;
                    }
                };
                rt.block_on(stt::whisper::prefetch(&model, |s| st.set_last_transcript(s)));
            })
            .ok();
    }

    // Start the dictation controller (installs the global hotkey listener).
    let _controller = match controller::Controller::start(state.clone()) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::error!("failed to start controller/hotkey: {e}");
            None
        }
    };

    // Enumerate microphones for the settings dropdown.
    let mic_devices: Vec<String> = audio::enumerate_input_devices();
    tracing::info!("{} input device(s) found", mic_devices.len());

    // Launch the settings window hidden — VoiceForge lives in the tray.
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("VoiceForge")
            .with_inner_size([460.0, 560.0])
            .with_min_inner_size([420.0, 420.0])
            .with_visible(false),
        ..Default::default()
    };

    let ui_state = state.clone();
    let result = eframe::run_native(
        "VoiceForge",
        native_options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            Ok(Box::new(ui::UiApp::new(ui_state, ctx, mic_devices)))
        }),
    );

    if let Err(e) = result {
        tracing::error!("UI event loop error: {e}");
    }

    tracing::info!("VoiceForge exiting");
    Ok(())
}
