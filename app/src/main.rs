//! Discord Rich Presence para Factorio 2.1.
//!
//! El mod de Factorio escribe el estado de la partida en `script-output`; esta
//! aplicación lo lee, lo completa con el registro del juego y lo publica en
//! Discord.
//!
//! Sin argumentos arranca en la bandeja del sistema, que es el uso normal. Los
//! modos de línea de órdenes existen para diagnosticar.

mod autostart;
mod config;
mod logging;
mod merge;
mod model;
mod paths;
mod presence;
mod run;
mod sources;
mod status;
mod tray;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Result};
use tracing::{info, warn};

use crate::config::Config;
use crate::presence::discord::unix_now;
use crate::presence::{ActivitySpec, DiscordSink};
use crate::status::Shared;

const HELP: &str = "\
Discord Rich Presence para Factorio 2.1

USO:
    factorio-discord-rp [OPCIONES]

Sin opciones arranca en la bandeja del sistema.

OPCIONES:
    --tray             Fuerza el modo bandeja (es lo que usa el autoarranque)
    --console          Vigila desde la consola, sin icono de bandeja
    --check            Comprueba configuración y rutas, sin conectar con Discord
    --dump             Muestra lo que ven las fuentes y qué se publicaría
    --selftest         Publica una actividad de prueba fija y la mantiene
    --config <RUTA>    Fichero de configuración concreto
    -h, --help         Muestra esta ayuda
";

#[derive(Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Tray,
    Console,
    Check,
    Dump,
    Selftest,
    Help,
}

#[derive(Debug, Default)]
struct Args {
    mode: Mode,
    config: Option<PathBuf>,
}

fn parse_args() -> Result<Args> {
    let mut args = Args::default();
    let mut raw = std::env::args().skip(1);

    while let Some(arg) = raw.next() {
        args.mode = match arg.as_str() {
            "--tray" => Mode::Tray,
            "--console" => Mode::Console,
            "--check" => Mode::Check,
            "--dump" => Mode::Dump,
            "--selftest" => Mode::Selftest,
            "-h" | "--help" => Mode::Help,
            "--config" => {
                let value = raw
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--config necesita una ruta"))?;
                args.config = Some(PathBuf::from(value));
                continue;
            }
            other => bail!("opción desconocida: {other}\n\n{HELP}"),
        };
    }

    Ok(args)
}

fn main() -> Result<()> {
    let args = parse_args()?;

    if args.mode == Mode::Help {
        print!("{HELP}");
        return Ok(());
    }

    // En la bandeja no hay consola donde mirar: el registro va a fichero.
    let log_path = if args.mode == Mode::Tray {
        hide_console();
        match logging::init_file() {
            Ok(path) => Some(path),
            Err(err) => {
                // Sin registro se puede seguir; peor sería no arrancar.
                logging::init_console();
                warn!(%err, "no se pudo abrir el registro en fichero");
                None
            }
        }
    } else {
        logging::init_console();
        None
    };

    let config = Config::load(args.config.as_deref())?;

    match args.mode {
        Mode::Help => unreachable!("atendido antes de preparar el registro"),
        Mode::Check => run_check(&config),
        Mode::Dump => run::dump(&config),
        Mode::Selftest => run_selftest(&config),
        Mode::Console => {
            let shared = Arc::new(Shared::default());
            run::run(&config, &shared)
        }
        Mode::Tray => tray::run(config, log_path),
    }
}

/// Oculta la ventana de consola en modo bandeja.
///
/// El ejecutable se compila como aplicación de consola para que `--check` y
/// `--dump` funcionen con normalidad desde una terminal. La contrapartida es que
/// al arrancar con Windows aparece una consola, que se cierra aquí. Si el
/// proceso viene de una terminal, esto sólo lo desengancha de ella.
fn hide_console() {
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
}

/// Comprueba lo que se puede comprobar sin depender de Discord ni de Factorio.
fn run_check(config: &Config) -> Result<()> {
    match config.application_id() {
        Ok(id) => println!("Application ID     ok ({} dígitos)", id.len()),
        Err(err) => println!("Application ID     FALTA\n  {err}"),
    }

    match config.factorio_data_dir() {
        Ok(dir) => {
            println!("Datos de Factorio  {}", dir.display());
            let log = dir.join("factorio-current.log");
            println!(
                "  factorio-current.log  {}",
                if log.is_file() {
                    "encontrado"
                } else {
                    "ausente"
                }
            );
            let script_output = dir.join("script-output");
            println!(
                "  script-output         {}",
                if script_output.is_dir() {
                    "encontrado"
                } else {
                    "ausente (se creará al activar el mod)"
                }
            );
        }
        Err(err) => println!("Datos de Factorio  NO ENCONTRADOS\n  {err}"),
    }

    match paths::app_dir() {
        Ok(dir) => println!("Datos de la app    {}", dir.display()),
        Err(err) => println!("Datos de la app    NO DISPONIBLES\n  {err}"),
    }
    println!(
        "Autoarranque       {}",
        if autostart::is_enabled() {
            "activado"
        } else {
            "desactivado"
        }
    );

    Ok(())
}

/// Publica una actividad fija y la mantiene viva.
///
/// Valida el camino completo Application ID → tubería con nombre → tarjeta
/// visible en el perfil, incluyendo la reconexión si se cierra y reabre Discord.
fn run_selftest(config: &Config) -> Result<()> {
    let application_id = config.application_id()?;
    let mut sink = DiscordSink::new(application_id)?;

    // Un save ficticio con 4 h 12 min de juego, para comprobar el cronómetro.
    let playtime_secs = 4 * 3600 + 12 * 60;
    let spec = ActivitySpec {
        details: Some("Fulgora · Cohetes S.A.".into()),
        state: Some("Investigando Planta electromagnética (64%)".into()),
        large_image: Some(config.large_image.clone()),
        large_text: Some("Fulgora · 142/247 tecnologías".into()),
        small_image: None,
        small_text: None,
        start_timestamp: Some(unix_now() - playtime_secs),
        party: None,
    };

    info!("publicando actividad de prueba; Ctrl+C para salir");
    info!("compruébalo desde OTRA cuenta de Discord: el propio perfil no muestra todo");

    let mut announced = false;
    loop {
        if sink.publish(&spec) {
            if !announced {
                info!("actividad enviada — debería verse ya en tu perfil");
                announced = true;
            }
        } else if !sink.is_connected() && !announced {
            warn!("esperando a que Discord esté disponible…");
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
