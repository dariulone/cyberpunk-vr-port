//! The reload recorder without the Lua host (formerly mods/cet/CyberpunkVRPort_ReloadRecorder): records the game's own
//! reload or throw animation frame by frame (VRIK off, so it plays on the arms) -- both wrists, the WeaponLeft /
//! WeaponRight anchor bones, the weapon's slots by candidate name, the mount's and the weapon's rotation and the
//! weapon's position, its three rigs' bones and the magazine rig's tracks, the round count, every finger joint of both
//! hands, and the item in each hand slot with its record -- all in the player's model space, written as the mod wrote
//! it: lua\CyberpunkVRPort_ReloadRecorder\reload_record_NN.lua, numbered, never overwritten. Its "VR Reload Recorder"
//! window (dev_windows.rs) starts and stops a take; the sampling and the file are the main thread's tick.
use super::belt::{components, conj, item_id, player, qmul, qrot, record_name, Q, V3};
use crate::script_call::{call_native, method, Value};
use red4ext_abi::handle::Handle;
use std::sync::Mutex;

/// Finger bones (player_bone_names.txt): 5 metacarpals, thumb 1-2, then index / middle / ring / pinky 1-2-3.
const FINGERS_L: [i32; 19] = [33, 34, 35, 36, 37, 45, 55, 46, 56, 65, 47, 57, 66, 48, 58, 67, 49, 59, 68];
const FINGERS_R: [i32; 19] = [38, 39, 40, 41, 42, 50, 60, 51, 61, 69, 52, 62, 70, 53, 63, 71, 54, 64, 72];
/// Every bone of the three rigs the sets have (magazine, frame, receiver).
const MAG_BONES: i32 = 16;
const FRAME_BONES: i32 = 21;
const RCV_BONES: i32 = 19;
/// The racking slot, most specific first (vrp_barrel last: it does not ride the moving part).
const SLIDE_SLOTS: [&str; 14] = [
    "vrp_back_slider",
    "vrp_slide",
    "vrp_barrel_back",
    "vrp_barrel_front",
    "vrp_magazine_open",
    "vrp_slider",
    "vrp_end_slider",
    "vrp_cylinder",
    "vrp_cocking_handle",
    "vrp_foreend",
    "vrp_barrel_hinge",
    "vrp_reload_handle",
    "vrp_cocking_device",
    "vrp_barrel",
];
const FRONT_SLOTS: [&str; 2] = ["vrp_front_slider", "vrp_muzzle_slot"];
/// The magazine's slot, in the module's own order (two can share a position and differ by 180 deg).
const MAG_SLOTS: [&str; 2] = ["vrp_mag_slot", "vrp_mag_std"];

struct Rec {
    on: bool,
    t: f64,
    lines: Vec<String>,
    slot_comp: Handle,
    last: Option<String>,
    /// Asked by the window, done by the tick: Some(true) start, Some(false) stop and save.
    ask: Option<bool>,
}
// SAFETY: the slot component is used on the game's main thread only (the window reads the counters).
unsafe impl Send for Rec {}
static REC: Mutex<Rec> = Mutex::new(Rec { on: false, t: 0.0, lines: Vec::new(), slot_comp: Handle::null(), last: None, ask: None });

fn native4(name: &str, idx: i32) -> [f32; 4] {
    // SAFETY: the plugin's own natives.
    unsafe { call_native::<[f32; 4]>(name, &[Value::Int32(idx)]) }.unwrap_or([0.0; 4])
}
fn native_i(name: &str, values: &[Value]) -> i32 {
    // SAFETY: the plugin's own natives.
    unsafe { call_native::<i32>(name, values) }.unwrap_or(0)
}
fn native_f(name: &str, values: &[Value]) -> f32 {
    // SAFETY: the plugin's own natives.
    unsafe { call_native::<f32>(name, values) }.unwrap_or(0.0)
}
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> Option<T> {
    if o.is_null() {
        return None;
    }
    // SAFETY: a live object, held.
    unsafe { method::<T>(o.instance, name, values) }
}
fn f64s(v: [f32; 4]) -> Q {
    v.map(f64::from)
}
fn vec_str(v: &[f64]) -> String {
    v.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join(",")
}

