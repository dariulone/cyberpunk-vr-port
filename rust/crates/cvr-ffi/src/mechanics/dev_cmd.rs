//! TEST COMMANDS TO THE RUNNING GAME (development): with a file `cvr_dev_commands` beside the game's executable, the
//! port reads log/dev_cmd.txt twice a second, takes it (deletes it) and runs its lines -- so a test drives a game that is
//! already running instead of relaunching it with environment switches. Every command answers with a "[dev]" line in
//! the log (the harness waits for it). Without the marker nothing is read.
//!
//!   ping                      -> "[dev] pong"
//!   vendor <name part>        walk to the nearest vendor whose record has that in its name (the H10 atrium if none
//!                             near), facing it; its lines are "[VRMenu] vendor walk: ..."
//!   trade                     with a visit's trade page open: sell one piece, buy it back, the money before and after
//!   weapon <record>           give the weapon (and handgun rounds), equip it to the active slot and draw it
//!   equip <record>            give the item and ask the equipment system to equip it (cyberware into its area)
//!   car | mount | unmount     an Archer spawned beside the player; the player put in its driver's seat; out again
//!   teleport <x> <y> <z>      the player put there
//!   roof [x y]                an open City Center roof; with a point, the highest flat roof round it
//!   ammo <n>                  the held gun's magazine set to n rounds
//!   holster                   every weapon put away (the equipment system's UnequipAll)
//!   menu [section]            the VR menu opened before the eyes, on that section (inventory, character, map ...) (with "reach 1" its rays go to log/menu_ray.json)
//!   lobby <row>               the main menu lobby's top page: that row pressed as a hand presses it (e.g. "new")
//!   reach <1|0>               the free hand's grip targets written to log/reload_reach.json (reload_reach.rs)
use crate::script_call::Value;
use std::cell::Cell;

thread_local! {
    static POLL: Cell<f64> = const { Cell::new(0.0) };
    static MARKER: Cell<(f64, bool)> = const { Cell::new((0.0, false)) };
}

/// The marker, looked at every 5 s (a file made while the game runs counts).
fn enabled(dt: f64) -> bool {
    MARKER.with(|m| {
        let (t, on) = m.get();
        let t = t - dt;
        if t > 0.0 {
            m.set((t, on));
            return on;
        }
        let on = cvr_host::paths::test_marker("cvr_dev_commands");
        m.set((5.0, on));
        on
    })
}

pub(in crate::mechanics) fn tick(dt: f64) {
    if !enabled(dt) {
        return;
    }
    let due = POLL.with(|c| {
        let v = c.get() + dt;
        c.set(if v > 0.5 { 0.0 } else { v });
        v > 0.5
    });
    if !due {
        return;
    }
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "dev_cmd.txt");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        run(line);
    }
}

fn run(line: &str) {
    let (cmd, rest) = line.split_once(' ').map_or((line, ""), |(c, r)| (c, r.trim()));
    match cmd {
        "ping" => cvr_host::log!("[dev] pong\n"),
        "vendor" => {
            super::menu_vendor::dev_vendor(rest);
            cvr_host::log!("[dev] vendor '{rest}': walking\n");
        }
        "trade" => {
            super::menu_vendor::dev_trade_start();
            cvr_host::log!("[dev] trade: started\n");
        }
        "weapon" => {
            super::reload_probe::dev_weapon(rest);
            cvr_host::log!("[dev] weapon '{rest}': giving\n");
        }
        "car" | "mount" | "unmount" => {
            let said = super::car_hands_probe::dev(cmd);
            cvr_host::log!("[dev] {cmd}: {said}\n");
        }
        "equip" => {
            let said = super::reload_probe::dev_equip(rest);
            cvr_host::log!("[dev] equip: {said}\n");
        }
        "teleport" => {
            let v: Vec<f32> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() < 3 {
                cvr_host::log!("[dev] teleport: x y z wanted\n");
                return;
            }
            teleport([v[0], v[1], v[2]]);
            cvr_host::log!("[dev] teleport ({}, {}, {}): done\n", v[0], v[1], v[2]);
        }
        "roof" => {
            // without a point: a known open roof (City Center, nothing over it); the search below can land under a
            // roof whose top has no collision (a megabuilding's) -- inside the geometry
            let v: Vec<f64> = rest.split_whitespace().filter_map(|x| x.parse().ok()).collect();
            if v.len() >= 2 {
                roof(Some([v[0], v[1]]));
            } else {
                teleport(OPEN_ROOF);
                cvr_host::log!("[dev] roof: put at ({:.1}, {:.1}, {:.1})\n", OPEN_ROOF[0], OPEN_ROOF[1], OPEN_ROOF[2]);
            }
        }
        "ammo" => {
            let n: i32 = rest.parse().unwrap_or(0);
            let w = super::belt::st_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(&super::belt::player())]);
            let r = if w.is_null() { "no weapon in the hands".to_string() } else { super::reload_core::dev_set_ammo(&w, n) };
            cvr_host::log!("[dev] ammo: {r}\n");
        }
        "holster" => {
            super::menu_main::holster_all();
            cvr_host::log!("[dev] holster: asked
");
        }
        "menu" => {
            // answered by the menu's update ("[dev] menu: open ...")
            super::menu_main::dev_open(rest);
        }
        "lobby" => {
            // answered by the lobby's tick ("[dev] lobby '<row>': pressed ...")
            super::lobby::dev_press(rest);
        }
        "reach" => {
            let on = rest != "0";
            cvr_rig::vrik::reach_dev::set(on);
            cvr_host::log!("[dev] reach: {on}\n");
        }
        _ => cvr_host::log!("[dev] unknown command '{line}'\n"),
    }
}

