//! PhysXStep (main's src/Natives/PhysXStep.cpp) -- the game's PhysX step, hooked in PhysX3_x64.dll: PxScene::simulate
//! and fetchResults.
//!
//! Measured on the simulator (frame_order, 2026-11-08), every frame in this order: the hand collision script's tick, the
//! player's hands solved and published (~3 ms before the step), PxScene::simulate on a job thread, fetchResults ~1.3 ms
//! later, the instances' pose records sent to the renderer, the frame submit. So what is set on an actor just before
//! simulate goes into this frame's step with this frame's hands, and its result is what this frame draws.
use super::frame_order::note;
use super::physhands;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering::*};
use cvr_engine::code::{guarded_call4, read};
use cvr_host::log;
use std::sync::Mutex;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};

// PxPhysics::getNbScenes / getScenes, PxScene::simulate / fetchResults (PhysX 3.4, counted from its headers and checked
// against the slots debug_view already calls).
const PHYSICS_GET_NB_SCENES: usize = 19;
const PHYSICS_GET_SCENES: usize = 20;
const SCENE_SIMULATE: usize = 56;
const SCENE_FETCH_RESULTS: usize = 61;

type SimulateFn = unsafe extern "C" fn(*mut c_void, f32, *mut c_void, *mut c_void, u32, bool);
type FetchFn = unsafe extern "C" fn(*mut c_void, bool, *mut u32) -> bool;
static mut ORIGINAL_SIMULATE: Option<SimulateFn> = None;
static mut ORIGINAL_FETCH: Option<FetchFn> = None;
static HOOKED: AtomicBool = AtomicBool::new(false);
static HOOK_LOCK: Mutex<()> = Mutex::new(());

/// The physics pose frame open for the step's work (physhands' begin/end_physics_pose_frame), closed however it ends.
struct PoseScope;
impl PoseScope {
    fn open() -> Self {
        physhands::pose::begin_physics_pose_frame();
        PoseScope
    }
}
impl Drop for PoseScope {
    fn drop(&mut self) {
        physhands::pose::end_physics_pose_frame();
    }
}
unsafe extern "C" fn simulate(scene: *mut c_void, dt: f32, completion: *mut c_void, scratch: *mut c_void, scratch_size: u32, control: bool) {
    note(b'S');
    {
        let _pose = PoseScope::open();
        physhands::heldbody::before_simulate(scene, dt);
        physhands::step::before_simulate(scene, dt);
    }
    if let Some(original) = ORIGINAL_SIMULATE {
        original(scene, dt, completion, scratch, scratch_size, control);
    }
    note(b's');
}
unsafe extern "C" fn fetch(scene: *mut c_void, block: bool, error: *mut u32) -> bool {
    note(b'F');
    let done = match ORIGINAL_FETCH {
        Some(original) => original(scene, block, error),
        None => false,
    };
    if done {
        physhands::step::after_fetch(scene);
    }
    note(b'f');
    done
}

unsafe extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
    fn MH_QueueEnableHook(target: *mut c_void) -> i32;
    fn MH_ApplyQueued() -> i32;
}
/// Both functions hooked, enabled in one pass. MH_EnableHook stops every thread of the process while it patches, and the
/// game has a few hundred: done for one hook and then the other, on the frame a thing was first taken, that was the 56
/// to 92 ms frame every session showed ("[cost] a long frame ... carrying"). So it is done once, at boot.
unsafe fn hook_at(simulate_at: *mut c_void, fetch_at: *mut c_void) -> bool {
    if MH_CreateHook(simulate_at, simulate as *const () as *mut c_void, (&raw mut ORIGINAL_SIMULATE).cast()) != 0 {
        return false;
    }
    if MH_CreateHook(fetch_at, fetch as *const () as *mut c_void, (&raw mut ORIGINAL_FETCH).cast()) != 0 {
        MH_RemoveHook(simulate_at);
        return false;
    }
    if MH_QueueEnableHook(simulate_at) != 0 || MH_QueueEnableHook(fetch_at) != 0 || MH_ApplyQueued() != 0 {
        MH_RemoveHook(simulate_at);
        MH_RemoveHook(fetch_at);
        return false;
    }
    true
}

