//! CyberpunkVRPort_Menu/modules/ink.lua: the VR menu's drawing kit -- game ink widgets in canvas pixels (the lobby's
//! kit, lobby_ink.rs, places them), the menu's colours and atlases, texts in the menu language's font, nine-sliced
//! rounded fills, cut-corner frames, the holo panel's look, a pool of drawn canvases kept between rebuilds (one panel
//! rebuild would otherwise make ~95 widgets), upper case for the Latin and Cyrillic letters, and the pointer beam.
use super::lobby_ink::{self as ink, C3};
use crate::script_call::{enum_value, Value};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use std::collections::HashMap;

pub(super) const FONT: &str = "base\\gameplay\\gui\\fonts\\raj\\raj.inkfontfamily";
pub(super) const FONT_THIN: &str = "base\\gameplay\\gui\\fonts\\blender\\blender.inkfontfamily";
const FOREIGN: &str = "base\\gameplay\\gui\\fonts\\foreign\\";
pub(super) const RED: C3 = [1.0, 0.30, 0.26];
pub(super) const CYAN: C3 = [0.43, 0.86, 1.0];
pub(super) const YELLOW: C3 = [1.0, 0.69, 0.16];
pub(super) const WHITE: C3 = [0.94, 0.98, 1.0];
pub(super) const GREY: C3 = [0.43, 0.62, 0.71];
pub(super) const BLACK: C3 = [0.0, 0.0, 0.0];
pub(super) const TEXT: C3 = [0.83, 0.94, 1.0];
pub(super) const ICE: C3 = [0.42, 0.72, 0.84];
pub(super) const NAVY: C3 = [0.012, 0.012, 0.012];
pub(super) const DARKRED: C3 = [0.30, 0.03, 0.04];
pub(super) const DARKCYAN: C3 = [0.02, 0.22, 0.26];
pub(super) const DARKYELLOW: C3 = [0.24, 0.21, 0.0];
pub(super) fn quality(q: &str) -> C3 {
    match q {
        "Uncommon" => [0.25, 0.85, 0.53],
        "Rare" => [0.25, 0.65, 1.0],
        "Epic" => [0.75, 0.3, 1.0],
        "Legendary" => [1.0, 0.62, 0.16],
        _ => GREY,
    }
}
pub(super) const ATLAS_PHONE: &str = "base\\gameplay\\gui\\widgets\\phone\\new_phone_assets.inkatlas";
pub(super) const ATLAS_PHONE_OLD: &str = "base\\gameplay\\gui\\widgets\\phone\\atlas_phone.inkatlas";
pub(super) const ATLAS_MAPPIN: &str = "base\\gameplay\\gui\\common\\icons\\mappin_icons.inkatlas";
pub(super) const ATLAS_DPAD: &str = "base\\gameplay\\gui\\widgets\\dpad_hint\\dpad_hint.inkatlas";
pub(super) const ATLAS_SHAPES: &str = "base\\gameplay\\gui\\common\\shapes\\atlas_shapes_sync.inkatlas";
pub(super) const ATLAS_COMMON: &str = "base\\gameplay\\gui\\common\\icons\\atlas_common.inkatlas";

