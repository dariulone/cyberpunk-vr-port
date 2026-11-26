//! The port's native tweaks: what its YAML records cannot say, run right after them with the same TweakDB
//! (src/Hooks/RoomscaleTweaks.cpp and src/Natives/LauncherReadyState.cpp in the C++ build).
//!
//! THE CAPSULE A HEADSET STANDS IN. Every on-foot locomotion state gives the player a 0.40 m radius, and the controller
//! keeps another 7 cm of contact offset: the body's axis stops 0.47 m from a wall, the shoulders with it, and a hand on
//! an arm measured from those shoulders could not reach a button on that wall. 0.25 m brings the axis to 0.32 m, where
//! the arm reaches past the surface, and the view's lead over a blocked capsule shrinks with it. The states on foot
//! only; swimming, grapples and takedowns, braindances and scene tiers keep the game's radius.
//!
//! NOT ON A LADDER. The ladder's exit over the top carries the capsule forward by what a 0.40 m one needs to stand on
//! the floor beyond; a 0.25 m capsule was left over the gap between the ladder and that floor and fell -- the whole
//! height of a long ladder (players, 2026-11-05, a 9.1 m ladder with exitStepTop -1: V reached the floor level, moved
//! 12 cm and dropped 9 m; with the ladder states at 0.40 it walked off onto the roof). Nothing on a ladder needs the
//! shoulders nearer a wall, so all five ladder states keep the game's radius.
//!
//! THE LAUNCHER'S READY STATE, IN THE LEFT-HAND CYBERWARE STATE MACHINE'S DEFINITION. The arm launcher waits in a
//! state of the port's own, vrLauncherReady (VrLauncherReadyDecisions / VrLauncherReadyEvents in
//! mods/redscript/CyberpunkVRPort_Cyberware/vrport_cyberware.reds), and the machine only knows a state its definition
//! names: the native PSM loader enumerates playerStateMachineDefinitions.LeftHandCyberware.states (loader RVA
//! 0x674E5C), and a transition target alone does not register one. So the definition gets the state as the game writes
//! its own: its name in the list -- the package-local name, as a qualified one resolves OnEnter but not the outgoing
//! transitions -- its flats beside the others', taken from leftHandCyberwareWaitForUnequip where they are the same,
//! and an edge to it at the head of each state it is entered from.

use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicPtr, Ordering};

use cvr_host::log;
use red4ext_abi::cstring::CString as EngineString;
use red4ext_abi::tweakdbid;

use crate::tweak_records::{
    cstr, rt_array_element, rt_array_insert_at, rt_array_length,
    rt_array_resize, rt_cstring_assign, rt_type_assign, flat, type_name, FlatWrites,
    Value,
};

// The RTTI system's types and TweakDB's instance and flat lock (formerly TweakRecords.cpp's adapters), through
// red4ext-abi.
/// `rtti ? rtti->GetType(CName(name)) : nullptr`.
///
/// # Safety
/// `name` is a NUL-terminated text.
pub(crate) unsafe fn rt_rtti_type(name: *const c_char) -> *mut c_void {
    // SAFETY: as documented.
    let name = unsafe { core::ffi::CStr::from_ptr(name) }.to_str().unwrap_or("");
    red4ext_abi::rtti::RttiSystem::get()
        .and_then(|rtti| rtti.get_type(red4ext_abi::cname::CName::hashed(name)))
        .map_or(core::ptr::null_mut(), |ty| ty as *const _ as *mut c_void)
}
/// `TweakDB::Get()`.
///
/// # Safety
/// None beyond the engine's.
pub(crate) unsafe fn rt_tdb_get() -> *mut c_void {
    red4ext_abi::tweakdb::get()
}
/// `db->mutex00.lock_shared()`.
///
/// # Safety
/// `db` is the live TweakDB.
pub(crate) unsafe fn rt_tdb_flats_lock_shared(db: *mut c_void) {
    // SAFETY: as documented.
    unsafe { red4ext_abi::tweakdb::SharedSpinLock::of(db, red4ext_abi::tweakdb::FLATS_LOCK) }.lock_shared();
}
/// `db->mutex00.unlock_shared()`.
///
/// # Safety
/// `db` is the live TweakDB, its flats held shared by this thread.
pub(crate) unsafe fn rt_tdb_flats_unlock_shared(db: *mut c_void) {
    // SAFETY: as documented.
    unsafe { red4ext_abi::tweakdb::SharedSpinLock::of(db, red4ext_abi::tweakdb::FLATS_LOCK) }.unlock_shared();
}

