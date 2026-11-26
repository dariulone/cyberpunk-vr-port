//! mods/cet/CyberpunkVRPort_Optics (with optics/policy.lua and optics/profiles.lua): the weapon's sight as a physical
//! optic. Every 0.2 s (and on a weapon change) the active ranged weapon is scanned: its optical path chosen from its
//! scope and zoom (Policy.select: none, collimator, shader, camera, native), an authored scope's body masked
//! (bodyMask), its lens's magnification appearance selected (z100..z399) or its camera surface/panel switched on, the
//! native ADS lens and cap gates kept on (both tracks carry the physical housing), the native reticle panels replaced by
//! the port's surfaces; scope HUD parts that screen the lens hidden (a pass shortly after the scope appears, then every
//! 5 s); a weapon left is restored. Main's 36d046482 and 0aa233e0a: a weapon's built-in sight (INTEGRATED: the port's
//! vr_builtin_* surfaces, a camera display or a collimator, the native parts they replace hidden), the long scopes'
//! housing variants on the shader path, Kyubi's and Senkoh's panel dots scaled, and a skinned aperture's frame from its
//! live bone (FRAMES: the Osprey's screen on scope_hinge, held open by the hinge request while aimed through). Every frame, while the eye is on a
//! sight's line (behind it, within 3/4.5 cm), the sight is brought closer: the eye approaches the eyepiece along the
//! sight's axis (SetVRSightApproach), a split shader scope takes part of its magnification as the game's ADS zoom
//! (ZoomAnimData and AnimFeature_AimPlayer, the weapon layer synced: SetVRSightZoom), shots leave the sight line
//! (SetVRSightLine); the camera path places vr_optics_camera at the sight in the WeaponRight slot's frame with the
//! magnification's FOV. Nothing until the eyes are settled on the player (the VRCAM selection) and the physical
//! optics are on. The path is published (SetVRPhysicalOpticsPath). The math is in f64, as Lua's numbers were.
use crate::script_call::{call_handle, method, method_typed, Value};
use core::ffi::c_void;
use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::Class;
use std::sync::Mutex;

type R<T> = Result<T, String>;
type V3 = [f64; 3];

const ENTER_AXIS: f64 = 0.030;
const LEAVE_AXIS: f64 = 0.045;
const RELEASE_HOLD: f64 = 0.35;
const APPROACH_TIME: f64 = 0.15;
const APPROACH_KEEP: f64 = 0.5;
const APPROACH_RELIEF: f64 = 0.03;
const APPROACH_MAX: f64 = 0.065;
const HIDE_FIRST: f64 = 0.4;
const HIDE_EVERY: f64 = 5.0;
const IDLE_CAMERA_CHECK: f64 = 0.25;
/// Policy: magnified scopes from this magnification on get the camera path; the camera image's coverage.
const CAMERA_THRESHOLD: f64 = 1.0;
const IMAGE_COVERAGE_DEGREES: f64 = 80.0;
/// The shader scope's split: the view's zoom from lo.zoom at lo.mag to hi.zoom at hi.mag.
const SPLIT_LO: (f64, f64) = (1.7, 1.25);
const SPLIT_HI: (f64, f64) = (2.35, 1.5);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Path {
    None,
    Collimator,
    Shader,
    Camera,
    Native,
}
impl Path {
    fn code(self) -> i32 {
        match self {
            Path::None => 0,
            Path::Collimator => 1,
            Path::Shader => 2,
            Path::Camera => 3,
            Path::Native => 4,
        }
    }
    /// How far behind the sight the eye may be for the sight to come closer (SIGHT_ZOOM).
    fn behind(self) -> Option<f64> {
        match self {
            Path::Collimator => Some(0.80),
            Path::Shader | Path::Camera | Path::Native => Some(0.30),
            Path::None => None,
        }
    }
}

