//! Merging the sources into a single `GameState`.
//!
//! Priority: the mod overrides the log, and the log overrides mere process
//! detection. Each field is resolved independently, so a mod without the
//! active-planet setting still contributes research while the save comes
//! from the log.

use crate::model::{GameState, ModState};
use crate::sources::logfile::LogFacts;

pub fn merge(running: bool, log: &LogFacts, mod_state: Option<&ModState>) -> GameState {
    let mut state = GameState {
        running,
        ..Default::default()
    };

    if !running {
        // With no process there's nothing to report: leftover data from the
        // other sources would describe a game that no longer exists.
        return state;
    }

    // --- log ---
    state.save_name = log.save_name.clone();
    state.game_version = log.game_version.clone();
    state.multiplayer = log.multiplayer;

    // --- mod (wins where it overlaps) ---
    if let Some(data) = mod_state {
        state.player_name = Some(data.player.name.clone());
        state.controller = Some(data.player.controller.clone());
        state.surface = data.surface.clone();
        state.research = data.research.clone();
        state.players_online = Some(data.game.players_online);
        state.ticks_played = Some(data.game.ticks_played);
        state.rockets_launched = Some(data.rockets_launched);
        state.mod_count = Some(data.mods.count);
        state.overhaul = data.mods.overhaul.clone();
        state.multiplayer = Some(data.game.multiplayer);
        state.evolution = data.evolution;
        state.trees_razed = data.trees_razed;
        state.enemies_killed = data.enemies_killed;
        state.player_deaths = data.player_deaths;
        state.pollution_emitted = data.pollution_emitted;
        state.afk_ticks = data.afk_ticks;
        state.session_ticks = data.game.session_ticks;
        state.sampled_at = data.sampled_at;
        state.display = data.display.clone();
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModGame, ModMods, ModPlayer, Research, Surface};

    fn log_facts() -> LogFacts {
        LogFacts {
            save_name: Some("SI".into()),
            game_version: Some("2.1.17".into()),
            multiplayer: Some(false),
        }
    }

    fn mod_state() -> ModState {
        ModState {
            schema: 2,
            seq: 10,
            player: ModPlayer {
                name: "villa".into(),
                controller: "character".into(),
            },
            game: ModGame {
                multiplayer: true,
                players_online: 4,
                ticks_played: 1_236_600,
                session_ticks: Some(36_000),
            },
            mods: ModMods {
                count: 170,
                overhaul: Some("space-age".into()),
            },
            rockets_launched: 12,
            surface: Some(Surface {
                name: "fulgora".into(),
                kind: "planet".into(),
                planet: Some("fulgora".into()),
            }),
            research: Some(Research {
                current: Some("electromagnetic-plant".into()),
                current_label: Some("Planta electromagnética".into()),
                progress: Some(0.64),
                done: 142,
                total: 247,
            }),
            evolution: Some(0.42),
            trees_razed: Some(1_337),
            enemies_killed: Some(58),
            player_deaths: Some(3),
            pollution_emitted: Some(98_765.4),
            afk_ticks: Some(120),
            display: None,
            sampled_at: Some(1_700_000_000),
        }
    }

    #[test]
    fn with_no_process_the_state_is_empty() {
        let state = merge(false, &log_facts(), Some(&mod_state()));
        assert!(!state.running);
        assert!(state.save_name.is_none());
        assert!(state.surface.is_none());
    }

    #[test]
    fn degraded_mode_uses_only_the_log() {
        let state = merge(true, &log_facts(), None);
        assert_eq!(state.save_name.as_deref(), Some("SI"));
        assert_eq!(state.game_version.as_deref(), Some("2.1.17"));
        assert!(!state.has_mod_data());
    }

    #[test]
    fn the_mod_contributes_what_the_log_does_not_know() {
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(
            state.save_name.as_deref(),
            Some("SI"),
            "the save name comes from the log"
        );
        assert_eq!(
            state.surface.as_ref().unwrap().planet.as_deref(),
            Some("fulgora")
        );
        assert_eq!(state.research.as_ref().unwrap().done, 142);
        assert!(state.has_mod_data());
    }

    #[test]
    fn the_mod_wins_over_the_log_for_multiplayer() {
        // The log says single because it loaded a local save; the mod sees the truth.
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(state.multiplayer, Some(true));
        assert_eq!(state.players_online, Some(4));
    }

    #[test]
    fn the_mods_write_time_reaches_the_state() {
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(state.sampled_at, Some(1_700_000_000));
    }

    #[test]
    fn the_meme_stats_reach_the_state() {
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(state.trees_razed, Some(1_337));
        assert_eq!(state.enemies_killed, Some(58));
        assert_eq!(state.player_deaths, Some(3));
        assert_eq!(state.pollution_emitted, Some(98_765.4));
        assert_eq!(state.afk_ticks, Some(120));
    }

    #[test]
    fn with_no_mod_or_log_only_the_process_remains() {
        let state = merge(true, &LogFacts::default(), None);
        assert!(state.running);
        assert!(state.save_name.is_none());
        assert!(!state.has_mod_data());
    }
}
