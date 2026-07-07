//! Global push-to-talk hotkey abstraction.
//!
//! A [`HotkeyListener`] watches for the configured modifier+key combination system-wide
//! and reports press/release edges so the controller can start and stop dictation. The
//! platform implementation lives in [`windows`]; this module also holds the key-name
//! mapping shared with the settings UI.
//!
//! M1 provides the name mapping and types; the Win32 low-level hook is added in M2.

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::{spawn_listener, update_hotkey, ListenerHandle};

use crate::config::Hotkey;

/// Edge event emitted by the listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
    /// The full combination just became held.
    Pressed,
    /// The combination was released.
    Released,
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
