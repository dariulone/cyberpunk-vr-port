//! WHICH BODY THE PLAYER SPAWNS IN (formerly src/Lobby/PlayerBody.cpp). Verified against game 2.5 with IDA headless and
//! live (2026-11-02).
//!
//! `Character.Player_Puppet_Base.genders` holds one entity per gender, one of them marked `initial`. A puppet's gender
//! is its persistent state's (gamePuppetPS +0x68, entity +0x168), and with none there the `initial` entry's -- and it is
//! WRITTEN that way too: the game stores nothing for the gender `initial` names, so the flag is part of the save format.
//! In the game as shipped `initial` is the woman: every woman's save holds no gender, every man's holds Male. THE FLAG
//! MUST NOT CHANGE, NOT EVEN FOR THE MAIN MENU (0.2.0 swapped it, and every woman's save read as a man).
//!
//! The player's spawn (sub_1409CE8FC) takes a gender from its request (+72): "Default" in the main menu, where no
//! character exists; nothing on a load, which reads the persistent state. A gender in the request is what the game does
//! for a new character. So:
//!  - the main menu's spawn asks for the man's, and what that writes lives and dies with the main menu's session;
//!  - a load asks for the man's when the save's character is a man's and its persistent state has no gender (a new
//!    game started on 0.2.0): the write puts the gender there, and the next save carries it.
//! Everything else goes as the game has it.
//!
//! VRLobbyResolveAs(male): the game takes the main menu's puppet for that body from now on. Nothing outside the main
//! menu: a V's gender is her save's.
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering};

use red4ext_abi::cname::CName;
use red4ext_abi::function::BaseFunction;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{RttiSystem, object_class};
use red4ext_abi::stack::{CStack, StackFrame, StackType};

use cvr_engine::code;

extern "C" {
    fn cvr_anim_player_entity() -> *mut c_void;
}

const RESOLVED_GENDER_RVA: usize = 0x5EEC80; // sub_1405EEC80: the persistent state's, or `initial`'s
const SET_GENDER_RVA: usize = 0x15B4DCC; // sub_1415B4DCC: gamePuppetPS's, as the game writes it
const REQUEST_GENDER: usize = 72;
const SPAWNER_SYSTEMS: usize = 64;
const JOB_PERSISTENT_ID: usize = 7;
const ENTITY_STATE: usize = 0x168;

const DEFAULT: CName = CName(0x1132_6FD2_590F_4E5E);
const MALE: CName = CName(0x49F8_5AAE_D016_DB90);
const FEMALE: CName = CName(0x040F_253D_3367_6EB9);

/// The trampoline slot (filled by the install pass; registered from cvr-render's engine_detours.rs).
#[no_mangle]
pub static cvr_player_body_spawn_original: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
extern "C" {
    /// CyberpunkVR_DebugPlayerSpawns[3]: main-menu spawns given the man's body, saves repaired, other spawns.
    static mut CyberpunkVR_DebugPlayerSpawns: [u64; 3];
}
static MAIN_MENU_BODY: AtomicBool = AtomicBool::new(false);

fn bump(i: usize) {
    // SAFETY: the export's three counters.
    unsafe {
        let p = core::ptr::addr_of_mut!(CyberpunkVR_DebugPlayerSpawns).cast::<u64>().add(i);
        core::ptr::write_volatile(p, core::ptr::read_volatile(p).wrapping_add(1));
    }
}

/// A script function of `object` with no arguments, its result into `out` (as RED4ext::ExecuteFunction calls it, the
/// engine's Execute under SEH).
unsafe fn call_into<T>(object: *mut c_void, f: &BaseFunction, out: &mut T) -> bool {
    let Some(rtti) = RttiSystem::get() else {
        return false;
    };
    // SAFETY: the function's own return type.
    let Some(ret) = (unsafe { f.return_type.as_ref() }) else {
        return false;
    };
    let mut result = StackType {
        ty: ret.ty,
        value: out as *mut T as *mut c_void,
    };
    let Some(mut stack) = CStack::new(rtti, object, &mut [], &mut result) else {
        return false;
    };
    let Some(address) = f.execute_address() else {
        return false;
    };
    // SAFETY: the engine's Execute of a function of this object's class, inside the SEH adapter.
    unsafe {
        code::guarded_call3(
            address,
            f as *const BaseFunction as *mut c_void,
            &mut stack as *mut CStack as *mut c_void,
            core::ptr::null_mut(),
        )
    }
    .unwrap_or(false)
}

