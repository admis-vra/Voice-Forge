//! System-tray icon and context menu.
//!
//! The tray is the app's persistent presence: it shows current status via its icon and
//! offers Enable/Disable, Settings, and Quit.
//!
//! Crucially, the tray runs on its **own thread with its own Win32 message pump**, not on
//! the egui/winit UI loop. A hidden eframe window does not run its `update()` loop, so
//! polling tray events there would make the menu unresponsive whenever the window is
//! hidden (i.e. almost always). Running the tray independently keeps Settings/Quit alive
//! regardless of the window's visibility; it wakes or closes the window through a shared
//! [`egui::Context`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use eframe::egui;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

use crate::app::{SharedState, Status};
use crate::icon;

/// Which menu action was clicked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TrayAction {
    ToggleEnabled,
    OpenSettings,
    Quit,
}

/// Owns the tray icon and remembers menu item ids so events can be routed.
struct Tray {
    _tray: TrayIcon,
    enabled_item: CheckMenuItem,
    id_enabled: MenuId,
    id_settings: MenuId,
    id_quit: MenuId,
    icon_idle: Icon,
    icon_listening: Icon,
    current_listening: bool,
}

impl Tray {
    fn new(enabled: bool) -> Result<Self> {
        let menu = Menu::new();

        let header = MenuItem::new("VoiceForge", false, None);
        let enabled_item = CheckMenuItem::new("Enabled", true, enabled, None);
        let settings_item = MenuItem::new("Settings…", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        menu.append(&header)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&enabled_item)?;
        menu.append(&settings_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let icon_idle = to_icon(icon::idle())?;
        let icon_listening = to_icon(icon::listening())?;

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("VoiceForge — hold the hotkey to dictate")
            .with_icon(icon_idle.clone())
            .build()
            .context("building tray icon")?;

        Ok(Tray {
            id_enabled: enabled_item.id().clone(),
            id_settings: settings_item.id().clone(),
            id_quit: quit_item.id().clone(),
            enabled_item,
            _tray: tray,
            icon_idle,
            icon_listening,
            current_listening: false,
        })
    }

    fn match_event(&self, event: &MenuEvent) -> Option<TrayAction> {
        let id = &event.id;
        if id == &self.id_enabled {
            Some(TrayAction::ToggleEnabled)
        } else if id == &self.id_settings {
            Some(TrayAction::OpenSettings)
        } else if id == &self.id_quit {
            Some(TrayAction::Quit)
        } else {
            None
        }
    }

    fn set_enabled_checked(&self, checked: bool) {
        if self.enabled_item.is_checked() != checked {
            self.enabled_item.set_checked(checked);
        }
    }

    fn set_status(&mut self, status: &Status) {
        let listening = matches!(status, Status::Listening | Status::Injecting);
        if listening != self.current_listening {
            let icon = if listening {
                self.icon_listening.clone()
            } else {
                self.icon_idle.clone()
            };
            let _ = self._tray.set_icon(Some(icon));
            self.current_listening = listening;
        }
    }
}

fn to_icon(img: icon::Rgba) -> Result<Icon> {
    Icon::from_rgba(img.bytes, img.width, img.height).context("creating tray icon from rgba")
}

/// Spawns the tray on its own thread. The thread creates the tray icon (so its hidden
/// message window belongs to a thread that pumps messages), then loops: pump Win32
/// messages, handle menu clicks, and keep the icon/checkbox in sync with app state.
///
/// - Settings click → show and focus the egui window via `ctx`.
/// - Quit click → set `quitting` and ask the window to close.
pub fn spawn(state: SharedState, ctx: egui::Context, quitting: Arc<AtomicBool>) -> Result<()> {
    std::thread::Builder::new()
        .name("voiceforge-tray".into())
        .spawn(move || {
            let mut tray = match Tray::new(state.config().enabled) {
                Ok(t) => t,
                Err(e) => {
                    tracing::error!("failed to create tray: {e}");
                    return;
                }
            };
            tracing::info!("tray created");

            #[cfg(windows)]
            let mut msg = MSG::default();
            loop {
                // Pump any pending Win32 messages so the tray's window can show its menu.
                // Not needed on macOS/Linux, where tray-icon drives its own native event
                // loop (AppKit / GTK) independently of this thread.
                #[cfg(windows)]
                unsafe {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                // Handle any menu clicks that were queued during message dispatch.
                while let Ok(event) = MenuEvent::receiver().try_recv() {
                    match tray.match_event(&event) {
                        Some(TrayAction::OpenSettings) => {
                            tracing::info!("settings requested from tray");
                            // Make the window visible, un-minimize, and bring it forward.
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                            ctx.request_repaint();
                        }
                        Some(TrayAction::Quit) => {
                            tracing::info!("quit requested from tray");
                            quitting.store(true, Ordering::SeqCst);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            ctx.request_repaint();
                            return;
                        }
                        Some(TrayAction::ToggleEnabled) => {
                            let mut cfg = state.config();
                            cfg.enabled = !cfg.enabled;
                            tray.set_enabled_checked(cfg.enabled);
                            let _ = state.update_config(cfg);
                            ctx.request_repaint();
                        }
                        None => {}
                    }
                }

                // Keep the icon and checkbox reflecting current state.
                tray.set_status(&state.status());
                tray.set_enabled_checked(state.config().enabled);

                if quitting.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(80));
            }
        })
        .context("spawning tray thread")?;
    Ok(())
}
