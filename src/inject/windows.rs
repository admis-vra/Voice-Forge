//! Windows text injection via `SendInput` (Unicode) with a clipboard-paste fallback.

use anyhow::{anyhow, Context, Result};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL,
};

/// Types `text` into the focused application. Tries `SendInput` first; on failure falls
/// back to clipboard paste (preserving the user's existing clipboard text).
pub fn type_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    match send_input_unicode(text) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("SendInput failed ({e}); falling back to clipboard paste");
            paste_via_clipboard(text)
        }
    }
}

/// Synthesizes Unicode key events for every code unit in `text`.
fn send_input_unicode(text: &str) -> Result<()> {
    // Build INPUT events: each UTF-16 code unit gets a key-down and a key-up.
    let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
    for unit in text.encode_utf16() {
        inputs.push(unicode_input(unit, false));
        inputs.push(unicode_input(unit, true));
    }

    // Send in chunks to stay well within SendInput's limits.
    const CHUNK: usize = 2000;
    for chunk in inputs.chunks(CHUNK) {
        let sent = unsafe { SendInput(chunk, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize != chunk.len() {
            return Err(anyhow!(
                "SendInput injected {sent}/{} events (input may be blocked)",
                chunk.len()
            ));
        }
    }
    Ok(())
}

fn unicode_input(code_unit: u16, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_UNICODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: code_unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Fallback: put the text on the clipboard, send Ctrl+V, then restore prior clipboard
/// text. Only text (CF_UNICODETEXT) is preserved — other formats are not restored.
fn paste_via_clipboard(text: &str) -> Result<()> {
    let previous = read_clipboard_text();
    set_clipboard_text(text).context("setting clipboard text")?;

    // Send Ctrl+V.
    send_ctrl_v().context("sending Ctrl+V")?;

    // Give the target app a moment to read the clipboard before restoring.
    std::thread::sleep(std::time::Duration::from_millis(120));

    if let Some(prev) = previous {
        let _ = set_clipboard_text(&prev);
    }
    Ok(())
}

fn send_ctrl_v() -> Result<()> {
    let v = VIRTUAL_KEY(b'V' as u16);
    let inputs = [
        vk_input(VK_CONTROL, false),
        vk_input(v, false),
        vk_input(v, true),
        vk_input(VK_CONTROL, true),
    ];
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        return Err(anyhow!("SendInput sent {sent}/4 events for Ctrl+V"));
    }
    Ok(())
}

fn vk_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    let flags = if key_up {
        KEYEVENTF_KEYUP
    } else {
        KEYBD_EVENT_FLAGS(0)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

// --- Clipboard helpers ---

/// RAII guard that opens the clipboard and closes it on drop.
struct Clipboard;

impl Clipboard {
    fn open() -> Result<Clipboard> {
        unsafe { OpenClipboard(HWND::default()) }.context("OpenClipboard")?;
        Ok(Clipboard)
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn read_clipboard_text() -> Option<String> {
    let _clip = Clipboard::open().ok()?;
    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
        if handle.0.is_null() {
            return None;
        }
        let ptr = GlobalLock(windows::Win32::Foundation::HGLOBAL(handle.0)) as *const u16;
        if ptr.is_null() {
            return None;
        }
        // Read until the NUL terminator.
        let mut len = 0usize;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let text = String::from_utf16_lossy(slice);
        let _ = GlobalUnlock(windows::Win32::Foundation::HGLOBAL(handle.0));
        Some(text)
    }
}

fn set_clipboard_text(text: &str) -> Result<()> {
    let _clip = Clipboard::open()?;
    unsafe {
        EmptyClipboard().context("EmptyClipboard")?;

        // Allocate a moveable global buffer holding the UTF-16 text + NUL.
        let mut utf16: Vec<u16> = text.encode_utf16().collect();
        utf16.push(0);
        let bytes = utf16.len() * std::mem::size_of::<u16>();

        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes).context("GlobalAlloc")?;
        let dst = GlobalLock(hglobal) as *mut u16;
        if dst.is_null() {
            return Err(anyhow!("GlobalLock returned null"));
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), dst, utf16.len());
        let _ = GlobalUnlock(hglobal);

        // Ownership of hglobal transfers to the system on success.
        SetClipboardData(CF_UNICODETEXT.0 as u32, HANDLE(hglobal.0))
            .context("SetClipboardData")?;
    }
    Ok(())
}