/// One bone as 7 floats (model-space position, rotation).
fn bone_pq(idx: i32) -> [f64; 7] {
    let p = native4("VRBoneModelPos", idx);
    let q = native4("VRBoneModelRot", idx);
    [p[0], p[1], p[2], q[0], q[1], q[2], q[3]].map(f64::from)
}
fn finger_row(list: &[i32; 19]) -> Vec<f64> {
    list.iter().flat_map(|&i| bone_pq(i)).collect()
}
/// A rig's bones as the game leaves them: 7 floats each (parent-local translation, rotation).
fn rig_row(which: i32, bones: i32) -> Vec<f64> {
    (0..bones).flat_map(|b| (0..7).map(move |f| f64::from(native_f("VRRigBone", &[Value::Int32(which), Value::Int32(b), Value::Int32(f)])))).collect()
}
/// The magazine rig's float tracks (at most 8).
fn track_row(which: i32) -> Vec<f64> {
    let n = (native_f("VRRigTrack", &[Value::Int32(which), Value::Int32(-1)]) as i32).clamp(0, 8);
    (0..n).map(|i| f64::from(native_f("VRRigTrack", &[Value::Int32(which), Value::Int32(i)]))).collect()
}

/// The player's frame: its position and the inverse of its orientation (world -> model).
fn player_frame(pl: &Handle) -> Option<(V3, Q)> {
    let p = m::<[f32; 4]>(pl, "GetWorldPosition", &[])?;
    let q = m::<[f32; 4]>(pl, "GetWorldOrientation", &[])?;
    Some(([p[0], p[1], p[2]].map(f64::from), conj(f64s(q))))
}
fn model_pos(frame: &(V3, Q), p: [f32; 4]) -> V3 {
    let (b, c) = frame;
    qrot(*c, [f64::from(p[0]) - b[0], f64::from(p[1]) - b[1], f64::from(p[2]) - b[2]])
}
/// The weapon's slot component (found once per take: the first SlotComponent that answers a slot).
fn slot_comp(rec: &mut Rec, weapon: &Handle, name: &str) -> Handle {
    if rec.slot_comp.is_null() {
        for c in components(weapon) {
            if super::belt::class_name(&c).contains("SlotComponent") && super::reload_core::slot_world(&c, name).is_some() {
                rec.slot_comp = c;
                break;
            }
        }
    }
    rec.slot_comp.clone()
}
fn slot_any(rec: &mut Rec, weapon: &Handle, frame: &(V3, Q), names: &[&str]) -> Option<V3> {
    names.iter().find_map(|n| {
        let sc = slot_comp(rec, weapon, n);
        let (p, _) = super::reload_core::slot_world(&sc, n)?;
        let (b, c) = frame;
        Some(qrot(*c, [p[0] - b[0], p[1] - b[1], p[2] - b[2]]))
    })
}
fn slot_rot_any(rec: &Rec, frame: &(V3, Q), names: &[&str]) -> Option<Q> {
    if rec.slot_comp.is_null() {
        return None;
    }
    names.iter().find_map(|n| super::reload_core::slot_world(&rec.slot_comp, n).map(|(_, q)| qmul(frame.1, q)))
}
/// The item in a hand slot: model-space position and rotation, and its record.
fn held(pl: &Handle, frame: &(V3, Q), slot: &str) -> (Option<[f64; 7]>, Option<String>) {
    let o = super::belt::in_slot(pl, slot);
    if o.is_null() {
        return (None, None);
    }
    let name = item_id(&o).and_then(|id| record_name(id[0]));
    let (Some(p), Some(q)) = (m::<[f32; 4]>(&o, "GetWorldPosition", &[]), m::<[f32; 4]>(&o, "GetWorldOrientation", &[])) else { return (None, name) };
    let v = model_pos(frame, p);
    let r = qmul(frame.1, f64s(q));
    (Some([v[0], v[1], v[2], r[0], r[1], r[2], r[3]]), name)
}

