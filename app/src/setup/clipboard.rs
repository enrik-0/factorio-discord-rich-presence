//! Copiar texto al portapapeles de Windows.
//!
//! Se usa la API directamente, en UTF-16, en vez de `clip.exe`: éste lee la
//! entrada en la página de códigos de la consola y estropea acentos y eñes de
//! rutas como `C:\Users\Ñandú\…`.

use std::time::Duration;

use anyhow::{bail, Result};
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// Formato de texto Unicode del portapapeles.
const CF_UNICODETEXT: u32 = 13;

/// Otro programa puede tener el portapapeles abierto un instante.
const OPEN_ATTEMPTS: u32 = 10;

pub fn copy(text: &str) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            bail!("no se pudo reservar memoria para el portapapeles");
        }

        let dest = GlobalLock(handle).cast::<u16>();
        if dest.is_null() {
            GlobalFree(handle);
            bail!("no se pudo bloquear la memoria del portapapeles");
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), dest, wide.len());
        GlobalUnlock(handle);

        let mut opened = false;
        for _ in 0..OPEN_ATTEMPTS {
            if OpenClipboard(std::ptr::null_mut()) != 0 {
                opened = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        if !opened {
            GlobalFree(handle);
            bail!("el portapapeles está ocupado por otro programa");
        }

        EmptyClipboard();
        let accepted = SetClipboardData(CF_UNICODETEXT, handle);
        CloseClipboard();

        if accepted.is_null() {
            // Si el portapapeles lo rechaza, la memoria sigue siendo nuestra.
            GlobalFree(handle);
            bail!("el portapapeles rechazó el texto");
        }
        // Aceptado: la memoria pasa a ser suya y no se libera aquí.
    }

    Ok(())
}
