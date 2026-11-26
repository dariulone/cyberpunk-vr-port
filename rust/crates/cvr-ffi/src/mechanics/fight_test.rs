//! FIGHT TEST (tests only, CVR_FIGHT_TEST=1): a live freeze repro with no CET in the process. Once the loaded world
//! runs: god mode (Invulnerable and Immortal) put on the player and READ BACK; only when both hold, an item dropped
//! before the player's feet (a pick-up interaction) and the police called with heat 3 (the prevention system's console
//! instruction) for CVR_FIGHT_TEST_SECONDS (default 90), then the heat taken off. Every step is logged with the QPC
//! clock in seconds, to line the game's events up with SteamVR's frame timings.
use super::lobby_rtti::{self as rt, Val, A};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;

fn n(s: &str) -> A<'static> {
    A::Name(CName::hashed(s).0)
}
fn qpc_s() -> f64 {
    #[link(name = "kernel32")]
    extern "system" {
        fn QueryPerformanceCounter(count: *mut i64) -> i32;
        fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
    }
    let (mut c, mut f) = (0i64, 1i64);
    // SAFETY: two out integers.
    unsafe {
        QueryPerformanceCounter(&mut c);
        QueryPerformanceFrequency(&mut f);
    }
    c as f64 / f as f64
}
fn god_modes(pl: &Handle) -> (bool, bool) {
    let god = super::game_system("GetGodModeSystem");
    let id = [super::melee::entity_id(pl), 0u64];
    let has = |m: &str| {
        !god.is_null() && rt::call(&god, "HasGodMode", &[A::P(id.as_ptr().cast()), A::N(rt::enum_value("gameGodModeType", m))]).is_some_and(|o| Val::of(&o).bool())
    };
    (has("Invulnerable"), has("Immortal"))
}
fn add_god_mode(pl: &Handle) -> String {
    let god = super::game_system("GetGodModeSystem");
    let id = [super::melee::entity_id(pl), 0u64];
    let mut said = Vec::new();
    for m in ["Invulnerable", "Immortal"] {
        let v = rt::enum_value("gameGodModeType", m);
        let ok = rt::call(&god, "AddGodMode", &[A::P(id.as_ptr().cast()), A::N(v), n("cvr_fight_test")]).is_some_and(|o| Val::of(&o).bool());
        said.push(format!("{m}={v} added {ok}"));
    }
    said.join(", ")
}
fn prevention() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    rt::obj(&c, "Get", &[n("PreventionSystem")])
}
fn heat(stage: &str) -> bool {
    let ps = prevention();
    let req = super::lobby_ink::new("PreventionConsoleInstructionRequest");
    if ps.is_null() || req.is_null() {
        return false;
    }
    let active = stage != "Heat_0";
    super::menu_rt::hset(&req, "instruction", super::menu_rt::ev("EPreventionSystemInstruction", if active { "Active" } else { "Safe" }));
    super::menu_rt::hset(&req, "heatStage", super::menu_rt::ev("EPreventionHeatStage", stage));
    rt::call(&ps, "QueueRequest", &[A::H(&req)]);
    true
}
fn heat_now() -> f64 {
    let ps = prevention();
    if ps.is_null() {
        return -1.0;
    }
    rt::num(&ps, "GetHeatStageAsInt", &[]).unwrap_or(-1.0)
}
fn drop_item(pl: &Handle) -> bool {
    let ts = super::belt::transactions();
    let Some(rid) = super::belt::st::<[u64; 2]>("ItemID", "FromTDBID", &[super::belt::tdb("Items.Preset_Lexington_Default")]) else { return false };
    let given = super::belt::m::<bool>(&ts, "GiveItem", &[Value::Handle(pl), Value::Ptr(rid.as_ptr().cast()), Value::Int32(1)]).unwrap_or(false);
    rt::static_call("ItemActionsHelper", "DropItem", &[A::H(pl), A::P(rid.as_ptr().cast())]);
    given
}