/// The foreign font of the menu's language: (family, bold style, thin style).
pub(super) fn foreign(code: &str) -> Option<(String, &'static str, &'static str)> {
    Some(match code {
        "de-de" | "fr-fr" | "es-es" | "es-mx" | "it-it" | "pt-br" | "pl-pl" | "cz-cz" | "hu-hu" | "tr-tr" | "ua-ua" => (FONT_THIN.into(), "Bold", "Book"),
        "jp-jp" => (format!("{FOREIGN}japanese\\mgenplus\\mgenplus.inkfontfamily"), "Medium", "Regular"),
        "kr-kr" => (format!("{FOREIGN}korean\\kbiz_go\\kbiz_go.inkfontfamily"), "Medium", "Regular"),
        "zh-cn" => (format!("{FOREIGN}chinese\\jing_xi_heig\\jing_xi_heig.inkfontfamily"), "Semi-Bold", "Regular"),
        "zh-tw" => (format!("{FOREIGN}chinese_traditional\\jing_xi_heig_b5\\jing_xi_heig_b5.inkfontfamily"), "Semi-Bold", "Medium"),
        "th-th" => (format!("{FOREIGN}thai\\th_sarabun_new\\th_sarabun_new.inkfontfamily"), "Semi-Bold", "Regular"),
        "ar-ar" => (format!("{FOREIGN}arabic\\ara_es_nawar\\ara_es_nawar.inkfontfamily"), "Semi-Bold", "Regular"),
        _ => return None,
    })
}
pub(super) fn rect(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, col: C3, op: f64) -> Handle {
    ink::rect(parent, x, y, sx, sy, col, op)
}
pub(super) fn line(parent: &Handle, x1: f64, y1: f64, x2: f64, y2: f64, col: C3, op: f64, width: f64) -> Handle {
    ink::line(parent, [x1, y1], [x2, y2], col, op, width)
}
pub(super) fn tint(list: &[Handle], col: C3, op: Option<f64>) {
    for w in list {
        ink::tint(w, col);
        if let Some(o) = op {
            ink::opacity(w, o);
        }
    }
}
/// K.frame: four rectangles round (x, y, sx, sy), `t` thick.
pub(super) fn frame(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, t: f64, col: C3, op: f64) -> Vec<Handle> {
    vec![rect(parent, x, y, sx, t, col, op), rect(parent, x, y + sy - t, sx, t, col, op), rect(parent, x, y + t, t, sy - 2.0 * t, col, op), rect(parent, x + sx - t, y + t, t, sy - 2.0 * t, col, op)]
}
pub(super) fn slash(parent: &Handle, cx: f64, cy: f64, len: f64, t: f64, col: C3, op: f64) -> Handle {
    let r = rect(parent, cx - len / 2.0, cy - t / 2.0, len, t, col, op);
    ink::rotation(&r, -45.0);
    r
}
/// K.text: a text in the menu language's font (thin or bold), centred vertically in its box.
#[allow(clippy::too_many_arguments)]
pub(super) fn text(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, s: &str, size: f64, col: C3, align: &str, thin: bool) -> Handle {
    let ax = match align {
        "center" => 0.5,
        "right" => 1.0,
        _ => 0.0,
    };
    let t = ink::new("inkTextWidget");
    if let Some(a) = enum_value("inkEAnchor", "TopLeft") {
        ink::call(&t, "SetAnchor", &[Value::U64(a as u64)]);
    }
    ink::v2(&t, "SetAnchorPoint", ax, 0.5);
    ink::move_to(&t, x + sx * ax, y + sy / 2.0);
    ink::reparent(&t, parent);
    match foreign(&super::menu_locale::code()) {
        Some((f, bold, th)) => ink::set_font(&t, &f, if thin { th } else { bold }),
        None if thin => ink::set_font(&t, FONT_THIN, "Book"),
        None => ink::set_font(&t, FONT, "Medium"),
    }
    ink::call(&t, "SetFontSize", &[Value::Int32(size as i32)]);
    let h = match align {
        "center" => "Center",
        "right" => "Right",
        _ => "Left",
    };
    if let Some(v) = enum_value("textHorizontalAlignment", h) {
        ink::call(&t, "SetHorizontalAlignment", &[Value::U64(v as u64)]);
    }
    if let Some(v) = enum_value("textVerticalAlignment", "Center") {
        ink::call(&t, "SetVerticalAlignment", &[Value::U64(v as u64)]);
    }
    // SAFETY: the text widget's Float property.
    if let Some(p) = unsafe { crate::script_call::property(t.instance, "scrollTextSpeed") } {
        unsafe { *(p as *mut f32) = 0.0 };
    }
    ink::set_text(&t, s);
    ink::tint(&t, col);
    t
}
pub(super) fn image(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, atlas: &str, part: &str, col: Option<C3>, op: f64) -> Handle {
    ink::image(parent, x, y, sx, sy, atlas, part, col.unwrap_or(WHITE), op)
}
/// An image whose atlas and part are already a resource token and a CName (the game's own icon records).
pub(super) fn image_raw(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, atlas: u64, part: u64, col: Option<C3>, op: f64) -> Handle {
    let q = ink::place(ink::new("inkImageWidget"), parent, x, y, sx, sy);
    ink::call(&q, "SetAtlasResource", &[Value::U64(atlas)]);
    ink::call(&q, "SetTexturePart", &[Value::U64(part)]);
    ink::tint(&q, col.unwrap_or(WHITE));
    ink::opacity(&q, op);
    q
}
pub(super) fn slice(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, atlas: &str, part: &str, col: Option<C3>, op: f64) -> Handle {
    let q = image(parent, x, y, sx, sy, atlas, part, col, op);
    // SAFETY: the image widget's Bool property.
    if let Some(p) = unsafe { crate::script_call::property(q.instance, "useNineSliceScale") } {
        unsafe { *(p as *mut bool) = true };
    }
    q
}
const ROUND: f64 = 12.0;
fn round_k(r: Option<f64>) -> f64 {
    (r.unwrap_or(ROUND) / ROUND).max(0.5)
}
fn rounded(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, part: &str, col: C3, op: f64, r: Option<f64>) -> Handle {
    let k = round_k(r);
    let q = slice(parent, x, y, sx / k, sy / k, ATLAS_COMMON, part, Some(col), op);
    ink::v2(&q, "SetRenderTransformPivot", 0.0, 0.0);
    ink::v2(&q, "SetScale", k, k);
    q
}
pub(super) fn round_fill(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, col: C3, op: f64, r: Option<f64>) -> Handle {
    rounded(parent, x, y, sx, sy, "frame_bg", col, op, r)
}
pub(super) fn round_edge(parent: &Handle, x: f64, y: f64, sx: f64, sy: f64, col: C3, op: f64, r: Option<f64>) -> Vec<Handle> {
    vec![rounded(parent, x, y, sx, sy, "frame_fg", col, op, r)]
}
pub(super) fn round_size(q: &Handle, sx: f64, sy: f64, r: Option<f64>) {
    let k = round_k(r);
    ink::size(q, sx / k, sy / k);
}

