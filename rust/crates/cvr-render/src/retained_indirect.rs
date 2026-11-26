//! The retained geometry's indirect draws and barriers (src/Render/NativeRetainedGeometry.cpp, `ExecuteIndirect`'s
//! buffer and target checks, `BeforeBarriers`' seal and `Touches`): an indirect draw's argument buffer holds its count
//! of strided commands from the offset, and its count buffer (when there is one) a 4-byte count from its offset; every
//! target the draw writes (the depth, and each of its colours) is still the imported one -- the same resource and view
//! identity; and the first barrier that seals an eye -- any aliasing barrier, a transition of an imported target, or a
//! UAV barrier on one or on every resource -- with its exit kind (transition 2, aliasing 3, UAV 4).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_indirect_buffer`, `cvr_nrg_indirect_targets` and
//! `cvr_nrg_barrier_seal` (the targets as cvr-gpu's GeometryTargetsRaw; the device checks stay the .cpp's); the unit
//! tests below cover each.

use cvr_gpu::scene_state::GeometryTargetsRaw;
use windows::Win32::Graphics::Direct3D12::*;

/// A buffer holding `bytes` from `offset`.
pub fn buffer_holds(desc: &D3D12_RESOURCE_DESC, offset: u64, bytes: u64) -> bool {
    desc.Dimension == D3D12_RESOURCE_DIMENSION_BUFFER && offset <= desc.Width && bytes <= desc.Width - offset
}
/// The draw's targets are the imported ones.
pub fn targets_held(held: &GeometryTargetsRaw, imported: &[bool; 5], current: &GeometryTargetsRaw) -> bool {
    (0..5usize).all(|slot| {
        !(slot == 4 || (slot as u32) < current.colors)
            || (imported[slot] && held.resources[slot] == current.resources[slot] && held.views[slot].identity == current.views[slot].identity)
    })
}
/// Touches: an imported target is this resource.
pub fn touches(held: &[usize; 5], imported: &[bool; 5], resource: usize) -> bool {
    resource != 0 && (0..5).any(|slot| imported[slot] && held[slot] == resource)
}
/// One barrier as BeforeBarriers reads it: its type and its resource (the transition's or the UAV's).
#[derive(Clone, Copy, Debug)]
pub struct Barrier {
    pub kind: i32,
    pub resource: usize,
}
const TRANSITION: i32 = 0;
const ALIASING: i32 = 1;
const UAV: i32 = 2;
/// The first barrier that seals the eye and its exit kind.
pub fn barrier_seal(barriers: &[Barrier], held: &[usize; 5], imported: &[bool; 5]) -> Option<(usize, u32)> {
    barriers.iter().enumerate().find_map(|(i, b)| {
        let seals = b.kind == ALIASING
            || (b.kind == TRANSITION && touches(held, imported, b.resource))
            || (b.kind == UAV && (b.resource == 0 || touches(held, imported, b.resource)));
        seals.then_some((i, match b.kind {
            TRANSITION => 2,
            ALIASING => 3,
            _ => 4,
        }))
    })
}

// ---- the C ABI

/// # Safety
/// `desc` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_indirect_buffer(desc: *const D3D12_RESOURCE_DESC, offset: u64, bytes: u64) -> bool {
    buffer_holds(unsafe { &*desc }, offset, bytes)
}
/// # Safety
/// Both targets are valid GeometryTargets; `imported` holds five flags.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_indirect_targets(held: *const GeometryTargetsRaw, imported: *const [bool; 5], current: *const GeometryTargetsRaw) -> bool {
    unsafe { targets_held(&*held, &*imported, &*current) }
}
/// # Safety
/// `barriers` holds `count` barriers; `held` is a valid GeometryTargets; `imported` holds five flags; `kind` is
/// writable. Returns whether a barrier seals (its exit kind written).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_barrier_seal(
    barriers: *const D3D12_RESOURCE_BARRIER,
    count: u32,
    held: *const GeometryTargetsRaw,
    imported: *const [bool; 5],
    kind: *mut u32,
) -> bool {
    if barriers.is_null() || count == 0 {
        return false;
    }
    let raw = unsafe { core::slice::from_raw_parts(barriers, count as usize) };
    let resource = |r: &core::mem::ManuallyDrop<Option<ID3D12Resource>>| r.as_ref().map_or(0, |r| windows::core::Interface::as_raw(r) as usize);
    let list: Vec<Barrier> = raw
        .iter()
        .map(|b| {
            // SAFETY: the type selects the union member.
            let r = unsafe {
                match b.Type {
                    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => resource(&b.Anonymous.Transition.pResource),
                    D3D12_RESOURCE_BARRIER_TYPE_UAV => resource(&b.Anonymous.UAV.pResource),
                    _ => 0,
                }
            };
            Barrier { kind: b.Type.0, resource: r }
        })
        .collect();
    match barrier_seal(&list, unsafe { &(*held).resources }, unsafe { &*imported }) {
        Some((_, k)) => {
            unsafe { *kind = k };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn buffers() {
        let d = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_BUFFER, Width: 100, ..Default::default() };
        assert!(buffer_holds(&d, 20, 80));
        assert!(!buffer_holds(&d, 20, 81));
        assert!(!buffer_holds(&d, 101, 0));
        assert!(buffer_holds(&d, 100, 0));
        let t = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D, Width: 100, ..Default::default() };
        assert!(!buffer_holds(&t, 0, 4));
    }
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
    fn held() {
        let all = [true; 5];
        assert!(targets_held(&targets(3), &all, &targets(3)));
        // A written slot not imported, or another view, fails; an unwritten slot does not count.
        assert!(!targets_held(&targets(3), &[true, false, true, true, true], &targets(3)));
        assert!(targets_held(&targets(3), &[true, true, true, false, true], &targets(3)));
        assert!(!targets_held(&targets(3), &[true, true, true, true, false], &targets(0)));
        let mut c = targets(3);
        c.views[2].identity = 1;
        assert!(!targets_held(&targets(3), &all, &c));
        let mut c = targets(2);
        c.resources[2] = 1;
        assert!(targets_held(&targets(3), &all, &c));
    }
    #[test]
    fn barriers() {
        let held = [0x100, 0x101, 0x102, 0x103, 0x104];
        let imported = [true, true, false, true, true];
        assert!(touches(&held, &imported, 0x101));
        assert!(!touches(&held, &imported, 0x102));
        assert!(!touches(&held, &imported, 0));
        let b = |kind, resource| Barrier { kind, resource };
        assert_eq!(barrier_seal(&[b(TRANSITION, 0x500), b(TRANSITION, 0x104)], &held, &imported), Some((1, 2)));
        assert_eq!(barrier_seal(&[b(TRANSITION, 0x102), b(ALIASING, 0)], &held, &imported), Some((1, 3)));
        assert_eq!(barrier_seal(&[b(UAV, 0x500), b(UAV, 0)], &held, &imported), Some((1, 4)));
        assert_eq!(barrier_seal(&[b(UAV, 0x500), b(TRANSITION, 0x102), b(3, 0x100)], &held, &imported), None);
    }
}
