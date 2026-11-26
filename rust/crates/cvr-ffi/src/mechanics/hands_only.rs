//! mods/cet/CyberpunkVRPort_HandsOnly (the player's body drawn as its hands alone: xr_body_hands_only, shared slot
//! [173]): every mesh component of the player is switched off except the arms drawn as their hands -- the game's arm
//! component masked to nothing and its copy cut at the wrists (vrp_hands__*, hands_only_data COPIES) following it, on
//! while it is on, in its skin tone, with the game's mask ANDed with the copy's own; arm pieces without a copy keep
//! their hand chunks (MASKS); an arm mesh neither table knows is kept whole -- and the port's own pieces (vrp_*, the
//! belt's items, the Smart Link grip, gloves) but the neck cap and the body stand-in. The mode ends with every mask the
//! game's again, every piece switched off here on again, every copy off. Passes every 0.25 s (the game turns garments
//! back on with the wardrobe). A scene that plays the body (VRSceneOwnsBody) gets it back, faded with the engine's
//! dissolve both ways (VRComponentDissolve/Dissolved/DissolveReady): in, every piece comes back dissolved out and fades
//! in once attached, the copies leaving after; out, every piece fades out first and only then goes. Nothing is touched
//! once the player detaches (the Lua observer reports it: VRHandsOnlyDetach). Natives are called as the Lua called them.
use super::hands_only_data::{COPIES, MASKS};
use crate::script_call::{call_handle, call_native, method, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

const SCAN_EVERY: f64 = 0.25;
const KEEP_PREFIX: [&str; 4] = ["vrp_", "ucbelt", "a0_004__weapon_grip", "g1_"];
/// Never kept, whatever the tables say: the personal link's cable and plug hang from the forearm.
const NEVER: [&str; 1] = ["personal_link"];
const DROP: [&str; 2] = ["vrp_neck_cap", "vrp_body_stand_in"];
/// The engine's dissolve (0.45 s measured) and a margin.
const FADE: f64 = 0.55;
/// A piece switched on is given a frame or two before it is asked whether it is attached, and faded in after this long.
const SHOW_DELAY: f64 = 0.05;
const SHOW_LIMIT: f64 = 1.5;
/// The engine's dissolve modes (measured): 0 fades both ways; 2 fades out the same but comes back in at once.
const DISSOLVE_NOW: i32 = 2;
const DISSOLVE_FADE: i32 = 0;
/// The arms themselves, by the names the player entities give them.
const ARM_NAMES: [&str; 14] = [
    "_hq__l",
    "_hq__r",
    "_hq__full",
    "_fpp__full",
    "left_arm",
    "right_arm",
    "strongarms_l",
    "strongarms_r",
    "strongarms_holstered_l",
    "strongarms_holstered_r",
    "mantisblade_left",
    "mantisblade_right",
    "launcher_holstered",
    "fists",
];

/// The second half of a fade, run once due (and ready).
enum Run {
    MaskZero(Handle, String),
    ToggleOff(Handle, String),
    Reveal(Handle),
    CopyOff(Handle),
}
enum Ready {
    Attached(Handle),
    ArmsIn,
}
struct Pending {
    at: f64,
    limit: f64,
    ready: Option<Ready>,
    run: Run,
}
struct Mask {
    game: u64,
    ours: i64,
}
struct Want {
    on: bool,
    mask: u64,
    keep: u64,
    tone: Option<u64>,
}
struct State {
    t: f64,
    at: f64,
    on: bool,
    force: Option<bool>,
    scene: Option<bool>,
    hidden: HashSet<String>,
    masks: HashMap<String, Mask>,
    copies: HashSet<String>,
    unknown: HashSet<String>,
    pending: Vec<Pending>,
    revealing: i32,
    revealed_at: f64,
    fading: HashSet<String>,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

fn native_bool(name: &str, values: &[Value]) -> Option<bool> {
    // SAFETY: one of the plugin's natives, as the script VM calls it.
    unsafe { call_native::<bool>(name, values) }
}
fn enabled(c: &Handle) -> bool {
    // SAFETY: the component, held.
    unsafe { method::<bool>(c.instance, "IsEnabled", &[]) }.unwrap_or(false)
}
fn toggle(c: &Handle, on: bool) {
    // SAFETY: as above.
    unsafe { method::<()>(c.instance, "Toggle", &[Value::Bool(on)]) };
}
fn nudge(c: &Handle) {
    if enabled(c) {
        toggle(c, false);
        toggle(c, true);
    }
}
fn dissolve(c: &Handle, hidden: bool, mode: i32) -> bool {
    native_bool("VRComponentDissolve", &[Value::Handle(c), Value::Bool(hidden), Value::Int32(mode)]) == Some(true)
}
fn undissolve(c: &Handle) {
    if native_bool("VRComponentDissolved", &[Value::Handle(c)]) == Some(true) {
        dissolve(c, false, DISSOLVE_NOW);
    }
}
fn dissolve_ready(c: &Handle) -> bool {
    native_bool("VRComponentDissolveReady", &[Value::Handle(c)]).is_none_or(|r| r)
}
/// A component's 8-byte property (chunkMask, meshAppearance, the mesh's hash).
fn prop_u64(c: &Handle, name: &str) -> Option<u64> {
    // SAFETY: the component, held; an 8-byte property.
    unsafe { crate::script_call::property(c.instance, name) }.map(|p| unsafe { *(p as *const u64) })
}
fn set_prop_u64(c: &Handle, name: &str, v: u64) {
    // SAFETY: as above.
    if let Some(p) = unsafe { crate::script_call::property(c.instance, name) } {
        unsafe { *(p as *mut u64) = v };
    }
}
fn low_bits(v: u64, bits: u32) -> u64 {
    if bits >= 64 {
        v
    } else {
        v & ((1u64 << bits) - 1)
    }
}
fn kept(name: &str) -> bool {
    !DROP.contains(&name) && (KEEP_PREFIX.iter().any(|p| name.starts_with(p)) || name.contains("glove"))
}
fn arm_like(name: &str) -> bool {
    ARM_NAMES.iter().any(|a| name.contains(a))
}
fn is_mesh(c: &Handle) -> bool {
    let is = |class: &str| {
        // SAFETY: the component, held.
        unsafe { method::<bool>(c.instance, "IsA", &[Value::U64(CName::hashed(class).0)]) }.unwrap_or(false)
    };
    is("entMeshComponent") || is("entISkinTargetComponent")
}
fn later(s: &mut State, delay: f64, run: Run, ready: Option<Ready>, limit: Option<f64>) {
    s.pending.push(Pending { at: s.t + delay, limit: s.t + limit.unwrap_or(delay), ready, run });
}
/// The mod's gameMask(): the game's mask of a component this mode masks.
fn game_mask(s: &mut State, c: &Handle, key: &str, ours: i64) -> Option<u64> {
    let cur = prop_u64(c, "chunkMask")?;
    if let Some(rec) = s.masks.get(key) {
        if low_bits(cur, 53) as i64 == rec.ours {
            return Some(rec.game);
        }
    }
    s.masks.insert(key.to_string(), Mask { game: cur, ours });
    Some(cur)
}
fn write_mask(s: &mut State, c: &Handle, key: &str, ours: u64) {
    let Some(rec) = s.masks.get_mut(key) else { return };
    rec.ours = ours as i64;
    if prop_u64(c, "chunkMask").is_some_and(|cur| low_bits(cur, 53) == ours) {
        return;
    }
    set_prop_u64(c, "chunkMask", ours);
    nudge(c);
}
fn run(s: &mut State, r: Run) {
    match r {
        Run::MaskZero(c, key) => {
            s.fading.remove(&key);
            write_mask(s, &c, &key, 0);
        }
        Run::ToggleOff(c, key) => {
            s.fading.remove(&key);
            toggle(&c, false);
        }
        Run::Reveal(c) => {
            s.revealing = (s.revealing - 1).max(0);
            s.revealed_at = s.t;
            dissolve(&c, false, if enabled(&c) { DISSOLVE_FADE } else { DISSOLVE_NOW });
        }
        Run::CopyOff(c) => {
            if enabled(&c) {
                toggle(&c, false);
            }
        }
    }
}
/// Every second half that is due (all of them with `all`: a new switch finishes the last one first).
fn run_pending(s: &mut State, all: bool) {
    if s.pending.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut s.pending);
    let mut due = Vec::new();
    for p in pending {
        let mut ready = s.t >= p.at;
        if ready && s.t < p.limit {
            if let Some(r) = &p.ready {
                ready = match r {
                    Ready::Attached(c) => !enabled(c) || dissolve_ready(c),
                    Ready::ArmsIn => s.revealing == 0 && s.t - s.revealed_at >= FADE,
                };
            }
        }
        if all || ready {
            due.push(p);
        } else {
            s.pending.push(p);
        }
    }
    for p in due {
        run(s, p.run);
    }
}
fn setting(s: &State) -> bool {
    // SAFETY: the shared block's read.
    s.force.unwrap_or_else(|| unsafe { cvr_rig::vrik::provider::shared_slot(173) } > 0.5)
}
fn scene_owns_body(s: &State) -> bool {
    s.scene.unwrap_or_else(|| native_bool("VRSceneOwnsBody", &[]) == Some(true))
}

/// One pass over the player's mesh components; `fade`: the mode is switching, not one of the periodic passes.
fn walk(s: &mut State, pl: &Handle, on: bool, fade: bool) {
    let mut seen: HashMap<String, u32> = HashMap::new();
    let mut want: HashMap<&'static str, Want> = HashMap::new();
    for c in crate::engine_objects::entity_components(pl) {
        if !is_mesh(&c) {
            continue;
        }
        // SAFETY: the component, held.
        let name = unsafe { method::<u64>(c.instance, "GetName", &[]) }.map(|n| CName(n).text().to_string()).unwrap_or_default();
        let n = seen.entry(name.clone()).or_insert(0);
        *n += 1;
        let key = format!("{name}#{n}");
        if on {
            if s.fading.contains(&key) || kept(&name) {
                continue;
            }
            let h = prop_u64(&c, "mesh");
            let never = NEVER.iter().any(|x| name.contains(x));
            let copy = h.filter(|_| !never).and_then(|h| COPIES.iter().find(|e| e.0 == h));
            let row = h.filter(|_| !never).and_then(|h| MASKS.iter().find(|e| e.0 == h));
            if let Some(&(_, copy_name, copy_mask)) = copy {
                // The game's arm: drawn as nothing, its copy drawn in its place while the game has it on.
                if let Some(game) = game_mask(s, &c, &key, 0) {
                    if fade && enabled(&c) && low_bits(game, 53) != 0 && dissolve(&c, true, DISSOLVE_FADE) {
                        s.fading.insert(key.clone());
                        later(s, FADE, Run::MaskZero(c.clone(), key.clone()), None, None);
                    } else {
                        write_mask(s, &c, &key, 0);
                    }
                    let w = want.entry(copy_name).or_insert(Want { on: false, mask: 0, keep: copy_mask, tone: None });
                    if enabled(&c) {
                        w.on = true;
                        w.mask |= low_bits(game, 24) & w.keep & 0xFF_FFFF;
                        if w.tone.is_none() {
                            w.tone = prop_u64(&c, "meshAppearance");
                        }
                    }
                }
            } else if let Some(&(_, mask, chunks)) = row {
                // An arm piece without a copy: its hand chunks only.
                if let Some(game) = game_mask(s, &c, &key, -1) {
                    let bits = chunks.min(24);
                    write_mask(s, &c, &key, low_bits(low_bits(game, 24) & mask, bits));
                }
            } else if arm_like(&name) {
                let tag = h.map(|h| h.to_string()).unwrap_or_else(|| name.clone());
                if s.unknown.insert(tag) {
                    log!("[HandsOnly] arm mesh not in the tables, kept whole: {name} {}\n", h.map_or("nil".into(), |h| h.to_string()));
                }
            } else if enabled(&c) {
                s.hidden.insert(key.clone());
                if fade && dissolve(&c, true, DISSOLVE_FADE) {
                    s.fading.insert(key.clone());
                    later(s, FADE, Run::ToggleOff(c.clone(), key.clone()), None, None);
                } else {
                    toggle(&c, false);
                }
            }
        } else {
            let masked = s.masks.get(&key).map(|r| r.game);
            let hidden = s.hidden.contains(&key);
            // Dissolved out while it is off, so the proxy it is given on comes in unseen.
            let veiled = fade && (masked.is_some() || hidden) && dissolve(&c, true, DISSOLVE_NOW);
            if let Some(game) = masked {
                set_prop_u64(&c, "chunkMask", game);
                if veiled && enabled(&c) {
                    toggle(&c, false);
                    dissolve(&c, true, DISSOLVE_NOW);
                    toggle(&c, true);
                } else {
                    nudge(&c);
                }
            }
            if hidden {
                toggle(&c, true);
            }
            if veiled {
                // Faded in once it is attached; one the game keeps off has only its dissolve cleared.
                s.revealing += 1;
                later(s, SHOW_DELAY, Run::Reveal(c.clone()), Some(Ready::Attached(c.clone())), Some(SHOW_LIMIT));
            } else if masked.is_some() || hidden {
                undissolve(&c);
            }
        }
    }
    // The copies: each as its game arm(s) want it, every other one off.
    let mut names: Vec<String> = want.keys().map(|k| k.to_string()).collect();
    for k in &s.copies {
        if !names.contains(k) {
            names.push(k.clone());
        }
    }
    for copy_name in names {
        let w = if on { want.get(copy_name.as_str()) } else { None };
        // SAFETY: the player, held.
        let comp = unsafe { call_handle(pl.instance, "FindComponentByName", &[Value::U64(CName::hashed(&copy_name).0)]) }.unwrap_or_else(Handle::null);
        if !comp.is_null() {
            match w.filter(|w| w.on && w.mask != 0) {
                Some(w) => {
                    if let Some(tone) = w.tone {
                        if prop_u64(&comp, "meshAppearance").is_some_and(|cur| CName(cur).text() != CName(tone).text()) {
                            crate::engine_objects::component_change_appearance(&comp, tone, false);
                        }
                    }
                    if prop_u64(&comp, "chunkMask").is_some_and(|cur| low_bits(cur, 53) != w.mask) {
                        set_prop_u64(&comp, "chunkMask", w.mask);
                        nudge(&comp);
                    }
                    if !enabled(&comp) {
                        toggle(&comp, true);
                    }
                    s.copies.insert(copy_name.clone());
                }
                None => {
                    if enabled(&comp) {
                        if fade && !on {
                            // The hands stay until the arms have faded in around them.
                            later(s, 0.0, Run::CopyOff(comp.clone()), Some(Ready::ArmsIn), Some(SHOW_LIMIT + FADE));
                        } else {
                            toggle(&comp, false);
                        }
                    }
                    s.copies.remove(&copy_name);
                }
            }
        } else if w.is_some() && s.unknown.insert(copy_name.clone()) {
            log!("[HandsOnly] no {copy_name} on this player entity (an entity of another mod?)\n");
        }
    }
    if !on {
        s.hidden.clear();
        s.masks.clear();
        s.copies.clear();
    }
}

fn fresh() -> State {
    State {
        t: 0.0,
        at: -1.0,
        on: false,
        force: None,
        scene: None,
        hidden: HashSet::new(),
        masks: HashMap::new(),
        copies: HashSet::new(),
        unknown: HashSet::new(),
        pending: Vec::new(),
        revealing: 0,
        revealed_at: -1.0,
        fading: HashSet::new(),
    }
}
/// The mod's onUpdate: the mode's state every frame, the passes every 0.25 s, none while a fade is to finish.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(fresh);
    s.t += f64::from(dt);
    run_pending(s, false);
    let want = setting(s) && !scene_owns_body(s);
    let switch = want != s.on;
    if !switch && (s.t - s.at < SCAN_EVERY || !s.pending.is_empty()) {
        return;
    }
    s.at = s.t;
    if !want && !s.on {
        return;
    }
    let mut pl = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut pl) };
    if pl.is_null() {
        return;
    }
    if switch {
        run_pending(s, true);
    }
    walk(s, &pl, want, switch);
    if switch {
        log!("[HandsOnly] {}{} (from Rust)\n", if want { "on" } else { "off" }, if scene_owns_body(s) { " (scene)" } else { "" });
    }
    s.on = want;
}
/// VRHandsOnlyDetach: the session's components go with it -- forgotten without being touched.
pub(in crate::mechanics) fn detach() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        s.hidden.clear();
        s.masks.clear();
        s.copies.clear();
        s.on = false;
        s.pending.clear();
        s.fading.clear();
        s.revealing = 0;
    }
}
/// The console's force(v) / scene(v) (-1: follow the setting / the game).
pub(in crate::mechanics) fn force(force: i32, scene: i32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(fresh);
    let opt = |v: i32| match v {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    };
    if force != -2 {
        s.force = opt(force);
    }
    if scene != -2 {
        s.scene = opt(scene);
    }
}
/// The mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        run_pending(s, true);
        if s.on {
            let mut pl = Handle::null();
            // SAFETY: GetPlayer(GameInstance) into a handle of ours.
            unsafe { crate::anim_lookup::get_player(&mut pl) };
            if !pl.is_null() {
                walk(s, &pl, false, false);
            }
        }
    }
}
