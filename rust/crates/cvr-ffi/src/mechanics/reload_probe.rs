//! RELOAD PROBE (tests only, CVR_RELOAD_PROBE=<weapon record>, e.g. Items.Preset_Silverhand_3516): once the loaded
//! world runs, the weapon and handgun rounds given, the weapon equipped to the active slot and drawn. From then on,
//! every 50 ms, a row of log/reload_probe.csv: the QPC clock in seconds, the reload's state (magazine, slide, rotator,
//! crane, cylinder), both rigs' bones as the game's animation left them (parent-local translation + quaternion) and the
//! rig write queue (which, bone, base kept, offset and base in mm) -- to see what moves a part, and whose write it is.
use super::lobby_rtti::{self as rt, A};
use super::reload_core::{rig_bone, State};
use crate::script_call::Value;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::{Handle, WeakHandle};
use std::io::Write;
use std::sync::Mutex;

const BONES: i32 = 17;

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
fn probe_write() -> Option<(i32, i32, f64, f64)> {
    let v = std::env::var("CVR_RELOAD_PROBE_WRITE").ok()?;
    let p: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (p.len() == 4).then(|| (p[0] as i32, p[1] as i32, p[2], p[3]))
}
fn write_at() -> f64 {
    std::env::var("CVR_RELOAD_PROBE_WRITE_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(45.0)
}
fn record() -> Option<String> {
    std::env::var("CVR_RELOAD_PROBE").ok().filter(|v| !v.is_empty())
}
/// A request object of `class` with its owner and item written in place.
fn request(class: &str, pl: &Handle, item: [u64; 2]) -> Option<Handle> {
    let class = crate::script_call::class_any(class)?;
    // SAFETY: a new instance owned by the handle made for it; its native properties written in place by their types.
    let r = unsafe { Handle::from_instance(class.create_instance()) };
    unsafe {
        if let Some(p) = crate::script_call::property(r.instance, "owner") {
            let old = std::ptr::replace(p as *mut WeakHandle, WeakHandle::from_handle(pl));
            drop(old);
        }
        if let Some(p) = crate::script_call::property(r.instance, "itemID") {
            *(p as *mut [u64; 2]) = item;
        }
    }
    Some(r)
}
fn equipment_system() -> Handle {
    let c = super::game_system("GetScriptableSystemsContainer");
    rt::obj(&c, "Get", &[A::Name(CName::hashed("EquipmentSystem").0)])
}
fn give(pl: &Handle, rec: &str, n: i32) -> Option<[u64; 2]> {
    let ts = super::belt::transactions();
    let id = super::belt::st::<[u64; 2]>("ItemID", "FromTDBID", &[super::belt::tdb(rec)])?;
    let ok = super::belt::m::<bool>(&ts, "GiveItem", &[Value::Handle(pl), Value::Ptr(id.as_ptr().cast()), Value::Int32(n)]).unwrap_or(false);
    ok.then_some(id)
}

/// The weapon given, equipped and drawn, step by step after the world runs.
pub(super) fn drive(dt: f64) {
    thread_local! {
        static T: std::cell::Cell<(f64, u32, usize, [u64; 2])> = const { std::cell::Cell::new((0.0, 0, 0, [0; 2])) };
    }
    let Some(rec) = record() else { return };
    let pl = super::belt::player();
    let (t, step, first, id) = T.with(|c| c.get());
    if pl.is_null() || step >= 6 {
        return;
    }
    let p = pl.instance as usize;
    let first = if first == 0 { p } else { first };
    if p == first || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) {
        T.with(|c| c.set((t, step, first, id)));
        return;
    }
    let t = t + dt;
    let (mut next, mut id) = (step, id);
    match step {
        0 if t > 12.0 => {
            let got = give(&pl, &rec, 1);
            let ammo = give(&pl, "Ammo.HandgunAmmo", 300).is_some();
            cvr_host::log!("[reload-probe] q={:.3} {rec} given: {got:?}, rounds {ammo} (from Rust)\n", qpc_s());
            id = got.unwrap_or([0; 2]);
            next = if got.is_some() { 1 } else { 4 };
        }
        1 if t > 13.0 => {
            let es = equipment_system();
            let req = request("EquipRequest", &pl, id);
            if let (false, Some(req)) = (es.is_null(), req) {
                // SAFETY: the request's own bool, written in place.
                unsafe {
                    if let Some(p) = crate::script_call::property(req.instance, "equipToCurrentActiveSlot") {
                        *(p as *mut bool) = true;
                    }
                }
                rt::call(&es, "QueueRequest", &[A::H(&req)]);
            }
            cvr_host::log!("[reload-probe] q={:.3} equip asked (from Rust)\n", qpc_s());
            next = 2;
        }
        2 if t > 15.0 => {
            let es = equipment_system();
            if let (false, Some(req)) = (es.is_null(), request("DrawItemRequest", &pl, id)) {
                rt::call(&es, "QueueRequest", &[A::H(&req)]);
            }
            cvr_host::log!("[reload-probe] q={:.3} draw asked (from Rust)\n", qpc_s());
            next = 3;
        }
        3 if t > 19.0 => {
            let w = super::belt::st_handle("ScriptedPuppet", "GetActiveWeapon", &[Value::Handle(&pl)]);
            let name = super::belt::item_id(&w).and_then(|i| super::belt::record_name(i[0])).unwrap_or_else(|| "-".into());
            cvr_host::log!("[reload-probe] q={:.3} active weapon {name} (from Rust)\n", qpc_s());
            next = 4;
        }
        // CVR_RELOAD_PROBE_WRITE="which,bone,dz,turn": the write a grabbed slide makes (a translation along the bone's z
        // and a turn about it), held 4 s from CVR_RELOAD_PROBE_WRITE_AT s on (default 45) -- its latched base shows
        // whose rig the pass read it from
        4 if t > write_at() => {
            if let Some((w, b, dz, turn)) = probe_write() {
                super::reload_core::rig_write(w, b, [0.0, 0.0, dz]);
                if turn != 0.0 {
                    super::reload_core::rig_write_rot(w, b, turn, [0.0, 0.0, 1.0]);
                }
                cvr_host::log!("[reload-probe] q={:.3} synthetic write on rig {w} bone {b}: dz {dz}, turn {turn} (from Rust)\n", qpc_s());
            }
            next = 5;
        }
        5 if t > write_at() + 4.0 => {
            if let Some((w, b, _, _)) = probe_write() {
                super::reload_core::rig_write_off(w, b);
                cvr_host::log!("[reload-probe] q={:.3} synthetic write released (from Rust)\n", qpc_s());
            }
            next = 6;
        }
        _ => {}
    }
    T.with(|c| c.set((t, next, first, id)));
}

