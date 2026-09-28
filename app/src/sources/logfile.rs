//! Incremental reading of `factorio-current.log`.
//!
//! It's the only source for the save name: the Factorio Lua API doesn't
//! expose it. It also covers degraded mode, when the mod isn't installed.
//!
//! Real formats (captured from Factorio 2.1.17):
//! ```text
//!    0.002 2026-08-29 21:44:28; Factorio 2.1.17 (build 87315, win64, steam, space-age)
//!  707.020 Loading map D:\factorio\saves\SI.zip: 10645541 bytes.
//!   47.399 Info ClientMultiplayerManager.cpp:514: MapTick(-1) changing state from(A) to(B)
//! ```

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use tracing::debug;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogFacts {
    pub save_name: Option<String>,
    pub game_version: Option<String>,
    pub multiplayer: Option<bool>,
}

pub struct LogWatcher {
    path: PathBuf,
    offset: u64,
    facts: LogFacts,
    /// Remainder of an incomplete line: the game may still be writing it.
    partial: String,
}

impl LogWatcher {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            facts: LogFacts::default(),
            partial: String::new(),
        }
    }

    pub fn facts(&self) -> &LogFacts {
        &self.facts
    }

    /// Reads whatever has been appended since the last call.
    pub fn poll(&mut self) {
        let mut file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(err) => {
                debug!(%err, path = %self.path.display(), "log not accessible");
                return;
            }
        };

        let len = match file.metadata() {
            Ok(meta) => meta.len(),
            Err(_) => return,
        };

        // Factorio rotates the log on every startup: current becomes previous
        // and a new one is created. If it shrinks, we start over and forget
        // what came before.
        if len < self.offset {
            debug!("log has been reset; rereading from the start");
            self.offset = 0;
            self.facts = LogFacts::default();
            self.partial.clear();
        }
        if len == self.offset {
            return;
        }

        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return;
        }
        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_err() {
            return;
        }
        self.offset = len;

        // The log is ASCII except for save names or paths with accents.
        let text = String::from_utf8_lossy(&buffer);
        let mut pending = std::mem::take(&mut self.partial);
        pending.push_str(&text);

        // If the chunk doesn't end in a newline, the last line is still
        // being written: it's saved for the next pass.
        let ends_complete = pending.ends_with('\n');
        let mut lines: Vec<&str> = pending.split('\n').collect();
        if !ends_complete {
            self.partial = lines.pop().unwrap_or("").to_string();
        }

        for line in lines {
            self.apply_line(line.trim_end_matches('\r'));
        }
    }

    fn apply_line(&mut self, line: &str) {
        if let Some(version) = parse_version(line) {
            self.facts.game_version = Some(version);
        }
        if let Some(save) = parse_loading_map(line) {
            match save {
                LoadedMap::Save(name) => {
                    self.facts.save_name = Some(name);
                    self.facts.multiplayer = Some(false);
                }
                LoadedMap::MultiplayerDownload => {
                    // The file is a download temp file: not a useful name.
                    self.facts.save_name = None;
                    self.facts.multiplayer = Some(true);
                }
            }
        }
        if line.contains("ClientMultiplayerManager") {
            self.facts.multiplayer = Some(true);
        }
    }
}

/// `... ; Factorio 2.1.17 (build 87315, win64, steam, space-age)` → `2.1.17`
fn parse_version(line: &str) -> Option<String> {
    let rest = line.split_once("; Factorio ")?.1;
    let version = rest.split_once(' ').map(|(v, _)| v).unwrap_or(rest);
    if version.is_empty() {
        return None;
    }
    Some(version.to_string())
}

enum LoadedMap {
    Save(String),
    MultiplayerDownload,
}

