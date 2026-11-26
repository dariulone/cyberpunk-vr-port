//! The animation pose-apply hook (formerly src/Hooks/AnimPose.cpp): the engine's apply first, then the player's work
//! (pose_hook.rs's cvr_anim_pose_apply) inside an SEH frame (cvr-engine's native/seh.c) with the player-state lock it
//! may leave held released afterwards, then the mirror double. Installed when script asks (the InstallVRAnimPoseHook
//! native, through cvr_cpp_install_anim_pose_hook), not at boot.
//!
//! With it (main's 0aa233e0a), an animated object's update: the object and its pose descriptor are this thread's scope
//! while the update runs, so a pose pass knows whose object it is (the Osprey's hinge, optic_hinge.rs). And the NPC
//! grips' completed pose (npc_grip.rs, main's NpcArmGrip): the final LocalToModel of that object's own descriptor --
//! told by its return address, the one call that comes after all its graph controllers -- or, when that call did not
//! come, the update's return.

use core::ffi::c_void;
use core::ptr::null_mut;

#[allow(non_snake_case)]
unsafe extern "system" {
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_DisableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}
unsafe extern "C" {
    /// cpp_services.rs: g_PlayerPoseStateLock released (taken by the work).
    fn cvr_cpp_pose_unlock();
}
const MH_OK: i32 = 0;
/// 2.5: the animation pose-apply.
const ANIM_POSE_RVA: usize = 0x1B_3C08;

/// 2.5: an animated object's update, its entry verified with the pose-copy target.
const ANIM_OBJECT_UPDATE_RVA: usize = 0x1B_3E00;
const ANIM_OBJECT_UPDATE_ENTRY: [u8; 7] = [0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x18];
/// 2.5: a pose descriptor's local-to-model conversion, and the return address of its call at the end of an object's
/// update (the same conversion also resets an object at the START of its update).
const POSE_LOCAL_TO_MODEL_RVA: usize = 0x58_2B3C;
const POSE_LOCAL_TO_MODEL_ENTRY: [u8; 6] = [0x40, 0x53, 0x48, 0x83, 0xEC, 0x40];
const COMPLETED_CONVERSION_RETURN_RVA: usize = 0x1B_427D;

/// The engine's own apply, written by the install.
static mut ORIGINAL: *mut c_void = null_mut();
type AnimPose = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, u32) -> *mut c_void;
/// The engine's own object update.
static mut ORIGINAL_OBJECT_UPDATE: *mut c_void = null_mut();
type ObjectUpdate = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
/// The engine's own conversion.
static mut ORIGINAL_LOCAL_TO_MODEL: *mut c_void = null_mut();
static COMPLETED_CONVERSION_RETURN: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

thread_local! {
    /// The animated object whose update this thread runs, its pose descriptor (object+0x18), and whether its completed
    /// conversion has been seen.
    static OBJECT_SCOPE: core::cell::Cell<(usize, usize, bool)> = const { core::cell::Cell::new((0, 0, false)) };
}
/// The object scope of the pass running on this thread: (object, its descriptor), or zeros outside an object update.
pub fn object_scope() -> (usize, usize) {
    let (object, descriptor, _) = OBJECT_SCOPE.with(|c| c.get());
    (object, descriptor)
}
/// Hooked_AnimObjectUpdate: the object and its descriptor this thread's scope for the update (the previous restored);
/// the NPC grips told the object is done when its completed conversion did not tell them already.
unsafe extern "C" fn hooked_anim_object_update(object: *mut c_void, context: *mut c_void) -> usize {
    // SAFETY: written by the install before the hook was enabled.
    let original: ObjectUpdate = unsafe { core::mem::transmute(ORIGINAL_OBJECT_UPDATE) };
    let descriptor = cvr_engine::code::read::<usize>(object as usize + 0x18).unwrap_or(0);
    let previous = OBJECT_SCOPE.with(|c| c.replace((object as usize, descriptor, false)));
    // SAFETY: the engine's own call.
    let result = unsafe { original(object, context) };
    let (_, _, converted) = OBJECT_SCOPE.with(|c| c.get());
    if !converted {
        super::npc_grip::object_updated(object as usize);
        super::vehicle_doors::pose_completed(descriptor);
    }
    OBJECT_SCOPE.with(|c| c.set(previous));
    result
}
type LocalToModel = unsafe extern "C" fn(*mut c_void, *mut c_void) -> usize;
// Hooked_PoseLocalToModel's entry: the caller's return address (main's _ReturnAddress()) handed to the work as its third
// argument, and a jump, so the work returns straight to the caller.
core::arch::global_asm!(
    ".globl cvr_pose_local_to_model_detour",
    "cvr_pose_local_to_model_detour:",
    "mov r8, [rsp]",
    "jmp {work}",
    work = sym pose_local_to_model,
);
unsafe extern "C" {
    fn cvr_pose_local_to_model_detour();
}
/// Hooked_PoseLocalToModel: only the verified final call of the scope's own descriptor has completed all graph
/// controllers -- the NPC grips solved on it, before the engine converts it.
unsafe extern "C" fn pose_local_to_model(descriptor: *mut c_void, rig: *mut c_void, returns_to: usize) -> usize {
    let (object, scope_descriptor, converted) = OBJECT_SCOPE.with(|c| c.get());
    if object != 0
        && descriptor as usize == scope_descriptor
        && returns_to == COMPLETED_CONVERSION_RETURN.load(core::sync::atomic::Ordering::Relaxed)
    {
        super::npc_grip::pose_completed(descriptor as usize, true);
        super::vehicle_doors::pose_completed(descriptor as usize);
        if !converted {
            OBJECT_SCOPE.with(|c| c.set((object, scope_descriptor, true)));
        }
    }
    // SAFETY: written by the install before the hook was enabled; the engine's own call.
    let original: LocalToModel = unsafe { core::mem::transmute(ORIGINAL_LOCAL_TO_MODEL) };
    unsafe { original(descriptor, rig) }
}

