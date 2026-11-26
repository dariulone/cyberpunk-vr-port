//! src/Hooks/HookServices.cpp's RTTI and engine-object services in Rust (the plugin's; vrik_tests keeps its stubs):
//! the surveillance control's owner, the tank test, the swimming player's input object and the external VRCAM slot.

use core::ffi::c_void;
use core::ptr::null_mut;
use cvr_engine::code::{guarded_call4, read};
use red4ext_abi::cname::CName;
use red4ext_abi::rtti::{object_class, RttiSystem};
use std::sync::OnceLock;

extern "C" {
    /// rust/crates/cvr-rig/src/vrik/roomscale_move.rs.
    fn cvr_roomscale_is_player_provider(provider: *const c_void) -> bool;
}

/// surveillance_body.rs's SurveillanceControl.
#[repr(C)]
pub struct SurveillanceControl {
    owner: usize,
    entity: u64,
    enabled: bool,
    /// 0 another owner type, 1 SurveillanceCamera, 2 SniperNest.
    kind: u8,
}
/// entIComponent::owner (+0x50) and isEnabled (+0x8B); entEntity::entityID (+0x48).
const COMPONENT_OWNER: usize = 0x50;
const COMPONENT_ENABLED: usize = 0x8B;
const ENTITY_ID: usize = 0x48;

/// The control component's owner, its entity and whether it is a surveillance camera or a sniper nest.
///
/// # Safety
/// `component` is a live entIComponent; `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_surveillance_control(component: usize, out: *mut SurveillanceControl) -> bool {
    if component == 0 || out.is_null() {
        return false;
    }
    // SAFETY: the component's own fields, then its owner's.
    unsafe {
        let owner = *((component + COMPONENT_OWNER) as *const usize);
        let enabled = *((component + COMPONENT_ENABLED) as *const bool);
        *out = SurveillanceControl { owner, entity: 0, enabled, kind: 0 };
        if owner == 0 {
            return true;
        }
        (*out).entity = *((owner + ENTITY_ID) as *const u64);
        let name = object_class(owner as *const c_void).map(|c| c.class_name());
        (*out).kind = if name == Some(CName::hashed("SniperNest")) {
            2
        } else if name == Some(CName::hashed("SurveillanceCamera")) {
            1
        } else {
            0
        };
    }
    true
}

/// Whether a vehicle is a tank (vehicleTankBaseObject or a subclass; the class looked up once, as the C++ static was).
///
/// # Safety
/// `owner` is a live ISerializable or 0.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_is_tank(owner: usize) -> bool {
    static TANK: OnceLock<bool> = OnceLock::new();
    if owner == 0 {
        return false;
    }
    let name = CName::hashed("vehicleTankBaseObject");
    let known = *TANK.get_or_init(|| RttiSystem::get().and_then(|r| r.get_class(name)).is_some());
    // SAFETY: as documented.
    known && unsafe { object_class(owner as *const c_void) }.is_some_and(|c| c.is_a(name))
}

/// The swimming locomotion's player input object: its wrapper's (+0x30) getter's context, when that context's provider
/// (+8) is the player's, holds it at +0x10. Every read and the engine's getter guarded (0 on a fault).
///
/// # Safety
/// `locomotion` is the engine's locomotion object or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_swimming_player_input_object(locomotion: *mut c_void) -> usize {
    if locomotion.is_null() {
        return 0;
    }
    let wrapper = locomotion as usize + 0x30;
    let Some(table) = read::<usize>(wrapper).filter(|&t| t != 0) else { return 0 };
    let Some(getter) = read::<usize>(table).filter(|&g| g != 0) else { return 0 };
    // SAFETY: the engine's own getter over its wrapper; a fault ends the call.
    let Some(context) = (unsafe { guarded_call4(getter, wrapper as *mut c_void, null_mut(), null_mut(), null_mut()) })
        .map(|c| c as usize)
        .filter(|&c| c != 0)
    else {
        return 0;
    };
    let Some(provider) = read::<usize>(context + 8) else { return 0 };
    // SAFETY: a plain query of the roomscale move.
    if !unsafe { cvr_roomscale_is_player_provider(provider as *const c_void) } {
        return 0;
    }
    read::<usize>(context + 0x10).unwrap_or(0)
}

// ---- the external VRCAM component's slot (formerly s_externalVrcam, a WeakObjectSlot of the component's own weak handle)

/// The slot: the weak reference, the component's address and its reference counts' address.
struct VrcamSlot {
    value: red4ext_abi::handle::WeakHandle,
    identity: usize,
    reference: usize,
}
// SAFETY: the handle's counts are the engine's atomics; the slot is only reached under its mutex.
unsafe impl Send for VrcamSlot {}
static EXTERNAL_VRCAM: std::sync::Mutex<VrcamSlot> =
    std::sync::Mutex::new(VrcamSlot { value: red4ext_abi::handle::WeakHandle::null(), identity: 0, reference: 0 });
/// ISerializable::ref (+0x08): the object's own weak handle.
const SELF_REF: usize = 0x08;
#[allow(non_upper_case_globals)]
extern "C" {
    /// cvr::detail::g_vrcam_comp (src/Stereo): the second eye's camera component.
    #[link_name = "?g_vrcam_comp@detail@cvr@@3U?$atomic@_K@std@@A"]
    static g_vrcam_comp: core::sync::atomic::AtomicUsize;
}

/// The VRCAM component seen by the engine's RTT callbacks (only called with their live argument): its own weak handle
/// bound when it is the component's, unless that component and counts are bound already.
///
/// # Safety
/// `component` is a live entIComponent or 0.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_external_vrcam_observe(component: usize) {
    if component == 0 {
        return;
    }
    // SAFETY: the component's own weak handle.
    let observed = unsafe { &*((component + SELF_REF) as *const red4ext_abi::handle::WeakHandle) };
    if observed.instance as usize != component || observed.ref_count.is_null() {
        return;
    }
    let control = observed.ref_count as usize;
    let mut slot = EXTERNAL_VRCAM.lock().unwrap_or_else(|p| p.into_inner());
    if slot.identity == component && slot.reference == control {
        return;
    }
    slot.value = observed.clone();
    slot.identity = component;
    slot.reference = control;
}

/// 0: notified, the component held alive through the call; 1: expired or replaced; 2: disabled, detached, or no
/// longer the second eye's camera.
///
/// # Safety
/// `notify` is callable with the eye camera.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_external_vrcam_notify(eye_camera: usize, notify: unsafe extern "C" fn(usize)) -> i32 {
    let live = {
        let slot = EXTERNAL_VRCAM.lock().unwrap_or_else(|p| p.into_inner());
        if eye_camera == 0 || slot.identity != eye_camera {
            return 1;
        }
        slot.value.lock()
    };
    // The strong reference is let go outside the slot's lock (its last release may destroy the component).
    if live.is_null() || live.instance as usize != eye_camera {
        return 1;
    }
    let component = live.instance as usize;
    // SAFETY: the component held alive by `live`.
    let (enabled, owner) = unsafe { (*((component + COMPONENT_ENABLED) as *const bool), *((component + COMPONENT_OWNER) as *const usize)) };
    // SAFETY: the C++ global's atomic.
    if !enabled || owner == 0 || eye_camera != unsafe { g_vrcam_comp.load(core::sync::atomic::Ordering::Acquire) } {
        return 2;
    }
    // SAFETY: as documented.
    unsafe { notify(eye_camera) };
    0
}
