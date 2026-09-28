//! Copying text to the Windows clipboard.
//!
//! The API is used directly, in UTF-16, instead of `clip.exe`: the latter
//! reads input in the console's code page and mangles accented characters
//! and ñ's in paths like `C:\Users\Ñandú\…`.

use std::time::Duration;

use anyhow::{bail, Result};
use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// Clipboard Unicode text format.
const CF_UNICODETEXT: u32 = 13;

/// Another program may have the clipboard open for a moment.
const OPEN_ATTEMPTS: u32 = 10;

pub fn copy(text: &str) -> Result<()> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = wide.len() * std::mem::size_of::<u16>();

    unsafe {
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            bail!("could not allocate memory for the clipboard");
        }

        let dest = GlobalLock(handle).cast::<u16>();
        if dest.is_null() {
            GlobalFree(handle);
            bail!("could not lock the clipboard memory");
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
            bail!("the clipboard is busy with another program");
        }

        EmptyClipboard();
        let accepted = SetClipboardData(CF_UNICODETEXT, handle);
        CloseClipboard();

        if accepted.is_null() {
            // If the clipboard rejects it, the memory is still ours.
            GlobalFree(handle);
            bail!("the clipboard rejected the text");
        }
        // Accepted: the memory now belongs to it and isn't freed here.
    }

    Ok(())
}
