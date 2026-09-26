//! Bucle principal: sondea las fuentes, fusiona, renderiza y publica.

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

/// Cadencia del bucle. Discord sólo acepta una actualización cada 15 s, así que
/// sondear cada 2 s va de sobra y mantiene el consumo en nada.
const TICK: Duration = Duration::from_secs(2);

/// Trocito en el que se parte la espera, para que cerrar desde la bandeja no
/// tarde un ciclo entero en notarse.
const SHUTDOWN_POLL: Duration = Duration::from_millis(200);

/// Con `exit_with_game` la aplicación pide su propio cierre cuando Factorio, ya
/// visto, deja de estar en ejecución (modo lanzador).
pub fn run(config: &Config, shared: &Shared, exit_with_game: bool) -> Result<()> {
    let application_id = config.application_id()?;
    let data_dir = config.factorio_data_dir()?;
    let script_output = data_dir.join("script-output");

    info!(datos = %data_dir.display(), "vigilando Factorio");

    let mut sink = DiscordSink::new(application_id)?;
    let mut process = ProcessWatcher::new();
    let mut log = LogWatcher::new(logfile::default_log_path(&data_dir));
    let mut modfile = ModFileWatcher::new(&script_output);

    debug!(fichero = %modfile.path().display(), "fichero de estado del mod");

    let mut was_running = false;
    let mut lifetime = exit_with_game.then(|| GameLifetime::new(Instant::now()));

    while !shared.is_shutdown() {
        process.poll();
        let running = process.is_running();

        if running {
            // Sólo tocamos disco mientras el juego está vivo.
            log.poll();
            modfile.poll();
        }

        if running != was_running {
            if running {
                info!("Factorio en ejecución");
            } else {
                info!("Factorio cerrado; limpiando el estado de Discord");
                sink.clear();
                shared.update(|status| status.headline = None);
            }
            was_running = running;
        }

        if let Some(lifetime) = lifetime.as_mut() {
            if lifetime.should_exit(running, Instant::now()) {
                info!("Factorio ya no está en ejecución; cerrando la aplicación");
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
                        detalles = spec.details.as_deref().unwrap_or("-"),
                        estado = spec.state.as_deref().unwrap_or("-"),
                        modo = if state.has_mod_data() {
                            "completo"
                        } else {
                            "degradado (sin mod)"
                        },
                        "estado actualizado"
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

    info!("cerrando");
    sink.clear();
    Ok(())
}

/// Duerme el ciclo, pero comprobando de vez en cuando si toca cerrar.
fn sleep_until_shutdown(shared: &Shared, total: Duration) {
    let mut left = total;
    while left > Duration::ZERO && !shared.is_shutdown() {
        let step = left.min(SHUTDOWN_POLL);
        std::thread::sleep(step);
        left -= step;
    }
}

/// Diagnóstico: imprime lo que ven las fuentes ahora mismo, sin tocar Discord.
pub fn dump(config: &Config) -> Result<()> {
    let data_dir = config.factorio_data_dir()?;
    let script_output = data_dir.join("script-output");

    let mut process = ProcessWatcher::new();
    let mut log = LogWatcher::new(logfile::default_log_path(&data_dir));
    let mut modfile = ModFileWatcher::new(&script_output);

    process.poll();
    log.poll();
    modfile.poll();

    // Se muestran los hechos crudos de cada fuente además del estado fusionado:
    // con Factorio cerrado la fusión queda vacía a propósito, y aun así interesa
    // ver si el log se está interpretando bien.
    println!("--- fuentes ---");
    println!("Proceso de Factorio  {}", yes_no(process.is_running()));
    println!("Log                  {}", log_path_hint(&data_dir));
    println!("  save               {}", opt(&log.facts().save_name));
    println!("  versión            {}", opt(&log.facts().game_version));
    println!(
        "  multijugador       {}",
        log.facts()
            .multiplayer
            .map(yes_no)
            .unwrap_or_else(|| "desconocido".into())
    );
    println!("Fichero del mod      {}", modfile.path().display());
    println!("  presente           {}", yes_no(modfile.path().is_file()));
    println!();

    let state = merge(process.is_running(), log.facts(), modfile.state());

    println!("--- estado fusionado ---");
    println!("Proceso de Factorio  {}", yes_no(state.running));
    println!(
        "Datos del mod        {}",
        if state.has_mod_data() {
            "sí"
        } else {
            "no (modo degradado)"
        }
    );
    println!("Fichero del mod      {}", modfile.path().display());
    println!("Save                 {}", opt(&state.save_name));
    println!("Versión              {}", opt(&state.game_version));
    println!(
        "Multijugador         {}",
        state
            .multiplayer
            .map(yes_no)
            .unwrap_or("desconocido".into())
    );
    println!(
        "Superficie           {}",
        state
            .surface
            .as_ref()
            .map(|s| format!("{} ({})", s.name, s.kind))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "Investigación        {}",
        state
            .research
            .as_ref()
            .map(|r| format!(
                "{}/{} · {}",
                r.done,
                r.total,
                r.label().unwrap_or("nada en cola")
            ))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "Tiempo jugado        {}",
        state
            .playtime_secs()
            .map(format_duration)
            .unwrap_or_else(|| "-".into())
    );

    println!();
    match render(&state, config) {
        Some(spec) => {
            println!("Se publicaría:");
            println!("  línea 1     {}", opt(&spec.details));
            println!("  línea 2     {}", opt(&spec.state));
            println!("  icono       {}", opt(&spec.large_image));
            println!("  tooltip     {}", opt(&spec.large_text));
        }
        None => println!("No se publicaría nada (Factorio no está en ejecución)."),
    }

    Ok(())
}

fn yes_no(value: bool) -> String {
    if value { "sí" } else { "no" }.to_string()
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
