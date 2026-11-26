//! modules/scene_list.lua (made by tools/vr_lobby_assets): the lobby's place in the main menu's world, its light, the
//! holo's arc, the figure's spot, the scene's entities and its loose items.
use super::lobby_holo::{Spec, WindowSpec};
use super::lobby_items::{Entry, EntryRef};
use std::rc::Rc;

pub(super) const EXPOSURE: f64 = 0.50;
pub(super) const HOUR: i64 = 21;
pub(super) const MINUTE: i64 = 30;
pub(super) const WEATHER: &str = "24h_weather_light_clouds";
pub(super) const PLACE: ([f64; 3], f64) = ([-417.0, 422.5, -215.0], -90.0);
pub(super) const FIGURE: (f64, f64) = (38.0, 2.30);
pub(super) const ENTITIES: [&str; 1] = ["vrport\\lobby\\vr_lobby_near.ent"];
pub(super) fn holo() -> Spec {
    Spec { bearing: 0.0, dist: 1.10, height: 1.55, panel: [0.98, 0.75], gap: 0.08, window: Some(WindowSpec { bearing: -10.0, dist: 1.50, height: 1.10, size: [2.2, 2.0] }) }
}
#[allow(clippy::too_many_arguments)]
fn item(name: &str, ent: &str, position: [f64; 3], rotation: [f64; 4], radius: f64, mass: f64, shape: &str, half: [f64; 3], grasp: Option<([f64; 3], [f64; 3])>) -> EntryRef {
    Rc::new(Entry {
        name: name.into(),
        ent: ent.into(),
        position,
        rotation,
        radius,
        mass,
        shape: Some(shape.into()),
        half: Some(half),
        grasp_half: grasp.map(|g| g.0),
        grasp_at: grasp.map(|g| g.1),
        centre: None,
        look: None,
    })
}
pub(super) fn items() -> Vec<EntryRef> {
    vec![
        item("item can", "vrport\\lobby\\vr_lobby_item_0.ent", [-1.97569, -0.77868, 0.86350], [0.51504, 0.0, 0.0, 0.85717], 0.0610, 0.350, "cylinder", [0.03300, 0.03300, 0.06100], None),
        item(
            "item chibi",
            "vrport\\lobby\\vr_lobby_item_1.ent",
            [-1.34237, -1.41577, 0.92762],
            [0.35793, 0.0, 0.0, 0.93375],
            0.1251,
            0.600,
            "box",
            [0.06742, 0.07062, 0.12513],
            Some(([0.05500, 0.02400, 0.06250], [-0.00011, -0.00486, -0.06262])),
        ),
        item("item cube", "vrport\\lobby\\vr_lobby_item_2.ent", [-1.85475, -0.96354, 0.88750], [0.51504, 0.0, 0.0, 0.85717], 0.0850, 1.200, "box", [0.08500, 0.08500, 0.08500], None),
        item("item shard", "vrport\\lobby\\vr_lobby_item_3.ent", [-1.58496, -0.66152, 0.81050], [0.51504, 0.0, 0.0, 0.85717], 0.0550, 0.050, "box", [0.05500, 0.03000, 0.00800], None),
        item("item shard.001", "vrport\\lobby\\vr_lobby_item_4.ent", [-1.59771, -0.50974, 0.81050], [0.51504, 0.0, 0.0, 0.85717], 0.0550, 0.050, "box", [0.05500, 0.03000, 0.00800], None),
        item(
            "item skippy",
            "vrport\\lobby\\vr_lobby_item_5.ent",
            [-1.99407, -0.40334, 0.87746],
            [0.65641, 0.0, 0.0, 0.75440],
            0.0750,
            0.150,
            "cylinder",
            [0.06718, 0.03825, 0.07497],
            Some(([0.03200, 0.03200, 0.06000], [-0.01002, -0.00003, -0.01496])),
        ),
        item("item sphere", "vrport\\lobby\\vr_lobby_item_6.ent", [-1.65981, -1.24496, 0.91250], [0.51504, 0.0, 0.0, 0.85717], 0.1100, 2.000, "sphere", [0.11000, 0.11000, 0.11000], None),
    ]
}
