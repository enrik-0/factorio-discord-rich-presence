//! Reparto automático de los campos en los huecos de la tarjeta.
//!
//! El jugador elige *qué* ve desde los ajustes del mod; el *dónde* es fijo, para
//! que ninguna casilla pueda activarse sin que aparezca nada. Cada hueco junta
//! sus campos activos en un orden de prioridad establecido:
//!
//! - **línea 1** identidad de la partida: save · planeta · modpack
//! - **línea 2** qué estás haciendo: investigación
//! - **tooltip** contadores: tecnologías · evolución · cohetes · mods · modo …

use crate::config::Privacy;
use crate::model::{GameState, TimerMode};
use crate::presence::discord::unix_now;
use crate::presence::spec::{ActivitySpec, MAX_TEXT_LEN};

/// Separador entre campos dentro de un mismo hueco.
pub const SEPARATOR: &str = " · ";

pub fn build(state: &GameState, privacy: &Privacy) -> ActivitySpec {
    let display = state.display();

    // --- línea 1: identidad ---
    let mut line1 = Vec::new();
    if display.save && privacy.share_save_name {
        if let Some(save) = &state.save_name {
            line1.push(save.clone());
        }
    }
    if display.planet {
        if let Some(planet) = planet_label(state) {
            line1.push(planet);
        }
    }
    if display.overhaul {
        if let Some(overhaul) = &state.overhaul {
            line1.push(prettify(overhaul));
        }
    }

    // --- línea 2: actividad ---
    let mut line2 = Vec::new();
    if display.research {
        if let Some(research) = &state.research {
            match (research.label(), research.percent()) {
                (Some(label), Some(percent)) => {
                    line2.push(format!("Investigando {} ({}%)", prettify(label), percent));
                }
                (Some(label), None) => line2.push(format!("Investigando {}", prettify(label))),
                // Sin nada en cola la línea quedaría vacía; el contador de
                // tecnologías es el respaldo natural.
                (None, _) if display.tech_count => {
                    line2.push(format!("{}/{} tecnologías", research.done, research.total));
                }
                _ => {}
            }
        }
    }

    // --- tooltip: contadores ---
    let mut tooltip = Vec::new();
    if display.tech_count {
        if let Some(research) = &state.research {
            tooltip.push(format!("{}/{} tecnologías", research.done, research.total));
        }
    }
    if display.evolution {
        if let Some(evolution) = state.evolution {
            tooltip.push(format!("Evolución {}%", (evolution * 100.0).round() as i64));
        }
    }
    if display.rockets {
        // Se oculta mientras no haya lanzado ninguno: un "0 cohetes" no aporta.
        if let Some(rockets) = state.rockets_launched.filter(|count| *count > 0) {
            tooltip.push(match rockets {
                1 => "1 cohete".to_string(),
                n => format!("{n} cohetes"),
            });
        }
    }
    if display.mod_count {
        if let Some(count) = state.mod_count.filter(|count| *count > 0) {
            tooltip.push(format!("{count} mods"));
        }
    }
    if display.mode {
        tooltip.push(mode_label(state));
    }
    if display.player_name {
        if let Some(name) = &state.player_name {
            tooltip.push(name.clone());
        }
    }
    // La dirección del servidor lleva doble llave: el ajuste del mod y el veto
    // de la aplicación. Es el único campo que expone algo fuera de la partida.
    if display.server && privacy.share_server_address {
        if let Some(address) = &state.server_address {
            tooltip.push(address.clone());
        }
    }

    ActivitySpec {
        details: join_fitting(line1),
        state: join_fitting(line2),
        large_image: None, // lo rellena el llamante desde la configuración
        large_text: join_fitting(tooltip),
        small_image: None,
        small_text: None,
        start_timestamp: timer_start(state, display.timer),
        party: party_size(state),
    }
}

fn timer_start(state: &GameState, mode: TimerMode) -> Option<i64> {
    let elapsed = match mode {
        TimerMode::Save => state.playtime_secs(),
        TimerMode::Session => state.session_secs(),
        TimerMode::None => return None,
    }?;
    Some(unix_now() - elapsed)
}

