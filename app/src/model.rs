//! Estado unificado de la partida y el formato que produce el mod.

use serde::Deserialize;

/// Versión del contrato con el mod. Un `schema` distinto se rechaza en lugar de
/// interpretarse mal.
pub const SUPPORTED_SCHEMA: u32 = 2;

/// Lo que la aplicación cree que está pasando, fusionado de todas las fuentes.
///
/// Todo es opcional: cada fuente aporta lo que sabe y ninguna es obligatoria.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameState {
    /// ¿Hay un proceso de Factorio vivo? Es la condición maestra.
    pub running: bool,

    // --- del log ---
    pub save_name: Option<String>,
    pub game_version: Option<String>,
    pub server_address: Option<String>,

    // --- del mod ---
    pub player_name: Option<String>,
    pub controller: Option<String>,
    pub surface: Option<Surface>,
    pub research: Option<Research>,
    pub players_online: Option<u32>,
    pub ticks_played: Option<u64>,
    pub rockets_launched: Option<u64>,
    pub mod_count: Option<u32>,
    pub overhaul: Option<String>,
    pub evolution: Option<f64>,
    /// Tiempo de esta sesión, frente a `ticks_played` que es el del save.
    pub session_ticks: Option<u64>,
    /// Instante Unix (segundos) en que el mod escribió los datos de arriba.
    ///
    /// Es el ancla del cronómetro: `ticks_played` y `session_ticks` describen ese
    /// instante, no el de nuestro sondeo.
    pub sampled_at: Option<i64>,
    /// Qué quiere ver el jugador. `None` en modo degradado, sin el mod.
    pub display: Option<Display>,

    /// El mod y el log pueden saberlo por separado.
    pub multiplayer: Option<bool>,
}

impl GameState {
    /// ¿Está el mod aportando datos, o vamos en modo degradado?
    pub fn has_mod_data(&self) -> bool {
        self.surface.is_some() || self.research.is_some() || self.ticks_played.is_some()
    }

    /// Tiempo jugado del save, en segundos. Factorio corre a 60 ticks/segundo.
    pub fn playtime_secs(&self) -> Option<i64> {
        self.ticks_played.map(|ticks| (ticks / 60) as i64)
    }

    /// Tiempo de la sesión actual, en segundos.
    pub fn session_secs(&self) -> Option<i64> {
        self.session_ticks.map(|ticks| (ticks / 60) as i64)
    }

