//! The log and the plugin's folders, for the C++ half: `Log()` (src/Core/Log.cpp) formats and hands the text here,
//! `cvr::paths` (src/Core/Paths.cpp) reads the folders here. One log file, one set of folders, owned by Rust.
use cvr_host::paths::{self, Folder};
use cvr_host::wide::ansi;
use std::ffi::{c_char, CString};
use std::os::windows::ffi::OsStrExt;
use std::sync::OnceLock;

const FOLDERS: [Folder; 9] = [
    Folder::Root,
    Folder::Shaders,
    Folder::Grips,
    Folder::Archive,
    Folder::Config,
    Folder::Log,
    Folder::Scripts,
    Folder::Redscript,
    Folder::Lua,
];

/// Writes `len` bytes of text to the plugin's log (lines end with '\n' in the text itself).
///
/// # Safety
/// `text` must point to `len` readable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_log_write(text: *const c_char, len: usize) {
    if text.is_null() {
        return;
    }
    // SAFETY: the caller's buffer.
    let bytes = unsafe { core::slice::from_raw_parts(text.cast::<u8>(), len) };
    cvr_host::log::write(&String::from_utf8_lossy(bytes));
}

#[no_mangle]
pub extern "C" fn cvr_log_flush() {
    cvr_host::log::flush();
}

fn wide_dirs() -> &'static [Vec<u16>; 9] {
    static DIRS: OnceLock<[Vec<u16>; 9]> = OnceLock::new();
    DIRS.get_or_init(|| {
        core::array::from_fn(|i| {
            let dir = paths::dir(FOLDERS[i]);
            let mut w: Vec<u16> = dir.as_os_str().encode_wide().collect();
            if !w.is_empty() && w.last() != Some(&(b'\\' as u16)) {
                w.push(b'\\' as u16);
            }
            w.push(0);
            w
        })
    })
}

fn ansi_dirs() -> &'static [CString; 9] {
    static DIRS: OnceLock<[CString; 9]> = OnceLock::new();
    DIRS.get_or_init(|| {
        core::array::from_fn(|i| {
            let mut a = ansi(paths::dir(FOLDERS[i]));
            if !a.is_empty() && a.last() != Some(&b'\\') {
                a.push(b'\\');
            }
            a.retain(|&b| b != 0);
            CString::new(a).unwrap_or_default()
        })
    })
}

/// The folder (`cvr::paths::Folder` order) with a trailing backslash, NUL-terminated UTF-16, valid for the process.
/// An unknown index is the root.
#[no_mangle]
pub extern "C" fn cvr_paths_dir_w(folder: u32) -> *const u16 {
    wide_dirs()[(folder as usize).min(8)].as_ptr()
}

/// The same in the ANSI code page.
#[no_mangle]
pub extern "C" fn cvr_paths_dir_a(folder: u32) -> *const c_char {
    ansi_dirs()[(folder as usize).min(8)].as_ptr()
}
