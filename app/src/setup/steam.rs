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
/// 1. `ActiveUser` del registro (sólo vale mientras Steam está abierto);
/// 2. la marcada `MostRecent` en `loginusers.vdf`;
/// 3. la única carpeta de `userdata`, si sólo hay una.
fn active_account(root: &Path) -> Result<u32> {
    if let Some(user) = registry::read_dword(r"Software\Valve\Steam\ActiveProcess", "ActiveUser") {
        if user != 0 {
            return Ok(user);
        }
    }

    if let Ok(text) = std::fs::read_to_string(root.join("config").join("loginusers.vdf")) {
        if let Some(account) = parse_most_recent_account(&text) {
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

/// Id de cuenta del usuario marcado `MostRecent "1"` en `loginusers.vdf`.
///
/// Cada usuario es un bloque cuyo nombre es su SteamID64 (17 dígitos).
pub fn parse_most_recent_account(text: &str) -> Option<u32> {
    let mut current: Option<u64> = None;

    for line in text.lines() {
        let Some((key, value)) = quoted_pair(line) else {
            // Línea con una sola cadena: puede ser el nombre de un bloque.
            let name = line.trim().trim_matches('"');
            if name.len() == 17 && name.bytes().all(|b| b.is_ascii_digit()) {
                current = name.parse().ok();
            }
            continue;
        };

        if key.eq_ignore_ascii_case("MostRecent") && value == "1" {
            return current
                .and_then(|id| id.checked_sub(STEAM_ID64_BASE))
                .and_then(|account| u32::try_from(account).ok());
        }
    }

    None
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

    const LOGINUSERS: &str = "\"users\"\n{\n\t\"76561198000000001\"\n\t{\n\t\t\"AccountName\"\t\t\"vieja\"\n\t\t\"MostRecent\"\t\t\"0\"\n\t}\n\t\"76561198236141462\"\n\t{\n\t\t\"AccountName\"\t\t\"actual\"\n\t\t\"MostRecent\"\t\t\"1\"\n\t}\n}\n";

    #[test]
    fn la_cuenta_activa_es_la_marcada_most_recent() {
        // 76561198236141462 - 76561197960265728 = 275875734
        assert_eq!(parse_most_recent_account(LOGINUSERS), Some(275_875_734));
    }

    #[test]
    fn sin_most_recent_no_hay_cuenta() {
        let sin = LOGINUSERS.replace("\"MostRecent\"\t\t\"1\"", "\"MostRecent\"\t\t\"0\"");
        assert_eq!(parse_most_recent_account(&sin), None);
        assert_eq!(parse_most_recent_account(""), None);
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
