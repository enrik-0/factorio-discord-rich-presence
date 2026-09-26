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
mod instance;
mod lifetime;
mod logging;
mod merge;
mod model;
mod paths;
mod presence;
mod run;
mod setup;
mod sources;
mod status;
mod tray;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tracing::{error, info, warn};

use crate::config::Config;
use crate::presence::discord::unix_now;
use crate::presence::{ActivitySpec, DiscordSink};
use crate::status::Shared;

const HELP: &str = "\
Discord Rich Presence para Factorio 2.1

USO:
    factorio-discord-rp [OPCIONES]
    factorio-discord-rp [OPCIONES] <factorio.exe> [ARGUMENTOS DEL JUEGO...]

Sin opciones arranca en la bandeja del sistema.

Con la ruta del juego actúa de lanzador: arranca Factorio, publica mientras siga
abierto y se cierra con él. Todo lo que sigue a la ruta es del juego. En Steam,
en las opciones de lanzamiento de Factorio:
    \"C:\\ruta\\factorio-discord-rp.exe\" %command%

OPCIONES:
    --tray             Fuerza el modo bandeja (es lo que usa el autoarranque)
    --console          Vigila desde la consola, sin icono de bandeja
    --check            Comprueba configuración y rutas, sin conectar con Discord
    --dump             Muestra lo que ven las fuentes y qué se publicaría
    --selftest         Publica una actividad de prueba fija y la mantiene
    --config <RUTA>    Fichero de configuración concreto
    -h, --help         Muestra esta ayuda

CONFIGURACIÓN DE STEAM (la usa el instalador, también sirve a mano):
    --setup            Detecta Steam y muestra la línea completa para las opciones
                       de lanzamiento de Factorio, copiada al portapapeles
    --apply            Pone la aplicación en esas opciones, con copia de seguridad
    --uninstall        La quita de ellas, dejando el resto
    --dry-run          Con lo anterior: enseña qué cambiaría, sin escribir nada
    --close-steam      Cierra Steam si está abierto (hay que cerrarlo para cambiar
                       su configuración)
    --restart-steam    Lo reabre después, si lo ha cerrado la aplicación
    --print-command    Imprime sólo la línea de lanzamiento
    --copy-command     Copia la línea de lanzamiento al portapapeles
    --autostart on|off Activa o desactiva el arranque con Windows

Códigos de salida de --apply y --uninstall: 0 hecho, 10 Steam abierto, 11 no se
encuentra Steam o Factorio, 12 no se pudo leer o escribir la configuración.
";

