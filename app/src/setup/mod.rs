//! Guided installation: sets up Steam without the user touching paths.
//!
//! The application acts as a launcher (`"…\factorio-discord-rp.exe" %command%`),
//! but pasting that line by hand is tedious. This module computes it with the
//! real paths and, if allowed to, writes it into Steam's configuration.
//!
//! The logic lives here and not in the installer so it can be tested: the
//! wizard only calls the application and checks its exit code.

mod clipboard;
mod options;
mod steam;
mod vdf;

use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use crate::paths;
use steam::{Steam, FACTORIO_APP_ID};

/// How long to wait for Steam to finish closing.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Close Steam if it's open, instead of refusing.
    pub close_steam: bool,
    /// Reopen Steam afterwards, only if we closed it.
    pub restart_steam: bool,
}

/// What to do with Steam's configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Install,
    Uninstall,
}

/// Why it failed, with a distinct exit code so the installer can decide what
/// to tell the user without having to parse text.
#[derive(Debug)]
pub enum Failure {
    /// Steam is open and closing it wasn't allowed. Code 10.
    SteamRunning,
    /// There's no Steam, or Factorio isn't installed on it. Code 11.
    NotFound(String),
    /// The configuration couldn't be read or written. Code 12.
    Write(String),
    /// Any other error. Code 1.
    Other(String),
}

impl Failure {
    pub fn exit_code(&self) -> i32 {
        match self {
            Failure::Other(_) => 1,
            Failure::SteamRunning => 10,
            Failure::NotFound(_) => 11,
            Failure::Write(_) => 12,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::SteamRunning => write!(
                f,
                "Steam is open: it has to be closed to change its configuration"
            ),
            Failure::NotFound(msg) | Failure::Write(msg) | Failure::Other(msg) => {
                write!(f, "{msg}")
            }
        }
    }
}

impl From<anyhow::Error> for Failure {
    fn from(err: anyhow::Error) -> Self {
        Failure::Other(format!("{err:#}"))
    }
}

type Outcome = Result<(), Failure>;

fn exe() -> Result<String> {
    Ok(paths::current_exe()?.to_string_lossy().into_owned())
}

fn read_options(steam: &Steam) -> Result<Option<String>> {
    let path = steam.localconfig();
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("could not read {}", path.display()))?;
    vdf::launch_options(&text, FACTORIO_APP_ID)
}

/// The complete line that needs to be in Steam's options.
///
/// If what's already there can be read, it's computed respecting it; if not,
/// it's the simple line. This is what's shown to the user who prefers to
/// paste it by hand.
fn resolved_line() -> Result<String> {
    let exe = exe()?;
    let existing = steam::locate()
        .and_then(|steam| read_options(&steam))
        .ok()
        .flatten()
        .unwrap_or_default();
    Ok(options::install(&existing, &exe))
}

/// Report of what was detected plus the line to paste, copied to the clipboard.
pub fn report() -> Outcome {
    let exe = exe()?;
    println!("App:               {exe}");

    match steam::locate() {
        Ok(steam) => {
            println!("Steam:             {}", steam.root.display());
            println!("Active account:    {}", steam.account);
            println!(
                "Factorio:          {}",
                if steam.factorio_installed() {
                    "installed"
                } else {
                    "does NOT appear to be installed"
                }
            );
            println!(
                "Steam open:        {}",
                if steam::is_running() {
                    "yes (it has to be closed to apply the change)"
                } else {
                    "no"
                }
            );
            match read_options(&steam) {
                Ok(Some(current)) => println!("Current options:   {current}"),
                Ok(None) => println!("Current options:   (none)"),
                Err(err) => println!("Current options:   could not be read ({err:#})"),
            }
        }
        Err(err) => println!("Steam:             not found ({err:#})"),
    }

    let line = resolved_line()?;
    println!();
    println!("Full line to paste into Steam → Factorio → Properties → Launch Options:");
    println!();
    println!("{line}");
    println!();
    match clipboard::copy(&line) {
        Ok(()) => println!("(Copied to the clipboard.)"),
        Err(err) => println!("(Could not copy to the clipboard: {err:#})"),
    }
    Ok(())
}

/// Prints only the line, nothing else: consumed by the installer.
pub fn print_command() -> Outcome {
    println!("{}", resolved_line()?);
    Ok(())
}

