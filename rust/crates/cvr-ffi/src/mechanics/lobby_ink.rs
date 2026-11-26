//! mods/cet/CyberpunkVRPort_Lobby/modules/ink.lua (and the same kit of the VR menu, CyberpunkVRPort_Menu/modules/ink.lua):
//! game ink widgets placed in canvas pixels, top-left anchored -- canvases, rectangles, texts in the language's font,
//! atlas images, lines turned about their start, rings and arcs of lines, the speaker glyph, the cut-corner frame.
use crate::script_call::{call_handle, enum_value, method, method_typed, Value};
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;

pub(super) type C3 = [f64; 3];
pub(super) const FONT: &str = "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily";
const FOREIGN: &str = "base\\gameplay\\gui\\fonts\\foreign\\";
const BLENDER: &str = "base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily";

pub(super) fn call(w: &Handle, name: &str, values: &[Value]) {
    if !w.is_null() {
        // SAFETY: the widget, held.
        unsafe { method::<()>(w.instance, name, values) };
    }
}
pub(super) fn get<T: crate::script_call::Ret>(w: &Handle, name: &str, values: &[Value]) -> Option<T> {
    if w.is_null() {
        return None;
    }
    // SAFETY: the widget, held.
    unsafe { method::<T>(w.instance, name, values) }
}
pub(super) fn child(w: &Handle, name: &str, values: &[Value]) -> Handle {
    if w.is_null() {
        return Handle::null();
    }
    // SAFETY: the object, held.
    unsafe { call_handle(w.instance, name, values) }.unwrap_or_else(Handle::null)
}
fn typed(w: &Handle, name: &str, ty: &str, p: *const core::ffi::c_void) {
    if !w.is_null() {
        // SAFETY: the widget, held; the value lives for the call.
        unsafe { method_typed::<()>(w.instance, name, &[ty], &[Value::Ptr(p)]) };
    }
}
pub(super) fn v2(w: &Handle, setter: &str, x: f64, y: f64) {
    let v = [x as f32, y as f32];
    typed(w, setter, "Vector2", v.as_ptr().cast());
}
/// K.move: the margin's left and top.
pub(super) fn move_to(w: &Handle, x: f64, y: f64) {
    let m = [x as f32, y as f32, 0.0, 0.0];
    typed(w, "SetMargin", "inkMargin", m.as_ptr().cast());
}
/// The margin, all four sides.
pub(super) fn margin(w: &Handle, left: f64, top: f64, right: f64, bottom: f64) {
    let m = [left as f32, top as f32, right as f32, bottom as f32];
    typed(w, "SetMargin", "inkMargin", m.as_ptr().cast());
}
pub(super) fn size(w: &Handle, x: f64, y: f64) {
    v2(w, "SetSize", x, y);
}
pub(super) fn color(c: C3, a: f64) -> [f32; 4] {
    [c[0] as f32, c[1] as f32, c[2] as f32, a as f32]
}
pub(super) fn tint(w: &Handle, c: C3) {
    let v = color(c, 1.0);
    typed(w, "SetTintColor", "HDRColor", v.as_ptr().cast());
}
pub(super) fn opacity(w: &Handle, a: f64) {
    call(w, "SetOpacity", &[Value::Float(a as f32)]);
}
pub(super) fn visible(w: &Handle, on: bool) {
    call(w, "SetVisible", &[Value::Bool(on)]);
}
pub(super) fn is_visible(w: &Handle) -> bool {
    get::<bool>(w, "IsVisible", &[]) == Some(true)
}
pub(super) fn reparent(w: &Handle, parent: &Handle) {
    call(w, "Reparent", &[Value::Handle(parent)]);
}
pub(super) fn remove_all(w: &Handle) {
    call(w, "RemoveAllChildren", &[]);
}
pub(super) fn rotation(w: &Handle, deg: f64) {
    call(w, "SetRotation", &[Value::Float(deg as f32)]);
}
pub(super) fn set_text(w: &Handle, text: &str) {
    let Ok(c) = std::ffi::CString::new(text.replace('\0', "")) else { return };
    let Some(cs) = CString::new(&c) else { return };
    call(w, "SetText", &[Value::Ptr((&cs as *const CString).cast())]);
}
fn anchor_top_left(w: &Handle) {
    if let Some(a) = enum_value("inkEAnchor", "TopLeft") {
        call(w, "SetAnchor", &[Value::U64(a as u64)]);
    }
}
/// A new widget of that class (CET's `inkCanvas.new()`).
pub(super) fn new(class: &str) -> Handle {
    let Some(c) = crate::script_call::class_any(class) else { return Handle::null() };
    // SAFETY: a new instance owned by the handle made for it.
    unsafe { Handle::from_instance(c.create_instance()) }
}
/// M.place: anchored top-left at (x, y) of the parent, its size, reparented.
pub(super) fn place(w: Handle, parent: &Handle, x: f64, y: f64, sx: f64, sy: f64) -> Handle {
    anchor_top_left(&w);
    v2(&w, "SetAnchorPoint", 0.0, 0.0);
    move_to(&w, x, y);
    size(&w, sx, sy);
    reparent(&w, parent);
    w
}
pub(super) fn canvas(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64) -> Handle {
    place(new("inkCanvas"), parent, x, y, sx, sy)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn rect(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, col: C3, op: f64) -> Handle {
    let r = place(new("inkRectangleWidget"), parent, x, y, sx, sy);
    tint(&r, col);
    opacity(&r, op);
    r
}
#[allow(clippy::too_many_arguments)]
pub(super) fn cut_frame(parent: &Handle, x: f64, y: f64, w: f64, h: f64, cut: f64, col: C3, op: f64, width: f64) -> Vec<Handle> {
    let pts = [[x + cut, y], [x + w, y], [x + w, y + h - cut], [x + w - cut, y + h], [x, y + h], [x, y + cut]];
    (0..pts.len()).map(|i| line(parent, pts[i], pts[(i + 1) % pts.len()], col, op, width)).collect()
}
static LANG: std::sync::Mutex<(String, Option<std::time::Instant>)> = std::sync::Mutex::new((String::new(), None));
/// The on-screen language (/language OnScreen), read at most once a second.
pub(super) fn language() -> String {
    let mut g = LANG.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(t) = g.1 {
        if t.elapsed().as_secs_f64() < 1.0 && !g.0.is_empty() {
            return g.0.clone();
        }
    }
    g.1 = Some(std::time::Instant::now());
    g.0 = read_language().unwrap_or_else(|| if g.0.is_empty() { "en-us".into() } else { g.0.clone() });
    g.0.clone()
}
/// A settings variable's object (/group, name), when the group has it.
pub(super) fn setting_var(group: &str, name: &str) -> Handle {
    let ss = super::game_system("GetSettingsSystem");
    if ss.is_null() {
        return Handle::null();
    }
    let g = CName::hashed(group).0;
    if get::<bool>(&ss, "HasGroup", &[Value::U64(g)]) != Some(true) {
        return Handle::null();
    }
    let grp = child(&ss, "GetGroup", &[Value::U64(g)]);
    if get::<bool>(&grp, "HasVar", &[Value::U64(CName::hashed(name).0)]) != Some(true) {
        return Handle::null();
    }
    child(&ss, "GetVar", &[Value::U64(g), Value::U64(CName::hashed(name).0)])
}
fn read_language() -> Option<String> {
    let v = setting_var("/language", "OnScreen");
    let n = get::<u64>(&v, "GetValue", &[])?;
    let s = CName(n).text().to_string();
    (!s.is_empty()).then_some(s)
}
fn font_of(lang: &str) -> Option<(String, &'static str)> {
    Some(match lang {
        "de-de" | "fr-fr" | "es-es" | "es-mx" | "it-it" | "pt-br" | "pl-pl" | "cz-cz" | "hu-hu" | "tr-tr" | "ua-ua" => (BLENDER.into(), "Bold"),
        "jp-jp" => (format!("{FOREIGN}japanese\\mgenplus\\mgenplus.inkfontfamily"), "Medium"),
        "kr-kr" => (format!("{FOREIGN}korean\\kbiz_go\\kbiz_go.inkfontfamily"), "Medium"),
        "zh-cn" => (format!("{FOREIGN}chinese\\jing_xi_heig\\jing_xi_heig.inkfontfamily"), "Semi-Bold"),
        "zh-tw" => (format!("{FOREIGN}chinese_traditional\\jing_xi_heig_b5\\jing_xi_heig_b5.inkfontfamily"), "Semi-Bold"),
        "th-th" => (format!("{FOREIGN}thai\\th_sarabun_new\\th_sarabun_new.inkfontfamily"), "Semi-Bold"),
        "ar-ar" => (format!("{FOREIGN}arabic\\ara_es_nawar\\ara_es_nawar.inkfontfamily"), "Semi-Bold"),
        _ => return None,
    })
}
pub(super) fn set_font(w: &Handle, family: &str, style: &str) {
    let Ok(c) = std::ffi::CString::new(family) else { return };
    let Some(path) = CString::new(&c) else { return };
    call(w, "SetFontFamily", &[Value::Ptr((&path as *const CString).cast()), Value::U64(CName::hashed(style).0)]);
}
fn set_enum(w: &Handle, setter: &str, en: &str, member: &str) {
    if let Some(v) = enum_value(en, member) {
        call(w, setter, &[Value::U64(v as u64)]);
    }
}
/// M.text: a text centred vertically in (x, y, w, h), aligned left / centre / right, upper case unless plain.
#[allow(clippy::too_many_arguments)]
pub(super) fn text(parent: &Handle, x: f64, y: f64, w: f64, h: f64, s: &str, sz: f64, col: C3, align: &str, plain: bool, style: Option<&str>) -> Handle {
    let t = new("inkTextWidget");
    let ax = match align {
        "centre" => 0.5,
        "right" => 1.0,
        _ => 0.0,
    };
    anchor_top_left(&t);
    v2(&t, "SetAnchorPoint", ax, 0.5);
    move_to(&t, x + w * ax, y + h / 2.0);
    reparent(&t, parent);
    match font_of(&language()) {
        Some((f, st)) => set_font(&t, &f, st),
        None => set_font(&t, FONT, style.unwrap_or("Semi-Bold")),
    }
    call(&t, "SetFontSize", &[Value::Int32(sz as i32)]);
    if !plain {
        set_enum(&t, "SetLetterCase", "textLetterCase", "UpperCase");
    }
    set_enum(
        &t,
        "SetHorizontalAlignment",
        "textHorizontalAlignment",
        match align {
            "centre" => "Center",
            "right" => "Right",
            _ => "Left",
        },
    );
    set_enum(&t, "SetVerticalAlignment", "textVerticalAlignment", "Center");
    set_text(&t, s);
    tint(&t, col);
    t
}
pub(super) fn set_atlas(w: &Handle, atlas: &str, part: &str) {
    call(w, "SetAtlasResource", &[Value::U64(red4ext_abi::resource::resource_path(atlas))]);
    call(w, "SetTexturePart", &[Value::U64(CName::hashed(part).0)]);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn image(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, atlas: &str, part: &str, col: C3, op: f64) -> Handle {
    let q = place(new("inkImageWidget"), parent, x, y, sx, sy);
    set_atlas(&q, atlas, part);
    tint(&q, col);
    opacity(&q, op);
    q
}
/// M.line: a rectangle from a to b, turned about its start.
pub(super) fn line(parent: &Handle, a: [f64; 2], b: [f64; 2], col: C3, op: f64, width: f64) -> Handle {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let r = rect(parent, a[0], a[1], (dx * dx + dy * dy).sqrt(), width, col, op);
    v2(&r, "SetRenderTransformPivot", 0.0, 0.0);
    rotation(&r, dy.atan2(dx).to_degrees());
    r
}
#[allow(clippy::too_many_arguments)]
pub(super) fn ring(parent: &Handle, cx: f64, cy: f64, r: f64, width: f64, col: C3, op: f64, segments: usize, gaps: Option<usize>) -> Vec<Handle> {
    let mut out = Vec::new();
    for i in 0..segments {
        if gaps.is_none_or(|g| i % g != g - 1) {
            let a = i as f64 * 2.0 * std::f64::consts::PI / segments as f64;
            let b = (i + 1) as f64 * 2.0 * std::f64::consts::PI / segments as f64;
            out.push(line(parent, [cx + a.cos() * r, cy + a.sin() * r], [cx + b.cos() * r, cy + b.sin() * r], col, op, width));
        }
    }
    out
}
#[allow(clippy::too_many_arguments)]
pub(super) fn arc(parent: &Handle, cx: f64, cy: f64, r: f64, a: f64, b: f64, width: f64, col: C3, op: f64, segments: usize) -> Vec<Handle> {
    (0..segments)
        .map(|i| {
            let p = a + (b - a) * i as f64 / segments as f64;
            let q = a + (b - a) * (i + 1) as f64 / segments as f64;
            line(parent, [cx + p.cos() * r, cy + p.sin() * r], [cx + q.cos() * r, cy + q.sin() * r], col, op, width)
        })
        .collect()
}
/// M.speaker: the loudspeaker glyph with `waves` arcs.
pub(super) fn speaker(parent: &Handle, x: f64, y: f64, sz: f64, waves: usize, col: C3, op: f64) -> Vec<Handle> {
    let s = sz / 100.0;
    let mut out = vec![rect(parent, x + 6.0 * s, y + 37.0 * s, 18.0 * s, 26.0 * s, col, op)];
    let (steps, x0, w) = (12.0, x + 24.0 * s, 22.0 * s);
    for i in 0..12 {
        let h = (26.0 + (f64::from(i) + 1.0) / steps * 44.0) * s;
        out.push(rect(parent, x0 + f64::from(i) * w / steps, y + 50.0 * s - h / 2.0, w / steps + 1.0, h, col, op));
    }
    for k in 1..=waves {
        out.extend(arc(parent, x + 48.0 * s, y + 50.0 * s, (12.0 + 11.0 * k as f64) * s, -0.75, 0.75, 6.0 * s, col, op, 8));
    }
    out
}
/// M.fit: the font size that fits the text's characters into the width.
pub(super) fn fit(s: &str, width: f64, sz: f64, least: f64) -> f64 {
    let n = s.chars().count();
    if n == 0 {
        return sz;
    }
    least.max(sz.min((width / (n as f64 * 0.55)).floor()))
}
pub(super) fn diamond(parent: &Handle, x: f64, y: f64, sz: f64, col: C3, op: f64) -> Handle {
    let r = rect(parent, x - sz / 2.0, y - sz / 2.0, sz, sz, col, op);
    v2(&r, "SetRenderTransformPivot", 0.5, 0.5);
    rotation(&r, 45.0);
    r
}
#[allow(clippy::too_many_arguments)]
pub(super) fn outline(parent: &Handle, x: f64, y: f64, w: f64, h: f64, col: C3, op: f64, t: f64) {
    rect(parent, x, y, w, t, col, op);
    rect(parent, x, y + h - t, w, t, col, op);
    rect(parent, x, y, t, h, col, op);
    rect(parent, x + w - t, y, t, h, col, op);
}
/// An image widget's photo (a save's screenshot) set into a parent at (x, y, w, h), tinted.
#[allow(clippy::too_many_arguments)]
pub(super) fn adopt(img: &Handle, parent: &Handle, x: f64, y: f64, w: f64, h: f64, col: C3) {
    anchor_top_left(img);
    v2(img, "SetAnchorPoint", 0.0, 0.0);
    move_to(img, x, y);
    size(img, w, h);
    tint(img, col);
    opacity(img, 1.0);
    visible(img, true);
    reparent(img, parent);
}
