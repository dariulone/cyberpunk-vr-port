//! The physics hands in a car (CVR_CAR_HANDS_TEST=1; item 6 of 2026-11-23's list): a car spawned beside the player,
//! the player mounted in its driver's seat at once (MountingFacility.Mount, an instant MountEventData), then the hands
//! measured for 8 s as the settings have them (xr_hand_collision_vehicle) and 60 s allowed there whatever they say
//! (physhands::worker::IN_VEHICLE): each second whether a hand is wanted, touching, on the world, how far its body is
//! from where it was sent and how many shapes the car's cabin gave the hands (physhands::cabin); once, every shape round
//! the seat logged; then the player dismounted and the car deleted.
use super::lobby_rtti::{self as rt, A};
use cvr_host::log;
use cvr_rig::vrik::physhands::worker::{IN_VEHICLE, STEPS, WANTED};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

struct Test {
    frames: u32,
    stage: u8,
    spawned: u64,
    wait: u32,
    worst: [f32; 2],
    touch: [u32; 2],
    world: [u32; 2],
    samples: u32,
}
static TEST: Mutex<Test> = Mutex::new(Test { frames: 0, stage: 0, spawned: 0, wait: 0, worst: [0.0; 2], touch: [0; 2], world: [0; 2], samples: 0 });
static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
/// How long the hands are measured allowed in the car (frames): long enough for a test to reach into the cabin.
const HANDS_ON_FRAMES: u32 = 3600;

fn in_vehicle() -> bool {
    // SAFETY: a plain read of the plugin's flag.
    unsafe { core::ptr::read_volatile(&raw const cvr_rig::vrik::env::g_isInVehicle) }
}
fn eid(h: &Handle) -> u64 {
    super::melee::entity_id(h)
}
/// MountingFacility.Mount with an instant mount into `slot` of `car`.
fn mount(pl: &Handle, car: &Handle, slot: &str) -> Result<(), String> {
    let req = super::lobby_ink::new("MountingRequest");
    let data = super::lobby_ink::new("MountEventData");
    if req.is_null() || data.is_null() {
        return Err("no MountingRequest / MountEventData".into());
    }
    super::menu_rt::hset(&data, "isInstant", 1.0);
    if let Some(f) = rt::field(&data, "slotName") {
        // SAFETY: a CName field.
        unsafe { *(f.ptr as *mut u64) = CName::hashed(slot).0 };
    }
    if let Some(f) = rt::field(&data, "mountParentEntityId") {
        // SAFETY: an EntityID field (one 64-bit hash).
        unsafe { *(f.ptr as *mut u64) = eid(car) };
    }
    let info = rt::field(&req, "lowLevelMountingInfo").ok_or("no lowLevelMountingInfo")?;
    let set_id = |name: &str, v: u64| {
        if let Some(f) = info.f(name) {
            // SAFETY: an EntityID (one 64-bit hash).
            unsafe { *(f.ptr as *mut u64) = v };
        }
    };
    set_id("parentId", eid(car));
    set_id("childId", eid(pl));
    if let Some(slot_id) = info.f("slotId").and_then(|s| s.f("id")) {
        // SAFETY: a CName.
        unsafe { *(slot_id.ptr as *mut u64) = CName::hashed(slot).0 };
    }
    super::menu_main::set_handle_prop(&req, "mountData", &data);
    let facility = super::menu_rt::static_gi("GameInstance", "GetMountingFacility", &[]).map(|o| super::lobby_rtti::Val::of(&o).handle()).unwrap_or_else(Handle::null);
    if facility.is_null() {
        return Err("no mounting facility".into());
    }
    rt::call(&facility, "Mount", &[A::H(&req)]).ok_or("Mount could not be called")?;
    Ok(())
}
fn unmount(pl: &Handle, car: &Handle) {
    let req = super::lobby_ink::new("UnmountingRequest");
    if req.is_null() {
        return;
    }
    if let Some(info) = rt::field(&req, "lowLevelMountingInfo") {
        if let Some(f) = info.f("parentId") {
            // SAFETY: an EntityID.
            unsafe { *(f.ptr as *mut u64) = eid(car) };
        }
        if let Some(f) = info.f("childId") {
            // SAFETY: an EntityID.
            unsafe { *(f.ptr as *mut u64) = eid(pl) };
        }
    }
    if let Some(facility) = super::menu_rt::static_gi("GameInstance", "GetMountingFacility", &[]).map(|o| super::lobby_rtti::Val::of(&o).handle()) {
        rt::call(&facility, "Unmount", &[A::H(&req)]);
    }
}
/// One second's look at the hands: wanted, touching, on the world, the body's distance from where it was sent.
fn sample(t: &mut Test, label: &str) {
    let w = WANTED.read();
    let Some(st) = STEPS.read() else { return };
    let mut parts = Vec::new();
    for side in 0..2 {
        // pose and sent are rotation x y z w, then position
        let d = (4..7).map(|k| (st.pose[side][k] - st.sent[side][k]).powi(2)).sum::<f32>().sqrt();
        if st.valid[side] {
            t.worst[side] = t.worst[side].max(d);
            t.touch[side] += u32::from(st.touching[side]);
            t.world[side] += u32::from(st.on_world[side]);
        }
        // SAFETY: plain reads of the physics thread's words (a torn read only garbles a log line).
        let (filter, simulating) = unsafe {
            (
                core::ptr::read_volatile(&raw const cvr_rig::vrik::physhands::body::filter_now[side]),
                core::ptr::read_volatile(&raw const cvr_rig::vrik::physhands::body::simulating[side]),
            )
        };
        parts.push(format!(
            "{} wanted {} valid {} simulating {} touching {} on the world {} off its target {:.3} m, its body at ({:.2} {:.2} {:.2}), palm mask {:08x}{:08x}",
            if side == 0 { "left" } else { "right" },
            w.is_some_and(|w| w.on[side]),
            st.valid[side],
            simulating,
            st.touching[side],
            st.on_world[side],
            d,
            st.pose[side][4],
            st.pose[side][5],
            st.pose[side][6],
            filter[3],
            filter[2]
        ));
    }
    t.samples += 1;
    let cabin = cvr_rig::vrik::physhands::cabin::CABIN_SHAPES.load(core::sync::atomic::Ordering::Relaxed);
    log!("[car-hands] {label}: {}; the cabin {} shapes\n", parts.join("; "), cabin);
}

