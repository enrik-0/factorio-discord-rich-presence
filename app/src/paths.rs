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

/// `%APPDATA%\factorio-discord-rp`, creating it if needed.
pub fn app_dir() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA").context("could not read %APPDATA%")?;
    let dir = PathBuf::from(appdata).join(APP_DIR_NAME);
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