/// A reticle layer of an authored scope (profiles.lua).
struct Reticle {
    surface: &'static str,
    panel: &'static str,
    original_panel: &'static str,
    original_mesh: &'static str,
}
/// An authored scope (profiles.lua, generated from the native apertures).
struct Profile {
    mesh: &'static str,
    key: &'static str,
    body_mask: u64,
    camera_only: bool,
    original_panel: Option<&'static str>,
    front_camera: bool,
    scope_offset: Option<V3>,
    reticles: &'static [Reticle],
}
const fn rt(surface: &'static str, panel: &'static str, original_panel: &'static str, original_mesh: &'static str) -> Reticle {
    Reticle { surface, panel, original_panel, original_mesh }
}
const FRONT: (&str, &str) = ("vr_optic_reticle_front", "vr_optic_reticle_front_panel");
const REAR: (&str, &str) = ("vr_optic_reticle_rear", "vr_optic_reticle_rear_panel");
static PROFILES: [Profile; 11] = [
    Profile { mesh: "w_att__scope_long_01_017225", key: "long_01", body_mask: 25, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui", "w_att__scope_long_01_0532"), rt(REAR.0, REAR.1, "on_weapon_ui2", "w_att__scope_long_01_6858")] },
    Profile { mesh: "w_att__scope_long_02_010120", key: "long_02", body_mask: 221, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui_2", "w_att__scope_long_02_2260"), rt(REAR.0, REAR.1, "on_weapon_ui_1", "w_att__scope_long_02_1426")] },
    Profile { mesh: "w_att__scope_long_03_014631", key: "long_03", body_mask: 51, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui_1", "w_att__scope_long_03_6644"), rt(REAR.0, REAR.1, "on_weapon_ui_2", "w_att__scope_long_03_1474")] },
    Profile { mesh: "w_att__scope_long_05_01", key: "long_05", body_mask: 13, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui_2", "w_att__scope_long_05_2238"), rt(REAR.0, REAR.1, "on_weapon_ui_5662", "w_att__scope_long_05_0285")] },
    Profile { mesh: "w_att__scope_short_02_012834", key: "short_02", body_mask: 9, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui_01", "w_att__scope_short_02_4071"), rt(REAR.0, REAR.1, "on_weapon_ui_02", "w_att__scope_short_02_4785")] },
    Profile { mesh: "w_att__scope_short_06_01", key: "short_06", body_mask: 115, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(REAR.0, REAR.1, "on_weapon_ui_2417", "w_att__scope_short_06_01_indicators_rear")] },
    Profile { mesh: "w_att__scope_sniper_01_010173", key: "sniper_01", body_mask: 221, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "WorldWidget0202", "w_att__scope_sniper_01_8133"), rt(REAR.0, REAR.1, "WorldWidget3231", "w_att__scope_sniper_01_1626")] },
    Profile { mesh: "w_att__scope_sniper_02_015733", key: "sniper_02", body_mask: 25, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "WorldWidget7520", "w_att__scope_sniper_02_7502"), rt(REAR.0, REAR.1, "WorldWidget8541", "w_att__scope_sniper_02_5032")] },
    Profile { mesh: "w_att__scope_sniper_03_018248", key: "sniper_03", body_mask: 29, camera_only: false, original_panel: None, front_camera: false, scope_offset: None,
        reticles: &[rt(FRONT.0, FRONT.1, "on_weapon_ui_2", "w_att__scope_sniper_03_01_indicator_front"), rt(REAR.0, REAR.1, "on_weapon_ui_1", "w_att__scope_sniper_03_01_indicator_rear")] },
    Profile { mesh: "w_rifle_sniper__tsunami_rasetsu__base1_5375", key: "rasetsu", body_mask: 0, camera_only: true, original_panel: Some("on_weapon_ui3445"), front_camera: true, scope_offset: None,
        reticles: &[] },
    Profile { mesh: "w_rifle_sniper__nokota_osprey__base1_8610", key: "osprey", body_mask: 0, camera_only: true, original_panel: None, front_camera: false,
        scope_offset: Some([-0.000555321574, -0.04717387814, 0.150453046]),
        reticles: &[rt(REAR.0, REAR.1, "on_weapon_ui3445", "w_rifle_sniper__nokota_osprey__base1_8610")] },
];
/// Scope HUD parts hidden while the port draws the lens.
fn hide_names(key: &str) -> &'static [&'static str] {
    match key {
        "short_06" => &["glow", "borderBoxTransp"],
        "long_05" => &["grid"],
        "long_02" => &["effect", "bg_fuzz", "fuzz_img", "scanline_fill", "inkImageWidget2"],
        "sniper_01" => &["interference", "crpsshair_bg"],
        _ => &[],
    }
}
/// A weapon's built-in sight (main's optics/integrated.lua, 36d046482): the port's named surfaces are authored
/// disabled; the native panels and glass `hide` lists go back to their own visibility when an attachment takes over or
/// the weapon leaves VR. `camera` sights show the optics camera on their panel (Guillotine, Hercules); an `attachment`
/// sight is the short_05 scope's (Dante's, or the scope installed); `scope_offset` the aperture's own centre;
/// `keep_eye_relief` keeps the eye where it is (Senkoh LX).
struct Integrated {
    key: &'static str,
    surface: &'static str,
    panel: Option<&'static str>,
    camera: bool,
    attachment: bool,
    hide: &'static [&'static str],
    scope_offset: Option<V3>,
    keep_eye_relief: bool,
}
const fn sight(key: &'static str, surface: &'static str, hide: &'static [&'static str]) -> Integrated {
    Integrated { key, surface, panel: None, camera: false, attachment: false, hide, scope_offset: None, keep_eye_relief: false }
}
static INTEGRATED: [Integrated; 11] = [
    Integrated {
        panel: Some("vr_builtin_guillotine_panel"),
        camera: true,
        ..sight("guillotine", "vr_builtin_guillotine", &["w_smg__budget_guillotine__base1_1413", "w_smg__budget_guillotine__base1_2436"])
    },
    Integrated {
        panel: Some("vr_builtin_hercules_panel"),
        camera: true,
        ..sight(
            "hercules",
            "vr_builtin_hercules",
            &["w_special__militech_hercules__base1_3813", "w_special__militech_hercules__base1_1840", "w_special__militech_hercules__base1_6261"],
        )
    },
    sight(
        "pozhar",
        "vr_builtin_pozhar",
        &[
            "w_rifle_precision__techtronika_pozhar__base1_5102",
            "w_rifle_precision__techtronika_pozhar__base1_1880",
            "w_rifle_precision__techtronika_pozhar__base1_01_screen",
            "w_rifle_precision__techtronika_pozhar__base1_01_screen7450",
        ],
    ),
    sight(
        "pozhar_xmod",
        "vr_builtin_pozhar_xmod",
        &[
            "w_rifle_precision__techtronika_pozhar__base1_03_indicator_front",
            "w_rifle_precision__techtronika_pozhar__base1_03_indicator_front3711",
            "w_rifle_precision__techtronika_pozhar__base1_03_holo",
            "w_rifle_precision__techtronika_pozhar__base1_01_screen7450",
        ],
    ),
    sight("ticon", "vr_builtin_ticon", &["w_handgun__militech_ticon__base1_8364", "w_handgun__militech_ticon__base1_7021"]),
    Integrated {
        attachment: true,
        ..sight("dante", "vr_builtin_dante", &["WorldWidget0856", "WorldWidget0352", "w_att__scope_short_05_7842", "w_att__scope_short_05_2507"])
    },
    Integrated { scope_offset: Some([0.0, 0.057893, 0.159352]), ..sight("shingen", "vr_builtin_shingen", &[]) },
    Integrated { scope_offset: Some([0.0, 0.0, 0.07]), ..sight("yukimura", "vr_builtin_yukimura", &[]) },
    Integrated { scope_offset: Some([0.0, 0.0678, 0.06661]), ..sight("ajax", "vr_builtin_ajax", &["on_weapon_ui"]) },
    Integrated { scope_offset: Some([0.0, -0.00365, 0.0724]), ..sight("burya", "vr_builtin_burya", &["w_revolver__techtronika_burya__base1_8120"]) },
    Integrated {
        keep_eye_relief: true,
        ..sight(
            "senkoh",
            "vr_builtin_senkoh",
            &["w_smg__senkoh_lx__base1_01_indicator_front", "w_smg__senkoh_lx__base1_01_indicator_front6451", "w_smg__senkoh_lx__base1_01_indicator_front5314"],
        )
    },
];
/// A skinned aperture's live frame (main's optics/frames.lua, from the packed Osprey surface's inverse bind): the
/// slot on the component that follows the bone, and the aperture's point and rotation in that bone's space.
struct Frame {
    key: &'static str,
    component: &'static str,
    slot: &'static str,
    point: V3,
    rotation: [f64; 4],
}
static FRAMES: [Frame; 1] = [Frame {
    key: "osprey",
    component: "FX_slots",
    slot: "vrp_scope_hinge",
    point: [0.02750940060000004, -0.026794045500000058, -0.014363877499999962],
    rotation: [0.7071067811865472, -5.028220693957672E-17, -9.579808849567385E-17, 0.7071067811865478],
}];
fn frame_of(key: Option<&str>) -> Option<&'static Frame> {
    key.and_then(|k| FRAMES.iter().find(|f| f.key == k))
}
/// optics/visibility.lua's housing: a shader scope's cap whose housing variant keeps the optical channel open (the
/// native cap and the port's variant).
fn housing(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "w_att__scope_long_01_5273" => Some((
            "base\\weapons\\firearms\\attachments\\scope\\w_att__scope_long_01\\entities\\meshes\\w_att__scope_long_01_01_cap.mesh",
            "base\\vrport\\optics\\long_01_housing.mesh",
        )),
        "w_att__scope_long_02_6688" => Some((
            "base\\weapons\\firearms\\attachments\\scope\\w_att__scope_long_02\\entities\\meshes\\w_att__scope_long_02_01_cap.mesh",
            "base\\vrport\\optics\\long_02_housing.mesh",
        )),
        _ => None,
    }
}
/// Kyubi's and Hawk's shared panels scale only their outer dot widget, Senkoh LX its three layers (the brackets, range
/// and ammunition at the native scale): (the panel component, the widget's path name, the scale).
const KYUBI_SCALE: &[(&str, &str, f32)] = &[("on_weapon_ui", "dot", 0.40), ("on_weapon_ui1321", "dot", 0.40)];
const SENKOH_SCALE: &[(&str, &str, f32)] = &[("on_weapon_ui", "Layer1", 0.55), ("on_weapon_ui7353", "Layer2", 0.55), ("on_weapon_ui7227", "Layer3", 0.55)];

