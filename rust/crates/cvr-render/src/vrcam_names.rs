//! Two small lookups of the VRCAM and headset setup: the braindance camera's name hash for the selected VRCAM component
//! (src/Stereo/SyncStereo.cpp, `CyberpunkVR_VrcamBdCamNameHash`: `vrcam_<W>x<H>` -> the CName hash of
//! `vrcam_braindance_<W>x<H>`, 0 when nothing is selected, the name is not a VRCAM component's or the braindance name
//! would not fit the .cpp's 96-byte buffer); and the headset's measured default horizontal field of view
//! (src/Runtimes/OpenXRManager.cpp, `CyberpunkVR_HeadsetDefaultFovDeg`: cvr-camera's headset table, the first use
//! logged once); and the resolution the launcher's pick resolves to (src/Stereo/SyncStereo.cpp,
//! `CyberpunkVR_GetSelectedResolution`: the selection's width and height, 0 while either is unknown).
//!
//! The .cpp's exported functions forward to `cvr_ss_bd_cam_name_hash`, `cvr_xr_headset_default_fov` and
//! `cvr_ss_selected_resolution`; the unit tests below check the name, the log line and the resolution.

/// The braindance camera's name for a selected VRCAM component.
pub fn braindance_name(selected: &[u8]) -> Option<Vec<u8>> {
    let rest = selected.strip_prefix(b"vrcam_")?;
    let name = [&b"vrcam_braindance_"[..], rest].concat();
    (!name.is_empty() && name.len() < 96).then_some(name)
}
/// The selected resolution: both sides known, or nothing.
pub fn selected_resolution(width: u32, height: u32) -> Option<(u32, u32)> {
    (width != 0 && height != 0).then_some((width, height))
}
/// The first use's log line.
pub fn fov_line(name: &str, degrees: f32) -> String {
    format!(
        "OpenXRManager[FOV]: headset \"{name}\" has a measured default horizontal FOV of {:.1} deg; using it because vrport.ini leaves xr_force_fov at 0. Set xr_force_fov to override.\n",
        f64::from(degrees)
    )
}

#[cfg(feature = "rust-build")]
mod abi {
    use super::*;
    use core::ffi::{c_char, CStr};
    use core::sync::atomic::{AtomicBool, Ordering::Relaxed};

    extern "C" {
        /// src/Stereo/VrcamSelection.cpp: the selected VRCAM component, "vrcam_<W>x<H>".
        static g_vrcam_component: [c_char; 96];
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_ss_bd_cam_name_hash() -> u64 {
        // SAFETY: the C++ half's NUL-terminated buffer, read by value.
        let buffer = unsafe { core::ptr::read_volatile(&raw const g_vrcam_component) };
        let bytes: Vec<u8> = buffer.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        braindance_name(&bytes).map_or(0, |name| crate::vrcam_config::cname_hash(&name))
    }
    extern "C" {
        /// src/Stereo/SyncStereo.cpp: the launcher pick's resolution, parsed from the component name.
        static g_vrcam_sel_w: core::sync::atomic::AtomicU32;
        static g_vrcam_sel_h: core::sync::atomic::AtomicU32;
    }
    /// # Safety
    /// `width` and `height` are null or writable.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_ss_selected_resolution(width: *mut u32, height: *mut u32) -> i32 {
        // SAFETY: the C++ half's atomics.
        let (w, h) = unsafe { (g_vrcam_sel_w.load(Relaxed), g_vrcam_sel_h.load(Relaxed)) };
        let Some((w, h)) = selected_resolution(w, h) else { return 0 };
        // SAFETY: as the caller vouches.
        unsafe {
            if let Some(out) = width.as_mut() {
                *out = w;
            }
            if let Some(out) = height.as_mut() {
                *out = h;
            }
        }
        1
    }
    static SAID: AtomicBool = AtomicBool::new(false);
    /// # Safety
    /// `name` is null or a NUL-terminated string.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_xr_headset_default_fov(name: *const c_char) -> f32 {
        if name.is_null() {
            return 0.0;
        }
        // SAFETY: as the caller vouches.
        let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
        if bytes.is_empty() {
            return 0.0;
        }
        let degrees = cvr_camera::headset_fov::default_fov(bytes);
        if degrees > 0.0 && !SAID.swap(true, Relaxed) {
            cvr_host::log::write(&fov_line(&String::from_utf8_lossy(bytes), degrees));
        }
        degrees
    }
}

#[cfg(test)]
mod tests {
    //! The name, the log line and the resolution.
    use super::*;

    #[test]
    fn names() {
        assert_eq!(braindance_name(b"vrcam_2444x2444").as_deref(), Some(&b"vrcam_braindance_2444x2444"[..]));
        assert_eq!(braindance_name(b"camera_2444x2444"), None);
        assert_eq!(braindance_name(b""), None);
        // 17 + 78 = 95 bytes fit the buffer with its NUL; 96 do not.
        let fits = [&b"vrcam_"[..], &[b'x'; 78]].concat();
        assert_eq!(braindance_name(&fits).map(|n| n.len()), Some(95));
        let long = [&b"vrcam_"[..], &[b'x'; 79]].concat();
        assert_eq!(braindance_name(&long), None);
    }
    #[test]
    fn resolution() {
        assert_eq!(selected_resolution(2444, 2444), Some((2444, 2444)));
        assert_eq!(selected_resolution(0, 2444), None);
        assert_eq!(selected_resolution(2444, 0), None);
    }
    #[test]
    fn lines() {
        assert_eq!(
            fov_line("Quest 3", 104.0),
            "OpenXRManager[FOV]: headset \"Quest 3\" has a measured default horizontal FOV of 104.0 deg; using it because vrport.ini leaves xr_force_fov at 0. Set xr_force_fov to override.\n"
        );
    }
}
