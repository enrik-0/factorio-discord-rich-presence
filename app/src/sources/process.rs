//! Detection of the Factorio process.
//!
//! This is the master condition: with no live process there's nothing to
//! publish, and the activity must be cleared so the profile isn't left
//! hanging.

use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

/// Executable names depending on platform and installation.
const EXECUTABLES: &[&str] = &["factorio.exe", "factorio"];

/// Enumerating processes isn't free; no more resolution than this is needed.
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
