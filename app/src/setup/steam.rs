//! Steam and user configuration location.
//!
//! The Windows registry (`HKCU\Software\Valve\Steam`) is read instead of assuming
//! `C:\Program Files (x86)\Steam`: Steam can be on another drive, and the active
//! user can only be known by asking it directly (`userdata` contains a folder
//! for every account that has ever logged in on this machine).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::vdf::unescape;

/// Factorio's identifier on Steam.
pub const FACTORIO_APP_ID: &str = "427520";

/// A SteamID64 minus this gives the account id that names the `userdata` folders.
const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

const STEAM_PROCESS: &str = "steam.exe";

#[derive(Debug, Clone)]
pub struct Steam {
    pub root: PathBuf,
    pub exe: PathBuf,
    pub account: u32,
}

impl Steam {
    /// `userdata\<account>\config\localconfig.vdf`, where the launch options live.
    pub fn localconfig(&self) -> PathBuf {
        self.root
            .join("userdata")
            .join(self.account.to_string())
            .join("config")
            .join("localconfig.vdf")
    }

    /// Does Factorio appear installed in any Steam library?
    pub fn factorio_installed(&self) -> bool {
        let manifest = format!("appmanifest_{FACTORIO_APP_ID}.acf");
        let libraries =
            std::fs::read_to_string(self.root.join("steamapps").join("libraryfolders.vdf"))
                .map(|text| parse_library_paths(&text))
                .unwrap_or_default();

        std::iter::once(self.root.clone())
            .chain(libraries)
            .any(|library| library.join("steamapps").join(&manifest).is_file())
    }

    /// Asks Steam to close and waits for the process to disappear.
    ///
    /// `steam.exe -shutdown` is the orderly way: Steam saves its state (and its
    /// `localconfig.vdf`) before exiting. Killing the process would lose changes.
    pub fn shutdown(&self, timeout: Duration) -> Result<()> {
        std::process::Command::new(&self.exe)
            .arg("-shutdown")
            .spawn()
            .with_context(|| format!("could not run {}", self.exe.display()))?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("Steam did not close within {} s", timeout.as_secs())
    }

    /// Starts Steam without waiting for it.
    pub fn start(&self) -> Result<()> {
        std::process::Command::new(&self.exe)
            .spawn()
            .with_context(|| format!("could not start {}", self.exe.display()))?;
        Ok(())
    }
}

/// Locates Steam and the active account.
pub fn locate() -> Result<Steam> {
    let root = registry::read_string(r"Software\Valve\Steam", "SteamPath")
        .map(|path| PathBuf::from(path.replace('/', "\\")))
        .filter(|path| path.is_dir())
        .context("Steam not found: HKCU\\Software\\Valve\\Steam\\SteamPath is missing")?;

    let exe = registry::read_string(r"Software\Valve\Steam", "SteamExe")
        .map(|path| PathBuf::from(path.replace('/', "\\")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| root.join("steam.exe"));

    let account = active_account(&root)?;
    Ok(Steam { root, exe, account })
}

/// Account with the session logged in. In order of reliability:
/// 1. registry `ActiveUser` (only valid while Steam is open: when cold it
///    is 0, which is discarded here);
/// 2. the one in `loginusers.vdf` — see [`parse_active_account`];
/// 3. the only `userdata` folder, if there is just one.
fn active_account(root: &Path) -> Result<u32> {
    if let Some(user) = registry::read_dword(r"Software\Valve\Steam\ActiveProcess", "ActiveUser") {
        if user != 0 {
            return Ok(user);
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("config").join("loginusers.vdf")) {
        if let Some(account) = parse_active_account(&text) {
            return Ok(account);
        }
    }

    let mut accounts = std::fs::read_dir(root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|id| *id != 0);
    match (accounts.next(), accounts.next()) {
        (Some(only), None) => Ok(only),
        _ => bail!("could not determine which Steam account is active"),
    }
}

/// Is there a Steam process running?
pub fn is_running() -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system.processes().values().any(|process| {
        process
            .name()
            .to_string_lossy()
            .eq_ignore_ascii_case(STEAM_PROCESS)
    })
}

//------------------------------------------------------------------------------
// VDF file parsing (pure, testable)
//------------------------------------------------------------------------------

/// Quoted value of a line `"key"   "value"`, if the key matches.
fn quoted_pair(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.trim().split('"');
    // "" | key | space | value | ""
    let (_, key, _, value) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    Some((key, value))
}

/// Paths of the Steam libraries in `libraryfolders.vdf`.
pub fn parse_library_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(quoted_pair)
        .filter(|(key, _)| key.eq_ignore_ascii_case("path"))
        .map(|(_, value)| PathBuf::from(unescape(value)))
        .collect()
}

