//! Visible scene-plane hand velocities (main's 269e9ee6d, include/Render/SceneVelocityPass.hpp and NodeDispatch.cpp's
//! draw_scene_object_velocity). With the first-person draws routed to the scene plane, a scene-depth hand writes the
//! right current -> previous vector in GBufferEarly_Velocity_Weapon, but InitVelocityBuffer later fills depth <= .9 with
//! the camera's motion before the scene's object velocity passes run. The late solid / discard velocity nodes each hold
//! one unsplit scene pass: the same material stage's first-person geometry (bucket 2) is appended to it, drawn with the
//! scene's depth test and cameras, on a call-local copy of the node (the graph's shared descriptor stays immutable while
//! a scope view and both eyes run on different workers).
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
#[cfg(feature = "rust-build")]
use cvr_engine::code::{read, read_bytes};

/// 2.5's render-stage descriptor (sub_1410808DC). The opaque and padding bytes are the engine's entry's; the bucket at
/// +20 selects the geometry independently of the flags at +8 that select the view parameters.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct Pass {
    pub callback: usize,
    pub flags: u32,
    pub sort_mode: u8,
    pub padding0: [u8; 3],
    pub stage: u32,
    pub bucket: u8,
    pub padding1: [u8; 3],
    pub draw_args: [u32; 2],
    pub override_state: u8,
    pub padding2: [u8; 3],
    pub shared_sort: i32,
    pub partition: u32,
    pub partitions: u32,
}
const _: () = assert!(
    core::mem::size_of::<Pass>() == 48
        && core::mem::offset_of!(Pass, stage) == 16
        && core::mem::offset_of!(Pass, bucket) == 20
        && core::mem::offset_of!(Pass, override_state) == 32
        && core::mem::offset_of!(Pass, shared_sort) == 36
);

/// WithObjectVelocity: a late solid / discard velocity node's one unsplit scene pass, and the same stage's
/// first-person geometry after it (as the early velbuff draw for that bucket: no sort).
pub fn with_object_velocity(scene: &Pass, solid: u32, discard: u32) -> Option<[Pass; 2]> {
    if scene.stage == u32::MAX
        || (scene.stage != solid && scene.stage != discard)
        || scene.flags != 0x11
        || scene.bucket != 0
        || scene.callback != 0
        || scene.override_state != 0
        || scene.shared_sort != -1
        || scene.partition != 0
        || scene.partitions != 0
        || scene.sort_mode != if scene.stage == solid { 1 } else { 0 }
    {
        return None;
    }
    let mut out = [*scene, *scene];
    out[1].bucket = 2;
    out[1].sort_mode = 0;
    Some(out)
}

/// The renderer assigns the stage numbers: the descriptor constructor is watched while it still has the stage's name.
static VELOCITY_SOLID: AtomicU32 = AtomicU32::new(u32::MAX);
static VELOCITY_DISCARD: AtomicU32 = AtomicU32::new(u32::MAX);
/// The constructor's trampoline (written by the detour registry).
#[no_mangle]
pub static cvr_scene_velocity_stage_original: AtomicUsize = AtomicUsize::new(0);
/// Live A/B on one binary: only the added draws follow it.
#[no_mangle]
pub static CyberpunkVR_SceneObjectVelocity: AtomicU32 = AtomicU32::new(1);
/// The added draws: [0] solid, [1] discard.
#[no_mangle]
pub static CyberpunkVR_SceneVelocityPasses: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// The stage descriptor constructor sub_1410808DC(dst, flags, name, bucket, sortMode).
///
/// # Safety
/// The engine's call, its own arguments.
pub unsafe extern "C" fn cvr_scene_velocity_stage_ctor(dst: *mut Pass, flags: u32, name: *const c_char, bucket: u8, sort_mode: u8) -> *mut Pass {
    type Ctor = unsafe extern "C" fn(*mut Pass, u32, *const c_char, u8, u8) -> *mut Pass;
    let original: Ctor = unsafe { core::mem::transmute(cvr_scene_velocity_stage_original.load(Acquire)) };
    let result = unsafe { original(dst, flags, name, bucket, sort_mode) };
    if !result.is_null() && !name.is_null() && flags == 0x11 && bucket == 0 {
        // SAFETY: the stage's literal name, NUL-terminated.
        let name = unsafe { core::ffi::CStr::from_ptr(name) }.to_bytes();
        // SAFETY: the descriptor the constructor just filled.
        let stage = unsafe { (*result).stage };
        match name {
            b"renderstage_gbuffer_velbuff_regular" => VELOCITY_SOLID.store(stage, Relaxed),
            b"renderstage_gbuffer_velbuff_discarded" => VELOCITY_DISCARD.store(stage, Relaxed),
            _ => {}
        }
    }
    result
}

