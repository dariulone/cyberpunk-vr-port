//! mods/cet/CyberpunkVRPort_WristHud (the readout on the left forearm's band): the band's widget tree -- fed by the
//! player's vrp_latd_hud WidgetHudComponent (entry vrp_wrist; the world widget vrp_latd_ui as the fallback) -- gets
//! its own widgets built into it once (stale runtime children swept, the authored placeholder hidden): the health
//! number and the word HP, the health bar's track, fill and the gold overshield, the cyberdeck RAM ticks and their
//! x/y, the level in its box, five wanted stars from the wanted bar's atlas and sixteen buff icons. Laid out from
//! wrist.cfg (its keys and defaults: wrist_hud_spec.rs; re-read when the file changes -- the CET overlay's SAVE), the
//! values every 0.12 s (health with overshield against max health, RAM used of max, the level, the wanted level from
//! UI_WantedBar), the icons every 0.5 s (the applied status effects' UiData icons through their UIIcon flats); the
//! band's component transform and the fallback's glitch every 0.5 s. Rebuilt whenever our children are gone from the
//! tree (a new widget instance), checked four times a second; emptied in the hands-only mode. The plugin's watchdog
//! (VRWristGuard(5)) catches the render-plane transition and freezes it: every frame until done, then four times a
//! second. The CET overlay's tuning window stays the Lua host's (it edits wrist.cfg).
use super::wrist_hud_spec::SPEC;
use crate::script_call::{call_handle, call_native, enum_value, method, method_owned, method_typed, Value};
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;
use std::sync::Mutex;

const SCREEN: &str = "vrp_latd_screen";
const UI: &str = "vrp_latd_ui";
const HUD: &str = "vrp_latd_hud";
const ENTRY: &str = "vrp_wrist";
const FONT_THIN: (&std::ffi::CStr, &str) = (c"base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily", "Book");
const FONT_BOLD: (&std::ffi::CStr, &str) = (c"base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily", "Medium");
const MAX_TICKS: usize = 32;
const MAX_ICONS: usize = 16;
const MAX_STARS: usize = 5;
const STAR_ATLAS: &str = "base\\gameplay\\gui\\widgets\\wanted\\atlas_wanted_2.inkatlas";
const STAR_PART: &str = "star_active";
/// The band's own frame in model space (the mesh build's numbers).
const PIVOT: [f64; 3] = [-0.4331, 0.1311, 1.1120];
const ALONG: [f64; 3] = [-0.3859, 0.7383, -0.5532];
const INWARD: [f64; 3] = [-0.6733, -0.6353, -0.3783];

#[derive(Default)]
struct Widgets {
    track: Handle,
    fill: Handle,
    over: Handle,
    hp: Handle,
    lbl: Handle,
    ram: Handle,
    lvl: Handle,
    lvl_box: Vec<Handle>,
    ticks: Vec<Handle>,
    icons: Vec<Handle>,
    stars: Vec<Handle>,
}
struct State {
    cfg: HashMap<&'static str, f64>,
    built: bool,
    child_mark: i32,
    w: Widgets,
    tick_n: usize,
    dirty: bool,
    acc_fast: f64,
    acc_slow: f64,
    acc_check: f64,
    acc_ours: f64,
    acc_watch: f64,
    rearmed: bool,
    guard: Option<bool>,
    note: String,
    reported: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);

