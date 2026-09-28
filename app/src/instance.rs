//! Single instance per Windows session.
//!
//! With autostart and the Steam launch line both active, every Factorio
//! startup would launch another copy of the application, all publishing to
//! the same Discord card. A named mutex only lets the first one through.

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Mutex name. `Local\` scopes it to the user's session, so two users on the
/// same machine don't get in each other's way.
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
        let name = unique("segunda");
        let first = acquire_named(&name);
        assert!(first.is_some());
        assert!(acquire_named(&name).is_none());
    }

    #[test]
    fn releasing_it_lets_another_copy_acquire_it() {
        let name = unique("soltar");
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