/// The body of the session's character: Some(true) a man's, Some(false) a woman's, None no character (the main menu).
fn character_body_male() -> Option<bool> {
    let rtti = RttiSystem::get()?;
    let class = rtti.get_class(CName::hashed("gameuiICharacterCustomizationSystem"))?;
    let system = red4ext_abi::engine::system_named("gameuiICharacterCustomizationSystem");
    let get_state = class.function(CName::hashed("GetState"))?;
    if system.is_null() {
        return None;
    }
    let mut state = Handle::null();
    // SAFETY: the system is a live instance of the class; the result a Handle<IScriptable>.
    if !unsafe { call_into(system, get_state, &mut state) } || state.is_null() {
        return None;
    }
    // SAFETY: a live scriptable.
    let is_male =
        unsafe { object_class(state.instance) }?.function(CName::hashed("IsBodyGenderMale"))?;
    let mut male = false;
    // SAFETY: as above; a bool result.
    unsafe { call_into(state.instance, is_male, &mut male) }.then_some(male)
}

/// Detour_PlayerSpawn: the request's gender for the main menu's spawn and for a man's save without one.
///
/// # Safety
/// Called by the engine in place of the player's spawn.
#[no_mangle]
pub unsafe extern "C" fn cvr_player_body_spawn(
    spawner: i64,
    job: *mut u64,
    request: i64,
    context: i64,
) -> i64 {
    let gender = (request as usize + REQUEST_GENDER) as *mut CName;
    // SAFETY: the request's gender field, as the C++ detour read and wrote it.
    let asked = unsafe { *gender };
    let body_male = character_body_male();
    let main_menu = asked == DEFAULT && body_male.is_none();
    MAIN_MENU_BODY.store(main_menu, Ordering::Relaxed);
    if main_menu {
        // SAFETY: as above.
        unsafe { *gender = MALE };
        bump(0);
    } else if asked == CName::NONE && body_male == Some(true) {
        let mut resolved = CName::NONE;
        let base = code::image_base();
        // SAFETY: the spawner's game systems and the job's persistent id, the engine's own resolve.
        let resolved_ok = unsafe {
            let systems = *((spawner as usize + SPAWNER_SYSTEMS) as *const usize);
            let id = *job.add(JOB_PERSISTENT_ID);
            code::guarded_call4(
                base + RESOLVED_GENDER_RVA,
                &mut resolved as *mut CName as *mut c_void,
                systems as *mut c_void,
                id as *mut c_void,
                core::ptr::null_mut(),
            )
            .is_some()
        };
        if resolved_ok && resolved == FEMALE {
            cvr_host::log!(
                "[body] a man's save with no gender in its persistent state (a new game on 0.2.0): repaired\n"
            );
            // SAFETY: as above.
            unsafe { *gender = MALE };
            bump(1);
        } else {
            bump(2);
        }
    } else {
        bump(2);
    }
    // SAFETY: the engine's own function, through MinHook's trampoline.
    let result = unsafe {
        let f: unsafe extern "C" fn(i64, *mut u64, i64, i64) -> i64 =
            core::mem::transmute(cvr_player_body_spawn_original.load(Ordering::Acquire));
        f(spawner, job, request, context)
    };
    // SAFETY: the request is the game's, and it is read here alone.
    unsafe { *gender = asked };
    result
}

/// VRLobbyResolveAs(male): the main menu's puppet resolves to that body from now on.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyResolveAs(
    _this: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    let mut male = true;
    // SAFETY: the VM's frame, a Bool parameter.
    if let Some(f) = unsafe { frame.as_mut() } {
        unsafe { f.get_parameter(&mut male) };
        f.finish_params();
    }
    if !MAIN_MENU_BODY.load(Ordering::Relaxed) {
        return;
    }
    // SAFETY: the player entity of the C++ half (main thread).
    let player = unsafe { cvr_anim_player_entity() } as usize;
    let state = if player != 0 {
        code::read::<usize>(player + ENTITY_STATE).unwrap_or(0)
    } else {
        0
    };
    // SAFETY: a live scriptable when non-null.
    let known = state != 0
        && unsafe { object_class(state as *const c_void) }
            .is_some_and(|c| c.is_a(CName::hashed("gamePuppetPS")));
    if !known {
        cvr_host::log!(
            "[body] the main menu's puppet has no persistent state of the known layout\n"
        );
        return;
    }
    let gender = if male { MALE } else { FEMALE };
    // SAFETY: gamePuppetPS's own setter, inside the SEH adapter.
    unsafe {
        code::guarded_call4(
            code::image_base() + SET_GENDER_RVA,
            state as *mut c_void,
            gender.0 as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        );
    }
}
