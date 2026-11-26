//! VRCreationTarget(entity) -> Bool (formerly src/Lobby/CreationTarget.cpp): the game's character customization dresses
//! `entity` -- the lobby's figure, an NPC of the player's TPP template -- instead of its own preview puppet; a null entity
//! gives the game its puppet back. gameuiCharacterCustomizationSystem (2.5, vtable RVA 0x36B6280) applies every option to
//! the one puppet it holds as a weak handle at +0x108. Main's mirror editor adds VRMirrorAppearanceOptions (the option
//! resources loaded from the mirror's double) and VRCustomizationReset (the reset that leaves the double dressed).
use core::ffi::c_void;
use red4ext_abi::engine::system_named;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

const SYSTEM_VTABLE: usize = 0x36B6280;
/// WeakHandle: the puppet the options go onto.
const TARGET: usize = 0x108;
/// SetCustomizationOptionsResource's native implementation (vtable +0x238; 2.5: 22345D0, 2D8BFB4, 2D8BE88, 2D8BDB0).
const SET_OPTIONS_RESOURCE_SLOT: usize = 0x238;
/// ResetCustomizationOptionsResource (vtable +0x240 -> 2D8F7FC -> 3744A0).
const RESET_OPTIONS_RESOURCE_SLOT: usize = 0x240;
/// The loaded option resource (non-null once SetCustomizationOptionsResource took).
const OPTIONS_RESOURCE: usize = 0xA0;

/// The game's own target while the lobby's figure stands in, and whether it does.
struct Swap {
    game_target: WeakHandle,
    swapped: bool,
}
// SAFETY: a weak engine reference, touched on the game thread under the mutex.
unsafe impl Send for Swap {}
static SWAP: Mutex<Swap> = Mutex::new(Swap {
    game_target: WeakHandle::null(),
    swapped: false,
});

fn system() -> usize {
    let system = system_named("gameuiICharacterCustomizationSystem") as usize;
    // SAFETY: the system's vtable pointer.
    if system == 0 || unsafe { *(system as *const usize) } != cvr_engine::code::image_base() + SYSTEM_VTABLE {
        return 0;
    }
    system
}

unsafe fn vslot(system: usize, slot: usize) -> usize {
    *(*(system as *const *const usize)).add(slot / 8)
}
fn known_build() -> bool {
    cvr_engine::game_build::supported() && !cvr_engine::game_build::overridden()
}

/// VRMirrorAppearanceOptions(entity, male) -> Bool: the same option resources the game's editor loads, from a disposable
/// mirror puppet rather than the player's head swapped for a while. SetCustomizationOptionsResource's script signature
/// says PlayerPuppet, but its native implementation only reads entity components and the TppHead / RightArm slots.
#[no_mangle]
pub unsafe extern "C" fn VRMirrorAppearanceOptions(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let (mut entity, mut male) = (Handle::null(), false);
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.get_parameter(&mut male);
        frame.finish_params();
    }
    let mut ok = false;
    let system = system();
    let puppet = !entity.is_null() && red4ext_abi::rtti::object_class(entity.instance).is_some_and(|c| c.is_a(red4ext_abi::cname::CName::hashed("NPCPuppet")));
    if system != 0 && puppet && known_build() {
        // an empty callback object, exactly as the game's public script wrapper constructs it
        #[repr(C, align(16))]
        struct Callback([u8; 80]);
        let mut callback = Callback([0; 80]);
        type SetFn = unsafe extern "C" fn(*mut c_void, *const Handle, bool, *mut c_void);
        let f: SetFn = core::mem::transmute(vslot(system, SET_OPTIONS_RESOURCE_SLOT));
        f(system as *mut c_void, &entity, male, callback.0.as_mut_ptr().cast());
        ok = *((system + OPTIONS_RESOURCE) as *const usize) != 0;
    }
    if let Some(out) = out.as_mut() {
        *out = ok;
    }
}

/// VRCustomizationReset() -> Bool: ResetCustomizationOptionsResource first takes every part the system put on its target
/// back off that target (by resource: every copy, the target's own ones too). The mirror makes its double that target,
/// so a plain reset strips the reflection of hair, beard and piercings: the reset runs with no target, then the game
/// gets back the target it had.
#[no_mangle]
pub unsafe extern "C" fn VRCustomizationReset(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    if let Some(frame) = frame.as_mut() {
        frame.finish_params();
    }
    let mut ok = false;
    let system = system();
    if system != 0 && known_build() {
        let target = (system + TARGET) as *mut WeakHandle;
        let kept = (*target).clone();
        WeakHandle::assign(target, &WeakHandle::null());
        type ResetFn = unsafe extern "C" fn(*mut c_void);
        let f: ResetFn = core::mem::transmute(vslot(system, RESET_OPTIONS_RESOURCE_SLOT));
        f(system as *mut c_void);
        WeakHandle::assign(target, &kept);
        ok = true;
    }
    if let Some(out) = out.as_mut() {
        *out = ok;
    }
}

#[no_mangle]
pub unsafe extern "C" fn VRCreationTarget(_this: *mut c_void, frame: *mut StackFrame, out: *mut bool, _unused: i64) {
    let mut entity = Handle::null();
    if let Some(frame) = frame.as_mut() {
        frame.get_parameter(&mut entity);
        frame.finish_params();
    }
    let system = system();
    if system == 0 {
        super::env::log("[VRLobby] creation target: no character customization system of the known layout\n");
        if let Some(out) = out.as_mut() {
            *out = false;
        }
        return;
    }
    let target = (system + TARGET) as *mut WeakHandle;
    let mut swap = SWAP.lock().unwrap_or_else(|p| p.into_inner());
    if !entity.is_null() {
        if !swap.swapped {
            swap.game_target = (*target).clone();
            swap.swapped = true;
        }
        WeakHandle::assign(target, &WeakHandle::from_handle(&entity));
    } else if swap.swapped {
        let game = core::mem::take(&mut swap.game_target);
        WeakHandle::assign(target, &game);
        swap.swapped = false;
    }
    if let Some(out) = out.as_mut() {
        *out = true;
    }
}
