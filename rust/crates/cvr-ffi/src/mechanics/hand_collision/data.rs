//! The hand collision's data, as main's mod carries it (generated from the game's assets by its tools, kept here as
//! JSON): the body's capsules (capsules.json, the same the collider asset carries: one capsule in its bone's frame, half
//! its length along the bone's X), the Silverhand's part capsules (weapon_silverhand.json, one per moving part, a
//! vrp_<bone> slot each), the guns' and the blades' boxes (weapon_shapes.json: each weapon as a few boxes in its own
//! frame -- origin at the grip, +Y along the barrel, +Z up -- { minX, maxX, minY, maxY, minZ, maxZ } by the name its
//! mesh components start with) and how far each attachment reaches from its mount (weapon_attachments.json).
use super::V3;
use red4ext_abi::cname::CName;
use serde_json::Value as J;
use std::collections::HashMap;
use std::sync::OnceLock;

pub(super) struct Cap {
    pub slot: String,
    pub index: i32,
    pub r: f32,
    pub half: f32,
    pub c: V3,
}
pub(super) struct WeaponCap {
    pub slot: CName,
}
/// A finger's capsules (indices into caps): its first joint, its second (where a ray starts), its tip.
#[derive(Clone, Copy, Default)]
pub(super) struct Finger {
    pub base: Option<usize>,
    pub mid: Option<usize>,
    pub tip: Option<usize>,
}
pub(super) struct Data {
    pub caps: Vec<Cap>,
    /// each hand's capsule (vrp_hand_l, vrp_hand_r)
    pub hand: [Option<usize>; 2],
    /// each hand's fingers, 0 thumb .. 4 little finger (slots vrp_f_<finger><joint>_<l|r>)
    pub fingers: [[Option<Finger>; 5]; 2],
    pub silver: Vec<WeaponCap>,
    pub shapes: HashMap<String, Vec<[f32; 6]>>,
    pub atts: HashMap<String, [f32; 6]>,
}
fn num(v: &J, k: &str) -> f32 {
    v.get(k).and_then(J::as_f64).unwrap_or(0.0) as f32
}
fn six(v: &J) -> Option<[f32; 6]> {
    let a = v.as_array()?;
    (a.len() == 6).then(|| [0, 1, 2, 3, 4, 5].map(|i| a[i].as_f64().unwrap_or(0.0) as f32))
}
fn load() -> Data {
    let caps_json: J = serde_json::from_str(include_str!("capsules.json")).unwrap_or(J::Null);
    let caps: Vec<Cap> = caps_json
        .as_array()
        .map(|a| {
            a.iter()
                .map(|c| Cap {
                    slot: c.get("slot").and_then(J::as_str).unwrap_or_default().to_string(),
                    index: c.get("index").and_then(J::as_i64).unwrap_or(-1) as i32,
                    r: num(c, "r"),
                    half: num(c, "half"),
                    c: [num(c, "cx"), num(c, "cy"), num(c, "cz")],
                })
                .collect()
        })
        .unwrap_or_default();
    let mut hand = [None; 2];
    let mut fingers = [[None; 5]; 2];
    const DIGITS: [&str; 5] = ["thumb", "index", "mid", "ring", "pink"];
    for (i, c) in caps.iter().enumerate() {
        if c.slot == "vrp_hand_l" {
            hand[0] = Some(i);
        }
        if c.slot == "vrp_hand_r" {
            hand[1] = Some(i);
        }
        // '^vrp_f_(%a+)(%d)_([lr])$'
        let Some(rest) = c.slot.strip_prefix("vrp_f_") else { continue };
        let b = rest.as_bytes();
        let n = b.iter().take_while(|x| x.is_ascii_alphabetic()).count();
        if n == 0 || b.len() != n + 3 || !b[n].is_ascii_digit() || b[n + 1] != b'_' || !(b[n + 2] == b'l' || b[n + 2] == b'r') {
            continue;
        }
        let Some(digit) = DIGITS.iter().position(|d| *d == &rest[..n]) else { continue };
        let joint = b[n] - b'0';
        let side = usize::from(b[n + 2] == b'r');
        let f = fingers[side][digit].get_or_insert_with(Finger::default);
        // thumb joints are 0, 1, 2, the rest 1, 2, 3: the second of them is where the ray starts, the last the tip
        let first = if digit == 0 { 0 } else { 1 };
        if joint == first {
            f.base = Some(i);
        }
        if joint == first + 1 {
            f.mid = Some(i);
        }
        if joint == first + 2 {
            f.tip = Some(i);
        }
    }
    let silver_json: J = serde_json::from_str(include_str!("weapon_silverhand.json")).unwrap_or(J::Null);
    let silver = silver_json
        .as_array()
        .map(|a| a.iter().map(|c| WeaponCap { slot: CName::hashed(c.get("slot").and_then(J::as_str).unwrap_or_default()) }).collect())
        .unwrap_or_default();
    let shapes_json: J = serde_json::from_str(include_str!("weapon_shapes.json")).unwrap_or(J::Null);
    let shapes = shapes_json
        .as_object()
        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_array().map(|a| a.iter().filter_map(six).collect()).unwrap_or_default())).collect())
        .unwrap_or_default();
    let atts_json: J = serde_json::from_str(include_str!("weapon_attachments.json")).unwrap_or(J::Null);
    let atts = atts_json.as_object().map(|o| o.iter().filter_map(|(k, v)| six(v).map(|b| (k.clone(), b))).collect()).unwrap_or_default();
    Data { caps, hand, fingers, silver, shapes, atts }
}
pub(super) fn data() -> &'static Data {
    static DATA: OnceLock<Data> = OnceLock::new();
    DATA.get_or_init(load)
}
