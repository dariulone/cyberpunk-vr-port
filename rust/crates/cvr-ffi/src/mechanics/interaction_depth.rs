//! mods/cet/CyberpunkVRPort_Stereo/modules/interaction_depth.lua (the depth of the object an interaction prompt is
//! about, for the prompt panel): every 33 ms, unless the VR menu is open, while a prompt is on screen (the
//! UIInteractions blackboard's InteractionChoiceHub variant holds choices), the distance along the camera's forward
//! axis -- the interaction system picks its object from the centre of the view -- to the look-at object's position
//! (items, NPCs, devices) or, with none or one behind the camera, to the first static surface straight ahead (doors,
//! panels built into walls) goes to SetVRInteractionDepth's slot; 0 without a prompt. The math is in f64, as Lua's
//! numbers were.
use super::view::{self, V3};
use crate::script_call::{call_handle, method, Value};
use cvr_host::log;
use red4ext_abi::handle::Handle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const MAX_RAY: f64 = 6.0;

struct State {
    elapsed: f64,
    error: Option<String>,
}
static STATE: Mutex<State> = Mutex::new(State { elapsed: 0.0, error: None });
static REPORTED_HUB: super::PerPlayer = super::PerPlayer::new();
static REPORTED_DEPTH: AtomicBool = AtomicBool::new(false);

/// The mod's promptVisible(): the UIInteractions blackboard's InteractionChoiceHub holds a hub with choices.
fn prompt_visible() -> Result<bool, String> {
    let defs = super::boards::defs()?;
    let Ok(choices) = super::boards::hub_choices(&defs)? else { return Ok(false) };
    if REPORTED_HUB.first() {
        log!("[mechanics] interaction depth: InteractionChoiceHub read from Rust (choices={choices:?})\n");
    }
    Ok(choices.is_some_and(|n| n > 0))
}

/// The mod's measure(): the depth along the camera's forward axis, 0 when there is nothing to measure.
fn measure() -> Result<(f64, &'static str), String> {
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    if player.is_null() || !prompt_visible()? {
        return Ok((0.0, "none"));
    }
    let Some((o, f4)) = view::camera()? else { return Ok((0.0, "none")) };
    let f = view::v3(f4);
    // 1. The object the interaction system is looking at.
    let targeting_held = super::game_system("GetTargetingSystem");
    let targeting = targeting_held.instance;
    if !targeting.is_null() {
        // SAFETY: the targeting system; the object held by its handle for the call after it.
        let object = unsafe { call_handle(targeting, "GetLookAtObject", &[Value::Handle(&player), Value::Bool(false), Value::Bool(false)]) }
            .ok_or("GetLookAtObject failed")?;
        if !object.is_null() {
            // SAFETY: as above.
            let p = unsafe { method::<[f32; 4]>(object.instance, "GetWorldPosition", &[]) }.ok_or("GetWorldPosition failed")?;
            let d = view::depth(view::v3(p), o, f);
            if d > 0.2 && d < 20.0 {
                return Ok((d, "look-at object"));
            }
        }
    }
    // 2. The first static surface straight ahead.
    let sq_held = super::game_system("GetSpatialQueriesSystem");
    let sq = sq_held.instance;
    if sq.is_null() {
        return Ok((0.0, "none"));
    }
    let end: V3 = [o[0] + f[0] * MAX_RAY, o[1] + f[1] * MAX_RAY, o[2] + f[2] * MAX_RAY];
    if let Some(p) = view::raycast(sq, "SyncRaycastByCollisionGroup", o, end, "Static", &[Value::Bool(false), Value::Bool(false)])? {
        let d = view::depth(p, o, f);
        if d > 0.2 {
            return Ok((d, "static surface"));
        }
    }
    Ok((0.0, "none"))
}

/// The mod's tick: every 33 ms, the depth published (SetVRInteractionDepth's slot).
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.elapsed += f64::from(dt).max(0.0);
    if s.elapsed < 0.033 {
        return;
    }
    s.elapsed = 0.0;
    let mut depth = 0.0;
    if !super::hud_clearance::OVERLAY_ACTIVE.load(Ordering::Relaxed) {
        match measure() {
            Ok((d, source)) => {
                depth = d;
                if s.error.take().is_some() {
                    log!("[mechanics] interaction depth: measuring again\n");
                }
                if d > 0.0 && !REPORTED_DEPTH.swap(true, Ordering::Relaxed) {
                    log!("[mechanics] interaction depth: measured from Rust (depth={d:.3}, {source})\n");
                }
            }
            Err(e) => {
                if s.error.as_deref() != Some(e.as_str()) {
                    log!("[mechanics] interaction depth: {e}\n");
                }
                s.error = Some(e);
            }
        }
    }
    cvr_xr::hud_natives::set_interaction_depth(depth as f32);
}
