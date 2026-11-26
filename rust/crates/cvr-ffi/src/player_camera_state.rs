//! The player's flags from RTTI, refreshed from the camera locate (formerly src/Camera/PlayerCameraState.cpp and the
//! cache src/Core/VrCore.cpp InitializeMountedVehicleCache made for it): mounted to a vehicle (and to a car), driving,
//! aiming, a weapon in either hand, the player's scene tier. They are gameplay-rate flags, so they are refreshed once per
//! entity tick (the Lua push's sequence in the shot block's [99]) and every 32nd call when that push is not running.
//!
//! The C linkage name is the one cvr-rig's locate hook already calls (`cvr_cpp_refresh_player_camera_state`, which
//! tools/vrik_tests stands in for); the flags it writes are the C++ half's globals, the two it cannot reach with C
//! linkage through PlayerCameraState.cpp's adapter.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering::*};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::function::Property;
use red4ext_abi::handle::{Handle, WeakHandle};
use red4ext_abi::rtti::{object_class, RttiSystem};
use std::sync::{Mutex, OnceLock};

#[allow(non_upper_case_globals)]
extern "C" {
    static mut g_isInVehicle: bool;
    static mut g_isAiming: bool;
    static mut g_hasWeaponEquipped: bool;
    static mut g_hasRightWeaponEquipped: bool;
    static mut g_hasLeftWeaponEquipped: bool;
    static g_sceneTier: AtomicI32;
    /// g_vehicleState and g_isDriving (src/Core/VrCore.cpp).
    #[link_name = "?g_vehicleState@@3U?$atomic@H@std@@A"]
    static g_vehicleState: AtomicI32;
    #[link_name = "?g_isDriving@@3U?$atomic@_N@std@@A"]
    static g_isDriving: core::sync::atomic::AtomicBool;
    /// OpenXRManager::SetSharedSlot.
    fn cvr_cpp_xr_set_shared_slot(index: i32, value: f32);
    /// GetShotShared (src/Core/VrCore.cpp).
    fn cvr_cpp_shot_shared() -> *mut f32;
    fn cvr_cpp_verbose_log() -> i32;
}

/// PlayerPuppet's properties read here and whether VehicleComponent::IsDriver exists; looked up once.
struct Cache {
    mounted_vehicle: Option<&'static Property>,
    vehicle_state: Option<&'static Property>,
    is_aiming: Option<&'static Property>,
    right_weapon: Option<&'static Property>,
    left_weapon: Option<&'static Property>,
    scene_tier: Option<&'static Property>,
}
// SAFETY: registered RTTI properties live for the process.
unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}
fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| {
        let rtti = RttiSystem::get();
        let puppet = rtti.and_then(|r| r.get_class(CName::hashed("PlayerPuppet")));
        let property = |name: &str| puppet.and_then(|c| c.property_named(name));
        let cache = Cache {
            mounted_vehicle: property("mountedVehicle"),
            vehicle_state: property("vehicleState"),
            is_aiming: property("isAiming"),
            right_weapon: property("equippedRightHandWeapon"),
            left_weapon: property("equippedLeftHandWeapon"),
            // PlayerPuppet carries `sceneTier : GameplayTier` (0 Undefined, 1 Tier1_FullGameplay, 4 Tier4_FPPCinematic,
            // 5 Tier5_Cinematic): one property read instead of the blackboard route.
            scene_tier: property("sceneTier"),
        };
        // THE DRIVER SEAT. The class is registered lower-case in some builds and capitalised in others.
        let driver = ["VehicleComponent", "vehicleComponent"]
            .iter()
            .filter_map(|c| rtti.and_then(|r| r.get_class(CName::hashed(c))))
            .any(|c| c.function(CName::hashed("IsDriver")).is_some());
        // SAFETY: the adapter.
        if unsafe { cvr_cpp_verbose_log() } != 0 {
            log::write(&format!(
                "[VR] VehicleComponent::IsDriver {} -- wheel grab is {}\n",
                if driver { "resolved" } else { "NOT FOUND" },
                if driver {
                    "driver-seat only"
                } else {
                    "requires vehicleState confirmation"
                }
            ));
        }
        cache
    })
}

static MOUNTED_VEHICLE: AtomicUsize = AtomicUsize::new(0);
/// The primary weapon last seen (main's previousWeapon), compared only.
static PREVIOUS_WEAPON: AtomicUsize = AtomicUsize::new(0);
/// CurrentMountedVehicleIdentity: the vehicle the player is mounted to, 0 for none.
#[no_mangle]
pub extern "C" fn cvr_mounted_vehicle_identity() -> usize {
    MOUNTED_VEHICLE.load(Acquire)
}

