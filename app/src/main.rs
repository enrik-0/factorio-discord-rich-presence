//! Discord Rich Presence for Factorio 2.0+.
//!
//! The Factorio mod writes the game state to `script-output`; this app
//! reads it, fills it in with the game's log, and publishes it to Discord.
//!
//! With no arguments it starts in the system tray, which is normal use. The
//! command-line modes exist for diagnostics.

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
Discord Rich Presence for Factorio 2.0+

USAGE:
    factorio-discord-rp [OPTIONS]
    factorio-discord-rp [OPTIONS] <factorio.exe> [GAME ARGUMENTS...]

With no options it starts in the system tray.

With the game's path it acts as a launcher: it starts Factorio, publishes
while it stays open, and closes with it. Everything after the path belongs
to the game. In Steam, in Factorio's launch options:
    \"C:\\path\\factorio-discord-rp.exe\" %command%

OPTIONS:
    --tray             Force tray mode (what autostart uses)
    --console          Watch from the console, no tray icon
    --check            Check configuration and paths, without connecting to Discord
    --dump             Show what the sources see and what would be published
    --selftest         Publish a fixed test activity and keep it up
    --config <PATH>    A specific configuration file
    -h, --help         Show this help

STEAM SETUP (used by the installer, also usable by hand):
    --setup            Detects Steam and shows the full line for Factorio's
                       launch options, copied to the clipboard
    --apply            Puts the app into those options, with a backup
    --uninstall        Removes it from them, leaving the rest
    --dry-run          With the above: shows what would change, without writing anything
    --close-steam      Closes Steam if it's open (it has to be closed to change
                       its configuration)
    --restart-steam    Reopens it afterwards, if the app closed it
    --print-command    Prints just the launch line
    --copy-command     Copies the launch line to the clipboard
    --autostart on|off Turns starting with Windows on or off

Exit codes for --apply and --uninstall: 0 done, 10 Steam is open, 11 Steam or
Factorio not found, 12 could not read or write the configuration.
";

/// The game to launch, exactly as Steam hands it over via `%command%`.
#[derive(Debug, PartialEq, Eq)]
struct GameCommand {
    exe: PathBuf,
    /// The game's arguments, uninterpreted: they aren't this app's own options.
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
    /// Starts the game and stays around while it's open.
    Launch(GameCommand),
    /// Steam report and the line to paste (with `--dry-run`, what would change).
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
    // Steam-configuration modifiers: valid in any order.
    close_steam: bool,
    restart_steam: bool,
    dry_run: bool,
}

fn parse_args() -> Result<Args> {
    parse(std::env::args_os().skip(1))
}

