//! The VR hand's recoil presentation per weapon (main's mods/cet/CyberpunkVRPort_Weapon/hand_kick.lua): the one- and
//! two-hand pose contribution and hand-pose smoothing of the linked VR-game prefabs -- references, not peak angles or
//! fire rates -- by the weapon's audio family within its item type. The native lever and the player's slider still
//! apply on top.
//!
//! Each row is (the controller pose, the CP presentation accent, the legacy cycle fraction); the fraction stays
//! available to telemetry. A supported (two-handed) hold scales the accent by the excursion ratio of a spring twice as
//! stiff: Footlong.Recoil's shared angular restoring strength is 17 one-handed and 34 two-handed, and for the same
//! impulse a spring's excursion scales with the inverse square root of its stiffness.

/// A presentation reference: one-hand and two-hand pose contribution, hand-pose smoothing (s), the prefab's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    one: f64,
    two: f64,
    smoothing: f64,
    reference: &'static str,
}
const CLASSIC: Pose = Pose { one: 0.20, two: 0.40, smoothing: 0.03, reference: "M1911A1" }; // Modern1911 the same
const BOLD: Pose = Pose { one: 0.30, two: 0.60, smoothing: 0.04, reference: "G17Gen3" };
const COMPACT: Pose = Pose { one: 0.15, two: 0.25, smoothing: 0.04, reference: "HPoint380" };
const LONG: Pose = Pose { one: 0.20, two: 0.20, smoothing: 0.06, reference: "R1022Classic" }; // shared with Tactical

type Row = (Pose, f64, f64);
const HANDGUN: &[(&str, Row)] = &[
    ("wea_set_unity", (CLASSIC, 1.0, 0.180 / 0.300)),
    ("wea_set_liberty", (BOLD, 1.0, 0.185 / 0.275)),
    ("wea_set_nue", (BOLD, 1.10, 0.205 / 0.350)),
    ("wea_set_grit", (COMPACT, 1.0, 0.160 / 0.200)),
    ("wea_set_omaha", (COMPACT, 1.0, 0.155 / 0.1777)),
    ("wea_set_silverhand", (BOLD, 1.42, 0.220 / 0.300)),
    ("wea_set_chao", (COMPACT, 0.78, 0.84)),
    ("wea_set_kappa", (COMPACT, 0.78, 0.90)),
    ("wea_set_yukimura", (COMPACT, 0.90, 0.88)),
    ("wea_set_slaught", (COMPACT, 1.0, 0.91)),
    ("wea_set_lexington", (CLASSIC, 0.90, 0.78)),
    ("wea_set_kenshin", (CLASSIC, 1.0, 0.82)),
    ("wea_set_ticon", (CLASSIC, 1.06, 0.75)),
    ("wea_set_toygun", (COMPACT, 0.45, 0.92)),
];
/// The handgun sets that share another's row.
const HANDGUN_ALIASES: &[(&str, &str)] = &[
    ("wea_set_unity_dex", "wea_set_unity"),
    ("wea_set_liberty_dex", "wea_set_liberty"),
    ("wea_set_chao_voodoo", "wea_set_chao"),
    ("wea_set_kenshin_chaos", "wea_set_kenshin"),
    ("wea_set_ticon_reed", "wea_set_ticon"),
    ("wea_set_yukimura_preset", "wea_set_yukimura"),
];
const REVOLVER: &[(&str, Row)] = &[
    ("wea_set_burya", (BOLD, 1.50, 0.390 / 0.600)),
    ("wea_set_metel", (BOLD, 1.35, 0.405 / 0.600)),
    ("wea_set_nova", (CLASSIC, 1.22, 0.215 / 0.300)),
    ("wea_set_nova_suppressor", (CLASSIC, 1.22, 0.215 / 0.300)),
    ("wea_set_overture", (BOLD, 1.32, 0.270 / 0.400)),
    ("wea_set_quasar", (CLASSIC, 1.18, 0.155 / 0.180)),
];
const RIFLE: &[(&str, Row)] = &[("wea_set_kyubi", (LONG, 1.0, 0.170 / 0.200)), ("wea_set_special_plasma_cutter", (LONG, 0.84, 0.82))];
const PRECISION: &[(&str, Row)] = &[
    ("wea_set_achilles", (LONG, 0.72, 0.175 / 0.200)),
    ("wea_set_sor22", (LONG, 0.85, 0.250 / 0.500)),
    ("wea_set_kolac", (LONG, 0.95, 0.265 / 0.500)),
];
const SNIPER: &[(&str, Row)] = &[
    ("wea_set_ashura", (LONG, 0.70, 0.410 / 0.600)),
    ("wea_set_grad", (LONG, 0.95, 1.15 / 2.70)),
    ("wea_set_grad_panam", (LONG, 0.95, 1.15 / 2.70)),
    ("wea_set_rasetsu", (LONG, 0.85, 0.720 / 1.20)),
    ("wea_set_nekomata", (LONG, 0.92, 0.450 / 0.600)),
    ("wea_set_osprey", (LONG, 0.88, 0.85)),
];
/// Records with no usable audio family, or one shared with another item.
const ROCKET: Row = (LONG, 1.02, 1.55 / 2.20);
const RETRO_NUE: Row = (BOLD, 1.10, 0.205 / 0.400);

