//! Localización de Steam y de la configuración del usuario.
//!
//! Se lee el registro de Windows (`HKCU\Software\Valve\Steam`) en vez de suponer
//! `C:\Program Files (x86)\Steam`: Steam puede estar en otra unidad, y el usuario
//! activo sólo se sabe preguntándoselo a él (`userdata` contiene una carpeta por
//! cada cuenta que haya iniciado sesión alguna vez en el equipo).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use super::vdf::unescape;

/// Identificador de Factorio en Steam.
pub const FACTORIO_APP_ID: &str = "427520";

/// Un SteamID64 menos esto da el id de cuenta que nombra las carpetas de `userdata`.
const STEAM_ID64_BASE: u64 = 76_561_197_960_265_728;

const STEAM_PROCESS: &str = "steam.exe";

#[derive(Debug, Clone)]
pub struct Steam {
    pub root: PathBuf,
    pub exe: PathBuf,
    pub account: u32,
}

impl Steam {
    /// `userdata\<cuenta>\config\localconfig.vdf`, donde viven las opciones de lanzamiento.
    pub fn localconfig(&self) -> PathBuf {
        self.root
            .join("userdata")
            .join(self.account.to_string())
            .join("config")
            .join("localconfig.vdf")
    }

    /// ¿Aparece Factorio instalado en alguna biblioteca de Steam?
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

    /// Pide a Steam que se cierre y espera a que el proceso desaparezca.
    ///
    /// `steam.exe -shutdown` es la forma ordenada: Steam guarda su estado (y su
    /// `localconfig.vdf`) antes de salir. Matar el proceso perdería cambios.
    pub fn shutdown(&self, timeout: Duration) -> Result<()> {
        std::process::Command::new(&self.exe)
            .arg("-shutdown")
            .spawn()
            .with_context(|| format!("no se pudo ejecutar {}", self.exe.display()))?;

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("Steam no se cerró en {} s", timeout.as_secs())
    }

    /// Arranca Steam sin esperarlo.
    pub fn start(&self) -> Result<()> {
        std::process::Command::new(&self.exe)
            .spawn()
            .with_context(|| format!("no se pudo arrancar {}", self.exe.display()))?;
        Ok(())
    }
}

/// Localiza Steam y la cuenta activa.
pub fn locate() -> Result<Steam> {
    let root = registry::read_string(r"Software\Valve\Steam", "SteamPath")
        .map(|path| PathBuf::from(path.replace('/', "\\")))
        .filter(|path| path.is_dir())
        .context("no se encontró Steam: falta HKCU\\Software\\Valve\\Steam\\SteamPath")?;

    let exe = registry::read_string(r"Software\Valve\Steam", "SteamExe")
        .map(|path| PathBuf::from(path.replace('/', "\\")))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| root.join("steam.exe"));

    let account = active_account(&root)?;
    Ok(Steam { root, exe, account })
}

/// Cuenta con la sesión iniciada. Por orden de fiabilidad:
/// 1. `ActiveUser` del registro (sólo vale mientras Steam está abierto: en
///    frío vale 0, que aquí se descarta);
/// 2. la de `loginusers.vdf` — ver [`parse_active_account`];
/// 3. la única carpeta de `userdata`, si sólo hay una.
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
        _ => bail!("no se pudo saber qué cuenta de Steam está activa"),
    }
}

/// ¿Hay un proceso de Steam en ejecución?
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
// Análisis de ficheros VDF (puro, comprobable)
//------------------------------------------------------------------------------

/// Valor entrecomillado de una línea `"clave"   "valor"`, si la clave coincide.
fn quoted_pair(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.trim().split('"');
    // "" | clave | espacio | valor | ""
    let (_, key, _, value) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    Some((key, value))
}

/// Rutas de las bibliotecas de Steam en `libraryfolders.vdf`.
pub fn parse_library_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(quoted_pair)
        .filter(|(key, _)| key.eq_ignore_ascii_case("path"))
        .map(|(_, value)| PathBuf::from(unescape(value)))
        .collect()
}

