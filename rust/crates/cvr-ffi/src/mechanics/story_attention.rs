//! mods/cet/CyberpunkVRPort_Stereo/modules/story_attention.lua (narrow story fixes adapted from Ajson44 V39; no quest
//! facts are written): every 50 ms the player's story context -- the q110_02 camera scan's facts, the locomotion,
//! scene tier and vehicle states, a workspot (locomotion 9), the passenger hub's choices (vehicle 3, 4, 7) -- into the
//! attention selection (VRStoryAttentionUpdate), and the manual focus clue looked at (the object's
//! CyberpunkVRPortManualFocusClueIndex, mods/redscript/CyberpunkVRPort_StoryAttention) into the clue press
//! (VRManualClueUpdate): on its claimed press the object inspects the clue itself (CyberpunkVRPortInspectManualFocusClue
//! revalidates the scan and the identity). No game object outlives a tick; without a context (the VR menu, a menu, no
//! player, a failure) the attention gets "paused" and the clue none. The Running state's exit publishes neither.
use crate::script_call::{call_handle, method, Value};
use cvr_host::log;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

struct State {
    elapsed: f64,
    error: Option<String>,
}
static STATE: Mutex<State> = Mutex::new(State { elapsed: 0.0, error: None });
static REPORTED: super::PerPlayer = super::PerPlayer::new();

/// What the attention selection takes besides the player.
#[derive(Debug)]
struct Context {
    started: i32,
    done: i32,
    locomotion: i32,
    tier: i32,
    workspot: i32,
    vehicle: i32,
    choices: i32,
}

/// A quest fact (QuestsSystem.GetFactStr).
fn fact(quests: *mut core::ffi::c_void, name: &core::ffi::CStr) -> Result<i32, String> {
    let name = CString::new(name).ok_or("no string")?;
    // SAFETY: the quests system; the name lives for the call.
    unsafe { method::<i32>(quests, "GetFactStr", &[Value::Ptr((&name as *const CString).cast())]) }.ok_or_else(|| "GetFactStr failed".into())
}

/// The mod's context(): None when there is nothing to attend (the VR menu open, no player, a menu up, no state machine).
fn context(player: &Handle) -> Result<Option<Context>, String> {
    use super::boards::{definition, field, get, hub_choices, in_menu, local_board};
    if super::hud_clearance::OVERLAY_ACTIVE.load(Ordering::Relaxed) || player.is_null() {
        return Ok(None);
    }
    let defs = super::boards::defs()?;
    if in_menu(&defs)? != Some(false) {
        return Ok(None);
    }
    // SAFETY: the player, held by the caller.
    let id = unsafe { method::<u64>(player.instance, "GetEntityID", &[]) }.ok_or("GetEntityID failed")?;
    let psm_def = definition(&defs, "PlayerStateMachine")?;
    let psm = local_board(id, psm_def)?;
    if psm.is_null() {
        return Ok(None);
    }
    let locomotion = get::<i32>(&psm, "GetInt", field(psm_def, "Locomotion")?)?;
    let tier = get::<i32>(&psm, "GetInt", field(psm_def, "SceneTier")?)?;
    let vehicle = get::<i32>(&psm, "GetInt", field(psm_def, "Vehicle")?)?;
    let mut workspot = false;
    if locomotion == 9 {
        let system_held = super::game_system("GetWorkspotSystem");
        let system = system_held.instance;
        if !system.is_null() {
            // SAFETY: the workspot system; the player held.
            workspot = unsafe { method::<bool>(system, "IsActorInWorkspot", &[Value::Handle(player)]) }.ok_or("IsActorInWorkspot failed")?;
        }
    }
    let mut choices = false;
    if matches!(vehicle, 3 | 4 | 7) {
        // Missing passenger-hub data is not permission to replace its ray.
        choices = match hub_choices(&defs)? {
            Err(()) => true,
            Ok(n) => n.is_some_and(|n| n > 0),
        };
    }
    let quests_held = super::game_system("GetQuestsSystem");
    let quests = quests_held.instance;
    let (started, done) = if quests.is_null() {
        (0, 1)
    } else {
        (fact(quests, c"q110_02_camera_scan_start")?, fact(quests, c"q110_02_scanning_done")?)
    };
    Ok(Some(Context { started, done, locomotion, tier, workspot: workspot as i32, vehicle, choices: choices as i32 }))
}

/// The mod's clue(): the object looked at when it is a manual focus clue, with its index.
fn clue(player: &Handle) -> Result<Option<(Handle, i32)>, String> {
    let targeting_held = super::game_system("GetTargetingSystem");
    let targeting = targeting_held.instance;
    if targeting.is_null() {
        return Ok(None);
    }
    // SAFETY: the targeting system; the object held by its handle for the call after it.
    let object = unsafe { call_handle(targeting, "GetLookAtObject", &[Value::Handle(player), Value::Bool(false), Value::Bool(false)]) }
        .ok_or("GetLookAtObject failed")?;
    if object.is_null() {
        return Ok(None);
    }
    // the specific mesh AND the current scan state
    let index = manual_focus_clue_index(&object);
    Ok((index >= 0).then_some((object, index)))
}

