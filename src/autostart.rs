//! Optional launch-at-startup support.
//!
//! On Windows this manages a value under
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, which runs VoiceForge for the
//! current user at sign-in. We drive `reg.exe` rather than linking the registry API to
//! keep this small and dependency-light; failures are logged and never fatal.

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "VoiceForge";

/// Applies the desired autostart state, adding or removing the Run entry as needed.
pub fn apply(enabled: bool) -> anyhow::Result<()> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

#[cfg(windows)]
fn enable() -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy().to_string();

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let status = Command::new("reg")
        .args([
            "add", RUN_KEY, "/v", VALUE_NAME, "/t", "REG_SZ", "/d", &exe, "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .status()?;
    if !status.success() {
        anyhow::bail!("reg add exited with {status}");
    }
    tracing::info!("autostart enabled → {exe}");
    Ok(())
}

#[cfg(windows)]
fn disable() -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // Ignore "value not found" — the desired end state (absent) is achieved either way.
    let _ = Command::new("reg")
        .args(["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    tracing::info!("autostart disabled");
    Ok(())
}

#[cfg(not(windows))]
fn enable() -> anyhow::Result<()> {
    anyhow::bail!("autostart is only implemented on Windows")
}

#[cfg(not(windows))]
fn disable() -> anyhow::Result<()> {
    Ok(())
}
