//! Rutas de la aplicación.
//!
//! Con autoarranque, el proceso nace desde el registro de Windows y su directorio
//! actual es `C:\Windows\System32`. Por eso la configuración necesita una
//! ubicación estable en `%APPDATA%`, y no puede depender de dónde se lance.

use std::path::PathBuf;

use anyhow::{Context, Result};

/// Carpeta propia dentro de los datos de usuario.
pub const APP_DIR_NAME: &str = "factorio-discord-rp";

pub const CONFIG_FILE: &str = "config.toml";
pub const LOG_FILE: &str = "factorio-discord-rp.log";

/// Carpeta base de datos de usuario: `%APPDATA%` en Windows,
/// `$XDG_DATA_HOME` (o `~/.local/share` si no está definida) en Unix.
#[cfg(windows)]
fn base_dir() -> Result<PathBuf> {
    let appdata = std::env::var("APPDATA").context("no se pudo leer %APPDATA%")?;
    Ok(PathBuf::from(appdata))
}

#[cfg(unix)]
fn base_dir() -> Result<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return Ok(PathBuf::from(xdg));
        }
    }
    let home = std::env::var("HOME").context("no se pudo leer $HOME")?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

/// `%APPDATA%\factorio-discord-rp` (Windows) o
/// `~/.local/share/factorio-discord-rp` (Unix), creándola si hace falta.
pub fn app_dir() -> Result<PathBuf> {
    let dir = base_dir()?.join(APP_DIR_NAME);
    std::fs::create_dir_all(&dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
    Ok(dir)
}

/// Ubicación estable de la configuración. Es la que usa el autoarranque.
pub fn config_in_app_dir() -> Option<PathBuf> {
    app_dir().ok().map(|dir| dir.join(CONFIG_FILE))
}

pub fn log_path() -> Result<PathBuf> {
    Ok(app_dir()?.join(LOG_FILE))
}

/// Ruta absoluta del ejecutable, necesaria para registrar el autoarranque.
pub fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("no se pudo determinar la ruta del ejecutable")
}
