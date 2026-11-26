//! UTF-16 and ANSI conversions at the Win32 boundary.
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use windows_sys::Win32::Globalization::{WideCharToMultiByte, CP_ACP};

/// NUL-terminated UTF-16 for a W call.
pub fn z(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(core::iter::once(0)).collect()
}

/// A path from UTF-16 without its NUL.
pub fn path_from(units: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(units))
}

/// The path in the ANSI code page (`cvr::paths::Narrow`): what the engine's own narrow paths use.
pub fn ansi(path: &Path) -> Vec<u8> {
    let w: Vec<u16> = path.as_os_str().encode_wide().collect();
    if w.is_empty() {
        return Vec::new();
    }
    // SAFETY: sizes come from the first call; buffers are owned here.
    unsafe {
        let n = WideCharToMultiByte(CP_ACP, 0, w.as_ptr(), w.len() as i32, core::ptr::null_mut(), 0,
            core::ptr::null(), core::ptr::null_mut());
        let mut out = vec![0u8; n.max(0) as usize];
        WideCharToMultiByte(CP_ACP, 0, w.as_ptr(), w.len() as i32, out.as_mut_ptr(), n, core::ptr::null(),
            core::ptr::null_mut());
        out
    }
}