pub(in crate::mechanics) fn tick() {
    if !*ON.get_or_init(|| std::env::var("CVR_CAR_HANDS_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    let pl = super::menu_rt::player();
    if pl.is_null() || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) || !super::menu_rt::b(&pl, "IsAttached", &[]) {
        return;
    }
    let mut t = TEST.lock().unwrap_or_else(|p| p.into_inner());
    t.frames += 1;
    let car = if t.spawned != 0 { crate::spawn_system::spawned(t.spawned) } else { Handle::null() };
    match t.stage {
        0 if t.frames >= 600 => {
            let at = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).unwrap_or([0.0; 4]);
            let f = super::belt::m::<[f32; 4]>(&pl, "GetWorldForward", &[]).unwrap_or([0.0, 1.0, 0.0, 0.0]);
            let right = [f[1], -f[0]];
            let pos = [at[0] + right[0] * 2.6 + f[0] * 0.5, at[1] + right[1] * 2.6 + f[1] * 0.5, at[2], 1.0];
            let yaw = (-f[0]).atan2(f[1]);
            let q = [0.0, 0.0, (yaw * 0.5).sin(), (yaw * 0.5).cos()];
            t.spawned = crate::spawn_system::spawn_record(red4ext_abi::tweakdbid::from_name("Vehicle.v_standard2_archer_hella_player"), &pos, &q);
            log!("[car-hands] a car spawned: {}\n", t.spawned);
            t.stage = 1;
        }
        1 => {
            t.wait += 1;
            if car.is_null() || t.wait < 400 {
                if t.wait > 1500 {
                    log!("[car-hands] no car: stopped\n");
                    t.stage = 9;
                }
                return;
            }
            match mount(&pl, &car, "seat_front_left") {
                Ok(()) => log!("[car-hands] the player's mount into the driver's seat asked\n"),
                Err(e) => {
                    log!("[car-hands] the mount failed: {e}\n");
                    t.stage = 9;
                    return;
                }
            }
            t.stage = 2;
            t.wait = 0;
        }
        2 => {
            t.wait += 1;
            if !in_vehicle() {
                if t.wait > 900 {
                    log!("[car-hands] the player is not in the car 15 s after the mount: stopped\n");
                    t.stage = 8;
                    t.wait = 0;
                }
                return;
            }
            log!("[car-hands] in the car after {} frames; the physics hands as the settings have them\n", t.wait);
            t.stage = 3;
            t.wait = 0;
        }
        3 => {
            t.wait += 1;
            if t.wait % 60 == 0 {
                sample(&mut t, "as set");
            }
            if t.wait >= 480 {
                IN_VEHICLE.store(true, core::sync::atomic::Ordering::Relaxed);
                log!("[car-hands] the physics hands allowed in the vehicle\n");
                t.worst = [0.0; 2];
                t.touch = [0; 2];
                t.world = [0; 2];
                t.samples = 0;
                t.stage = 4;
                t.wait = 0;
            }
        }
        4 => {
            t.wait += 1;
            if t.wait == 30 {
                // every shape round the seat: what the car's cabin is made of in PhysX, and its filter words
                let at = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).unwrap_or([0.0; 4]);
                cvr_rig::vrik::physhands::api::test_stack(&[at[0], at[1], at[2] + 0.8], true, true);
                log!("[car-hands] the shapes round the seat asked ({:.2} {:.2} {:.2})\n", at[0], at[1], at[2] + 0.8);
            }
            if t.wait % 60 == 0 {
                sample(&mut t, "on");
            }
            if t.wait >= HANDS_ON_FRAMES {
                log!(
                    "[car-hands] with the physics hands on in the car, over {} s: the bodies at most {:.3} / {:.3} m off their targets, touching {} / {} s, on the world {} / {} s\n",
                    t.samples, t.worst[0], t.worst[1], t.touch[0], t.touch[1], t.world[0], t.world[1]
                );
                IN_VEHICLE.store(false, core::sync::atomic::Ordering::Relaxed);
                t.stage = 8;
                t.wait = 0;
            }
        }
        8 => {
            t.wait += 1;
            if t.wait == 1 {
                unmount(&pl, &car);
            }
            if t.wait == 240 {
                let deleted = crate::spawn_system::delete_spawned(t.spawned);
                log!("[car-hands] the car deleted: {deleted}\n");
                log!("[car-hands] done\n");
                t.stage = 9;
            }
        }
        _ => {}
    }
}
