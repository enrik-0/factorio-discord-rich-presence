//! State shared between the watcher thread and the tray icon.
//!
//! Without a console or a window, the icon is the only way to know whether
//! the application is alive and what it's doing. That's why the worker
//! thread publishes what it sees here, and the menu reads it for the tooltip.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub factorio_running: bool,
    pub discord_connected: bool,
    /// `false` when the mod isn't present and we only have the game log.
    pub mod_data: bool,
    /// What's being published right now, to see it at a glance.
    pub headline: Option<String>,
}

impl Status {
    /// The icon's tooltip text. Must fit on one line and say the essentials:
    /// whether Discord or Factorio is missing, and what's being published.
    #[cfg_attr(
        unix,
        allow(
            dead_code,
            reason = "consumed by the Windows tray (tray.rs); no tray on Unix yet"
        )
    )]
    pub fn tooltip(&self) -> String {
        let mut lines = vec!["Factorio Discord Rich Presence".to_string()];

        lines.push(match (self.factorio_running, self.discord_connected) {
            (false, _) => "Factorio is not running".into(),
            (true, false) => "Waiting for Discord…".into(),
            (true, true) if self.mod_data => "Publishing".into(),
            (true, true) => "Publishing (without the mod)".into(),
        });

        if let Some(headline) = &self.headline {
            lines.push(headline.clone());
        }

        lines.join("\n")
    }
}

#[derive(Default)]
pub struct Shared {
    status: Mutex<Status>,
    shutdown: AtomicBool,
}

impl Shared {
    #[cfg_attr(
        unix,
        allow(
            dead_code,
            reason = "consumed by the Windows tray (tray.rs); no tray on Unix yet"
        )
    )]
    pub fn snapshot(&self) -> Status {
        // A poisoned lock shouldn't bring down the application: what's inside
        // is informational, not critical.
        match self.status.lock() {
            Ok(status) => status.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    pub fn update(&self, apply: impl FnOnce(&mut Status)) {
        let mut guard = match self.status.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        apply(&mut guard);
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_text_says_whats_missing() {
        let parado = Status::default();
        assert!(parado.tooltip().contains("Factorio is not running"));

        let sin_discord = Status {
            factorio_running: true,
            ..Default::default()
        };
        assert!(sin_discord.tooltip().contains("Waiting for Discord"));

        let degradado = Status {
            factorio_running: true,
            discord_connected: true,
            mod_data: false,
            headline: Some("claro".into()),
        };
        let texto = degradado.tooltip();
        assert!(texto.contains("without the mod"));
        assert!(texto.contains("claro"));

        let completo = Status {
            mod_data: true,
            ..degradado
        };
        assert!(completo.tooltip().contains("Publishing"));
        assert!(!completo.tooltip().contains("without the mod"));
    }

    #[test]
    fn shutdown_is_requested_once_and_persists() {
        let shared = Shared::default();
        assert!(!shared.is_shutdown());
        shared.request_shutdown();
        assert!(shared.is_shutdown());
        assert!(shared.is_shutdown());
    }

    #[test]
    fn status_is_updated_and_read() {
        let shared = Shared::default();
        shared.update(|status| status.factorio_running = true);
        assert!(shared.snapshot().factorio_running);
    }
}