// ---------------------------------------------------------------- the pool of kept canvases
struct Kept {
    gen: u64,
    canvas: Handle,
    parts: Vec<Handle>,
}
#[derive(Default)]
pub(super) struct Pool {
    entries: HashMap<String, Vec<Kept>>,
    gen: u64,
    pooling: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for Pool {}
impl Pool {
    pub(super) fn begin(&mut self) {
        self.gen += 1;
        self.pooling = true;
    }
    pub(super) fn end(&mut self) {
        self.pooling = false;
    }
    pub(super) fn flush(&mut self) {
        self.entries.clear();
    }
    pub(super) fn drop_key(&mut self, key: &str) {
        self.entries.remove(key);
    }
    /// K.kept: a canvas at (x, y, w, h) holding what `build` drew -- one kept from an earlier rebuild when pooling
    /// (moved into place), else drawn now; (its parts, whether it was reused).
    pub(super) fn kept(&mut self, key: &str, parent: &Handle, x: f64, y: f64, w: f64, h: f64, build: &dyn Fn(&Handle) -> Vec<Handle>) -> (Vec<Handle>, bool) {
        if !self.pooling {
            let c = ink::canvas(parent, x, y, w, h);
            return (build(&c), false);
        }
        let gen = self.gen;
        let list = self.entries.entry(key.to_string()).or_default();
        for e in list.iter_mut() {
            if e.gen != gen {
                e.gen = gen;
                let c = e.canvas.clone();
                ink::place(c, parent, x, y, w, h);
                return (e.parts.clone(), true);
            }
        }
        let canvas = ink::canvas(parent, x, y, w, h);
        let parts = build(&canvas);
        list.push(Kept { gen, canvas, parts: parts.clone() });
        (parts, false)
    }
}
fn cut_lines(parent: &Handle, x: f64, y: f64, w: f64, h: f64, col: C3, op: f64, cut: f64, width: f64) -> Vec<Handle> {
    let p = [[x + cut, y], [x + w - cut, y], [x + w, y + cut], [x + w, y + h - cut], [x + w - cut, y + h], [x + cut, y + h], [x, y + h - cut], [x, y + cut]];
    (0..8).map(|i| line(parent, p[i][0], p[i][1], p[(i + 1) % 8][0], p[(i + 1) % 8][1], col, op, width)).collect()
}
/// K.cutFrame: an octagonal frame (pooled by its size).
#[allow(clippy::too_many_arguments)]
pub(super) fn cut_frame(pool: &mut Pool, parent: &Handle, x: f64, y: f64, w: f64, h: f64, col: C3, op: f64, cut: Option<f64>, width: Option<f64>) -> Vec<Handle> {
    let cut = cut.unwrap_or(12.0).min(w * 0.2).min(h * 0.35);
    let wd = width.unwrap_or(2.0);
    let key = format!("cut:{}:{}:{}:{}", g(w), g(h), g(cut), g(wd));
    let (parts, reused) = pool.kept(&key, parent, x, y, w, h, &|c| cut_lines(c, 0.0, 0.0, w, h, col, op, cut, wd));
    if reused {
        tint(&parts, col, Some(op));
    }
    parts
}
/// Lua's %g.
fn g(v: f64) -> String {
    super::lobby_holo::fmt_num(v)
}
pub(super) fn ring(parent: &Handle, cx: f64, cy: f64, r: f64, width: f64, col: C3, op: f64, segments: usize, gaps: Option<usize>) -> Vec<Handle> {
    ink::ring(parent, cx, cy, r, width, col, op, segments, gaps)
}
/// K.holoPanel: the panel's glass, scan lines, frames and corner marks (pooled by size).
pub(super) fn holo_panel(pool: &mut Pool, parent: &Handle, w: f64, h: f64) {
    let key = format!("holo:{}:{}", g(w), g(h));
    // The inner frames are drawn unpooled inside the pooled canvas (drawn once per kept canvas).
    let _ = pool.kept(&key, parent, 0.0, 0.0, w, h, &|c| {
        holo_draw(&mut Pool::default(), c, w, h);
        Vec::new()
    });
}
fn holo_draw(pool: &mut Pool, parent: &Handle, w: f64, h: f64) {
    let cut = if h < 100.0 { 10.0 } else { 24.0 };
    round_fill(parent, 10.0, 10.0, w - 20.0, h - 20.0, NAVY, 0.18, Some(4.0));
    if h >= 180.0 {
        let step = 18f64.max((h / 64.0).ceil());
        let mut y = 32.0;
        while y <= h - 28.0 {
            rect(parent, 24.0, y, w - 48.0, 1.0, CYAN, 0.025);
            y += step;
        }
    }
    cut_frame(pool, parent, 2.0, 2.0, w - 4.0, h - 4.0, CYAN, 0.07, Some(cut), Some(6.0));
    cut_frame(pool, parent, 2.0, 2.0, w - 4.0, h - 4.0, CYAN, 0.72, Some(cut), Some(2.0));
    cut_frame(pool, parent, 10.0, 10.0, w - 20.0, h - 20.0, CYAN, 0.17, Some((cut - 6.0).max(6.0)), Some(1.0));
    if h >= 180.0 {
        line(parent, 2.0, cut + 2.0, cut + 2.0, 2.0, CYAN, 0.95, 5.0);
        line(parent, w - cut - 2.0, h - 2.0, w - 2.0, h - cut - 2.0, CYAN, 0.95, 5.0);
        rect(parent, cut + 12.0, 2.0, 90.0, 3.0, CYAN, 0.9);
        rect(parent, w - cut - 102.0, h - 4.0, 90.0, 3.0, CYAN, 0.9);
        for i in 0..3 {
            let i = f64::from(i);
            rect(parent, w - 58.0, 30.0 + i * 8.0, 28.0 - i * 7.0, 2.0, CYAN, 0.3);
            rect(parent, 22.0, h - 54.0 + i * 8.0, 22.0 + i * 5.0, 2.0, CYAN, 0.25);
        }
    }
}
/// K.clip: at most `n` characters, the last one an ellipsis when cut.
pub(super) fn clip(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    let mut out: String = chars[..n.saturating_sub(1)].iter().collect();
    out.push('…');
    out
}
/// K.upper: upper case for ASCII, Latin-1, Latin Extended-A and Cyrillic (as the Lua did; ß and the rest unchanged).
pub(super) fn upper(s: &str) -> String {
    s.chars()
        .map(|c| {
            let cp = c as u32;
            let up = if c.is_ascii_lowercase() {
                cp - 32
            } else if (0xE0..=0xFE).contains(&cp) && cp != 0xF7 {
                cp - 32
            } else if cp == 0xFF {
                0x178
            } else if cp == 0x131 {
                0x49
            } else if (0x100..=0x137).contains(&cp) || (0x14A..=0x177).contains(&cp) {
                if cp % 2 == 1 {
                    cp - 1
                } else {
                    cp
                }
            } else if (0x139..=0x148).contains(&cp) || (0x179..=0x17E).contains(&cp) {
                if cp % 2 == 0 {
                    cp - 1
                } else {
                    cp
                }
            } else if (0x430..=0x44F).contains(&cp) {
                cp - 32
            } else if (0x450..=0x45F).contains(&cp) {
                cp - 80
            } else if cp == 0x491 {
                0x490
            } else {
                cp
            };
            char::from_u32(up).unwrap_or(c)
        })
        .collect()
}
pub(super) fn cname(n: &str) -> u64 {
    CName::hashed(n).0
}
