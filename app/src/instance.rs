//! Single instance per session.
//!
//! With autostart and the Steam launch line both active, every Factorio
//! startup would launch another copy of the application, all publishing to
//! the same Discord card. A named lock only lets the first one through.

#[cfg(windows)]
pub use windows_impl::acquire;

#[cfg(unix)]
pub use unix_impl::acquire;

#[cfg(windows)]
mod windows_impl {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    /// Mutex name. `Local\` scopes it to the user's session, so two users on
    /// the same machine don't get in each other's way.
    const MUTEX_NAME: &str = "Local\\FactorioDiscordRichPresence";

    /// While it exists, this copy is the only one. It's released when dropped.
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

    /// Takes the single instance, or returns `None` if another copy is already running.
    pub fn acquire() -> Option<InstanceGuard> {
        acquire_named(MUTEX_NAME)
    }

    fn acquire_named(name: &str) -> Option<InstanceGuard> {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
        if handle.is_null() {
            // Without being able to create the mutex, there's no way to know if
            // another copy exists. Better to start twice than not start at all:
            // presence is what's being asked for.
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
        fn the_second_copy_cannot_acquire_it() {
            let name = unique("second");
            let first = acquire_named(&name);
            assert!(first.is_some());
            assert!(acquire_named(&name).is_none());
        }

        #[test]
        fn releasing_it_lets_another_copy_acquire_it() {
            let name = unique("release");
            let first = acquire_named(&name);
            assert!(first.is_some());
            drop(first);
            assert!(acquire_named(&name).is_some());
        }

        #[test]
        fn different_names_do_not_interfere_with_each_other() {
            let a = acquire_named(&unique("a"));
            let b = acquire_named(&unique("b"));
            assert!(a.is_some() && b.is_some());
        }
    }
}

/// On Unix there's no OS-level "named mutex" object: a `flock` (advisory
/// lock) is used instead, on a fixed file in the app's data directory. The
/// file stays behind after the process dies, but the lock releases on its
/// own: it's held by the open descriptor, not by the file itself.
#[cfg(unix)]
mod unix_impl {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::path::{Path, PathBuf};

    use fd_lock::{RwLock, RwLockWriteGuard};

    const LOCK_FILE_NAME: &str = "instance.lock";

    /// While it exists, this copy is the only one. It's released when dropped.
    ///
    /// The `RwLock<File>` lives on the heap and is deliberately leaked
    /// (`Box::leak`) so the write guard, which borrows from it, can be stored
    /// without a self-referential struct or `unsafe` code. The only cost is
    /// not freeing that allocation until the process exits, at which point
    /// the OS reclaims it anyway.
    ///
    /// `None` when the file could neither be opened nor locked for an
    /// ambiguous reason (permissions, etc.): just like a null mutex on
    /// Windows, it's let through instead of refusing to start over something
    /// that can't be interpreted.
    #[allow(
        dead_code,
        reason = "the field is only held for its Drop (releases the flock); never read"
    )]
    pub struct InstanceGuard(Option<RwLockWriteGuard<'static, File>>);

    /// Takes the single instance, or returns `None` if another copy is already running.
    pub fn acquire() -> Option<InstanceGuard> {
        let path = match lock_path() {
            Ok(path) => path,
            // Without a path there's no way to know if another copy exists.
            // Better to start twice than not start at all: presence is what's
            // being asked for.
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
            // Any other failure (permissions, a filesystem without locking...)
            // is as ambiguous as having no path: let through, same as above.
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
        fn the_second_copy_cannot_acquire_it() {
            let path = unique_path("second");
            let first = acquire_at(&path);
            assert!(first.is_some());
            assert!(acquire_at(&path).is_none());
            drop(first);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn releasing_it_lets_another_copy_acquire_it() {
            let path = unique_path("release");
            let first = acquire_at(&path);
            assert!(first.is_some());
            drop(first);
            assert!(acquire_at(&path).is_some());
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn different_paths_do_not_interfere_with_each_other() {
            let a = acquire_at(&unique_path("a"));
            let b = acquire_at(&unique_path("b"));
            assert!(a.is_some() && b.is_some());
            let _ = std::fs::remove_file(unique_path("a"));
            let _ = std::fs::remove_file(unique_path("b"));
        }
    }
}