/// El juego que hay que lanzar, tal y como lo entrega Steam con `%command%`.
#[derive(Debug, PartialEq, Eq)]
struct GameCommand {
    exe: PathBuf,
    /// Argumentos del juego, sin interpretar: no son opciones de esta aplicación.
    args: Vec<OsString>,
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Mode {
    #[default]
    Tray,
    Console,
    Check,
    Dump,
    Selftest,
    Help,
    /// Arranca el juego y se queda mientras siga abierto.
    Launch(GameCommand),
    /// Informe de Steam y línea para pegar (con `--dry-run`, qué cambiaría).
    Setup,
    Apply,
    Uninstall,
    PrintCommand,
    CopyCommand,
    Autostart(bool),
}

#[derive(Debug, Default)]
struct Args {
    mode: Mode,
    config: Option<PathBuf>,
    // Modificadores de la configuración de Steam: valen en cualquier orden.
    close_steam: bool,
    restart_steam: bool,
    dry_run: bool,
}

fn parse_args() -> Result<Args> {
    parse(std::env::args_os().skip(1))
}

/// Recibe `OsString` y no `String`: la ruta del juego y sus argumentos pueden
/// no ser Unicode válido, y no hay motivo para fallar por eso.
fn parse(raw: impl IntoIterator<Item = OsString>) -> Result<Args> {
    let mut args = Args::default();
    let mut raw = raw.into_iter();

    while let Some(arg) = raw.next() {
        let Some(text) = arg.to_str().filter(|text| text.starts_with('-')) else {
            // El primer argumento que no es una opción nuestra es el juego; todo
            // lo que le sigue es suyo, incluidas las cosas que empiezan por `-`.
            if args.mode != Mode::Tray {
                bail!("la ruta del juego no se puede combinar con otras opciones\n\n{HELP}");
            }
            args.mode = Mode::Launch(GameCommand {
                exe: PathBuf::from(arg),
                args: raw.collect(),
            });
            break;
        };

        args.mode = match text {
            "--tray" => Mode::Tray,
            "--console" => Mode::Console,
            "--check" => Mode::Check,
            "--dump" => Mode::Dump,
            "--selftest" => Mode::Selftest,
            "-h" | "--help" => Mode::Help,
            // `--setup` es el modo por omisión de la configuración de Steam y no
            // debe pisar a `--apply` o `--uninstall`, vengan antes o después.
            "--setup" => {
                if args.mode == Mode::Tray {
                    args.mode = Mode::Setup;
                }
                continue;
            }
            "--apply" => Mode::Apply,
            "--uninstall" => Mode::Uninstall,
            "--print-command" => Mode::PrintCommand,
            "--copy-command" => Mode::CopyCommand,
            "--close-steam" => {
                args.close_steam = true;
                continue;
            }
            "--restart-steam" => {
                args.restart_steam = true;
                continue;
            }
            "--dry-run" => {
                args.dry_run = true;
                continue;
            }
            "--autostart" => match raw.next().and_then(|value| value.into_string().ok()) {
                Some(value) if value == "on" => Mode::Autostart(true),
                Some(value) if value == "off" => Mode::Autostart(false),
                _ => bail!("--autostart necesita `on` u `off`"),
            },
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

    // La configuración de Steam imprime su resultado y termina con un código de
    // salida que el instalador interpreta. Sin registro: cualquier línea de más
    // en la salida estropearía `--print-command`.
    if matches!(
        args.mode,
        Mode::Setup
            | Mode::Apply
            | Mode::Uninstall
            | Mode::PrintCommand
            | Mode::CopyCommand
            | Mode::Autostart(_)
    ) {
        return run_setup(&args);
    }

    // En la bandeja no hay consola donde mirar: el registro va a fichero. El
    // lanzador también vive en la bandeja, y además lo arranca Steam sin consola.
    let log_path = if matches!(args.mode, Mode::Tray | Mode::Launch(_)) {
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

    // El lanzador va antes de leer la configuración: si esta falla, Factorio tiene
    // que arrancar igualmente. Sin presencia, pero arrancar.
    if let Mode::Launch(game) = args.mode {
        return launch(args.config.as_deref(), log_path, game);
    }

    let config = Config::load(args.config.as_deref())?;

    match args.mode {
        Mode::Help
        | Mode::Launch(_)
        | Mode::Setup
        | Mode::Apply
        | Mode::Uninstall
        | Mode::PrintCommand
        | Mode::CopyCommand
        | Mode::Autostart(_) => unreachable!("atendidos antes"),
        Mode::Check => run_check(&config),
        Mode::Dump => run::dump(&config),
        Mode::Selftest => run_selftest(&config),
        Mode::Console => {
            let shared = Arc::new(Shared::default());
            run::run(&config, &shared, false)
        }
        Mode::Tray => run_resident(config, log_path),
    }
}

/// Modos de configuración de Steam: hacen su trabajo, imprimen y salen con el
/// código que corresponde (0, 10, 11, 12 o 1; ver `setup::Failure`).
fn run_setup(args: &Args) -> Result<()> {
    use setup::Action;

    let options = setup::Options {
        close_steam: args.close_steam,
        restart_steam: args.restart_steam,
    };

    let outcome = match &args.mode {
        Mode::Setup if args.dry_run => setup::dry_run(Action::Install),
        Mode::Setup => setup::report(),
        Mode::Apply if args.dry_run => setup::dry_run(Action::Install),
        Mode::Apply => setup::apply(&options),
        Mode::Uninstall if args.dry_run => setup::dry_run(Action::Uninstall),
        Mode::Uninstall => setup::uninstall(&options),
        Mode::PrintCommand => setup::print_command(),
        Mode::CopyCommand => setup::copy_command(),
        Mode::Autostart(on) => setup::set_autostart(*on),
        _ => unreachable!("sólo se llama con los modos de configuración"),
    };

    if let Err(failure) = outcome {
        eprintln!("error: {failure}");
        std::process::exit(failure.exit_code());
    }
    Ok(())
}

/// Bandeja residente, por ejemplo la del autoarranque. Una sola por sesión.
fn run_resident(config: Config, log_path: Option<PathBuf>) -> Result<()> {
    let Some(_guard) = instance::acquire() else {
        info!("ya hay otra copia de la aplicación en marcha; no se abre otra");
        return Ok(());
    };
    tray::run(config, log_path, false)
}

/// Modo lanzador: arranca Factorio y se queda mientras siga abierto.
///
/// Si ya hay una copia residente (autoarranque) es ella la que publica, y aquí
/// sólo se lanza el juego: dos copias pisarían la misma tarjeta de Discord.
fn launch(config_path: Option<&Path>, log_path: Option<PathBuf>, game: GameCommand) -> Result<()> {
    let guard = instance::acquire();

    if let Err(err) = spawn_game(&game) {
        error!("no se pudo lanzar el juego: {err:#}");
        return Err(err);
    }

    let Some(_guard) = guard else {
        info!("ya hay otra copia en marcha: ella se encarga de la presencia");
        return Ok(());
    };

    let config = match Config::load(config_path) {
        Ok(config) => config,
        Err(err) => {
            error!("configuración inválida; Factorio arranca sin presencia: {err:#}");
            return Ok(());
        }
    };

    tray::run(config, log_path, true)
}

/// Arranca el juego sin esperarlo. Soltar el `Child` no lo detiene.
///
/// No se fija el directorio de trabajo: es el que Steam ha preparado para esta
/// aplicación y el juego debe heredarlo igual que si lo hubiera lanzado Steam.
///
/// Entrada y salida van a `NUL`. Antes de llegar aquí se ha soltado la consola
/// (`FreeConsole`), y sin ella no hay manejadores estándar que heredar: `spawn`
/// falla con "controlador no válido" (error 6) y Factorio no llegaría a abrirse.
fn spawn_game(game: &GameCommand) -> Result<()> {
    use std::process::Stdio;

    info!(juego = %game.exe.display(), argumentos = game.args.len(), "lanzando Factorio");
    std::process::Command::new(&game.exe)
        .args(&game.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("no se pudo lanzar {}", game.exe.display()))?;
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(items: &[&str]) -> Result<Args> {
        parse(items.iter().map(OsString::from))
    }

    fn game(exe: &str, args: &[&str]) -> Mode {
        Mode::Launch(GameCommand {
            exe: PathBuf::from(exe),
            args: args.iter().map(OsString::from).collect(),
        })
    }

    #[test]
    fn sin_argumentos_arranca_la_bandeja() {
        let args = parse_strs(&[]).unwrap();
        assert_eq!(args.mode, Mode::Tray);
        assert_eq!(args.config, None);
    }

    #[test]
    fn las_opciones_de_diagnostico_siguen_funcionando() {
        assert_eq!(parse_strs(&["--console"]).unwrap().mode, Mode::Console);
        assert_eq!(parse_strs(&["--check"]).unwrap().mode, Mode::Check);
        assert_eq!(parse_strs(&["--dump"]).unwrap().mode, Mode::Dump);
        assert_eq!(parse_strs(&["--selftest"]).unwrap().mode, Mode::Selftest);
        assert_eq!(parse_strs(&["-h"]).unwrap().mode, Mode::Help);
    }

    #[test]
    fn la_ruta_del_juego_activa_el_modo_lanzador() {
        let args = parse_strs(&[r"D:\SteamLibrary\Factorio\bin\x64\factorio.exe"]).unwrap();
        assert_eq!(
            args.mode,
            game(r"D:\SteamLibrary\Factorio\bin\x64\factorio.exe", &[])
        );
    }

    #[test]
    fn los_argumentos_del_juego_pasan_intactos() {
        // Incluye opciones que empiezan por `-` y rutas con espacios: nada de eso
        // es de esta aplicación.
        let args = parse_strs(&[
            r"C:\Juegos\Factorio\factorio.exe",
            "--mod-directory",
            r"D:\mis mods",
            "--load-game",
            "partida.zip",
            "--check",
        ])
        .unwrap();

        assert_eq!(
            args.mode,
            game(
                r"C:\Juegos\Factorio\factorio.exe",
                &[
                    "--mod-directory",
                    r"D:\mis mods",
                    "--load-game",
                    "partida.zip",
                    "--check"
                ]
            )
        );
    }

    #[test]
    fn las_opciones_propias_van_antes_del_juego() {
        let args =
            parse_strs(&["--config", r"C:\c.toml", "factorio.exe", "--config", "x"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from(r"C:\c.toml")));
        assert_eq!(args.mode, game("factorio.exe", &["--config", "x"]));
    }

    #[test]
    fn tray_explicito_tambien_admite_juego() {
        // El autoarranque pasa `--tray`; combinarlo con un juego no debe fallar.
        let args = parse_strs(&["--tray", "factorio.exe"]).unwrap();
        assert_eq!(args.mode, game("factorio.exe", &[]));
    }

    #[test]
    fn el_juego_no_se_mezcla_con_los_modos_de_diagnostico() {
        assert!(parse_strs(&["--check", "factorio.exe"]).is_err());
        assert!(parse_strs(&["--console", "factorio.exe"]).is_err());
    }

    #[test]
    fn una_opcion_propia_desconocida_es_un_error() {
        assert!(parse_strs(&["--nada"]).is_err());
    }

    #[test]
    fn config_sin_ruta_es_un_error() {
        assert!(parse_strs(&["--config"]).is_err());
    }

    #[test]
    fn las_opciones_de_steam_valen_en_cualquier_orden() {
        let a = parse_strs(&["--setup", "--apply", "--close-steam", "--restart-steam"]).unwrap();
        let b = parse_strs(&["--restart-steam", "--close-steam", "--apply", "--setup"]).unwrap();
        for args in [a, b] {
            assert_eq!(args.mode, Mode::Apply);
            assert!(args.close_steam && args.restart_steam);
        }
    }

    #[test]
    fn setup_a_secas_es_el_informe() {
        let args = parse_strs(&["--setup"]).unwrap();
        assert_eq!(args.mode, Mode::Setup);
        assert!(!args.dry_run && !args.close_steam && !args.restart_steam);
        assert!(parse_strs(&["--setup", "--dry-run"]).unwrap().dry_run);
    }

    #[test]
    fn los_modos_de_steam_se_reconocen() {
        assert_eq!(parse_strs(&["--uninstall"]).unwrap().mode, Mode::Uninstall);
        assert_eq!(
            parse_strs(&["--print-command"]).unwrap().mode,
            Mode::PrintCommand
        );
        assert_eq!(
            parse_strs(&["--copy-command"]).unwrap().mode,
            Mode::CopyCommand
        );
    }

    #[test]
    fn autostart_pide_on_u_off() {
        assert_eq!(
            parse_strs(&["--autostart", "on"]).unwrap().mode,
            Mode::Autostart(true)
        );
        assert_eq!(
            parse_strs(&["--autostart", "off"]).unwrap().mode,
            Mode::Autostart(false)
        );
        assert!(parse_strs(&["--autostart"]).is_err());
        assert!(parse_strs(&["--autostart", "quizas"]).is_err());
    }

    #[test]
    fn la_configuracion_de_steam_no_se_mezcla_con_un_juego() {
        assert!(parse_strs(&["--apply", "factorio.exe"]).is_err());
        assert!(parse_strs(&["--setup", "factorio.exe"]).is_err());
    }

    #[test]
    fn la_ruta_del_juego_puede_no_ser_unicode() {
        use std::os::windows::ffi::OsStringExt;
        // Un sustituto suelto no es UTF-16 válido, pero Windows lo permite en rutas.
        let raw: OsString = OsString::from_wide(&[0x0043, 0xD800, 0x0046]);
        let args = parse([raw.clone()]).unwrap();
        assert_eq!(
            args.mode,
            Mode::Launch(GameCommand {
                exe: PathBuf::from(raw),
                args: vec![]
            })
        );
    }
}