/// One frame of the take, as the mod's line of its file.
fn sample(rec: &mut Rec) -> Option<String> {
    let pl = player();
    let frame = player_frame(&pl)?;
    let weapon = super::belt::st_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(&pl)]);
    let (sl, sf, mg) = if weapon.is_null() {
        (None, None, None)
    } else {
        (slot_any(rec, &weapon, &frame, &SLIDE_SLOTS), slot_any(rec, &weapon, &frame, &FRONT_SLOTS), slot_any(rec, &weapon, &frame, &MAG_SLOTS))
    };
    let mq = slot_rot_any(rec, &frame, &MAG_SLOTS);
    let wq = m::<[f32; 4]>(&weapon, "GetWorldOrientation", &[]).map(|q| qmul(frame.1, f64s(q)));
    let wp = m::<[f32; 4]>(&weapon, "GetWorldPosition", &[]).map(|p| model_pos(&frame, p));
    let am = super::belt::st::<u32>("WeaponObject", "GetMagazineAmmoCount", &[Value::Handle(&weapon)]).filter(|_| !weapon.is_null());
    let (hl, hl_name) = held(&pl, &frame, "AttachmentSlots.WeaponLeft");
    let (hr, hr_name) = held(&pl, &frame, "AttachmentSlots.WeaponRight");
    let hn = hr_name.or(hl_name).unwrap_or_default();
    let v = |o: Option<&[f64]>| o.map(vec_str).unwrap_or_default();
    Some(format!(
        "{{t={:.4},l={{{}}},r={{{}}},wl={{{}}},wr={{{}}},sl={{{}}},sf={{{}}},mg={{{}}},mq={{{}}},wq={{{}}},wp={{{}}},mb={{{}}},fb={{{}}},rb={{{}}},mt={{{}}},am={},fL={{{}}},fR={{{}}},hl={{{}}},hr={{{}}},hn=\"{}\"}},",
        rec.t,
        vec_str(&bone_pq(23)),
        vec_str(&bone_pq(24)),
        vec_str(&bone_pq(26)),
        vec_str(&bone_pq(28)),
        v(sl.as_ref().map(|x| &x[..])),
        v(sf.as_ref().map(|x| &x[..])),
        v(mg.as_ref().map(|x| &x[..])),
        v(mq.as_ref().map(|x| &x[..])),
        v(wq.as_ref().map(|x| &x[..])),
        v(wp.as_ref().map(|x| &x[..])),
        vec_str(&rig_row(0, MAG_BONES)),
        vec_str(&rig_row(1, FRAME_BONES)),
        vec_str(&rig_row(2, RCV_BONES)),
        vec_str(&track_row(0)),
        am.map_or("nil".to_string(), |n| n.to_string()),
        vec_str(&finger_row(&FINGERS_L)),
        vec_str(&finger_row(&FINGERS_R)),
        v(hl.as_ref().map(|x| &x[..])),
        v(hr.as_ref().map(|x| &x[..])),
        hn.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

const HEADER: &str = "-- VR reload recording v2, MODEL space. l/r = wrist L/R, wl/wr = WeaponLeft/WeaponRight bone,
-- each 7 floats (x,y,z, qi,qj,qk,qr). sl/sf/mg = back_slider/front_slider/mag_std slot pos.
-- fL/fR = 19 bones x 7 floats, order: InHand Thumb,Index,Middle,Ring,Pinky; Thumb1,2;
-- Index1,2,3; Middle1,2,3; Ring1,2,3; Pinky1,2,3.
-- mq = magazine mount slot ROTATION, wq = the WEAPON own rotation, 4 floats each, model space:
-- with these two, nothing has to be reconstructed from slot positions.
-- mb = magazine rig bones 0,1,2,3,4 (mag_plug, magazine, magazine_reload, mag_std, mag_stdr),
-- rb = receiver rig bones 0..9 (a break-action top lever), mt = magazine rig float tracks,
--      am = the magazine round count.
-- fb = frame rig bones 5,6,9,10,11,12,13,14 (front/back slider, bullet_pull, bullet,
--      bullet_reload, hammer, rotator, ammo_mover); 7 floats per bone: local pos xyz + quat xyzw.
-- hl/hr = the ITEM held in AttachmentSlots.WeaponLeft/Right, 7 floats, model space; hn = its
--      record name. Empty on a reload, and the whole point of a grenade take: hr against wr
--      is the pose the game holds the thing at, which is what the port has to reproduce.
";

fn start(rec: &mut Rec) {
    super::reload_frame::register_rigs();
    (rec.on, rec.t, rec.slot_comp) = (true, 0.0, Handle::null());
    rec.lines.clear();
    native_i("VRRecordFK", &[Value::Int32(1)]);
    cvr_host::log!("[Recorder] recording (from Rust)\n");
}
/// recDump: the take written to the next free reload_record_NN.lua.
fn dump(rec: &mut Rec) {
    native_i("VRRecordFK", &[Value::Int32(0)]);
    let dir = cvr_host::paths::module_folder().unwrap_or_default().join("lua").join("CyberpunkVRPort_ReloadRecorder");
    let name = (1..=99).map(|i| format!("reload_record_{i:02}.lua")).find(|n| !dir.join(n).exists()).unwrap_or_else(|| "reload_record_overflow.lua".into());
    let mut text = String::from(HEADER);
    text.push_str("return {\n");
    for l in &rec.lines {
        text.push_str(l);
        text.push('\n');
    }
    text.push_str("}\n");
    if std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(&name), text)).is_ok() {
        cvr_host::log!("[Recorder] saved {} frames to {name} (from Rust)\n", rec.lines.len());
        rec.last = Some(name);
    } else {
        cvr_host::log!("[Recorder] io.open FAILED for {name}; {} frames lost (from Rust)\n", rec.lines.len());
        rec.last = Some("SAVE FAILED".into());
    }
}

