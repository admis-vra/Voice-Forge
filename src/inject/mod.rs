//! Text injection: insert recognized text at the cursor of whatever application
//! currently has keyboard focus.
//!
//! The primary path synthesizes Unicode keystrokes with `enigo`, which works in
//! virtually any editable control without touching the clipboard on Windows, macOS, and
//! Linux (X11). A clipboard-paste fallback (via `arboard`, saving and restoring the
//! user's clipboard) is used if direct keystroke synthesis fails.
//!
//! macOS requires the app to be granted Accessibility permission for synthetic input to
//! work; Linux requires an X11 session (or a Wayland compositor with the relevant
//! virtual-input protocol support) for both keystroke synthesis and clipboard access.

use anyhow::{Context, Result};
use enigo::{Enigo, Keyboard, Settings};

/// Types `text` into the focused application at the cursor position.
pub fn type_text(text: &str) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    match send_unicode(text) {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!("direct keystroke injection failed ({e}); falling back to clipboard paste");
            paste_via_clipboard(text)
        }
    }
}

fn send_unicode(text: &str) -> Result<()> {
    let mut enigo = Enigo::new(&Settings::default()).context("initializing input simulator")?;
    enigo
        .text(text)
        .map_err(|e| anyhow::anyhow!("enigo text injection failed: {e}"))
}

/// Fallback: put the text on the clipboard, send Ctrl/Cmd+V, then restore the prior
/// clipboard text.
fn paste_via_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new().context("opening clipboard")?;
    let previous = clipboard.get_text().ok();

    clipboard.set_text(text).context("setting clipboard text")?;
    send_paste_shortcut().context("sending paste shortcut")?;

    // Give the target app a moment to read the clipboard before restoring.
    std::thread::sleep(std::time::Duration::from_millis(120));

    if let Some(prev) = previous {
        let _ = clipboard.set_text(prev);
    }
    Ok(())
}

fn send_paste_shortcut() -> Result<()> {
    use enigo::{Direction, Key};

    let mut enigo = Enigo::new(&Settings::default()).context("initializing input simulator")?;
    let modifier = if cfg!(target_os = "macos") {
        Key::Meta
    } else {
        Key::Control
    };

    enigo
        .key(modifier, Direction::Press)
        .map_err(|e| anyhow::anyhow!("key press failed: {e}"))?;
    enigo
        .key(Key::Unicode('v'), Direction::Click)
        .map_err(|e| anyhow::anyhow!("key click failed: {e}"))?;
    enigo
        .key(modifier, Direction::Release)
        .map_err(|e| anyhow::anyhow!("key release failed: {e}"))?;
    Ok(())
}
