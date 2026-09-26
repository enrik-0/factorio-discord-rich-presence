//! Construcción de la tarjeta de Discord.
//!
//! Hay dos caminos:
//!
//! - **automático** (por defecto): el jugador elige los campos desde los ajustes
//!   del mod, dentro del juego, y [`super::layout`] los reparte en huecos fijos.
//! - **plantillas** (avanzado): si `config.toml` define una sección
//!   `[templates]`, manda ella y se ignoran las casillas del mod.
//!
//! Los dos no pueden ser autoridad a la vez: si las casillas eligieran los
//! campos pero la plantilla decidiera el sitio, activar una casilla que la
//! plantilla no menciona no haría absolutamente nada.

use std::collections::HashMap;

use crate::config::{Config, Privacy};
use crate::model::GameState;
use crate::presence::layout::{self, prettify};
use crate::presence::spec::{truncate, ActivitySpec};

pub fn render(state: &GameState, config: &Config) -> Option<ActivitySpec> {
    if !state.running {
        return None;
    }

    // El reparto automático resuelve además cronómetro y party, que no dependen
    // del modo elegido.
    let base = layout::build(state, &config.privacy);

    let mut spec = match &config.templates {
        Some(templates) => ActivitySpec {
            details: from_template(&templates.details, state, &config.privacy),
            state: from_template(&templates.state, state, &config.privacy)
                .or_else(|| from_template(&templates.fallback.state, state, &config.privacy)),
            large_text: from_template(&templates.large_text, state, &config.privacy),
            ..base
        },
        None => base,
    };

    let image = config.large_image.trim();
    spec.large_image = (!image.is_empty()).then(|| image.to_string());

    Some(spec)
}

//------------------------------------------------------------------------------
// Modo plantillas
//------------------------------------------------------------------------------

fn from_template(template: &str, state: &GameState, privacy: &Privacy) -> Option<String> {
    render_template(template, &build_vars(state, privacy))
}

fn build_vars(state: &GameState, privacy: &Privacy) -> HashMap<&'static str, String> {
    let mut vars = HashMap::new();

    if privacy.share_save_name {
        if let Some(save) = &state.save_name {
            vars.insert("save", save.clone());
        }
    }
    if privacy.share_server_address {
        if let Some(address) = &state.server_address {
            vars.insert("server", address.clone());
        }
    }

    if let Some(surface) = &state.surface {
        vars.insert("surface", surface.name.clone());
        let pretty = match surface.kind.as_str() {
            "platform" => "Plataforma espacial".to_string(),
            _ => prettify(&surface.name),
        };
        vars.insert("planet", pretty);
    }

    if let Some(research) = &state.research {
        if let Some(label) = research.label() {
            vars.insert("research", prettify(label));
        }
        if let Some(percent) = research.percent() {
            vars.insert("research_pct", percent.to_string());
        }
        vars.insert("tech_done", research.done.to_string());
        vars.insert("tech_total", research.total.to_string());
    }

    if let Some(evolution) = state.evolution {
        vars.insert(
            "evolution",
            ((evolution * 100.0).round() as i64).to_string(),
        );
    }
    // Los contadores a cero se omiten, para que su bloque desaparezca solo.
    if let Some(rockets) = state.rockets_launched.filter(|count| *count > 0) {
        vars.insert("rockets", rockets.to_string());
    }
    if let Some(count) = state.mod_count.filter(|count| *count > 0) {
        vars.insert("mod_count", count.to_string());
    }
    if let Some(online) = state.players_online {
        vars.insert("players_online", online.to_string());
    }
    if let Some(overhaul) = &state.overhaul {
        vars.insert("overhaul", prettify(overhaul));
    }
    if let Some(version) = &state.game_version {
        vars.insert("version", version.clone());
    }
    if let Some(name) = &state.player_name {
        vars.insert("player", name.clone());
    }

    vars.insert(
        "mode",
        match state.multiplayer {
            Some(true) => "Multijugador".to_string(),
            _ => "Un jugador".to_string(),
        },
    );

    vars
}