fn rtti_type(name: &str) -> *mut c_void {
    let name = cstr(name);
    // SAFETY: CRTTISystem::GetType by name; null when the type is not there.
    unsafe { rt_rtti_type(name.as_ptr()) }
}

// ---- the capsule

const VR_CAPSULE_RADIUS: f32 = 0.25;
const ON_FOOT_STATES: &[(&str, &[&str])] = &[
    (
        "playerStateMachineBaseLocomotion",
        &[
            "baseLocomotionState",
            "locomotionAir",
            "locomotionDash",
            "locomotionGround",
        ],
    ),
    (
        "playerStateMachineBaseLocomotionLowGravity",
        &[
            "baseLocomotionStateLowGravity",
            "locomotionAirLowGravity",
            "locomotionGroundLowGravity",
        ],
    ),
    (
        "playerStateMachineLocomotion",
        &[
            "abstractStatusEffect",
            "aimWalk",
            "airHover",
            "airThrusters",
            "bodySlamJump",
            "chargeJump",
            "climb",
            "coolExitJump",
            "crouch",
            "crouchSprint",
            "deathLand",
            "dodge",
            "dodgeAir",
            "doubleJump",
            "failedLandingAbstract",
            "fall",
            "felled",
            "forceFreeze",
            "forceIdle",
            "forceWalk",
            "forcedKnockdown",
            "hardLand",
            "hoverJump",
            "jump",
            "knockdown",
            "landingAbstract",
            "regularLand",
            "slide",
            "slideAfterFall",
            "slideFall",
            "sprint",
            "stand",
            "stunned",
            "superheroFall",
            "superheroLand",
            "superheroLandRecovery",
            "unsecureFootingFall",
            "vault",
            "vehicleKnockdown",
            "veryHardLand",
            "workspot",
        ],
    ),
    (
        "playerStateMachineLocomotionLowGravity",
        &[
            "climbLowGravity",
            "crouchLowGravity",
            "dodgeAirLowGravity",
            "dodgeCrouchLowGravity",
            "dodgeLowGravity",
            "fallLowGravity",
            "jumpLowGravity",
            "preCrouchLowGravity",
            "regularLandLowGravity",
            "slideLowGravity",
            "sprintJumpLowGravity",
            "sprintLowGravity",
            "sprintWindupLowGravity",
            "standLowGravity",
        ],
    ),
    ("playerStateMachineMelee", &["meleeGroundSlamAttack"]),
];

/// ShrinkOnFootCapsules: the on-foot states' capsule radius down to 0.25 m where it is larger.
///
/// # Safety
/// `db` is the TweakDB the engine just loaded.
pub unsafe extern "C" fn shrink_on_foot_capsules(db: *mut c_void) {
    let float = rtti_type("Float");
    let mut writes = FlatWrites::new(db);
    let (mut changed, mut missing) = (0, 0);
    for (package, states) in ON_FOOT_STATES {
        for state in *states {
            let id = tweakdbid::from_name(&format!("{package}.{state}.capsuleRadius"));
            let Some((_, value)) =
                flat(db, id).filter(|&(ty, value)| ty == float && !value.is_null())
            else {
                missing += 1;
                continue;
            };
            // SAFETY: a Float flat's value.
            if !(unsafe { *(value as *const f32) } > VR_CAPSULE_RADIUS) {
                continue;
            }
            let mut radius = VR_CAPSULE_RADIUS;
            if writes.set(id, float, (&mut radius as *mut f32).cast()) {
                changed += 1;
            }
        }
    }
    log!("[tweaks] on-foot capsule radius {VR_CAPSULE_RADIUS:.2} m: {changed} states set, {missing} not found\n");
}

static FLOAT_TYPE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());

