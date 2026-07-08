//! The settings window, built with egui/eframe.
//!
//! The window normally stays hidden — VoiceForge lives in the tray. Opening "Settings…"
//! from the tray shows it; closing the window hides it again rather than quitting.
//!
//! This same eframe app also owns the winit event loop, so it is where we poll the
//! tray's menu events each frame (see [`UiApp::poll_tray`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;

use crate::app::{SharedState, Status};
use crate::config::{Config, Hotkey, Provider};
use crate::hotkey;
use crate::secrets;

/// Tabs in the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Dashboard,
    Settings,
    About,
}

/// The eframe application.
pub struct UiApp {
    state: SharedState,
    /// Set by the tray thread when the user chooses Quit, so the window's close handler
    /// lets the window actually close instead of hiding to tray.
    quitting: Arc<AtomicBool>,
    tab: Tab,

    // Editable working copy of config; committed on "Save".
    draft: Config,

    // Settings form scratch fields.
    api_key_input: String,
    api_key_saved_note: Option<String>,
    capturing_hotkey: bool,
    mic_devices: Vec<String>,
    /// True until the first frame has run; used to force-hide the window once, since
    /// `ViewportBuilder::with_visible(false)` is not always honored at creation on
    /// Windows (the window can flash visible briefly before this fires).
    first_frame: bool,
    /// Set on close_requested; the actual hide command is sent on the *next* frame,
    /// decoupled from `CancelClose`, so it isn't processed as part of the same close
    /// sequence (which could let the window get destroyed anyway).
    pending_hide: bool,
}

impl UiApp {
    /// Creates the app and spawns the tray thread (which owns the tray icon/menu).
    pub fn new(state: SharedState, egui_ctx: egui::Context, mic_devices: Vec<String>) -> Self {
        let draft = state.config();
        let quitting = Arc::new(AtomicBool::new(false));

        // Debug-only self-test: mimic a "Settings" tray click after a delay so the
        // show-window path can be verified without interacting with the tray.
        #[cfg(debug_assertions)]
        if std::env::var("VOICEFORGE_SELFTEST_SHOW").is_ok() {
            let ctx = egui_ctx.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                tracing::info!("selftest: showing window");
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                ctx.request_repaint();
            });
        }

        if let Err(e) = crate::tray::spawn(state.clone(), egui_ctx, quitting.clone()) {
            tracing::error!("failed to start tray: {e}");
        }

        UiApp {
            state,
            quitting,
            tab: Tab::Dashboard,
            draft,
            api_key_input: String::new(),
            api_key_saved_note: None,
            capturing_hotkey: false,
            mic_devices,
            first_frame: true,
            pending_hide: false,
        }
    }

    // `Visible(false)` fully unmaps the window on Windows, which stops winit from
    // pumping further frames for it — so a later `Visible(true)` command has nothing
    // left to process it and the window never comes back. `Minimized(true)` keeps the
    // window alive (just iconified), so it reliably restores; we use that to "hide" to
    // tray instead.
    fn hide_window(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

impl eframe::App for UiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // `with_visible(false)` at creation isn't always honored on Windows (the window
        // can flash visible for a frame). Force it minimized once, on the very first frame.
        if self.first_frame {
            self.first_frame = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            self.hide_window(ctx);
            ctx.request_repaint();
        }

        // Intercept the window close button: hide to tray instead of quitting — unless
        // the tray requested a real Quit, in which case let the window close.
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.quitting.load(Ordering::SeqCst) {
                // Allow the close to proceed; eframe will exit run_native.
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending_hide = true;
                ctx.request_repaint();
            }
        }

        // Send the hide command on the frame *after* CancelClose, not the same one, so
        // it isn't coalesced into the close sequence and the window survives to be
        // reopened later from the tray.
        if self.pending_hide {
            self.pending_hide = false;
            self.hide_window(ctx);
            ctx.request_repaint();
        }

        let prev_tab = self.tab;
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Dashboard, "Dashboard");
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
                ui.selectable_value(&mut self.tab, Tab::About, "About");
            });
            ui.add_space(4.0);
        });
        // Re-scan microphones whenever Settings is opened, so newly plugged-in devices
        // (e.g. a headset connected after launch) show up without a manual refresh.
        if prev_tab != Tab::Settings && self.tab == Tab::Settings {
            self.mic_devices = crate::audio::enumerate_input_devices();
        }

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Dashboard => self.dashboard(ui),
            Tab::Settings => self.settings(ui),
            Tab::About => self.about(ui),
        });

        // Repaint periodically so status/tray polling stays live even when idle.
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
    }
}

