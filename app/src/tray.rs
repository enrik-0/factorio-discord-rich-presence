//! System tray icon.
//!
//! Without it, the app in autostart mode would be an invisible process with
//! no way to know whether it's working or to close it. The icon is what
//! makes autostart acceptable, not a decorative extra.
//!
//! Windows' tray icon needs a message queue on the main thread, so watching
//! moves to a worker thread and the message loop stays here.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{error, info, warn};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

use crate::autostart;
use crate::config::Config;
use crate::status::Shared;

/// How often the icon's tooltip text refreshes.
const REFRESH: Duration = Duration::from_secs(2);

const ICON_SIZE: u32 = 32;

/// With `exit_with_game`, the app closes itself once Factorio closes.
pub fn run(
    config: Config,
    log_path: Option<std::path::PathBuf>,
    exit_with_game: bool,
) -> Result<()> {
    let shared = Arc::new(Shared::default());

    // Watching can't live here: this thread stays busy handling messages.
    let worker = {
        let shared = Arc::clone(&shared);
        let config = config.clone();
        std::thread::Builder::new()
            .name("watcher".into())
            .spawn(move || {
                if let Err(err) = crate::run::run(&config, &shared, exit_with_game) {
                    error!(%err, "watching has stopped");
                }
                // As a launcher there's nothing left to show: the icon goes
                // with it, instead of being left hanging with no watcher behind it.
                if exit_with_game {
                    shared.request_shutdown();
                }
            })
            .context("could not spawn the watcher thread")?
    };

    let menu = Menu::new();
    let status_item = MenuItem::new("Starting…", false, None);
    let autostart_item =
        CheckMenuItem::new("Start with Windows", true, autostart::is_enabled(), None);
    let log_item = MenuItem::new("Open log", log_path.is_some(), None);
    let quit_item = MenuItem::new("Exit", true, None);

    menu.append_items(&[
        &status_item,
        &PredefinedMenuItem::separator(),
        &autostart_item,
        &log_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ])
    .context("could not build the tray menu")?;

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("Factorio Discord Rich Presence")
        .with_icon(build_icon()?)
        .build()
        .context("could not create the tray icon")?;

    info!("tray icon ready");

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

/// Windows message loop, with periodic tooltip-text refresh.
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

        // The watcher can request shutdown on its own (launcher mode).
        if shared.is_shutdown() {
            return;
        }

        while let Ok(event) = menu_channel.try_recv() {
            if event.id == *quit_item.id() {
                return;
            } else if event.id == *autostart_item.id() {
                // The item has already flipped its own check mark; the
                // registry needs to follow it.
                let wanted = autostart_item.is_checked();
                if let Err(err) = autostart::set_enabled(wanted) {
                    warn!(%err, "could not change autostart");
                    // Undo the check mark so it doesn't lie about the real state.
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
            // The tooltip's first line is the title; the rest is enough for the menu.
            let summary = tooltip.lines().skip(1).collect::<Vec<_>>().join(" — ");
            status_item.set_text(summary);
            last_tooltip = tooltip;
        }
    }
}

/// Icon drawn in code: an orange ring, a nod to Factorio's gear.
///
/// Generating it avoids dragging in an image file and a dependency to
/// decode it, for 32×32 pixels nobody looks at closely.
fn build_icon() -> Result<Icon> {
    const ORANGE: [u8; 3] = [0xE8, 0x8A, 0x1E];

    let size = ICON_SIZE as i32;
    let center = (size - 1) as f32 / 2.0;
    let outer_radius = center;
    let inner_radius = center * 0.45;

    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let inside = distance <= outer_radius && distance >= inner_radius;

            if inside {
                rgba.extend_from_slice(&[ORANGE[0], ORANGE[1], ORANGE[2], 0xFF]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).context("could not build the icon")
}

//------------------------------------------------------------------------------
// Win32 wrappers
//------------------------------------------------------------------------------

mod windows {
    use std::time::Duration;

    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_QUIT,
    };

    /// Handles pending messages and waits up to `timeout`.
    ///
    /// Uses `PeekMessage` instead of `GetMessage` because, besides messages,
    /// the state needs refreshing periodically, and `GetMessage` would block
    /// with no messages to handle.
    ///
    /// Returns `false` if Windows asks to close.
    pub fn pump_once(timeout: Duration) -> bool {
        const STEP: Duration = Duration::from_millis(50);
        let mut remaining = timeout;

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

            if remaining == Duration::ZERO {
                return true;
            }
            let step = remaining.min(STEP);
            std::thread::sleep(step);
            remaining -= step;
        }
    }

    /// Opens a path with the system's associated application.
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
    fn the_icon_has_the_declared_size() {
        // `from_rgba` fails if the buffer doesn't match the dimensions, so
        // building it already validates the geometry.
        assert!(build_icon().is_ok());
    }
}