fn row_in(table: &[(&str, Row)], name: &str) -> Option<Row> {
    table.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

/// The item type's `wea_*` token (main's `%f[%w_]wea_[%w_]+`): `ItemType.Wea_Rifle` -> `wea_rifle`.
pub fn type_token(item_type: &str) -> Option<String> {
    let low = item_type.to_lowercase();
    let bytes = low.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while let Some(at) = low[i..].find("wea_") {
        let start = i + at;
        if start == 0 || !word(bytes[start - 1]) {
            let end = low[start..].find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).map_or(low.len(), |e| start + e);
            return Some(low[start..end].to_string());
        }
        i = start + 4;
    }
    None
}

/// What the weapon's draw publishes: the one-handed spring accent (its kick), the supported one, the legacy cycle
/// fraction, the hand-pose smoothing (s) and the reference's name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandKick {
    pub solo: f64,
    pub supported: f64,
    pub tap_scale: f64,
    pub blend_time: f64,
    pub reference: &'static str,
}

/// The weapon's profile from its record `key`, its item type, its audio family (`<key>.audioName`) and whether either
/// trigger mode names a `TriggerMode.*` record; None for an unprofiled weapon (the legacy family kick stays).
pub fn profile(key: &str, item_type: &str, audio: Option<&str>, has_trigger_mode: bool) -> Option<HandKick> {
    let token = type_token(item_type)?;
    let name = audio.unwrap_or("");
    let mut row = match token.as_str() {
        "wea_handgun" => row_in(HANDGUN, name).or_else(|| {
            HANDGUN_ALIASES.iter().find(|(a, _)| *a == name).and_then(|(_, to)| row_in(HANDGUN, to))
        }),
        "wea_revolver" => row_in(REVOLVER, name),
        "wea_rifle" => row_in(RIFLE, name),
        "wea_precisionrifle" => row_in(PRECISION, name),
        "wea_sniperrifle" => row_in(SNIPER, name),
        _ => return None,
    };
    if token == "wea_handgun" {
        let by_record = match key {
            "Items.Base_Arasaka3D_Gun" => row_in(HANDGUN, "wea_set_silverhand"),
            "Items.Craftable_Legendary_Yukimura" => row_in(HANDGUN, "wea_set_yukimura"),
            "Items.BlackwallForce" => row_in(HANDGUN, "wea_set_chao"),
            _ => None,
        };
        row = by_record.or(row);
        if name == "wea_set_nue" && key.starts_with("Items.Preset_Nue_Arasaka_2020") {
            row = Some(RETRO_NUE);
        }
    } else if token == "wea_sniperrifle" && name == "wea_set_grad" && (key == "Items.Base_RocketLauncher" || key.starts_with("Items.Preset_RocketLauncher_")) {
        row = Some(ROCKET);
    }
    let (pose, accent, fraction) = row?;
    if !has_trigger_mode {
        return None;
    }
    let solo = accent * (1.0 + 2.0 * (pose.one - 0.20));
    let supported_excursion = (17.0f64 / 34.0).sqrt();
    Some(HandKick {
        solo,
        supported: solo * supported_excursion * (1.0 + pose.one) / (1.0 + pose.two),
        tap_scale: fraction,
        blend_time: pose.smoothing,
        reference: pose.reference,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_types_token() {
        assert_eq!(type_token("ItemType.Wea_Rifle").as_deref(), Some("wea_rifle"));
        assert_eq!(type_token("ItemType.Wea_PrecisionRifle").as_deref(), Some("wea_precisionrifle"));
        assert_eq!(type_token("ItemType.Cyb_Launcher"), None);
        assert_eq!(type_token("ItemType.xwea_rifle"), None, "a frontier before the token");
    }

    #[test]
    fn the_profiles() {
        let unity = profile("Items.Preset_Unity_Default", "ItemType.Wea_Handgun", Some("wea_set_unity"), true).unwrap();
        assert!((unity.solo - 1.0).abs() < 1e-12);
        let expect = (0.5f64).sqrt() * 1.2 / 1.4;
        assert!((unity.supported - expect).abs() < 1e-12, "{}", unity.supported);
        assert_eq!((unity.blend_time, unity.reference), (0.03, "M1911A1"));
        assert!((unity.tap_scale - 0.6).abs() < 1e-12);
        // an alias, a record override, the retro Nue, the rocket launcher
        assert_eq!(profile("Items.X", "ItemType.Wea_Handgun", Some("wea_set_unity_dex"), true), Some(unity));
        let silver = profile("Items.Base_Arasaka3D_Gun", "ItemType.Wea_Handgun", Some("wea_set_whatever"), true).unwrap();
        assert!((silver.solo - 1.42 * 1.2).abs() < 1e-12);
        let retro = profile("Items.Preset_Nue_Arasaka_2020_A", "ItemType.Wea_Handgun", Some("wea_set_nue"), true).unwrap();
        assert!((retro.tap_scale - 0.205 / 0.400).abs() < 1e-12);
        let rocket = profile("Items.Preset_RocketLauncher_X", "ItemType.Wea_SniperRifle", Some("wea_set_grad"), true).unwrap();
        assert!((rocket.solo - 1.02).abs() < 1e-12);
        // not profiled: another type, an unknown family, no trigger mode
        assert_eq!(profile("Items.X", "ItemType.Wea_AssaultRifle", Some("wea_set_unity"), true), None);
        assert_eq!(profile("Items.X", "ItemType.Wea_Handgun", Some("wea_set_nope"), true), None);
        assert_eq!(profile("Items.X", "ItemType.Wea_Handgun", Some("wea_set_unity"), false), None);
    }
}