fn party_size(state: &GameState) -> Option<(i32, i32)> {
    if state.multiplayer != Some(true) {
        return None;
    }
    let online = state.players_online? as i32;
    // Factorio no impone un máximo de jugadores: se declara igual al conectado.
    Some((online, online))
}

fn planet_label(state: &GameState) -> Option<String> {
    let surface = state.surface.as_ref()?;
    Some(match surface.kind.as_str() {
        "platform" => "Plataforma espacial".to_string(),
        _ => prettify(&surface.name),
    })
}

fn mode_label(state: &GameState) -> String {
    match (state.multiplayer, state.players_online) {
        (Some(true), Some(players)) => format!("Multijugador ({players})"),
        (Some(true), None) => "Multijugador".to_string(),
        _ => "Un jugador".to_string(),
    }
}

/// Une los campos y, si no caben, va soltando los de menor prioridad.
///
/// Recortar a mitad de palabra es peor que mostrar un campo menos, y como el
/// orden es de prioridad, lo que se cae es siempre lo menos importante.
pub fn join_fitting(parts: Vec<String>) -> Option<String> {
    let mut parts: Vec<String> = parts.into_iter().filter(|p| !p.trim().is_empty()).collect();

    while !parts.is_empty() {
        let joined = parts.join(SEPARATOR);
        if joined.chars().count() <= MAX_TEXT_LEN {
            return Some(joined);
        }
        parts.pop();
    }
    None
}

