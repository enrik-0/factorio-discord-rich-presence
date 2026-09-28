//! Building the Discord card.
//!
//! There are two paths:
//!
//! - **automatic** (default): the player chooses the fields from the mod's
//!   settings, in-game, and [`super::layout`] distributes them into fixed slots.
//! - **templates** (advanced): if `config.toml` defines a `[templates]`
//!   section, it takes over and the mod's checkboxes are ignored.
//!
//! The two can't be in charge at the same time: if the checkboxes chose the
//! fields but the template decided the placement, turning on a checkbox the
//! template doesn't mention would do absolutely nothing.

use std::collections::HashMap;

use crate::config::{Config, Privacy};
use crate::model::GameState;
use crate::presence::layout::{self, prettify};
use crate::presence::spec::{truncate, ActivitySpec};

pub fn render(state: &GameState, config: &Config) -> Option<ActivitySpec> {
    if !state.running {
        return None;
    }

    // The automatic layout also resolves the timer and party, which don't depend
    // on the chosen mode.
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
// Template mode
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
            "platform" => "Space platform".to_string(),
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
    // Counters at zero are omitted, so their block disappears on its own.
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
            Some(true) => "Multiplayer".to_string(),
            _ => "Singleplayer".to_string(),
        },
    );

    vars
}

/// Substitutes `{variable}` and removes blocks whose variables are missing.
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

/// Returns `None` if the block references a variable that has no value.
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
    fn substitutes_variables() {
        assert_eq!(
            render_template("Researching {research} ({research_pct}%)", &vars()).as_deref(),
            Some("Researching Planta electromagnética (64%)")
        );
    }

    #[test]
    fn discards_the_block_with_a_missing_variable() {
        let mut partial = vars();
        partial.remove("planet");
        assert_eq!(
            render_template("{planet} · {save}", &partial).as_deref(),
            Some("Cohetes S.A."),
            "no dangling separator should be left behind"
        );
    }

    #[test]
    fn with_no_variable_at_all_it_returns_none() {
        assert_eq!(render_template("{planet} · {save}", &HashMap::new()), None);
    }

    #[test]
    fn literal_text_survives_with_no_variables() {
        assert_eq!(
            render_template("In Factorio", &HashMap::new()).as_deref(),
            Some("In Factorio")
        );
    }

    #[test]
    fn nothing_renders_without_a_running_process() {
        assert!(render(&GameState::default(), &Config::default()).is_none());
    }

    #[test]
    fn with_no_templates_the_automatic_layout_takes_over() {
        let spec = render(&state(), &Config::default()).unwrap();
        assert_eq!(spec.details.as_deref(), Some("claro · Nauvis"));
        assert_eq!(spec.large_image.as_deref(), Some("factorio"));
    }

    #[test]
    fn with_templates_they_take_over() {
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
    fn an_empty_asset_disables_the_image() {
        let config = Config {
            large_image: "   ".into(),
            ..Config::default()
        };
        assert_eq!(render(&state(), &config).unwrap().large_image, None);
    }
}
