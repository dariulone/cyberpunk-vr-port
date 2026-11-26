//! CyberpunkVRPort_Menu/modules/gameplay_data.lua, reference_data.lua and library_data.lua: the game's own data for the
//! menu's character, journal, codex, shards, tarot, summary and clock, read through the same records and journal
//! methods the game's hub uses -- and its requests (an attribute or a perk bought, an objective tracked, the time
//! skipped, a shard cracked). Nothing here grants points, reveals inactive journal entries or bypasses a restriction.
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_data::{self as md, Icon};
use super::menu_jobs::{Factory, Jobs, Step};
use super::menu_rt::*;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::rc::Rc;

pub(super) const ATTRIBUTE_ATLAS: &str = "base\\gameplay\\gui\\common\\icons\\attributes_icons.inkatlas";
pub(super) fn icon_path(atlas: &str, part: &str) -> Icon {
    Icon { atlas: red4ext_abi::resource::resource_path(atlas), part: cname(part), name: part.to_string() }
}
/// D.image: an icon record's atlas and part (None for an invalid id).
pub(super) fn image(id: u64) -> Option<Icon> {
    if id == 0 || !sb("TDBID", "IsValid", &[A::Name(id)]) {
        return None;
    }
    md::ui_icon(id)
}
fn gi() -> crate::script_call::GameInstance {
    rt::game()
}
fn gia(g: &crate::script_call::GameInstance) -> A<'_> {
    A::P((g as *const crate::script_call::GameInstance).cast())
}
/// Lua's string.format("%g", v).
pub(super) fn fmt_g(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    if !v.is_finite() {
        return format!("{v}");
    }
    let exp = v.abs().log10().floor() as i32;
    let trim = |s: String| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s
        }
    };
    if !(-4..6).contains(&exp) {
        let s = format!("{v:.5e}");
        let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
        let e: i32 = e.parse().unwrap_or(0);
        format!("{}e{}{:02}", trim(m.to_string()), if e < 0 { '-' } else { '+' }, e.abs())
    } else {
        trim(format!("{:.*}", (5 - exp).max(0) as usize, v))
    }
}
/// `{key...}` placeholders filled from `values` (else `missing(key)`).
pub(super) fn fill(text: &str, values: &HashMap<String, String>, keep_missing: bool) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let key_len = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        let close = after.find('}');
        match close {
            Some(c) if key_len > 0 && key_len <= c => {
                let key = &after[..key_len];
                match values.get(key) {
                    Some(v) => out.push_str(v),
                    None if keep_missing => out.push_str(&format!("{{{key}}}")),
                    None => {}
                }
                rest = &after[c + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}
/// The values of a UILocalizationDataPackage, by placeholder name.
fn package_values(pkg: &Handle) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for (prefix, field) in [("float", "floatValues"), ("int", "intValues"), ("stat", "statValues")] {
        if let Some(arr) = pf(pkg, field) {
            for (i, v) in items(&arr).iter().enumerate() {
                values.insert(format!("{prefix}_{i}"), fmt_g(v.num().unwrap_or(0.0)));
            }
        }
    }
    if let Some(arr) = pf(pkg, "nameValues") {
        for (i, v) in items(&arr).iter().enumerate() {
            values.insert(format!("name_{i}"), loc(&v.text()));
        }
    }
    if let Some(arr) = pf(pkg, "statNames") {
        for (i, v) in items(&arr).iter().enumerate() {
            values.entry(format!("stat_{i}")).or_insert_with(|| loc(&v.text()));
        }
    }
    values
}
/// R.params: a text with its package's values filled in (missing ones dropped).
pub(super) fn params(text: &str, pkg: &Handle) -> String {
    if pkg.is_null() {
        return loc(text);
    }
    fill(&loc(text), &package_values(pkg), false)
}

// ---------------------------------------------------------------- the character
struct Dev {
    player: Handle,
    manager: Handle,
    data: Handle,
}
fn development() -> Option<Dev> {
    let player = player();
    if player.is_null() {
        return None;
    }
    let manager = super::lobby_ink::new("PlayerDevelopmentDataManager");
    rt::call(&manager, "Initialize", &[A::H(&player), A::H(&Handle::null())]);
    let data = so("PlayerDevelopmentSystem", "GetData", &[A::H(&player)]);
    Some(Dev { player, manager, data })
}
const SKILLS: [&str; 5] = ["CoolSkill", "IntelligenceSkill", "ReflexesSkill", "StrengthSkill", "TechnicalAbilitySkill"];
pub(super) fn stat(name: &str) -> f64 {
    ev("gamedataStatType", name)
}
fn prof(name: &str) -> f64 {
    ev("gamedataProficiencyType", name)
}
#[derive(Clone, Debug)]
pub(super) struct Attr {
    pub ty: f64,
    pub name: String,
    pub value: f64,
    pub max: f64,
    pub description: String,
    pub can_buy: bool,
    pub relic: bool,
    pub icon: Option<Icon>,
}
impl Attr {
    /// The stat type's member name (Strength, Reflexes, ...).
    pub(super) fn type_name(&self) -> String {
        enum_name("gamedataStatType", self.ty)
    }
}
#[derive(Clone, Debug)]
pub(super) struct Reward {
    pub level: f64,
    pub text: String,
    pub unlocked: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Skill {
    pub name: String,
    pub level: f64,
    pub xp: f64,
    pub remaining: f64,
    pub max: f64,
    pub rewards: Vec<Reward>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Character {
    pub attributes: Vec<Attr>,
    pub skills: Vec<Skill>,
    pub points: f64,
    pub perks: f64,
    pub relic: f64,
    pub level: f64,
}
/// R.character: the attributes with their icons, the skills with their rewards.
pub(super) fn character() -> Option<Character> {
    let d = development()?;
    let mut out = Character {
        points: n(&d.manager, "GetAttributePoints", &[]),
        perks: n(&d.manager, "GetPerkPoints", &[]),
        relic: n(&d.manager, "GetSpyPerkPoints", &[]),
        level: n(&d.data, "GetProficiencyLevel", &[A::N(prof("Level"))]),
        ..Default::default()
    };
    let espionage = stat("Espionage");
    let wanted = [stat("Strength"), stat("Reflexes"), stat("TechnicalAbility"), stat("Intelligence"), stat("Cool")];
    let attrs = rt::call(&d.manager, "GetAttributes", &[]);
    for a in attrs.as_ref().map(|o| items(&Val::of(o))).unwrap_or_default() {
        let ty = fnum(&a, "type");
        let value = fnum(&a, "value");
        if wanted.contains(&ty) || (ty == espionage && value > 0.0) {
            out.attributes.push(Attr {
                ty,
                name: loc(&ftext(&a, "label")),
                value,
                max: fnum(&a, "maxValue"),
                description: loc(&ftext(&a, "description")),
                can_buy: ty != espionage && b(&d.data, "CanAttributeBeBought", &[A::N(ty)]),
                relic: false,
                icon: None,
            });
        }
    }
    if !out.attributes.iter().any(|a| a.ty == espionage) {
        let qs = super::game_system("GetQuestsSystem");
        let fact = Arg::new("ep1_tree_unlocked");
        if n(&qs, "GetFactStr", &[fact.a()]) > 0.0 {
            out.attributes.push(Attr {
                ty: espionage,
                name: loc("LocKey#20997"),
                value: n(&d.data, "GetAttributePoints", &[A::N(ev("gamedataAttributeDataType", "EspionageAttributeData"))]),
                max: 15.0,
                description: String::new(),
                can_buy: false,
                relic: true,
                icon: None,
            });
        }
    }
    for a in out.attributes.iter_mut() {
        a.icon = Some(if a.relic {
            icon_path("base\\gameplay\\gui\\fullscreen\\perks\\atlas_perks.inkatlas", "ico_atrib_relic")
        } else {
            let menu_attr = sn("PerkAttributeHelper", "BaseStatToPerkMenuAttribute", &[A::N(a.ty)]);
            let part = rt::static_call("PerkAttributeHelper", "GetIconAtlasPart", &[A::N(menu_attr)]).map(|o| Val::of(&o).text()).unwrap_or_default();
            icon_path(ATTRIBUTE_ATLAS, &part)
        });
    }
    for name in SKILLS {
        let kind = prof(name);
        let info = o(&d.manager, "GetProficiencyDisplayData", &[A::N(kind)]);
        let level = n(&d.data, "GetProficiencyLevel", &[A::N(kind)]);
        let mut rewards = Vec::new();
        if let Some(arr) = pf(&info, "passiveBonusesData") {
            for r in items(&arr) {
                let rh = r.handle();
                let rl = pnum(&rh, "level");
                if rl % 5.0 == 0.0 {
                    rewards.push(Reward { level: rl, text: params(&ptext(&rh, "description"), &pf(&rh, "locPackage").map(|v| v.handle()).unwrap_or_else(Handle::null)), unlocked: level >= rl });
                }
            }
        }
        out.skills.push(Skill {
            name: ptext(&info, "localizedName"),
            level,
            xp: n(&d.data, "GetCurrentLevelProficiencyExp", &[A::N(kind)]),
            remaining: n(&d.data, "GetRemainingExpForLevelUp", &[A::N(kind)]),
            max: pnum(&info, "maxLevel"),
            rewards,
        });
    }
    Some(out)
}
#[derive(Clone)]
pub(super) struct Perk {
    pub ty: f64,
    pub key: String,
    pub name: String,
    pub description: String,
    pub record: Handle,
    pub level: f64,
    pub max: f64,
    pub locked: bool,
    pub required: f64,
    pub cost: f64,
    pub can_buy: bool,
    pub can_sell: bool,
    pub parents: Vec<String>,
    pub slot: String,
    pub icon: Option<Icon>,
    pub video: u64,
}
/// D.perks + R.perks: an attribute's perks (its free roots unlocked first, as the game's screen does), by tier and name.
pub(super) fn perks(attr_ty: f64) -> Vec<Perk> {
    let Some(d) = development() else { return Vec::new() };
    let rec_id = rt::static_call("PerkAttributeHelper", "BaseStatToAttribute", &[A::N(attr_ty)]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
    let record = so("TweakDBInterface", "GetAttributeDataRecord", &[A::Name(rec_id)]);
    let adt = sn("PlayerDevelopmentData", "StatTypeToAttributeDataType", &[A::N(attr_ty)]);
    rt::call(&d.data, "UnlockFreeNewPerks", &[A::N(adt)]);
    let bb = o(&d.player, "GetPlayerStateMachineBlackboard", &[]);
    let in_combat = (|| -> Result<i32, String> {
        let defs = super::boards::defs()?;
        let dd = super::boards::definition(&defs, "PlayerStateMachine")?;
        let f = super::boards::field(dd, "Combat")?;
        super::boards::get::<i32>(&bb, "GetInt", f)
    })()
    .unwrap_or(0)
        == 1;
    let mut out = Vec::new();
    for perk in out_list(&record, "Perks", &[]) {
        let kind = n(&perk, "Type", &[]);
        let info = super::lobby_ink::new("NewPerkDisplayData");
        let level = n(&d.data, "IsNewPerkBought", &[A::N(kind)]).max(0.0);
        let max = n(&d.data, "GetNewPerkMaxLevel", &[A::N(kind)]).max(n(&perk, "GetLevelsCount", &[]));
        let locked = !b(&d.data, "IsNewPerkUnlocked", &[A::N(kind)]);
        hset(&info, "type", kind);
        hset(&info, "level", level);
        hset(&info, "maxLevel", max);
        hset(&info, "locked", if locked { 1.0 } else { 0.0 });
        let can_sell = b(&d.manager, "IsNewPerkRefundable", &[A::H(&info)])
            && rt::static_call("PlayerDevelopmentSystem", "CanSellNewPerk", &[A::H(&d.player), A::N(kind)]).is_some_and(|o| fbool(&Val::of(&o), "success"))
            && !in_combat;
        let parents: Vec<String> = out_list(&perk, "RequiresPerks", &[]).iter().map(|p| fmt_g(n(p, "Type", &[]))).collect();
        out.push(Perk {
            ty: kind,
            key: fmt_g(kind),
            name: loc(&s(&perk, "Loc_name_key", &[])),
            description: loc(&s(&perk, "Loc_desc_key", &[])),
            level,
            max,
            locked,
            required: n(&o(&perk, "Tier", &[]), "RequiredAttributePoints", &[]),
            cost: if b(&d.data, "IsEspionageMilestonePerk", &[A::N(kind)]) { 3.0 } else { 1.0 },
            can_buy: b(&d.manager, "IsPerkUpgradeable", &[A::H(&info), A::B(false)]),
            can_sell,
            parents,
            slot: s(&o(&perk, "Slot", &[]), "EnumName", &[]),
            icon: image(id(&o(&perk, "PerkIcon", &[]), "GetID", &[])),
            video: id(&perk, "BinkPath", &[]),
            record: perk,
        });
    }
    out.sort_by(|a, b| a.required.partial_cmp(&b.required).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.name.cmp(&b.name)));
    out
}
pub(super) struct LevelText {
    pub level: usize,
    pub text: String,
}
/// D.perkDescription: each level's text with its values filled in.
pub(super) fn perk_description(p: &Perk) -> Vec<LevelText> {
    let mut out = Vec::new();
    for (index, level) in out_list(&p.record, "Levels", &[]).iter().enumerate() {
        let mut text = loc(&s(level, "Loc_desc_key", &[]));
        if text.is_empty() {
            text = p.description.clone();
        }
        let mut ui = o(level, "UiData", &[]);
        if ui.is_null() {
            ui = o(&p.record, "UiData", &[]);
        }
        if !ui.is_null() {
            let pkg = so("UILocalizationDataPackage", "FromNewPerkUIDataPackage", &[A::H(&ui)]);
            text = fill(&text, &package_values(&pkg), true);
        }
        let package = o(level, "DataPackage", &[]);
        if !package.is_null() {
            for modifier in out_list(&package, "Stats", &[]) {
                if is_a(&modifier, "gamedataConstantStatModifier_Record") {
                    let key = format!("<BaseStats.{}>", s(&o(&modifier, "StatType", &[]), "EnumName", &[]));
                    text = text.replace(&key, &((n(&modifier, "Value", &[]) + 0.5).floor() as i64).to_string());
                }
            }
        }
        out.push(LevelText { level: index + 1, text });
    }
    out
}
pub(super) fn buy_attribute(kind: f64) -> bool {
    let Some(d) = development() else { return false };
    if kind == stat("Espionage") || !b(&d.data, "CanAttributeBeBought", &[A::N(kind)]) {
        return false;
    }
    let r = super::lobby_ink::new("BuyAttribute");
    rt::call(&r, "Set", &[A::H(&d.player), A::N(kind), A::B(false)]);
    let pds = so("PlayerDevelopmentSystem", "GetInstance", &[A::H(&d.player)]);
    rt::call(&pds, "QueueRequest", &[A::H(&r)]).is_some()
}
pub(super) fn change_perk(attr_ty: f64, kind: f64, refund: bool) -> bool {
    let pl = player();
    let Some(cur) = perks(attr_ty).into_iter().find(|p| p.ty == kind) else { return false };
    if !(if refund { cur.can_sell } else { cur.can_buy }) {
        return false;
    }
    let r = super::lobby_ink::new(if refund { "SellNewPerk" } else { "BuyNewPerk" });
    rt::call(&r, "Set", &[A::H(&pl), A::N(kind)]);
    let pds = so("PlayerDevelopmentSystem", "GetInstance", &[A::H(&pl)]);
    rt::call(&pds, "QueueRequest", &[A::H(&r)]);
    let ui = so("UIInventoryScriptableSystem", "GetInstance", &[]);
    rt::call(&ui, "QueueRequest", &[A::H(&r)]);
    true
}
/// D.signature("character"): the points, the attributes' values and the skills' experience.
pub(super) fn character_signature() -> String {
    let Some(d) = development() else { return String::new() };
    let mut v = vec![fmt_g(n(&d.manager, "GetAttributePoints", &[])), fmt_g(n(&d.manager, "GetPerkPoints", &[])), fmt_g(n(&d.manager, "GetSpyPerkPoints", &[]))];
    let attrs = rt::call(&d.manager, "GetAttributes", &[]);
    for a in attrs.as_ref().map(|o| items(&Val::of(o))).unwrap_or_default() {
        v.push(fmt_g(fnum(&a, "value")));
    }
    for k in SKILLS {
        v.push(fmt_g(n(&d.data, "GetTotalProfExperience", &[A::N(prof(k))])));
    }
    v.join("|")
}

// ---------------------------------------------------------------- the journal
fn wrapper(g: &crate::script_call::GameInstance) -> Handle {
    let w = super::lobby_ink::new("JournalWrapper");
    rt::call(&w, "Init", &[gia(g)]);
    w
}
fn hn(h: f64) -> A<'static> {
    A::N(f64::from(h as i64 as i32 as u32))
}
fn active_filter(inactive: bool) -> Option<crate::script_call::Owned> {
    let f = new_struct("JournalRequestStateFilter")?;
    let v = val(&f);
    set(&v, "active", 1.0);
    if inactive {
        set(&v, "inactive", 1.0);
    }
    Some(f)
}
fn active_context() -> Option<(crate::script_call::Owned, crate::script_call::Owned)> {
    let f = active_filter(false)?;
    let c = new_struct("JournalRequestContext")?;
    if let Some(sf) = val(&c).f("stateFilter") {
        // SAFETY: the context's filter, the same struct type.
        unsafe { sf.ty.assign(sf.ptr, f.as_ptr().cast()) };
    }
    Some((c, f))
}
/// obj:name(args..., out list): the list of handles the out parameter returns (CET's extra return).
pub(super) fn out_list(obj: &Handle, name: &str, args: &[A]) -> Vec<Handle> {
    match rt::call_outs(obj, name, args, 1) {
        Some((_, outs)) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
        None => Vec::new(),
    }
}
fn children(jm: &Handle, e: &Handle, filter: &crate::script_call::Owned) -> Vec<Handle> {
    out_list(jm, "GetChildren", &[A::H(e), A::P(ptr(filter))])
}
fn game_seconds(t: &crate::script_call::Owned) -> f64 {
    sn("GameTime", "GetSeconds", &[A::P(ptr(t))])
}
fn entry_seconds(jm: &Handle, e: &Handle) -> f64 {
    rt::call(jm, "GetEntryTimestamp", &[A::H(e)]).map_or(0.0, |t| game_seconds(&t))
}
fn distance_to(jm: &Handle, objective: &Handle) -> f64 {
    match rt::static_call("QuestLogUtils", "GetDefaultFilter", &[]) {
        Some(f) => n(jm, "GetDistanceToNearestMappin", &[A::H(objective), A::P(ptr(&f))]),
        None => 0.0,
    }
}
#[derive(Clone, Debug)]
pub(super) struct QuestRow {
    pub hash: f64,
    pub name: String,
    pub state: f64,
    pub kind: String,
    pub story: bool,
    pub at: f64,
    pub objective: String,
    pub tracked: bool,
    pub distance: Option<f64>,
}
#[derive(Clone, Debug, Default)]
pub(super) struct QuestList {
    pub quests: Vec<QuestRow>,
    pub structure: String,
}
/// R.distanceKey: how a distance reads in a row (the list's stamp keeps this, not the metres).
pub(super) fn distance_key(d: Option<f64>) -> String {
    match d {
        None => String::new(),
        Some(d) if d >= 1000.0 => format!("{:.1}k", d / 1000.0),
        Some(d) => ((d + 0.5).floor() as i64).to_string(),
    }
}
/// R.questList as a producer: one active quest (its objective tree) or up to eight finished ones a step.
pub(super) fn quest_list() -> Factory {
    Rc::new(|| {
        struct St {
            jm: Handle,
            w: Handle,
            quests: Vec<Handle>,
            out: Vec<QuestRow>,
            i: usize,
            started: bool,
        }
        let mut st = St { jm: Handle::null(), w: Handle::null(), quests: Vec::new(), out: Vec::new(), i: 0, started: false };
        Box::new(move || {
            if !st.started {
                st.started = true;
                let g = gi();
                st.jm = jm();
                st.w = wrapper(&g);
                st.quests = out_list(&st.w, "GetQuests", &[]);
                return Step::More;
            }
            let active = ev("gameJournalEntryState", "Active");
            let main = ev("gameJournalQuestType", "MainQuest");
            let mut cost = 0;
            while st.i < st.quests.len() && cost < 8 {
                let e = st.quests[st.i].clone();
                st.i += 1;
                let jm = &st.jm;
                let state = n(jm, "GetEntryState", &[A::H(&e)]);
                let mut q = QuestRow {
                    hash: n(jm, "GetEntryHash", &[A::H(&e)]),
                    name: loc(&s(&e, "GetTitle", &[A::H(jm)])),
                    state,
                    kind: enum_name("gameJournalQuestType", n(&e, "GetType", &[])),
                    story: n(jm, "GetQuestType", &[A::H(&e)]) == main,
                    at: entry_seconds(jm, &e),
                    objective: String::new(),
                    tracked: false,
                    distance: None,
                };
                if state == active {
                    let data = o(&st.w, "BuildQuestData", &[A::H(&e)]);
                    q.tracked = b(&data, "IsTrackedInHierarchy", &[]);
                    for ob in os(&data, "GetObjectives", &[]) {
                        if b(&ob, "IsActive", &[]) {
                            if q.objective.is_empty() || b(&ob, "IsTrackedInHierarchy", &[]) {
                                q.objective = loc(&s(&ob, "GetDescription", &[]));
                            }
                            let d = distance_to(jm, &o(&ob, "GetQuestObjective", &[]));
                            if d > 0.0 && q.distance.is_none_or(|x| d < x) {
                                q.distance = Some(d);
                            }
                        }
                    }
                    cost += 8;
                } else {
                    cost += 1;
                }
                st.out.push(q);
            }
            if st.i < st.quests.len() {
                return Step::More;
            }
            let mut out = std::mem::take(&mut st.out);
            out.sort_by(|a, b| b.story.cmp(&a.story).then(b.at.partial_cmp(&a.at).unwrap_or(std::cmp::Ordering::Equal)).then_with(|| a.name.cmp(&b.name)));
            let structure: Vec<String> = out.iter().map(|q| format!("{}:{}:{}:{}:{}", fmt_g(q.hash), fmt_g(q.state), q.tracked, q.name, q.objective)).collect();
            let distances: Vec<String> = out.iter().map(|q| distance_key(q.distance)).collect();
            let structure = structure.join("|");
            let stamp = format!("{structure}#{}", distances.join("|"));
            Step::Done(Rc::new(QuestList { quests: out, structure }), Some(stamp))
        })
    })
}
#[derive(Clone, Debug)]
pub(super) struct Objective {
    pub hash: f64,
    pub name: String,
    pub active: bool,
    pub state: f64,
    pub tracked: bool,
    pub sub: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Quest {
    pub name: String,
    pub description: String,
    pub objectives: Vec<Objective>,
}
/// D.quest: a quest's title, description and objectives (sub-objectives after theirs).
pub(super) fn quest(hash: f64) -> Option<Quest> {
    let g = gi();
    let w = wrapper(&g);
    let jm = o(&w, "GetJournalManager", &[]);
    let jm = if jm.is_null() { super::menu_rt::jm() } else { jm };
    let entry = o(&jm, "GetEntry", &[hn(hash)]);
    if entry.is_null() {
        return None;
    }
    let q = o(&w, "BuildQuestData", &[A::H(&entry)]);
    let mut out = Quest { name: loc(&s(&q, "GetTitle", &[])), description: loc(&s(&q, "GetDescription", &[])), objectives: Vec::new() };
    let add = |out: &mut Quest, ob: &Handle, sub: bool| {
        out.objectives.push(Objective {
            hash: n(&jm, "GetEntryHash", &[A::H(&o(ob, "GetQuestObjective", &[]))]),
            name: loc(&s(ob, "GetDescription", &[])) + &s(ob, "GetCounterText", &[]),
            active: b(ob, "IsActive", &[]),
            state: n(ob, "GetStatus", &[]),
            tracked: b(ob, "IsTrackedInHierarchy", &[]),
            sub,
        });
    };
    for ob in os(&q, "GetObjectives", &[]) {
        add(&mut out, &ob, false);
        for sub in os(&ob, "GetSubObjectives", &[]) {
            add(&mut out, &sub, true);
        }
    }
    Some(out)
}
pub(super) fn track(hash: f64) -> bool {
    let jm = jm();
    let e = o(&jm, "GetEntry", &[hn(hash)]);
    if e.is_null() || n(&jm, "GetEntryState", &[A::H(&e)]) != ev("gameJournalEntryState", "Active") {
        return false;
    }
    rt::call(&jm, "TrackEntry", &[A::H(&e)]).is_some()
}
pub(super) fn visit(hash: f64) {
    let jm = jm();
    let e = o(&jm, "GetEntry", &[hn(hash)]);
    if !e.is_null() {
        rt::call(&jm, "SetEntryVisited", &[A::H(&e), A::B(true)]);
    }
}
#[derive(Clone, Debug)]
pub(super) struct QuestAction {
    pub kind: &'static str,
    pub hash: f64,
    pub name: String,
    pub thread: Option<md::Thread>,
    pub contact: i64,
}
/// R.actions: the shards and message threads an active objective links to (QuestDetailsPanelController's action links).
pub(super) fn actions(hash: f64) -> Vec<QuestAction> {
    let g = gi();
    let jm = jm();
    let w = wrapper(&g);
    let q = o(&w, "BuildQuestData", &[A::H(&o(&jm, "GetEntry", &[hn(hash)]))]);
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let Some(filter) = active_filter(true) else { return out };
    fn walk(jm: &Handle, entry: &Handle, depth: u32, filter: &crate::script_call::Owned, seen: &mut Vec<f64>, out: &mut Vec<QuestAction>) {
        if depth > 12 {
            return;
        }
        for child in children(jm, entry, filter) {
            if is_a(&child, "gameJournalQuestCodexLink") {
                let linked = o(jm, "GetEntry", &[hn(n(&child, "GetLinkPathHash", &[]))]);
                if linked.is_null() {
                    continue;
                }
                let id = n(jm, "GetEntryHash", &[A::H(&linked)]);
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if is_a(&linked, "gameJournalOnscreen") {
                    out.push(QuestAction { kind: "shard", hash: id, name: loc(&s(&linked, "GetTitle", &[])), thread: None, contact: 0 });
                } else if is_a(&linked, "gameJournalPhoneMessage") || is_a(&linked, "gameJournalPhoneConversation") || is_a(&linked, "gameJournalPhoneChoiceGroup") {
                    let contact = sn("MessengerUtils", "FindContactHash", &[A::H(jm), A::H(&linked)]) as i64;
                    let conversation = sn("MessengerUtils", "FindConversationHash", &[A::H(jm), A::H(&linked)]) as i64;
                    if let Some(t) = md::threads(true, Some(contact)).into_iter().find(|t| !t.multi || t.conversation_hash == conversation) {
                        out.push(QuestAction { kind: "message", hash: id, name: t.name.clone(), thread: Some(t), contact });
                    }
                }
            } else if is_a(&child, "gameJournalContainerEntry") {
                walk(jm, &child, depth + 1, filter, seen, out);
            }
        }
    }
    for ob in os(&q, "GetObjectives", &[]) {
        if b(&ob, "IsActive", &[]) {
            walk(&jm, &o(&ob, "GetQuestObjective", &[]), 0, &filter, &mut seen, &mut out);
        }
    }
    out
}
#[derive(Clone, Debug)]
pub(super) struct Link {
    pub hash: f64,
    pub name: String,
    pub image: Option<Icon>,
}
/// R.links: the codex entries the chosen objective links to (activated, as the game's details panel does).
pub(super) fn links(hash: f64) -> Vec<Link> {
    let g = gi();
    let w = wrapper(&g);
    let jm = jm();
    let entry = o(&jm, "GetEntry", &[hn(hash)]);
    if entry.is_null() {
        return Vec::new();
    }
    let objectives = os(&o(&w, "BuildQuestData", &[A::H(&entry)]), "GetObjectives", &[]);
    let mut chosen: Option<Handle> = None;
    for ob in &objectives {
        if b(ob, "IsActive", &[]) && (chosen.is_none() || b(ob, "IsTrackedInHierarchy", &[])) {
            chosen = Some(ob.clone());
        }
    }
    let Some(chosen) = chosen.or_else(|| objectives.last().cloned()) else { return Vec::new() };
    let Some(filter) = active_filter(true) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = Vec::new();
    fn walk(jm: &Handle, parent: &Handle, depth: u32, filter: &crate::script_call::Owned, seen: &mut Vec<f64>, out: &mut Vec<Link>) {
        if depth > 12 {
            return;
        }
        for child in children(jm, parent, filter) {
            if is_a(&child, "gameJournalQuestCodexLink") {
                let mut link = o(jm, "GetEntry", &[hn(n(&child, "GetLinkPathHash", &[]))]);
                if link.is_null() || !is_a(&link, "gameJournalCodexEntry") {
                    continue;
                }
                let rep = so("CodexUtils", "GetCodexReplacementRecord", &[A::H(jm), A::H(&link)]);
                if !rep.is_null() {
                    link = rep;
                }
                let id = n(jm, "GetEntryHash", &[A::H(&link)]);
                if seen.contains(&id) {
                    continue;
                }
                seen.push(id);
                if n(jm, "GetEntryState", &[A::H(&link)]) == ev("gameJournalEntryState", "Inactive") {
                    rt::call(jm, "ChangeEntryStateByHash", &[hn(id), A::N(ev("gameJournalEntryState", "Active")), A::N(ev("JournalNotifyOption", "DoNotNotify"))]);
                }
                let img = image(id_of(&link, "GetLinkImageID")).or_else(|| image(id_of(&link, "GetImageID")));
                out.push(Link { hash: id, name: loc(&s(&link, "GetTitle", &[])), image: img });
            } else if is_a(&child, "gameJournalContainerEntry") {
                walk(jm, &child, depth + 1, filter, seen, out);
            }
        }
    }
    walk(&jm, &o(&chosen, "GetQuestObjective", &[]), 0, &filter, &mut seen, &mut out);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
fn id_of(h: &Handle, name: &str) -> u64 {
    super::menu_rt::id(h, name, &[])
}
#[derive(Clone, Debug)]
pub(super) struct QuestDetail {
    pub quest: Quest,
    pub actions: Vec<QuestAction>,
    pub links: Vec<Link>,
}
/// R.questDetail as a producer in three steps: the quest, its action links, its codex links.
pub(super) fn quest_detail(hash: f64) -> Factory {
    Rc::new(move || {
        let mut phase = 0;
        let mut q: Option<Quest> = None;
        let mut acts = Vec::new();
        Box::new(move || {
            phase += 1;
            match phase {
                1 => {
                    q = quest(hash);
                    if q.is_none() {
                        return Step::Done(Rc::new(None::<QuestDetail>), Some("none".into()));
                    }
                    Step::More
                }
                2 => {
                    acts = actions(hash);
                    Step::More
                }
                _ => {
                    let l = links(hash);
                    let qq = q.take().unwrap_or(Quest { name: String::new(), description: String::new(), objectives: Vec::new() });
                    let mut stamp = vec![qq.name.clone(), qq.description.clone()];
                    for o in &qq.objectives {
                        stamp.push(format!("{}{}{}{}{}", fmt_g(o.hash), o.name, fmt_g(o.state), o.active, o.tracked));
                    }
                    for a in &acts {
                        stamp.push(format!("{}{}{}", a.kind, fmt_g(a.hash), a.name));
                    }
                    for x in &l {
                        stamp.push(fmt_g(x.hash));
                    }
                    Step::Done(Rc::new(Some(QuestDetail { quest: qq, actions: std::mem::take(&mut acts), links: l })), Some(stamp.join("|")))
                }
            }
        })
    })
}

// ---------------------------------------------------------------- the codex
#[derive(Clone, Debug)]
pub(super) struct CodexEntry {
    pub hash: f64,
    pub name: String,
    pub description: String,
    pub category: f64,
    pub image: u64,
    pub fresh: bool,
}
pub(super) fn codex() -> Vec<CodexEntry> {
    let jm = jm();
    let mut out = Vec::new();
    if let Some(arr) = rt::static_call("CodexUtils", "GetCodexDataArray", &[A::H(&jm), A::H(&Handle::null()), A::B(true)]) {
        for item in items(&Val::of(&arr)) {
            let h = item.handle();
            if pf(&h, "isHeader").is_some_and(|v| v.bool()) {
                continue;
            }
            let Some(d) = pf(&h, "data").map(|v| v.handle()) else { continue };
            out.push(CodexEntry {
                hash: pnum(&d, "hash"),
                name: loc(&ptext(&d, "title")),
                description: loc(&ptext(&d, "description")),
                category: pnum(&d, "category"),
                image: pf(&d, "imageId").map_or(0, |v| unsafe { *(v.ptr as *const u64) }),
                fresh: pf(&d, "isNew").is_some_and(|v| v.bool()),
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
/// D.codexList: the codex as a producer (the native array is one call, its own step); stamp: entries, names, new flags.
pub(super) fn codex_list() -> Factory {
    Rc::new(|| {
        let mut started = false;
        Box::new(move || {
            if !started {
                started = true;
                return Step::More;
            }
            let out = codex();
            let stamp: Vec<String> = out.iter().map(|e| format!("{}{}{}", fmt_g(e.hash), if e.fresh { "*" } else { "" }, e.name)).collect();
            Step::Done(Rc::new(out), Some(stamp.join("|")))
        })
    })
}
/// D.codexWatch: the codex's active entries walked a group per step, a cheap watch for codex_list to rerun.
pub(super) fn codex_watch() -> Factory {
    Rc::new(|| {
        let mut st: Option<(Handle, crate::script_call::Owned, Vec<Handle>, Vec<String>, usize)> = None;
        Box::new(move || {
            let Some((jm, filter, groups, values, i)) = st.as_mut() else {
                let jm = jm();
                let Some((ctx, filter)) = active_context() else { return Step::Done(Rc::new(true), Some(String::new())) };
                let mut groups = Vec::new();
                for category in out_list(&jm, "GetCodexCategories", &[A::P(ptr(&ctx))]) {
                    groups.extend(children(&jm, &category, &filter));
                }
                st = Some((jm, filter, groups, Vec::new(), 0));
                return Step::More;
            };
            if let Some(group) = groups.get(*i).cloned() {
                *i += 1;
                for e in children(jm, &group, filter) {
                    values.push(fmt_g(n(jm, "GetEntryHash", &[A::H(&e)])));
                }
                return Step::More;
            }
            values.sort();
            Step::Done(Rc::new(true), Some(values.join("|")))
        })
    })
}

// ---------------------------------------------------------------- shards (library_data.lua)
#[derive(Clone, Debug)]
pub(super) struct Shard {
    pub hash: f64,
    pub name: String,
    pub text: String,
    pub fresh: bool,
    pub crypted: bool,
    pub item: Option<Id16>,
}
#[derive(Clone, Debug)]
pub(super) struct ShardGroup {
    pub key: String,
    pub name: String,
    pub entries: Vec<Shard>,
}
/// L.shardList: the journal's shard list, then the backpack's encrypted chips, sixty items a step.
pub(super) fn shard_list() -> Factory {
    Rc::new(|| {
        let mut groups: Option<Vec<ShardGroup>> = None;
        let mut list: Option<Vec<Handle>> = None;
        let mut encrypted = ShardGroup { key: "encrypted".into(), name: String::new(), entries: Vec::new() };
        let mut i = 0usize;
        Box::new(move || {
            let jm = jm();
            let Some(gs) = groups.as_mut() else {
                let mut gs: Vec<ShardGroup> = Vec::new();
                let mut by_level: HashMap<i64, usize> = HashMap::new();
                if let Some(arr) = rt::static_call("CodexUtils", "GetShardsDataArray", &[A::H(&jm), A::H(&Handle::null())]) {
                    let all = items(&Val::of(&arr));
                    for it in &all {
                        let h = it.handle();
                        if pf(&h, "isHeader").is_some_and(|v| v.bool()) {
                            let lvl = pnum(&h, "level") as i64;
                            let d = pf(&h, "data").map(|v| v.handle()).unwrap_or_else(Handle::null);
                            by_level.insert(lvl, gs.len());
                            gs.push(ShardGroup { key: lvl.to_string(), name: loc(&ptext(&d, "title")), entries: Vec::new() });
                        }
                    }
                    for it in &all {
                        let h = it.handle();
                        if pf(&h, "isHeader").is_some_and(|v| v.bool()) {
                            continue;
                        }
                        let d = pf(&h, "data").map(|v| v.handle()).unwrap_or_else(Handle::null);
                        if let Some(&gi) = by_level.get(&(pnum(&h, "level") as i64)) {
                            gs[gi].entries.push(Shard { hash: pnum(&d, "hash"), name: loc(&ptext(&d, "title")), text: loc(&ptext(&d, "description")), fresh: pf(&d, "isNew").is_some_and(|v| v.bool()), crypted: false, item: None });
                        }
                    }
                }
                groups = Some(gs);
                return Step::More;
            };
            let Some(items_l) = list.as_ref() else {
                let ts = super::game_system("GetTransactionSystem");
                let pl = player();
                let got = rt::call_outs(&ts, "GetItemList", &[A::H(&pl)], 1);
                list = Some(match got {
                    Some((ok, outs)) if ok.as_ref().is_some_and(|o| Val::of(o).bool()) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
                    _ => Vec::new(),
                });
                encrypted.name = loc("LocKey#42347");
                i = 0;
                return Step::More;
            };
            let last = items_l.len().min(i + 60);
            let misc = ev("gamedataItemType", "Gen_Misc");
            let hide = cname("HideInBackpackUI");
            while i < last {
                let item = &items_l[i];
                i += 1;
                if item.is_null() || n(item, "GetItemType", &[]) != misc || b(item, "HasTag", &[A::Name(hide)]) {
                    continue;
                }
                let iid = id16(item, "GetID", &[]);
                let action = so("ItemActionsHelper", "GetCrackAction", &[iid.a()]);
                if action.is_null() {
                    continue;
                }
                let effect = o(&action, "Effector", &[]);
                if effect.is_null() || !is_a(&effect, "gamedataTriggerHackingMinigameEffector_Record") {
                    continue;
                }
                let path = Arg::new(&s(&effect, "JournalEntry", &[]));
                let cls = Arg::new("gameJournalOnscreen");
                let entry = o(&jm, "GetEntryByString", &[path.a(), cls.a()]);
                if !entry.is_null() && n(&jm, "GetEntryState", &[A::H(&entry)]) == ev("gameJournalEntryState", "Inactive") {
                    let desc = s(&entry, "GetDescription", &[]);
                    let da = Arg::new(&desc);
                    encrypted.entries.push(Shard {
                        hash: n(&jm, "GetEntryHash", &[A::H(&entry)]),
                        name: loc(&s(&entry, "GetTitle", &[])),
                        text: ss("CodexUtils", "GetShardTextString", &[A::B(true), da.a()]),
                        fresh: !b(&jm, "IsEntryVisited", &[A::H(&entry)]),
                        crypted: true,
                        item: Some(iid),
                    });
                }
            }
            if i < items_l.len() {
                return Step::More;
            }
            let mut gs = std::mem::take(gs);
            if !encrypted.entries.is_empty() {
                gs.push(std::mem::replace(&mut encrypted, ShardGroup { key: "encrypted".into(), name: String::new(), entries: Vec::new() }));
            }
            let mut stamp = Vec::new();
            for g in &gs {
                stamp.push(g.key.clone());
                for e in &g.entries {
                    stamp.push(format!("{}{}{}", fmt_g(e.hash), if e.fresh { "*" } else { "" }, if e.crypted { "!" } else { "" }));
                }
            }
            Step::Done(Rc::new(gs), Some(stamp.join("|")))
        })
    })
}
/// L.codex: a codex entry, from the job's last list when it has it, else read now.
pub(super) fn codex_entry(jobs: &mut Jobs, hash: f64) -> Option<CodexEntry> {
    if let Some(list) = jobs.get::<Vec<CodexEntry>>("codex") {
        if let Some(e) = list.iter().find(|e| e.hash == hash) {
            return Some(e.clone());
        }
    }
    let (v, stamp) = Jobs::now(&codex_list());
    let list = v.downcast::<Vec<CodexEntry>>().ok()?;
    let found = list.iter().find(|e| e.hash == hash).cloned();
    jobs.put("codex", (*list).clone(), stamp);
    found
}
pub(super) fn shards(jobs: &mut Jobs) -> Rc<Vec<ShardGroup>> {
    if let Some(g) = jobs.get::<Vec<ShardGroup>>("shards") {
        return g;
    }
    let (v, stamp) = Jobs::now(&shard_list());
    let g = v.downcast::<Vec<ShardGroup>>().unwrap_or_default();
    jobs.put("shards", (*g).clone(), stamp);
    g
}
pub(super) fn find_shard(jobs: &mut Jobs, hash: f64) -> Option<Shard> {
    shards(jobs).iter().flat_map(|g| g.entries.iter()).find(|e| e.hash == hash).cloned()
}
/// L.readShard: the shard visited (ShardsMenuGameController.OnShardSelectedEvent) and shown as read at once.
pub(super) fn read_shard(jobs: &mut Jobs, hash: f64) -> Option<Shard> {
    let e = find_shard(jobs, hash)?;
    let jm = jm();
    let native = o(&jm, "GetEntry", &[hn(hash)]);
    if !native.is_null() && !b(&jm, "IsEntryVisited", &[A::H(&native)]) {
        rt::call(&jm, "SetEntryVisited", &[A::H(&native), A::B(true)]);
    }
    if let Some(mut g) = jobs.get::<Vec<ShardGroup>>("shards").map(|g| (*g).clone()) {
        for e in g.iter_mut().flat_map(|g| g.entries.iter_mut()) {
            if e.hash == hash {
                e.fresh = false;
            }
        }
        let stamp = None;
        jobs.put("shards", g, stamp);
    }
    Some(e)
}
/// L.crack: the encrypted chip's own inventory action (the breach).
pub(super) fn crack(jobs: &mut Jobs, hash: f64) -> bool {
    let Some(e) = find_shard(jobs, hash) else { return false };
    let Some(item) = e.item.filter(|_| e.crypted) else { return false };
    if so("ItemActionsHelper", "GetCrackAction", &[item.a()]).is_null() {
        return false;
    }
    if let Ok(defs) = super::boards::defs() {
        if let Ok(dd) = super::boards::definition(&defs, "HackingMinigame") {
            if let (Ok(bb), Ok(f)) = (super::boards::system_board(dd), super::boards::field(dd, "IsJournalTarget")) {
                rt::call(&bb, "SetBool", &[A::P(f.cast_const()), A::B(false)]);
            }
        }
    }
    rt::static_call("ItemActionsHelper", "PerformItemAction", &[A::H(&player()), item.a()]);
    jobs.drop_key("shards");
    true
}

// ---------------------------------------------------------------- the tarot, the summary, the clock
#[derive(Clone, Debug)]
pub(super) struct Card {
    pub index: i64,
    pub name: String,
    pub description: String,
    pub image: Option<Icon>,
    pub found: bool,
}
pub(super) fn tarot() -> (Vec<Card>, usize, usize) {
    let jm = jm();
    let mut found: HashMap<i64, Card> = HashMap::new();
    let mut count = 0;
    if let Some((ctx, filter)) = active_context() {
        for group in out_list(&jm, "GetTarots", &[A::P(ptr(&ctx))]) {
            for card in children(&jm, &group, &filter) {
                let index = n(&card, "GetIndex", &[]) as i64;
                count += 1;
                let part = s(&card, "GetImagePart", &[]);
                found.insert(index, Card { index, name: loc(&s(&card, "GetName", &[])), description: loc(&s(&card, "GetDescription", &[])), image: image(tdb(&format!("UIIcon.{part}_BIG"))), found: true });
            }
        }
    }
    let ep1 = rt::global_call("IsEP1", &[]).is_some_and(|o| Val::of(&o).bool());
    let total = if ep1 { 26 } else { 22 };
    let back = image(tdb("UIIcon.TarotCard_Reverse"));
    let cards = (0..total as i64).map(|i| found.remove(&i).unwrap_or(Card { index: i, name: String::new(), description: String::new(), image: back.clone(), found: false })).collect();
    (cards, count, total)
}
#[derive(Clone, Debug)]
pub(super) struct Progress {
    pub level: f64,
    pub max: f64,
    pub xp: f64,
    pub total: f64,
}
#[derive(Clone, Debug, Default)]
pub(super) struct Summary {
    pub health: f64,
    pub armor: f64,
    pub dps: f64,
    pub other: Vec<(String, String)>,
    pub progress: Vec<Progress>,
    pub life: String,
    pub seconds: f64,
    pub rewards: Vec<(f64, String)>,
}
pub(super) fn summary() -> Option<Summary> {
    let pl = player();
    if pl.is_null() {
        return None;
    }
    let stats = super::game_system("GetStatsSystem");
    let eid = id16(&pl, "GetEntityID", &[]);
    let st = |kind: f64| n(&stats, "GetStatValue", &[eid.a(), A::N(kind)]);
    let dev = so("PlayerDevelopmentSystem", "GetData", &[A::H(&pl)]);
    let mut out = Summary { health: st(stat("Health")), armor: st(stat("Armor")), dps: st(stat("EffectiveDPS")), ..Default::default() };
    let rec = so("TweakDBInterface", "GetUIStatsMapRecord", &[A::Name(tdb("UIMaps.Player_Stat_Panel_Other"))]);
    let percent = [stat("Visibility"), stat("MemoryRegenRate"), stat("FallDamageReduction"), stat("IntrinsicQuickHackUploadBonus")];
    for r in out_list(&rec, "PrimaryStats", &[]) {
        let kind = n(&r, "StatType", &[]);
        let v = st(kind);
        if v != 0.0 {
            let value = if percent.contains(&kind) {
                format!("{:.0}%", v * 100.0)
            } else {
                let s2 = format!("{v:.2}");
                let t = s2.trim_end_matches('0');
                t.strip_suffix('.').unwrap_or(t).to_string()
            };
            out.other.push((loc(&s(&r, "LocalizedName", &[])), value));
        }
    }
    for kind in ["Level", "StreetCred"] {
        let p = prof(kind);
        let xp = n(&dev, "GetCurrentLevelProficiencyExp", &[A::N(p)]);
        let remaining = n(&dev, "GetRemainingExpForLevelUp", &[A::N(p)]);
        out.progress.push(Progress { level: n(&dev, "GetProficiencyLevel", &[A::N(p)]), max: n(&dev, "GetProficiencyAbsoluteMaxLevel", &[A::N(p)]), xp, total: xp + remaining });
    }
    let pds = so("PlayerDevelopmentSystem", "GetInstance", &[A::H(&pl)]);
    out.life = enum_name("gamedataLifePath", n(&pds, "GetLifePath", &[A::H(&pl)]));
    let g = gi();
    if let Some(t) = rt::global_call("GetPlaythroughTime", &[gia(&g)]) {
        out.seconds = sn("EngineTime", "ToFloat", &[A::P(ptr(&t))]);
    }
    if let Some(arr) = rt::static_call("SteetCredDataHelper", "GetData", &[]) {
        for r in items(&Val::of(&arr)) {
            let h = r.handle();
            out.rewards.push((pnum(&h, "level"), loc(&ptext(&h, "description"))));
        }
    }
    Some(out)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Clock {
    pub hour: f64,
    pub minute: f64,
    pub allowed: bool,
    pub hidden: bool,
}
fn time_part(t: &crate::script_call::Owned, f: &str) -> f64 {
    sn("GameTime", f, &[A::P(ptr(t))])
}
pub(super) fn clock() -> Clock {
    let pl = player();
    let ts = super::game_system("GetTimeSystem");
    let (hour, minute) = rt::call(&ts, "GetGameTime", &[]).map_or((0.0, 0.0), |t| (time_part(&t, "Hours"), time_part(&t, "Minutes")));
    Clock { hour, minute, allowed: sb("GameTimeUtils", "CanPlayerTimeSkip", &[A::H(&pl)]), hidden: sb("GameTimeUtils", "IsTimeDisplayGlitched", &[A::H(&pl)]) }
}
pub(super) fn game_seconds_now() -> f64 {
    let ts = super::game_system("GetTimeSystem");
    rt::call(&ts, "GetGameTime", &[]).map_or(0.0, |t| game_seconds(&t))
}
/// D.skip: the time-system request and the player's fast-forward, as TimeskipGameController.Apply.
pub(super) fn skip(hours: f64) -> bool {
    let hours = hours.floor();
    let pl = player();
    if !(1.0..=24.0).contains(&hours) || !sb("GameTimeUtils", "CanPlayerTimeSkip", &[A::H(&pl)]) {
        return false;
    }
    let ts = super::game_system("GetTimeSystem");
    let Some(now) = rt::call(&ts, "GetGameTime", &[]) else { return false };
    rt::call(&ts, "SetGameTimeByHMS", &[A::N(time_part(&now, "Hours") + hours), A::N(time_part(&now, "Minutes")), A::N(time_part(&now, "Seconds")), A::Name(cname("ui_menu_timeskip"))]);
    rt::static_call("GameTimeUtils", "FastForwardPlayerState", &[A::H(&pl)]);
    true
}
