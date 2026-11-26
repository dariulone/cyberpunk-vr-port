//! The native early depth's draw admission (src/Render/NativeEarlyDepth.cpp, `early_depth::Draw`): a draw is a candidate
//! only without render targets, with every required flag (bits 0-5 and 7) and not bit 6, with an instance stream in
//! slot 7 and no slot past 15; a GPU-identity draw's 48-byte instance window (from its first instance, inside the stream
//! and the address space); and the alpha modes a family takes (an alpha-tested family, or a skinned alpha one when
//! skinned alpha and skinning are on; the reference in reference mode, the optimized path in optimized mode, each with
//! the family's native camera pipeline of the vertex stage and the work's native camera).
//!
//! In the Rust build (`CVR_RUST_VRIK`) Draw's checks forward to `cvr_ned_draw_admitted`, `cvr_ned_identity_window` and
//! `cvr_ned_alpha_modes`; the unit tests below cover them.

use crate::geometry_commands::DrawRecord;

/// The draw's shape admitted.
pub fn draw_admitted(d: &DrawRecord) -> bool {
    d.rt_count == 0 && d.flags & 191 == 191 && d.flags & 64 == 0 && d.input_slot_mask & 128 != 0 && d.input_slot_mask & !65535 == 0
}
/// The GPU-identity draw's instance window's address.
pub fn identity_window(d: &DrawRecord) -> Option<u64> {
    let stream = &d.vertices[7];
    let offset = d.first_instance as u64 * 48;
    let size = stream.SizeInBytes as u64;
    if offset > size || d.instances as u64 * 48 > size - offset || stream.BufferLocation > u64::MAX - offset {
        return None;
    }
    Some(stream.BufferLocation + offset)
}
/// The family's alpha modes (reference, optimized).
#[allow(clippy::too_many_arguments)]
pub fn alpha_modes(
    alpha_contract: u32,
    skinning_contract: u32,
    camera_pipeline: bool,
    camera_stages: u32,
    mode: u32,
    alpha_reference: bool,
    alpha_optimized: bool,
    skinned_alpha: bool,
    skinning: bool,
    native_camera: bool,
) -> (bool, bool) {
    let family = alpha_contract == 1 || (skinned_alpha && alpha_contract == 2 && skinning_contract == 2 && skinning);
    let camera = family && camera_pipeline && camera_stages == 1 && native_camera;
    (alpha_reference && mode == 1 && camera, alpha_optimized && mode == 2 && camera)
}

// ---- the C ABI

/// # Safety
/// `d` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_draw_admitted(d: *const DrawRecord) -> bool {
    draw_admitted(unsafe { &*d })
}
/// # Safety
/// `d` is valid; `address` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ned_identity_window(d: *const DrawRecord, address: *mut u64) -> bool {
    match identity_window(unsafe { &*d }) {
        Some(at) => {
            unsafe { *address = at };
            true
        }
        None => false,
    }
}
/// The alpha modes: bit 0 the reference, bit 1 the optimized path.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "C-unwind" fn cvr_ned_alpha_modes(
    alpha_contract: u32,
    skinning_contract: u32,
    camera_pipeline: bool,
    camera_stages: u32,
    mode: u32,
    alpha_reference: bool,
    alpha_optimized: bool,
    skinned_alpha: bool,
    skinning: bool,
    native_camera: bool,
) -> u32 {
    let (reference, optimized) =
        alpha_modes(alpha_contract, skinning_contract, camera_pipeline, camera_stages, mode, alpha_reference, alpha_optimized, skinned_alpha, skinning, native_camera);
    reference as u32 | (optimized as u32) << 1
}

#[cfg(test)]
mod tests {
    //! The admission's flags and slots, the identity window's bounds and the alpha modes.
    use super::*;

    fn draw() -> DrawRecord {
        // SAFETY: a plain record, all zero.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.flags = 191;
        d.input_slot_mask = 1 | 128;
        d
    }

    #[test]
    fn admission() {
        let d = draw();
        assert!(draw_admitted(&d));
        assert!(!draw_admitted(&DrawRecord { rt_count: 1, ..d }));
        assert!(!draw_admitted(&DrawRecord { flags: 191 | 64, ..d }));
        assert!(!draw_admitted(&DrawRecord { flags: 190, ..d }));
        assert!(!draw_admitted(&DrawRecord { input_slot_mask: 1, ..d }));
        assert!(!draw_admitted(&DrawRecord { input_slot_mask: 128 | 65536, ..d }));
    }
    #[test]
    fn identity_windows() {
        let mut d = draw();
        d.vertices[7].BufferLocation = 0x10000;
        d.vertices[7].SizeInBytes = 48 * 10;
        d.first_instance = 2;
        d.instances = 8;
        assert_eq!(identity_window(&d), Some(0x10000 + 96));
        d.instances = 9;
        assert_eq!(identity_window(&d), None);
        d.first_instance = 11;
        d.instances = 0;
        assert_eq!(identity_window(&d), None);
        d.first_instance = 0;
        d.vertices[7].BufferLocation = u64::MAX;
        assert_eq!(identity_window(&d), Some(u64::MAX));
        d.first_instance = 1;
        assert_eq!(identity_window(&d), None);
    }
    #[test]
    fn alpha_mode_rules() {
        assert_eq!(alpha_modes(1, 0, true, 1, 1, true, false, false, false, true), (true, false));
        assert_eq!(alpha_modes(1, 0, true, 1, 2, false, true, false, false, true), (false, true));
        assert_eq!(alpha_modes(1, 0, true, 3, 1, true, false, false, false, true), (false, false));
        assert_eq!(alpha_modes(1, 0, false, 1, 1, true, false, false, false, true), (false, false));
        assert_eq!(alpha_modes(1, 0, true, 1, 1, true, false, false, false, false), (false, false));
        assert_eq!(alpha_modes(2, 2, true, 1, 2, false, true, true, true, true), (false, true));
        assert_eq!(alpha_modes(2, 2, true, 1, 2, false, true, true, false, true), (false, false));
        assert_eq!(alpha_modes(2, 1, true, 1, 2, false, true, true, true, true), (false, false));
    }
}
