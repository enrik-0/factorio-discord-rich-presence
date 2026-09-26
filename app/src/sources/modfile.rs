//! Lectura de `script-output/discord-rp/state.json`, el fichero que escribe el mod.
//!
//! Se sondea en vez de vigilarse con un watcher del sistema de ficheros: Discord
//! sólo acepta una actualización cada 15 s, así que un sondeo cada pocos segundos
//! va sobrado y evita hilos, colas y antirrebotes.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{debug, warn};

use crate::model::{ModState, SUPPORTED_SCHEMA};

/// Ruta relativa dentro de `script-output`, en componentes para que el separador
/// sea el nativo. Debe coincidir con `OUTPUT_FILE` del mod.
pub const RELATIVE_PARTS: [&str; 2] = ["discord-rp", "state.json"];

/// Si el fichero deja de actualizarse durante este tiempo, damos el estado por
/// muerto: la partida está pausada, en el menú, o el mod se ha desactivado.
const STALE_AFTER: Duration = Duration::from_secs(20);

pub struct ModFileWatcher {
    path: PathBuf,
    last_seq: Option<u64>,
    /// Fecha del fichero, no de cuándo lo leímos nosotros.
    ///
    /// Usar nuestro propio reloj daba por fresco un fichero de hace horas: al
    /// arrancar no hay historial con el que comparar, así que la primera lectura
    /// parecía siempre reciente aunque Factorio llevara cerrado desde ayer.
    modified: Option<SystemTime>,
    state: Option<ModState>,
    /// Evita repetir el mismo aviso en cada sondeo.
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

    /// Estado vigente, o `None` si no hay fichero o está rancio.
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
        // Un reloj desajustado puede dar una fecha futura; ante la duda, fresco.
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
                // Leer a la vez que el mod escribe da JSON truncado. No es un
                // error: el siguiente sondeo lo pillará entero.
                debug!(%err, "state.json ilegible (probable escritura a medias)");
                return;
            }
        };

        if parsed.schema != SUPPORTED_SCHEMA {
            if !self.warned_schema {
                warn!(
                    encontrado = parsed.schema,
                    soportado = SUPPORTED_SCHEMA,
                    "el mod usa un formato distinto; se ignoran sus datos. Actualiza la aplicación."
                );
                self.warned_schema = true;
            }
            self.state = None;
            return;
        }
        self.warned_schema = false;

        // Los tiempos del JSON describen el momento en que el mod escribió, no el
        // de esta lectura: la fecha del fichero es el ancla del cronómetro.
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
    fn lee_un_estado_valido() {
        let dir = tempdir("valido");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert_eq!(watcher.state().map(|s| s.seq), Some(1));
    }

    #[test]
    fn rechaza_un_schema_desconocido() {
        let dir = tempdir("schema");
        let mut watcher = watcher_with(&dir, &payload(1, 999));
        watcher.poll();
        assert!(watcher.state().is_none());
    }

    #[test]
    fn json_truncado_no_borra_el_estado_anterior() {
        let dir = tempdir("truncado");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert!(watcher.state().is_some());

        // Simula una lectura pillando al mod a media escritura.
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
            "debe conservar el último estado bueno"
        );
    }

    #[test]
    fn fichero_ausente_no_produce_estado() {
        let dir = tempdir("ausente");
        let mut watcher = ModFileWatcher::new(&dir);
        watcher.poll();
        assert!(watcher.state().is_none());
    }

    #[test]
    fn un_fichero_viejo_se_descarta_aunque_sea_la_primera_lectura() {
        // Ocurre al arrancar la aplicación con Factorio recién abierto: el
        // state.json de la sesión anterior sigue en disco, y sin historial
        // propio parecería recién escrito.
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
            "un fichero de hace una hora no describe la partida actual"
        );
    }

    #[test]
    fn la_fecha_del_fichero_es_el_ancla_del_cronometro() {
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
    fn un_fichero_recien_escrito_se_acepta() {
        let dir = tempdir("reciente");
        let mut watcher = watcher_with(&dir, &payload(1, SUPPORTED_SCHEMA));
        watcher.poll();
        assert!(watcher.state().is_some());
    }
}