type R<T> = Result<T, String>;
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> Option<T> {
    if o.is_null() {
        return None;
    }
    // SAFETY: the caller's live object, held.
    unsafe { method::<T>(o.instance, name, values) }
}
fn h(o: &Handle, name: &str, values: &[Value]) -> Handle {
    if o.is_null() {
        return Handle::null();
    }
    // SAFETY: as above.
    unsafe { call_handle(o.instance, name, values) }.unwrap_or_else(Handle::null)
}
fn cfg(s: &State, key: &str) -> f64 {
    s.cfg.get(key).copied().unwrap_or(0.0)
}
fn cfg_path() -> std::path::PathBuf {
    cvr_host::paths::file(cvr_host::paths::Folder::Lua, "CyberpunkVRPort_WristHud\\wrist.cfg")
}
/// The layout the mod shipped as its wrist.cfg (mods/cet/CyberpunkVRPort_WristHud/wrist.cfg), built in: the package
/// carries no Lua mod folders any more.
const SHIPPED_CFG: &str = include_str!("../../../../../mods/cet/CyberpunkVRPort_WristHud/wrist.cfg");
/// The mod's loadCfg(): the defaults, the shipped layout over them, then `key=number` lines of the player's wrist.cfg.
fn load_cfg(s: &mut State) {
    s.cfg = SPEC.iter().map(|&(k, d)| (k, d)).collect();
    apply_cfg(s, SHIPPED_CFG);
    let Ok(body) = std::fs::read_to_string(cfg_path()) else { return };
    apply_cfg(s, &body);
}
/// `key=number` lines over the settings.
fn apply_cfg(s: &mut State, body: &str) {
    for line in body.lines() {
        let line = line.trim();
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || !v.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
            continue;
        }
        if let (Some(key), Ok(value)) = (SPEC.iter().find(|e| e.0 == k).map(|e| e.0), v.parse::<f64>()) {
            s.cfg.insert(key, value);
        }
    }
}
fn hdr(r: f64, g: f64, b: f64) -> [f32; 4] {
    [r as f32, g as f32, b as f32, 1.0]
}
/// The mod's tintOf(prefix): the item's own colour triple.
fn tint_of(s: &State, pre: &str) -> [f32; 4] {
    let get = |c: &str, fallback: &str| s.cfg.get(format!("{pre}_{c}").as_str()).copied().unwrap_or_else(|| cfg(s, fallback));
    hdr(get("cr", "col_r"), get("cg", "col_g"), get("cb", "col_b"))
}
fn font(s: &State) -> (&'static std::ffi::CStr, &'static str) {
    if cfg(s, "thin") >= 0.5 {
        FONT_THIN
    } else {
        FONT_BOLD
    }
}
fn v2(w: &Handle, setter: &str, x: f64, y: f64) {
    let v = [x as f32, y as f32];
    if !w.is_null() {
        // SAFETY: the widget, held; the value lives for the call.
        unsafe { method_typed::<()>(w.instance, setter, &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) };
    }
}
fn set_tint(w: &Handle, c: [f32; 4]) {
    if !w.is_null() {
        // SAFETY: as above.
        unsafe { method_typed::<()>(w.instance, "SetTintColor", &["HDRColor"], &[Value::Ptr(c.as_ptr().cast())]) };
    }
}
fn set_text(w: &Handle, text: &str) {
    let Ok(c) = std::ffi::CString::new(text) else { return };
    let Some(cs) = CString::new(&c) else { return };
    m::<()>(w, "SetText", &[Value::Ptr((&cs as *const CString).cast())]);
}
fn visible(w: &Handle, on: bool) {
    m::<()>(w, "SetVisible", &[Value::Bool(on)]);
}
fn opacity(w: &Handle, a: f64) {
    m::<()>(w, "SetOpacity", &[Value::Float(a as f32)]);
}
fn set_font(w: &Handle, fam: (&std::ffi::CStr, &str)) {
    let Some(path) = CString::new(fam.0) else { return };
    m::<()>(w, "SetFontFamily", &[Value::Ptr((&path as *const CString).cast()), Value::U64(CName::hashed(fam.1).0)]);
}
/// The mod's place(): size, centred anchor, the anchor point and the margin.
fn place(w: &Handle, sx: f64, sy: f64, left: f64, top: f64, ax: f64, ay: f64) {
    v2(w, "SetSize", sx, sy);
    if let Some(c) = enum_value("inkEAnchor", "Centered") {
        m::<()>(w, "SetAnchor", &[Value::U64(c as u64)]);
    }
    v2(w, "SetAnchorPoint", ax, ay);
    let margin = [left as f32, top as f32, 0.0, 0.0];
    if !w.is_null() {
        // SAFETY: the widget, held; the margin lives for the call.
        unsafe { method_typed::<()>(w.instance, "SetMargin", &["inkMargin"], &[Value::Ptr(margin.as_ptr().cast())]) };
    }
}
fn new_widget(class: &str, root: &Handle) -> Handle {
    let Some(c) = crate::script_call::class_any(class) else { return Handle::null() };
    // SAFETY: a new instance owned by the handle made for it.
    let q = unsafe { Handle::from_instance(c.create_instance()) };
    m::<()>(&q, "Reparent", &[Value::Handle(root)]);
    visible(&q, false);
    q
}
fn mk_rect(root: &Handle) -> Handle {
    new_widget("inkRectangleWidget", root)
}
fn mk_text(s: &State, root: &Handle, align: &str) -> Handle {
    let q = new_widget("inkTextWidget", root);
    set_font(&q, font(s));
    if let Some(a) = enum_value("textHorizontalAlignment", align) {
        m::<()>(&q, "SetHorizontalAlignment", &[Value::U64(a as u64)]);
    }
    if let Some(a) = enum_value("textVerticalAlignment", "Center") {
        m::<()>(&q, "SetVerticalAlignment", &[Value::U64(a as u64)]);
    }
    // SAFETY: the text widget's Int32/Float properties.
    unsafe {
        if let Some(p) = crate::script_call::property(q.instance, "tracking") {
            *(p as *mut i32) = 0;
        }
        if let Some(p) = crate::script_call::property(q.instance, "scrollTextSpeed") {
            *(p as *mut f32) = 0.0;
        }
    }
    q
}
fn mk_image(root: &Handle) -> Handle {
    let q = new_widget("inkImageWidget", root);
    // White is "untinted": the buff icons keep their own colours.
    set_tint(&q, [1.0, 1.0, 1.0, 1.0]);
    q
}
fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
fn comp(name: &str) -> Handle {
    let p = player();
    h(&p, "FindComponentByName", &[Value::U64(CName::hashed(name).0)])
}
/// The mod's surface(): the HUD component's widget for its entry, else the world widget's.
fn surface() -> Handle {
    let ch = comp(HUD);
    if !ch.is_null() {
        let w = h(&ch, "GetWidget", &[Value::U64(CName::hashed(ENTRY).0)]);
        if !w.is_null() {
            return w;
        }
        let w = h(&ch, "GetWidget", &[]);
        if !w.is_null() {
            return w;
        }
    }
    h(&comp(UI), "GetWidget", &[])
}
fn name_of(w: &Handle) -> String {
    m::<u64>(w, "GetName", &[]).map(|n| if n == 0 { "None".to_string() } else { CName(n).text().to_string() }).unwrap_or_default()
}
/// The mod's sweep(): what an earlier run left behind (runtime children have no real name).
fn sweep(root: &Handle) -> i32 {
    let mut dropped = 0;
    for _ in 0..400 {
        let n = m::<i32>(root, "GetNumChildren", &[]).unwrap_or(0);
        let victim = (0..n).map(|i| h(root, "GetWidgetByIndex", &[Value::Int32(i)])).find(|k| {
            let nm = name_of(k);
            !k.is_null() && (nm.is_empty() || nm == "UNINITIALIZED_WIDGET" || nm == "None")
        });
        let Some(victim) = victim else { break };
        if m::<()>(root, "RemoveChild", &[Value::Handle(&victim)]).is_none() {
            break;
        }
        dropped += 1;
    }
    dropped
}
/// The mod's hideAuthored(): the authored placeholder parts hidden.
fn hide_authored(w: &Handle, depth: i32) {
    if depth > 6 {
        return;
    }
    // a leaf widget has no GetNumChildren: nothing below it, and no call made
    // SAFETY: the widget, held.
    if w.is_null() || unsafe { crate::script_call::find_function(w.instance, "GetNumChildren", 0) }.is_none() {
        return;
    }
    let Some(n) = m::<i32>(w, "GetNumChildren", &[]) else { return };
    for i in 0..n {
        let k = h(w, "GetWidgetByIndex", &[Value::Int32(i)]);
        if k.is_null() {
            continue;
        }
        let nm = name_of(&k);
        if nm == "price_widget" || nm == "bg" || nm == "hover_buy_button" {
            visible(&k, false);
            opacity(&k, 0.0);
        } else {
            hide_authored(&k, depth + 1);
        }
    }
}
fn build(s: &mut State) -> bool {
    let root = surface();
    if root.is_null() {
        s.note = "no readout surface".into();
        return false;
    }
    let dropped = sweep(&root);
    hide_authored(&root, 0);
    let mut w = Widgets { track: mk_rect(&root), fill: mk_rect(&root), over: mk_rect(&root), ..Default::default() };
    w.hp = mk_text(s, &root, "Right");
    w.lbl = mk_text(s, &root, "Left");
    w.ram = mk_text(s, &root, "Right");
    w.lvl = mk_text(s, &root, "Center");
    w.lvl_box = (0..4).map(|_| mk_rect(&root)).collect();
    set_text(&w.lbl, "HP");
    w.ticks = (0..MAX_TICKS).map(|_| mk_rect(&root)).collect();
    w.icons = (0..MAX_ICONS).map(|_| mk_image(&root)).collect();
    for _ in 0..MAX_STARS {
        let q = mk_image(&root);
        // The wanted bar's own atlas: the game's art.
        m::<()>(&q, "SetAtlasResource", &[Value::U64(red4ext_abi::resource::resource_path(STAR_ATLAS))]);
        m::<()>(&q, "SetTexturePart", &[Value::U64(CName::hashed(STAR_PART).0)]);
        w.stars.push(q);
    }
    visible(&root, true);
    opacity(&root, 1.0);
    s.child_mark = m::<i32>(&root, "GetNumChildren", &[]).unwrap_or(0);
    s.w = w;
    s.tick_n = 0;
    s.built = true;
    s.dirty = true;
    s.note = format!("built, {dropped} stale children removed");
    true
}
/// The mod's ours(): our children are still under the surface (a fresh instance has only the asset's few).
fn ours(s: &State) -> bool {
    s.built && {
        let root = surface();
        !root.is_null() && m::<i32>(&root, "GetNumChildren", &[]).unwrap_or(-1) >= s.child_mark
    }
}
fn row_width(s: &State, n: usize) -> f64 {
    if n == 0 {
        0.0
    } else {
        n as f64 * cfg(s, "tick_w") + (n as f64 - 1.0) * cfg(s, "tick_gap")
    }
}
fn place_ram(s: &State, n: usize) {
    let f = cfg(s, "ram_font");
    place(&s.w.ram, f * 7.0, f * 1.8, cfg(s, "org_x") + row_width(s, n) + cfg(s, "ram_x"), cfg(s, "org_y") + cfg(s, "ram_y"), 1.0, 0.5);
}
fn styled_text(s: &State, w: &Handle, key: &str) {
    set_font(w, font(s));
    m::<()>(w, "SetFontSize", &[Value::Int32((cfg(s, &format!("{key}_font")) + 0.5).floor() as i32)]);
    set_tint(w, tint_of(s, key));
    opacity(w, cfg(s, &format!("{key}_alpha")));
    visible(w, true);
}
fn layout(s: &mut State) {
    if !s.built {
        return;
    }
    let (ox, oy) = (cfg(s, "org_x"), cfg(s, "org_y"));
    let w = &s.w;
    place(&w.hp, cfg(s, "hp_font") * 3.2, cfg(s, "hp_font") * 1.4, ox + cfg(s, "hp_x"), oy + cfg(s, "hp_y"), 1.0, 0.5);
    styled_text(s, &w.hp, "hp");
    place(&w.lbl, cfg(s, "lbl_font") * 3.0, cfg(s, "lbl_font") * 1.8, ox + cfg(s, "lbl_x"), oy + cfg(s, "lbl_y"), 0.0, 0.5);
    styled_text(s, &w.lbl, "lbl");
    let (bw, bh, by) = (cfg(s, "bar_w"), cfg(s, "bar_h"), cfg(s, "bar_y"));
    place(&w.track, bw, bh, ox, oy + by, 0.0, 0.5);
    set_tint(&w.track, tint_of(s, "track"));
    opacity(&w.track, cfg(s, "track_alpha"));
    visible(&w.track, cfg(s, "track_alpha") > 0.01);
    place(&w.fill, bw, bh, ox, oy + by, 0.0, 0.5);
    set_tint(&w.fill, tint_of(s, "bar"));
    opacity(&w.fill, cfg(s, "bar_alpha"));
    visible(&w.fill, true);
    let oh = if cfg(s, "over_h") < 0.5 { bh } else { cfg(s, "over_h") };
    place(&w.over, bw, oh, ox, oy + by, 0.0, 0.5);
    set_tint(&w.over, tint_of(s, "over"));
    opacity(&w.over, s.cfg.get("over_alpha").copied().unwrap_or(0.95));
    set_font(&w.ram, font(s));
    m::<()>(&w.ram, "SetFontSize", &[Value::Int32((cfg(s, "ram_font") + 0.5).floor() as i32)]);
    set_tint(&w.ram, tint_of(s, "ram"));
    opacity(&w.ram, cfg(s, "ram_alpha"));
    visible(&w.ram, true);
    place_ram(s, s.tick_n);
    for (i, q) in w.ticks.iter().enumerate() {
        place(q, cfg(s, "tick_w"), cfg(s, "tick_h"), ox + i as f64 * (cfg(s, "tick_w") + cfg(s, "tick_gap")), oy + cfg(s, "tick_y"), 0.0, 0.5);
        set_tint(q, tint_of(s, "tick"));
    }
    // The whole readout, turned and sized in the widget's own space.
    let root = surface();
    if !root.is_null() {
        m::<()>(&root, "SetRotation", &[Value::Float(cfg(s, "spin") as f32)]);
        let sc = cfg(s, "scale").max(0.05);
        v2(&root, "SetScale", sc, sc);
    }
    // The level: a bare number centred in a square of four thin lines (lvl_x/lvl_y the square's centre).
    let (bx, byl) = (ox + cfg(s, "lvl_x"), oy + cfg(s, "lvl_y"));
    let (bs, be) = (cfg(s, "lvl_box"), cfg(s, "lvl_edge"));
    place(&w.lvl, bs.max(cfg(s, "lvl_font") * 2.5), bs.max(cfg(s, "lvl_font") * 1.6), bx, byl, 0.5, 0.5);
    styled_text(s, &w.lvl, "lvl");
    if let Some(a) = enum_value("textHorizontalAlignment", "Center") {
        m::<()>(&w.lvl, "SetHorizontalAlignment", &[Value::U64(a as u64)]);
    }
    if bs > 1.0 {
        place(&w.lvl_box[0], bs, be, bx - bs / 2.0, byl - bs / 2.0, 0.0, 0.5);
        place(&w.lvl_box[1], bs, be, bx - bs / 2.0, byl + bs / 2.0, 0.0, 0.5);
        place(&w.lvl_box[2], be, bs, bx - bs / 2.0, byl, 0.0, 0.5);
        place(&w.lvl_box[3], be, bs, bx + bs / 2.0 - be, byl, 0.0, 0.5);
    }
    for q in &w.lvl_box {
        set_tint(q, tint_of(s, "box"));
        opacity(q, cfg(s, "lvl_box_a"));
        visible(q, bs > 1.0 && cfg(s, "lvl_box_a") > 0.01);
    }
    for (i, q) in w.stars.iter().enumerate() {
        place(q, cfg(s, "star_size"), cfg(s, "star_size"), ox + cfg(s, "star_x") + i as f64 * cfg(s, "star_step"), oy + cfg(s, "star_y"), 0.0, 0.5);
    }
    let per_row = ((bw / cfg(s, "icon_step").max(1.0)).floor() as usize).max(1);
    for (i, q) in w.icons.iter().enumerate() {
        let (c0, r0) = (i % per_row, i / per_row);
        place(q, cfg(s, "icon_size"), cfg(s, "icon_size"), ox + c0 as f64 * cfg(s, "icon_step"), oy + cfg(s, "icon_y") + r0 as f64 * cfg(s, "icon_row"), 0.0, 0.5);
    }
    s.dirty = false;
}
fn stat(stats: &Handle, id: [u64; 2], ty: &str) -> Option<f64> {
    let v = enum_value("gamedataStatType", ty)?;
    m::<f32>(stats, "GetStatValue", &[Value::Ptr(id.as_ptr().cast()), Value::U64(v as u64)]).map(f64::from)
}
fn pool(pools: &Handle, id: [u64; 2], ty: &str) -> Option<f64> {
    let v = enum_value("gamedataStatPoolType", ty)?;
    m::<f32>(pools, "GetStatPoolValue", &[Value::Ptr(id.as_ptr().cast()), Value::U64(v as u64), Value::Bool(false)]).map(f64::from)
}
fn values(s: &mut State) {
    let p = player();
    if p.is_null() || !s.built {
        return;
    }
    let id = [m::<u64>(&p, "GetEntityID", &[]).unwrap_or(0), 0];
    let (pools, stats) = (super::game_system("GetStatPoolsSystem"), super::game_system("GetStatsSystem"));
    let hp = pool(&pools, id, "Health").unwrap_or(0.0);
    let hp_max = stat(&stats, id, "Health").filter(|v| *v > 0.0).unwrap_or(1.0);
    let ram = pool(&pools, id, "Memory");
    let ram_max = stat(&stats, id, "Memory");
    let over = pool(&pools, id, "Overshield").filter(|v| *v >= 0.0).unwrap_or(0.0);
    let frac = (hp / hp_max).clamp(0.0, 1.0);
    let ofrac = (over / hp_max).clamp(0.0, 1.0);
    // The number is health plus overshield, reading above the maximum while the gold is up.
    set_text(&s.w.hp, &format!("{}", (hp + over + 0.5).floor() as i64));
    let (bw, bh) = (cfg(s, "bar_w"), cfg(s, "bar_h"));
    v2(&s.w.fill, "SetSize", (bw * frac).max(0.0), bh);
    let oh = if cfg(s, "over_h") < 0.5 { bh } else { cfg(s, "over_h") };
    v2(&s.w.over, "SetSize", (bw * ofrac).max(0.0), oh);
    visible(&s.w.over, ofrac > 0.001);
    let n = ((ram_max.or(ram).unwrap_or(0.0) + 0.5).floor().max(0.0) as usize).min(MAX_TICKS);
    let used = (ram.unwrap_or(0.0) + 0.5).floor() as i64;
    for (i, q) in s.w.ticks.iter().enumerate() {
        if i < n {
            opacity(q, if (i as i64) < used { cfg(s, "tick_alpha") } else { cfg(s, "tick_empty") });
            visible(q, true);
        } else {
            visible(q, false);
        }
    }
    if n != s.tick_n {
        s.tick_n = n;
        place_ram(s, n);
    }
    set_text(&s.w.ram, &format!("RAM {used}/{n}"));
    let lvl = stat(&stats, id, "Level").unwrap_or(0.0);
    set_text(&s.w.lvl, &format!("{}", (lvl + 0.5).floor() as i64));
    // The heat, off the field the wanted bar listens to.
    let heat = (|| -> R<i32> {
        let defs = super::boards::defs()?;
        let d = super::boards::definition(&defs, "UI_WantedBar")?;
        let board = super::boards::system_board(d)?;
        if board.is_null() {
            return Ok(0);
        }
        super::boards::get::<i32>(&board, "GetInt", super::boards::field(d, "CurrentWantedLevel")?)
    })()
    .unwrap_or(0)
    .max(0) as usize;
    for (i, q) in s.w.stars.iter().enumerate() {
        set_tint(q, tint_of(s, "star"));
        if i < heat {
            opacity(q, cfg(s, "star_alpha"));
            visible(q, true);
        } else if cfg(s, "star_empty") > 0.01 {
            opacity(q, cfg(s, "star_empty"));
            visible(q, true);
        } else {
            visible(q, false);
        }
    }
}
/// A TweakDB flat's 8-byte value (a ResRef, a CName) by its name.
fn flat_u64(name: &str) -> Option<u64> {
    super::weapon::flat_u64(name)
}
/// The HUD's own chain: an applied effect's record -> UiData():IconPath() -> the UIIcon record's atlas and part.
fn icons(s: &mut State) {
    let p = player();
    if p.is_null() || !s.built {
        return;
    }
    let mut want: Vec<(u64, u64)> = Vec::new();
    let system = super::game_system("GetStatusEffectSystem");
    let id = m::<u64>(&p, "GetEntityID", &[]).unwrap_or(0);
    // SAFETY: the system, held; the array result owned while read.
    if let Some(list) = (!system.is_null()).then(|| unsafe { method_owned(system.instance, "GetAppliedEffects", &[Value::U64(id)]) }).flatten() {
        // SAFETY: a DynArray of handles: entries, capacity, size.
        let (entries, size) = unsafe { (*(list.as_ptr() as *const *const Handle), *(list.as_ptr().add(0x0C) as *const u32)) };
        for i in 0..size as usize {
            if want.len() >= MAX_ICONS {
                break;
            }
            // SAFETY: the array's own element.
            let effect = unsafe { (*entries.add(i)).clone() };
            let rec = h(&effect, "GetRecord", &[]);
            let ui = h(&rec, "UiData", &[]);
            if ui.is_null() {
                continue;
            }
            // SAFETY: the String result owned while read.
            let path = unsafe { method_owned(ui.instance, "IconPath", &[]) }
                .map(|o| unsafe { (*(o.as_ptr() as *const CString)).as_str().to_string() })
                .unwrap_or_default();
            if path.is_empty() || path == "nil" {
                continue;
            }
            if let (Some(atlas), Some(part)) = (flat_u64(&format!("UIIcon.{path}.atlasResourcePath")), flat_u64(&format!("UIIcon.{path}.atlasPartName"))) {
                want.push((atlas, part));
            }
        }
    }
    for (i, q) in s.w.icons.iter().enumerate() {
        if let Some(&(atlas, part)) = want.get(i) {
            m::<()>(q, "SetAtlasResource", &[Value::U64(atlas)]);
            m::<()>(q, "SetTexturePart", &[Value::U64(part)]);
            set_tint(q, tint_of(s, "icon"));
            opacity(q, 1.0);
            visible(q, true);
        } else {
            visible(q, false);
        }
    }
}
fn q_axis(ax: [f64; 3], deg: f64) -> [f64; 4] {
    let a = deg.to_radians() * 0.5;
    let sn = a.sin();
    [ax[0] * sn, ax[1] * sn, ax[2] * sn, a.cos()]
}
fn q_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn q_rot(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let (x, y, z, w) = (q[0], q[1], q[2], q[3]);
    let (tx, ty, tz) = (2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0]));
    [v[0] + w * tx + (y * tz - z * ty), v[1] + w * ty + (z * tx - x * tz), v[2] + w * tz + (x * ty - y * tx)]
}
/// The mod's transform(): the band's component transform from the baking knobs.
fn transform(s: &State) {
    let cs = comp(SCREEN);
    if cs.is_null() {
        return;
    }
    let around = [
        ALONG[1] * INWARD[2] - ALONG[2] * INWARD[1],
        ALONG[2] * INWARD[0] - ALONG[0] * INWARD[2],
        ALONG[0] * INWARD[1] - ALONG[1] * INWARD[0],
    ];
    let out = [-INWARD[0], -INWARD[1], -INWARD[2]];
    let q = q_mul(q_mul(q_axis(out, cfg(s, "yaw")), q_axis(around, cfg(s, "pitch"))), q_axis(ALONG, cfg(s, "roll")));
    let mut pos = [cfg(s, "plate_x"), cfg(s, "plate_y"), cfg(s, "plate_z")];
    for i in 0..3 {
        pos[i] += (ALONG[i] * cfg(s, "arm_along") + INWARD[i] * cfg(s, "arm_into") + around[i] * cfg(s, "arm_around")) * 0.001;
    }
    let rc = q_rot(q, PIVOT);
    for i in 0..3 {
        pos[i] += PIVOT[i] - rc[i];
    }
    let p4 = [pos[0] as f32, pos[1] as f32, pos[2] as f32, 1.0];
    let q4 = q.map(|v| v as f32);
    m::<()>(&cs, "SetLocalPosition", &[Value::Ptr(p4.as_ptr().cast())]);
    m::<()>(&cs, "SetLocalOrientation", &[Value::Ptr(q4.as_ptr().cast())]);
}
/// The mod's unglitch(): the fallback world widget's glitch zeroed.
fn unglitch() {
    let cu = comp(UI);
    if cu.is_null() {
        return;
    }
    // SAFETY: the world widget's Float property.
    if let Some(p) = unsafe { crate::script_call::property(cu.instance, "glitchValue") } {
        unsafe {
            if *(p as *const f32) != 0.0 {
                *(p as *mut f32) = 0.0;
            }
        }
    }
}

