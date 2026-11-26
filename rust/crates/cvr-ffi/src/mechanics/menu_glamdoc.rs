//! glamdoc_data.lua and glamdoc_view.lua (main's 2026-11-17): a Glamdoc's arm cyberware styles in the world menu -- the
//! four arm categories (launcher, mantis blades, monowire, gorilla arms), the catalogue made of the player's own styles
//! and the vendor's GetGlamDocItems (owned ones not offered again, EP1 ones hidden without the expansion, recipes, side
//! upgrades and preview items left out); compatibility by the ArmsCW actually installed (an incompatible style can be
//! bought for later, not previewed nor applied). The preview is the game's non-persistent request on V's own arms
//! (ApplyCyberwareCustomizationDataRequest): the saved data and the equipment slot stay until APPLY, so CANCEL never
//! leaves an unowned style. The brand's two five-colour palettes, swap, defaults, removal; a purchase confirmed in the
//! menu, checked again against the stock, ownership, availability, the expansion and the money, bought once and awaited
//! (the vendor's spend and the game's threshold / glam_bought facts saved); APPLY saves and equips the style (the long
//! sleeves variant when worn).
use super::lobby_ink as ink;
use super::lobby_rtti::{self as rt, Val, A};
use super::menu_core::*;
use super::menu_ink as k;
use super::menu_locale as loc;
use super::menu_rt::*;
use red4ext_abi::handle::Handle;
use std::cell::RefCell;
use std::rc::Rc;

const TAGS: [&str; 4] = ["ProjectileLauncher", "MantisBlades", "NanoWires", "StrongArms"];
const TYPES: [&str; 4] = ["Cyb_Launcher", "Cyb_MantisBlades", "Cyb_NanoWires", "Cyb_StrongArms"];
const LABELS: [&str; 4] = ["LocKey#3722", "LocKey#3717", "LocKey#3720", "LocKey#3718"];
const FALLBACK: [&str; 4] = ["Launcher", "Mantis Blades", "Monowire", "Gorilla Arms"];
/// The standalone panel, centred in the 3200-pixel menu canvas.
const W: f64 = 1780.0;
pub(super) const X: f64 = 710.0;

