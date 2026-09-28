//! Main loop: polls the sources, merges, renders, and publishes.

use std::time::{Duration, Instant};

use anyhow::Result;
use tracing::{debug, info};

use crate::config::Config;
use crate::lifetime::GameLifetime;
use crate::merge::merge;
use crate::presence::render::render;
use crate::presence::DiscordSink;
use crate::sources::{logfile, LogWatcher, ModFileWatcher, ProcessWatcher};
use crate::status::Shared;

/// Loop cadence. Discord only accepts one update every 15 s, so polling every
/// 2 s is more than enough and keeps resource use negligible.
const TICK: Duration = Duration::from_secs(2);

/// The chunk size the wait is split into, so closing from the tray isn't
/// noticed a whole cycle late.
const SHUTDOWN_POLL: Duration = Duration::from_millis(200);

/// With `exit_with_game` the application requests its own shutdown when
/// Factorio, once seen, stops running (launcher mode).
pub fn run(config: &Config, shared: &Shared, exit_with_game: bool) -> Result<()> {
    let application_id = config.application_id()?;
    let data_dir = config.factorio_data_dir()?;
    let script_output = data_dir.join("script-output");

    info!(data = %data_dir.display(), "watching Factorio");

    let mut sink = DiscordSink::new(application_id)?;
    let mut process = ProcessWatcher::new();
    let mut log = LogWatcher::new(logfile::default_log_path(&data_dir));
    let mut modfile = ModFileWatcher::new(&script_output);

    debug!(file = %modfile.path().display(), "mod state file");

    let mut was_running = false;
    let mut lifetime = exit_with_game.then(|| GameLifetime::new(Instant::now()));

    while !shared.is_shutdown() {
        process.poll();
        let running = process.is_running();

        if running {
            // Disk is only touched while the game is alive.
            log.poll();
            modfile.poll();
        }

        if running != was_running {
            if running {
                info!("Factorio running");
            } else {
                info!("Factorio closed; clearing Discord state");
                sink.clear();
                shared.update(|status| status.headline = None);
            }
            was_running = running;
        }

        if let Some(lifetime) = lifetime.as_mut() {
            if lifetime.should_exit(running, Instant::now()) {
                info!("Factorio is no longer running; closing the application");
                shared.request_shutdown();
            }
        }

        if running {
            let state = merge(running, log.facts(), modfile.state());
            if let Some(spec) = render(&state, config) {
                if sink.publish(&spec) {
                    let headline = [spec.details.as_deref(), spec.state.as_deref()]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" — ");
                    shared.update(|status| {
                        status.mod_data = state.has_mod_data();
                        status.headline = (!headline.is_empty()).then_some(headline);
                    });
                    info!(
                        details = spec.details.as_deref().unwrap_or("-"),
                        state = spec.state.as_deref().unwrap_or("-"),
                        mode = if state.has_mod_data() {
                            "complete"
                        } else {
                            "degraded (no mod)"
                        },
                        "state updated"
                    );
                }
            }
        }

        shared.update(|status| {
            status.factorio_running = running;
            status.discord_connected = sink.is_connected();
        });

        sleep_until_shutdown(shared, TICK);
    }

    info!("closing");
    sink.clear();
    Ok(())
}

/// Sleeps the cycle, but checking now and then whether it's time to close.
fn sleep_until_shutdown(shared: &Shared, total: Duration) {
    let mut left = total;
    while left > Duration::ZERO && !shared.is_shutdown() {
        let step = left.min(SHUTDOWN_POLL);
        std::thread::sleep(step);
        left -= step;
    }
}

/// Diagnostics: prints what the sources currently see, without touching Discord.
pub fn dump(config: &Config) -> Result<()> {
    let data_dir = config.factorio_data_dir()?;
    let script_output = data_dir.join("script-output");

    let mut process = ProcessWatcher::new();
    let mut log = LogWatcher::new(logfile::default_log_path(&data_dir));
    let mut modfile = ModFileWatcher::new(&script_output);

    process.poll();
    log.poll();
    modfile.poll();

    // The raw facts from each source are shown alongside the merged state:
    // with Factorio closed the merge is intentionally empty, and it's still
    // useful to see whether the log is being interpreted correctly.
    println!("--- sources ---");
    println!("Factorio process     {}", yes_no(process.is_running()));
    println!("Log                  {}", log_path_hint(&data_dir));
    println!("  save               {}", opt(&log.facts().save_name));
    println!("  version            {}", opt(&log.facts().game_version));
    println!(
        "  multiplayer        {}",
        log.facts()
            .multiplayer
            .map(yes_no)
            .unwrap_or_else(|| "unknown".into())
    );
    println!("Mod file             {}", modfile.path().display());
    println!("  present            {}", yes_no(modfile.path().is_file()));
    println!();

    let state = merge(process.is_running(), log.facts(), modfile.state());

    println!("--- merged state ---");
    println!("Factorio process     {}", yes_no(state.running));
    println!(
        "Mod data             {}",
        if state.has_mod_data() {
            "yes"
        } else {
            "no (degraded mode)"
        }
    );
    println!("Mod file             {}", modfile.path().display());
    println!("Save                 {}", opt(&state.save_name));
    println!("Version              {}", opt(&state.game_version));
    println!(
        "Multiplayer          {}",
        state.multiplayer.map(yes_no).unwrap_or("unknown".into())
    );
    println!(
        "Surface              {}",
        state
            .surface
            .as_ref()
            .map(|s| format!("{} ({})", s.name, s.kind))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "Research             {}",
        state
            .research
            .as_ref()
            .map(|r| format!(
                "{}/{} · {}",
                r.done,
                r.total,
                r.label().unwrap_or("nothing queued")
            ))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "Playtime             {}",
        state
            .playtime_secs()
            .map(format_duration)
            .unwrap_or_else(|| "-".into())
    );

    println!();
    match render(&state, config) {
        Some(spec) => {
            println!("Would publish:");
            println!("  line 1      {}", opt(&spec.details));
            println!("  line 2      {}", opt(&spec.state));
            println!("  icon        {}", opt(&spec.large_image));
            println!("  tooltip     {}", opt(&spec.large_text));
        }
        None => println!("Nothing would be published (Factorio is not running)."),
    }

    Ok(())
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_string()
}

fn opt(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "-".into())
}

fn format_duration(total_secs: i64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    format!("{hours} h {minutes:02} min")
}

fn log_path_hint(data_dir: &std::path::Path) -> String {
    logfile::default_log_path(data_dir).display().to_string()
}
