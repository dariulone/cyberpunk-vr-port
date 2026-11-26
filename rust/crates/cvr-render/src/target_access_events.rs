//! The target access journal's events (src/Render/NativeTargetAccess.cpp, `IsMarker`, `Barriers`, `MarkSource` and
//! `DiagnosticProofScope`): the marker kinds (shader, unknown, source, exit and the proof brackets); a barrier's event
//! (a transition with its subresource and both states, an aliasing barrier by its before- and after-resource, a UAV
//! barrier by its resource, anything else unknown -- its flags kept, the resources to hold returned in order); a source mark's arguments (side 0
//! or 1, a frame, the lease's token, list and generation, no source already on the recording); and a proof scope's
//! four distinct held resources.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_tae_marker`, `cvr_tae_barrier`, `cvr_tae_source_arguments`
//! and `cvr_tae_proof_resources` (target_access.rs's Event mirrored; holding and appending stay the .cpp's); the unit
//! tests below cover each.

use crate::target_access::{kind, Event};
use core::ffi::c_void;
use windows::Win32::Graphics::Direct3D12::*;

/// IsMarker.
pub fn is_marker(k: u32) -> bool {
    matches!(k, kind::SHADER | kind::UNKNOWN | kind::SOURCE | kind::EXIT | kind::PROOF_BEGIN | kind::PROOF_END)
}

