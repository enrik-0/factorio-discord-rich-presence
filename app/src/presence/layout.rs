//! Automatic distribution of fields into the card's slots.
//!
//! The player chooses *what* is shown from the mod's settings; the *where* is
//! fixed, so that no checkbox can be turned on without anything appearing.
//! Each slot gathers its active fields in an established priority order:
//!
//! - **line 1** game identity: save · planet · modpack
//! - **line 2** what you're doing: research
//! - **tooltip** counters: technologies · evolution · rockets · mods · mode …

use crate::config::Privacy;
use crate::model::{GameState, TimerMode};
use crate::presence::discord::unix_now;
use crate::presence::spec::{ActivitySpec, MAX_TEXT_LEN};

/// Separator between fields within the same slot.
pub const SEPARATOR: &str = " · ";

pub fn build(state: &GameState, privacy: &Privacy) -> ActivitySpec {
    let display = state.display();

    // --- line 1: identity ---
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

    // --- line 2: activity ---
    let mut line2 = Vec::new();
    if display.research {
        if let Some(research) = &state.research {
            match (research.label(), research.percent()) {
                (Some(label), Some(percent)) => {
                    line2.push(format!("Researching {} ({}%)", prettify(label), percent));
                }
                (Some(label), None) => line2.push(format!("Researching {}", prettify(label))),
                // With nothing queued the line would be empty; the technology
                // counter is the natural fallback.
                (None, _) if display.tech_count => {
                    line2.push(format!("{}/{} technologies", research.done, research.total));
                }
                _ => {}
            }
        }
    }
    // Always on, but hidden while nothing is being consumed. Pushed last so
    // it's the first thing dropped if the line overflows.
    if let Some(spm) = state
        .research
        .as_ref()
        .and_then(|r| r.spm)
        .filter(|spm| *spm >= 0.05)
    {
        line2.push(format!("{spm:.1} SPM"));
    }

    // --- tooltip: counters ---
    let mut tooltip = Vec::new();
    if display.tech_count {
        if let Some(research) = &state.research {
            tooltip.push(format!("{}/{} technologies", research.done, research.total));
        }
    }
    if display.evolution {
        if let Some(evolution) = state.evolution {
            tooltip.push(format!("Evolution {}%", (evolution * 100.0).round() as i64));
        }
    }
    if display.rockets {
        // Hidden until at least one has been launched: a "0 rockets" adds nothing.
        if let Some(rockets) = state.rockets_launched.filter(|count| *count > 0) {
            tooltip.push(match rockets {
                1 => "1 rocket".to_string(),
                n => format!("{n} rockets"),
            });
        }
    }
    // "Meme" stats: trees, enemies and deaths are hidden at zero, just like
    // the rockets — a "0 deaths" right at the start adds nothing.
    if display.trees {
        if let Some(count) = state.trees_razed.filter(|count| *count > 0) {
            tooltip.push(match count {
                1 => "1 tree razed".to_string(),
                n => format!("{} trees razed", format_count(n as f64)),
            });
        }
    }
    if display.enemies {
        if let Some(count) = state.enemies_killed.filter(|count| *count > 0) {
            tooltip.push(match count {
                1 => "1 enemy killed".to_string(),
                n => format!("{} enemies killed", format_count(n as f64)),
            });
        }
    }
    if display.deaths {
        if let Some(count) = state.player_deaths.filter(|count| *count > 0) {
            tooltip.push(match count {
                1 => "1 death".to_string(),
                n => format!("{} deaths", format_count(n as f64)),
            });
        }
    }
    // Pollution and AFK are shown even at zero: unlike the ones above, a low
    // value here is no less interesting than a high one.
    if display.pollution {
        if let Some(pollution) = state.pollution_emitted {
            tooltip.push(format!("Pollution {}", format_count(pollution)));
        }
    }
    if display.afk {
        if let Some(secs) = state.afk_secs() {
            tooltip.push(format!("AFK {}", format_afk(secs)));
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
    // The server address has a double lock: the mod's setting and the
    // application's veto. It's the only field that exposes anything outside the game.
    if display.server && privacy.share_server_address {
        if let Some(address) = &state.server_address {
            tooltip.push(address.clone());
        }
    }

    ActivitySpec {
        details: join_fitting(line1),
        state: join_fitting(line2),
        large_image: None, // filled in by the caller from the configuration
        large_text: join_fitting(tooltip),
        small_image: None,
        small_text: None,
        start_timestamp: timer_start(state, display.timer),
        party: party_size(state),
    }
}

/// Unix instant at which the timer started.
///
/// The elapsed time was measured by the mod when it wrote, so it is subtracted
/// from *that* instant and not the current one. With `unix_now()` the result
/// kept advancing with every poll while the mod hadn't rewritten, and Discord's
/// timer restarted every time the drift exceeded the deduplicator's tolerance.
fn timer_start(state: &GameState, mode: TimerMode) -> Option<i64> {
    let elapsed = match mode {
        TimerMode::Save => state.playtime_secs(),
        TimerMode::Session => state.session_secs(),
        TimerMode::None => return None,
    }?;
    Some(state.sampled_at.unwrap_or_else(unix_now) - elapsed)
}

fn party_size(state: &GameState) -> Option<(i32, i32)> {
    if state.multiplayer != Some(true) {
        return None;
    }
    let online = state.players_online? as i32;
    // Factorio doesn't impose a player maximum: it's declared equal to the connected count.
    Some((online, online))
}

fn planet_label(state: &GameState) -> Option<String> {
    let surface = state.surface.as_ref()?;
    Some(match surface.kind.as_str() {
        "platform" => "Space platform".to_string(),
        _ => prettify(&surface.name),
    })
}

fn mode_label(state: &GameState) -> String {
    match (state.multiplayer, state.players_online) {
        (Some(true), Some(players)) => format!("Multiplayer ({players})"),
        (Some(true), None) => "Multiplayer".to_string(),
        _ => "Singleplayer".to_string(),
    }
}

/// Formats a potentially large number compactly: `950`, `1.2k`,
/// `3.4M`. Pollution and, in long games, razed trees
/// can reach into the hundreds of thousands.
fn format_count(n: f64) -> String {
    let n = n.round();
    if n.abs() < 1000.0 {
        format!("{n:.0}")
    } else if n.abs() < 1_000_000.0 {
        format!("{:.1}k", n / 1000.0)
    } else {
        format!("{:.1}M", n / 1_000_000.0)
    }
}

/// `125` → `2 min`, `4200` → `1h 10min`. Only hours and minutes: seconds add
/// nothing for "how long has it been since you touched anything".
fn format_afk(total_secs: i64) -> String {
    let minutes = total_secs / 60;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{}h {:02}min", minutes / 60, minutes % 60)
    }
}

/// Joins the fields and, if they don't fit, drops the lowest-priority ones.
///
/// Cutting off mid-word is worse than showing one field fewer, and since the
/// order is by priority, what gets dropped is always the least important.
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
/// Only acts on raw prototype names: when the mod's translation arrives,
/// the text already comes well-formed and has no hyphens to touch.
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

    /// Replica of the real state of the test save: Krastorio2, 240 mods,
    /// Nauvis, researching oil extraction.
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
                spm: None,
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
    fn default_layout_over_real_data() {
        let spec = build(&real_state(), &Privacy::default());
        assert_eq!(spec.details.as_deref(), Some("claro · Nauvis"));
        assert_eq!(
            spec.state.as_deref(),
            Some("Researching Extracción de petróleo (88%)")
        );
        // No rockets (they're 0), no evolution, no mods, which are off by default.
        assert_eq!(
            spec.large_text.as_deref(),
            Some("41/1510 technologies · Singleplayer")
        );
    }

    #[test]
    fn enabling_fields_makes_them_appear_in_their_slot() {
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
            Some("41/1510 technologies · Evolution 12% · 240 mods · Singleplayer")
        );
    }

    #[test]
    fn disabling_fields_removes_them_without_leaving_separators() {
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
            "the tooltip is left empty, not full of dots"
        );
    }

    #[test]
    fn rockets_are_hidden_while_zero() {
        let mut state = with_display(Display::default());
        assert!(!build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("rocket"));

        state.rockets_launched = Some(1);
        assert!(build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("1 rocket"));

        state.rockets_launched = Some(12);
        assert!(build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("12 rockets"));
    }

    #[test]
    fn trees_enemies_and_deaths_are_hidden_while_zero() {
        let display = Display {
            trees: true,
            enemies: true,
            deaths: true,
            ..Display::default()
        };
        let mut state = with_display(display);
        state.trees_razed = Some(0);
        state.enemies_killed = Some(0);
        state.player_deaths = Some(0);
        let empty = build(&state, &Privacy::default()).large_text.unwrap();
        assert!(!empty.contains("tree"));
        assert!(!empty.contains("enemy") && !empty.contains("enemies"));
        assert!(!empty.contains("death"));

        state.trees_razed = Some(1);
        state.enemies_killed = Some(1);
        state.player_deaths = Some(1);
        let singular = build(&state, &Privacy::default()).large_text.unwrap();
        assert!(singular.contains("1 tree razed"));
        assert!(singular.contains("1 enemy killed"));
        assert!(singular.contains("1 death") && !singular.contains("1 deaths"));

        state.trees_razed = Some(2_500);
        state.enemies_killed = Some(58);
        state.player_deaths = Some(3);
        let plural = build(&state, &Privacy::default()).large_text.unwrap();
        assert!(plural.contains("2.5k trees razed"));
        assert!(plural.contains("58 enemies killed"));
        assert!(plural.contains("3 deaths"));
    }

    #[test]
    fn pollution_and_afk_show_even_when_zero() {
        let display = Display {
            pollution: true,
            afk: true,
            ..Display::default()
        };
        let mut state = with_display(display);
        state.pollution_emitted = Some(0.0);
        state.afk_ticks = Some(0);

        let spec = build(&state, &Privacy::default());
        let text = spec.large_text.unwrap();
        assert!(text.contains("Pollution 0"));
        assert!(text.contains("AFK 0 min"));
    }

    #[test]
    fn large_pollution_is_formatted_compactly() {
        let mut state = with_display(Display {
            pollution: true,
            ..Display::default()
        });
        state.pollution_emitted = Some(1_234_567.0);
        let text = build(&state, &Privacy::default()).large_text.unwrap();
        assert!(text.contains("Pollution 1.2M"), "{text}");
    }

    #[test]
    fn afk_rolls_over_to_hours_and_minutes() {
        let mut state = with_display(Display {
            afk: true,
            ..Display::default()
        });
        state.afk_ticks = Some(70 * 60 * 60); // 70 minutes in ticks (60 t/s)
        let text = build(&state, &Privacy::default()).large_text.unwrap();
        assert!(text.contains("AFK 1h 10min"), "{text}");
    }

    #[test]
    fn the_timer_respects_the_chosen_mode() {
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
    fn the_timer_start_does_not_depend_on_when_it_is_polled() {
        // Regression: the start was calculated as `now - elapsed`, so every
        // poll gave a different value and Discord kept restarting the timer.
        let mut state = with_display(Display {
            timer: TimerMode::Session,
            ..Display::default()
        });
        state.sampled_at = Some(1_700_000_000);

        let first = build(&state, &Privacy::default()).start_timestamp;
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        let second = build(&state, &Privacy::default()).start_timestamp;

        // 36000 ticks / 60 = 600 s of session.
        assert_eq!(first, Some(1_700_000_000 - 600));
        assert_eq!(second, first, "same data, same start");
    }

    #[test]
    fn the_app_can_veto_the_server_even_if_the_mod_allows_it() {
        let state = GameState {
            server_address: Some("203.0.113.7:34197".into()),
            ..with_display(Display {
                server: true,
                ..Display::default()
            })
        };

        let allowed = Privacy {
            share_server_address: true,
            share_save_name: true,
        };
        assert!(build(&state, &allowed)
            .large_text
            .unwrap()
            .contains("203.0.113.7"));

        // By default the application vetoes it even if the mod's setting says yes.
        assert!(!build(&state, &Privacy::default())
            .large_text
            .unwrap()
            .contains("203.0.113.7"));
    }

    #[test]
    fn with_no_research_queued_line_2_falls_back_to_the_counter() {
        let mut state = with_display(Display::default());
        state.research = Some(Research {
            current: None,
            current_label: None,
            progress: None,
            done: 41,
            total: 1510,
            spm: None,
        });
        assert_eq!(
            build(&state, &Privacy::default()).state.as_deref(),
            Some("41/1510 technologies")
        );
    }

    #[test]
    fn spm_follows_the_research_on_line_2() {
        let mut state = with_display(Display::default());
        state.research.as_mut().unwrap().spm = Some(123.45);
        assert_eq!(
            build(&state, &Privacy::default()).state.as_deref(),
            Some("Researching Extracción de petróleo (88%) · 123.5 SPM")
        );

        // Shown even with the research field turned off: it has no setting.
        state.display = Some(Display {
            research: false,
            ..Display::default()
        });
        assert_eq!(
            build(&state, &Privacy::default()).state.as_deref(),
            Some("123.5 SPM")
        );
    }

    #[test]
    fn spm_is_hidden_while_zero() {
        let mut state = with_display(Display::default());
        state.research.as_mut().unwrap().spm = Some(0.0);
        assert!(!build(&state, &Privacy::default())
            .state
            .unwrap()
            .contains("SPM"));
    }

    #[test]
    fn degraded_mode_uses_the_factory_preferences() {
        // Without the mod no `display` block arrives: only save and mode are present.
        let state = GameState {
            running: true,
            save_name: Some("claro".into()),
            multiplayer: Some(false),
            ..Default::default()
        };
        let spec = build(&state, &Privacy::default());
        assert_eq!(spec.details.as_deref(), Some("claro"));
        assert_eq!(spec.large_text.as_deref(), Some("Singleplayer"));
        assert_eq!(
            spec.start_timestamp, None,
            "without the mod there's no playtime"
        );
    }

    #[test]
    fn multiplayer_shows_how_many_are_online() {
        let mut state = with_display(Display::default());
        state.multiplayer = Some(true);
        state.players_online = Some(4);
        let spec = build(&state, &Privacy::default());
        assert!(spec.large_text.unwrap().contains("Multiplayer (4)"));
        assert_eq!(spec.party, Some((4, 4)));
    }

    #[test]
    fn a_slot_that_is_too_long_drops_the_least_important_part() {
        let long = "x".repeat(100);
        let result = join_fitting(vec![long.clone(), long.clone(), "tail".into()]).unwrap();
        assert_eq!(result, long, "only the first one fits, and uncut");
        assert!(result.chars().count() <= MAX_TEXT_LEN);
    }

    #[test]
    fn join_fitting_ignores_empty_parts() {
        assert_eq!(
            join_fitting(vec!["a".into(), "  ".into(), "b".into()]).as_deref(),
            Some("a · b")
        );
        assert_eq!(join_fitting(vec![]), None);
    }
}