/// `electromagnetic-plant` → `Electromagnetic plant`.
///
/// Sólo actúa sobre nombres crudos de prototipo: cuando llega la traducción del
/// mod, el texto ya viene bien escrito y no contiene guiones que tocar.
pub fn prettify(raw: &str) -> String {
    if !raw.contains('-') && raw.chars().next().is_some_and(|c| c.is_uppercase()) {
        return raw.to_string();
    }
    let spaced = raw.replace(['-', '_'], " ");
    let mut chars = spaced.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => spaced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Display, Research, Surface};

    /// Réplica del estado real de la partida de pruebas: Krastorio2, 240 mods,
    /// Nauvis, investigando extracción de petróleo.
    fn real_state() -> GameState {
        GameState {
            running: true,
            save_name: Some("claro".into()),
            game_version: Some("2.1.17".into()),
            multiplayer: Some(false),
            players_online: Some(1),
            player_name: Some("enrik0".into()),
            controller: Some("character".into()),
            ticks_played: Some(1_894_800),
            session_ticks: Some(36_000),
            rockets_launched: Some(0),
            mod_count: Some(240),
            overhaul: Some("Krastorio2".into()),
            evolution: Some(0.1234),
            surface: Some(Surface {
                name: "nauvis".into(),
                kind: "planet".into(),
                planet: Some("nauvis".into()),
            }),
            research: Some(Research {
                current: Some("oil-gathering".into()),
                current_label: Some("Extracción de petróleo".into()),
                progress: Some(0.88),
                done: 41,
                total: 1510,
            }),
            ..Default::default()
        }
    }

    fn with_display(display: Display) -> GameState {
        GameState {
            display: Some(display),
            ..real_state()
        }
    }

    #[test]
    fn reparto_por_defecto_sobre_datos_reales() {
        let spec = build(&real_state(), &Privacy::default());
        assert_eq!(spec.details.as_deref(), Some("claro · Nauvis"));
        assert_eq!(
            spec.state.as_deref(),
            Some("Investigando Extracción de petróleo (88%)")
        );
        // Sin cohetes (son 0) ni evolución ni mods, que van apagados de fábrica.
        assert_eq!(
            spec.large_text.as_deref(),
            Some("41/1510 tecnologías · Un jugador")
        );
    }

    #[test]
    fn activar_campos_los_hace_aparecer_en_su_hueco() {
        let spec = build(
            &with_display(Display {
                overhaul: true,
                evolution: true,
                mod_count: true,
                ..Display::default()
            }),
            &Privacy::default(),
        );
        assert_eq!(spec.details.as_deref(), Some("claro · Nauvis · Krastorio2"));
        assert_eq!(
            spec.large_text.as_deref(),
            Some("41/1510 tecnologías · Evolución 12% · 240 mods · Un jugador")
        );
    }

    #[test]
    fn desactivar_campos_los_quita_sin_dejar_separadores() {
        let spec = build(
            &with_display(Display {
                save: false,
                tech_count: false,
                mode: false,
                ..Display::default()
            }),
            &Privacy::default(),
        );
        assert_eq!(spec.details.as_deref(), Some("Nauvis"));
        assert_eq!(
            spec.large_text, None,
            "el tooltip queda vacío, no con puntos"
        );
    }

    #[test]
    fn los_cohetes_se_ocultan_mientras_sean_cero() {
        let mut state = with_display(Display::default());
        assert!(!build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("cohete"));

        state.rockets_launched = Some(1);
        assert!(build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("1 cohete"));

        state.rockets_launched = Some(12);
        assert!(build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("12 cohetes"));
    }

    #[test]
    fn el_cronometro_respeta_el_modo_elegido() {
        let now = unix_now();

        let save = build(&with_display(Display::default()), &Privacy::default());
        // 1894800 ticks / 60 = 31580 s
        assert!((now - save.start_timestamp.unwrap() - 31_580).abs() <= 1);

        let session = build(
            &with_display(Display {
                timer: TimerMode::Session,
                ..Display::default()
            }),
            &Privacy::default(),
        );
        assert!((now - session.start_timestamp.unwrap() - 600).abs() <= 1);

        let none = build(
            &with_display(Display {
                timer: TimerMode::None,
                ..Display::default()
            }),
            &Privacy::default(),
        );
        assert_eq!(none.start_timestamp, None);
    }

    #[test]
    fn la_aplicacion_puede_vetar_el_servidor_aunque_el_mod_lo_permita() {
        let state = GameState {
            server_address: Some("203.0.113.7:34197".into()),
            ..with_display(Display {
                server: true,
                ..Display::default()
            })
        };

        let permitido = Privacy {
            share_server_address: true,
            share_save_name: true,
        };
        assert!(build(&state, &permitido)
            .large_text
            .unwrap()
            .contains("203.0.113.7"));

        // Por defecto la aplicación lo veta aunque el ajuste del mod diga que sí.
        assert!(!build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("203.0.113.7"));
    }

    #[test]
    fn sin_investigacion_en_cola_la_linea_2_cae_al_contador() {
        let mut state = with_display(Display::default());
        state.research = Some(Research {
            current: None,
            current_label: None,
            progress: None,
            done: 41,
            total: 1510,
        });
        assert_eq!(
            build(&state, &Privacy::default()).state.as_deref(),
            Some("41/1510 tecnologías")
        );
    }

    #[test]
    fn modo_degradado_usa_las_preferencias_de_fabrica() {
        // Sin el mod no llega bloque `display`: sólo hay save y modo.
        let state = GameState {
            running: true,
            save_name: Some("claro".into()),
            multiplayer: Some(false),
            ..Default::default()
        };
        let spec = build(&state, &Privacy::default());
        assert_eq!(spec.details.as_deref(), Some("claro"));
        assert_eq!(spec.large_text.as_deref(), Some("Un jugador"));
        assert_eq!(
            spec.start_timestamp, None,
            "sin el mod no hay tiempo jugado"
        );
    }

    #[test]
    fn multijugador_muestra_cuantos_hay() {
        let mut state = with_display(Display::default());
        state.multiplayer = Some(true);
        state.players_online = Some(4);
        let spec = build(&state, &Privacy::default());
        assert!(spec.large_text.unwrap().contains("Multijugador (4)"));
        assert_eq!(spec.party, Some((4, 4)));
    }

    #[test]
    fn un_hueco_demasiado_largo_suelta_lo_menos_importante() {
        let largo = "x".repeat(100);
        let resultado = join_fitting(vec![largo.clone(), largo.clone(), "cola".into()]).unwrap();
        assert_eq!(resultado, largo, "sólo cabe el primero, y sin cortarlo");
        assert!(resultado.chars().count() <= MAX_TEXT_LEN);
    }

    #[test]
    fn join_fitting_ignora_vacios() {
        assert_eq!(
            join_fitting(vec!["a".into(), "  ".into(), "b".into()]).as_deref(),
            Some("a · b")
        );
        assert_eq!(join_fitting(vec![]), None);
    }
}