/// The player's capsule radius as the locomotion states set it (stand, crouch, sprint, slide, jump, fall... all one),
/// or 0 while TweakDB is not there or the value is implausible. The roomscale move (cvr-rig roomscale_move) reads it.
#[no_mangle]
pub extern "C" fn cvr_tweak_capsule_radius() -> f32 {
    // SAFETY: TweakDB::Get; null before the engine made it.
    let db = unsafe { rt_tdb_get() };
    if db.is_null() {
        return 0.0;
    }
    let mut float = FLOAT_TYPE.load(Ordering::Relaxed);
    if float.is_null() {
        float = rtti_type("Float");
        FLOAT_TYPE.store(float, Ordering::Relaxed);
    }
    let id = tweakdbid::from_name("playerStateMachineLocomotion.stand.capsuleRadius");
    // SAFETY: the flats read under their shared lock, as the C++ build reads them.
    let radius = unsafe {
        rt_tdb_flats_lock_shared(db);
        let radius = flat(db, id)
            .filter(|&(ty, value)| ty == float && !value.is_null())
            .map(|(_, value)| *(value as *const f32));
        rt_tdb_flats_unlock_shared(db);
        radius
    };
    match radius {
        Some(r) if r.is_finite() && r > 0.1 && r < 2.0 => r,
        _ => 0.0,
    }
}

// ---- the launcher's ready state

const ROOT: &str = "playerStateMachineLeftHandCyberware.";
const STATE: &str = "vrLauncherReady";

/// An array:String flat's value.
fn strings(db: *mut c_void, id: u64) -> Option<(*mut c_void, *mut c_void)> {
    flat(db, id).filter(|&(ty, value)| !value.is_null() && type_name(ty) == "array:String")
}
fn length(ty: *mut c_void, array: *mut c_void) -> u32 {
    // SAFETY: the array type's own accessor on a live instance.
    unsafe { rt_array_length(ty, array) }
}
fn contains(ty: *mut c_void, array: *mut c_void, text: &str) -> bool {
    (0..length(ty, array)).any(|i| {
        // SAFETY: an array:String's element is a CString.
        let element = unsafe { &*(rt_array_element(ty, array, i) as *const EngineString) };
        element.as_str() == text
    })
}
/// A copy of `source` (an array of `ty`) to edit and write.
fn copy_of(ty: *mut c_void, source: *mut c_void) -> Value {
    let value = Value::new(ty);
    // SAFETY: the type's own assignment between two of its values.
    unsafe { rt_type_assign(ty, value.memory, source) };
    value
}
fn put_text(ty: *mut c_void, array: *mut c_void, at_front: bool, text: &str) {
    let text = cstr(text);
    // SAFETY: the array type's own accessors; the new element is a constructed CString.
    unsafe {
        let index = if at_front {
            rt_array_insert_at(ty, array, 0);
            0
        } else {
            let n = rt_array_length(ty, array);
            rt_array_resize(ty, array, n + 1);
            n
        };
        rt_cstring_assign(rt_array_element(ty, array, index), text.as_ptr());
    }
}

struct Types {
    strings: *mut c_void,
    bools: *mut c_void,
    boolean: *mut c_void,
}

/// The edge to the ready state, first of `state`'s transitions. A definition whose lists disagree is left alone
/// rather than guessed at.
fn add_ready_edge(db: *mut c_void, writes: &mut FlatWrites, types: &Types, state: &str) {
    let prefix = format!("{ROOT}{state}.");
    let to = tweakdbid::from_name(&format!("{prefix}transitionTo"));
    let condition = tweakdbid::from_name(&format!("{prefix}transitionCondition"));
    let (Some((to_ty, targets)), Some((condition_ty, conditions))) =
        (strings(db, to), strings(db, condition))
    else {
        return;
    };
    let n = length(to_ty, targets);
    if n == 0 || n != length(condition_ty, conditions) || contains(to_ty, targets, STATE) {
        return;
    }
    let new_targets = copy_of(types.strings, targets);
    put_text(types.strings, new_targets.memory, true, STATE);
    let new_conditions = copy_of(types.strings, conditions);
    put_text(types.strings, new_conditions.memory, true, "=");
    writes.set(to, types.strings, new_targets.memory);
    writes.set(condition, types.strings, new_conditions.memory);
    for field in [
        "transitionConditionOverridesEnterCondition",
        "transitionSkipInternalCondition",
    ] {
        let id = tweakdbid::from_name(&format!("{prefix}{field}"));
        let Some((ty, flags)) = flat(db, id) else {
            continue;
        };
        if ty != types.bools || length(ty, flags) == 0 {
            continue;
        }
        let count = length(ty, flags);
        let new_flags = Value::new(types.bools);
        // SAFETY: an array:Bool's elements are bools; the new array holds one more than the old.
        unsafe {
            rt_array_resize(types.bools, new_flags.memory, count + 1);
            *(rt_array_element(types.bools, new_flags.memory, 0) as *mut bool) = false;
            for i in 0..count {
                let b = *(rt_array_element(ty, flags, i) as *const bool);
                *(rt_array_element(types.bools, new_flags.memory, i + 1) as *mut bool) = b;
            }
        }
        writes.set(id, types.bools, new_flags.memory);
    }
}