/// PSMVehicle::Driving and DriverCombat (Anim/VehiclePosePolicy.hpp IsDrivingVehicleState).
fn is_driving_vehicle_state(state: i32) -> bool {
    state == 1 || state == 6
}

/// RefreshPlayerCameraState.
#[no_mangle]
pub extern "C" fn cvr_cpp_refresh_player_camera_state() {
    static REFRESH: Mutex<()> = Mutex::new(());
    let Ok(_guard) = REFRESH.try_lock() else {
        return;
    };
    let c = cache();
    // PERF: gameplay-rate flags, refreshed once per entity tick (the Lua push's sequence bump), with an every-32nd-call
    // fallback for sessions where the VRIK Lua entity push is not running.
    static LAST_SEQ: AtomicU32 = AtomicU32::new((-1.0f32).to_bits());
    static CALLS: AtomicU32 = AtomicU32::new(0);
    let mut refresh = CALLS.fetch_add(1, Relaxed) & 31 == 0;
    // SAFETY: the adapter; the shot block's slots.
    unsafe {
        let shot = cvr_cpp_shot_shared();
        if !shot.is_null() {
            let seq = core::ptr::read_volatile(shot.add(99));
            if seq != f32::from_bits(LAST_SEQ.load(Relaxed)) {
                LAST_SEQ.store(seq.to_bits(), Relaxed);
                refresh = true;
            }
        }
    }
    if !refresh {
        return;
    }
    // SAFETY: the C++ half's globals and adapters, the player's live properties.
    unsafe {
        let mut player = Handle::null();
        if crate::anim_lookup::get_player(&mut player) {
            cvr_rig::vrik::roomscale_move::cvr_roomscale_set_player(player.instance as usize);
        }
        let p = player.instance;
        let mut in_vehicle = false;
        let mut mounted_car = false;
        let mut mounted_bike = false;
        let mut mounted_identity = 0usize;
        if !p.is_null() {
            if let Some(prop) = c.mounted_vehicle {
                let weak = &*prop.value_ptr::<WeakHandle>(p);
                let mounted = weak.lock();
                mounted_identity = mounted.instance as usize;
                in_vehicle = mounted_identity != 0;
                if !mounted.is_null() {
                    let class = object_class(mounted.instance as *const c_void);
                    mounted_car = class.is_some_and(|k| k.is_a(CName::hashed("vehicleCarBaseObject")));
                    mounted_bike = class.is_some_and(|k| k.is_a(CName::hashed("vehicleBikeBaseObject")));
                }
            }
        }
        core::ptr::write_volatile(&raw mut g_isInVehicle, in_vehicle);
        cvr_rig::vrik::wheel_grab::cvr_anim_wheel_set_mounted_car(mounted_car);
        cvr_rig::vrik::wheel_grab::cvr_anim_wheel_set_mounted_bike(mounted_bike);
        MOUNTED_VEHICLE.store(mounted_identity, Release);
        // PlayerPuppet::vehicleState mirrors PSM.Vehicle. Combat is passenger window combat; DriverCombat is a
        // separate state. A missing IsDriver function must never classify every mounted passenger as the driver.
        let vehicle_state = match (p.is_null(), c.vehicle_state) {
            (false, Some(prop)) => *prop.value_ptr::<i32>(p),
            _ => -1,
        };
        let mut driving = false;
        if in_vehicle {
            if vehicle_state >= 0 {
                driving = is_driving_vehicle_state(vehicle_state);
            } else if !p.is_null() {
                let r = is_driver(&player);
                if r >= 0 {
                    driving = r != 0;
                }
            }
        }
        g_vehicleState.store(vehicle_state, Relaxed);
        g_isDriving.store(driving, Relaxed);
        if let (false, Some(prop)) = (p.is_null(), c.is_aiming) {
            core::ptr::write_volatile(&raw mut g_isAiming, *prop.value_ptr::<bool>(p));
        }
        let mut primary_weapon = 0usize;
        if let (false, Some(prop)) = (p.is_null(), c.right_weapon) {
            let weapon = (*prop.value_ptr::<WeakHandle>(p)).instance as usize;
            core::ptr::write_volatile(&raw mut g_hasRightWeaponEquipped, weapon != 0);
            primary_weapon = weapon;
        }
        if let (false, Some(prop)) = (p.is_null(), c.left_weapon) {
            let weapon = (*prop.value_ptr::<WeakHandle>(p)).instance as usize;
            core::ptr::write_volatile(&raw mut g_hasLeftWeaponEquipped, weapon != 0);
            if primary_weapon == 0 {
                primary_weapon = weapon;
            }
        }
        // THE PRIMARY WEAPON'S REVISION (main's PrimaryWeaponRevision): a trigger held through an equip never presses for
        // the new weapon. Compared only, under the refresh's lock.
        if !p.is_null() && primary_weapon != PREVIOUS_WEAPON.load(Relaxed) {
            PREVIOUS_WEAPON.store(primary_weapon, Relaxed);
            cvr_rig::vrik::xinput::PRIMARY_WEAPON_REVISION.fetch_add(1, Relaxed);
        }
        // A WEAPON IN HAND is the right hand's -- or, left-handed, the left hand's alone.
        let weapon = core::ptr::read_volatile(&raw const g_hasRightWeaponEquipped)
            || cvr_rig::vrik::weapon_arm::cvr_anim_weapon_in_left_hand();
        core::ptr::write_volatile(&raw mut g_hasWeaponEquipped, weapon);
        // The weapon flag in [144]; the in-vehicle flag in [31] (the VRIK hook keeps the body chain off while seated).
        cvr_cpp_xr_set_shared_slot(144, if weapon { 1.0 } else { 0.0 });
        cvr_cpp_xr_set_shared_slot(31, if in_vehicle { 1.0 } else { 0.0 });
        // CUTSCENE SUSPEND, producer half: the player's own scene tier, read by the property's own size so a future
        // widening cannot silently read three bytes of something else. It goes in a global, not a shared slot (the
        // button publish overwrote slots [157]/[158]).
        if let (false, Some(prop)) = (p.is_null(), c.scene_tier) {
            let at = prop.value_ptr::<u8>(p);
            let size = prop.ty.as_ref().map_or(4, |t| t.size());
            let tier = if at.is_null() {
                0
            } else if size == 1 {
                *(at as *const i8) as i32
            } else if size == 2 {
                core::ptr::read_unaligned(at as *const i16) as i32
            } else {
                core::ptr::read_unaligned(at as *const i32)
            };
            g_sceneTier.store(tier, Relaxed);
        }
    }
}

