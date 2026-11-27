//! The Reload mod's run-time data (reload_data.json, made by the scratch generator from mods/cet/CyberpunkVRPort_Reload:
//! reload/weapons/*.lua in index order -- the derived Igla and Testera applied -- reload/poses/*.lua, reload/rigs.lua,
//! reload/handful/*.lua, reload/glow_list.lua, capsules.lua, weapon_silverhand.lua): parsed once, read as the Lua read
//! its tables. Regenerate when the Lua data changes.
use serde_json::Value as J;
use std::sync::OnceLock;

static DATA: OnceLock<J> = OnceLock::new();
pub(super) fn data() -> &'static J {
    DATA.get_or_init(|| serde_json::from_str(include_str!("reload_data.json")).unwrap_or(J::Null))
}
/// A finger pose: (bone, quaternion) per joint, LEFT-hand names.
pub(super) type Pose = Vec<(String, [f64; 4])>;
pub(super) fn pose(name: &str) -> Option<Pose> {
    pose_j(data().get("poses")?.get(name)?)
}
/// A pose written inline (the handful's recorded ones).
pub(super) fn pose_j(p: &serde_json::Value) -> Option<Pose> {
    let p = p.as_array()?;
    Some(
        p.iter()
            .filter_map(|e| {
                let a = e.as_array()?;
                Some((a.first()?.as_str()?.to_string(), [a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?, a.get(4)?.as_f64()?]))
            })
            .collect(),
    )
}
/// The weapon configs, in the index's order.
pub(super) fn configs() -> Vec<&'static J> {
    let d = data();
    let Some(index) = d.get("index").and_then(J::as_array) else { return Vec::new() };
    index.iter().filter_map(|n| n.as_str().and_then(|n| d.get("weapons")?.get(n))).collect()
}

/// Lua-like reads of a JSON table.
pub(super) trait Lua {
    fn g(&self, k: &str) -> &J;
    fn i(&self, n: usize) -> &J;
    fn f(&self, k: &str, d: f64) -> f64;
    fn fo(&self, k: &str) -> Option<f64>;
    fn truthy(&self) -> bool;
    fn b(&self, k: &str) -> bool;
    fn s(&self, k: &str) -> Option<&str>;
    fn v3(&self, k: &str) -> Option<[f64; 3]>;
    fn v4(&self, k: &str) -> Option<[f64; 4]>;
    fn arr(&self) -> &[J];
    fn present(&self) -> bool;
}
impl Lua for J {
    fn g(&self, k: &str) -> &J {
        self.get(k).unwrap_or(&J::Null)
    }
    /// Element `n` of an array (1-based, as Lua indexes) -- or, of an object, its key "n": a Lua table that held both
    /// numbered entries and named ones (shotgun.carry2 = { [1], [2], pose }) was written out as an object. Read as an
    /// array only, both shells of a two-shell carry had no hold at all: turned off the hand and one inside the other.
    fn i(&self, n: usize) -> &J {
        match self {
            J::Object(o) => o.get(&n.to_string()).unwrap_or(&J::Null),
            _ => n.checked_sub(1).and_then(|n| self.get(n)).unwrap_or(&J::Null),
        }
    }
    fn f(&self, k: &str, d: f64) -> f64 {
        self.g(k).as_f64().unwrap_or(d)
    }
    fn fo(&self, k: &str) -> Option<f64> {
        self.g(k).as_f64()
    }
    fn truthy(&self) -> bool {
        !matches!(self, J::Null | J::Bool(false))
    }
    fn b(&self, k: &str) -> bool {
        self.g(k).truthy()
    }
    fn s(&self, k: &str) -> Option<&str> {
        self.g(k).as_str()
    }
    fn v3(&self, k: &str) -> Option<[f64; 3]> {
        let a = self.g(k).as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?])
    }
    fn v4(&self, k: &str) -> Option<[f64; 4]> {
        let a = self.g(k).as_array()?;
        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?, a.get(2)?.as_f64()?, a.get(3)?.as_f64()?])
    }
    fn arr(&self) -> &[J] {
        self.as_array().map(Vec::as_slice).unwrap_or(&[])
    }
    fn present(&self) -> bool {
        !self.is_null()
    }
}