fn report(s: &mut State, result: Result<(), String>) {
    match result {
        Ok(()) => {
            if s.error.take().is_some() {
                log!("[mechanics] story attention: working again\n");
            }
        }
        Err(e) => {
            if s.error.as_deref() != Some(e.as_str()) {
                log!("[mechanics] story attention: {e}\n");
            }
            s.error = Some(e);
        }
    }
}

/// The mod's tick: every 50 ms the attention and the clue press.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.elapsed += f64::from(dt).max(0.0);
    if s.elapsed < 0.05 {
        return;
    }
    s.elapsed = 0.0;
    let mut player = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut player) };
    let context = context(&player);
    let ok = context.is_ok();
    let context = context.unwrap_or_else(|e| {
        report(&mut s, Err(e));
        None
    });
    // Without a context the attention gets no player ("paused").
    let attended = if context.is_some() { player.clone() } else { Handle::null() };
    let c = context.unwrap_or(Context { started: 0, done: 1, locomotion: -1, tier: 0, workspot: 0, vehicle: 0, choices: 0 });
    // SAFETY: the player held for the call.
    let mode = unsafe {
        cvr_rig::vrik::story_attention::update_attention(
            &attended, c.started, c.done, c.locomotion, c.tier, c.workspot, c.vehicle, c.choices, attended.is_null() as i32,
        )
    };
    let clue = if attended.is_null() { Ok(None) } else { clue(&attended) };
    let found = clue.as_ref().map(|c| c.is_some()).unwrap_or(false);
    let clue = clue.unwrap_or_else(|e| {
        report(&mut s, Err(e));
        None
    });
    let object = clue.as_ref().map_or(Handle::null(), |(o, _)| o.clone());
    // SAFETY: the handles held for the call.
    let request = unsafe { cvr_rig::vrik::story_attention::update_manual_clue(&attended, &object) };
    if request == 1 {
        if let Some((object, index)) = &clue {
            // Explicit X edge only; revalidate scan and identity first.
            inspect_manual_focus_clue(object, *index);
        }
    }
    if ok && !attended.is_null() && REPORTED.first() {
        log!("[mechanics] story attention: from Rust ({c:?} mode={mode} clue={found})\n");
    }
}

/// The mod's shutdown: no attention, no clue.
pub(in crate::mechanics) fn shutdown() {
    STATE.lock().unwrap_or_else(|p| p.into_inner()).elapsed = 0.0;
    // SAFETY: null handles.
    unsafe {
        cvr_rig::vrik::story_attention::update_attention(&Handle::null(), 0, 1, -1, 0, 0, 0, 0, 1);
        cvr_rig::vrik::story_attention::update_manual_clue(&Handle::null(), &Handle::null());
    }
}

/// GameObject.CyberpunkVRPortManualFocusClueIndex (formerly mods/redscript/CyberpunkVRPort_StoryAttention): the
/// object's available clue when it is scanned through, enabled, not inspected, not auto-inspected, and the object
/// carries the Haitian book's mesh (q110_haitian_book_a); else -1.
fn manual_focus_clue_index(object: &Handle) -> i32 {
    use super::lobby_rtti as rt;
    let sc = rt::field(object, "m_scanningComponent").map(|v| v.handle()).unwrap_or_else(Handle::null);
    if sc.is_null() {
        return -1;
    }
    let num = |n: &str| rt::num(&sc, n, &[]).unwrap_or(-1.0);
    let flag = |n: &str| rt::call(&sc, n, &[]).is_some_and(|o| rt::Val::of(&o).bool());
    let index = num("GetAvailableClueIndex") as i32;
    if index < 0
        || !flag("IsScanned")
        || num("GetScanningProgress") < 1.0
        || !flag("IsAnyClueEnabled")
        || flag("IsClueInspected")
        || flag("IsActiveClueUsingAutoInspect")
    {
        return -1;
    }
    let book = crate::engine_objects::entity_components(object)
        .iter()
        .any(|c| rt::call(c, "GetName", &[]).is_some_and(|o| rt::Val::of(&o).text().contains("q110_haitian_book_a")));
    if book {
        index
    } else {
        -1
    }
}
/// GameObject.CyberpunkVRPortInspectManualFocusClue (the same module): the clue inspected when it is still the one.
fn inspect_manual_focus_clue(object: &Handle, index: i32) -> bool {
    use super::lobby_rtti::{self as rt, A};
    if index < 0 || manual_focus_clue_index(object) != index {
        return false;
    }
    let sc = rt::field(object, "m_scanningComponent").map(|v| v.handle()).unwrap_or_else(Handle::null);
    rt::call(&sc, "SetClueState", &[A::N(f64::from(index)), A::B(true), A::B(true), A::B(true), A::B(false)]);
    rt::call(&sc, "IsClueInspected", &[]).is_some_and(|o| rt::Val::of(&o).bool())
}
