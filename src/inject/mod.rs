//! Text injection: insert recognized text at the cursor of whatever application
//! currently has keyboard focus.
//!
//! The primary path synthesizes Unicode keystrokes with `SendInput`, which works in
//! virtually any editable control without touching the clipboard. A clipboard-paste
//! fallback (that saves and restores the user's clipboard) is available for the rare
//! cases where synthetic keystrokes are dropped.

#[cfg(windows)]
mod windows;

use anyhow::Result;

/// Types `text` into the focused application at the cursor position.
pub fn type_text(text: &str) -> Result<()> {
    #[cfg(windows)]
    {
        windows::type_text(text)
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        anyhow::bail!("text injection is only implemented on Windows")
    }
}
