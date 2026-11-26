//! The native early depth's work settings (src/Render/NativeEarlyDepth.cpp, `early_depth::Begin`): the alpha, skinned
//! alpha and material capture modes a work may combine with its target mode, and the material hints it takes (at most
//! 128 pairs of a native material handle and the alpha texture's descriptor index).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Begin's checks forward to `cvr_ned_settings` and `cvr_ned_hints`; the unit tests
//! below cover each refusal.

/// Begin's work settings: 0, or the failure -- 55 an alpha reference outside a reference whole-target run with the
/// native camera or an optimized alpha outside an optimized one (or with a reference), 59 a skinned alpha outside
/// skinning with an alpha mode, 53 a material capture outside reference mode.
#[allow(clippy::too_many_arguments)]
pub fn settings(mode: u32, running: bool, native_camera: bool, skinning: bool, alpha_reference: bool, alpha_optimized: bool, skinned_alpha: bool, material_capture: bool) -> u32 {
    if alpha_reference && (running || mode != 1 || !native_camera) {
        return 55;
    }
    if alpha_optimized && (running || mode != 2 || !native_camera || alpha_reference) {
        return 55;
    }
    if skinned_alpha && (running || !skinning || (!alpha_reference && !alpha_optimized)) {
        return 59;
    }
    if material_capture && mode != 1 {
        return 53;
    }
    0
}
/// Begin's material hints: at most 128 (handle, alpha texture index) pairs, each handle non-zero, at most 0x100000 and
/// unique, each index below 32768; None for a refused list.
pub fn hints(raw: &[[u32; 2]]) -> Option<Vec<[u32; 2]>> {
    if raw.len() > 128 {
        return None;
    }
    let mut out: Vec<[u32; 2]> = Vec::with_capacity(raw.len());
    for hint in raw {
        if hint[0] == 0 || hint[0] > 0x100000 || hint[1] >= 32768 || out.iter().any(|h| h[0] == hint[0]) {
            return None;
        }
        out.push(*hint);
    }
    Some(out)
}

// ---- the C ABI

#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C-unwind" fn cvr_ned_settings(mode: u32, running: bool, native_camera: bool, skinning: bool, alpha_reference: bool, alpha_optimized: bool, skinned_alpha: bool, material_capture: bool) -> u32 {
    settings(mode, running, native_camera, skinning, alpha_reference, alpha_optimized, skinned_alpha, material_capture)
}
/// The hints of `count` raw pairs (a count past 128 is refused before reading) into `out` (room for 128 pairs): their
/// count, or usize::MAX when refused.
///
/// # Safety
/// `raw` holds 128 pairs; `out` has room for 128.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_hints(raw: *const [u32; 2], count: u32, out: *mut [u32; 2]) -> usize {
    if count > 128 {
        return usize::MAX;
    }
    let raw = unsafe { core::slice::from_raw_parts(raw, count as usize) };
    match hints(raw) {
        Some(found) => {
            unsafe { core::ptr::copy_nonoverlapping(found.as_ptr(), out, found.len()) };
            found.len()
        }
        None => usize::MAX,
    }
}

#[cfg(test)]
mod tests {
    //! Each settings refusal and each hint refusal.
    use super::*;

    #[test]
    fn settings_refusals() {
        assert_eq!(settings(1, false, true, false, true, false, false, true), 0);
        assert_eq!(settings(2, false, true, false, true, false, false, false), 55);
        assert_eq!(settings(1, false, false, false, true, false, false, false), 55);
        assert_eq!(settings(1, true, true, false, true, false, false, false), 55);
        assert_eq!(settings(2, false, true, false, false, true, false, false), 0);
        assert_eq!(settings(2, false, true, false, true, true, false, false), 55);
        assert_eq!(settings(2, false, true, true, false, true, true, false), 0);
        assert_eq!(settings(2, false, true, false, false, true, true, false), 59);
        assert_eq!(settings(0, false, true, true, false, false, true, false), 59);
        assert_eq!(settings(2, false, false, false, false, false, false, true), 53);
    }
    #[test]
    fn hint_refusals() {
        assert_eq!(hints(&[[5, 7], [6, 32767]]), Some(vec![[5, 7], [6, 32767]]));
        assert_eq!(hints(&[[0, 1]]), None);
        assert_eq!(hints(&[[0x100001, 1]]), None);
        assert_eq!(hints(&[[5, 32768]]), None);
        assert_eq!(hints(&[[5, 1], [5, 2]]), None);
        assert_eq!(hints(&vec![[1, 1]; 129]), None);
        let raw = [[9u32, 3]; 128];
        let mut out = [[0u32; 2]; 128];
        assert_eq!(unsafe { cvr_ned_hints(raw.as_ptr(), 129, out.as_mut_ptr()) }, usize::MAX);
        assert_eq!(unsafe { cvr_ned_hints(raw.as_ptr(), 1, out.as_mut_ptr()) }, 1);
        assert_eq!(out[0], [9, 3]);
    }
}