/// Copies the line to the clipboard and prints it.
pub fn copy_command() -> Outcome {
    let line = resolved_line()?;
    clipboard::copy(&line)?;
    println!("{line}");
    Ok(())
}

/// Enables or disables startup with Windows.
pub fn set_autostart(enabled: bool) -> Outcome {
    crate::autostart::set_enabled(enabled)?;
    Ok(())
}

/// What would change in Steam's configuration.
struct Plan {
    before: Option<String>,
    after: Option<String>,
    /// Full content of the already-modified file.
    text: String,
}

impl Plan {
    fn changes_anything(&self) -> bool {
        self.before != self.after
    }
}

fn plan(text: &str, exe: &str, action: Action) -> Result<Plan> {
    let before = vdf::launch_options(text, FACTORIO_APP_ID)?;
    let current = before.clone().unwrap_or_default();

    let target = match action {
        Action::Install => options::install(&current, exe),
        Action::Uninstall => options::uninstall(&current, exe),
    };
    // Empty = nothing left worth saving: the key is deleted instead of left blank.
    let after = (!target.is_empty()).then_some(target);

    let text = if after == before {
        text.to_string()
    } else {
        vdf::set_launch_options(text, FACTORIO_APP_ID, after.as_deref())?
    };

    Ok(Plan {
        before,
        after,
        text,
    })
}

fn locate_steam() -> Result<Steam, Failure> {
    steam::locate().map_err(|err| Failure::NotFound(format!("{err:#}")))
}

fn read_config(steam: &Steam) -> Result<String, Failure> {
    let path = steam.localconfig();
    std::fs::read_to_string(&path)
        .map_err(|err| Failure::Write(format!("could not read {}: {err}", path.display())))
}

/// Shows what would change, without writing anything.
pub fn dry_run(action: Action) -> Outcome {
    let exe = exe()?;
    let steam = locate_steam()?;
    let text = read_config(&steam)?;
    let plan = plan(&text, &exe, action)?;

    println!("File:     {}", steam.localconfig().display());
    println!(
        "Steam:    {}",
        if steam::is_running() {
            "open (it would need to be closed to actually apply this)"
        } else {
            "closed"
        }
    );
    println!(
        "Before:   {}",
        plan.before.as_deref().unwrap_or("(no options)")
    );
    println!(
        "After:    {}",
        plan.after.as_deref().unwrap_or("(no options)")
    );
    if plan.changes_anything() {
        println!("A backup of the file would be made and only that key would change.");
    } else {
        println!("Nothing would change.");
    }
    println!("Nothing was written (--dry-run).");
    Ok(())
}

/// Puts the application in Factorio's launch options.
pub fn apply(opts: &Options) -> Outcome {
    change(opts, Action::Install)
}

/// Removes the application from the launch options, leaving the rest as is.
pub fn uninstall(opts: &Options) -> Outcome {
    change(opts, Action::Uninstall)
}

fn change(opts: &Options, action: Action) -> Outcome {
    let exe = exe()?;
    let steam = locate_steam()?;

    if action == Action::Install && !steam.factorio_installed() {
        return Err(Failure::NotFound(
            "Factorio does not appear to be installed in any Steam library".into(),
        ));
    }

    // Steam rewrites localconfig.vdf while it's running: editing it while open gets lost.
    let mut closed_by_us = false;
    if steam::is_running() {
        if !opts.close_steam {
            return Err(Failure::SteamRunning);
        }
        println!("Closing Steam…");
        steam
            .shutdown(SHUTDOWN_TIMEOUT)
            .map_err(|err| Failure::Other(format!("{err:#}")))?;
        closed_by_us = true;
    }

    let result = rewrite(&steam, &exe, action);

    // Reopened even if the change failed: nobody should be left without Steam.
    if closed_by_us && opts.restart_steam {
        println!("Reopening Steam…");
        if let Err(err) = steam.start() {
            eprintln!("warning: could not reopen Steam: {err:#}");
        }
    }

    result
}