/// draw_scene_object_velocity: the node drawn here with its first-person velocity added (true), or left to the caller.
///
/// # Safety
/// The pass driver's node, work and args; `original` the driver's trampoline.
#[cfg(feature = "rust-build")]
pub unsafe fn draw(node: usize, work: usize, args: usize, base: usize, original: unsafe extern "C" fn(usize, usize, usize)) -> bool {
    if crate::render_tls::view_side() < 0
        || !crate::plane_patches::cvr_scene_plane_applied()
        || CyberpunkVR_SceneObjectVelocity.load(Relaxed) == 0
        || node == 0
        || work == 0
        || base == 0
    {
        return false;
    }
    if read::<u32>(node + 60) != Some(1) {
        return false;
    }
    let Some(predicate) = read::<usize>(node + 24) else { return false };
    if predicate != base + 0x22_749F0 && predicate != base + 0x22_74AE0 {
        return false;
    }
    let Some(entry) = read::<usize>(node + 48).filter(|e| *e != 0) else { return false };
    let mut raw = [0u8; 48];
    if !read_bytes(entry, &mut raw) {
        return false;
    }
    // SAFETY: 48 bytes read as the descriptor they are.
    let scene: Pass = unsafe { core::ptr::read_unaligned(raw.as_ptr().cast()) };
    let Some(passes) = with_object_velocity(&scene, VELOCITY_SOLID.load(Relaxed), VELOCITY_DISCARD.load(Relaxed)) else { return false };
    #[repr(C, align(16))]
    struct NodeCopy([u8; 72]);
    let mut copy = NodeCopy([0; 72]);
    if !read_bytes(node, &mut copy.0) {
        return false;
    }
    let entries = passes.as_ptr() as usize;
    copy.0[48..56].copy_from_slice(&entries.to_ne_bytes());
    copy.0[56..60].copy_from_slice(&2u32.to_ne_bytes());
    copy.0[60..64].copy_from_slice(&2u32.to_ne_bytes());
    // SAFETY: the driver on a call-local node whose pass list lives on this frame.
    unsafe { original(copy.0.as_ptr() as usize, work, args) };
    if crate::node_features::diagnostics() {
        CyberpunkVR_SceneVelocityPasses[if passes[0].sort_mode == 1 { 0 } else { 1 }].fetch_add(1, Relaxed);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene(stage: u32, sort: u8) -> Pass {
        Pass { flags: 0x11, sort_mode: sort, stage, shared_sort: -1, ..Default::default() }
    }
    #[test]
    fn appends_first_person_geometry_to_the_late_velocity_passes() {
        let p = with_object_velocity(&scene(7, 1), 7, 9).expect("solid");
        assert_eq!((p[0].bucket, p[0].sort_mode, p[1].bucket, p[1].sort_mode, p[1].stage), (0, 1, 2, 0, 7));
        let d = with_object_velocity(&scene(9, 0), 7, 9).expect("discard");
        assert_eq!((d[1].bucket, d[1].sort_mode), (2, 0));
    }
    #[test]
    fn leaves_every_other_pass_alone() {
        assert!(with_object_velocity(&scene(8, 1), 7, 9).is_none());
        assert!(with_object_velocity(&scene(7, 0), 7, 9).is_none());
        assert!(with_object_velocity(&scene(u32::MAX, 1), u32::MAX, 9).is_none());
        let mut s = scene(7, 1);
        s.bucket = 2;
        assert!(with_object_velocity(&s, 7, 9).is_none());
        let mut s = scene(7, 1);
        s.partitions = 2;
        assert!(with_object_velocity(&s, 7, 9).is_none());
    }
}