/// Id de cuenta más probable de `loginusers.vdf`.
///
/// Cada usuario es un bloque cuyo nombre es su SteamID64 (17 dígitos). Se
/// prueban, en orden:
/// 1. el marcado `MostRecent "1"` — algunas versiones de Steam lo escriben;
/// 2. si sólo hay una cuenta recordada, ésa, tenga o no las claves de arriba —
///    es el caso real más común y el que falla si no se cubre aparte: con
///    Steam cerrado no hay ninguna otra pista;
/// 3. si hay varias y ninguna está marcada, la de `Timestamp` más alto (el
///    último inicio de sesión).
pub fn parse_active_account(text: &str) -> Option<u32> {
    let mut accounts: Vec<(u64, bool, u64)> = Vec::new(); // (SteamID64, MostRecent, Timestamp)
    let mut current: Option<usize> = None; // índice en `accounts` del bloque en curso

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

        // Línea con una sola cadena: puede ser el SteamID64 que abre un bloque.
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
// Registro de Windows
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

        // Primera llamada: sólo pregunta cuántos bytes hacen falta.
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
    fn lee_todas_las_bibliotecas_y_deshace_el_escapado() {
        assert_eq!(
            parse_library_paths(LIBRARIES),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r"D:\SteamLibrary"),
            ]
        );
    }

    #[test]
    fn sin_bibliotecas_da_lista_vacia() {
        assert!(parse_library_paths("").is_empty());
        assert!(parse_library_paths("\"libraryfolders\"\n{\n}\n").is_empty());
    }

    const LOGINUSERS: &str = "\"users\"\n{\n\t\"76561198000000001\"\n\t{\n\t\t\"AccountName\"\t\t\"vieja\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t\t\"Timestamp\"\t\t\"1000\"\n\t}\n\t\"76561198236141462\"\n\t{\n\t\t\"AccountName\"\t\t\"actual\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"500\"\n\t}\n}\n";

    // El formato real capturado en este equipo: una sola cuenta recordada, sin
    // MostRecent (esta versión de Steam no lo escribe), sólo Timestamp. Es el
    // caso que rompía la detección con Steam cerrado antes de este arreglo.
    const LOGINUSERS_REAL: &str = "\"users\"\n{\n\t\"76561198236141462\"\n\t{\n\t\t\"AccountName\"\t\t\"puertollano7\"\n\t\t\"PersonaName\"\t\t\"enrik0\"\n\t\t\"RememberPassword\"\t\t\"1\"\n\t\t\"WantsOfflineMode\"\t\t\"0\"\n\t\t\"SkipOfflineModeWarning\"\t\t\"0\"\n\t\t\"AutoLogin\"\t\t\"1\"\n\t\t\"Timestamp\"\t\t\"1790450811\"\n\t}\n}\n";

    #[test]
    fn la_cuenta_activa_es_la_marcada_most_recent_aunque_no_sea_la_ultima() {
        // 76561198236141462 - 76561197960265728 = 275875734. Tiene MostRecent
        // pero un Timestamp menor que la otra cuenta: gana igualmente.
        assert_eq!(parse_active_account(LOGINUSERS), Some(275_875_734));
    }

    #[test]
    fn sin_most_recent_gana_el_timestamp_mas_alto() {
        let sin = LOGINUSERS.replace("\"MostRecent\"\t\t\"1\"\n\t\t", "");
        // Ahora ninguna tiene MostRecent; la de Timestamp 1000 (la "vieja") gana.
        // 76561198000000001 - 76561197960265728 = 39734273
        assert_eq!(parse_active_account(&sin), Some(39_734_273));
    }

    #[test]
    fn una_sola_cuenta_recordada_se_usa_aunque_no_tenga_ninguna_marca() {
        // Caso real: Steam cerrado, loginusers.vdf sin MostRecent, una cuenta.
        assert_eq!(parse_active_account(LOGINUSERS_REAL), Some(275_875_734));
    }

    #[test]
    fn sin_cuentas_no_hay_nada_que_elegir() {
        assert_eq!(parse_active_account(""), None);
        assert_eq!(parse_active_account("\"users\"\n{\n}\n"), None);
    }

    #[test]
    fn la_ruta_de_localconfig_sale_de_la_cuenta() {
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