/// ` 707.020 Loading map D:\factorio\saves\SI.zip: 10645541 bytes.` → `SI`
fn parse_loading_map(line: &str) -> Option<LoadedMap> {
    let rest = line.split_once(" Loading map ")?.1;

    // The path may contain ':' (drive letter), so the suffix is stripped
    // from the end, not by splitting on the first ':'.
    let path = match rest.rsplit_once(": ") {
        Some((head, tail)) if tail.ends_with(" bytes.") => head,
        _ => rest,
    }
    .trim();

    if path.is_empty() {
        return None;
    }

    let file = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".zip");

    // When joining a server, Factorio loads the downloaded map into temp/.
    if file.eq_ignore_ascii_case("mp-download")
        || path.contains("/temp/")
        || path.contains("\\temp\\")
    {
        return Some(LoadedMap::MultiplayerDownload);
    }

    if file.is_empty() {
        return None;
    }
    Some(LoadedMap::Save(file.to_string()))
}

/// Path to the log inside Factorio's data folder.
pub fn default_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("factorio-current.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Literal lines captured from a real Factorio 2.1.17 installation.
    const HEADER: &str =
        "   0.002 2026-08-29 21:44:28; Factorio 2.1.17 (build 87315, win64, steam, space-age)";
    const LOADING: &str = " 707.020 Loading map D:\\factorio\\saves\\SI.zip: 10645541 bytes.";
    const MP_DOWNLOAD: &str =
        "  47.399 Loading map C:\\Users\\shaojie\\AppData\\Roaming\\Factorio\\temp\\mp-download.zip";
    const MP_STATE: &str = "  47.399 Info ClientMultiplayerManager.cpp:514: MapTick(-1) changing \
                            state from(ConnectedDownloadingMap) to(ConnectedLoadingMap)";

    #[test]
    fn extracts_the_game_version() {
        assert_eq!(parse_version(HEADER).as_deref(), Some("2.1.17"));
        assert_eq!(parse_version(LOADING), None);
    }

    #[test]
    fn extracts_the_save_name_stripping_the_bytes_suffix() {
        match parse_loading_map(LOADING) {
            Some(LoadedMap::Save(name)) => assert_eq!(name, "SI"),
            _ => panic!("should recognize a normal save"),
        }
    }

    #[test]
    fn a_path_without_bytes_suffix_also_works() {
        let line = " 707.020 Loading map /home/villa/.factorio/saves/Mi Partida.zip";
        match parse_loading_map(line) {
            Some(LoadedMap::Save(name)) => assert_eq!(name, "Mi Partida"),
            _ => panic!("should recognize unix paths"),
        }
    }

    #[test]
    fn the_multiplayer_downloaded_map_is_not_a_save_name() {
        assert!(matches!(
            parse_loading_map(MP_DOWNLOAD),
            Some(LoadedMap::MultiplayerDownload)
        ));
    }

    #[test]
    fn full_sequence_leaves_the_correct_facts() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(HEADER);
        watcher.apply_line(LOADING);
        assert_eq!(watcher.facts().game_version.as_deref(), Some("2.1.17"));
        assert_eq!(watcher.facts().save_name.as_deref(), Some("SI"));
        assert_eq!(watcher.facts().multiplayer, Some(false));
    }

    #[test]
    fn joining_a_server_marks_multiplayer_and_clears_the_save() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(LOADING);
        assert_eq!(watcher.facts().save_name.as_deref(), Some("SI"));

        watcher.apply_line(MP_DOWNLOAD);
        assert_eq!(watcher.facts().save_name, None);
        assert_eq!(watcher.facts().multiplayer, Some(true));
    }

    #[test]
    fn state_transitions_mark_multiplayer() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(MP_STATE);
        assert_eq!(watcher.facts().multiplayer, Some(true));
    }

    #[test]
    fn irrelevant_lines_change_nothing() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line("   0.232 Memory info:");
        watcher
            .apply_line("  28.825 Loading mod Redrawn-Space-Connections 2.5.0 (data-updates.lua)");
        assert_eq!(watcher.facts(), &LogFacts::default());
    }
}