impl UiApp {
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let status = self.state.status();
        ui.heading("VoiceForge");
        ui.label("Hold your hotkey, speak, release — your words are typed where the cursor is.");
        ui.add_space(12.0);

        egui::Grid::new("dash").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Status:");
            let (dot, text) = match &status {
                Status::Idle => (egui::Color32::from_rgb(0x2f, 0x6f, 0xed), status.label()),
                Status::Listening => (egui::Color32::from_rgb(0xe0, 0x3b, 0x3b), status.label()),
                Status::Injecting => (egui::Color32::from_rgb(0xe0, 0x8b, 0x00), status.label()),
                Status::Downloading { .. } => (egui::Color32::from_rgb(0x00, 0x9a, 0x9a), status.label()),
                Status::Error(_) => (egui::Color32::from_rgb(0xc0, 0x00, 0x00), status.label()),
            };
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, dot);
                ui.label(text);
            });
            ui.end_row();

            ui.label("Hotkey:");
            ui.label(self.draft.hotkey.to_string());
            ui.end_row();

            ui.label("Enabled:");
            ui.label(if self.draft.enabled { "Yes" } else { "No" });
            ui.end_row();

            ui.label("Provider:");
            ui.label(format!("{:?}", self.draft.provider));
            ui.end_row();

            ui.label("API key:");
            ui.label(if self.state.api_key_present() { "Configured" } else { "Not set" });
            ui.end_row();
        });

        // A one-time model download (e.g. first-ever Whisper use) gets its own clearly
        // labeled progress bar here, instead of being surfaced through "Last transcript"
        // where it could be mistaken for recognized speech.
        if let Status::Downloading { percent, .. } = &status {
            ui.add_space(8.0);
            let bar = egui::ProgressBar::new(percent.unwrap_or(0.0)).show_percentage();
            ui.add(if percent.is_some() { bar } else { bar.animate(true) });
        }

        ui.add_space(12.0);
        ui.label("Last transcript:");
        let last = self.state.last_transcript();
        ui.group(|ui| {
            ui.set_min_height(40.0);
            if last.is_empty() {
                ui.weak("(nothing yet)");
            } else {
                ui.label(last);
            }
        });
    }

    fn settings(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Settings");
            ui.add_space(8.0);

            // --- Enabled ---
            ui.checkbox(&mut self.draft.enabled, "Enable dictation");

            ui.add_space(8.0);
            ui.separator();

            // --- Microphone ---
            ui.horizontal(|ui| {
                ui.label("Microphone");
                if ui.small_button("⟳ Refresh").clicked() {
                    self.mic_devices = crate::audio::enumerate_input_devices();
                }
            });
            let current_mic = self.draft.microphone.clone().unwrap_or_else(|| "System default".into());
            egui::ComboBox::from_id_salt("mic")
                .selected_text(current_mic)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.draft.microphone, None, "System default");
                    for dev in &self.mic_devices {
                        ui.selectable_value(&mut self.draft.microphone, Some(dev.clone()), dev);
                    }
                });
            ui.weak("Newly plugged-in devices won't show until you hit Refresh.");

            ui.add_space(8.0);

            // --- Language ---
            ui.label("Language (BCP-47 code, e.g. en, en-US, es, fr)");
            ui.text_edit_singleline(&mut self.draft.language);

            ui.add_space(8.0);

            // --- Provider ---
            ui.label("Speech provider");
            egui::ComboBox::from_id_salt("provider")
                .selected_text(provider_label(self.draft.provider))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.draft.provider, Provider::Whisper, "Local Whisper (offline)");
                    ui.selectable_value(&mut self.draft.provider, Provider::Openai, "OpenAI");
                    ui.selectable_value(&mut self.draft.provider, Provider::Deepgram, "Deepgram");
                    ui.selectable_value(&mut self.draft.provider, Provider::Mock, "Mock (offline test)");
                });

            // Local Whisper: model picker + offline note.
            if self.draft.provider == Provider::Whisper {
                ui.add_space(8.0);
                ui.label("Whisper model");
                egui::ComboBox::from_id_salt("whisper_model")
                    .selected_text(self.draft.whisper_model.clone())
                    .show_ui(ui, |ui| {
                        for m in crate::stt::model::MODELS {
                            ui.selectable_value(
                                &mut self.draft.whisper_model,
                                m.name.to_string(),
                                m.name,
                            );
                        }
                    });
                ui.weak("Runs fully offline. The model downloads automatically once (~142 MB for 'base'), then no internet is needed.");
            }

            ui.add_space(8.0);
            ui.separator();

            // --- Hotkey ---
            ui.label("Push-to-talk hotkey");
            egui::ComboBox::from_id_salt("hotkey_preset")
                .selected_text(hotkey_preset_label(&self.draft.hotkey))
                .show_ui(ui, |ui| {
                    for (label, hk) in hotkey_presets() {
                        if ui.selectable_label(hotkey_matches(&self.draft.hotkey, &hk), label).clicked() {
                            self.draft.hotkey = hk;
                            self.capturing_hotkey = false;
                        }
                    }
                    if ui.selectable_label(self.capturing_hotkey, "Custom…").clicked() {
                        self.capturing_hotkey = true;
                    }
                });

            if self.capturing_hotkey {
                ui.horizontal(|ui| {
                    ui.label(self.draft.hotkey.to_string());
                    ui.weak("Press your desired combo…");
                });
                if let Some(hk) = capture_hotkey(ui) {
                    self.draft.hotkey = hk;
                    self.capturing_hotkey = false;
                }
            } else {
                ui.label(self.draft.hotkey.to_string());
            }
            ui.weak("Tip: a modifier + a key works best, e.g. Alt + Space.");

            ui.add_space(8.0);
            ui.separator();

            // --- Autostart ---
            ui.checkbox(&mut self.draft.autostart, "Launch VoiceForge at Windows startup");
            ui.weak("Applied when you click Save.");

            ui.add_space(8.0);
            ui.separator();

            // --- API key (not used by the local providers) ---
            let key_label = match self.draft.provider {
                Provider::Openai => "OpenAI API key",
                Provider::Deepgram => "Deepgram API key",
                Provider::Whisper => "API key (not needed — Whisper runs offline)",
                Provider::Mock => "API key (not needed for Mock)",
            };
            ui.label(key_label);
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.api_key_input).password(true).hint_text("paste key…"));
                if ui.button("Save key").clicked() && !self.api_key_input.trim().is_empty() {
                    match secrets::set_api_key(self.api_key_input.trim()) {
                        Ok(()) => {
                            self.state.set_api_key_present(true);
                            self.api_key_input.clear();
                            self.api_key_saved_note = Some("Saved to Windows Credential Manager.".into());
                        }
                        Err(e) => self.api_key_saved_note = Some(format!("Failed: {e}")),
                    }
                }
                if ui.button("Remove").clicked() {
                    let _ = secrets::delete_api_key();
                    self.state.set_api_key_present(false);
                    self.api_key_saved_note = Some("Removed.".into());
                }
            });
            ui.label(if self.state.api_key_present() { "Status: configured" } else { "Status: not set" });
            if let Some(note) = &self.api_key_saved_note {
                ui.weak(note);
            }

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);

            // --- Save / Revert ---
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    match self.state.update_config(self.draft.clone()) {
                        Ok(()) => {
                            // Apply the (possibly changed) hotkey to the live listener.
                            #[cfg(windows)]
                            hotkey::update_hotkey(&self.draft.hotkey);
                            // Apply launch-at-startup preference.
                            if let Err(e) = crate::autostart::apply(self.draft.autostart) {
                                tracing::warn!("autostart apply failed: {e}");
                            }
                            self.api_key_saved_note = Some("Settings saved.".into());
                        }
                        Err(e) => self.api_key_saved_note = Some(format!("Save failed: {e}")),
                    }
                }
                if ui.button("Revert").clicked() {
                    self.draft = self.state.config();
                }
            });
        });
    }

    fn about(&mut self, ui: &mut egui::Ui) {
        ui.heading("About VoiceForge");
        ui.add_space(8.0);
        ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(6.0);
        ui.label("A universal voice-typing utility. Speech → Text → Type.");
        ui.label("Hold the hotkey anywhere you can type, speak, and release.");
        ui.add_space(8.0);
        ui.label("Speech recognition by Deepgram. Runs locally in your system tray;");
        ui.label("audio is only captured while the hotkey is held.");
    }
}

