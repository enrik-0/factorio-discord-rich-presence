//! Fusión de las fuentes en un único `GameState`.
//!
//! Prioridad: el mod manda sobre el log, y el log sobre la mera detección de
//! proceso. Cada campo se resuelve por separado, así que un mod sin el ajuste de
//! planeta activo sigue aportando investigación mientras el save viene del log.

use crate::model::{GameState, ModState};
use crate::sources::logfile::LogFacts;

pub fn merge(running: bool, log: &LogFacts, mod_state: Option<&ModState>) -> GameState {
    let mut state = GameState {
        running,
        ..Default::default()
    };

    if !running {
        // Sin proceso no hay nada que contar: los datos residuales de las otras
        // fuentes describirían una partida que ya no existe.
        return state;
    }

    // --- log ---
    state.save_name = log.save_name.clone();
    state.game_version = log.game_version.clone();
    state.multiplayer = log.multiplayer;

    // --- mod (gana donde solape) ---
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
            display: None,
            sampled_at: Some(1_700_000_000),
        }
    }

    #[test]
    fn sin_proceso_el_estado_queda_vacio() {
        let state = merge(false, &log_facts(), Some(&mod_state()));
        assert!(!state.running);
        assert!(state.save_name.is_none());
        assert!(state.surface.is_none());
    }

    #[test]
    fn modo_degradado_usa_solo_el_log() {
        let state = merge(true, &log_facts(), None);
        assert_eq!(state.save_name.as_deref(), Some("SI"));
        assert_eq!(state.game_version.as_deref(), Some("2.1.17"));
        assert!(!state.has_mod_data());
    }

    #[test]
    fn el_mod_aporta_lo_que_el_log_no_sabe() {
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(
            state.save_name.as_deref(),
            Some("SI"),
            "el save viene del log"
        );
        assert_eq!(
            state.surface.as_ref().unwrap().planet.as_deref(),
            Some("fulgora")
        );
        assert_eq!(state.research.as_ref().unwrap().done, 142);
        assert!(state.has_mod_data());
    }

    #[test]
    fn el_mod_gana_al_log_en_multijugador() {
        // El log dice single porque cargó un save local; el mod ve la verdad.
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(state.multiplayer, Some(true));
        assert_eq!(state.players_online, Some(4));
    }

    #[test]
    fn la_hora_de_escritura_del_mod_llega_al_estado() {
        let state = merge(true, &log_facts(), Some(&mod_state()));
        assert_eq!(state.sampled_at, Some(1_700_000_000));
    }

    #[test]
    fn sin_mod_ni_log_queda_solo_el_proceso() {
        let state = merge(true, &LogFacts::default(), None);
        assert!(state.running);
        assert!(state.save_name.is_none());
        assert!(!state.has_mod_data());
    }
}