/// Hooked_AnimPoseApply: the engine's apply, the guarded work (a fault in it ends it, the lock it held released), then
/// the mirror double over the pose's bone and track buffers.
unsafe extern "C" fn hooked_anim_pose_apply(a1: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: u32) -> *mut c_void {
    // SAFETY: written by the install before the hook was enabled.
    let original: AnimPose = unsafe { core::mem::transmute(ORIGINAL) };
    // SAFETY: the engine's own call.
    let result = unsafe { original(a1, a2, a3, a4) };
    let mut locked = false;
    // SAFETY: the work's arguments; a fault inside it ends the call.
    let _ = unsafe {
        cvr_engine::code::guarded_call4(
            super::pose_hook::cvr_anim_pose_apply as *const () as usize,
            a1,
            a2,
            a4 as usize as *mut c_void,
            (&raw mut locked).cast(),
        )
    };
    if locked {
        // SAFETY: taken by this thread's work.
        unsafe { cvr_cpp_pose_unlock() };
    }
    if !super::vrik_arming::VRIK_MirrorDoubleActive() {
        return result;
    }
    // poseDesc = a2[7]; its bone buffer [0], track buffer [3] and slot count (+8), each read where a fault ends it.
    use cvr_engine::code::read;
    let Some(desc) = read::<usize>(a2 as usize + 7 * 8).filter(|&d| d != 0) else { return result };
    let (Some(bones), Some(tracks), Some(slots)) = (read::<usize>(desc), read::<usize>(desc + 3 * 8), read::<u32>(desc + 8))
    else {
        return result;
    };
    if bones != 0 && tracks != 0 {
        // SAFETY: the pose's buffers; a fault inside the pass ends it.
        let _ = unsafe {
            cvr_engine::code::guarded_call4(
                super::vrik_arming::VRIK_MirrorDoublePass as *const () as usize,
                bones as *mut c_void,
                tracks as *mut c_void,
                slots as usize as *mut c_void,
                null_mut(),
            )
        };
    }
    result
}

/// InstallAnimPoseHook (the native's): the detour on the engine's pose-apply.
#[no_mangle]
pub extern "C" fn cvr_cpp_install_anim_pose_hook() -> bool {
    static INSTALLED: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
    if INSTALLED.load(core::sync::atomic::Ordering::Acquire) {
        return true;
    }
    let installed = install();
    if installed {
        INSTALLED.store(true, core::sync::atomic::Ordering::Release);
    }
    installed
}
fn install() -> bool {
    let name: Vec<u16> = "Cyberpunk2077.exe".encode_utf16().chain(Some(0)).collect();
    // SAFETY: a terminated name.
    let module = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name.as_ptr()) };
    if module.is_null() {
        return false;
    }
    let target = (module as usize + ANIM_POSE_RVA) as *mut c_void;
    let object_target = (module as usize + ANIM_OBJECT_UPDATE_RVA) as *mut c_void;
    let conversion_target = (module as usize + POSE_LOCAL_TO_MODEL_RVA) as *mut c_void;
    COMPLETED_CONVERSION_RETURN.store(module as usize + COMPLETED_CONVERSION_RETURN_RVA, core::sync::atomic::Ordering::Relaxed);
    // SAFETY: the executable's code, mapped.
    let (object_entry, conversion_entry) = unsafe {
        (
            core::slice::from_raw_parts(object_target as *const u8, ANIM_OBJECT_UPDATE_ENTRY.len()),
            core::slice::from_raw_parts(conversion_target as *const u8, POSE_LOCAL_TO_MODEL_ENTRY.len()),
        )
    };
    if object_entry != ANIM_OBJECT_UPDATE_ENTRY || conversion_entry != POSE_LOCAL_TO_MODEL_ENTRY {
        super::env::log("[npcgrip] complete object pose hook has an unsupported entry
");
        return false;
    }
    let sites: [(*mut c_void, *mut c_void, *mut *mut c_void); 3] = [
        (target, hooked_anim_pose_apply as *const () as *mut c_void, (&raw mut ORIGINAL).cast()),
        (object_target, hooked_anim_object_update as *const () as *mut c_void, (&raw mut ORIGINAL_OBJECT_UPDATE).cast()),
        (conversion_target, cvr_pose_local_to_model_detour as *const () as *mut c_void, (&raw mut ORIGINAL_LOCAL_TO_MODEL).cast()),
    ];
    // SAFETY: MinHook on the engine's functions (initialising it is a no-op when done); all three or none.
    unsafe {
        MH_Initialize();
        for (i, &(t, detour, original)) in sites.iter().enumerate() {
            if MH_CreateHook(t, detour, original) != MH_OK {
                for &(undo, _, _) in &sites[..i] {
                    MH_RemoveHook(undo);
                }
                return false;
            }
        }
        if sites.iter().any(|&(t, _, _)| MH_EnableHook(t) != MH_OK) {
            for &(t, _, _) in &sites {
                MH_DisableHook(t);
                MH_RemoveHook(t);
            }
            return false;
        }
    }
    super::env::log("[npcgrip] complete object pose hook active after all native graph controllers
");
    true
}