/// VehicleComponent::IsDriver(GameInstance, player) (formerly src/Camera/PlayerCameraState.cpp's): 1 or 0, -1 without
/// the function (looked up once, as the C++ static was) or when the call fails.
///
/// # Safety
/// `player` is a live handle.
unsafe fn is_driver(player: *const Handle) -> i32 {
    use red4ext_abi::stack::{CStack, StackType};
    static FUNCTION: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let Some(rtti) = red4ext_abi::rtti::RttiSystem::get() else { return -1 };
    let found = *FUNCTION.get_or_init(|| {
        ["VehicleComponent", "vehicleComponent"]
            .iter()
            .filter_map(|c| rtti.get_class(CName::hashed(c)))
            .find_map(|c| c.function(CName::hashed("IsDriver")))
            .map_or(0, |f| f as *const red4ext_abi::function::BaseFunction as usize)
    });
    // SAFETY: a registered function, for the process.
    let Some(f) = (unsafe { (found as *const red4ext_abi::function::BaseFunction).as_ref() }) else { return -1 };
    if player.is_null() || unsafe { (*player).instance.is_null() } {
        return -1;
    }
    #[repr(C)]
    struct ScriptGameInstance {
        instance: *mut c_void,
        unk8: i8,
        unk10: i64,
    }
    let game = ScriptGameInstance { instance: red4ext_abi::engine::game_instance(), unk8: 1, unk10: 0 };
    // The arguments untyped, as the C++ passed them (StackArgs_t's emplace_back(nullptr, value)).
    let mut args = [
        StackType { ty: core::ptr::null(), value: (&game as *const ScriptGameInstance).cast_mut().cast() },
        StackType { ty: core::ptr::null(), value: player.cast_mut().cast() },
    ];
    let mut out = false;
    let mut result = StackType {
        // SAFETY: the function's own return property, or none.
        ty: unsafe { f.return_type.as_ref() }.map_or(core::ptr::null(), |r| r.ty),
        value: (&raw mut out).cast(),
    };
    // SAFETY: as above.
    let result_ptr = if unsafe { f.return_type.as_ref() }.is_none() { core::ptr::null_mut() } else { &mut result as *mut StackType };
    let Some(mut stack) = CStack::new(rtti, core::ptr::null_mut(), &mut args, result_ptr) else { return -1 };
    let Some(address) = f.execute_address() else { return -1 };
    type Execute = unsafe extern "C" fn(*mut c_void, *mut CStack, *mut c_void) -> bool;
    // SAFETY: CBaseFunction::Execute(stack, none) over the function and the stack built above.
    let ran = unsafe { core::mem::transmute::<usize, Execute>(address)((f as *const red4ext_abi::function::BaseFunction).cast_mut().cast(), &mut stack, core::ptr::null_mut()) };
    if ran { i32::from(out) } else { -1 }
}