struct Original {
    mask: Option<u64>,
    visibility: Option<u64>,
    enabled: bool,
    /// A housing swapped to its variant: the native cap's path and the variant's (the component's mesh hash then).
    shell_variant: bool,
    shell_source: &'static str,
    shell_hash: u64,
}
impl Original {
    fn new(mask: Option<u64>, visibility: Option<u64>, enabled: bool) -> Self {
        Original { mask, visibility, enabled, shell_variant: false, shell_source: "", shell_hash: 0 }
    }
}
/// A panel widget scaled: the widget, its scale before and the scale set.
struct PanelScale {
    widget: Handle,
    original: [f32; 2],
    applied: [f32; 2],
}
struct State {
    elapsed: f64,
    weapon_id: Option<u64>,
    originals: std::collections::HashMap<String, Original>,
    camera_on: bool,
    next_idle_camera_check: f64,
    hidden_widgets: Vec<Handle>,
    widget_key: Option<String>,
    widget_hide: f64,
    applied: Option<String>,
    profile: Option<usize>,
    /// The weapon's built-in sight in use (INTEGRATED), the native parts it hid and their visibility before.
    integrated: Option<usize>,
    integrated_visibility: std::collections::HashMap<String, bool>,
    /// short_05 installed (None before the first scan) and WorldWidget0856's visibility before it was.
    short05: Option<bool>,
    short05_visibility: std::collections::HashMap<String, bool>,
    /// The component whose slot follows the aperture's bone (FRAMES), and whether the hinge is requested.
    optic_slot: Option<Handle>,
    hinge_requested: bool,
    /// The panel scales applied (by panel) and which rules (1 Kyubi, 2 Senkoh).
    panel_scales: std::collections::HashMap<&'static str, PanelScale>,
    scale_rules: u8,
    low_app: Option<String>,
    smart: bool,
    path: Path,
    magnification: f64,
    // the sight
    looking: bool,
    active: bool,
    synced: bool,
    hold: f64,
    approach: f64,
    o: V3,
    f: Option<V3>,
    line_on: bool,
    dot: bool,
    camera_frame: Option<bool>,
    published: Option<i32>,
    error: Option<String>,
    reported: bool,
}
// SAFETY: the handles are made, used and dropped on the game's main thread, where the mechanics tick.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static CLOCK: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
fn clock() -> f64 {
    CLOCK.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

fn h(o: &Handle, name: &str, values: &[Value]) -> R<Handle> {
    // SAFETY: the caller's live object, held.
    unsafe { call_handle(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn m<T: crate::script_call::Ret>(o: &Handle, name: &str, values: &[Value]) -> R<T> {
    // SAFETY: as above.
    unsafe { method::<T>(o.instance, name, values) }.ok_or_else(|| format!("{name} failed"))
}
fn is_a(o: &Handle, class: &str) -> bool {
    !o.is_null() && m::<bool>(o, "IsA", &[Value::U64(CName::hashed(class).0)]).unwrap_or(false)
}
fn name_text(c: &Handle) -> String {
    m::<u64>(c, "GetName", &[]).map(|n| CName(n).text().to_string()).unwrap_or_default()
}
fn toggle(c: &Handle, value: bool) -> R<()> {
    if !c.is_null() && m::<bool>(c, "IsEnabled", &[])? != value {
        m::<()>(c, "Toggle", &[Value::Bool(value)])?;
    }
    Ok(())
}
/// A component's property, in place.
fn prop(o: &Handle, name: &str) -> R<*mut c_void> {
    // SAFETY: the object, held; its property by RTTI.
    unsafe { crate::script_call::property(o.instance, name) }.ok_or_else(|| format!("no {name}"))
}
fn read_u64(o: &Handle, name: &str) -> R<u64> {
    // SAFETY: an 8-byte property (Uint64, CName).
    Ok(unsafe { *(prop(o, name)? as *const u64) })
}
fn write_u64(o: &Handle, name: &str, v: u64) -> R<()> {
    // SAFETY: as above.
    unsafe { *(prop(o, name)? as *mut u64) = v };
    Ok(())
}
fn v3_of(v: [f32; 4]) -> V3 {
    [v[0] as f64, v[1] as f64, v[2] as f64]
}
fn qmul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let (ai, aj, ak, ar) = (a[0], a[1], a[2], a[3]);
    let (bi, bj, bk, br) = (b[0], b[1], b[2], b[3]);
    [ar * bi + ai * br + aj * bk - ak * bj, ar * bj - ai * bk + aj * br + ak * bi, ar * bk + ai * bj - aj * bi + ak * br, ar * br - ai * bi - aj * bj - ak * bk]
}
fn rotate(q: [f64; 4], v: V3) -> V3 {
    let t = qmul(q, [v[0], v[1], v[2], 0.0]);
    let r = qmul(t, [-q[0], -q[1], -q[2], q[3]]);
    [r[0], r[1], r[2]]
}
fn q64(q: [f32; 4]) -> [f64; 4] {
    [q[0] as f64, q[1] as f64, q[2] as f64, q[3] as f64]
}

fn shader_zoom(mag: f64) -> f64 {
    if mag <= 1.0 {
        return 1.0;
    }
    let t = ((mag - SPLIT_LO.0) / (SPLIT_HI.0 - SPLIT_LO.0).max(1e-3)).clamp(0.0, 1.0);
    mag.min(SPLIT_LO.1 + (SPLIT_HI.1 - SPLIT_LO.1) * t)
}
/// policy.lua's select().
fn select(has_sight: bool, collimator: bool, sniper: bool, magnification: f64) -> (Path, f64) {
    if !has_sight {
        return (Path::None, 1.0);
    }
    if collimator {
        return (Path::Collimator, 1.0);
    }
    if !magnification.is_finite() || magnification < 1.0 {
        return (Path::None, 1.0);
    }
    if sniper || magnification >= CAMERA_THRESHOLD {
        return (Path::Camera, magnification);
    }
    (Path::Shader, magnification)
}
fn camera_fov(magnification: f64) -> f64 {
    let half = (IMAGE_COVERAGE_DEGREES * 0.5).to_radians().tan();
    (2.0 * (half / magnification).atan()).to_degrees()
}
fn low_appearance(magnification: f64) -> String {
    format!("z{:03}", ((magnification * 100.0 + 0.5).floor() as i64).clamp(100, 399))
}

/// The sight in use: the authored scope's key, or the built-in sight's.
fn profile_key(s: &State) -> Option<&'static str> {
    s.profile.map(|p| PROFILES[p].key).or_else(|| s.integrated.map(|g| INTEGRATED[g].key))
}
/// The sight's authored aperture centre (the weapon's scope offset otherwise).
fn authored_offset(s: &State) -> Option<V3> {
    s.profile.and_then(|p| PROFILES[p].scope_offset).or_else(|| s.integrated.and_then(|g| INTEGRATED[g].scope_offset))
}
/// A slot component's world transform of `slot`: the position (fixed point, 1/131072 m) and the orientation.
fn slot_transform(component: &Handle, slot: &str) -> Option<(V3, [f64; 4])> {
    let mut t = [0u8; 0x20];
    // SAFETY: the slot component, held; the transform's storage for the out parameter.
    let found = !component.is_null()
        && unsafe { method::<bool>(component.instance, "GetSlotTransform", &[Value::U64(CName::hashed(slot).0), Value::Out(t.as_mut_ptr().cast())]) }.unwrap_or(false);
    if !found {
        return None;
    }
    let fixed = |k: usize| i32::from_le_bytes(t[k * 4..k * 4 + 4].try_into().expect("4 bytes")) as f32 as f64 / 131072.0;
    let fq = |k: usize| f32::from_le_bytes(t[0x10 + k * 4..0x14 + k * 4].try_into().expect("4 bytes")) as f64;
    Some(([fixed(0), fixed(1), fixed(2)], [fq(0), fq(1), fq(2), fq(3)]))
}
/// opticalFrame: the sight's point and orientation. A skinned aperture follows its live bone (FRAMES: the Osprey's
/// screen is on scope_hinge, and its mesh centre alone describes the bind pose, beside the screen once the hinge has
/// moved); any other sight the weapon's frame with the authored centre or the weapon's scope offset.
fn optical_frame(s: &State, weapon: &Handle) -> R<(V3, [f64; 4])> {
    if let Some(binding) = frame_of(profile_key(s)) {
        let component = s.optic_slot.as_ref().ok_or("Optic bone slot component unavailable")?;
        let (p, q) = slot_transform(component, binding.slot).ok_or_else(|| format!("Optic bone slot unavailable: {}", binding.slot))?;
        let offset = rotate(q, binding.point);
        return Ok(([p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]], qmul(q, binding.rotation)));
    }
    let q = q64(m::<[f32; 4]>(weapon, "GetWorldOrientation", &[])?);
    let p = v3_of(m::<[f32; 4]>(weapon, "GetWorldPosition", &[])?);
    let authored = match authored_offset(s) {
        Some(a) => a,
        None => v3_of(m::<[f32; 4]>(weapon, "GetScopeOffset", &[])?),
    };
    let offset = rotate(q, authored);
    Ok(([p[0] + offset[0], p[1] + offset[1], p[2] + offset[2]], q))
}
/// The Osprey's screen hinge held open while it is aimed through (VRPhysicalOpticsHinge, every frame), let go once.
fn hinge(s: &mut State, weapon: Option<&Handle>, aiming: bool) {
    let enabled = aiming && weapon.is_some() && profile_key(s) == Some("osprey") && s.optic_slot.is_some();
    if !enabled && !s.hinge_requested {
        return;
    }
    let none = Handle::null();
    let accepted = crate::optic_hinge::request(if enabled { weapon.unwrap_or(&none) } else { &none }, enabled);
    s.hinge_requested = accepted && enabled;
}
/// An item's TweakDBID (an ItemID's first word), without its flat offset.
fn item_tdbid(item: &crate::script_call::Owned) -> u64 {
    // SAFETY: an ItemID result.
    unsafe { *(item.as_ptr() as *const u64) & 0xFF_FFFF_FFFF }
}
fn tdbid(name: &str) -> u64 {
    red4ext_abi::tweakdbid::from_name(name) & 0xFF_FFFF_FFFF
}
/// attachedShort05: the weapon's scope slot holds short_05.
fn attached_short05(player: &Handle, weapon: &Handle, mounted: bool) -> bool {
    use super::lobby_rtti::{self as rt, A};
    if !mounted || player.is_null() {
        return false;
    }
    let Some(id) = rt::call(weapon, "GetItemID", &[]) else { return false };
    let ts = super::game_system("GetTransactionSystem");
    let data = rt::obj(&ts, "GetItemData", &[A::H(player), A::P(id.as_ptr().cast())]);
    if data.is_null() {
        return false;
    }
    let Some((_, outs)) = rt::method_outs(&data, "GetItemPart", &[A::Name(red4ext_abi::tweakdbid::from_name("AttachmentSlots.Scope"))], 1) else { return false };
    let Some(part) = outs.into_iter().next() else { return false };
    rt::static_call("InnerItemData", "GetItemID", &[A::P(part.as_ptr().cast())]).is_some_and(|i| item_tdbid(&i) == tdbid("Items.w_att_scope_short_05"))
}
fn scale_of(widget: &Handle) -> Option<[f32; 2]> {
    m::<[f32; 2]>(widget, "GetScale", &[]).ok()
}
fn set_scale(widget: &Handle, v: [f32; 2]) {
    // SAFETY: the widget, held; the value lives for the call.
    unsafe { method_typed::<()>(widget.instance, "SetScale", &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) };
}
/// restorePanelScale: the scale put back while it is still ours.
fn restore_panel_scale(saved: &PanelScale) {
    if scale_of(&saved.widget) == Some(saved.applied) {
        set_scale(&saved.widget, saved.original);
    }
}
fn clear_panel_scales(s: &mut State) {
    for saved in s.panel_scales.values() {
        restore_panel_scale(saved);
    }
    s.panel_scales.clear();
    s.scale_rules = 0;
}
/// fitPanelArtwork: on an open-sight Kyubi, Hawk or Senkoh LX, the panels' dot widgets scaled (each scan: the widgets
/// spawn asynchronously and are rebuilt).
fn fit_panel_artwork(s: &mut State, components: &std::collections::HashMap<String, Handle>, weapon: &Handle, mounted: bool) {
    let mut rules = 0u8;
    if !mounted {
        let name = super::lobby_rtti::call(weapon, "GetItemID", &[]).and_then(|i| crate::tdb_names::name_of(item_tdbid(&i))).unwrap_or_default();
        if name.contains("Preset_Kyubi") {
            rules = 1;
        } else if name.contains("Preset_Senkoh") {
            rules = 2;
        }
    }
    if rules != s.scale_rules {
        clear_panel_scales(s);
        s.scale_rules = rules;
    }
    let table = match rules {
        1 => KYUBI_SCALE,
        2 => SENKOH_SCALE,
        _ => return,
    };
    for &(panel_name, path, factor) in table {
        let widget = components
            .get(panel_name)
            .filter(|p| m::<bool>(p, "IsEnabled", &[]).unwrap_or(false))
            .and_then(|p| h(p, "GetWidget", &[]).ok())
            .filter(|r| !r.is_null())
            .and_then(|root| h(&root, "GetWidgetByPathName", &[Value::U64(CName::hashed(path).0)]).ok())
            .filter(|w| !w.is_null());
        let original = widget.as_ref().and_then(scale_of);
        match (widget, original) {
            (Some(widget), Some(original)) => {
                if s.panel_scales.get(panel_name).is_some_and(|saved| saved.applied == original) {
                    // the same native widget again
                    if let Some(saved) = s.panel_scales.get_mut(panel_name) {
                        saved.widget = widget;
                    }
                } else {
                    if let Some(saved) = s.panel_scales.get(panel_name) {
                        restore_panel_scale(saved);
                    }
                    let applied = [original[0] * factor, original[1] * factor];
                    set_scale(&widget, applied);
                    s.panel_scales.insert(panel_name, PanelScale { widget, original, applied });
                }
            }
            _ => {
                if let Some(saved) = s.panel_scales.remove(panel_name) {
                    restore_panel_scale(&saved);
                }
            }
        }
    }
}

fn player() -> Handle {
    let mut p = Handle::null();
    // SAFETY: GetPlayer(GameInstance) into a handle of ours.
    unsafe { crate::anim_lookup::get_player(&mut p) };
    p
}
fn camera_off(s: &mut State, player: &Handle) {
    let now = clock();
    if !player.is_null() && (s.camera_on || now >= s.next_idle_camera_check) {
        s.next_idle_camera_check = now + IDLE_CAMERA_CHECK;
        if let Ok(c) = h(player, "FindComponentByName", &[Value::U64(CName::hashed("vr_optics_camera").0)]) {
            let _ = toggle(&c, false);
        }
    }
    s.camera_on = false;
}
fn walk_widgets(widget: &Handle, depth: i32, visit: &mut dyn FnMut(&Handle)) {
    if widget.is_null() {
        return;
    }
    visit(widget);
    if depth < 6 && is_a(widget, "inkCompoundWidget") {
        for i in 0..m::<i32>(widget, "GetNumChildren", &[]).unwrap_or(0) {
            if let Ok(child) = h(widget, "GetWidgetByIndex", &[Value::Int32(i)]) {
                walk_widgets(&child, depth + 1, visit);
            }
        }
    }
}
fn scope_widgets(s: &mut State, weapon: &Handle, key: &str) {
    let hide = hide_names(key);
    if hide.is_empty() {
        return;
    }
    for c in crate::engine_objects::entity_components(weapon) {
        if is_a(&c, "WorldWidgetComponent") && m::<bool>(&c, "IsEnabled", &[]).unwrap_or(false) {
            if let Ok(root) = h(&c, "GetWidget", &[]) {
                walk_widgets(&root, 0, &mut |w| {
                    if hide.contains(&name_text(w).as_str()) && m::<bool>(w, "IsVisible", &[]).unwrap_or(false) {
                        let _ = m::<()>(w, "SetVisible", &[Value::Bool(false)]);
                        s.hidden_widgets.push(w.clone());
                    }
                });
            }
        }
    }
}
fn update_widgets(s: &mut State, weapon: &Handle, delta: f64) {
    let Some(p) = s.profile else { return };
    let key = PROFILES[p].key;
    if hide_names(key).is_empty() {
        return;
    }
    let wkey = format!("{}:{:?}", s.weapon_id.unwrap_or(0), s.path);
    if s.widget_key.as_deref() != Some(wkey.as_str()) {
        s.widget_key = Some(wkey);
        s.widget_hide = HIDE_EVERY - HIDE_FIRST;
    }
    s.widget_hide += delta;
    if s.widget_hide >= HIDE_EVERY {
        s.widget_hide = 0.0;
        scope_widgets(s, weapon, key);
    }
}
fn restore_weapon(s: &mut State) {
    hinge(s, None, false);
    clear_panel_scales(s);
    for w in s.hidden_widgets.drain(..) {
        let _ = m::<()>(&w, "SetVisible", &[Value::Bool(true)]);
    }
    s.widget_key = None;
    // A live entity resolved again: no cached component outlives an equipment swap or a save.
    let live = s.weapon_id.and_then(|id| super::find_entity(id).ok()).filter(|w| !w.is_null());
    if let Some(weapon) = &live {
        if !s.short05_visibility.is_empty() {
            for c in crate::engine_objects::entity_components(weapon) {
                if let Some(&enabled) = s.short05_visibility.get(&name_text(&c)) {
                    let _ = toggle(&c, enabled);
                }
            }
        }
    }
    s.short05_visibility.clear();
    if let Some(id) = s.weapon_id {
        if let Ok(weapon) = super::find_entity(id) {
            if !weapon.is_null() {
                for c in crate::engine_objects::entity_components(&weapon) {
                    let name = name_text(&c);
                    if let Some(&native) = s.integrated_visibility.get(&name) {
                        let _ = toggle(&c, native);
                    }
                    if let Some(saved) = s.originals.get(&name) {
                        // a housing still on its variant goes back to the native cap
                        if saved.shell_variant && crate::engine_objects::component_resource(&c) == Some(saved.shell_hash) {
                            crate::engine_objects::component_change_resource(&c, saved.shell_source, false);
                        }
                        if let Some(mask) = saved.mask {
                            let _ = write_u64(&c, "chunkMask", mask);
                        }
                        if let Some(v) = saved.visibility {
                            let _ = write_u64(&c, "visibilityAnimationParam", v);
                        }
                        let _ = toggle(&c, saved.enabled);
                        if saved.mask.is_some() {
                            crate::engine_objects::component_refresh_appearance(&c);
                        }
                    }
                    if name == "vr_optic_low"
                        || name == "vr_optic_surface"
                        || name == "vr_optic_panel"
                        || name.starts_with("vr_optic_reticle_")
                        || name.starts_with("vr_builtin_")
                    {
                        let _ = toggle(&c, false);
                    }
                }
            }
        }
    }
    s.originals.clear();
    s.weapon_id = None;
    s.applied = None;
    s.profile = None;
    s.low_app = None;
    s.smart = false;
    s.optic_slot = None;
    s.integrated = None;
    s.integrated_visibility.clear();
}
fn set_translation(panel: &Handle, x: f32, y: f32) {
    if let Ok(root) = h(panel, "GetWidget", &[]) {
        if !root.is_null() {
            let v = [x, y];
            // SAFETY: the widget, held; the value lives for the call.
            unsafe { method_typed::<()>(root.instance, "SetTranslation", &["Vector2"], &[Value::Ptr(v.as_ptr().cast())]) };
        }
    }
}
/// A record's Type() enum value (ItemType_Record, WeaponEvolution_Record).
fn record_type(record: &Handle) -> R<i64> {
    // SAFETY: the record, held; an enum result read at its size.
    m::<i32>(record, "Type", &[]).map(|v| v as i64)
}
fn scan(s: &mut State, player: &Handle, weapon: &Handle) -> R<()> {
    let id = if weapon.is_null() { None } else { m::<u64>(weapon, "GetEntityID", &[]).ok() };
    if id != s.weapon_id {
        restore_weapon(s);
        s.weapon_id = id;
    }
    if weapon.is_null() || !m::<bool>(weapon, "IsRanged", &[])? {
        s.path = Path::None;
        camera_off(s, player);
        return Ok(());
    }
    let list: Vec<(String, Handle)> = crate::engine_objects::entity_components(weapon).into_iter().map(|c| (name_text(&c), c)).collect();
    let (mut profile, mut main, mut integrated, mut attachment) = (None, None, None, None);
    for (name, c) in &list {
        if let Some(i) = PROFILES.iter().position(|p| p.mesh == name) {
            profile = Some(i);
            main = Some(c.clone());
        }
        if let Some(i) = INTEGRATED.iter().position(|g| g.surface == name) {
            if INTEGRATED[i].attachment {
                attachment = Some(i);
            } else {
                integrated = Some(i);
            }
        }
    }
    let components: std::collections::HashMap<String, Handle> = list.iter().cloned().collect();
    let mounted = m::<bool>(weapon, "HasScope", &[])?;
    let short05 = attached_short05(player, weapon, mounted);
    if s.short05 != Some(short05) {
        s.short05 = Some(short05);
        let widget = components.get("WorldWidget0856");
        if short05 {
            if let Some(w) = widget {
                s.short05_visibility.insert("WorldWidget0856".into(), m::<bool>(w, "IsEnabled", &[])?);
                toggle(w, false)?;
            }
        } else if let Some(&was) = s.short05_visibility.get("WorldWidget0856") {
            if let Some(w) = widget {
                toggle(w, was)?;
            }
            s.short05_visibility.clear();
        }
    }
    // A built-in sight: not under a mounted scope; short_05's own only on Dante or with that scope installed; never
    // with an authored scope, nor a camera sight without its panel.
    if mounted {
        integrated = None;
    }
    let item = super::lobby_rtti::call(weapon, "GetItemID", &[]).map_or(0, |i| item_tdbid(&i));
    if attachment.is_some() {
        integrated = (item == tdbid("Items.Preset_Overture_Dante") || short05).then_some(attachment).flatten();
    }
    if profile.is_some() {
        integrated = None;
    }
    if integrated.is_some_and(|g| INTEGRATED[g].panel.is_some_and(|p| !components.contains_key(p))) {
        integrated = None;
    }
    fit_panel_artwork(s, &components, weapon, mounted || integrated.is_some() || profile.is_some());
    if s.integrated != integrated {
        for (name, enabled) in s.integrated_visibility.drain() {
            if let Some(c) = components.get(&name) {
                toggle(c, enabled)?;
            }
        }
        s.integrated = integrated;
    }
    for (name, c) in &list {
        let hidden_by = integrated.filter(|&g| {
            INTEGRATED[g].hide.contains(&name.as_str()) || (INTEGRATED[g].key == "dante" && (name == "WorldWidget0856" || name == "WorldWidget0352"))
        });
        if name.starts_with("vr_builtin_") {
            toggle(c, integrated.is_some_and(|g| name == INTEGRATED[g].surface || Some(name.as_str()) == INTEGRATED[g].panel))?;
        } else if hidden_by.is_some() {
            if !s.integrated_visibility.contains_key(name) {
                s.integrated_visibility.insert(name.clone(), m::<bool>(c, "IsEnabled", &[])?);
            }
            toggle(c, false)?;
        } else if is_a(c, "entSkinnedMeshComponent") || is_a(c, "entPhysicalMeshComponent") {
            // The native ADS gates: both tracks carry physical geometry (some caps are the whole exterior housing), so
            // they stay on; the port's reticle panels are chosen apart once the optical path is (optics/visibility.lua).
            let vis = CName(read_u64(c, "visibilityAnimationParam")?);
            if vis == CName::hashed("visibleWhenAiming") || vis == CName::hashed("invisibleWhenAiming") {
                if !s.originals.contains_key(name) {
                    s.originals.insert(name.clone(), Original::new(None, Some(vis.0), m::<bool>(c, "IsEnabled", &[])?));
                }
                write_u64(c, "visibilityAnimationParam", 0)?;
                toggle(c, true)?;
            }
        }
    }
    let stats = super::game_system("GetStatsSystem");
    let stat = crate::script_call::enum_value("gamedataStatType", "ZoomLevel").ok_or("no ZoomLevel")?;
    let object_id = [id.unwrap_or(0), 0u64];
    // SAFETY: the stats system with the StatsObjectID (the entity's hash) in place.
    let zoom = if stats.is_null() {
        return Err("no stats system".into());
    } else {
        unsafe { method::<f32>(stats.instance, "GetStatValue", &[Value::Ptr(object_id.as_ptr().cast()), Value::U64(stat as u64)]) }.ok_or("GetStatValue failed")? as f64
    };
    let record = h(weapon, "GetWeaponRecord", &[])?;
    let item_type = h(&record, "ItemType", &[])?;
    let sniper = record_type(&item_type)? == crate::script_call::enum_value("gamedataItemType", "Wea_SniperRifle").ok_or("no Wea_SniperRifle")?;
    // A smart sniper (Ashura) keeps the game's own ADS zoom and HUD on the trigger.
    let evolution = h(&record, "Evolution", &[])?;
    s.smart = sniper
        && !evolution.is_null()
        && record_type(&evolution)? == crate::script_call::enum_value("gamedataWeaponEvolution", "Smart").ok_or("no Smart")?;
    let (mut path, mut mag) = select(mounted || sniper, profile.is_none() && !sniper, sniper, zoom);
    if profile.is_none() && path == Path::Camera {
        path = Path::Native;
    }
    if let Some(g) = integrated {
        // A built-in sight: its camera display at the weapon's zoom, or a collimator.
        (path, mag) = if INTEGRATED[g].camera { (Path::Camera, zoom.max(1.0)) } else { (Path::Collimator, 1.0) };
    }
    s.path = path;
    s.magnification = mag;
    s.profile = profile;
    if let Some(g) = integrated {
        s.error = None;
        if s.path != Path::Camera {
            camera_off(s, player);
        }
        if !s.reported {
            s.reported = true;
            log!("[mechanics] optics: weapon scanned from Rust (path {:?}, magnification {mag:.2}, built-in sight {})\n", s.path, INTEGRATED[g].key);
        }
        return Ok(());
    }
    s.optic_slot = frame_of(profile.map(|p| PROFILES[p].key)).and_then(|f| components.get(f.component).cloned());
    let (low, surface, panel) = (components.get("vr_optic_low").cloned(), components.get("vr_optic_surface").cloned(), components.get("vr_optic_panel").cloned());
    if let Some(pi) = profile {
        let p = &PROFILES[pi];
        if (low.is_none() && !p.camera_only) || surface.is_none() || panel.is_none() {
            s.path = Path::Native;
            s.error = Some(format!("Missing authored optical surface: {}", p.key));
            camera_off(s, player);
            return Ok(());
        }
        // A camera panel stands in front of the native cap; a shader lens looks through it, so the two housings swap
        // to their variants with an open optical channel (and back off the shader path).
        for (part, c) in &list {
            let Some((source, variant)) = housing(part) else { continue };
            let Some(saved) = s.originals.get_mut(part) else { continue };
            let hash = crate::engine_objects::component_resource(c).unwrap_or(0);
            if saved.shell_variant && hash != saved.shell_hash {
                saved.shell_variant = false;
            }
            if saved.shell_variant != (path == Path::Shader) {
                let use_variant = path == Path::Shader;
                if crate::engine_objects::component_change_resource(c, if use_variant { variant } else { source }, false) {
                    saved.shell_source = source;
                    saved.shell_variant = use_variant;
                    saved.shell_hash = crate::engine_objects::component_resource(c).unwrap_or(0);
                }
            }
        }
        let (surface, panel) = (surface.expect("checked"), panel.expect("checked"));
        let main = main.expect("found with the profile");
        let name = name_text(&main);
        let mask_now = read_u64(&main, "chunkMask")?;
        let o = s.originals.entry(name.clone()).or_insert(Original::new(Some(mask_now), None, m::<bool>(&main, "IsEnabled", &[])?));
        if o.mask.is_none() {
            o.mask = Some(mask_now);
        }
        // The lens takes what the view's own zoom leaves of the magnification.
        let lens_mag = if path == Path::Shader { mag / shader_zoom(mag) } else { mag };
        let appearance = low_appearance(lens_mag);
        let applied = format!("{}:{:?}:{}", p.key, path, appearance);
        let low_app_now = low.as_ref().and_then(|l| read_u64(l, "meshAppearance").ok()).map(|a| CName(a).text().to_string());
        let missing = mask_now != p.body_mask
            || low.as_ref().is_some_and(|l| m::<bool>(l, "IsEnabled", &[]).unwrap_or(false) != (path == Path::Shader))
            || m::<bool>(&surface, "IsEnabled", &[])? != (path == Path::Camera)
            || m::<bool>(&panel, "IsEnabled", &[])? != (path == Path::Camera)
            || (low.is_some() && path == Path::Shader && low_app_now.as_deref() != Some(s.low_app.as_deref().unwrap_or(&appearance)));
        if s.applied.as_deref() != Some(applied.as_str()) || missing {
            write_u64(&main, "chunkMask", p.body_mask)?;
            crate::engine_objects::component_refresh_appearance(&main);
            if let Some(low) = &low {
                toggle(low, false)?;
                if path == Path::Shader {
                    s.low_app = Some(appearance.clone());
                    if !crate::engine_objects::component_change_appearance(low, CName::hashed(&appearance).0, true) {
                        return Err(format!("Cannot select lens magnification {appearance}"));
                    }
                    toggle(low, true)?;
                }
            }
            toggle(&surface, path == Path::Camera)?;
            toggle(&panel, path == Path::Camera)?;
            s.applied = Some(applied);
        }
        if let Some(op) = p.original_panel {
            if let Some(original) = components.get(op) {
                let enabled = m::<bool>(original, "IsEnabled", &[])?;
                s.originals.entry(op.to_string()).or_insert(Original::new(None, None, enabled));
                toggle(original, false)?;
            }
        }
        // The port's reticle surfaces carry the native panels on both magnified paths.
        let ours = path == Path::Camera || path == Path::Shader;
        for layer in p.reticles {
            for name in [layer.original_panel, layer.original_mesh] {
                if let Some(original) = components.get(name) {
                    let enabled = m::<bool>(original, "IsEnabled", &[])?;
                    let was = s.originals.entry(name.to_string()).or_insert(Original::new(None, None, enabled)).enabled;
                    toggle(original, !ours && was)?;
                }
            }
            if let Some(c) = components.get(layer.surface) {
                toggle(c, ours)?;
            }
            if let Some(c) = components.get(layer.panel) {
                toggle(c, ours)?;
                // the camera shader maps the authored canvas into the lens, the Osprey's rear panel too
                set_translation(c, 0.0, 0.0);
            }
        }
    } else {
        s.applied = None;
    }
    if path != Path::Camera {
        camera_off(s, player);
    }
    s.error = None;
    if !s.reported {
        s.reported = true;
        log!("[mechanics] optics: weapon scanned from Rust (path {:?}, magnification {mag:.2}, profile {})\n", s.path, profile.map_or("none", |p| PROFILES[p].key));
    }
    Ok(())
}

/// The nearer eye's distance to the sight's line while it is behind the sight, how far behind, the sight's axis and
/// the sight's point (the camera already carries the approach applied: the eye taken without it).
fn eye_to_axis(s: &State, weapon: &Handle, max_behind: f64) -> R<(Option<(f64, f64)>, V3, V3)> {
    let cam = super::game_system("GetCameraSystem");
    if cam.is_null() {
        return Err("no camera system".into());
    }
    let (o, _) = super::view::camera()?.ok_or("no camera")?;
    let c = [o[0] - s.o[0], o[1] - s.o[1], o[2] - s.o[2]];
    let right = v3_of(m::<[f32; 4]>(&cam, "GetActiveCameraRight", &[])?);
    // SAFETY: the shared slots' read.
    // (Lua's `GetVRSharedSlot(95) or 0.032` keeps a 0: only nil falls back.)
    let half = unsafe { cvr_rig::vrik::provider::shared_slot(95) } as f64;
    let (point, q) = optical_frame(s, weapon)?;
    let f = rotate(q, [0.0, 1.0, 0.0]);
    // The active camera is MAIN: one eye or the head centre; GetVRMainEyeOffset says which.
    // SAFETY: the eye offset's read.
    let main_eye = unsafe { cvr_rig::vrik::provider::main_eye_offset() } as f64;
    let mut best: Option<(f64, f64)> = None;
    for side in [-1.0, 1.0] {
        let along = half * side - main_eye;
        let v = [c[0] + right[0] * along - point[0], c[1] + right[1] * along - point[1], c[2] + right[2] * along - point[2]];
        let behind = -(v[0] * f[0] + v[1] * f[1] + v[2] * f[2]);
        if behind > -0.03 && behind < max_behind {
            let l = [v[0] + f[0] * behind, v[1] + f[1] * behind, v[2] + f[2] * behind];
            let d = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, behind));
            }
        }
    }
    Ok((best, f, point))
}
/// The shot leaves the sight line while the eye is on a sight: the bore-to-line offset, perpendicular to the barrel.
fn sight_line(s: &mut State, point: Option<V3>, f: V3) {
    let Some(point) = point else {
        if s.line_on {
            cvr_rig::vrik::provider::set_sight_line(0.0, 0.0, 0.0);
            s.line_on = false;
        }
        return;
    };
    // SAFETY: the shared slots' read.
    let mz = unsafe { [cvr_rig::vrik::provider::shared_slot(200), cvr_rig::vrik::provider::shared_slot(201), cvr_rig::vrik::provider::shared_slot(202)] }.map(|v| v as f64);
    if mz[0] * mz[0] + mz[1] * mz[1] + mz[2] * mz[2] < 1.0 {
        return;
    }
    let v = [point[0] - mz[0], point[1] - mz[1], point[2] - mz[2]];
    let along = v[0] * f[0] + v[1] * f[1] + v[2] * f[2];
    cvr_rig::vrik::provider::set_sight_line((v[0] - f[0] * along) as f32, (v[1] - f[1] * along) as f32, (v[2] - f[2] * along) as f32);
    s.line_on = true;
}
/// A new instance of a class, owned by its handle.
fn new_object(class: &str) -> R<(Handle, &'static Class)> {
    let c = crate::script_call::class_any(class).ok_or_else(|| format!("no {class}"))?;
    // SAFETY: a new instance owned by the handle made for it.
    Ok((unsafe { Handle::from_instance(c.create_instance()) }, c))
}
fn set_field<T: Copy>(o: &Handle, name: &str, v: T) -> R<()> {
    // SAFETY: a plain property of the type written.
    unsafe { *(prop(o, name)? as *mut T) = v };
    Ok(())
}
/// The zoom the aim state machine would send, at `level` and without the weapon's own aim FOV; both features re-sent.
fn send_sight_zoom(player: &Handle, level: f64, zoomed: bool, aiming: bool) -> R<()> {
    let (zoom, _) = new_object("AnimFeature_Zoom")?;
    let cam = super::game_system("GetCameraSystem");
    let fov = m::<f32>(&cam, "GetActiveCameraFOV", &[])?;
    set_field(&zoom, "finalZoomLevel", level as f32)?;
    set_field(&zoom, "weaponZoomLevel", 1.0f32)?;
    set_field(&zoom, "weaponScopeFov", fov)?;
    set_field(&zoom, "weaponAimFOV", fov)?;
    set_field(&zoom, "worldFOV", fov)?;
    set_field(&zoom, "zoomLevelNum", 0i32)?;
    set_field(&zoom, "noWeaponAimInTime", 0.2f32)?;
    set_field(&zoom, "noWeaponAimOutTime", 0.2f32)?;
    set_field(&zoom, "shouldUseWeaponZoomStats", false)?;
    set_field(&zoom, "focusModeActive", false)?;
    let apply = crate::script_call::static_function("AnimationControllerComponent", "ApplyFeatureToReplicate").ok_or("no ApplyFeatureToReplicate")?;
    // SAFETY: the static with the player and the feature held.
    unsafe { crate::script_call::invoke::<()>(core::ptr::null_mut(), apply, &[Value::Handle(player), Value::U64(CName::hashed("ZoomAnimData").0), Value::Handle(&zoom)]) }
        .ok_or("ApplyFeatureToReplicate failed")?;
    if aiming {
        return Ok(());
    }
    let (aim, _) = new_object("AnimFeature_AimPlayer")?;
    let unaimed = crate::script_call::enum_value("animAimState", "Unaimed").ok_or("no animAimState")?;
    let aimed = crate::script_call::enum_value("animAimState", "Aimed").ok_or("no animAimState")?;
    m::<()>(&aim, "SetAimState", &[Value::U64(unaimed as u64)])?;
    m::<()>(&aim, "SetZoomState", &[Value::U64(if zoomed { aimed } else { unaimed } as u64)])?;
    m::<()>(&aim, "SetAimInTime", &[Value::Float(0.2)])?;
    m::<()>(&aim, "SetAimOutTime", &[Value::Float(0.2)])?;
    // SAFETY: as above.
    unsafe { crate::script_call::invoke::<()>(core::ptr::null_mut(), apply, &[Value::Handle(player), Value::U64(CName::hashed("AnimFeature_AimPlayer").0), Value::Handle(&aim)]) }
        .ok_or("ApplyFeatureToReplicate failed")?;
    Ok(())
}
fn sight_dot(s: &mut State, on: bool) {
    if s.dot != on {
        cvr_rig::vrik::provider::set_sight_dot(on);
        s.dot = on;
    }
}
fn sync_weapon_layer(s: &mut State, on: bool) {
    if s.synced != on {
        cvr_rig::vrik::provider::set_sight_zoom(on);
        s.synced = on;
    }
}
/// weapon None (or a path without open sights) lets the zoom go.
fn update_sight_zoom(s: &mut State, player: &Handle, weapon: Option<&Handle>, delta: f64) -> R<()> {
    let mut level = 1.0;
    let rule = weapon.filter(|_| !s.smart).and_then(|_| s.path.behind());
    let (mut scanning, mut aiming) = (false, false);
    if rule.is_some() || s.active {
        use super::boards::{definition, field, get, local_board};
        let defs = super::boards::defs()?;
        let psm_def = definition(&defs, "PlayerStateMachine")?;
        let board = local_board(m::<u64>(player, "GetEntityID", &[])?, psm_def)?;
        if board.is_null() {
            return Err("no state machine board".into());
        }
        scanning = get::<i32>(&board, "GetInt", field(psm_def, "Vision")?)? == 1;
        aiming = get::<i32>(&board, "GetInt", field(psm_def, "UpperBody")?)? == 6;
    }
    let owned = scanning || (aiming && s.smart);
    hinge(s, weapon, aiming && !owned && rule.is_some());
    let mut approach = 0.0;
    if let (Some(behind_max), Some(weapon), false) = (rule, weapon, owned) {
        let (best, f, point) = eye_to_axis(s, weapon, behind_max).unwrap_or((None, [0.0; 3], [0.0; 3]));
        let d = best.map(|b| b.0);
        s.looking = d.is_some_and(|d| d < if s.looking { LEAVE_AXIS } else { ENTER_AXIS });
        // A built-in display on an open-sight weapon only changes the image: the shot keeps its own line.
        let native_line = s.integrated.is_none_or(|g| INTEGRATED[g].attachment);
        sight_line(s, (s.looking && native_line).then_some(point), f);
        if s.looking && !s.integrated.is_some_and(|g| INTEGRATED[g].keep_eye_relief) {
            // A split shader scope is brought closer by the view zoom itself; the eye is not moved in as well.
            let split = s.path == Path::Shader && shader_zoom(s.magnification) > 1.0;
            if split {
                level = shader_zoom(s.magnification);
            } else {
                let behind = best.expect("looking").1;
                approach = APPROACH_MAX.min(behind - APPROACH_RELIEF.max(behind * APPROACH_KEEP)).max(0.0);
                s.f = Some(f);
            }
        }
    } else {
        s.looking = false;
        sight_line(s, None, [0.0; 3]);
    }
    sight_dot(s, false);
    // Eased both ways; leaving, the last axis carries it back out.
    s.approach += (approach - s.approach) * (delta / APPROACH_TIME).min(1.0);
    if (s.approach - approach).abs() < 0.0005 {
        s.approach = approach;
    }
    let a = if s.f.is_some() { s.approach } else { 0.0 };
    let f = s.f.unwrap_or([0.0; 3]);
    let o = [f[0] * a, f[1] * a, f[2] * a];
    if o != s.o {
        cvr_rig::vrik::provider::set_sight_approach(o[0] as f32, o[1] as f32, o[2] as f32);
        s.o = o;
    }
    if level > 1.0 {
        sync_weapon_layer(s, true);
        s.hold = RELEASE_HOLD;
        send_sight_zoom(player, level, true, aiming)?;
        s.active = true;
        return Ok(());
    }
    if s.active {
        // Back to what the state machine shows outside aiming; the scanner and a smart sniper send their own.
        if !owned {
            send_sight_zoom(player, 1.0, false, aiming)?;
        }
        s.active = false;
    }
    if s.synced {
        s.hold -= delta;
        if s.hold <= 0.0 {
            sync_weapon_layer(s, false);
        }
    }
    Ok(())
}
/// The camera path: vr_optics_camera at the sight, in the WeaponRight slot's frame, with the magnification's FOV.
fn place_camera(s: &mut State, player: &Handle, weapon: &Handle) -> R<()> {
    // SAFETY: the readiness flag's read.
    if !unsafe { cvr_rig::vrik::provider::optics_camera_ready() } {
        camera_off(s, player);
        return Ok(());
    }
    let camera = h(player, "FindComponentByName", &[Value::U64(CName::hashed("vr_optics_camera").0)])?;
    if camera.is_null() {
        s.error = Some("Optics camera missing".into());
        return Ok(());
    }
    let (point, q) = match optical_frame(s, weapon) {
        Ok(frame) => frame,
        Err(why) => {
            s.error = Some(why);
            camera_off(s, player);
            return Ok(());
        }
    };
    if s.camera_frame.is_none() {
        // SAFETY: the camera component's handle property.
        let bound = unsafe { crate::script_call::property(camera.instance, "parentTransform") }.is_some_and(|p| unsafe { !(*(p as *const Handle)).is_null() });
        s.camera_frame = Some(bound);
    }
    let (mut ep, mut eq) = (v3_of(m::<[f32; 4]>(player, "GetWorldPosition", &[])?), q64(m::<[f32; 4]>(player, "GetWorldOrientation", &[])?));
    if s.camera_frame == Some(true) {
        let slots = h(player, "FindComponentByName", &[Value::U64(CName::hashed("ItemAttachmentSlots").0)])?;
        let Some((p, q)) = slot_transform(&slots, "WeaponRight") else {
            s.error = Some("WeaponRight slot unavailable".into());
            return Ok(());
        };
        (ep, eq) = (p, q);
    }
    let inverse = [-eq[0], -eq[1], -eq[2], eq[3]];
    // 0.2 m ahead of the sight's point along its frame
    let ahead = rotate(q, [0.0, 0.2, 0.0]);
    let mut world = [point[0] + ahead[0], point[1] + ahead[1], point[2] + ahead[2]];
    if s.profile.is_some_and(|p| PROFILES[p].front_camera) {
        // Rasetsu's rear LCD is not its optical sensor: the sensor at the native weapon front, at the LCD's height.
        let wp = v3_of(m::<[f32; 4]>(weapon, "GetWorldPosition", &[])?);
        let mut offset = match authored_offset(s) {
            Some(a) => a,
            None => v3_of(m::<[f32; 4]>(weapon, "GetScopeOffset", &[])?),
        };
        offset[1] += 0.2;
        offset[1] = offset[1].max(m::<[f32; 4]>(weapon, "GetMuzzleOffset", &[])?[1] as f64 + 0.02);
        let moved = rotate(q, offset);
        world = [wp[0] + moved[0], wp[1] + moved[1], wp[2] + moved[2]];
    }
    let lp = rotate(inverse, [world[0] - ep[0], world[1] - ep[1], world[2] - ep[2]]);
    let pos = [lp[0] as f32, lp[1] as f32, lp[2] as f32, 0.0];
    let rot = qmul(inverse, q).map(|v| v as f32);
    // SAFETY: the camera component, held; the values live for the call.
    unsafe { method::<()>(camera.instance, "SetLocalTransform", &[Value::Ptr(pos.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast())]) }.ok_or("SetLocalTransform failed")?;
    set_field(&camera, "fov", camera_fov(s.magnification) as f32)?;
    toggle(&camera, true)?;
    s.camera_on = true;
    Ok(())
}

