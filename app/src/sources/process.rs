//! Detección del proceso de Factorio.
//!
//! Es la condición maestra: sin proceso vivo no hay nada que publicar, y hay que
//! borrar la actividad para no dejar el perfil colgado.

use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Nombres del ejecutable según plataforma e instalación.
const EXECUTABLES: &[&str] = &["factorio.exe", "factorio"];

/// Enumerar procesos no es gratis; no hace falta más resolución que ésta.
const REFRESH_EVERY: Duration = Duration::from_secs(2);

pub struct ProcessWatcher {
    system: System,
    running: bool,
    last_refresh: Option<Instant>,
}

impl Default for ProcessWatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessWatcher {
    pub fn new() -> Self {
        Self {
            system: System::new(),
            running: false,
            last_refresh: None,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn poll(&mut self) {
        if let Some(last) = self.last_refresh {
            if last.elapsed() < REFRESH_EVERY {
                return;
            }
        }
        self.last_refresh = Some(Instant::now());

        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );

        self.running = self.system.processes().values().any(|process| {
            let name = process.name().to_string_lossy().to_ascii_lowercase();
            EXECUTABLES.iter().any(|candidate| name == *candidate)
        });
    }
}