/// AT BOOT, FROM THE DLL ITSELF. No scene exists yet, but every scene's table is NpScene's, at RVA 0x20CAF0 of the
/// PhysX3_x64.dll the game ships (simulate 56 = RVA 0x53760, fetchResults 61 = RVA 0x53980, the same two a live scene's
/// table gave). Another DLL fails the checks below and is left to the lazy path, which reads a live scene's table.
const SCENE_TABLE_RVA: usize = 0x20CAF0;
const SIMULATE_RVA: usize = 0x53760;
const FETCH_RESULTS_RVA: usize = 0x53980;
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_physx_step_install() -> bool {
    let dll = GetModuleHandleA(c"PhysX3_x64.dll".as_ptr().cast()) as usize;
    if dll == 0 {
        return false;
    }
    let table = dll + SCENE_TABLE_RVA;
    let (Some(simulate_at), Some(fetch_at)) = (read::<usize>(table + 8 * SCENE_SIMULATE), read::<usize>(table + 8 * SCENE_FETCH_RESULTS)) else { return false };
    if simulate_at != dll + SIMULATE_RVA || fetch_at != dll + FETCH_RESULTS_RVA {
        log!("[physxstep] PhysX3_x64.dll is not the one read: the step is hooked from the first scene instead\n");
        return false;
    }
    let _lock = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let ok = !HOOKED.load(SeqCst) && hook_at(simulate_at as *mut c_void, fetch_at as *mut c_void);
    if ok {
        HOOKED.store(true, Release);
    }
    ok || HOOKED.load(SeqCst)
}

unsafe fn slot(object: *mut c_void, slot: usize) -> Option<usize> {
    read::<usize>(read::<usize>(object as usize)? + 8 * slot)
}
/// The lazy path: from a live scene's table, when the boot's checks did not hold.
unsafe fn hook() -> Option<bool> {
    let dll = GetModuleHandleA(c"PhysX3_x64.dll".as_ptr().cast());
    if dll.is_null() {
        return Some(false);
    }
    let Some(get_physics) = GetProcAddress(dll, c"PxGetPhysics".as_ptr().cast()) else { return Some(false) };
    let null = core::ptr::null_mut();
    let physics = guarded_call4(get_physics as usize, null, null, null, null)?;
    if physics.is_null() {
        return Some(false);
    }
    let count = guarded_call4(slot(physics, PHYSICS_GET_NB_SCENES)?, physics, null, null, null)? as usize as u32;
    let mut scene: *mut c_void = null;
    if count == 0 || guarded_call4(slot(physics, PHYSICS_GET_SCENES)?, physics, (&raw mut scene).cast(), 1 as *mut c_void, null)? as usize as u32 != 1 || scene.is_null() {
        return Some(false);
    }
    let simulate_at = slot(scene, SCENE_SIMULATE)? as *mut c_void;
    let fetch_at = slot(scene, SCENE_FETCH_RESULTS)? as *mut c_void;
    let in_dll = |p: *mut c_void| {
        let mut m: MEMORY_BASIC_INFORMATION = core::mem::zeroed();
        VirtualQuery(p, &mut m, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 && m.AllocationBase == dll
    };
    if !in_dll(simulate_at) || !in_dll(fetch_at) {
        log!("[physxstep] the scene's table is not PhysX's own: not hooked\n");
        return Some(false);
    }
    if !hook_at(simulate_at, fetch_at) {
        return Some(false);
    }
    log!("[physxstep] PxScene::simulate {:#x} and fetchResults {:#x} hooked ({} scenes)\n", simulate_at as usize, fetch_at as usize, count);
    Some(true)
}
/// Ensure: the two hooked once, from a live scene's table if the boot did not. False if there is no scene yet or the
/// table is not PhysX's.
pub fn ensure() -> bool {
    if HOOKED.load(Acquire) {
        return true;
    }
    let _lock = HOOK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut ok = HOOKED.load(SeqCst);
    if !ok {
        // SAFETY: guarded calls into PhysX; the hook made under the lock.
        ok = unsafe { hook() }.unwrap_or(false);
        HOOKED.store(ok, Release);
    }
    ok
}
