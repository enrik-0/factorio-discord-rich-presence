//! Application paths.
//!
//! With autostart, the process is born from the Windows registry and its
//! current directory is `C:\Windows\System32`. That's why the configuration
//! needs a stable location in `%APPDATA%`, and can't depend on where it's
//! launched from.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Own folder inside the user data directory.
pub const APP_DIR_NAME: &str = "factorio-discord-rp";

pub const CONFIG_FILE: &str = "config.toml";
pub const LOG_FILE: &str = "factorio-discord-rp.log";

/// Base user data directory: `%APPDATA%` on Windows,
/// `$XDG_DATA_HOME` (or `~/.local/share` if unset) on Unix.
#[cfg(windows)]
fn base_dir() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA").context("could not read %APPDATA%")?;
    Ok(PathBuf::from(appdata))
}

#[cfg(unix)]
fn base_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return Ok(PathBuf::from(xdg));
        }
    }
    let home = std::env::var("HOME").context("could not read $HOME")?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

/// `%APPDATA%\factorio-discord-rp` (Windows) or
/// `~/.local/share/factorio-discord-rp` (Unix), creating it if needed.
pub fn app_dir() -> Result<PathBuf> {
    let dir = base_dir()?.join(APP_DIR_NAME);
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}

/// Stable location of the configuration. This is the one autostart uses.
pub fn config_in_app_dir() -> Option<PathBuf> {
    app_dir().ok().map(|dir| dir.join(CONFIG_FILE))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(app_dir()?.join(LOG_FILE))
}

/// Absolute path to the executable, needed to register autostart.
pub fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("could not determine the executable path")
}