/// One barrier as Barriers reads it.
#[derive(Clone, Copy, Debug)]
pub enum Barrier {
    Transition { resource: *mut c_void, subresource: u32, before: u32, after: u32 },
    Aliasing { before: *mut c_void, after: *mut c_void },
    Uav { resource: *mut c_void },
    Other,
}
/// The barrier's event (on the .cpp's default) and the resources to hold, in order: the event's resource, then its other.
pub fn barrier_event(barrier: Barrier, flags: u32, event: &mut Event) -> Vec<*mut c_void> {
    event.flags = flags;
    match barrier {
        Barrier::Transition { resource, subresource, before, after } => {
            event.kind = kind::TRANSITION;
            event.subresource = subresource;
            event.before = before;
            event.after = after;
            vec![resource]
        }
        Barrier::Aliasing { before, after } => {
            event.kind = kind::ALIAS;
            vec![before, after]
        }
        Barrier::Uav { resource } => {
            event.kind = kind::UAV;
            vec![resource]
        }
        Barrier::Other => {
            event.kind = kind::UNKNOWN;
            Vec::new()
        }
    }
}
/// MarkSource's arguments.
pub fn source_arguments(side: u32, frame: u32, token: u64, lease_list: u64, list: u64, generation: u64, held_token: u64) -> bool {
    side <= 1 && frame != 0 && token != 0 && lease_list == list && generation != 0 && held_token == 0
}
/// DiagnosticProofScope's resources: exactly four distinct non-null ones.
pub fn proof_resources(held: &[*mut c_void]) -> Option<[*mut c_void; 4]> {
    let mut unique = [core::ptr::null_mut(); 4];
    let mut count = 0;
    for &resource in held.iter().filter(|r| !r.is_null()) {
        if count == unique.len() || unique[..count].contains(&resource) {
            return None;
        }
        unique[count] = resource;
        count += 1;
    }
    (count == 4).then_some(unique)
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_tae_marker(k: u32) -> bool {
    is_marker(k)
}
/// # Safety
/// `barrier` and `event` are valid; `held` holds two pointers. Returns how many of them to hold.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tae_barrier(barrier: *const D3D12_RESOURCE_BARRIER, event: *mut Event, held: *mut [*mut c_void; 2]) -> u32 {
    let b = unsafe { &*barrier };
    let raw = |r: &core::mem::ManuallyDrop<Option<ID3D12Resource>>| r.as_ref().map_or(core::ptr::null_mut(), |r| windows::core::Interface::as_raw(r));
    // SAFETY: the type selects the union member.
    let barrier = unsafe {
        match b.Type {
            D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => {
                let t = &b.Anonymous.Transition;
                Barrier::Transition { resource: raw(&t.pResource), subresource: t.Subresource, before: t.StateBefore.0 as u32, after: t.StateAfter.0 as u32 }
            }
            D3D12_RESOURCE_BARRIER_TYPE_ALIASING => {
                let a = &b.Anonymous.Aliasing;
                Barrier::Aliasing { before: raw(&a.pResourceBefore), after: raw(&a.pResourceAfter) }
            }
            D3D12_RESOURCE_BARRIER_TYPE_UAV => Barrier::Uav { resource: raw(&b.Anonymous.UAV.pResource) },
            _ => Barrier::Other,
        }
    };
    let holds = barrier_event(barrier, b.Flags.0 as u32, unsafe { &mut *event });
    let mut out = [core::ptr::null_mut(); 2];
    out[..holds.len()].copy_from_slice(&holds);
    unsafe { *held = out };
    holds.len() as u32
}
#[no_mangle]
pub extern "C-unwind" fn cvr_tae_source_arguments(side: u32, frame: u32, token: u64, lease_list: u64, list: u64, generation: u64, held_token: u64) -> bool {
    source_arguments(side, frame, token, lease_list, list, generation, held_token)
}
/// # Safety
/// `held` holds `count` pointers (or is null with 0); `unique` holds four.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_tae_proof_resources(held: *const *mut c_void, count: usize, unique: *mut [*mut c_void; 4]) -> bool {
    let held = if held.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(held, count) } };
    match proof_resources(held) {
        Some(found) => {
            unsafe { *unique = found };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    fn event() -> Event {
        Event { kind: kind::READ, resource: u32::MAX, other: u32::MAX, subresource: u32::MAX, before: 0, after: 0, flags: 0, proof: 0, tag: 0 }
    }
    fn p(v: usize) -> *mut c_void {
        v as *mut c_void
    }
    #[test]
    fn markers() {
        let marked: Vec<u32> = (0..11).filter(|&k| is_marker(k)).collect();
        assert_eq!(marked, [kind::SHADER, kind::UNKNOWN, kind::SOURCE, kind::EXIT, kind::PROOF_BEGIN, kind::PROOF_END]);
    }
    #[test]
    fn barriers() {
        let mut e = event();
        assert_eq!(barrier_event(Barrier::Transition { resource: p(8), subresource: 3, before: 4, after: 0x800 }, 1, &mut e), [p(8)]);
        assert_eq!((e.kind, e.subresource, e.before, e.after, e.flags, e.resource), (kind::TRANSITION, 3, 4, 0x800, 1, u32::MAX));
        let mut e = event();
        assert_eq!(barrier_event(Barrier::Aliasing { before: p(16), after: p(32) }, 0, &mut e), [p(16), p(32)]);
        assert_eq!((e.kind, e.subresource), (kind::ALIAS, u32::MAX));
        let mut e = event();
        assert_eq!(barrier_event(Barrier::Uav { resource: p(24) }, 2, &mut e), [p(24)]);
        assert_eq!((e.kind, e.flags), (kind::UAV, 2));
        let mut e = event();
        assert!(barrier_event(Barrier::Other, 0, &mut e).is_empty());
        assert_eq!(e.kind, kind::UNKNOWN);
    }
    #[test]
    fn abi() {
        let mut b = D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, Flags: D3D12_RESOURCE_BARRIER_FLAGS(2), ..Default::default() };
        b.Anonymous.Transition = core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: core::mem::ManuallyDrop::new(None),
            Subresource: 5,
            StateBefore: D3D12_RESOURCE_STATE_RENDER_TARGET,
            StateAfter: D3D12_RESOURCE_STATE_COPY_SOURCE,
        });
        let (mut e, mut held) = (event(), [p(1), p(1)]);
        assert_eq!(unsafe { cvr_tae_barrier(&b, &mut e, &mut held) }, 1);
        assert_eq!((e.kind, e.subresource, e.before, e.after, e.flags, held[0]), (kind::TRANSITION, 5, 4, 0x800, 2, core::ptr::null_mut()));
        b.Type = D3D12_RESOURCE_BARRIER_TYPE_ALIASING;
        b.Anonymous.Aliasing = core::mem::ManuallyDrop::new(D3D12_RESOURCE_ALIASING_BARRIER::default());
        let mut e = event();
        assert_eq!(unsafe { cvr_tae_barrier(&b, &mut e, &mut held) }, 2);
        assert_eq!(e.kind, kind::ALIAS);
        b.Type = D3D12_RESOURCE_BARRIER_TYPE(7);
        let mut e = event();
        assert_eq!(unsafe { cvr_tae_barrier(&b, &mut e, &mut held) }, 0);
        assert_eq!(e.kind, kind::UNKNOWN);
    }
    #[test]
    fn sources() {
        assert!(source_arguments(1, 3, 9, 7, 7, 2, 0));
        for (side, frame, token, lease, list, generation, held) in
            [(2, 3, 9, 7, 7, 2, 0), (0, 0, 9, 7, 7, 2, 0), (0, 3, 0, 7, 7, 2, 0), (0, 3, 9, 6, 7, 2, 0), (0, 3, 9, 7, 7, 0, 0), (0, 3, 9, 7, 7, 2, 5)]
        {
            assert!(!source_arguments(side, frame, token, lease, list, generation, held));
        }
    }
    #[test]
    fn proofs() {
        let null = core::ptr::null_mut();
        assert_eq!(proof_resources(&[p(1), null, p(2), p(3), p(4)]), Some([p(1), p(2), p(3), p(4)]));
        assert_eq!(proof_resources(&[p(1), p(2), p(3)]), None);
        assert_eq!(proof_resources(&[p(1), p(2), p(3), p(3)]), None);
        assert_eq!(proof_resources(&[p(1), p(2), p(3), p(4), p(5)]), None);
        assert_eq!(proof_resources(&[]), None);
    }
}