fn update(s: &mut State, delta: f64) -> R<()> {
    let player = player();
    if player.is_null() {
        hinge(s, None, false);
        let path = s.published;
        *s = fresh();
        s.published = path;
        sync_weapon_layer_force_off();
        cvr_rig::vrik::provider::set_sight_dot(false);
        cvr_rig::vrik::provider::set_sight_line(0.0, 0.0, 0.0);
        cvr_rig::vrik::provider::set_sight_approach(0.0, 0.0, 0.0);
        return Ok(());
    }
    let waiting = !super::vrcam_select::settled();
    // SAFETY: the session flag's read.
    if !unsafe { cvr_rig::vrik::provider::optics_enabled() } || waiting {
        camera_off(s, &player);
        let _ = update_sight_zoom(s, &player, None, delta);
        if s.weapon_id.is_some() {
            restore_weapon(s);
        }
        s.path = Path::None;
        return Ok(());
    }
    // CET's player:GetActiveWeapon() is the static ScriptedPuppet.GetActiveWeapon(obj) (a static found on the class
    // chain takes the object as its first argument); its wref result locked.
    let f = crate::script_call::static_function("ScriptedPuppet", "GetActiveWeapon").ok_or("no ScriptedPuppet.GetActiveWeapon")?;
    // SAFETY: the scripted static with the player held.
    let weapon = unsafe { crate::script_call::invoke_handle(core::ptr::null_mut(), f, &[Value::Handle(&player)]) }.ok_or("GetActiveWeapon failed")?;
    s.elapsed += delta;
    let id = if weapon.is_null() { None } else { m::<u64>(&weapon, "GetEntityID", &[]).ok() };
    if id != s.weapon_id || s.elapsed >= 0.2 {
        s.elapsed = 0.0;
        scan(s, &player, &weapon)?;
    }
    let weapon = (!weapon.is_null()).then_some(weapon);
    if let Err(e) = update_sight_zoom(s, &player, weapon.as_ref(), delta) {
        s.error = Some(e);
    }
    if let (Some(w), Some(_), Path::Shader | Path::Camera) = (&weapon, s.profile, s.path) {
        update_widgets(s, w, delta);
    }
    if s.path != Path::Camera {
        return Ok(());
    }
    if let Some(w) = weapon {
        place_camera(s, &player, &w)?;
    }
    Ok(())
}
fn sync_weapon_layer_force_off() {
    cvr_rig::vrik::provider::set_sight_zoom(false);
}
fn fresh() -> State {
    State {
        elapsed: 0.0,
        weapon_id: None,
        originals: Default::default(),
        camera_on: false,
        next_idle_camera_check: 0.0,
        hidden_widgets: Vec::new(),
        widget_key: None,
        widget_hide: 0.0,
        applied: None,
        profile: None,
        integrated: None,
        integrated_visibility: Default::default(),
        short05: None,
        short05_visibility: Default::default(),
        optic_slot: None,
        hinge_requested: false,
        panel_scales: Default::default(),
        scale_rules: 0,
        low_app: None,
        smart: false,
        path: Path::None,
        magnification: 1.0,
        looking: false,
        active: false,
        synced: false,
        hold: 0.0,
        approach: 0.0,
        o: [0.0; 3],
        f: None,
        line_on: false,
        dot: false,
        camera_frame: None,
        published: None,
        error: None,
        reported: false,
    }
}

/// The mod's onUpdate: the update, then the path published.
pub(in crate::mechanics) fn tick(dt: f32) {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let s = guard.get_or_insert_with(fresh);
    if let Err(e) = update(s, f64::from(dt)) {
        if s.error.as_deref() != Some(e.as_str()) {
            log!("[mechanics] optics: {e}\n");
        }
        s.error = Some(e);
    }
    let code = s.path.code();
    if s.published != Some(code) {
        cvr_rig::vrik::provider::set_optics_path(code);
        s.published = Some(code);
    }
}
/// The mod's onShutdown.
pub(in crate::mechanics) fn shutdown() {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = guard.as_mut() {
        hinge(s, None, false);
        let p = player();
        if !p.is_null() {
            camera_off(s, &p);
            restore_weapon(s);
        }
    }
    cvr_rig::vrik::provider::set_optics_path(0);
    cvr_rig::vrik::provider::set_sight_approach(0.0, 0.0, 0.0);
    cvr_rig::vrik::provider::set_sight_line(0.0, 0.0, 0.0);
    cvr_rig::vrik::provider::set_sight_dot(false);
    cvr_rig::vrik::provider::set_sight_zoom(false);
}
