//! The port's settings (stage 3, docs/rust-migration-plan.md): vrport.ini read line by line into the variables
//! src/Core/LiveControlsPoll.cpp applies. The file is read through the C runtime -- text mode, `fgets`, `sscanf_s` --
//! so a line means exactly what it meant to the C++ loop, quirks included (a key read as `%d` stops at its first
//! non-digit, a 200-byte line is two lines, Ctrl-Z ends the file).
pub use cvr_crt as crt;
pub mod fbt_settings;
pub mod fbt_font;
pub mod fbt_calibration_file;
pub mod first_launch;
pub mod game_settings;
pub mod hud_layout;
pub mod json;
pub mod launcher;
pub mod launcher_dialog;
pub mod launcher_presets;
pub mod xml_merge;
pub mod yaml;
pub mod archive_manifest;
pub mod live_controls;
pub mod live_controls_block;
pub mod live_controls_ui_state;
pub mod haptic_route;
pub mod wheel_range;

use core::ffi::{c_char, c_void};
pub use live_controls::LiveControlsValues;

/// The ini at `path` (the plugin's ANSI path) parsed into `v`, the subsystems' keys into `s` through their own C++
/// parsers; false when the file cannot be opened, and then nothing is changed.
///
/// # Safety
/// `path` is a NUL-terminated string, `v` a LiveControlsValues and `s` the LiveControlsSubsystems of
/// include/Core/LiveControlsParse.hpp.
#[no_mangle]
pub unsafe extern "C" fn cvr_settings_parse_live_controls(path: *const c_char, v: *mut LiveControlsValues, s: *mut c_void) -> bool {
    if path.is_null() || v.is_null() {
        return false;
    }
    // SAFETY: the caller's path, values and subsystems; the C runtime's file.
    unsafe {
        let Some(mut file) = crt::File::open_shared(path) else { return false };
        let mut line = [0 as c_char; 128];
        while file.gets(&mut line) {
            live_controls::parse_line(line.as_ptr(), &mut *v, s);
        }
    }
    true
}

/// The unit tests' stand-in for the C++ subsystems: none takes a line.
#[cfg(test)]
#[no_mangle]
extern "C" fn cvr_cpp_settings_parse_subsystem(_which: i32, _line: *const c_char, _s: *mut c_void) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_a_file() {
        let path = std::env::temp_dir().join(format!("cvr_settings_unit_{}.ini", std::process::id()));
        let mut text = b"\xEF\xBB\xBFxr_world_scale = 1.25\r\nxr_head_offset_x=0.5\nXR_NVIDIA_REFLEX = 2\n".to_vec();
        text.extend_from_slice(b"xr_scanner_frame=0.5,0.25,1\nxr_vrcam_cam_fields=ff\nxr_bd_push_transform=2\n");
        text.extend_from_slice(&[b'x'; 300]);
        text.extend_from_slice(b"\nxr_haptics=0\nxr_hidden\0x=1\n\x1Axr_world_scale=9\n");
        std::fs::write(&path, &text).unwrap();
        let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
        let mut v: LiveControlsValues = unsafe { core::mem::zeroed() };
        assert!(unsafe { cvr_settings_parse_live_controls(c.as_ptr(), &mut v, core::ptr::null_mut()) });
        assert_eq!(v.xrWorldScale, 0.0, "the BOM keeps the first line from matching, and nothing after Ctrl-Z is read");
        assert_eq!(v.xrHeadOffsetX, 0.5);
        assert_eq!(v.nvidiaReflex, 2);
        assert_eq!(v.xrScannerSlots[..3], [0.5, 0.25, 1.0]);
        assert_eq!(v.xrVrcamCamFields, 0xF);
        assert_eq!(v.xrBdPushTransform, 2);
        let _ = std::fs::remove_file(&path);
        // A file that cannot be opened changes nothing (and closes nothing: fclose(NULL) is a fail-fast).
        let before = v;
        assert!(!unsafe { cvr_settings_parse_live_controls(c.as_ptr(), &mut v, core::ptr::null_mut()) });
        assert_eq!(unsafe { core::slice::from_raw_parts(&raw const v as *const u8, core::mem::size_of_val(&v)) },
                   unsafe { core::slice::from_raw_parts(&raw const before as *const u8, core::mem::size_of_val(&before)) });
    }
}