/// The mod's onUpdate.
pub(in crate::mechanics) fn tick(dt: f32) {
    let dt = f64::from(dt);
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(|| {
        let mut s = State {
            cfg: HashMap::new(),
            built: false,
            child_mark: 0,
            w: Widgets::default(),
            tick_n: 0,
            dirty: true,
            acc_fast: 0.0,
            acc_slow: 0.0,
            acc_check: 0.0,
            acc_ours: 0.0,
            acc_watch: 0.0,
            rearmed: false,
            guard: None,
            note: "waiting for the player".into(),
            reported: false,
        };
        load_cfg(&mut s);
        s
    });
    if player().is_null() {
        // A load screen: the components start over and so does the content; the re-arm is owed again.
        s.built = false;
        s.rearmed = false;
        return;
    }
    // wrist.cfg is read at load and at the window's "reload the file", as the mod's loadCfg was: the window's sliders
    // change this config itself.
    if s.guard.is_none() {
        // SAFETY: the RTTI's global function lookup.
        s.guard = Some(red4ext_abi::rtti::RttiSystem::get().and_then(|r| r.get_function(CName::hashed("VRWristGuard"))).is_some());
    }
    if s.built && s.guard == Some(true) {
        // Before the freeze every frame (the transition is one frame wide), after it four times a second.
        let mut due = true;
        if s.rearmed {
            s.acc_watch += dt;
            due = s.acc_watch >= 0.25;
            if due {
                s.acc_watch = 0.0;
            }
        }
        if due {
            // SAFETY: the plugin's own native.
            match unsafe { call_native::<i32>("VRWristGuard", &[Value::Int32(5)]) } {
                None => s.guard = Some(false),
                Some(1) => {
                    s.rearmed = true;
                    s.note = "caught the transition -> frozen and re-armed".into();
                }
                Some(2) => s.rearmed = true,
                Some(3) => s.note = "the target died again -> re-armed".into(),
                Some(_) => {}
            }
        }
    }
    s.acc_ours += dt;
    if !s.built || s.acc_ours >= 0.25 {
        s.acc_ours = 0.0;
        if !ours(s) {
            s.built = false;
            build(s);
        }
        // Hands only: the forearm is gone, so the content goes (a live target with nothing in it draws nothing).
        if s.built {
            // SAFETY: the shared block's read.
            let blank = unsafe { cvr_rig::vrik::provider::shared_slot(173) } > 0.5;
            opacity(&surface(), if blank { 0.0 } else { 1.0 });
        }
    }
    s.acc_check += dt;
    if s.acc_check >= 0.5 {
        s.acc_check = 0.0;
        unglitch();
        transform(s);
    }
    if !s.built {
        return;
    }
    if s.dirty {
        layout(s);
        values(s);
        icons(s);
        transform(s);
    }
    s.acc_fast += dt;
    if s.acc_fast >= 0.12 {
        s.acc_fast = 0.0;
        values(s);
    }
    s.acc_slow += dt;
    if s.acc_slow >= 0.5 {
        s.acc_slow = 0.0;
        icons(s);
    }
    if !s.reported {
        s.reported = true;
        log!("[mechanics] wrist hud: {} from Rust (children {}, ticks {})\n", s.note, s.child_mark, s.tick_n);
    }
}

