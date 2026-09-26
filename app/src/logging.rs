//! Registro de la aplicación.
//!
//! En modo bandeja no hay consola donde mirar, así que los mensajes van a un
//! fichero en `%APPDATA%`. En modo consola siguen saliendo por pantalla.

use std::fs::OpenOptions;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

/// A partir de este tamaño el log se vacía al arrancar.
///
/// Rotar de verdad exigiría otra dependencia para algo que crece unas pocas
/// líneas por minuto; truncar al arrancar mantiene el fichero acotado y es
/// suficiente para diagnosticar la sesión en curso.
const MAX_LOG_BYTES: u64 = 1024 * 1024;

fn filter() -> EnvFilter {
    EnvFilter::try_from_env("FACTORIO_DRP_LOG")
        .unwrap_or_else(|_| EnvFilter::new("factorio_discord_rp=info"))
}

/// Registro por consola, para los modos de línea de órdenes.
pub fn init_console() {
    tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_target(false)
        .init();
}

/// Registro a fichero, para cuando corre en la bandeja sin consola.
///
/// Devuelve la ruta del fichero, para poder abrirla desde el menú.
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
        .with_context(|| format!("no se pudo abrir el log en {}", path.display()))?;

    tracing_subscriber::fmt()
        .with_env_filter(filter())
        .with_target(false)
        .with_ansi(false)
        .with_writer(Mutex::new(file))
        .init();

    Ok(path)
}
