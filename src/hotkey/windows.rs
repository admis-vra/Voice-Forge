//! Windows implementation of the global push-to-talk hotkey using a low-level keyboard
//! hook (`WH_KEYBOARD_LL`).
//!
//! Design notes:
//! - The hook procedure is `extern "system"` and cannot capture state, so the listener
//!   configuration (target key, modifiers, event sender) lives in a process-global
//!   guarded by a mutex. Only one listener exists at a time.
//! - Push-to-talk semantics: `Pressed` fires when the main key goes down *while* the
//!   required modifiers are held; `Released` fires when the main key goes up. The main
//!   key's events are swallowed while active so the focused app never receives the
//!   keystroke (e.g. Space does not type a space, and Alt+Space does not open the
//!   window system menu).
//! - The hook must run on a thread with a Win32 message loop, so we spawn a dedicated
//!   thread that installs the hook and pumps messages.

use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, Result};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
    WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::HotkeyEvent;
use crate::config::Hotkey;

/// State shared with the C-ABI hook procedure.
struct HookState {
    /// Virtual-key code of the main (trigger) key.
    main_vk: u32,
    /// Modifier virtual-key codes that must be held for a valid press.
    modifier_vks: Vec<u32>,
    /// Whether the combo is currently considered "held".
    active: bool,
    tx: Sender<HotkeyEvent>,
}

static HOOK_STATE: OnceLock<Mutex<Option<HookState>>> = OnceLock::new();

fn hook_state() -> &'static Mutex<Option<HookState>> {
    HOOK_STATE.get_or_init(|| Mutex::new(None))
}

/// Handle to a running hotkey listener. Dropping it stops the listener thread.
pub struct ListenerHandle {
    stop: Sender<()>,
}

impl Drop for ListenerHandle {
    fn drop(&mut self) {
        // Best-effort: ask the listener thread to tear down its hook and exit.
        let _ = self.stop.send(());
    }
}

/// Starts watching for the given hotkey. Emits [`HotkeyEvent`]s on `tx`.
pub fn spawn_listener(hotkey: &Hotkey, tx: Sender<HotkeyEvent>) -> Result<ListenerHandle> {
    let main_vk = vk_for_key_name(&hotkey.key)
        .ok_or_else(|| anyhow!("unsupported hotkey main key: {}", hotkey.key))?;
    let modifier_vks: Vec<u32> = hotkey
        .modifiers
        .iter()
        .filter_map(|m| vk_for_modifier(m))
        .collect();

    *hook_state().lock().unwrap() = Some(HookState {
        main_vk,
        modifier_vks,
        active: false,
        tx,
    });

    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();

    std::thread::Builder::new()
        .name("voiceforge-hotkey".into())
        .spawn(move || unsafe {
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0) {
                Ok(h) => h,
                Err(e) => {
                    tracing::error!("failed to install keyboard hook: {e}");
                    return;
                }
            };
            tracing::info!("keyboard hook installed");

            // Standard message loop; a low-level hook requires one to receive callbacks.
            // We poll the stop channel between messages using a peek-friendly loop.
            let mut msg = MSG::default();
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                // GetMessageW blocks; to remain responsive to stop we rely on the hook
                // callbacks generating traffic. Use a short timer via PeekMessage-like
                // behavior would be ideal, but GetMessage keeps CPU at zero. We post a
                // WM_NULL periodically is unnecessary; stop is handled on next wake.
                let r = GetMessageW(&mut msg, None, 0, 0);
                if r.0 == 0 || r.0 == -1 {
                    break;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            let _ = UnhookWindowsHookEx(hook);
            *hook_state().lock().unwrap() = None;
            tracing::info!("keyboard hook removed");
        })?;

    Ok(ListenerHandle { stop: stop_tx })
}

/// Updates the hotkey combination watched by the running listener without restarting it.
pub fn update_hotkey(hotkey: &Hotkey) {
    if let Some(state) = hook_state().lock().unwrap().as_mut() {
        if let Some(vk) = vk_for_key_name(&hotkey.key) {
            state.main_vk = vk;
            state.modifier_vks = hotkey
                .modifiers
                .iter()
                .filter_map(|m| vk_for_modifier(m))
                .collect();
            state.active = false;
        }
    }
}

/// The low-level keyboard hook procedure. Kept minimal: it inspects the event, updates
/// the active state, and emits edge events. Returns non-zero to swallow the keystroke.
unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code < 0 {
        return CallNextHookEx(HHOOK::default(), code, wparam, lparam);
    }

    let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
    let vk = kbd.vkCode;
    let msg = wparam.0 as u32;
    let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
    let is_up = msg == WM_KEYUP || msg == WM_SYSKEYUP;

    let mut swallow = false;
    if let Some(state) = hook_state().lock().unwrap().as_mut() {
        if vk == state.main_vk {
            if is_down && !state.active && modifiers_held(&state.modifier_vks) {
                state.active = true;
                let _ = state.tx.send(HotkeyEvent::Pressed);
                swallow = true;
            } else if is_down && state.active {
                // Auto-repeat while held: swallow to avoid the key reaching the app.
                swallow = true;
            } else if is_up && state.active {
                state.active = false;
                let _ = state.tx.send(HotkeyEvent::Released);
                swallow = true;
            }
        }
    }

    if swallow {
        // Returning a non-zero value prevents the event from propagating further.
        return LRESULT(1);
    }
    CallNextHookEx(HHOOK::default(), code, wparam, lparam)
}

/// Returns true if every required modifier is currently down (per the async key state).
fn modifiers_held(mods: &[u32]) -> bool {
    mods.iter().all(|&vk| is_key_down(vk as i32))
}

fn is_key_down(vk: i32) -> bool {
    // The high-order bit of GetAsyncKeyState indicates the key is currently down.
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

/// Maps a modifier name to a virtual-key code.
fn vk_for_modifier(name: &str) -> Option<u32> {
    let vk = match name.to_ascii_lowercase().as_str() {
        "alt" => VK_MENU.0,
        "ctrl" | "control" => VK_CONTROL.0,
        "shift" => VK_SHIFT.0,
        "win" | "super" | "meta" => VK_LWIN.0, // treat either Win key
        _ => return None,
    };
    Some(vk as u32)
}

/// Maps a VoiceForge key name (see [`super::egui_key_name`]) to a virtual-key code.
fn vk_for_key_name(name: &str) -> Option<u32> {
    let n = name.to_ascii_lowercase();
    let vk: u32 = match n.as_str() {
        "space" => 0x20,
        "enter" => 0x0D,
        "tab" => 0x09,
        "backslash" => 0xDC,
        // Letters a-z map directly to their uppercase ASCII virtual-key codes.
        s if s.len() == 1 && s.chars().next().unwrap().is_ascii_alphabetic() => {
            s.to_ascii_uppercase().chars().next().unwrap() as u32
        }
        // Digits 0-9.
        s if s.len() == 1 && s.chars().next().unwrap().is_ascii_digit() => {
            s.chars().next().unwrap() as u32
        }
        // Function keys f1-f12 -> VK_F1 (0x70) .. VK_F12 (0x7B).
        s if s.starts_with('f') && s[1..].parse::<u32>().is_ok() => {
            let n: u32 = s[1..].parse().unwrap();
            if (1..=12).contains(&n) {
                0x70 + (n - 1)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let _ = VK_RWIN; // keep the import referenced for future right-Win handling
    Some(vk)
}