/// Built-in hotkey choices offered in Settings, alongside a "Custom…" capture option.
fn hotkey_presets() -> Vec<(&'static str, Hotkey)> {
    vec![
        ("Alt + Space", Hotkey { modifiers: vec!["alt".into()], key: "space".into() }),
        ("Ctrl + Space", Hotkey { modifiers: vec!["ctrl".into()], key: "space".into() }),
        ("Caps Lock", Hotkey { modifiers: vec![], key: "capslock".into() }),
        ("` (backtick)", Hotkey { modifiers: vec![], key: "grave".into() }),
    ]
}

fn hotkey_matches(a: &Hotkey, b: &Hotkey) -> bool {
    a.key.eq_ignore_ascii_case(&b.key)
        && a.modifiers.len() == b.modifiers.len()
        && a.modifiers.iter().all(|m| b.modifiers.iter().any(|m2| m.eq_ignore_ascii_case(m2)))
}

fn hotkey_preset_label(hk: &Hotkey) -> String {
    hotkey_presets()
        .into_iter()
        .find(|(_, preset)| hotkey_matches(hk, preset))
        .map(|(label, _)| label.to_string())
        .unwrap_or_else(|| format!("Custom ({hk})"))
}

/// Human-friendly label for a provider in the dropdown's collapsed state.
fn provider_label(p: Provider) -> &'static str {
    match p {
        Provider::Whisper => "Local Whisper (offline)",
        Provider::Openai => "OpenAI",
        Provider::Deepgram => "Deepgram",
        Provider::Mock => "Mock (offline test)",
    }
}

/// Reads currently-pressed keys from egui input and, if a valid modifier+key combo is
/// held, returns it. Returns `None` until a usable combination is detected.
fn capture_hotkey(ui: &egui::Ui) -> Option<Hotkey> {
    ui.input(|i| {
        let mut modifiers = Vec::new();
        if i.modifiers.alt {
            modifiers.push("alt".to_string());
        }
        if i.modifiers.ctrl {
            modifiers.push("ctrl".to_string());
        }
        if i.modifiers.shift {
            modifiers.push("shift".to_string());
        }
        // Find the first non-modifier key currently down.
        for key in i.keys_down.iter() {
            let name = hotkey::egui_key_name(*key);
            if let Some(name) = name {
                return Some(Hotkey { modifiers, key: name });
            }
        }
        None
    })
}
