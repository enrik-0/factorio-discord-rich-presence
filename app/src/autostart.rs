//! Arranque automático con la sesión de Windows.
//!
//! Se registra en `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`: por
//! usuario, sin permisos de administrador, y visible en el Administrador de
//! tareas, de modo que se puede desactivar desde ahí aunque la aplicación
//! desaparezca.
//!
//! Siempre desactivado de fábrica: meter algo en el arranque del sistema es
//! decisión del usuario, no del programa.

use anyhow::{Context, Result};
use auto_launch::AutoLaunchBuilder;
use tracing::info;

/// Nombre de la entrada en el registro, tal y como se ve en el sistema.
const APP_NAME: &str = "Factorio Discord Rich Presence";

fn launcher() -> Result<auto_launch::AutoLaunch> {
    let exe = crate::paths::current_exe()?;
    let exe = exe.to_string_lossy().to_string();

    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&exe)
        // Sin esto, al arrancar desde el registro se abriría en modo consola.
        .set_args(&["--tray"])
        .build()
        .context("no se pudo preparar el registro de autoarranque")
}

pub fn is_enabled() -> bool {
    launcher()
        .and_then(|l| l.is_enabled().context("consulta de autoarranque"))
        .unwrap_or(false)
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    let launcher = launcher()?;
    if enabled {
        launcher
            .enable()
            .context("no se pudo activar el autoarranque")?;
        info!("autoarranque activado");
    } else {
        launcher
            .disable()
            .context("no se pudo desactivar el autoarranque")?;
        info!("autoarranque desactivado");
    }
    Ok(())
}
