//! Icono en la bandeja del sistema.
//!
//! Sin él, la aplicación en modo autoarranque sería un proceso invisible sin
//! forma de saber si funciona ni de cerrarlo. El icono es lo que hace aceptable
//! el autoarranque, no un añadido decorativo.
//!
//! El icono de bandeja de Windows necesita una cola de mensajes en el hilo
//! principal, así que la vigilancia se va a un hilo trabajador y aquí se queda
//! el bucle de mensajes.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::autostart;
use crate::config::Config;
use crate::status::Shared;

/// Cada cuánto se refresca el texto emergente del icono.
const REFRESH: Duration = Duration::from_secs(2);

const ICON_SIZE: u32 = 32;

pub fn run(config: Config, log_path: Option<std::path::PathBuf>) -> Result<()> {
    let shared = Arc::new(Shared::default());

    // La vigilancia no puede vivir aquí: este hilo se queda atendiendo mensajes.
    let worker = {
        let shared = Arc::clone(&shared);
        let config = config.clone();
        std::thread::Builder::new()
            .name("vigilancia".into())
            .spawn(move || {
                if let Err(err) = crate::run::run(&config, &shared) {
                    error!(%err, "la vigilancia se ha detenido");
                }
            })
            .context("no se pudo lanzar el hilo de vigilancia")?
    };

    let menu = Menu::new();
    let status_item = MenuItem::new("Iniciando…", false, None);
    let autostart_item =
        CheckMenuItem::new("Arrancar con Windows", true, autostart::is_enabled(), None);
    let log_item = MenuItem::new("Abrir el registro", log_path.is_some(), None);
    let quit_item = MenuItem::new("Salir", true, None);

    menu.append_items(&[
        &status_item,
        &PredefinedMenuItem::separator(),
        &autostart_item,
        &log_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .context("no se pudo construir el menú de la bandeja")?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Factorio Discord Rich Presence")
        .with_icon(build_icon()?)
        .build()
        .context("no se pudo crear el icono de la bandeja")?;

    info!("icono de bandeja listo");

    pump_messages(
        &tray,
        &shared,
        &status_item,
        &autostart_item,
        &log_item,
        &quit_item,
        log_path.as_deref(),
    );

    shared.request_shutdown();
    let _ = worker.join();
    Ok(())
}

/// Bucle de mensajes de Windows, con refresco periódico del texto emergente.
#[allow(clippy::too_many_arguments)]
fn pump_messages(
    tray: &TrayIcon,
    shared: &Shared,
    status_item: &MenuItem,
    autostart_item: &CheckMenuItem,
    log_item: &MenuItem,
    quit_item: &MenuItem,
    log_path: Option<&std::path::Path>,
) {
    let menu_channel = MenuEvent::receiver();
    let mut last_tooltip = String::new();

    loop {
        if !windows::pump_once(REFRESH) {
            break;
        }

        while let Ok(event) = menu_channel.try_recv() {
            if event.id == *quit_item.id() {
                return;
            } else if event.id == *autostart_item.id() {
                // El elemento ya ha cambiado su marca; el registro debe seguirla.
                let wanted = autostart_item.is_checked();
                if let Err(err) = autostart::set_enabled(wanted) {
                    warn!(%err, "no se pudo cambiar el autoarranque");
                    // Deshacer la marca para no mentir sobre el estado real.
                    autostart_item.set_checked(!wanted);
                }
            } else if event.id == *log_item.id() {
                if let Some(path) = log_path {
                    windows::open_in_explorer(path);
                }
            }
        }

        let status = shared.snapshot();
        let tooltip = status.tooltip();
        if tooltip != last_tooltip {
            let _ = tray.set_tooltip(Some(&tooltip));
            // La primera línea del emergente es el título; en el menú basta el resto.
            let resumen = tooltip.lines().skip(1).collect::<Vec<_>>().join(" — ");
            status_item.set_text(resumen);
            last_tooltip = tooltip;
        }
    }
}

/// Icono dibujado en código: un anillo naranja, guiño al engranaje de Factorio.
///
/// Generarlo evita arrastrar un fichero de imagen y una dependencia para
/// decodificarlo, por 32×32 píxeles que nadie mira de cerca.
fn build_icon() -> Result<Icon> {
    const NARANJA: [u8; 3] = [0xE8, 0x8A, 0x1E];

    let size = ICON_SIZE as i32;
    let centro = (size - 1) as f32 / 2.0;
    let radio_exterior = centro;
    let radio_interior = centro * 0.45;

    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - centro;
            let dy = y as f32 - centro;
            let distancia = (dx * dx + dy * dy).sqrt();
            let dentro = distancia <= radio_exterior && distancia >= radio_interior;

            if dentro {
                rgba.extend_from_slice(&[NARANJA[0], NARANJA[1], NARANJA[2], 0xFF]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).context("no se pudo construir el icono")
}

//------------------------------------------------------------------------------
// Envoltorios de Win32
//------------------------------------------------------------------------------

mod windows {
    use std::time::Duration;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };

    /// Atiende los mensajes pendientes y espera hasta `timeout`.
    ///
    /// Se usa `PeekMessage` en vez de `GetMessage` porque además de los mensajes
    /// hay que refrescar el estado periódicamente, y `GetMessage` bloquearía sin
    /// mensajes que atender.
    ///
    /// Devuelve `false` si Windows pide cerrar.
    pub fn pump_once(timeout: Duration) -> bool {
        const PASO: Duration = Duration::from_millis(50);
        let mut restante = timeout;

        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                if msg.message == WM_QUIT {
                    return false;
                }
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }

            if restante == Duration::ZERO {
                return true;
            }
            let paso = restante.min(PASO);
            std::thread::sleep(paso);
            restante -= paso;
        }
    }

    /// Abre una ruta con la aplicación asociada del sistema.
    pub fn open_in_explorer(path: &std::path::Path) {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path.to_string_lossy()])
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_icono_tiene_el_tamano_declarado() {
        // `from_rgba` falla si el buffer no cuadra con las dimensiones, así que
        // construirlo ya valida la geometría.
        assert!(build_icon().is_ok());
    }
}