struct Sink {
    out: std::io::BufWriter<std::fs::File>,
    acc: f64,
    rows: u32,
}
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// One row every 50 ms while the probe is asked for.
pub(super) fn row(s: &State, note: &str, dt: f64) {
    if record().is_none() {
        return;
    }
    let mut g = SINK.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "reload_probe.csv");
        let Ok(f) = std::fs::File::create(path) else { return };
        let mut out = std::io::BufWriter::new(f);
        let mut head = String::from("qpc_s,note,mag,mag_hand,rack,grabbed,locked,fired,rot_t,hatch_p,hatch_open,spin_ang,spin_w,spin_dbg");
        for w in 0..2 {
            for b in 0..BONES {
                for k in ["x", "y", "z", "qx", "qy", "qz", "qw"] {
                    head.push_str(&format!(",r{w}b{b}{k}"));
                }
            }
        }
        head.push_str(",writes\n");
        let _ = out.write_all(head.as_bytes());
        *g = Some(Sink { out, acc: 1.0, rows: 0 });
    }
    let Some(sink) = g.as_mut() else { return };
    sink.acc += dt;
    if sink.acc < 0.05 {
        return;
    }
    sink.acc = 0.0;
    let f = &s.frame;
    let mut line = format!(
        "{:.4},{},{},{},{:.4},{},{},{},{:.3},{:.3},{},{:.2},{:.1},{}",
        qpc_s(),
        note.replace(',', ";"),
        f.mag.state,
        f.mag.hand.map_or(-1, |h| h as i32),
        f.slide.rack,
        u8::from(f.slide.grabbed),
        u8::from(f.slide.locked),
        u8::from(f.slide.fired),
        f.rot_t,
        f.hatch.p,
        u8::from(f.hatch.open),
        f.spin.ang,
        f.spin.w,
        f.spin.dbg.replace(',', ";")
    );
    for w in 0..2 {
        for b in 0..BONES {
            for k in 0..7 {
                line.push_str(&format!(",{:.5}", rig_bone(w, b, k).unwrap_or(0.0)));
            }
        }
    }
    let diag = |slot: i32, field: i32| super::belt::native::<i32>("VRRigWriteDiag", &[Value::Int32(slot), Value::Int32(field)]).unwrap_or(-1);
    let n = diag(-1, 0).clamp(0, 32);
    let held = cvr_rig::vrik::weapon_rig::HELD_STATS.each_ref().map(|c| c.load(std::sync::atomic::Ordering::Relaxed));
    line.push_str(&format!(",held {}/{}/{}|", held[0], held[1], held[2]));
    for k in 0..n {
        let v: Vec<String> = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 12, 13, 14, 15].iter().map(|&fl| diag(k, fl).to_string()).collect();
        line.push_str(&v.join(":"));
        line.push(';');
    }
    line.push('\n');
    let _ = sink.out.write_all(line.as_bytes());
    sink.rows += 1;
    if sink.rows % 20 == 0 {
        let _ = sink.out.flush();
    }
}
