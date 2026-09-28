//! Unified game state and the format the mod produces.

use serde::Deserialize;

/// Version of the contract with the mod. A different `schema` is rejected
/// rather than misinterpreted.
pub const SUPPORTED_SCHEMA: u32 = 2;

/// What the app believes is happening, merged from every source.
///
/// Everything is optional: each source contributes what it knows and none
/// of them is mandatory.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GameState {
    /// Is a Factorio process alive? This is the master condition.
    pub running: bool,

    // --- from the log ---
    pub save_name: Option<String>,
    pub game_version: Option<String>,
    pub server_address: Option<String>,

    // --- from the mod ---
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
    /// Time for this session, as opposed to `ticks_played`, which is the save's.
    pub session_ticks: Option<u64>,
    // --- "meme" stats ---
    pub trees_razed: Option<u64>,
    pub enemies_killed: Option<u64>,
    pub player_deaths: Option<u64>,
    /// Across every surface in the game, not just the force's: the API
    /// doesn't allow splitting it by force (see `collect.total_pollution` in
    /// the mod).
    pub pollution_emitted: Option<f64>,
    /// Ticks since the player's last action. A live value: it doesn't come
    /// from a cache, so it may not exactly match what the player sees on
    /// screen at this very instant.
    pub afk_ticks: Option<u64>,
    /// Unix instant (seconds) at which the mod wrote the data above.
    ///
    /// This anchors the timer: `ticks_played` and `session_ticks` describe
    /// that instant, not the moment we polled it.
    pub sampled_at: Option<i64>,
    /// What the player wants to see. `None` in degraded mode, without the mod.
    pub display: Option<Display>,

    /// The mod and the log can each know this independently.
    pub multiplayer: Option<bool>,
}

impl GameState {
    /// Is the mod contributing data, or are we in degraded mode?
    pub fn has_mod_data(&self) -> bool {
        self.surface.is_some() || self.research.is_some() || self.ticks_played.is_some()
    }

    /// Save playtime, in seconds. Factorio runs at 60 ticks/second.
    pub fn playtime_secs(&self) -> Option<i64> {
        self.ticks_played.map(|ticks| (ticks / 60) as i64)
    }

    /// Current session time, in seconds.
    pub fn session_secs(&self) -> Option<i64> {
        self.session_ticks.map(|ticks| (ticks / 60) as i64)
    }

    /// AFK time, in seconds.
    pub fn afk_secs(&self) -> Option<i64> {
        self.afk_ticks.map(|ticks| (ticks / 60) as i64)
    }

    /// Player preferences, or the factory defaults if the mod isn't present.
    pub fn display(&self) -> Display {
        self.display.clone().unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Surface {
    pub name: String,
    /// `planet`, `platform`, or `other`.
    pub kind: String,
    /// Only present when `kind == "planet"`.
    pub planet: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Research {
    /// Raw prototype name. `None` if nothing is queued.
    pub current: Option<String>,
    /// Name translated into the player's language, once the translation has arrived.
    #[serde(default, deserialize_with = "deserialize_repaired")]
    pub current_label: Option<String>,
    pub progress: Option<f64>,
    pub done: u32,
    pub total: u32,
}

impl Research {
    /// Label to display: the translated one if it exists, otherwise the prototype name.
    pub fn label(&self) -> Option<&str> {
        self.current_label.as_deref().or(self.current.as_deref())
    }

    pub fn percent(&self) -> Option<u32> {
        self.progress
            .map(|p| (p.clamp(0.0, 1.0) * 100.0).round() as u32)
    }
}

//------------------------------------------------------------------------------
// Display preferences
//------------------------------------------------------------------------------

/// Which fields the player wants to see. Configured in the mod's in-game
/// settings, and sent with every write.
///
/// The state file never leaves the machine, so choosing what's shown is also
/// the privacy control: the card is the only thing anyone else sees.
/// The defaults mirror the mod's, so degraded mode (mod not installed)
/// behaves the same way.
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
    pub trees: bool,
    pub enemies: bool,
    pub deaths: bool,
    pub pollution: bool,
    pub afk: bool,
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
            trees: false,
            enemies: false,
            deaths: false,
            pollution: false,
            afk: false,
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
    /// Accumulated save playtime.
    #[default]
    Save,
    /// Current session time.
    Session,
    /// No timer.
    None,
}

//------------------------------------------------------------------------------
// Encoding repair
//------------------------------------------------------------------------------

/// Undoes the double-escaping produced by Factorio's `helpers.table_to_json`.
///
/// The translations `on_string_translated` returns are UTF-8 strings, but
/// `table_to_json` treats each **byte** as if it were a character and
/// escapes it separately. So `ó` (bytes `C3 B3`) comes out of the mod as
/// `Ã³`. Without this, every technology with an accent would arrive
/// unreadable.
///
/// The repair only applies when reinterpreting the characters as bytes
/// produces valid UTF-8, so correct text is never damaged: a legitimate `ó`
/// is the byte `F3`, which on its own isn't valid UTF-8 and gets discarded.
fn repair_mojibake(text: &str) -> String {
    if text.is_ascii() {
        return text.to_string();
    }
    // If any character is outside a byte's range, it doesn't come from this defect.
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
// Format of the file the mod writes
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
    pub trees_razed: Option<u64>,
    #[serde(default)]
    pub enemies_killed: Option<u64>,
    #[serde(default)]
    pub player_deaths: Option<u64>,
    #[serde(default)]
    pub pollution_emitted: Option<f64>,
    #[serde(default)]
    pub afk_ticks: Option<u64>,
    #[serde(default)]
    pub display: Option<Display>,
    /// Not present in the JSON: the reader fills this in with the file's date.
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
    fn deserializes_the_full_payload() {
        let state: ModState = serde_json::from_str(SAMPLE).unwrap();
        assert_eq!(state.schema, 1);
        assert_eq!(state.seq, 412);
        assert_eq!(state.player.name, "villa");
        assert_eq!(state.surface.unwrap().planet.unwrap(), "fulgora");
        assert_eq!(state.game.players_online, 4);
    }

    #[test]
    fn tolerates_missing_optional_sections() {
        // This happens for real: the player can turn off "share planet" and
        // "research" in their per-user settings.
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
    fn research_label_falls_back_to_raw_name() {
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
    fn percent_is_clamped_to_the_valid_range() {
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
    fn repairs_the_accents_table_to_json_breaks() {
        // Captured verbatim from the state.json of a real save in Spanish.
        assert_eq!(
            repair_mojibake("ExtracciÃ³n de petrÃ³leo"),
            "Extracción de petróleo"
        );
    }

    #[test]
    fn leaves_already_correct_text_untouched() {
        assert_eq!(
            repair_mojibake("Extracción de petróleo"),
            "Extracción de petróleo"
        );
        assert_eq!(repair_mojibake("Oil gathering"), "Oil gathering");
        assert_eq!(repair_mojibake("物流"), "物流");
    }

    #[test]
    fn the_repair_reaches_all_the_way_to_the_payload() {
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
    fn playtime_converts_ticks_to_seconds() {
        let state = GameState {
            ticks_played: Some(1_236_600),
            ..Default::default()
        };
        // 1236600 / 60 = 20610 s = 5 h 43 min 30 s
        assert_eq!(state.playtime_secs(), Some(20_610));
    }
}
