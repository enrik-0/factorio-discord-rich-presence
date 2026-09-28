//! Automatic startup with the Windows session.
//!
//! Registered under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`:
//! per-user, no administrator permissions needed, and visible in Task
//! Manager, so it can be disabled from there even if the application
//! disappears.
//!
//! Always disabled by default: adding something to system startup is the
//! user's decision, not the program's.

use anyhow::{Context, Result};
use auto_launch::AutoLaunchBuilder;
use tracing::info;

/// Name of the registry entry, as it appears in the system.
const APP_NAME: &str = "Factorio Discord Rich Presence";

fn launcher() -> Result<auto_launch::AutoLaunch> {
    let exe = crate::paths::current_exe()?;
    let exe = exe.to_string_lossy().to_string();

    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe)
        // Without this, starting from the registry would open in console mode.
        .set_args(&["--tray"])
        .build()
        .context("could not prepare the autostart registry entry")
}

pub fn is_enabled() -> bool {
    launcher()
        .and_then(|l| l.is_enabled().context("autostart query"))
        .unwrap_or(false)
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let launcher = launcher()?;
    if enabled {
        launcher.enable().context("could not enable autostart")?;
        info!("autostart enabled");
    } else {
        launcher.disable().context("could not disable autostart")?;
        info!("autostart disabled");
    }
    Ok(())
}
