//! mods/cet/CyberpunkVRPort_Stereo/modules/hud_clearance.lua (how much free space is in front of the eyes, for the
//! HUD panels to stay in front of the world): every 33 ms, unless the VR menu is open, a fan of rays across the panels'
//! area -- 5 across, 3 down, from the camera, static geometry only (SyncRaycastByQueryPreset with staticOnly: it skips
//! the player's own body and hands, the rays start inside V's head) -- and the people nearby (the player's
//! VRHudPeopleDepth, mods/redscript/CyberpunkVRPort_HudClearance). The nearest hit's depth along the camera's forward
//! axis -- where a flat panel facing the player would touch it -- goes to SetVRHudClearance's slot: 99 when nothing is
//! within reach, 0 without a player, in a menu, with the VR menu open or when the measure failed. The math is in f64,
//! as Lua's numbers were.
use super::view::{cross, depth, norm, v3, V3};
use crate::script_call::{method, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// The raycast presets, the remembered one tried first.
const PRESETS: [&str; 3] = ["Sight Blocker", "Camera", "Bullet"];
const MAX_RAY: f64 = 5.5;
const PEOPLE_RANGE: f64 = 3.0;
/// Tangents: about +/-32 deg across.
const H: [f64; 5] = [-0.62, -0.31, 0.0, 0.31, 0.62];
/// About -22 / +20 deg down and up.
const V: [f64; 3] = [-0.40, 0.0, 0.36];

/// VrOverlay.active as the Stereo mod's onUpdate reports it (VRStereoOverlayActive): the VR menu is open.
pub(in crate::mechanics) static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

struct State {
    elapsed: f64,
    /// The preset that answered last (an index into PRESETS).
    pick: usize,
    error: Option<String>,
}
static STATE: Mutex<State> = Mutex::new(State { elapsed: 0.0, pick: 0, error: None });
static REPORTED: super::PerPlayer = super::PerPlayer::new();
/// The first measure whose rays hit something (the world's walls; nothing at the main menu).
static HIT_REPORTED: AtomicBool = AtomicBool::new(false);

/// The mod's ray(): the remembered preset first, its "nothing" trusted; a hit makes its preset the remembered one.
fn ray(sq: *mut c_void, pick: &mut usize, o: V3, d: V3) -> Result<Option<V3>, String> {
    let b = [o[0] + d[0] * MAX_RAY, o[1] + d[1] * MAX_RAY, o[2] + d[2] * MAX_RAY];
    for n in 1..=PRESETS.len() {
        let i = (*pick + n - 1) % PRESETS.len();
        if let Some(p) = super::view::raycast(sq, "SyncRaycastByQueryPreset", o, b, PRESETS[i], &[Value::Bool(true)])? {
            *pick = i;
            return Ok(Some(p));
        }
        if n == 1 {
            return Ok(None);
        }
    }
    Ok(None)
}

/// The one-time ray through the player's chest (its world position, 1.2 m up), twice as far: the first preset that
/// hits, with the hit.
fn puppet_ray(sq: *mut c_void, player: &Handle, o: V3) -> Result<Option<(&'static str, V3)>, String> {
    // SAFETY: the player, held by the caller.
    let at = unsafe { method::<[f32; 4]>(player.instance, "GetWorldPosition", &[]) }.ok_or("GetWorldPosition failed")?;
    let target = [at[0] as f64, at[1] as f64, at[2] as f64 + 1.2];
    let Some(d) = norm([target[0] - o[0], target[1] - o[1], target[2] - o[2]]) else { return Ok(None) };
    let reach = 2.0 * depth(target, o, d).max(0.5);
    let end = [o[0] + d[0] * reach, o[1] + d[1] * reach, o[2] + d[2] * reach];
    for preset in ["Bullet", "Sight Blocker", "Camera"] {
        if let Some(hit) = super::view::raycast(sq, "SyncRaycastByQueryPreset", o, end, preset, &[Value::Bool(false)])? {
            return Ok(Some((preset, hit)));
        }
    }
    Ok(None)
}

/// What the measure saw, for the one-time report.
struct Seen {
    camera: V3,
    hits: usize,
    people: Option<f32>,
    /// The one-time proof of the raycast's out parameter: a ray straight down from the camera (any geometry).
    down: Result<Option<V3>, String>,
    /// And a ray from the camera through the player's chest, static and dynamic geometry, each preset: the first hit.
    puppet: Result<Option<(&'static str, V3)>, String>,
}

/// The mod's measure(): the nearest depth along the camera's forward axis (99: nothing hit), 0 when there is nothing to
/// measure (no player, a menu up, no camera).
fn measure(pick: &mut usize, seen: &mut Option<Seen>) -> Result<f64, String> {
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() {
        return Ok(0.0);
    }
    if super::world_map::game_says_in_menu() != Some(false) {
        return Ok(0.0);
    }
    let sq_held = super::game_system("GetSpatialQueriesSystem");
    let sq = sq_held.instance;
    let camera = super::view::camera()?;
    let Some((o, f4)) = camera.filter(|_| !sq.is_null()) else { return Ok(0.0) };
    let f = v3(f4);
    // Camera right (world up = +Z) and up.
    let Some(r) = norm(cross(f, [0.0, 0.0, 1.0])) else { return Ok(0.0) };
    let u = cross(r, f);
    let mut nearest = 99.0;
    let mut hits = 0;
    for v in V {
        for h in H {
            let d = norm([f[0] + r[0] * h + u[0] * v, f[1] + r[1] * h + u[1] * v, f[2] + r[2] * h + u[2] * v]);
            let p = match d {
                Some(d) => ray(sq, pick, o, d)?,
                None => None,
            };
            if let Some(p) = p {
                hits += 1;
                let d = depth(p, o, f);
                if d > 0.05 && d < nearest {
                    nearest = d;
                }
            }
        }
    }
    // People: the same area, as tangents of the fan's outermost rays (formerly the REDscript
    // PlayerPuppet.VRHudPeopleDepth: the NPCs' points within the view).
    let people = people_depth(&player, o, f, r, u, H[H.len() - 1], -V[0], PEOPLE_RANGE).map(|d| d as f32);
    if let Some(d) = people.map(f64::from) {
        if d > 0.05 && d < nearest {
            nearest = d;
        }
    }
    let down = if !REPORTED.pending() {
        Ok(None)
    } else {
        super::view::raycast(sq, "SyncRaycastByQueryPreset", o, [o[0], o[1], o[2] - MAX_RAY], PRESETS[0], &[Value::Bool(false)])
    };
    let puppet = if !REPORTED.pending() { Ok(None) } else { puppet_ray(sq, &player, o) };
    *seen = Some(Seen { camera: o, hits, people, down, puppet });
    Ok(nearest)
}

/// The mod's tick: every 33 ms, the clearance published (SetVRHudClearance's slot).
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.elapsed += f64::from(dt).max(0.0);
    if s.elapsed < 0.033 {
        return;
    }
    s.elapsed = 0.0;
    let mut clearance = 0.0;
    if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
        let mut seen = None;
        match measure(&mut s.pick, &mut seen) {
            Ok(c) => {
                clearance = c;
                if s.error.take().is_some() {
                    log!("[mechanics] hud clearance: measuring again\n");
                }
            }
            Err(e) => {
                if s.error.as_deref() != Some(e.as_str()) {
                    log!("[mechanics] hud clearance: {e}\n");
                }
                s.error = Some(e);
            }
        }
        if let Some(seen) = seen {
            if seen.hits > 0 && !HIT_REPORTED.swap(true, Ordering::Relaxed) {
                log!(
                    "[mechanics] hud clearance: first rays that hit, from Rust (clearance={clearance:.3} hits={}/15 people={:?} preset={})\n",
                    seen.hits,
                    seen.people,
                    PRESETS[s.pick]
                );
            }
            if REPORTED.first() {
                log!(
                    "[mechanics] hud clearance: measured from Rust (clearance={clearance:.3} hits={}/15 people={:?} preset={} camera=({:.2}, {:.2}, {:.2}); ray down: {}; ray through the player: {})\n",
                    seen.hits,
                    seen.people,
                    PRESETS[s.pick],
                    seen.camera[0],
                    seen.camera[1],
                    seen.camera[2],
                    match &seen.down {
                        Ok(Some(p)) => format!("hit at ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]),
                        Ok(None) => "nothing".into(),
                        Err(e) => e.clone(),
                    },
                    match &seen.puppet {
                        Ok(Some((preset, p))) => format!("{preset} hit at ({:.2}, {:.2}, {:.2})", p[0], p[1], p[2]),
                        Ok(None) => "nothing".into(),
                        Err(e) => e.clone(),
                    }
                );
            }
        }
    }
    cvr_xr::hud_natives::set_clearance(clearance as f32);
}

/// PlayerPuppet.VRHudPeopleDepth (formerly mods/redscript/CyberpunkVRPort_HudClearance): the NPCs within `max_dist`
/// (the targeting system's parts, each entity once), five points up each body (0.20 m + 0.35 m steps), each a 0.30 m
/// sphere; the nearest front depth along `forward` of one inside the view's tangents.
#[allow(clippy::too_many_arguments)]
fn people_depth(player: &Handle, origin: [f64; 3], forward: [f64; 3], right: [f64; 3], up: [f64; 3], tan_h: f64, tan_v: f64, max_dist: f64) -> Option<f64> {
    use super::lobby_rtti::{self as rt, Val, A};
    use super::menu_rt as r;
    const RADIUS: f64 = 0.30;
    let mut q = r::new_struct("TargetSearchQuery")?;
    let qv = r::val(&q);
    r::set(&qv, "testedSet", r::ev("TargetingSet", "Complete"));
    r::set(&qv, "maxDistance", max_dist);
    r::set(&qv, "filterObjectByDistance", 1.0);
    r::set(&qv, "ignoreInstigator", 1.0);
    let filter = rt::global_call("TSF_NPC", &[])?;
    if let Some(sf) = qv.f("searchFilter") {
        // SAFETY: the filter field and the global's result of the same type.
        unsafe { sf.ty.assign(sf.ptr, filter.as_ptr().cast()) };
    }
    let targeting = super::game_system("GetTargetingSystem");
    let (_, outs) = rt::call_outs(&targeting, "GetTargetParts", &[A::H(player), A::P(q.as_mut_ptr().cast_const().cast())], 1)?;
    let parts = outs.first()?;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let mut seen = Vec::new();
    let mut best = 99.0f64;
    for part in r::items(&Val::of(parts)) {
        let comp = rt::static_call("TS_TargetPartInfo", "GetComponent", &[A::P(part.ptr.cast_const())]).map(|o| Val::of(&o).handle()).unwrap_or_else(Handle::null);
        let e = r::o(&comp, "GetEntity", &[]);
        if e.is_null() || !r::is_a(&e, "GameObject") {
            continue;
        }
        let id = super::belt::entity_hash(&e).unwrap_or(0);
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        let Some(base) = rt::call(&e, "GetWorldPosition", &[]).map(|o| {
            let v = Val::of(&o);
            [r::fnum(&v, "X"), r::fnum(&v, "Y"), r::fnum(&v, "Z")]
        }) else {
            continue;
        };
        for k in 0..5 {
            let p = [base[0], base[1], base[2] + 0.20 + 0.35 * f64::from(k)];
            let d = [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
            let depth = dot(d, forward);
            if depth > 0.05 {
                let side = dot(d, right).abs() - RADIUS;
                let vertical = dot(d, up).abs() - RADIUS;
                if side <= tan_h * depth && vertical <= tan_v * depth {
                    best = best.min(depth - RADIUS);
                }
            }
        }
    }
    Some(best)
}