/// AddLauncherReadyState: vrLauncherReady in the left-hand cyberware machine's definition, with its flats and the
/// edges to it.
///
/// # Safety
/// `db` is the TweakDB the engine just loaded.
pub unsafe extern "C" fn add_launcher_ready_state(db: *mut c_void) {
    let types = Types {
        strings: rtti_type("array:String"),
        bools: rtti_type("array:Bool"),
        boolean: rtti_type("Bool"),
    };
    if types.strings.is_null() || types.bools.is_null() || types.boolean.is_null() {
        log!("[tweaks] no array:String, array:Bool or Bool type: no launcher ready state\n");
        return;
    }
    let mut writes = FlatWrites::new(db);
    let states_flat =
        tweakdbid::from_name("playerStateMachineDefinitions.LeftHandCyberware.states");
    let Some((states_ty, states)) = strings(db, states_flat).filter(|&(ty, v)| length(ty, v) > 0)
    else {
        log!("[tweaks] the left-hand cyberware machine lists no states: no launcher ready state\n");
        return;
    };
    // The states it is entered from: the machine's own, as listed before the ready state joins them.
    let from = [
        "leftHandCyberwareEquip",
        "leftHandCyberwareQuickAction",
        "leftHandCyberwareChargeAction",
        "leftHandCyberwareChargeRepeatAction",
        "leftHandCyberwareWaitForUnequip",
    ];
    if !contains(states_ty, states, STATE) {
        let listed = copy_of(types.strings, states);
        put_text(types.strings, listed.memory, false, STATE);
        writes.set(states_flat, types.strings, listed.memory);
    }
    let ready = format!("{ROOT}{STATE}.");
    let like = format!("{ROOT}leftHandCyberwareWaitForUnequip.");
    for field in [
        "stateBodyClassName",
        "affinity",
        "alias",
        "forceInputContext",
        "transitionConditionOverridesEnterCondition",
        "transitionSkipInternalCondition",
    ] {
        if let Some((ty, value)) = flat(db, tweakdbid::from_name(&format!("{like}{field}"))) {
            writes.set(tweakdbid::from_name(&format!("{ready}{field}")), ty, value);
        }
    }
    for (field, on) in [
        ("hasEnterCondition", true),
        ("hasExitCondition", false),
        ("hasOnEnter", true),
        ("hasOnExit", true),
        ("hasOnUpdate", true),
    ] {
        let mut value = on;
        writes.set(
            tweakdbid::from_name(&format!("{ready}{field}")),
            types.boolean,
            (&mut value as *mut bool).cast(),
        );
    }
    let targets = Value::new(types.strings);
    let conditions = Value::new(types.strings);
    for target in [
        "leftHandCyberwareUnequip",
        "leftHandCyberwareCharge",
        "leftHandCyberwareQuickAction",
    ] {
        put_text(types.strings, targets.memory, false, target);
        put_text(types.strings, conditions.memory, false, "=");
    }
    writes.set(
        tweakdbid::from_name(&format!("{ready}transitionTo")),
        types.strings,
        targets.memory,
    );
    writes.set(
        tweakdbid::from_name(&format!("{ready}transitionCondition")),
        types.strings,
        conditions.memory,
    );
    for state in from {
        add_ready_edge(db, &mut writes, &types, state);
    }
    writes.commit();
}
