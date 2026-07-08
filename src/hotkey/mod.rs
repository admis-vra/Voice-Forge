//! Global push-to-talk hotkey abstraction.
//!
//! A [`ListenerHandle`] watches for the configured modifier+key combination system-wide
//! and reports press/release edges so the controller can start and stop dictation.
//! Implemented once, cross-platform, on top of the `global-hotkey` crate (Windows,
//! macOS, and Linux/X11).

use std::sync::mpsc::Sender;

use anyhow::{anyhow, Result};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use crate::config::Hotkey;

/// Edge event emitted by the listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The full combination just became held.
    Pressed,
    /// The combination was released.
    Released,
}

/// Handle to a running hotkey listener. Dropping it unregisters the hotkey and stops
/// the listener thread.
pub struct ListenerHandle {
    stop: Sender<()>,
    _manager: GlobalHotKeyManager,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// Starts watching for the given hotkey. Emits [`HotkeyEvent`]s on `tx`.
pub fn spawn_listener(hotkey: &Hotkey, tx: Sender<HotkeyEvent>) -> Result<ListenerHandle> {
    let hk = build_hotkey(hotkey)?;

    let manager = GlobalHotKeyManager::new().map_err(|e| anyhow!("creating hotkey manager: {e}"))?;
    manager
        .register(hk)
        .map_err(|e| anyhow!("registering hotkey: {e}"))?;

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let receiver = GlobalHotKeyEvent::receiver().clone();
    let target_id = hk.id();

    std::thread::Builder::new()
        .name("voiceforge-hotkey".into())
        .spawn(move || {
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                if let Ok(event) = receiver.recv_timeout(std::time::Duration::from_millis(50)) {
                    if event.id != target_id {
                        continue;
                    }
                    let mapped = match event.state {
                        HotKeyState::Pressed => Some(HotkeyEvent::Pressed),
                        HotKeyState::Released => Some(HotkeyEvent::Released),
                    };
                    if let Some(ev) = mapped {
                        let _ = tx.send(ev);
                    }
                }
            }
            tracing::info!("hotkey listener stopped");
        })?;

    Ok(ListenerHandle {
        stop: stop_tx,
        _manager: manager,
    })
}

/// Updates the hotkey combination watched by the running listener.
///
/// `global-hotkey` does not support mutating a registered hotkey in place, so this is a
/// no-op placeholder; the settings UI restarts the listener (via `Controller`) when the
/// hotkey changes.
pub fn update_hotkey(_hotkey: &Hotkey) {}

fn build_hotkey(hotkey: &Hotkey) -> Result<HotKey> {
    let code = code_for_key_name(&hotkey.key)
        .ok_or_else(|| anyhow!("unsupported hotkey main key: {}", hotkey.key))?;

    let mut modifiers = Modifiers::empty();
    for m in &hotkey.modifiers {
        modifiers |= modifier_for_name(m)
            .ok_or_else(|| anyhow!("unsupported hotkey modifier: {m}"))?;
    }

    Ok(HotKey::new(Some(modifiers), code))
}

fn modifier_for_name(name: &str) -> Option<Modifiers> {
    Some(match name.to_ascii_lowercase().as_str() {
        "alt" => Modifiers::ALT,
        "ctrl" | "control" => Modifiers::CONTROL,
        "shift" => Modifiers::SHIFT,
        "win" | "super" | "meta" | "cmd" | "command" => Modifiers::META,
        _ => return None,
    })
}

/// Maps a VoiceForge key name (see [`egui_key_name`]) to a `global-hotkey` [`Code`].
fn code_for_key_name(name: &str) -> Option<Code> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "space" => Code::Space,
        "enter" => Code::Enter,
        "tab" => Code::Tab,
        "backslash" => Code::Backslash,
        "capslock" => Code::CapsLock,
        "grave" | "backtick" => Code::Backquote,
        "a" => Code::KeyA, "b" => Code::KeyB, "c" => Code::KeyC, "d" => Code::KeyD,
        "e" => Code::KeyE, "f" => Code::KeyF, "g" => Code::KeyG, "h" => Code::KeyH,
        "i" => Code::KeyI, "j" => Code::KeyJ, "k" => Code::KeyK, "l" => Code::KeyL,
        "m" => Code::KeyM, "n" => Code::KeyN, "o" => Code::KeyO, "p" => Code::KeyP,
        "q" => Code::KeyQ, "r" => Code::KeyR, "s" => Code::KeyS, "t" => Code::KeyT,
        "u" => Code::KeyU, "v" => Code::KeyV, "w" => Code::KeyW, "x" => Code::KeyX,
        "y" => Code::KeyY, "z" => Code::KeyZ,
        "0" => Code::Digit0, "1" => Code::Digit1, "2" => Code::Digit2, "3" => Code::Digit3,
        "4" => Code::Digit4, "5" => Code::Digit5, "6" => Code::Digit6, "7" => Code::Digit7,
        "8" => Code::Digit8, "9" => Code::Digit9,
        "f1" => Code::F1, "f2" => Code::F2, "f3" => Code::F3, "f4" => Code::F4,
        "f5" => Code::F5, "f6" => Code::F6, "f7" => Code::F7, "f8" => Code::F8,
        "f9" => Code::F9, "f10" => Code::F10, "f11" => Code::F11, "f12" => Code::F12,
        _ => return None,
    })
}

/// Maps an egui key to VoiceForge's lowercase key name used in [`Hotkey`].
///
/// Returns `None` for keys we do not accept as a push-to-talk main key.
pub fn egui_key_name(key: eframe::egui::Key) -> Option<String> {
    use eframe::egui::Key::*;
    let name = match key {
        Space => "space",
        Enter => "enter",
        Tab => "tab",
        Backslash => "backslash",
        Backtick => "grave",
        A => "a", B => "b", C => "c", D => "d", E => "e", F => "f", G => "g",
        H => "h", I => "i", J => "j", K => "k", L => "l", M => "m", N => "n",
        O => "o", P => "p", Q => "q", R => "r", S => "s", T => "t", U => "u",
        V => "v", W => "w", X => "x", Y => "y", Z => "z",
        F1 => "f1", F2 => "f2", F3 => "f3", F4 => "f4", F5 => "f5", F6 => "f6",
        F7 => "f7", F8 => "f8", F9 => "f9", F10 => "f10", F11 => "f11", F12 => "f12",
        Num0 => "0", Num1 => "1", Num2 => "2", Num3 => "3", Num4 => "4",
        Num5 => "5", Num6 => "6", Num7 => "7", Num8 => "8", Num9 => "9",
        _ => return None,
    };
    Some(name.to_string())
}

/// Validates that a hotkey is usable (has a main key). Modifiers are optional but
/// recommended to avoid clashing with ordinary typing.
#[allow(dead_code)] // used by the settings UI validation path
pub fn is_valid(hk: &Hotkey) -> bool {
    !hk.key.trim().is_empty()
}
