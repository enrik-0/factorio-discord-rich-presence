//! Instancia única por sesión.
//!
//! Con el autoarranque y la línea de Steam a la vez, cada arranque de Factorio
//! lanzaría otra copia de la aplicación, todas publicando sobre la misma tarjeta
//! de Discord. Un bloqueo con nombre deja pasar sólo a la primera.

#[cfg(windows)]
pub use windows_impl::acquire;

#[cfg(unix)]
pub use unix_impl::acquire;

#[cfg(windows)]
mod windows_impl {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    /// Nombre del mutex. `Local\` lo limita a la sesión del usuario, de modo que
    /// dos usuarios en el mismo equipo no se estorban.
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
}

/// En Unix no hay un objeto "mutex con nombre" del sistema: se usa un `flock`
/// (advisory lock) sobre un fichero fijo en el directorio de datos de la app.
/// Al morir el proceso el fichero queda ahí, pero el bloqueo se libera solo:
/// lo mantiene el descriptor abierto, no el fichero en sí.
#[cfg(unix)]
mod unix_impl {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};

    use fd_lock::{RwLock, RwLockWriteGuard};

    const LOCK_FILE_NAME: &str = "instance.lock";

    /// Mientras exista, esta copia es la única. Se libera al soltarlo.
    ///
    /// El `RwLock<File>` vive en el heap y se filtra deliberadamente (`Box::leak`)
    /// para poder guardar el guard de escritura, que toma prestado de él, sin
    /// recurrir a una estructura autorreferencial ni a código `unsafe`. El único
    /// coste es no liberar esa asignación hasta que el proceso termine, momento
    /// en el que el sistema operativo la recupera igualmente.
    ///
    /// `None` cuando no se pudo ni abrir el fichero ni bloquearlo por un motivo
    /// ambiguo (permisos, etc.): igual que en Windows con un mutex nulo, se deja
    /// pasar en vez de negar el arranque por algo que no se sabe interpretar.
    #[allow(
        dead_code,
        reason = "el campo sólo se sostiene por su Drop (libera el flock); nunca se lee"
    )]
    pub struct InstanceGuard(Option<RwLockWriteGuard<'static, File>>);

    /// Toma la instancia única, o devuelve `None` si ya hay otra copia en marcha.
    pub fn acquire() -> Option<InstanceGuard> {
        let path = match lock_path() {
            Ok(path) => path,
            // Sin ruta no se puede saber si hay otra copia. Mejor arrancar dos
            // veces que no arrancar: la presencia es lo que se pide.
            Err(_) => return Some(InstanceGuard(None)),
        };
        acquire_at(&path)
    }

    fn acquire_at(path: &Path) -> Option<InstanceGuard> {
        let Ok(file) = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path)
        else {
            return Some(InstanceGuard(None));
        };

        let lock: &'static mut RwLock<File> = Box::leak(Box::new(RwLock::new(file)));
        match lock.try_write() {
            Ok(guard) => Some(InstanceGuard(Some(guard))),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => None,
            // Cualquier otro fallo (permisos, sistema de ficheros sin locking...)
            // es tan ambiguo como no tener ruta: se deja pasar, igual que arriba.
            Err(_) => Some(InstanceGuard(None)),
        }
    }

    fn lock_path() -> anyhow::Result<PathBuf> {
        Ok(crate::paths::app_dir()?.join(LOCK_FILE_NAME))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn unique_path(tag: &str) -> PathBuf {
            std::env::temp_dir().join(format!(
                "factorio-drp-test-{tag}-{}.lock",
                std::process::id()
            ))
        }

        #[test]
        fn la_segunda_copia_no_puede_tomarla() {
            let path = unique_path("segunda");
            let first = acquire_at(&path);
            assert!(first.is_some());
            assert!(acquire_at(&path).is_none());
            drop(first);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn al_soltarla_otra_copia_puede_tomarla() {
            let path = unique_path("soltar");
            let first = acquire_at(&path);
            assert!(first.is_some());
            drop(first);
            assert!(acquire_at(&path).is_some());
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn rutas_distintas_no_se_estorban() {
            let a = acquire_at(&unique_path("a"));
            let b = acquire_at(&unique_path("b"));
            assert!(a.is_some() && b.is_some());
            let _ = std::fs::remove_file(unique_path("a"));
            let _ = std::fs::remove_file(unique_path("b"));
        }
    }
}