/// Takes `OsString`, not `String`: the game's path and its arguments may not
/// be valid Unicode, and there's no reason to fail over that.
fn parse(raw: impl IntoIterator<Item = OsString>) -> Result<Args> {
    let mut args = Args::default();
    let mut raw = raw.into_iter();

    while let Some(arg) = raw.next() {
        let Some(text) = arg.to_str().filter(|text| text.starts_with('-')) else {
            // The first argument that isn't one of our options is the game;
            // everything after it belongs to it, including things starting with `-`.
            if args.mode != Mode::Tray {
                bail!("the game's path cannot be combined with other options\n\n{HELP}");
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
            // `--setup` is the Steam configuration's default mode and must not
            // override `--apply` or `--uninstall`, whether they come before or after.
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
                _ => bail!("--autostart needs `on` or `off`"),
            },
            "--config" => {
                let value = raw
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--config needs a path"))?;
                args.config = Some(PathBuf::from(value));
                continue;
            }
            other => bail!("unknown option: {other}\n\n{HELP}"),
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

    // The Steam configuration prints its result and exits with a code the
    // installer interprets. No logging: any extra line in the output would
    // break `--print-command`.
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

    // In the tray there's no console to look at: logging goes to a file. The
    // launcher also lives in the tray, and besides, Steam starts it with no console.
    let log_path = if matches!(args.mode, Mode::Tray | Mode::Launch(_)) {
        hide_console();
        match logging::init_file() {
            Ok(path) => Some(path),
            Err(err) => {
                // It can carry on without logging; not starting at all would be worse.
                logging::init_console();
                warn!(%err, "could not open the log file");
                None
            }
        }
    } else {
        logging::init_console();
        None
    };

    // The launcher runs before reading the configuration: if that fails,
    // Factorio still has to start. Without presence, but it starts.
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
        | Mode::Autostart(_) => unreachable!("handled earlier"),
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

/// Steam-configuration modes: they do their job, print, and exit with the
/// matching code (0, 10, 11, 12, or 1; see `setup::Failure`).
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
        _ => unreachable!("only called with the configuration modes"),
    };

    if let Err(failure) = outcome {
        eprintln!("error: {failure}");
        std::process::exit(failure.exit_code());
    }
    Ok(())
}

/// Resident tray, e.g. the one from autostart. Only one per session.
fn run_resident(config: Config, log_path: Option<PathBuf>) -> Result<()> {
    let Some(_guard) = instance::acquire() else {
        info!("another copy of the app is already running; not opening a second one");
        return Ok(());
    };
    tray::run(config, log_path, false)
}

/// Launcher mode: starts Factorio and stays around while it's open.
///
/// If a resident copy (autostart) is already running, it's the one
/// publishing, and here only the game gets launched: two copies would
/// stomp on the same Discord card.
fn launch(config_path: Option<&Path>, log_path: Option<PathBuf>, game: GameCommand) -> Result<()> {
    let guard = instance::acquire();

    if let Err(err) = spawn_game(&game) {
        error!("could not launch the game: {err:#}");
        return Err(err);
    }

    let Some(_guard) = guard else {
        info!("another copy is already running: it's handling the presence");
        return Ok(());
    };

    let config = match Config::load(config_path) {
        Ok(config) => config,
        Err(err) => {
            error!("invalid configuration; Factorio starts without presence: {err:#}");
            return Ok(());
        }
    };

    tray::run(config, log_path, true)
}

/// Starts the game without waiting for it. Dropping the `Child` doesn't stop it.
///
/// The working directory isn't set: it's whatever Steam has prepared for
/// this app, and the game must inherit it just as if Steam had launched it.
///
/// Stdin/stdout/stderr go to `NUL`. By the time we get here the console has
/// already been released (`FreeConsole`), and without it there are no
/// standard handles to inherit: `spawn` fails with "invalid handle" (error
/// 6) and Factorio would never open.
fn spawn_game(game: &GameCommand) -> Result<()> {
    use std::process::Stdio;

    info!(game = %game.exe.display(), arguments = game.args.len(), "launching Factorio");
    std::process::Command::new(&game.exe)
        .args(&game.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("could not launch {}", game.exe.display()))?;
    Ok(())
}

/// Hides the console window in tray mode.
///
/// The executable is built as a console application so `--check` and
/// `--dump` work normally from a terminal. The trade-off is that a console
/// appears when starting with Windows, which gets closed here. If the
/// process came from a terminal, this only detaches it from that terminal.
fn hide_console() {
    unsafe {
        windows_sys::Win32::System::Console::FreeConsole();
    }
}

/// Checks what can be checked without depending on Discord or Factorio.
fn run_check(config: &Config) -> Result<()> {
    match config.application_id() {
        Ok(id) => println!("Application ID     ok ({} digits)", id.len()),
        Err(err) => println!("Application ID     MISSING\n  {err}"),
    }

    match config.factorio_data_dir() {
        Ok(dir) => {
            println!("Factorio data      {}", dir.display());
            let log = dir.join("factorio-current.log");
            println!(
                "  factorio-current.log  {}",
                if log.is_file() { "found" } else { "missing" }
            );
            let script_output = dir.join("script-output");
            println!(
                "  script-output         {}",
                if script_output.is_dir() {
                    "found"
                } else {
                    "missing (will be created once the mod is enabled)"
                }
            );
        }
        Err(err) => println!("Factorio data      NOT FOUND\n  {err}"),
    }

    match paths::app_dir() {
        Ok(dir) => println!("App data           {}", dir.display()),
        Err(err) => println!("App data           NOT AVAILABLE\n  {err}"),
    }
    println!(
        "Autostart          {}",
        if autostart::is_enabled() {
            "enabled"
        } else {
            "disabled"
        }
    );

    Ok(())
}

/// Publishes a fixed activity and keeps it alive.
///
/// Validates the full path from Application ID → named pipe → visible card
/// on the profile, including reconnecting if Discord closes and reopens.
fn run_selftest(config: &Config) -> Result<()> {
    let application_id = config.application_id()?;
    let mut sink = DiscordSink::new(application_id)?;

    // A fake save with 4 h 12 min of playtime, to check the timer.
    let playtime_secs = 4 * 3600 + 12 * 60;
    let spec = ActivitySpec {
        details: Some("Fulgora · Rocket Co.".into()),
        state: Some("Researching Electromagnetic plant (64%)".into()),
        large_image: Some(config.large_image.clone()),
        large_text: Some("Fulgora · 142/247 technologies".into()),
        small_image: None,
        small_text: None,
        start_timestamp: Some(unix_now() - playtime_secs),
        party: None,
    };

    info!("publishing test activity; Ctrl+C to quit");
    info!("check it from ANOTHER Discord account: your own profile doesn't show everything");

    let mut announced = false;
    loop {
        if sink.publish(&spec) {
            if !announced {
                info!("activity sent — it should already show on your profile");
                announced = true;
            }
        } else if !sink.is_connected() && !announced {
            warn!("waiting for Discord to become available…");
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
    fn no_arguments_starts_the_tray() {
        let args = parse_strs(&[]).unwrap();
        assert_eq!(args.mode, Mode::Tray);
        assert_eq!(args.config, None);
    }

    #[test]
    fn the_diagnostic_options_still_work() {
        assert_eq!(parse_strs(&["--console"]).unwrap().mode, Mode::Console);
        assert_eq!(parse_strs(&["--check"]).unwrap().mode, Mode::Check);
        assert_eq!(parse_strs(&["--dump"]).unwrap().mode, Mode::Dump);
        assert_eq!(parse_strs(&["--selftest"]).unwrap().mode, Mode::Selftest);
        assert_eq!(parse_strs(&["-h"]).unwrap().mode, Mode::Help);
    }

    #[test]
    fn the_games_path_triggers_launcher_mode() {
        let args = parse_strs(&[r"D:\SteamLibrary\Factorio\bin\x64\factorio.exe"]).unwrap();
        assert_eq!(
            args.mode,
            game(r"D:\SteamLibrary\Factorio\bin\x64\factorio.exe", &[])
        );
    }

    #[test]
    fn the_games_arguments_pass_through_untouched() {
        // Includes options starting with `-` and paths with spaces: none of
        // that belongs to this app.
        let args = parse_strs(&[
            r"C:\Games\Factorio\factorio.exe",
            "--mod-directory",
            r"D:\my mods",
            "--load-game",
            "save.zip",
            "--check",
        ])
        .unwrap();

        assert_eq!(
            args.mode,
            game(
                r"C:\Games\Factorio\factorio.exe",
                &[
                    "--mod-directory",
                    r"D:\my mods",
                    "--load-game",
                    "save.zip",
                    "--check"
                ]
            )
        );
    }

    #[test]
    fn our_own_options_go_before_the_game() {
        let args =
            parse_strs(&["--config", r"C:\c.toml", "factorio.exe", "--config", "x"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from(r"C:\c.toml")));
        assert_eq!(args.mode, game("factorio.exe", &["--config", "x"]));
    }

    #[test]
    fn explicit_tray_also_accepts_a_game() {
        // Autostart passes `--tray`; combining it with a game must not fail.
        let args = parse_strs(&["--tray", "factorio.exe"]).unwrap();
        assert_eq!(args.mode, game("factorio.exe", &[]));
    }

    #[test]
    fn the_game_does_not_mix_with_diagnostic_modes() {
        assert!(parse_strs(&["--check", "factorio.exe"]).is_err());
        assert!(parse_strs(&["--console", "factorio.exe"]).is_err());
    }

    #[test]
    fn an_unknown_option_is_an_error() {
        assert!(parse_strs(&["--nada"]).is_err());
    }

    #[test]
    fn config_without_a_path_is_an_error() {
        assert!(parse_strs(&["--config"]).is_err());
    }

    #[test]
    fn the_steam_options_work_in_any_order() {
        let a = parse_strs(&["--setup", "--apply", "--close-steam", "--restart-steam"]).unwrap();
        let b = parse_strs(&["--restart-steam", "--close-steam", "--apply", "--setup"]).unwrap();
        for args in [a, b] {
            assert_eq!(args.mode, Mode::Apply);
            assert!(args.close_steam && args.restart_steam);
        }
    }

    #[test]
    fn setup_alone_is_the_report() {
        let args = parse_strs(&["--setup"]).unwrap();
        assert_eq!(args.mode, Mode::Setup);
        assert!(!args.dry_run && !args.close_steam && !args.restart_steam);
        assert!(parse_strs(&["--setup", "--dry-run"]).unwrap().dry_run);
    }

    #[test]
    fn the_steam_modes_are_recognized() {
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
    fn autostart_requires_on_or_off() {
        assert_eq!(
            parse_strs(&["--autostart", "on"]).unwrap().mode,
            Mode::Autostart(true)
        );
        assert_eq!(
            parse_strs(&["--autostart", "off"]).unwrap().mode,
            Mode::Autostart(false)
        );
        assert!(parse_strs(&["--autostart"]).is_err());
        assert!(parse_strs(&["--autostart", "maybe"]).is_err());
    }

    #[test]
    fn steam_configuration_does_not_mix_with_a_game() {
        assert!(parse_strs(&["--apply", "factorio.exe"]).is_err());
        assert!(parse_strs(&["--setup", "factorio.exe"]).is_err());
    }

    #[test]
    fn the_games_path_may_not_be_unicode() {
        use std::os::windows::ffi::OsStringExt;
        // A lone surrogate isn't valid UTF-16, but Windows allows it in paths.
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