    /// Preferencias del jugador, o las de fábrica si el mod no está.
    pub fn display(&self) -> Display {
        self.display.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Surface {
    pub name: String,
    /// `planet`, `platform` u `other`.
    pub kind: String,
    /// Sólo presente cuando `kind == "planet"`.
    pub planet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Research {
    /// Nombre crudo del prototipo. `None` si no hay nada en cola.
    pub current: Option<String>,
    /// Nombre traducido al idioma del jugador, si ya llegó la traducción.
    #[serde(default, deserialize_with = "deserialize_repaired")]
    pub current_label: Option<String>,
    pub progress: Option<f64>,
    pub done: u32,
    pub total: u32,
}

impl Research {
    /// Etiqueta a mostrar: la traducida si existe, si no el nombre del prototipo.
    pub fn label(&self) -> Option<&str> {
        self.current_label.as_deref().or(self.current.as_deref())
    }

    pub fn percent(&self) -> Option<u32> {
        self.progress
            .map(|p| (p.clamp(0.0, 1.0) * 100.0).round() as u32)
    }
}

//------------------------------------------------------------------------------
// Preferencias de visualización
//------------------------------------------------------------------------------

/// Qué campos quiere ver el jugador. Se configura en los ajustes del mod, dentro
/// del juego, y llega en cada escritura.
///
/// El fichero de estado nunca sale del equipo, así que elegir qué se muestra es
/// a la vez el control de privacidad: lo único que ven los demás es la tarjeta.
/// Los valores por defecto replican los del mod, para que el modo degradado
/// (sin mod instalado) se comporte igual.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Display {
    pub save: bool,
    pub planet: bool,
    pub overhaul: bool,
    pub research: bool,
    pub tech_count: bool,
    pub evolution: bool,
    pub rockets: bool,
    pub mod_count: bool,
    pub mode: bool,
    pub player_name: bool,
    pub server: bool,
    pub timer: TimerMode,
}

impl Default for Display {
    fn default() -> Self {
        Self {
            save: true,
            planet: true,
            overhaul: false,
            research: true,
            tech_count: true,
            evolution: false,
            rockets: true,
            mod_count: false,
            mode: true,
            player_name: false,
            server: false,
            timer: TimerMode::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimerMode {
    /// Tiempo jugado acumulado de la partida.
    #[default]
    Save,
    /// Tiempo de la sesión actual.
    Session,
    /// Sin cronómetro.
    None,
}

//------------------------------------------------------------------------------
// Reparación de la codificación
//------------------------------------------------------------------------------

/// Deshace el doble escapado que produce `helpers.table_to_json` de Factorio.
///
/// Las traducciones que devuelve `on_string_translated` son cadenas UTF-8, pero
/// `table_to_json` trata cada **byte** como si fuera un carácter y lo escapa por
/// separado. Así, `ó` (bytes `C3 B3`) sale del mod como `Ã³`, es decir
/// `Ã³`. Sin esto, toda tecnología con acento llegaría ilegible.
///
/// La reparación sólo se aplica cuando reinterpretar los caracteres como bytes
/// produce UTF-8 válido, así que un texto correcto nunca se estropea: una `ó`
/// legítima es el byte `F3`, que por sí solo no es UTF-8 válido y se descarta.
fn repair_mojibake(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    // Si hay algún carácter fuera del rango de un byte, no viene de este defecto.
    if text.chars().any(|c| c as u32 > 0xFF) {
        return text.to_string();
    }

    let bytes: Vec<u8> = text.chars().map(|c| c as u8).collect();
    match String::from_utf8(bytes) {
        Ok(repaired) => repaired,
        Err(_) => text.to_string(),
    }
}

fn deserialize_repaired<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.map(|text| repair_mojibake(&text)))
}

//------------------------------------------------------------------------------
// Formato del fichero que escribe el mod
//------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct ModState {
    pub schema: u32,
    pub seq: u64,
    pub player: ModPlayer,
    pub game: ModGame,
    #[serde(default)]
    pub mods: ModMods,
    #[serde(default)]
    pub rockets_launched: u64,
    #[serde(default)]
    pub surface: Option<Surface>,
    #[serde(default)]
    pub research: Option<Research>,
    #[serde(default)]
    pub evolution: Option<f64>,
    #[serde(default)]
    pub display: Option<Display>,
    /// No viene en el JSON: lo rellena el lector con la fecha del fichero.
    #[serde(skip)]
    pub sampled_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModPlayer {
    pub name: String,
    pub controller: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModGame {
    pub multiplayer: bool,
    pub players_online: u32,
    pub ticks_played: u64,
    #[serde(default)]
    pub session_ticks: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ModMods {
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub overhaul: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "schema": 1,
        "seq": 412,
        "player": { "name": "villa", "index": 1, "controller": "character" },
        "surface": { "name": "fulgora", "kind": "planet", "planet": "fulgora" },
        "research": {
            "current": "electromagnetic-plant",
            "current_label": "Planta electromagnética",
            "progress": 0.64,
            "done": 142,
            "total": 247
        },
        "game": { "multiplayer": true, "players_online": 4, "ticks_played": 1236600, "speed": 1.0 },
        "mods": { "count": 7, "overhaul": "space-age" },
        "rockets_launched": 12
    }"#;

    #[test]
    fn deserializa_el_payload_completo() {
        let state: ModState = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(state.schema, 1);
        assert_eq!(state.seq, 412);
        assert_eq!(state.player.name, "villa");
        assert_eq!(state.surface.unwrap().planet.unwrap(), "fulgora");
        assert_eq!(state.game.players_online, 4);
    }

    #[test]
    fn tolera_secciones_opcionales_ausentes() {
        // Ocurre de verdad: el jugador puede apagar "compartir planeta" e
        // "investigación" en los ajustes por usuario.
        let json = r#"{
            "schema": 1, "seq": 1,
            "player": { "name": "a", "index": 1, "controller": "god" },
            "game": { "multiplayer": false, "players_online": 1, "ticks_played": 60 }
        }"#;
        let state: ModState = serde_json::from_str(json).unwrap();
        assert!(state.surface.is_none());
        assert!(state.research.is_none());
        assert_eq!(state.mods.count, 0);
    }

    #[test]
    fn etiqueta_de_investigacion_cae_al_nombre_crudo() {
        let research = Research {
            current: Some("electromagnetic-plant".into()),
            current_label: None,
            progress: Some(0.5),
            done: 1,
            total: 2,
        };
        assert_eq!(research.label(), Some("electromagnetic-plant"));
        assert_eq!(research.percent(), Some(50));
    }

    #[test]
    fn porcentaje_se_recorta_al_rango_valido() {
        let research = Research {
            current: Some("x".into()),
            current_label: None,
            progress: Some(1.4),
            done: 0,
            total: 0,
        };
        assert_eq!(research.percent(), Some(100));
    }

    #[test]
    fn repara_los_acentos_que_rompe_table_to_json() {
        // Capturado literalmente del state.json de una partida real en español.
        assert_eq!(
            repair_mojibake("ExtracciÃ³n de petrÃ³leo"),
            "Extracción de petróleo"
        );
    }

    #[test]
    fn no_toca_un_texto_ya_correcto() {
        assert_eq!(
            repair_mojibake("Extracción de petróleo"),
            "Extracción de petróleo"
        );
        assert_eq!(repair_mojibake("Oil gathering"), "Oil gathering");
        assert_eq!(repair_mojibake("物流"), "物流");
    }

    #[test]
    fn la_reparacion_llega_hasta_el_payload() {
        let json = r#"{
            "schema": 1, "seq": 1,
            "player": { "name": "enrik0", "index": 1, "controller": "character" },
            "game": { "multiplayer": false, "players_online": 1, "ticks_played": 60 },
            "research": {
                "current": "oil-gathering",
                "current_label": "ExtracciÃ³n de petrÃ³leo",
                "progress": 0.57, "done": 41, "total": 1510
            }
        }"#;
        let state: ModState = serde_json::from_str(json).unwrap();
        assert_eq!(
            state.research.unwrap().label(),
            Some("Extracción de petróleo")
        );
    }

    #[test]
    fn tiempo_jugado_convierte_ticks_a_segundos() {
        let state = GameState {
            ticks_played: Some(1_236_600),
            ..Default::default()
        };
        // 1236600 / 60 = 20610 s = 5 h 43 min 30 s
        assert_eq!(state.playtime_secs(), Some(20_610));
    }
}