fn rewrite(steam: &Steam, exe: &str, action: Action) -> Outcome {
    let path = steam.localconfig();
    let text = read_config(steam)?;
    let plan = plan(&text, exe, action)?;

    if !plan.changes_anything() {
        println!(
            "{}",
            match action {
                Action::Install => "It was already configured: nothing to change.",
                Action::Uninstall => "There was nothing to remove.",
            }
        );
        return Ok(());
    }

    let backup = backup_path(&path);
    std::fs::copy(&path, &backup).map_err(|err| {
        Failure::Write(format!(
            "could not create backup {}: {err}",
            backup.display()
        ))
    })?;
    println!("Backup: {}", backup.display());

    write_atomically(&path, &plan.text)
        .map_err(|err| Failure::Write(format!("could not write {}: {err}", path.display())))?;

    println!(
        "Factorio launch options: {}",
        plan.after.as_deref().unwrap_or("(no options)")
    );
    Ok(())
}

/// Writes to a temporary file and renames it over the original: if something
/// fails midway, the original stays intact instead of truncated.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let temp = path.with_extension("vdf.tmp");
    std::fs::write(&temp, text)?;
    std::fs::rename(&temp, path)
}

fn backup_path(path: &Path) -> std::path::PathBuf {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    path.with_extension(format!("vdf.bak-{}", utc_stamp(secs)))
}

/// `YYYYMMDD-HHMMSS` in UTC, without depending on a date library.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);

    // Days since 1970 → civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);

    format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str =
        r"C:\Users\villa\AppData\Local\Programs\Factorio Discord RP\factorio-discord-rp.exe";

    /// Minimal file with the real structure: `apps` → Factorio.
    fn sample() -> String {
        format!(
            "\"UserLocalConfigStore\"\n{{\n\t\"apps\"\n\t{{\n\t\t\"{FACTORIO_APP_ID}\"\n\t\t{{\n\t\t\t\"playtime\"\t\t\"54904\"\n\t\t}}\n\t}}\n}}\n"
        )
    }

    #[test]
    fn installing_and_uninstalling_leaves_the_file_as_it_was() {
        let original = sample();

        let installed = plan(&original, EXE, Action::Install).unwrap();
        assert!(installed.changes_anything());
        assert_eq!(
            installed.after.as_deref(),
            Some(format!("\"{EXE}\" %command%").as_str())
        );

        let removed = plan(&installed.text, EXE, Action::Uninstall).unwrap();
        assert_eq!(removed.after, None);
        assert_eq!(removed.text, original);
    }

    #[test]
    fn installing_twice_changes_nothing_the_second_time() {
        let one = plan(&sample(), EXE, Action::Install).unwrap();
        let two = plan(&one.text, EXE, Action::Install).unwrap();
        assert!(!two.changes_anything());
        assert_eq!(two.text, one.text);
    }

    #[test]
    fn uninstalling_when_not_installed_changes_nothing() {
        let plan = plan(&sample(), EXE, Action::Uninstall).unwrap();
        assert!(!plan.changes_anything());
        assert_eq!(plan.text, sample());
    }

    #[test]
    fn respects_options_the_user_already_had() {
        let with_options =
            vdf::set_launch_options(&sample(), FACTORIO_APP_ID, Some("%command% -x")).unwrap();

        let installed = plan(&with_options, EXE, Action::Install).unwrap();
        assert_eq!(
            installed.after.as_deref(),
            Some(format!("\"{EXE}\" %command% -x").as_str())
        );

        let removed = plan(&installed.text, EXE, Action::Uninstall).unwrap();
        assert_eq!(removed.after.as_deref(), Some("%command% -x"));
    }

    #[test]
    fn the_exit_codes_are_the_documented_ones() {
        assert_eq!(Failure::SteamRunning.exit_code(), 10);
        assert_eq!(Failure::NotFound(String::new()).exit_code(), 11);
        assert_eq!(Failure::Write(String::new()).exit_code(), 12);
        assert_eq!(Failure::Other(String::new()).exit_code(), 1);
    }

    #[test]
    fn the_backup_stamp_is_a_readable_date() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_000_000_000), "20010909-014640");
        // February 29 of a leap year.
        assert_eq!(utc_stamp(951_782_400), "20000229-000000");
    }

    #[test]
    fn the_backup_carries_the_stamp_in_its_name() {
        let backup = backup_path(Path::new(r"C:\Steam\userdata\1\config\localconfig.vdf"));
        let name = backup.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("localconfig.vdf.bak-"), "{name}");
    }
}
