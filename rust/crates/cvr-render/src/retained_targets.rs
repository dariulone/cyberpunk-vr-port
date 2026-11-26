//! The retained geometry's target continuity (src/Render/NativeRetainedGeometry.cpp, `Draw`): once an eye has started,
//! a draw whose targets differ from the imported ones -- any imported slot among the draw's colors or the depth (4)
//! with another resource or view identity -- seals the eye.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the check forwards to `cvr_nrg_targets_changed` (the targets as cvr-gpu's
//! GeometryTargetsRaw); the unit tests below cover each field.

use cvr_gpu::scene_state::GeometryTargetsRaw;

/// Whether the draw's targets left the imported ones.
pub fn targets_changed(held: &GeometryTargetsRaw, imported: &[bool; 5], current: &GeometryTargetsRaw) -> bool {
    (0..5usize).any(|slot| {
        (slot == 4 || (slot as u32) < current.colors)
            && imported[slot]
            && (held.resources[slot] != current.resources[slot] || held.views[slot].identity != current.views[slot].identity)
    })
}

// ---- the C ABI

/// # Safety
/// Both targets are valid GeometryTargets; `imported` holds five flags.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_targets_changed(held: *const GeometryTargetsRaw, imported: *const [bool; 5], current: *const GeometryTargetsRaw) -> bool {
    unsafe { targets_changed(&*held, &*imported, &*current) }
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;

    fn targets(colors: u32) -> GeometryTargetsRaw {
        let mut t: GeometryTargetsRaw = unsafe { core::mem::zeroed() };
        t.colors = colors;
        for slot in 0..5 {
            t.resources[slot] = 0x100 + slot;
            t.views[slot].identity = 0x200 + slot as u64;
        }
        t
    }
    #[test]
    fn changes() {
        let held = targets(3);
        let all = [true; 5];
        assert!(!targets_changed(&held, &all, &targets(3)));
        let mut c = targets(3);
        c.resources[2] = 9;
        assert!(targets_changed(&held, &all, &c));
        let mut c = targets(3);
        c.views[4].identity = 9;
        assert!(targets_changed(&held, &all, &c));
        // A color slot past the draw's colors, or not imported, does not count.
        let mut c = targets(2);
        c.resources[2] = 9;
        assert!(!targets_changed(&held, &all, &c));
        let mut c = targets(3);
        c.resources[1] = 9;
        assert!(!targets_changed(&held, &[true, false, true, true, true], &c));
        // The depth counts whatever the colors.
        let mut c = targets(0);
        c.resources[4] = 9;
        assert!(targets_changed(&held, &all, &c));
    }
}