/// A style the panel lists: the player's own or the vendor's.
#[derive(Clone)]
pub(super) struct Style {
    raw: Handle,
    key: String,
    name: String,
    kind: usize,
    vendor: bool,
    price: f64,
    quality: f64,
    icon: String,
}
struct Before {
    raw: Handle,
    enabled: bool,
    primary: i64,
    secondary: i64,
    dirty: bool,
}
struct Pending {
    key: String,
    price: f64,
    at: f64,
    colors: Option<(i64, i64)>,
    compatible: bool,
    apply_after: bool,
}
struct Baseline {
    item: Id16,
    primary: i64,
    secondary: i64,
    switcher: u64,
}
struct Quote {
    allowed: bool,
    price: Option<f64>,
    raw: Handle,
    compatible: bool,
}
struct Session {
    manager: Handle,
    vendor: Handle,
    player: Handle,
    player_key: u64,
    system: Handle,
    inventory: Handle,
    baseline: Baseline,
    raw: Handle,
    enabled: bool,
    primary: i64,
    secondary: i64,
    time: f64,
    revision: u64,
    drawn: u64,
    spend_cost: i64,
    spend_count: i64,
    dirty: bool,
    previewed: bool,
    preview_switchers: Vec<(String, u64)>,
    area: usize,
    focus: Option<Style>,
    confirm: Option<(Style, bool)>,
    preview: Option<Style>,
    before: Option<Before>,
    pending: Option<Pending>,
    next_poll: f64,
    error: Option<String>,
    closed: bool,
    accepted: bool,
    gone: bool,
}
thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
}
/// The session lent out for `f` (no borrow held across the game's calls); dropped when `f` marks it gone.
fn with_s<R>(f: impl FnOnce(&mut Session) -> R) -> Option<R> {
    let mut s = SESSION.with(|c| c.borrow_mut().take())?;
    let r = f(&mut s);
    if !s.gone {
        SESSION.with(|c| {
            let mut c = c.borrow_mut();
            if c.is_none() {
                *c = Some(s);
            }
        });
    }
    Some(r)
}
fn identity(e: &Handle) -> u64 {
    rt::call(e, "GetEntityID", &[]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
fn scriptable(name: &str) -> Handle {
    o(&super::game_system("GetScriptableSystemsContainer"), "Get", &[A::Name(cname(name))])
}
fn ts() -> Handle {
    super::game_system("GetTransactionSystem")
}
fn quests() -> Handle {
    super::game_system("GetQuestsSystem")
}
fn record_of(id: &Id16) -> String {
    ss("TDBID", "ToStringDEBUG", &[A::Name(id.tdbid())])
}
fn has(raw: &Handle, tag: &str) -> bool {
    !raw.is_null() && b(raw, "HasTag", &[A::Name(cname(tag))])
}
fn kind_of(raw: &Handle) -> Option<usize> {
    TAGS.iter().position(|t| has(raw, t)).map(|i| i + 1)
}
fn preview_flag(id: &Id16) -> bool {
    sb("ItemID", "HasFlag", &[id.a(), A::N(ev("gameEItemIDFlag", "Preview"))])
}
fn item_list(p: &Handle) -> Vec<Handle> {
    match rt::call_outs(&ts(), "GetItemList", &[A::H(p)], 1) {
        Some((_, outs)) => outs.first().map(|a| items(&Val::of(a)).iter().map(|v| v.handle()).collect()).unwrap_or_default(),
        None => Vec::new(),
    }
}
fn owned(s: &Session, key: &str) -> Handle {
    for raw in item_list(&s.player) {
        let id = id16(&raw, "GetID", &[]);
        if record_of(&id) == key && !preview_flag(&id) {
            return raw;
        }
    }
    Handle::null()
}
fn player_data(p: &Handle) -> Handle {
    rt::static_obj("EquipmentSystem", "GetData", &[A::H(p)])
}
/// G.arm: the category of the arm cyberware installed (1..4), or None.
fn arm(s: &Session) -> Option<usize> {
    let id = id16(&player_data(&s.player), "GetActiveItem", &[A::N(ev("gamedataEquipmentArea", "ArmsCW"))]);
    if !sb("ItemID", "IsValid", &[id.a()]) {
        return None;
    }
    let raw = o(&ts(), "GetItemData", &[A::H(&s.player), id.a()]);
    if raw.is_null() {
        return None;
    }
    let ty = n(&raw, "GetItemType", &[]);
    TYPES.iter().position(|t| ty == ev("gamedataItemType", t)).map(|i| i + 1)
}
fn compatible(s: &Session, raw: &Handle) -> bool {
    kind_of(raw).is_some_and(|k| Some(k) == arm(s))
}
fn helper_num(name: &str, raw: &Handle) -> i64 {
    sn("CyberwareCustomizationHelper", name, &[A::H(raw)]) as i64
}
fn helper_name(name: &str, raw: &Handle) -> u64 {
    rt::static_call("CyberwareCustomizationHelper", name, &[A::H(raw)]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) })
}
fn defaults(raw: &Handle) -> (i64, i64) {
    (helper_num("GetDefaultPrimaryColorIndexFromCustomizationItem", raw), helper_num("GetDefaultSecondaryColorIndexFromCustomizationItem", raw))
}
/// A property set to a CName.
fn set_name(h: &Handle, field: &str, name: u64) {
    if let Some(f) = rt::field(h, field) {
        // SAFETY: a CName property, eight bytes.
        unsafe { *(f.ptr as *mut u64) = name };
    }
}
/// A property set to an ItemID.
fn set_id(h: &Handle, field: &str, id: &Id16) {
    if let Some(f) = rt::field(h, field) {
        // SAFETY: an ItemID property; its bytes, at most sixteen.
        unsafe { core::ptr::copy_nonoverlapping(id.0.as_ptr().cast::<u8>(), f.ptr.cast::<u8>(), (f.ty.size() as usize).min(16)) };
    }
}
fn switcher(s: &Session, name: u64) {
    rt::call(&s.system, "SetIndexForCustomizationSwitcher", &[A::Name(name), A::N(0.0), A::B(true)]);
}
/// The preview on V's arms: the style's switcher on (any other previewed one reset), its colours; or, disabled, every
/// previewed switcher reset and the customization turned off (the saved item is not replaced).
fn preview(s: &mut Session) {
    if !s.raw.is_null() && s.enabled {
        let sw = helper_name("GetCyberarmTypeFromCustomizationItem", &s.raw);
        let key = red4ext_abi::cname::CName(sw).text().to_string();
        for (other, name) in s.preview_switchers.clone() {
            if other != key {
                switcher(s, name);
            }
        }
        if !s.preview_switchers.iter().any(|(k0, _)| *k0 == key) {
            s.preview_switchers.push((key, sw));
        }
        let request = ink::new("ApplyCyberwareCustomizationDataRequest");
        hset(&request, "enabled", 1.0);
        super::menu_main::set_handle_prop(&request, "customizationItem", &s.raw);
        hset(&request, "primaryColorIndex", s.primary as f64);
        hset(&request, "secondaryColorIndex", s.secondary as f64);
        rt::call(&s.system, "QueueRequest", &[A::H(&request)]);
    } else {
        for (_, name) in s.preview_switchers.clone() {
            switcher(s, name);
        }
        if s.baseline.switcher != 0 && s.baseline.switcher != cname("None") {
            switcher(s, s.baseline.switcher);
        }
        let (Some(c1), Some(c2)) = (new_struct("Color"), new_struct("Color")) else { return };
        rt::call(&s.system, "SetCustomizationParameters", &[A::B(false), A::P(ptr(&c1)), A::P(ptr(&c2))]);
    }
}
/// The game's saved look back: every previewed switcher reset first (a player with no saved style has switcherName None,
/// and Restore alone cannot undo a newly previewed switcher).
fn restore(s: &mut Session) {
    let pl = player();
    if s.previewed && !pl.is_null() && identity(&pl) == s.player_key {
        for (_, name) in s.preview_switchers.clone() {
            switcher(s, name);
        }
        rt::call(&s.system, "RestoreCyberwareCustomization", &[A::B(true)]);
    }
    s.previewed = false;
}
fn refresh(s: &mut Session) {
    s.revision += 1;
}
fn visit_ok() -> Option<(Handle, Handle)> {
    super::menu_vendor::glamdoc_visit()
}
/// G.begin: the session of the active Glamdoc visit (a new one unless one is open; none while a closed one still awaits
/// its purchase).
fn begin() -> bool {
    if with_s(|s| !s.closed) == Some(true) {
        return true;
    }
    let Some((vendor, manager)) = visit_ok() else { return false };
    let pl = player();
    if pl.is_null() || with_s(|s| s.pending.is_some()) == Some(true) {
        return false;
    }
    let system = scriptable("CyberwareCustomizationSystem");
    if system.is_null() {
        return false;
    }
    let saved = rt::call(&system, "GetSavedData", &[]);
    let sv = saved.as_ref().map(Val::of);
    let field = |f: &str| sv.as_ref().and_then(|v| v.f(f));
    let item = field("customizationItemID").map(|v| Id16::of(&v)).unwrap_or_default();
    let raw = if sb("ItemID", "IsValid", &[item.a()]) { o(&ts(), "GetItemData", &[A::H(&pl), item.a()]) } else { Handle::null() };
    let baseline = Baseline {
        item,
        primary: field("primaryColorIndex").and_then(|v| v.num()).unwrap_or(0.0) as i64,
        secondary: field("secondaryColorIndex").and_then(|v| v.num()).unwrap_or(0.0) as i64,
        switcher: field("switcherName").map_or(0, |v| unsafe { *(v.ptr as *const u64) }),
    };
    let enabled = field("enabled").is_some_and(|v| v.bool()) && !raw.is_null();
    let vendor_id = id(&manager, "GetVendorID", &[]);
    let spend = n(&super::game_system("GetInventoryManager"), "GetSpendByVendorID", &[A::Name(vendor_id)]) as i64;
    let mut s = Session {
        manager,
        vendor,
        player_key: identity(&pl),
        player: pl,
        system,
        inventory: scriptable("UIInventoryScriptableSystem"),
        primary: baseline.primary,
        secondary: baseline.secondary,
        baseline,
        raw,
        enabled,
        time: 0.0,
        revision: 1,
        drawn: 0,
        spend_cost: spend / 100,
        spend_count: spend % 100,
        dirty: false,
        previewed: false,
        preview_switchers: Vec::new(),
        area: 1,
        focus: None,
        confirm: None,
        preview: None,
        before: None,
        pending: None,
        next_poll: 0.0,
        error: None,
        closed: false,
        accepted: false,
        gone: false,
    };
    s.area = arm(&s).unwrap_or(1);
    SESSION.with(|c| *c.borrow_mut() = Some(s));
    true
}
fn style(s: &Session, raw: &Handle, vendor: bool) -> Option<Style> {
    let id = id16(raw, "GetID", &[]);
    if !has(raw, "Glamdoc") || preview_flag(&id) {
        return None;
    }
    let kind = kind_of(raw)?;
    let owner = if vendor { &s.vendor } else { &s.player };
    let ui = so("UIInventoryItem", "Make", &[A::H(owner), A::H(raw), A::H(&o(&s.inventory, "GetInventoryItemsManager", &[]))]);
    if ui.is_null() || b(&ui, "IsRecipe", &[]) || b(&player_data(&s.player), "IsSideUpgradeEquipped", &[id.a()]) {
        return None;
    }
    Some(Style {
        raw: raw.clone(),
        key: record_of(&id),
        name: s_(&ui, "GetName"),
        kind,
        vendor,
        price: if vendor { n(&s.manager, "GetBuyingPrice", &[id.a()]) } else { 0.0 },
        quality: n(&ui, "GetQualityInt", &[]),
        icon: s_(&ui, "GetIconPath"),
    })
}
fn s_(h: &Handle, name: &str) -> String {
    super::menu_rt::s(h, name, &[])
}
fn ep1() -> bool {
    rt::global_call("IsEP1", &[]).is_some_and(|o| Val::of(&o).bool())
}
/// The vendor's Glamdoc stock: (the item, available).
fn stock(s: &Session) -> Vec<(Handle, bool)> {
    let Some(arr) = rt::call(&s.manager, "GetGlamDocItems", &[]) else { return Vec::new() };
    items(&Val::of(&arr))
        .iter()
        .map(|st| {
            let raw = st.f("itemData").map(|v| v.handle()).unwrap_or_else(Handle::null);
            let available = st.f("itemStack").and_then(|s0| rt::static_call("VendorItemAdditionalData", "Make", &[A::P(s0.ptr.cast_const())])).is_some_and(|a| fbool(&Val::of(&a), "IsAvailable"));
            (raw, available)
        })
        .collect()
}
/// G.rows: the open category's styles -- the player's own first, then by quality, name and record.
fn rows(s: &Session) -> Vec<Style> {
    let mut out = Vec::new();
    let mut have = std::collections::HashSet::new();
    for raw in item_list(&s.player) {
        if let Some(v) = style(s, &raw, false) {
            have.insert(v.key.clone());
            if v.kind == s.area {
                out.push(v);
            }
        }
    }
    let ep1 = ep1();
    for (raw, _) in stock(s) {
        if raw.is_null() || have.contains(&record_of(&id16(&raw, "GetID", &[]))) || (!ep1 && has(&raw, "EP1")) {
            continue;
        }
        if let Some(v) = style(s, &raw, true).filter(|v| v.kind == s.area) {
            out.push(v);
        }
    }
    out.sort_by(|a, b| {
        a.vendor
            .cmp(&b.vendor)
            .then(b.quality.partial_cmp(&a.quality).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.name.cmp(&b.name))
            .then(a.key.cmp(&b.key))
    });
    out
}
/// G.select: a compatible style previewed (the vendor's) or chosen (the player's own).
fn select(s: &mut Session, v: &Style) -> bool {
    if s.closed || s.pending.is_some() || !compatible(s, &v.raw) {
        return false;
    }
    let raw = if v.vendor { v.raw.clone() } else { owned(s, &v.key) };
    if raw.is_null() {
        return false;
    }
    if v.vendor && s.preview.is_none() {
        s.before = Some(Before { raw: s.raw.clone(), enabled: s.enabled, primary: s.primary, secondary: s.secondary, dirty: s.dirty });
    }
    (s.primary, s.secondary) = defaults(&raw);
    (s.raw, s.enabled) = (raw, true);
    s.preview = v.vendor.then(|| v.clone());
    if !v.vendor {
        s.before = None;
        s.dirty = true;
    }
    preview(s);
    s.previewed = true;
    refresh(s);
    true
}
fn cancel_preview(s: &mut Session) -> bool {
    if s.preview.is_none() || s.pending.is_some() {
        return false;
    }
    if let Some(b0) = s.before.take() {
        (s.raw, s.enabled, s.primary, s.secondary, s.dirty) = (b0.raw, b0.enabled, b0.primary, b0.secondary, b0.dirty);
    }
    s.preview = None;
    preview(s);
    s.previewed = true;
    refresh(s);
    true
}
/// G.colors: the brand's five colours (0..1).
fn colors(s: &Session) -> Option<Vec<[f64; 3]>> {
    if s.raw.is_null() || !s.enabled {
        return None;
    }
    let brand = rt::static_obj("CyberwareCustomizationHelper", "GetBrandDataFromCustomizationItem", &[A::H(&s.raw)]);
    if brand.is_null() || !sb("TDBID", "IsValid", &[A::Name(id(&brand, "GetRecordID", &[]))]) {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..5 {
        let c = rt::static_call("CyberwareCustomizationHelper", "GetColorFromBrandData", &[A::H(&brand), A::N(f64::from(i))]);
        let v = c.as_ref().map(Val::of);
        let f = |n0: &str| v.as_ref().map(|v| fnum(v, n0)).unwrap_or(0.0) / 255.0;
        out.push([f("Red"), f("Green"), f("Blue")]);
    }
    Some(out)
}
fn recolor(s: &mut Session) {
    if s.preview.is_none() {
        s.dirty = true;
    }
    preview(s);
    s.previewed = true;
    refresh(s);
}
fn color(s: &mut Session, primary: bool, index: i64) -> bool {
    if s.closed || s.pending.is_some() || colors(s).is_none() || !(0..=4).contains(&index) {
        return false;
    }
    if primary {
        s.primary = index;
    } else {
        s.secondary = index;
    }
    recolor(s);
    true
}
fn swap(s: &mut Session) -> bool {
    if s.pending.is_some() || colors(s).is_none() {
        return false;
    }
    (s.primary, s.secondary) = (s.secondary, s.primary);
    recolor(s);
    true
}
fn reset_colors(s: &mut Session) -> bool {
    if s.pending.is_some() || colors(s).is_none() {
        return false;
    }
    (s.primary, s.secondary) = defaults(&s.raw);
    recolor(s);
    true
}
fn remove(s: &mut Session) -> bool {
    if s.closed || s.pending.is_some() || s.preview.is_some() {
        return false;
    }
    s.enabled = false;
    s.dirty = true;
    preview(s);
    s.previewed = true;
    refresh(s);
    true
}
/// G.quote: a vendor's style checked against the stock, ownership, availability, the expansion and the money.
fn quote(s: &Session, v: &Style) -> Quote {
    let refused = |_reason: &str, price| Quote { allowed: false, price, raw: Handle::null(), compatible: false };
    if s.closed || s.pending.is_some() || !v.vendor {
        return refused("busy", None);
    }
    if !owned(s, &v.key).is_null() {
        return refused("owned", None);
    }
    let ep1 = ep1();
    for (raw, available) in stock(s) {
        let rid = id16(&raw, "GetID", &[]);
        if raw.is_null() || record_of(&rid) != v.key {
            continue;
        }
        let price = n(&s.manager, "GetBuyingPrice", &[rid.a()]);
        if !(available && (ep1 || !has(&raw, "EP1"))) {
            return refused("requirements", Some(price));
        }
        let compatible = compatible(s, &raw) && b(&player_data(&s.player), "IsEquippable", &[A::H(&raw)]);
        return Quote { allowed: n(&s.manager, "GetLocalPlayerCurrencyAmount", &[]) >= price, price: Some(price), raw, compatible };
    }
    refused("missing", None)
}
fn buy(s: &mut Session, v: &Style, apply_after: bool) -> bool {
    let q = quote(s, v);
    if !q.allowed || (apply_after && !q.compatible) {
        return false;
    }
    let keep = s.preview.as_ref().is_some_and(|p| p.key == v.key);
    s.pending = Some(Pending { key: v.key.clone(), price: q.price.unwrap_or(0.0), at: s.time, colors: keep.then_some((s.primary, s.secondary)), compatible: q.compatible, apply_after });
    rt::call(&s.manager, "BuyItemFromVendor", &[A::H(&q.raw), A::N(1.0)]);
    s.error = None;
    refresh(s);
    true
}
/// The vendor's spend saved, and the Glamdoc record's thresholds turned into their facts.
fn save_spend(s: &Session) {
    let vendor_id = id(&s.manager, "GetVendorID", &[]);
    rt::call(&super::game_system("GetInventoryManager"), "SaveSpendByVendorID", &[A::Name(vendor_id), A::N(s.spend_cost as f64), A::N(s.spend_count as f64)]);
    let data = rt::static_obj("TweakDBInterface", "GetGlamdocRecord", &[A::Name(vendor_id)]);
    if data.is_null() {
        return;
    }
    let qs = quests();
    for i in 0..n(&data, "GetFactThresholdsCount", &[]) as i64 {
        let t = o(&data, "GetFactThresholdsItem", &[A::N(i as f64)]);
        let key = rt::call(&t, "FactKey", &[]).map_or(0, |o| unsafe { *(o.as_ptr() as *const u64) });
        if (s.spend_cost as f64) > n(&t, "SpendCost", &[]) && (s.spend_count as f64) > n(&t, "SpendCount", &[]) && n(&qs, "GetFact", &[A::Name(key)]) <= i as f64 {
            rt::call(&qs, "SetFact", &[A::Name(key), A::N((i + 1) as f64)]);
        }
    }
}
fn set_fact_str(name: &str, v: f64) {
    let a = Arg::new(name);
    rt::call(&quests(), "SetFactStr", &[a.a(), A::N(v)]);
}
/// G.poll: an ordered style awaited (owned within 15 s), then put on when it fits; a closed session let go.
pub(super) fn poll(dt: f64) {
    with_s(|s| {
        s.time += dt.max(0.0);
        let pl = player();
        if pl.is_null() || identity(&pl) != s.player_key {
            s.gone = true;
            return;
        }
        if s.pending.is_some() && s.time >= s.next_poll {
            s.next_poll = s.time + 0.2;
            let purchase = s.pending.take().expect("pending");
            let raw = owned(s, &purchase.key);
            if !raw.is_null() {
                s.spend_cost += purchase.price as i64;
                s.spend_count += 1;
                set_fact_str("glam_bought", 1.0);
                save_spend(s);
                if !s.closed && purchase.compatible && compatible(s, &raw) {
                    (s.primary, s.secondary) = defaults(&raw);
                    (s.raw, s.enabled, s.preview, s.before, s.dirty) = (raw, true, None, None, true);
                    if let Some((p0, s0)) = purchase.colors {
                        (s.primary, s.secondary) = (p0, s0);
                    }
                    preview(s);
                    s.previewed = true;
                }
                refresh(s);
                if purchase.apply_after && !s.closed {
                    finish_inner(s, true);
                }
            } else if s.time - purchase.at > 15.0 {
                s.error = Some("purchase-timeout".into());
                refresh(s);
            } else {
                s.pending = Some(purchase);
            }
        }
        if s.closed && s.pending.is_none() {
            cvr_host::log!("[VRMenu] glamdoc closed: accepted={} error={} (from Rust)
", s.accepted, s.error.as_deref().unwrap_or(""));
            s.gone = true;
        }
    });
}
/// G.finish: APPLY saves and equips the style (or takes it off) when it changed, CANCEL gives the saved look back.
fn finish_inner(s: &mut Session, accept: bool) -> bool {
    if s.closed {
        return true;
    }
    if accept && (s.pending.is_some() || s.preview.is_some()) {
        return false;
    }
    if accept {
        if s.enabled && (s.raw.is_null() || owned(s, &record_of(&id16(&s.raw, "GetID", &[]))).is_null() || !compatible(s, &s.raw.clone())) {
            return false;
        }
        if s.dirty {
            let request = ink::new("SaveAndApplyCyberwareCustomizationDataRequest");
            hset(&request, "enabled", if s.enabled { 1.0 } else { 0.0 });
            super::menu_main::set_handle_prop(&request, "customizationItem", &if s.enabled { s.raw.clone() } else { Handle::null() });
            let sw = if s.raw.is_null() { s.baseline.switcher } else { helper_name("GetCyberarmTypeFromCustomizationItem", &s.raw) };
            set_name(&request, "switcherName", sw);
            let mut index = 0;
            if s.enabled {
                let long = helper_num("GetLongSleevesCustomizationIndexFromCustomizationItem", &s.raw);
                index = if long != -1 && sb("EquipmentSystemPlayerData", "HasLongSleeves", &[A::H(&s.player)]) { long } else { helper_num("GetCustomizationIndexFromCustomizationItem", &s.raw) };
            }
            hset(&request, "switcherIndex", index as f64);
            hset(&request, "primaryColorIndex", s.primary as f64);
            hset(&request, "secondaryColorIndex", s.secondary as f64);
            rt::call(&s.system, "QueueRequest", &[A::H(&request)]);
            let equipment = scriptable("EquipmentSystem");
            if s.enabled {
                let equip = ink::new("EquipRequest");
                super::menu_main::set_handle_prop(&equip, "owner", &s.player);
                set_id(&equip, "itemID", &id16(&s.raw, "GetID", &[]));
                hset(&equip, "slotIndex", 0.0);
                rt::call(&equipment, "QueueRequest", &[A::H(&equip)]);
            } else {
                let unequip = ink::new("UnequipRequest");
                super::menu_main::set_handle_prop(&unequip, "owner", &s.player);
                super::menu_main::set_enum_prop(&unequip, "areaType", "gamedataEquipmentArea", "ArmsCWStyle");
                hset(&unequip, "slotIndex", 0.0);
                hset(&unequip, "force", 0.0);
                rt::call(&equipment, "QueueRequest", &[A::H(&unequip)]);
            }
            set_fact_str("glam_equipped", if s.enabled { 1.0 } else { 0.0 });
            set_fact_str("cyberware_changes_save", 1.0);
            let item_changed = !s.raw.is_null() && id16(&s.raw, "GetID", &[]) != s.baseline.item;
            if s.primary != s.baseline.primary || s.secondary != s.baseline.secondary || item_changed {
                set_fact_str("glamdoc_color", 1.0);
            }
            s.previewed = false;
        } else {
            restore(s);
        }
        s.accepted = true;
    } else {
        restore(s);
    }
    save_spend(s);
    s.closed = true;
    true
}
pub(super) fn finish(accept: bool) -> bool {
    with_s(|s| finish_inner(s, accept)).unwrap_or(true)
}
/// G.current: an open session (not closed).
pub(super) fn open_session() -> bool {
    with_s(|s| !s.closed) == Some(true)
}
/// A closed session still waiting for its purchase.
pub(super) fn closed_pending() -> bool {
    with_s(|s| s.closed && s.pending.is_some()) == Some(true)
}

// ---------------------------------------------------------------- the panel (glamdoc_view.lua)
fn label(index: usize) -> String {
    let t = super::menu_data::loc(LABELS[index - 1]);
    if t.is_empty() || t == LABELS[index - 1] {
        FALLBACK[index - 1].to_string()
    } else {
        t
    }
}
fn redraw(m: &mut Menu) {
    super::menu_main::build_panels(m, false, false);
}
fn act(m: &mut Menu, p: &Handle, x: f64, y: f64, w: f64, text: &str, enabled: bool, f: Act, key: &str) {
    let bg = k::round_fill(p, x, y, w, 68.0, k::NAVY, if enabled { 0.18 } else { 0.04 }, Some(4.0));
    let edge = k::cut_frame(&mut m.pool, p, x, y, w, 68.0, if enabled { k::CYAN } else { k::GREY }, if enabled { 0.5 } else { 0.2 }, Some(10.0), Some(1.5));
    let tw = k::text(p, x + 12.0, y + 2.0, w - 24.0, 64.0, text, 32.0, if enabled { k::TEXT } else { k::GREY }, "center", true);
    let b = button(
        m,
        "p1",
        &at(X, P1Y),
        x,
        y,
        w,
        68.0,
        Some(Rc::new(move |m: &mut Menu, hand| {
            if enabled {
                f(m, hand);
            }
        })),
        Some(Rc::new(move |on, _| {
            if !enabled {
                return;
            }
            ink::tint(&bg, if on { k::YELLOW } else { k::NAVY });
            ink::opacity(&bg, if on { 0.22 } else { 0.18 });
            k::tint(&edge, if on { k::YELLOW } else { k::CYAN }, Some(if on { 1.0 } else { 0.5 }));
            ink::tint(&tw, if on { k::YELLOW } else { k::TEXT });
        })),
    );
    *b.key.borrow_mut() = Some(key.into());
}
/// A view action on the session; then the panel drawn again.
fn on(f: impl Fn(&mut Session) + 'static) -> Act {
    Rc::new(move |m: &mut Menu, _| {
        with_s(|s| f(s));
        redraw(m);
    })
}
pub(super) fn draw(m: &mut Menu, p: &Handle) {
    if !begin() {
        return;
    }
    let Some(mut s) = SESSION.with(|c| c.borrow_mut().take()) else { return };
    draw_session(m, p, &mut s);
    SESSION.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = Some(s);
        }
    });
}
fn draw_session(m: &mut Menu, p: &Handle, s: &mut Session) {
    let tr = loc::tr;
    s.drawn = s.revision;
    glass(m, p, W, 1210.0, Some((X, P1Y)));
    k::text(p, 24.0, 8.0, 1060.0, 76.0, &tr("ГЛЭМДОК", "GLAMDOC"), 42.0, k::CYAN, "left", false);
    k::text(p, 1120.0, 12.0, 630.0, 64.0, &format!("€$ {}", super::menu_gpdata::fmt_g(n(&s.manager, "GetLocalPlayerCurrencyAmount", &[]))), 34.0, k::YELLOW, "right", false);
    let busy = s.pending.is_some();
    for i in 1..=4 {
        let x = 24.0 + (i - 1) as f64 * 435.0;
        act(
            m,
            p,
            x,
            100.0,
            420.0,
            &label(i),
            !busy && s.preview.is_none(),
            on(move |s| {
                (s.area, s.focus, s.confirm) = (i, None, None);
                s.revision += 1;
            }),
            &format!("glamdocArea:{i}"),
        );
        if s.area == i {
            k::rect(p, x + 8.0, 170.0, 404.0, 3.0, k::YELLOW, 1.0);
        }
    }
    let installed = arm(s);
    let text = match installed {
        Some(a) => format!("{}{}", tr("Установлено: ", "Installed: "), label(a)),
        None => tr("Имплант рук не установлен. Покупка доступна, примерка и применение требуют совместимого импланта.", "No arm cyberware installed. You can purchase styles; preview and apply require compatible arm cyberware."),
    };
    k::text(p, 26.0, 185.0, 1725.0, 60.0, &text, 28.0, if installed.is_some() { k::TEXT } else { k::YELLOW }, "left", true);
    let entries = rows(s);
    let mut list_rows = Vec::new();
    for v in entries.iter().cloned() {
        let selected = s.focus.as_ref().is_some_and(|f| f.key == v.key);
        list_rows.push(Row {
            h: 124.0,
            build: Rc::new(move |m: &mut Menu, c: &Handle, it: RowCtx| {
                let bg = k::round_fill(c, 4.0, 6.0, it.cw - 8.0, 110.0, if selected { k::YELLOW } else { k::ICE }, if selected { 0.19 } else { 0.06 }, Some(7.0));
                let mut x = 16.0;
                if !v.icon.is_empty() && v.icon != "None" {
                    if let Some(pic) = super::menu_gpdata::image(tdb(&v.icon)) {
                        k::image_raw(c, 12.0, 12.0, 90.0, 90.0, pic.atlas, pic.part, Some(k::WHITE), 1.0);
                        x = 116.0;
                    }
                }
                k::text(c, x, 10.0, it.cw - x - 12.0, 49.0, &k::clip(&v.name, 42), 29.0, k::TEXT, "left", false);
                let price = if v.vendor { format!("€$ {}", super::menu_gpdata::fmt_g(v.price)) } else { loc::tr("В СОБСТВЕННОСТИ", "OWNED") };
                k::text(c, x, 61.0, it.cw - x - 12.0, 40.0, &price, 25.0, if v.vendor { k::YELLOW } else { k::CYAN }, "left", false);
                let v2 = v.clone();
                let b = button(
                    m,
                    "p1",
                    &At { ox: it.vx, oy: it.y, view: Some(it.view.clone()) },
                    4.0,
                    6.0,
                    it.cw - 8.0,
                    110.0,
                    Some(Rc::new(move |m: &mut Menu, _| {
                        if busy {
                            return;
                        }
                        with_s(|s| (s.focus, s.confirm) = (Some(v2.clone()), None));
                        redraw(m);
                    })),
                    Some(Rc::new(move |h, _| ink::opacity(&bg, if h { 0.25 } else if selected { 0.19 } else { 0.06 }))),
                );
                *b.key.borrow_mut() = Some(format!("glamdocItem:{}", v.key));
            }),
        });
    }
    let empty = list_rows.is_empty();
    let keep = m.keep_scroll;
    list(m, p, "p1", "glamdoc", X, P1Y, 265.0, 815.0, list_rows, 790.0, keep);
    if empty {
        k::text(p, 35.0, 310.0, 770.0, 120.0, &tr("Нет доступных стилей для этой категории", "No available styles in this category"), 32.0, k::TEXT, "left", true);
    }
    let x = 880.0;
    match s.focus.clone() {
        Some(focus) => {
            k::text(p, x, 270.0, 850.0, 90.0, &focus.name, 34.0, k::CYAN, "left", true);
            let fits = compatible(s, &focus.raw);
            let hint = if fits { tr("Примерьте стиль и посмотрите на свои руки.", "Preview the style on your own arms.") } else { tr("Для примерки и применения нужен другой имплант рук. Стиль можно купить заранее.", "Preview and apply require different arm cyberware. You can buy this style for later.") };
            k::text(p, x, 366.0, 850.0, 80.0, &hint, 28.0, if fits { k::TEXT } else { k::YELLOW }, "left", true);
            let f2 = focus.clone();
            act(
                m,
                p,
                x,
                460.0,
                410.0,
                &if focus.vendor { tr("ПРИМЕРИТЬ", "PREVIEW") } else { tr("ВЫБРАТЬ", "SELECT") },
                fits && !busy,
                Rc::new(move |m: &mut Menu, _| {
                    let ok = with_s(|s| {
                        let ok = select(s, &f2);
                        s.confirm = None;
                        ok
                    }) == Some(true);
                    if !ok {
                        let t = loc::tr("Стиль сейчас недоступен", "Style is currently unavailable");
                        toast(m, &t);
                    }
                    redraw(m);
                }),
                "glamdocPreview",
            );
            if focus.vendor {
                let q = quote(s, &focus);
                let f3 = focus.clone();
                act(
                    m,
                    p,
                    x + 430.0,
                    460.0,
                    410.0,
                    &format!("{}  €$ {}", tr("КУПИТЬ", "BUY"), super::menu_gpdata::fmt_g(focus.price)),
                    q.allowed,
                    on(move |s| s.confirm = Some((f3.clone(), false))),
                    "glamdocBuy",
                );
            }
        }
        None => {
            k::text(p, x, 290.0, 850.0, 120.0, &tr("Выберите стиль слева", "Select a style on the left"), 34.0, k::TEXT, "left", true);
        }
    }
    if let Some((item, apply_after)) = s.confirm.clone() {
        let q = quote(s, &item);
        k::round_fill(p, x - 10.0, 555.0, 870.0, 430.0, k::NAVY, 0.95, Some(12.0));
        k::text(p, x + 20.0, 580.0, 810.0, 90.0, &format!("{}{}", tr("Подтвердить покупку: ", "Confirm purchase: "), item.name), 32.0, k::TEXT, "left", true);
        k::text(p, x + 20.0, 687.0, 810.0, 70.0, &format!("€$ {}", super::menu_gpdata::fmt_g(q.price.unwrap_or(item.price))), 40.0, k::YELLOW, "left", false);
        if !q.compatible {
            k::text(p, x + 20.0, 758.0, 810.0, 70.0, &tr("Стиль будет куплен без установки", "Style will be purchased without equipping"), 28.0, k::YELLOW, "left", true);
        }
        act(m, p, x + 20.0, 875.0, 390.0, &tr("НАЗАД", "BACK"), true, on(|s| s.confirm = None), "glamdocBuyCancel");
        act(
            m,
            p,
            x + 430.0,
            875.0,
            390.0,
            &tr("ПОДТВЕРДИТЬ", "CONFIRM"),
            q.allowed,
            Rc::new(move |m: &mut Menu, _| {
                let ok = with_s(|s| {
                    let ok = buy(s, &item, apply_after);
                    s.confirm = None;
                    ok
                }) == Some(true);
                if !ok {
                    let t = loc::tr("Покупка недоступна: проверьте баланс и условия", "Purchase unavailable: check funds and requirements");
                    toast(m, &t);
                }
                redraw(m);
            }),
            "glamdocBuyConfirm",
        );
    } else {
        if let Some(palette) = colors(s) {
            k::text(p, x, 556.0, 850.0, 50.0, &if s.preview.is_some() { tr("ЦВЕТА ПРИМЕРКИ", "PREVIEW COLORS") } else { tr("ЦВЕТА СТИЛЯ", "STYLE COLORS") }, 30.0, k::CYAN, "left", false);
            for (ci, primary) in [(0usize, true), (1, false)] {
                let y = 620.0 + ci as f64 * 124.0;
                k::text(p, x, y, 220.0, 70.0, &if primary { tr("Основной", "Primary") } else { tr("Дополнительный", "Secondary") }, 26.0, k::TEXT, "left", false);
                let chosen = if primary { s.primary } else { s.secondary };
                for (i, c) in palette.iter().enumerate() {
                    let index = i as i64;
                    let bx = x + 240.0 + i as f64 * 116.0;
                    k::round_fill(p, bx, y, 94.0, 74.0, *c, 1.0, Some(8.0));
                    if chosen == index {
                        k::round_edge(p, bx - 4.0, y - 4.0, 102.0, 82.0, k::WHITE, 1.0, Some(8.0));
                    }
                    let b = button(
                        m,
                        "p1",
                        &at(X, P1Y),
                        bx,
                        y,
                        94.0,
                        74.0,
                        Some(Rc::new(move |m: &mut Menu, _| {
                            if with_s(|s| color(s, primary, index)) == Some(true) {
                                redraw(m);
                            }
                        })),
                        None,
                    );
                    *b.key.borrow_mut() = Some(format!("glamdocColor:{}:{index}", if primary { "primary" } else { "secondary" }));
                }
            }
            act(m, p, x, 885.0, 410.0, &tr("ПО УМОЛЧАНИЮ", "DEFAULT COLORS"), !busy, on(|s| { reset_colors(s); }), "glamdocColorsDefault");
            act(m, p, x + 430.0, 885.0, 410.0, &tr("ПОМЕНЯТЬ МЕСТАМИ", "SWAP COLORS"), !busy, on(|s| { swap(s); }), "glamdocColorsSwap");
        }
        if s.preview.is_some() {
            act(m, p, x, 985.0, 840.0, &tr("ЗАКОНЧИТЬ ПРИМЕРКУ", "CANCEL PREVIEW"), !busy, on(|s| { cancel_preview(s); }), "glamdocPreviewCancel");
        } else if s.enabled {
            act(m, p, x, 985.0, 840.0, &tr("СНЯТЬ СТИЛЬ", "REMOVE STYLE"), !busy, on(|s| { remove(s); }), "glamdocRemove");
        }
    }
    if busy {
        k::text(p, 30.0, 1065.0, 1700.0, 42.0, &tr("Покупка выполняется…", "Purchase in progress…"), 28.0, k::YELLOW, "left", false);
    } else if s.error.is_some() {
        k::text(p, 30.0, 1065.0, 1700.0, 42.0, &tr("Покупка пока не подтверждена. Проверьте инвентарь и баланс.", "Purchase not confirmed yet. Check your inventory and funds."), 27.0, k::YELLOW, "left", false);
    }
    act(
        m,
        p,
        25.0,
        1120.0,
        825.0,
        &tr("ОТМЕНА", "CANCEL"),
        true,
        Rc::new(|m: &mut Menu, _| {
            finish(false);
            super::menu_main::close(m);
        }),
        "glamdocCancel",
    );
    let previewing = s.preview.clone();
    act(
        m,
        p,
        900.0,
        1120.0,
        825.0,
        &if previewing.is_some() { tr("КУПИТЬ И ПРИМЕНИТЬ", "BUY AND APPLY") } else { tr("ПРИМЕНИТЬ", "APPLY") },
        !busy,
        Rc::new(move |m: &mut Menu, _| {
            if let Some(pv) = previewing.clone() {
                with_s(|s| s.confirm = Some((pv, true)));
                redraw(m);
            } else if finish(true) {
                super::menu_main::close(m);
            } else {
                let t = loc::tr("Для применения нужен совместимый имплант рук", "Compatible arm cyberware is required to apply this style");
                toast(m, &t);
            }
        }),
        "glamdocApply",
    );
}
/// V.update: the menu closes with the session; a change made elsewhere (a purchase landing) draws the panel again.
pub(super) fn update(m: &mut Menu) {
    match with_s(|s| (s.closed, s.revision != s.drawn)) {
        None | Some((true, _)) => super::menu_main::close(m),
        Some((false, true)) => redraw(m),
        _ => {}
    }
}
