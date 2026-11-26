//! The headset's measured default FOV (src/Runtimes/OpenXRManager.cpp, `CyberpunkVR_HeadsetDefaultFovDeg` and
//! `ContainsFragmentNoCase`): a runtime system name containing a known headset's fragment (ASCII case aside) gives
//! that headset's measured horizontal FOV -- the Pico 4's 104 degrees -- and any other name 0 (the runtime's own FOV).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the lookup forwards to `cvr_xr_headset_fov_default` (the once-only log line
//! stays the .cpp's); the unit tests below cover the matching.

use core::ffi::{c_char, CStr};

/// The measured defaults: (name fragment, horizontal FOV in degrees).
pub const DEFAULTS: [(&[u8], f32); 1] = [(b"pico 4", 104.0)];

/// ContainsFragmentNoCase: `needle` (non-empty) occurs in `haystack`, ASCII case aside.
pub fn contains_no_case(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.len() >= needle.len() && haystack.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}
/// The default for a system name, or 0.
pub fn default_fov(name: &[u8]) -> f32 {
    DEFAULTS.iter().find(|(fragment, _)| contains_no_case(name, fragment)).map_or(0.0, |&(_, degrees)| degrees)
}

// ---- the C ABI

/// # Safety
/// `name` is a NUL-terminated string (or null).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_xr_headset_fov_default(name: *const c_char) -> f32 {
    if name.is_null() {
        return 0.0;
    }
    default_fov(unsafe { CStr::from_ptr(name) }.to_bytes())
}

#[cfg(test)]
mod tests {
    //! The matching.
    use super::*;

    #[test]
    fn matches() {
        assert_eq!(default_fov(b"PICO 4 Ultra"), 104.0);
        assert_eq!(default_fov(b"Pico 4"), 104.0);
        assert_eq!(default_fov(b"my pIcO 4"), 104.0);
        assert_eq!(default_fov(b"Pico 3"), 0.0);
        assert_eq!(default_fov(b"pico"), 0.0);
        assert_eq!(default_fov(b""), 0.0);
        assert!(!contains_no_case(b"abc", b""));
        assert!(contains_no_case(b"ABC", b"bc"));
        assert_eq!(unsafe { cvr_xr_headset_fov_default(core::ptr::null()) }, 0.0);
        assert_eq!(unsafe { cvr_xr_headset_fov_default(c"Oculus Quest 3".as_ptr()) }, 0.0);
    }
}
