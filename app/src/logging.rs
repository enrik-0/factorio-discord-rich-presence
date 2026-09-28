//! Application logging.
//!
//! In tray mode there's no console to look at, so messages go to a file in
//! `%APPDATA%`. In console mode they still print to the screen.

use std::fs::OpenOptions;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

/// Once the log reaches this size, it's cleared on startup.
///
/// Real rotation would require another dependency for something that grows
/// a few lines per minute; truncating on startup keeps the file bounded and
/// is enough to diagnose the current session.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

fn filter() -> EnvFilter {
    EnvFilter::try_from_env("FACTORIO_DRP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("factorio_discord_rp=info"))
}

/// Console logging, for command-line modes.
pub fn init_console() {
    tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_target(false)
        .init();
}

/// File logging, for when it runs in the tray without a console.
///
/// Returns the file's path, so it can be opened from the menu.
pub fn init_file() -> Result<std::path::PathBuf> {
    let path = crate::paths::log_path()?;

    if let Ok(metadata) = std::fs::metadata(&path) {
        if metadata.len() > MAX_LOG_BYTES {
            let _ = std::fs::remove_file(&path);
        }
    }

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("could not open the log at {}", path.display()))?;

    tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_target(false)
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .init();

    Ok(path)
}
