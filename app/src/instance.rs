//! Instancia única por sesión de Windows.
//!
//! Con el autoarranque y la línea de Steam a la vez, cada arranque de Factorio
//! lanzaría otra copia de la aplicación, todas publicando sobre la misma tarjeta
//! de Discord. Un mutex con nombre deja pasar sólo a la primera.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Nombre del mutex. `Local\` lo limita a la sesión del usuario, de modo que dos
/// usuarios en el mismo equipo no se estorban.
const MUTEX_NAME: &str = "Local\\FactorioDiscordRichPresence";

/// Mientras exista, esta copia es la única. Se libera al soltarlo.
pub struct InstanceGuard(HANDLE);

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// Toma la instancia única, o devuelve `None` si ya hay otra copia en marcha.
pub fn acquire() -> Option<InstanceGuard> {
    acquire_named(MUTEX_NAME)
}

fn acquire_named(name: &str) -> Option<InstanceGuard> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        // Sin poder crear el mutex no se puede saber si hay otra copia. Mejor
        // arrancar dos veces que no arrancar: la presencia es lo que se pide.
        return Some(InstanceGuard(handle));
    }

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            CloseHandle(handle);
        }
        return None;
    }

    Some(InstanceGuard(handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique(tag: &str) -> String {
        format!("Local\\FactorioDrpTest-{tag}-{}", std::process::id())
    }

    #[test]
    fn la_segunda_copia_no_puede_tomarla() {
        let name = unique("segunda");
        let first = acquire_named(&name);
        assert!(first.is_some());
        assert!(acquire_named(&name).is_none());
    }

    #[test]
    fn al_soltarla_otra_copia_puede_tomarla() {
        let name = unique("soltar");
        let first = acquire_named(&name);
        assert!(first.is_some());
        drop(first);
        assert!(acquire_named(&name).is_some());
    }

    #[test]
    fn nombres_distintos_no_se_estorban() {
        let a = acquire_named(&unique("a"));
        let b = acquire_named(&unique("b"));
        assert!(a.is_some() && b.is_some());
    }
}