/// wrist.cfg as the mod writes it: a header, each group as a comment, `key=value` with four decimals.
fn save_cfg(s: &State) -> bool {
    use super::wrist_hud_ui::{Row, ROWS};
    let mut text = String::from("-- CyberpunkVRPort wrist readout. Written by the CET overlay: VR wrist readout -> SAVE.\n");
    for row in ROWS.iter() {
        match row {
            Row::Group(g) => text.push_str(&format!("\n-- {g}\n")),
            Row::Slider { key, .. } | Row::Check { key, .. } => text.push_str(&format!("{key}={:.4}\n", cfg(s, key))),
        }
    }
    std::fs::write(cfg_path(), text).is_ok()
}
/// The mod's "VR wrist readout" window (dev_windows.rs): its settings applied as they move, saved to wrist.cfg, and
/// its repairs (the main thread's tick does them).
pub(super) fn dev_window(frame: [f32; 2]) {
    use super::dev_windows as w;
    use super::wrist_hud_ui::{Row, ROWS};
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    w::window("VR wrist readout", [0.01, 0.5], [0.19, 0.45], frame, || {
        let Some(s) = guard.as_mut() else {
            w::text("waiting for the player");
            return;
        };
        w::text(&if s.built { format!("on the arm -- {}", s.note) } else { format!("NOT BUILT -- {}", s.note) });
        w::text(&format!("frozen + re-armed: {}", if s.rearmed { "yes" } else { "not yet (needs a weapon in hand)" }));
        w::text(if s.guard == Some(true) { "detector: plugin" } else { "transition detector: not in the plugin -- using the backstop timer" });
        w::text("Everything applies as you drag. SAVE writes wrist.cfg.");
        w::text("Spin and size are widget space and exact. The band group is entity space: it shows a");
        w::text("number but pulls away as the arm turns -- SAVE it and ask for a bake into the mesh.");
        for row in ROWS.iter() {
            match *row {
                Row::Group(g) => {
                    w::separator();
                    w::text(g);
                }
                Row::Check { key, label } => {
                    let mut b = cfg(s, key) >= 0.5;
                    if w::checkbox(&format!("{label}##{key}"), &mut b) {
                        s.cfg.insert(key, if b { 1.0 } else { 0.0 });
                        s.dirty = true;
                    }
                }
                Row::Slider { key, label, lo, hi, int } => {
                    let mut v = cfg(s, key) as f32;
                    if w::slider(&format!("{label}##{key}"), &mut v, lo, hi) {
                        s.cfg.insert(key, f64::from(if int { v.round() } else { v }));
                        s.dirty = true;
                    }
                }
            }
        }
        w::separator();
        if w::button("SAVE") {
            s.note = String::from(if save_cfg(s) { "saved to wrist.cfg" } else { "could not write wrist.cfg" });
        }
        w::same_line();
        if w::button("reload the file") {
            load_cfg(s);
            s.dirty = true;
            s.note = "re-read wrist.cfg".into();
        }
        w::same_line();
        if w::button("defaults") {
            s.cfg = SPEC.iter().map(|&(k, d)| (k, d)).collect();
            s.dirty = true;
            s.note = "back to the built-in numbers".into();
        }
        w::same_line();
        if w::button("repair the target now") {
            s.built = false;
            s.note = "repaired by hand".into();
        }
        w::same_line();
        if w::button("re-arm again") {
            s.rearmed = false;
            s.guard = None;
            // SAFETY: the plugin's own native: the freeze owed again.
            unsafe { call_native::<i32>("VRWristGuard", &[Value::Int32(6)]) };
            s.note = "will freeze and re-arm on the next weapon drawn".into();
        }
        w::same_line();
        if w::button("rebuild") {
            s.built = false;
        }
    });
}