/// Most likely account id from `loginusers.vdf`.
///
/// Each user is a block whose name is its SteamID64 (17 digits). These are
/// tried, in order:
/// 1. the `MostRecent "1"` flag — some versions of Steam write it;
/// 2. if there is only one remembered account, that one, whether or not it has
///    the keys above — it's the most common real case, and the one that breaks
///    if not handled separately: with Steam closed there is no other clue;
/// 3. if there are several and none is flagged, the one with the highest
///    `Timestamp` (the most recent login).
pub fn parse_active_account(text: &str) -> Option<u32> {
    let mut accounts: Vec<(u64, bool, u64)> = Vec::new(); // (SteamID64, MostRecent, Timestamp)
    let mut current: Option<usize> = None; // index in `accounts` of the block being parsed

    for line in text.lines() {
        if let Some((key, value)) = quoted_pair(line) {
            let Some(entry) = current.map(|i| &mut accounts[i]) else {
                continue;
            };
            if key.eq_ignore_ascii_case("MostRecent") && value == "1" {
                entry.1 = true;
            } else if key.eq_ignore_ascii_case("Timestamp") {
                entry.2 = value.parse().unwrap_or(0);
            }
            continue;
        }

        // Line with a single string: may be the SteamID64 that opens a block.
        let name = line.trim().trim_matches('"');
        if name.len() == 17 && name.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(id) = name.parse() {
                current = Some(accounts.len());
                accounts.push((id, false, 0));
            }
        }
    }

    let chosen = accounts
        .iter()
        .find(|(_, most_recent, _)| *most_recent)
        .or_else(|| match accounts.as_slice() {
            [only] => Some(only),
            _ => accounts.iter().max_by_key(|(_, _, timestamp)| *timestamp),
        })?;

    chosen
        .0
        .checked_sub(STEAM_ID64_BASE)
        .and_then(|account| u32::try_from(account).ok())
}

//------------------------------------------------------------------------------
// Windows registry
//------------------------------------------------------------------------------

mod registry {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn read_string(subkey: &str, name: &str) -> Option<String> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut size: u32 = 0;

        // First call: just asks how many bytes are needed.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if status != 0 || size == 0 {
            return None;
        }

        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != 0 {
            return None;
        }

        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..len]))
    }

    pub fn read_dword(subkey: &str, name: &str) -> Option<u32> {
        let (subkey, name) = (wide(subkey), wide(name));
        let mut value: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;

        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut value as *mut u32).cast(),
                &mut size,
            )
        };
        (status == 0).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARIES: &str = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"427520\"\t\t\"123\"\n\t\t}\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";

    #[test]
    fn reads_all_libraries_and_unescapes() {
        assert_eq!(
            parse_library_paths(LIBRARIES),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn no_libraries_gives_empty_list() {
        assert!(parse_library_paths("").is_empty());
        assert!(parse_library_paths("\"libraryfolders\"\n{\n}\n").is_empty());
    }

    const LOGINUSERS: &str = "\"users\"\n{\n\t\"76561198000000001\"\n\t{\n\t\t\"AccountName\"\t\t\"vieja\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t\t\"Timestamp\"\t\t\"1000\"\n\t}\n\t\"76561198236141462\"\n\t{\n\t\t\"AccountName\"\t\t\"actual\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"500\"\n\t}\n}\n";

    // The real format captured on this machine: a single remembered account,
    // without MostRecent (this version of Steam doesn't write it), only
    // Timestamp. This is the case that broke detection with Steam closed
    // before this fix.
    const LOGINUSERS_REAL: &str = "\"users\"\n{\n\t\"76561198236141462\"\n\t{\n\t\t\"AccountName\"\t\t\"puertollano7\"\n\t\t\"PersonaName\"\t\t\"enrik0\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"WantsOfflineMode\"\t\t\"0\"\n\t\t\"SkipOfflineModeWarning\"\t\t\"0\"\n\t\t\"AutoLogin\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"1790450811\"\n\t}\n}\n";

    #[test]
    fn active_account_is_the_one_marked_most_recent_even_if_not_the_latest() {
        // 76561198236141462 - 76561197960265728 = 275875734. It has MostRecent
        // but a lower Timestamp than the other account: it still wins.
        assert_eq!(parse_active_account(LOGINUSERS), Some(275_875_734));
    }

    #[test]
    fn without_most_recent_the_highest_timestamp_wins() {
        let sin = LOGINUSERS.replace("\"MostRecent\"\t\t\"1\"\n\t\t", "");
        // Now neither has MostRecent; the one with Timestamp 1000 (the "old" one) wins.
        // 76561198000000001 - 76561197960265728 = 39734273
        assert_eq!(parse_active_account(&sin), Some(39_734_273));
    }

    #[test]
    fn a_single_remembered_account_is_used_even_without_any_flag() {
        // Real case: Steam closed, loginusers.vdf without MostRecent, one account.
        assert_eq!(parse_active_account(LOGINUSERS_REAL), Some(275_875_734));
    }

    #[test]
    fn no_accounts_means_nothing_to_choose() {
        assert_eq!(parse_active_account(""), None);
        assert_eq!(parse_active_account("\"users\"\n{\n}\n"), None);
    }

    #[test]
    fn localconfig_path_comes_from_the_account() {
        let steam = Steam {
            root: PathBuf::from(r"C:\Steam"),
            exe: PathBuf::from(r"C:\Steam\steam.exe"),
            account: 275_875_734,
        };
        assert_eq!(
            steam.localconfig(),
            PathBuf::from(r"C:\Steam\userdata\275875734\config\localconfig.vdf")
        );
    }
}
