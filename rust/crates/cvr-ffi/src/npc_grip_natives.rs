//! The NPC grips' natives (main's src/Anim/NpcArmGrip.cpp): VRNpcGrip, VRNpcGripBind and VRNpcGripState. The work is
//! cvr-rig's (vrik/npc_grip); the person's animated object is found here, with the animation system's lookup (main's
//! FindAnimatedObjectOf: the entity's root animated object), and only when a grip begins.
use crate::native_args::args;
use core::ffi::c_void;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::stack::StackFrame;

/// The entity's root animated object, by walking the animation system's buckets.
pub(crate) fn root_object(entity: &Handle) -> usize {
    crate::anim_lookup::animated_object_for_entity(entity.instance as usize, Some(c"root"), false)
}

/// VRNpcGrip(entity, side, part, target, turn) -> Float: see cvr_rig::vrik::npc_grip::grip.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRNpcGrip(_context: *mut c_void, frame: *mut StackFrame, out: *mut f32, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    let side = a.get::<i32>();
    let part = a.get::<i32>();
    let target = a.get::<[f32; 4]>();
    let turn = a.get::<[f32; 4]>();
    a.finish();
    let answer = cvr_rig::vrik::npc_grip::grip(&entity, side, part, &target, &turn, &|| root_object(&entity));
    if !out.is_null() {
        unsafe { *out = answer };
    }
}
/// VRNpcGripBind(entity, side, part, segment, owner) -> Float: see cvr_rig::vrik::npc_grip::grip_bind.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRNpcGripBind(_context: *mut c_void, frame: *mut StackFrame, out: *mut f32, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    let side = a.get::<i32>();
    let part = a.get::<i32>();
    let segment = a.get::<i32>();
    let owner = a.get::<i32>();
    a.finish();
    let answer = cvr_rig::vrik::npc_grip::grip_bind(&entity, side, part, segment, owner, &|| root_object(&entity));
    if !out.is_null() {
        unsafe { *out = answer };
    }
}
/// VRNpcGripState(side) -> array<Vector4>: see cvr_rig::vrik::npc_grip::grip_state.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRNpcGripState(_context: *mut c_void, frame: *mut StackFrame, out: *mut DynArray<[f32; 4]>, _a4: i64) {
    let mut a = unsafe { args(frame) };
    let side = a.get::<i32>();
    a.finish();
    let Some(out) = (unsafe { out.as_mut() }) else { return };
    out.clear();
    for row in cvr_rig::vrik::npc_grip::grip_state(side) {
        out.push(row);
    }
}