/// Sustituye `{variable}` y elimina los bloques cuyas variables falten.
fn render_template(template: &str, vars: &HashMap<&'static str, String>) -> Option<String> {
    let mut blocks = Vec::new();

    for block in template.split(layout::SEPARATOR) {
        if let Some(rendered) = render_block(block, vars) {
            if !rendered.trim().is_empty() {
                blocks.push(rendered);
            }
        }
    }

    if blocks.is_empty() {
        return None;
    }
    Some(truncate(&blocks.join(layout::SEPARATOR)))
}

/// Devuelve `None` si el bloque referencia una variable que no tiene valor.
fn render_block(block: &str, vars: &HashMap<&'static str, String>) -> Option<String> {
    let mut output = String::with_capacity(block.len());
    let mut rest = block;

    while let Some(open) = rest.find('{') {
        let Some(close_offset) = rest[open..].find('}') else {
            break;
        };
        let close = open + close_offset;

        output.push_str(&rest[..open]);
        let name = &rest[open + 1..close];
        output.push_str(vars.get(name)?);
        rest = &rest[close + 1..];
    }

    output.push_str(rest);
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Fallback, Templates};
    use crate::model::{Research, Surface};

    fn vars() -> HashMap<&'static str, String> {
        HashMap::from([
            ("planet", "Fulgora".to_string()),
            ("save", "Cohetes S.A.".to_string()),
            ("research", "Planta electromagnética".to_string()),
            ("research_pct", "64".to_string()),
        ])
    }

    fn state() -> GameState {
        GameState {
            running: true,
            save_name: Some("claro".into()),
            multiplayer: Some(false),
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

    #[test]
    fn sustituye_variables() {
        assert_eq!(
            render_template("Investigando {research} ({research_pct}%)", &vars()).as_deref(),
            Some("Investigando Planta electromagnética (64%)")
        );
    }

    #[test]
    fn descarta_el_bloque_con_variable_ausente() {
        let mut partial = vars();
        partial.remove("planet");
        assert_eq!(
            render_template("{planet} · {save}", &partial).as_deref(),
            Some("Cohetes S.A."),
            "no debe quedar un separador colgando"
        );
    }

    #[test]
    fn sin_ninguna_variable_devuelve_none() {
        assert_eq!(render_template("{planet} · {save}", &HashMap::new()), None);
    }

    #[test]
    fn el_texto_literal_sobrevive_sin_variables() {
        assert_eq!(
            render_template("En Factorio", &HashMap::new()).as_deref(),
            Some("En Factorio")
        );
    }

    #[test]
    fn sin_proceso_no_se_renderiza_nada() {
        assert!(render(&GameState::default(), &Config::default()).is_none());
    }

    #[test]
    fn sin_plantillas_manda_el_reparto_automatico() {
        let spec = render(&state(), &Config::default()).unwrap();
        assert_eq!(spec.details.as_deref(), Some("claro · Nauvis"));
        assert_eq!(spec.large_image.as_deref(), Some("factorio"));
    }

    #[test]
    fn con_plantillas_mandan_ellas() {
        let config = Config {
            templates: Some(Templates {
                details: "{save} en {planet}".into(),
                state: "{tech_done} de {tech_total}".into(),
                large_text: "Factorio".into(),
                fallback: Fallback::default(),
            }),
            ..Config::default()
        };
        let spec = render(&state(), &config).unwrap();
        assert_eq!(spec.details.as_deref(), Some("claro en Nauvis"));
        assert_eq!(spec.state.as_deref(), Some("41 de 1510"));
    }

    #[test]
    fn un_asset_vacio_desactiva_la_imagen() {
        let config = Config {
            large_image: "   ".into(),
            ..Config::default()
        };
        assert_eq!(render(&state(), &config).unwrap().large_image, None);
    }
}