pub(super) fn tick(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_FIGHT_TEST").is_none() {
        return;
    }
    let pl = super::belt::player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 9 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let fight: f64 = std::env::var("CVR_FIGHT_TEST_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(90.0);
    // the police come this long after the car (CVR_FIGHT_TEST_CALM seconds, default 10); the fight's steps count from them
    let heat_at = 25.0 + std::env::var("CVR_FIGHT_TEST_CALM").ok().and_then(|v| v.parse::<f64>().ok()).unwrap_or(10.0);
    let t = t + dt;
    let mut next = step;
    let q = qpc_s();
    match step {
        0 if t > 15.0 => {
            let (inv, imm) = god_modes(&pl);
            let said = add_god_mode(&pl);
            cvr_host::log!("[fight-test] t={q:.3} world runs; before: invulnerable {inv}, immortal {imm}; god mode asked for: {said} (from Rust)\n");
            next = 1;
        }
        1 if t > 17.0 => {
            let (inv, imm) = god_modes(&pl);
            cvr_host::log!("[fight-test] t={q:.3} god mode read back: invulnerable {inv}, immortal {imm} (from Rust)\n");
            // Immortal is what keeps V alive (health stops at its floor); the game's own Invulnerable sources come and go
            next = if imm { 2 } else { 9 };
            if next == 9 {
                cvr_host::log!("[fight-test] god mode NOT on: no police called (from Rust)\n");
            }
        }
        2 if t > 25.0 => {
            let given = drop_item(&pl);
            // the player's car summoned: parked beside V, it brings the "get in" interaction (seen crouched)
            let vs = super::game_system("GetVehicleSystem");
            // the VR menu's own call: the player's first car that may be summoned, as its CALL VEHICLE button does
            let _ = &vs;
            let cars = super::menu_data::vehicles();
            let pick = cars.iter().find(|v| v.kind == "car" && v.repair <= 0.0).or_else(|| cars.first());
            let (active, said) = pick.map_or((false, "no vehicle"), super::menu_data::summon);
            cvr_host::log!("[fight-test] {} vehicles; summon: {said} ({}) (from Rust)\n", cars.len(), pick.map_or("-", |v| v.name.as_str()));
            // summoning is barred in some places (the H10 entrance): a car put down 2.5 m before V, side on, instead
            let any = !active && {
                let at = |name: &str| {
                    rt::call(&pl, name, &[]).map(|o| {
                        let v = Val::of(&o);
                        let c = |k: &str| v.f(k).and_then(|x| x.num()).unwrap_or(0.0);
                        [c("X"), c("Y"), c("Z")]
                    })
                };
                // along the view (the headset's, which a harness cannot turn on SteamVR), else the body's forward;
                // CVR_FIGHT_TEST_CAR_DISTANCE metres away (default 2.5)
                let cam = super::game_system("GetCameraSystem");
                let view = (!cam.is_null())
                    .then(|| rt::call(&cam, "GetActiveCameraForward", &[]))
                    .flatten()
                    .map(|o| {
                        let v = Val::of(&o);
                        let c = |k: &str| v.f(k).and_then(|x| x.num()).unwrap_or(0.0);
                        [c("X"), c("Y"), c("Z")]
                    })
                    .filter(|f| f[0].hypot(f[1]) > 0.2);
                let dist: f64 = std::env::var("CVR_FIGHT_TEST_CAR_DISTANCE").ok().and_then(|v| v.parse().ok()).unwrap_or(2.5);
                match (at("GetWorldPosition"), view.or_else(|| at("GetWorldForward"))) {
                    (Some(p), Some(f)) => {
                        let len = (f[0] * f[0] + f[1] * f[1]).sqrt().max(1e-6);
                        let (fx, fy) = (f[0] / len, f[1] / len);
                        let pos = [p[0] + fx * dist, p[1] + fy * dist, p[2]];
                        // the car's forward along V's right: its side (a door) faces V
                        let yaw = fy.atan2(fx) - std::f64::consts::FRAC_PI_2 - std::f64::consts::FRAC_PI_2;
                        let q = [0.0, 0.0, (yaw / 2.0).sin(), (yaw / 2.0).cos()];
                        let id = super::lobby_spawn::spawn_record("Vehicle.v_standard2_archer_hella", pos, q, "cvr_fight_car");
                        cvr_host::log!("[fight-test] car put down at ({:.2}, {:.2}, {:.2}): entity {id:?} (from Rust)\n", pos[0], pos[1], pos[2]);
                        id.is_some()
                    }
                    _ => false,
                }
            };
            // weapon slot 1 drawn, for the shots the harness fires through the test pad
            let c = super::game_system("GetScriptableSystemsContainer");
            let equipment = rt::obj(&c, "Get", &[n("EquipmentSystem")]);
            let req = super::lobby_ink::new("EquipmentSystemWeaponManipulationRequest");
            if !equipment.is_null() && !req.is_null() {
                super::menu_rt::hset(&req, "requestType", super::menu_rt::ev("EquipmentManipulationAction", "RequestWeaponSlot1"));
                super::menu_main::set_handle_prop(&req, "owner", &pl);
                rt::call(&equipment, "QueueRequest", &[A::H(&req)]);
            }
            cvr_host::log!("[fight-test] t={q:.3} item given {given} and dropped before the player; car summoned: active {active}, any {any}; weapon slot 1 asked for (from Rust)\n");
            next = 3;
        }
        3 if t > heat_at => {
            let asked = heat("Heat_3");
            cvr_host::log!("[fight-test] t={q:.3} police heat 3 asked {asked} (from Rust)\n");
            next = 4;
        }
        4 if t > heat_at + 5.0 => {
            let (inv, imm) = god_modes(&pl);
            cvr_host::log!("[fight-test] t={q:.3} heat now {}; god mode invulnerable {inv}, immortal {imm} (from Rust)\n", heat_now());
            next = 5;
        }
        5 if t > heat_at + fight / 2.0 => {
            cvr_host::log!("[fight-test] t={q:.3} half way: heat {} (from Rust)\n", heat_now());
            next = 6;
        }
        6 if t > heat_at + fight => {
            heat("Heat_0");
            cvr_host::log!("[fight-test] t={q:.3} heat taken off (from Rust)\n");
            next = 7;
        }
        7 if t > heat_at + 5.0 + fight => {
            cvr_host::log!("[fight-test] t={q:.3} done: heat {} (from Rust)\n", heat_now());
            next = 9;
        }
        _ => {}
    }
    T.with(|c| c.set((t, next, first)));
}
