//! Reads `script-output/discord-rp/state.json`, the file the mod writes.
//!
//! It's polled instead of watched with a filesystem watcher: Discord only
//! accepts one update every 15 s, so polling every few seconds is more than
//! enough and avoids threads, queues, and debouncing.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{debug, warn};

use crate::model::{ModState, SUPPORTED_SCHEMA};

/// Relative path inside `script-output`, as components so the separator is
/// native. Must match `OUTPUT_FILE` in the mod.
pub const RELATIVE_PARTS: [&str; 2] = ["discord-rp", "state.json"];

/// If the file stops being updated for this long, the state is considered
/// dead: the game is paused, in the menu, or the mod has been disabled.
const STALE_AFTER: Duration = Duration::from_secs(20);

pub struct ModFileWatcher {
    path: PathBuf,
    last_seq: Option<u64>,
    /// The file's timestamp, not when we read it.
    ///
    /// Using our own clock made a file from hours ago look fresh: at startup
    /// there's no history to compare against, so the first read always
    /// looked recent even if Factorio had been closed since yesterday.
    modified: Option<SystemTime>,
    state: Option<ModState>,
    /// Avoids repeating the same warning on every poll.
    warned_schema: bool,
}

impl ModFileWatcher {
    pub fn new(script_output: &Path) -> Self {
        Self {
            path: RELATIVE_PARTS
                .iter()
                .fold(script_output.to_path_buf(), |acc, part| acc.join(part)),
            last_seq: None,
            modified: None,
            state: None,
            warned_schema: false,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current state, or `None` if there's no file or it's stale.
    pub fn state(&self) -> Option<&ModState> {
        if self.is_stale() {
            return None;
        }
        self.state.as_ref()
    }

    fn is_stale(&self) -> bool {
        let Some(modified) = self.modified else {
            return true;
        };
        // A misaligned clock could give a future date; when in doubt, treat as fresh.
        modified.elapsed().unwrap_or(Duration::ZERO) > STALE_AFTER
    }

    pub fn poll(&mut self) {
        self.modified = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();

        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(_) => return,
        };

        let mut parsed: ModState = match serde_json::from_str(&text) {
            Ok(parsed) => parsed,
            Err(err) => {
                // Reading while the mod is writing produces truncated JSON.
                // It's not an error: the next poll will catch it whole.
                debug!(%err, "state.json unreadable (likely a partial write)");
                return;
            }
        };

        if parsed.schema != SUPPORTED_SCHEMA {
            if !self.warned_schema {
                warn!(
                    found = parsed.schema,
                    supported = SUPPORTED_SCHEMA,
                    "the mod is using a different format; its data is being ignored. Update the app."
                );
                self.warned_schema = true;
            }
            self.state = None;
            return;
        }
        self.warned_schema = false;

        // The timestamps in the JSON describe when the mod wrote it, not
        // when this read happened: the file's timestamp is the clock's anchor.
        parsed.sampled_at = self
            .modified
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|since_epoch| since_epoch.as_secs() as i64);

        self.last_seq = Some(parsed.seq);
        self.state = Some(parsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(seq: u64, schema: u32) -> String {
        format!(
            r#"{{"schema":{schema},"seq":{seq},
               "player":{{"name":"villa","index":1,"controller":"character"}},
               "game":{{"multiplayer":false,"players_online":1,"ticks_played":600}}}}"#
        )
    }

    fn watcher_with(dir: &Path, body: &str) -> ModFileWatcher {
        let full = RELATIVE_PARTS
            .iter()
            .fold(dir.to_path_buf(), |acc, p| acc.join(p));
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
        ModFileWatcher::new(dir)
    }

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("drp-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_a_valid_state() {
        let dir = tempdir("valido");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert_eq!(watcher.state().map(|s| s.seq), Some(1));
    }

    #[test]
    fn rejects_an_unknown_schema() {
        let dir = tempdir("schema");
        let mut watcher = watcher_with(&dir, &payload(1, 999));
        watcher.poll();
        assert!(watcher.state().is_none());
    }

    #[test]
    fn truncated_json_does_not_clear_the_previous_state() {
        let dir = tempdir("truncado");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert!(watcher.state().is_some());

        // Simulates a read catching the mod mid-write.
        let full = RELATIVE_PARTS
            .iter()
            .fold(dir.clone(), |acc, p| acc.join(p));
        std::fs::write(
            full,
            format!(r#"{{"schema":{SUPPORTED_SCHEMA},"seq":2,"pla"#),
        )
        .unwrap();
        watcher.poll();
        assert_eq!(
            watcher.state().map(|s| s.seq),
            Some(1),
            "should keep the last good state"
        );
    }

    #[test]
    fn missing_file_produces_no_state() {
        let dir = tempdir("ausente");
        let mut watcher = ModFileWatcher::new(&dir);
        watcher.poll();
        assert!(watcher.state().is_none());
    }

    #[test]
    fn an_old_file_is_discarded_even_on_the_first_read() {
        // This happens when starting the app with Factorio freshly open: the
        // previous session's state.json is still on disk, and without its
        // own history it would look freshly written.
        let dir = tempdir("viejo");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));

        let full = RELATIVE_PARTS
            .iter()
            .fold(dir.clone(), |acc, p| acc.join(p));
        let antiguo = SystemTime::now() - Duration::from_secs(3600);
        filetime::set_file_mtime(&full, antiguo.into()).unwrap();

        watcher.poll();
        assert!(
            watcher.state().is_none(),
            "a file from an hour ago doesn't describe the current game"
        );
    }

    #[test]
    fn the_file_date_is_the_timer_anchor() {
        let dir = tempdir("ancla");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));

        let full = RELATIVE_PARTS
            .iter()
            .fold(dir.clone(), |acc, p| acc.join(p));
        let escrito = SystemTime::now() - Duration::from_secs(7);
        filetime::set_file_mtime(&full, escrito.into()).unwrap();

        watcher.poll();
        let esperado = escrito.duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        assert_eq!(watcher.state().and_then(|s| s.sampled_at), Some(esperado));
    }

    #[test]
    fn a_freshly_written_file_is_accepted() {
        let dir = tempdir("reciente");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert!(watcher.state().is_some());
    }
}
