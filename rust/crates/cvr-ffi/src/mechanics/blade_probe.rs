//! The blade natives' self-test (CVR_BLADE_TEST=1): some seconds after the first frame with a player, the people
//! within 40 m: each one's hit representation walked (its shapes, their capsules and slots), the Head's capsule read,
//! a segment swept through the person's middle; and resource tokens -- one found, one loaded and held until its load
//! finished, then given back (the loader's last strong reference: the token destroyed) -- the log says what each
//! answered.
use crate::blade_natives as bn;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::resource::{resource_path, TokenRef};
use std::sync::Mutex;

struct Test {
    frames: u32,
    stage: u8,
    token: TokenRef,
    held_for: u32,
    spawned: u64,
    waited: u32,
}
static STATE: Mutex<Test> = Mutex::new(Test { frames: 0, stage: 0, token: TokenRef::null(), held_for: 0, spawned: 0, waited: 0 });
static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn people(extra: Option<red4ext_abi::handle::Handle>) {
    let pl = super::menu_rt::player();
    let tsq = super::lobby_rtti::global_call("TSQ_ALL", &[]).is_some();
    let mut near = super::hand_collision::targets_near(&pl, 40.0);
    let npcs = super::melee::npcs(&pl, 40.0);
    if let Some(e) = extra {
        let hair = cvr_rig::vrik::hair_dangle::describe(e.instance.cast());
        let points = cvr_rig::vrik::hair_dangle::hair_points(e.instance.cast());
        log!("[blade-test] the spawned person's dangles: {}\n", hair);
        log!("[blade-test] their hair points: {} (\"{}\")\n", points.matches(';').count(), points.chars().take(120).collect::<String>());
        near.push(e);
    }
    log!("[blade-test] TSQ_ALL {}: {} targets within 40 m; the melee's NPC query: {}\n", if tsq { "made" } else { "MISSING" }, near.len(), npcs.len());
    let mut tested = 0;
    let mut seen: Vec<usize> = Vec::new();
    for e in near.iter().chain(npcs.iter()).filter(|e| super::menu_rt::is_a(e, "NPCPuppet")) {
        if seen.contains(&(e.instance as usize)) {
            continue;
        }
        seen.push(e.instance as usize);
        let comp = super::lobby_rtti::obj(e, "FindComponentByName", &[super::lobby_rtti::A::Name(CName::hashed("HitRepresentation").0)]);
        if comp.is_null() {
            log!("[blade-test] a person without a HitRepresentation component\n");
            continue;
        }
        let c = comp.instance.cast();
        let (mut shapes, mut capsules, mut slots) = (0, 0, 0);
        let mut first = None;
        bn::visit_shapes(c, |item| {
            shapes += 1;
            if let Some(cap) = item.capsule() {
                capsules += 1;
                if bn::slot_frame(c, item.binding()).is_some() {
                    slots += 1;
                }
                if first.is_none() {
                    first = Some((CName(item.name()).text().to_string(), CName(item.binding()).text().to_string(), cap));
                }
            }
            false
        });
        let head = bn::read_capsule(c, CName::hashed("Head").0, core::ptr::null_mut(), None);
        let at = super::hand_collision::world_pos(e).unwrap_or([0.0; 3]);
        let (a, b) = ([at[0] - 0.6, at[1], at[2] + 1.1], [at[0] + 0.6, at[1], at[2] + 1.1]);
        let (oa, ob) = ([at[0] - 0.6, at[1] - 0.5, at[2] + 1.1], [at[0] + 0.6, at[1] - 0.5, at[2] + 1.1]);
        let sweep = bn::swept_contact(c, &[oa, ob, a, b], None);
        // where the slots are against the person, and the segment's nearest approach to a capsule's centre
        let mut nearest = (f32::MAX, String::new(), [0f32; 3], [0f32; 3]);
        let mid = [at[0], at[1], at[2] + 1.1];
        bn::visit_shapes(c, |item| {
            if let (Some((cap, _)), Some(frame)) = (item.capsule(), bn::slot_frame(c, item.binding())) {
                let local = bn::to_local(&frame, &mid);
                let d = ((local[0] - cap[0]).powi(2) + (local[1] - cap[1]).powi(2) + (local[2] - cap[2]).powi(2)).sqrt();
                if d < nearest.0 {
                    let fp = frame.position();
                    nearest = (d, CName(item.name()).text().to_string(), [fp[0] - at[0], fp[1] - at[1], fp[2] - at[2]], local);
                }
            }
            false
        });
        log!("[blade-test] the middle's nearest capsule: {:?}\n", nearest);
        let head_hr = bn::slot_frame(c, CName::hashed("Head").0).map(|f| f.position());
        let attach = crate::engine_objects::entity_components(e).into_iter().find(|x| super::hand_collision::component_name(x) == "Item_Attachment_Slot");
        let head_attach = attach.as_ref().and_then(|x| bn::slot_frame(x.instance.cast(), CName::hashed("Head").0)).map(|f| f.position());
        log!("[blade-test] the person at {:?}; Head by HitRepresentation {:?}, by Item_Attachment_Slot {:?}\n", at, head_hr, head_attach);
        log!(
            "[blade-test] person: {} shapes, {} capsules, {} slots answer; first {:?}; Head {:?}; a segment swept into the middle {:?}\n",
            shapes,
            capsules,
            slots,
            first,
            head,
            sweep
        );
        tested += 1;
        if tested >= 3 {
            break;
        }
    }
}
pub(in crate::mechanics) fn tick() {
    if !*ON.get_or_init(|| std::env::var("CVR_BLADE_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.frames += 1;
    match s.stage {
        0 if s.frames >= 600 => {
            // a friendly person 1.5 m before the player, for the shapes
            let pl = super::menu_rt::player();
            let at = super::hand_collision::world_pos(&pl).unwrap_or([0.0; 3]);
            let fwd = super::lobby_rtti::call(&pl, "GetWorldForward", &[])
                .map(|o| {
                    let v = super::lobby_rtti::Val::of(&o);
                    [super::menu_rt::fnum(&v, "X") as f32, super::menu_rt::fnum(&v, "Y") as f32]
                })
                .unwrap_or([0.0, 1.0]);
            let pos = [at[0] + fwd[0] * 1.5, at[1] + fwd[1] * 1.5, at[2], 1.0];
            let yaw = (-fwd[0]).atan2(-fwd[1]);
            let q = [0.0, 0.0, (-yaw * 0.5).sin(), (-yaw * 0.5).cos()];
            s.spawned = crate::spawn_system::spawn_record(red4ext_abi::tweakdbid::from_name("Character.Judy"), &pos, &q);
            log!("[blade-test] spawned Character.Judy: id {}\n", s.spawned);
            s.stage = 3;
        }
        3 => {
            s.waited += 1;
            let e = crate::spawn_system::spawned(s.spawned);
            let ready = !e.is_null() && !super::lobby_rtti::obj(&e, "FindComponentByName", &[super::lobby_rtti::A::Name(CName::hashed("HitRepresentation").0)]).is_null();
            if !(ready && s.waited > 700) && s.waited < 1500 {
                return;
            }
            log!("[blade-test] the person is {} after {} frames\n", if ready { "in" } else { "NOT in" }, s.waited);
            people(ready.then_some(e));
            let found = TokenRef::find(resource_path("base\\gameplay\\gui\\common\\main_colors.inkstyle"));
            log!("[blade-test] FindToken: {} (loaded {})\n", !found.is_null(), found.loaded());
            drop(found);
            // a mesh the save has no use for: its token is the loader's and ours alone
            s.token = TokenRef::load_async(resource_path("base\\vehicles\\special\\av_zetatech_atlus\\entities\\meshes\\av_zetatech_atlus__ext01_body_01.mesh"));
            let strong = unsafe { s.token.ref_count.as_ref() }.map_or(0, |c| c.strong.load(core::sync::atomic::Ordering::Relaxed));
            log!("[blade-test] LoadAsync: {} (finished {}, strong {})\n", !s.token.is_null(), s.token.finished(), strong);
            s.stage = 1;
        }
        1 => {
            s.held_for += 1;
            if s.token.is_null() || s.token.finished() || s.held_for > 600 {
                let strong = unsafe { s.token.ref_count.as_ref() }.map_or(0, |c| c.strong.load(core::sync::atomic::Ordering::Relaxed));
                log!("[blade-test] the loaded token after {} frames: finished {} loaded {} strong {}\n", s.held_for, s.token.finished(), s.token.loaded(), strong);
                s.token = TokenRef::null();
                let id = s.spawned;
                log!("[blade-test] the spawned person let go: {}\n", crate::spawn_system::delete_spawned(id));
                log!("[blade-test] tokens given back\n");
                s.stage = 2;
            }
        }
        _ => {}
    }
}

/// The physical melee's damage self-test (CVR_MELEE_DAMAGE_TEST=1), every kind of melee weapon in turn: a knife and a
/// katana (the blade's sweep, VRBladeSweep), a baseball bat (a blunt weapon), the fists, the Gorilla Arms and the Mantis
/// Blades (the arms' contacts, VRCyberwareContacts, as the controller's motion drives them; the implants installed by
/// the console's EquipCyberware). For each: the hands emptied, the weapon put in the right hand, a Tyger Claw spawned
/// 1.3 m before the player, a thrust through their chest, their health read before and 3 s after -- the damage the
/// stroke dealt through the game's own pipeline -- and the person deleted. A case skipped (the weapon not in hand, the
/// person not ready) is tried once more after the others.
struct Damage {
    t: f64,
    /// the cases' order (DAMAGE_CASES' indices), the skipped ones appended once
    queue: Vec<usize>,
    skipped: Vec<usize>,
    retried: bool,
    case: usize,
    stage: u8,
    at: f64,
    spawned: u64,
    health: f32,
    dealt: Vec<(&'static str, f32)>,
}
static DAMAGE: Mutex<Damage> =
    Mutex::new(Damage { t: 0.0, queue: Vec::new(), skipped: Vec::new(), retried: false, case: 0, stage: 0, at: 0.0, spawned: 0, health: 0.0, dealt: Vec::new() });
static DAMAGE_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
/// (the case, how the weapon is had: an item record given, "fists", or the EquipCyberware kind)
const DAMAGE_CASES: [(&str, &str); 6] = [
    ("knife", "Items.Preset_Knife_Default"),
    ("katana", "Items.Preset_Katana_Default"),
    ("baseball bat", "Items.Preset_Baseball_Bat_Default"),
    ("fists", "fists"),
    ("gorilla arms", "gorilla"),
    ("mantis blades", "mantis"),
];
/// The damage test's target record (CVR_MELEE_DAMAGE_TARGET), its distance before the player (_DISTANCE, m) and the
/// thrust's height over its feet (_HEIGHT, m).
fn damage_target() -> String {
    std::env::var("CVR_MELEE_DAMAGE_TARGET").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| String::from("Character.tyger_claws_gangster1_melee1_fists_ma"))
}
fn damage_distance() -> f32 {
    std::env::var("CVR_MELEE_DAMAGE_DISTANCE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.5..=6.0).contains(v)).unwrap_or(1.3)
}
fn damage_height() -> f32 {
    std::env::var("CVR_MELEE_DAMAGE_HEIGHT").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.1..=4.0).contains(v)).unwrap_or(1.25)
}
fn health_of(e: &red4ext_abi::handle::Handle) -> f32 {
    use super::lobby_rtti::{self as rt, A};
    let pools = super::game_system("GetStatPoolsSystem");
    let sid = [super::melee::entity_id(e), 0u64];
    rt::num(&pools, "GetStatPoolValue", &[A::P(sid.as_ptr().cast()), A::N(super::menu_rt::ev("gamedataStatPoolType", "Health")), A::B(false)]).unwrap_or(-1.0) as f32
}
pub(in crate::mechanics) fn damage_tick(dt: f64) {
    if !*DAMAGE_ON.get_or_init(|| std::env::var("CVR_MELEE_DAMAGE_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    use super::lobby_rtti::{self as rt, A};
    let pl = super::menu_rt::player();
    if pl.is_null() || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) || !super::menu_rt::b(&pl, "IsAttached", &[]) {
        return;
    }
    let mut s = DAMAGE.lock().unwrap_or_else(|p| p.into_inner());
    s.t += dt;
    let t = s.t;
    if s.stage == 9 || t < 40.0 {
        return;
    }
    if s.queue.is_empty() {
        // CVR_MELEE_DAMAGE_CASES: the cases by name, comma-separated (default: every one)
        let only = std::env::var("CVR_MELEE_DAMAGE_CASES").unwrap_or_default();
        s.queue = (0..DAMAGE_CASES.len()).filter(|&i| only.is_empty() || only.split(',').any(|n| n.trim() == DAMAGE_CASES[i].0)).collect();
        if s.queue.is_empty() {
            s.stage = 9;
            return;
        }
    }
    let Some(index) = s.queue.get(s.case).copied() else { return };
    let (name, how) = DAMAGE_CASES[index];
    let held = super::blade::physical_weapon(&pl);
    let cyber = matches!(how, "gorilla" | "mantis");
    match s.stage {
        // the hands emptied
        0 => {
            super::holster::equip_request(&pl, "UnequipWeapon");
            s.stage = 1;
            s.at = t;
        }
        // the weapon asked for
        1 if t - s.at > 2.5 => {
            if cyber {
                log!("[melee-damage] {name}: EquipCyberware({how}) -> {:?}\n", super::vrik_mirror::equip_cyberware(how));
            } else if how == "fists" {
                log!("[melee-damage] {name}: RequestFists ({})\n", super::menu_rt::ev("EquipmentManipulationAction", "RequestFists"));
                super::holster::equip_request(&pl, "RequestFists");
            } else {
                let ts = super::belt::transactions();
                let id = rt::static_call("ItemID", "FromTDBID", &[A::Name(red4ext_abi::tweakdbid::from_name(how))]);
                let Some(id) = id else { return };
                let given = super::menu_rt::b(&ts, "GiveItem", &[A::H(&pl), A::P(id.as_ptr().cast()), A::N(1.0)]);
                let req = super::lobby_ink::new("GameplayEquipRequest");
                super::menu_main::set_handle_prop(&req, "owner", &pl);
                if let Some(f) = super::lobby_rtti::field(&req, "itemID") {
                    // SAFETY: an ItemID field, assigned from the call's ItemID of the same type.
                    unsafe { f.ty.assign(f.ptr, id.as_ptr().cast()) };
                }
                super::menu_rt::hset(&req, "slotIndex", 0.0);
                super::menu_rt::hset(&req, "forceEquipWeapon", 1.0);
                let c = super::game_system("GetScriptableSystemsContainer");
                let equipment = rt::obj(&c, "Get", &[A::Name(CName::hashed("EquipmentSystem").0)]);
                rt::call(&equipment, "QueueRequest", &[A::H(&req)]);
                log!("[melee-damage] {name}: {how} given ({given}), its equip and draw asked\n");
            }
            s.stage = 2;
            s.at = t;
        }
        // the weapon in hand: then the person spawned before the player
        2 => {
            let ready = !held.is_null()
                && super::blade::record_ready(&held)
                && match how {
                    "fists" => super::melee::weapon_is_type(&held, "Wea_Fists"),
                    "gorilla" => super::melee::kind(&held) == 1,
                    "mantis" => super::melee::kind(&held) == 2,
                    record => super::melee::item_id(&held)[0] == red4ext_abi::tweakdbid::from_name(record),
                };
            if !ready && t - s.at < 20.0 {
                return;
            }
            if !ready {
                let class = if held.is_null() { String::from("nothing") } else { super::belt::class_name(&held) };
                log!(
                    "[melee-damage] {name}: not in hand after 20 s ({class}, record {:#x}, fists {}; cyberware {}): skipped\n",
                    super::melee::item_id(&held)[0],
                    super::melee::weapon_is_type(&held, "Wea_Fists"),
                    super::vrik_mirror::cyberware_status()
                );
                s.dealt.push((name, f32::NAN));
                s.skipped.push(index);
                s.stage = 5;
                s.at = t;
                return;
            }
            let at = super::hand_collision::world_pos(&pl).unwrap_or([0.0; 3]);
            let fwd = rt::call(&pl, "GetWorldForward", &[])
                .map(|o| {
                    let v = rt::Val::of(&o);
                    [super::menu_rt::fnum(&v, "X") as f32, super::menu_rt::fnum(&v, "Y") as f32]
                })
                .unwrap_or([0.0, 1.0]);
            let d = damage_distance();
            let pos = [at[0] + fwd[0] * d, at[1] + fwd[1] * d, at[2], 1.0];
            let yaw = (-fwd[0]).atan2(-fwd[1]);
            let q = [0.0, 0.0, (-yaw * 0.5).sin(), (-yaw * 0.5).cos()];
            s.spawned = crate::spawn_system::spawn_record(red4ext_abi::tweakdbid::from_name(&damage_target()), &pos, &q);
            log!(
                "[melee-damage] {name}: in hand {} (melee kind {}, blade kind {}, reach {:.2}); {} spawned {d:.1} m away: id {}\n",
                super::belt::class_name(&held),
                super::melee::kind(&held),
                super::blade::kind(&held),
                super::melee::reach(&held),
                damage_target(),
                s.spawned
            );
            s.stage = 3;
            s.at = t;
        }
        // the person ready (their hit shapes): the thrust through the chest
        3 => {
            let e = crate::spawn_system::spawned(s.spawned);
            let ready = !e.is_null() && !super::blade::hit_representation(&e).is_null();
            if !(ready && t - s.at > 6.0) && t - s.at < 25.0 {
                return;
            }
            if !ready || held.is_null() {
                log!("[melee-damage] {name}: no person ({ready}) or no weapon in hand: skipped\n");
                s.dealt.push((name, f32::NAN));
                s.skipped.push(index);
                s.stage = 5;
                s.at = t;
                return;
            }
            let npc_at = super::hand_collision::world_pos(&e).unwrap_or([0.0; 3]);
            let me = super::hand_collision::world_pos(&pl).unwrap_or([0.0; 3]);
            let mut dir = [npc_at[0] - me[0], npc_at[1] - me[1], 0.0, 0.0];
            let l = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt().max(1e-3);
            dir = [dir[0] / l, dir[1] / l, 0.0, 0.0];
            let h = damage_height();
            let mut chest = [npc_at[0], npc_at[1], npc_at[2] + h, 1.0];
            // a body wider than a person's (a mech): the thrust 15 cm into the surface a ray from the player finds on it
            // at that height (CVR_MELEE_DAMAGE_SURFACE=1), every preset's answer logged
            if std::env::var("CVR_MELEE_DAMAGE_SURFACE").is_ok_and(|v| v == "1") {
                let sq = super::blade::queries();
                let from = [me[0], me[1], npc_at[2] + h, 1.0];
                let mut found = None;
                for preset in ["Melee", "Bullet", "Ragdoll", "Interaction", "Vehicle", "Dynamic Entity"] {
                    let mut t = super::blade::Trace::zeroed();
                    let hit = super::blade::raycast(&sq, &from, &chest, preset, false, &mut t);
                    let e2 = if hit { t.entity() } else { red4ext_abi::handle::Handle::null() };
                    let mine = !e2.is_null() && e2.instance == e.instance;
                    log!(
                        "[melee-damage] {name}: a ray by {preset} from the player at {h:.2} m: {hit}, {:.2} m out from the centre, on {} (the target: {mine})\n",
                        super::blade::dist(&t.position(), &chest),
                        if e2.is_null() { String::from("-") } else { super::belt::class_name(&e2) }
                    );
                    if hit && found.is_none() && (mine || preset == "Bullet") {
                        found = Some(t.position());
                    }
                }
                if let Some(p) = found {
                    chest = [p[0] + dir[0] * 0.15, p[1] + dir[1] * 0.15, p[2], 1.0];
                }
                let comp = super::blade::hit_representation(&e);
                log!(
                    "[melee-damage] {name}: the target's HitRepresentation {}; the thrust's centre {:.2} m from its axis\n",
                    !comp.is_null(),
                    ((chest[0] - npc_at[0]).powi(2) + (chest[1] - npc_at[1]).powi(2)).sqrt()
                );
                for line in crate::blade_natives::describe_shapes(comp.instance.cast(), 80) {
                    log!("[melee-damage]   shape {line}\n");
                }
                let (f3, c3) = ([from[0], from[1], from[2]], [npc_at[0], npc_at[1], npc_at[2] + h]);
                log!("[melee-damage]   the ray from the player to the axis is {:.2} m long\n", super::blade::dist(&from, &[c3[0], c3[1], c3[2], 1.0]));
                for line in crate::blade_natives::obb_entries(comp.instance.cast(), &f3, &c3) {
                    log!("[melee-damage]   box {line}\n");
                }
            }
            let at = |k: f32| [chest[0] - dir[0] * k, chest[1] - dir[1] * k, chest[2], 1.0];
            let (old_a, old_b, a, b) = (at(0.42), at(0.22), at(0.30), at(0.04));
            s.health = health_of(&e);
            let struck = if super::blade::kind(&held) > 0 {
                super::blade::sweep(&pl, &held, &old_a, &old_b, &a, &b, false, true, [dir[0] * 0.2, dir[1] * 0.2, 0.0, 0.0], false, 4.0)
            } else {
                let began = super::melee::begin_swing(&pl, &held, 1, false, 8.0);
                let hits = if began { super::melee::sweep_hit(&pl, &held, b, a, old_b, old_a, 0.08, false, [0.0; 4], false, false, 4.0) } else { 0 };
                if !began {
                    log!("[melee-damage] {name}: the stroke was refused\n");
                }
                hits
            };
            log!(
                "[melee-damage] {name}: the thrust struck {struck} (friendly {}, health {:.1})\n",
                super::blade::sb("GameObject", "IsFriendlyTowardsPlayer", &[A::H(&e)]),
                s.health
            );
            s.stage = 4;
            s.at = t;
        }
        // 3 s later: the health the stroke took
        4 if t - s.at > 3.0 => {
            let e = crate::spawn_system::spawned(s.spawned);
            let after = if e.is_null() { -1.0 } else { health_of(&e) };
            let dealt = s.health - after;
            log!("[melee-damage] {name}: health {:.1} -> {after:.1}: dealt {dealt:.1}\n", s.health);
            s.dealt.push((name, dealt));
            let id = s.spawned;
            log!("[melee-damage] {name}: the person deleted: {}\n", crate::spawn_system::delete_spawned(id));
            s.stage = 5;
            s.at = t;
        }
        // the next case
        5 if t - s.at > 1.5 => {
            s.case += 1;
            s.stage = 0;
            if s.case >= s.queue.len() && !s.retried && !s.skipped.is_empty() {
                let again = std::mem::take(&mut s.skipped);
                log!("[melee-damage] the skipped cases tried once more: {}\n", again.iter().map(|&i| DAMAGE_CASES[i].0).collect::<Vec<_>>().join(", "));
                s.queue.extend(again);
                s.retried = true;
            }
            if s.case >= s.queue.len() {
                super::holster::equip_request(&pl, "UnequipWeapon");
                let summary: Vec<String> = s.dealt.iter().map(|(n, d)| format!("{n} {d:.1}")).collect();
                log!("[melee-damage] done: {}\n", summary.join(", "));
                s.stage = 9;
            }
        }
        _ => {}
    }
}

/// The blade's combat self-test (CVR_BLADE_COMBAT_TEST=1): a knife given and drawn, a Tyger Claw spawned before the
/// player, the blade thrust into their chest through the sweep the controller's motion makes (VRBladeSweep); the
/// attack's way through the damage pipeline (the impact claimed, the hit published, the receipt) logged frame by
/// frame, the person deleted and the knife put away.
struct Combat {
    frames: u32,
    stage: u8,
    spawned: u64,
    waited: u32,
    watched: u32,
}
static COMBAT: Mutex<Combat> = Mutex::new(Combat { frames: 0, stage: 0, spawned: 0, waited: 0, watched: 0 });
static COMBAT_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
pub(in crate::mechanics) fn combat_tick() {
    if !*COMBAT_ON.get_or_init(|| std::env::var("CVR_BLADE_COMBAT_TEST").is_ok_and(|v| v == "1")) {
        return;
    }
    use super::lobby_rtti::{self as rt, A};
    let pl = super::menu_rt::player();
    // gameplay only: not the lobby, not before the player is in the world
    if pl.is_null() || super::belt::native::<bool>("VRLobbyActive", &[]) == Some(true) || !super::menu_rt::b(&pl, "IsAttached", &[]) {
        return;
    }
    let mut s = COMBAT.lock().unwrap_or_else(|p| p.into_inner());
    s.frames += 1;
    let held = super::blade::physical_weapon(&pl);
    match s.stage {
        0 if s.frames >= 500 => {
            let ts = super::belt::transactions();
            let id = rt::static_call("ItemID", "FromTDBID", &[A::Name(red4ext_abi::tweakdbid::from_name("Items.Preset_Knife_Default"))]);
            let Some(id) = id else { return };
            let given = super::menu_rt::b(&ts, "GiveItem", &[A::H(&pl), A::P(id.as_ptr().cast()), A::N(1.0)]);
            let req = super::lobby_ink::new("GameplayEquipRequest");
            super::menu_main::set_handle_prop(&req, "owner", &pl);
            if let Some(f) = super::lobby_rtti::field(&req, "itemID") {
                // SAFETY: an ItemID field, assigned from the call's ItemID of the same type.
                unsafe { f.ty.assign(f.ptr, id.as_ptr().cast()) };
            }
            super::menu_rt::hset(&req, "slotIndex", 0.0);
            super::menu_rt::hset(&req, "forceEquipWeapon", 1.0);
            let c = super::game_system("GetScriptableSystemsContainer");
            let equipment = rt::obj(&c, "Get", &[A::Name(CName::hashed("EquipmentSystem").0)]);
            rt::call(&equipment, "QueueRequest", &[A::H(&req)]);
            log!("[blade-combat] a knife given ({given}) and its equip and draw asked\n");
            s.stage = 1;
        }
        1 if s.frames >= 800 && (super::blade::kind(&held) > 0 || s.frames >= 1400) => {
            let kind = super::blade::kind(&held);
            let (class, reach) = if held.is_null() { (String::from("nothing"), 0.0) } else { (super::belt::class_name(&held), super::melee::reach(&held)) };
            log!("[blade-combat] in hand: {class} (blade kind {kind}, allowed {}, reach {reach:.2})\n", super::blade::allowed(&pl, &held));
            let at = super::hand_collision::world_pos(&pl).unwrap_or([0.0; 3]);
            let fwd = super::lobby_rtti::call(&pl, "GetWorldForward", &[])
                .map(|o| {
                    let v = super::lobby_rtti::Val::of(&o);
                    [super::menu_rt::fnum(&v, "X") as f32, super::menu_rt::fnum(&v, "Y") as f32]
                })
                .unwrap_or([0.0, 1.0]);
            // how far before the player (CVR_BLADE_COMBAT_DISTANCE, metres; 1.3 by default)
            let d = std::env::var("CVR_BLADE_COMBAT_DISTANCE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.3..=3.0).contains(v)).unwrap_or(1.3);
            let pos = [at[0] + fwd[0] * d, at[1] + fwd[1] * d, at[2], 1.0];
            let yaw = (-fwd[0]).atan2(-fwd[1]);
            let q = [0.0, 0.0, (-yaw * 0.5).sin(), (-yaw * 0.5).cos()];
            s.spawned = crate::spawn_system::spawn_record(red4ext_abi::tweakdbid::from_name("Character.tyger_claws_gangster1_melee1_fists_ma"), &pos, &q);
            log!("[blade-combat] a Tyger Claw spawned: id {}\n", s.spawned);
            s.stage = 2;
        }
        2 => {
            s.waited += 1;
            let e = crate::spawn_system::spawned(s.spawned);
            let ready = !e.is_null() && !super::blade::hit_representation(&e).is_null();
            if !(ready && s.waited > 600) && s.waited < 1400 {
                return;
            }
            if !ready || held.is_null() {
                log!("[blade-combat] no person ({ready}) or no weapon in hand: stopped\n");
                s.stage = 9;
                return;
            }
            let npc_at = super::hand_collision::world_pos(&e).unwrap_or([0.0; 3]);
            let me = super::hand_collision::world_pos(&pl).unwrap_or([0.0; 3]);
            let mut dir = [npc_at[0] - me[0], npc_at[1] - me[1], 0.0, 0.0];
            let l = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt().max(1e-3);
            dir = [dir[0] / l, dir[1] / l, 0.0, 0.0];
            let chest = [npc_at[0], npc_at[1], npc_at[2] + 1.25, 1.0];
            let at = |k: f32| [chest[0] - dir[0] * k, chest[1] - dir[1] * k, chest[2], 1.0];
            let (old_a, old_b, a, b) = (at(0.42), at(0.22), at(0.30), at(0.04));
            let friendly = super::blade::sb("GameObject", "IsFriendlyTowardsPlayer", &[A::H(&e)]);
            let hit = super::blade::sweep(&pl, &held, &old_a, &old_b, &a, &b, false, true, [dir[0] * 0.2, dir[1] * 0.2, 0.0, 0.0], false, 4.0);
            log!("[blade-combat] the thrust swept into the chest: {hit} (friendly {friendly}, allowed {}, person at {:?})\n", super::blade::allowed(&pl, &held), npc_at);
            log!("[blade-combat] the sweep's stage: {:?}\n", super::blade::last_sweep(&held));
            let sq = super::blade::queries();
            let above = [chest[0], chest[1], chest[2] + 2.0, 1.0];
            let side = [chest[0] - dir[1] * 1.0, chest[1] + dir[0] * 1.0, chest[2], 1.0];
            for (preset, from) in [("Melee", at(0.8)), ("Melee", at(0.3)), ("Melee", above), ("Melee", side), ("Bullet", at(0.45)), ("Melee", [me[0] - dir[1] * 3.0, me[1] + dir[0] * 3.0, chest[2] + 1.0, 1.0])] {
                let mut t = super::blade::Trace::zeroed();
                let hit = super::blade::raycast(&sq, &from, &chest, preset, false, &mut t);
                let e2 = if hit { t.entity() } else { red4ext_abi::handle::Handle::null() };
                let class = if e2.is_null() { String::new() } else { super::belt::class_name(&e2) };
                let material = CName(u64::from_le_bytes(t.0[0x18..0x20].try_into().unwrap_or([0; 8]))).text().to_string();
                log!(
                    "[blade-combat] a ray to the chest by {preset} from {:.2} m away: {hit}, {:.2} m short of it, material {material}, on {class} (the person: {})\n",
                    super::blade::dist(&from, &chest),
                    super::blade::dist(&t.position(), &chest),
                    !e2.is_null() && e2.instance == e.instance
                );
            }
            let comp = super::blade::hit_representation(&e);
            let p3 = |v: &[f32; 4]| [v[0], v[1], v[2]];
            let swept = crate::blade_natives::swept_contact(comp.instance.cast(), &[p3(&old_a), p3(&old_b), p3(&a), p3(&b)], Some(&p3(&a)));
            log!("[blade-combat] the hit shapes' sweep: {:?}\n", swept);
            s.stage = 3;
        }
        3 => {
            s.watched += 1;
            let serial = super::blade::confirmed_serial(&held);
            if s.watched % 20 == 1 || serial != 0 {
                let r = super::blade::receipt(&held);
                let info = r.map(|r| {
                    let r = r.borrow();
                    format!("serial {} puncture {} lodged {} slot {} shape {} health {:.1} mesh requests {}", r.serial, r.puncture, r.lodged, CName(r.slot).text(), CName(r.shape_name).text(), r.health_loss, r.mesh_requests)
                });
                log!("[blade-combat] frame {}: receipt {:?}\n", s.watched, info);
            }
            if serial != 0 || s.watched > 240 {
                let id = s.spawned;
                log!("[blade-combat] the person deleted: {}\n", crate::spawn_system::delete_spawned(id));
                super::holster::equip_request(&pl, "UnequipWeapon");
                log!("[blade-combat] done\n");
                s.stage = 9;
            }
        }
        _ => {}
    }
}
