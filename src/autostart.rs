//! Optional launch-at-startup support.
//!
//! Uses the `auto-launch` crate, which manages the platform-native mechanism: the
//! `HKCU\...\Run` registry key on Windows, a LaunchAgent plist on macOS, and a
//! `.desktop` autostart entry on Linux. Failures are logged and never fatal.

use auto_launch::AutoLaunchBuilder;

const APP_NAME: &str = "VoiceForge";

/// Applies the desired autostart state, adding or removing the platform entry as needed.
pub fn apply(enabled: bool) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe = exe.to_string_lossy().to_string();

    let auto = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe)
        .build()?;

    if enabled {
        auto.enable()?;
        tracing::info!("autostart enabled → {exe}");
    } else {
        // Ignore "not found" style errors — the desired end state (absent) is
        // achieved either way.
        let _ = auto.disable();
        tracing::info!("autostart disabled");
    }
    Ok(())
}