/// An open roof in City Center: flat, lit, nobody about, sky over it.
const OPEN_ROOF: [f32; 3] = [-666.89, 850.52, 64.2];

fn teleport(p: [f32; 3]) {
    let pl = super::belt::player();
    let tf = super::game_system("GetTeleportationFacility");
    let to = [p[0], p[1], p[2], 1.0];
    let rot = [0.0f32; 3];
    super::lobby_ink::call(&tf, "Teleport", &[Value::Handle(&pl), Value::Ptr(to.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast())]);
}

/// The "roof x y" command: the highest flat static surface within 60 m of the player (or over the point given) -- a roof to
/// try weapons on with nobody about. Rays straight down from 800 m on a 10 m grid; a hit counts when four more 1.5 m
/// round it land within 0.3 m of it.
fn roof(at: Option<[f64; 2]>) {
    let pl = super::belt::player();
    let me = super::belt::m::<[f32; 4]>(&pl, "GetWorldPosition", &[]).ok().map(|v| [f64::from(v[0]), f64::from(v[1])]).unwrap_or([0.0; 2]);
    let c = at.unwrap_or(me);
    let sq_held = super::game_system("GetSpatialQueriesSystem");
    if sq_held.instance.is_null() {
        cvr_host::log!("[dev] roof: no spatial queries\n");
        return;
    }
    let down = |x: f64, y: f64| -> Option<f64> {
        super::view::raycast(sq_held.instance, "SyncRaycastByCollisionGroup", [x, y, 800.0], [x, y, -50.0], "Static", &[Value::Bool(false), Value::Bool(false)])
            .ok()
            .flatten()
            .map(|p| p[2])
    };
    let mut best: Option<[f64; 3]> = None;
    let span = if at.is_some() { 2 } else { 6 };
    for i in -span..=span {
        for j in -span..=span {
            let (x, y) = (c[0] + f64::from(i) * 10.0, c[1] + f64::from(j) * 10.0);
            let Some(z) = down(x, y) else { continue };
            if best.is_some_and(|b| z <= b[2]) {
                continue;
            }
            let flat = [[1.5, 0.0], [-1.5, 0.0], [0.0, 1.5], [0.0, -1.5]].iter().all(|o| down(x + o[0], y + o[1]).is_some_and(|z2| (z2 - z).abs() < 0.3));
            if flat {
                best = Some([x, y, z]);
            }
        }
    }
    let Some(p) = best else {
        cvr_host::log!("[dev] roof: no flat surface found round ({:.1}, {:.1})\n", c[0], c[1]);
        return;
    };
    let tf = super::game_system("GetTeleportationFacility");
    let to = [p[0] as f32, p[1] as f32, (p[2] + 0.1) as f32, 1.0];
    let rot = [0.0f32; 3];
    super::lobby_ink::call(&tf, "Teleport", &[Value::Handle(&pl), Value::Ptr(to.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast())]);
    cvr_host::log!("[dev] roof: put at ({:.1}, {:.1}, {:.1})\n", p[0], p[1], p[2]);
}
