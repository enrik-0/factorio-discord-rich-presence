//! Lectura incremental de `factorio-current.log`.
//!
//! Es la única fuente para el nombre del save: la API Lua de Factorio no lo
//! expone. También cubre el modo degradado, cuando el mod no está instalado.
//!
//! Formatos reales (capturados de Factorio 2.1.17):
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
    /// Restos de una línea incompleta: el juego puede estar escribiéndola.
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

    /// Lee lo que se haya añadido desde la última llamada.
    pub fn poll(&mut self) {
        let mut file = match std::fs::File::open(&self.path) {
            Ok(file) => file,
            Err(err) => {
                debug!(%err, ruta = %self.path.display(), "log no accesible");
                return;
            }
        };

        let len = match file.metadata() {
            Ok(meta) => meta.len(),
            Err(_) => return,
        };

        // Factorio rota el log en cada arranque: current pasa a previous y se
        // crea uno nuevo. Si encoge, empezamos de cero y olvidamos lo anterior.
        if len < self.offset {
            debug!("el log se ha reiniciado; releyendo desde el principio");
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

        // El log es ASCII salvo por nombres de save o rutas con acentos.
        let text = String::from_utf8_lossy(&buffer);
        let mut pending = std::mem::take(&mut self.partial);
        pending.push_str(&text);

        // Si el fragmento no acaba en salto de línea, la última línea está a
        // medio escribir: se guarda para la próxima pasada.
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
                    // El fichero es un temporal de descarga: no es un nombre útil.
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

    // La ruta puede contener ':' (letra de unidad), así que el sufijo se quita
    // por el final, no partiendo por el primer ':'.
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

    // Al unirse a un servidor, Factorio carga el mapa descargado en temp/.
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

/// Ruta del log dentro de la carpeta de datos de Factorio.
pub fn default_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("factorio-current.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Líneas literales capturadas de una instalación real de Factorio 2.1.17.
    const HEADER: &str =
        "   0.002 2026-08-29 21:44:28; Factorio 2.1.17 (build 87315, win64, steam, space-age)";
    const LOADING: &str = " 707.020 Loading map D:\\factorio\\saves\\SI.zip: 10645541 bytes.";
    const MP_DOWNLOAD: &str =
        "  47.399 Loading map C:\\Users\\shaojie\\AppData\\Roaming\\Factorio\\temp\\mp-download.zip";
    const MP_STATE: &str = "  47.399 Info ClientMultiplayerManager.cpp:514: MapTick(-1) changing \
                            state from(ConnectedDownloadingMap) to(ConnectedLoadingMap)";

    #[test]
    fn extrae_la_version_del_juego() {
        assert_eq!(parse_version(HEADER).as_deref(), Some("2.1.17"));
        assert_eq!(parse_version(LOADING), None);
    }

    #[test]
    fn extrae_el_nombre_del_save_quitando_el_sufijo_de_bytes() {
        match parse_loading_map(LOADING) {
            Some(LoadedMap::Save(name)) => assert_eq!(name, "SI"),
            _ => panic!("debería reconocer un save normal"),
        }
    }

    #[test]
    fn una_ruta_sin_sufijo_de_bytes_tambien_vale() {
        let line = " 707.020 Loading map /home/villa/.factorio/saves/Mi Partida.zip";
        match parse_loading_map(line) {
            Some(LoadedMap::Save(name)) => assert_eq!(name, "Mi Partida"),
            _ => panic!("debería reconocer rutas unix"),
        }
    }

    #[test]
    fn el_mapa_descargado_de_multijugador_no_es_un_nombre_de_save() {
        assert!(matches!(
            parse_loading_map(MP_DOWNLOAD),
            Some(LoadedMap::MultiplayerDownload)
        ));
    }

    #[test]
    fn secuencia_completa_deja_los_hechos_correctos() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(HEADER);
        watcher.apply_line(LOADING);
        assert_eq!(watcher.facts().game_version.as_deref(), Some("2.1.17"));
        assert_eq!(watcher.facts().save_name.as_deref(), Some("SI"));
        assert_eq!(watcher.facts().multiplayer, Some(false));
    }

    #[test]
    fn unirse_a_un_servidor_marca_multijugador_y_borra_el_save() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(LOADING);
        assert_eq!(watcher.facts().save_name.as_deref(), Some("SI"));

        watcher.apply_line(MP_DOWNLOAD);
        assert_eq!(watcher.facts().save_name, None);
        assert_eq!(watcher.facts().multiplayer, Some(true));
    }

    #[test]
    fn las_transiciones_de_estado_marcan_multijugador() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line(MP_STATE);
        assert_eq!(watcher.facts().multiplayer, Some(true));
    }

    #[test]
    fn lineas_irrelevantes_no_cambian_nada() {
        let mut watcher = LogWatcher::new("no-existe");
        watcher.apply_line("   0.232 Memory info:");
        watcher
            .apply_line("  28.825 Loading mod Redrawn-Space-Connections 2.5.0 (data-updates.lua)");
        assert_eq!(watcher.facts(), &LogFacts::default());
    }
}