/// Every frame: what the window asked, then a frame of the take while it records.
pub(super) fn tick(dt: f64) {
    let mut rec = REC.lock().unwrap_or_else(|p| p.into_inner());
    match rec.ask.take() {
        Some(true) if !rec.on => start(&mut rec),
        Some(false) if rec.on => {
            rec.on = false;
            dump(&mut rec);
        }
        _ => {}
    }
    if !rec.on {
        return;
    }
    rec.t += if dt > 0.0 { dt } else { 0.016 };
    if let Some(line) = sample(&mut rec) {
        rec.lines.push(line);
    }
}
/// The mod's "VR Reload Recorder" window.
/// The recorder installed: an authoring tool, never in a release package -- as CET ran the mod only when its folder
/// was there (lua\CyberpunkVRPort_ReloadRecorder with its init.lua), the window is drawn only then (the self-test asks
/// for it by its switch).
fn installed() -> bool {
    static INSTALLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *INSTALLED.get_or_init(|| {
        cvr_host::paths::dir(cvr_host::paths::Folder::Lua).join("CyberpunkVRPort_ReloadRecorder").join("init.lua").is_file()
            || std::env::var_os("CVR_RECORDER_TEST").is_some()
    })
}
pub(super) fn dev_window(frame: [f32; 2]) {
    use super::dev_windows as w;
    if !installed() {
        return;
    }
    let mut rec = REC.lock().unwrap_or_else(|p| p.into_inner());
    w::window("VR Reload Recorder", [0.71, 0.04], [0.24, 0.91], frame, || {
        if rec.on {
            w::text(&format!("RECORDING  {:.2}s  {} frames", rec.t, rec.lines.len()));
            if w::button("STOP + save") {
                rec.ask = Some(false);
            }
        } else {
            w::text("1) VRIK off  2) Record  3) reload  4) Stop");
            if let Some(last) = &rec.last {
                w::text(&format!("last: {last}"));
            }
            if w::button("Record") {
                rec.ask = Some(true);
            }
        }
    });
}
/// The mod's onShutdown: the FK recording let go (a take in progress is not saved, as the mod's was not).
pub(super) fn shutdown() {
    if REC.lock().unwrap_or_else(|p| p.into_inner()).on {
        native_i("VRRecordFK", &[Value::Int32(0)]);
    }
}

/// A development check (CVR_RECORDER_TEST): 50 s into the loaded save a take starts and 3 s later it is saved, as the
/// window's Record and STOP + save would.
pub(super) fn self_test(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize)> = const { std::cell::Cell::new((0.0, 0, 0)) };
    }
    if std::env::var_os("CVR_RECORDER_TEST").is_none() {
        return;
    }
    let pl = player();
    let (t, step, first) = T.with(|c| c.get());
    if pl.is_null() || step >= 2 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first)));
        return;
    }
    let t = t + dt;
    let mut step = step;
    if step == 0 && t > 44.0 && t - dt <= 44.0 {
        // a gun drawn first, so the take has its slots and rigs
        super::holster::equip_request(&pl, "RequestWeaponSlot1");
    }
    if step == 0 && t > 50.0 {
        REC.lock().unwrap_or_else(|p| p.into_inner()).ask = Some(true);
        cvr_host::log!("[Recorder] self-test: Record (from Rust)\n");
        step = 1;
    } else if step == 1 && t > 53.0 {
        REC.lock().unwrap_or_else(|p| p.into_inner()).ask = Some(false);
        cvr_host::log!("[Recorder] self-test: STOP + save (from Rust)\n");
        step = 2;
    }
    T.with(|c| c.set((t, step, first)));
}
