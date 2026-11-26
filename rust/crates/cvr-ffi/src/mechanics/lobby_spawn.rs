//! The lobby's plumbing: modules/spawner.lua (templates spawned through the plugin's VRSpawn* natives, tagged VRLobby
//! and their kind), modules/sound.lua (the logo's whoosh and glitch, the ambient loop, the menu's clicks, the intro's
//! hum, all at the game's volume sliders), modules/beams.lua (the two pointer beams, a strip of fading rectangles on a
//! spawned UI quad turned to the eye), the natives with array or string results, and the world frames of the panels.
use super::lobby_ink::{self as ink, C3};
use crate::script_call::{call_global_handle, call_native, invoke_owned, Owned, Value};
use red4ext_abi::cname::CName;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::RttiSystem;

pub(super) type V3 = [f64; 3];
pub(super) type Q = [f64; 4];
pub(super) const TAG: &str = "VRLobby";

pub(super) fn native<T: crate::script_call::Ret>(name: &str, values: &[Value]) -> Option<T> {
    // SAFETY: the plugin's natives, on the main thread.
    unsafe { call_native::<T>(name, values) }
}
pub(super) fn native_handle(name: &str, values: &[Value]) -> Handle {
    // SAFETY: a registered global function returning a handle.
    unsafe { call_global_handle(core::ptr::null_mut(), name, values) }.unwrap_or_else(Handle::null)
}
pub(super) fn native_owned(name: &str, values: &[Value]) -> Option<Owned> {
    let f = RttiSystem::get()?.get_function(CName::hashed(name))?;
    // SAFETY: a registered global function, no context; the result kept while read.
    unsafe { invoke_owned(core::ptr::null_mut(), &f.base, values) }
}
pub(super) fn native_v4s(name: &str, values: &[Value]) -> Vec<[f32; 4]> {
    native_owned(name, values)
        // SAFETY: a DynArray of Vector4.
        .map(|o| unsafe { &*(o.as_ptr() as *const red4ext_abi::dynarray::DynArray<[f32; 4]>) }.as_slice().to_vec())
        .unwrap_or_default()
}
pub(super) fn native_string(name: &str, values: &[Value]) -> Option<String> {
    let o = native_owned(name, values)?;
    // SAFETY: the String result.
    Some(unsafe { (*(o.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string() })
}
/// A global function called with one text argument made as its parameter's own type through the type's FromString
/// (GetLocalizedText's parameter is not a plain String: a String handed in place crashed the engine), its result read
/// as text (a String, or a CName's text). The parameter's type is reported once per function.
pub(super) fn global_text(name: &str, arg: &str) -> Option<String> {
    super::lobby_rtti::breadcrumb(|| format!("global_text {name}"));
    static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let f = RttiSystem::get()?.get_function(CName::hashed(name))?;
    let p = *f.base.params.as_slice().first()?;
    // SAFETY: the function's own first parameter and its type (types live as long as the game).
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*(*p).ty };
    {
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.iter().any(|n| n == name) {
            seen.push(name.to_string());
            cvr_host::log!("[VRLobby] {name}'s parameter is a {} (from Rust)\n", ty.name().text());
        }
    }
    let s = Str::new(arg);
    let tname = ty.name().text().to_string();
    let r = if tname.starts_with("script_ref:") {
        // ScriptRef<String>: 0x10 bytes, the inner type, the pointer to the value, a CName (RED4ext's ScriptRef).
        let inner = RttiSystem::get()?.get_type(CName::hashed("String"))? as *const red4ext_abi::rtti::IType as usize;
        let mut sref = [0usize; 5];
        sref[2] = inner;
        sref[3] = (&s.0 as *const red4ext_abi::cstring::CString) as usize;
        // SAFETY: the global function with a script reference to our String.
        unsafe { invoke_owned(core::ptr::null_mut(), &f.base, &[Value::Ptr(sref.as_ptr().cast())]) }?
    } else if tname == "CName" {
        // the name's hash itself (the CName type's FromString reads its text as another layout than a CString: the
        // engine hashed through a wild pointer -- GetLocalizedTextByKey crashed so)
        // SAFETY: the global function with a CName by value.
        unsafe { invoke_owned(core::ptr::null_mut(), &f.base, &[Value::U64(CName::hashed(arg).0)]) }?
    } else {
        let mut owned = Owned::of_type(ty)?;
        // SAFETY: a constructed value of the type, parsed from the text by the type itself.
        if !unsafe { ty.from_string(owned.as_mut_ptr().cast(), &s.0) } {
            return None;
        }
        // SAFETY: the global function with a value of its own parameter type.
        unsafe { invoke_owned(core::ptr::null_mut(), &f.base, &[Value::Ptr(owned.as_ptr().cast())]) }?
    };
    let rty = r.ty().name().text().to_string();
    // SAFETY: the result read by its own type.
    Some(unsafe {
        match rty.as_str() {
            "CName" => CName(*(r.as_ptr() as *const u64)).text().to_string(),
            _ => (*(r.as_ptr() as *const red4ext_abi::cstring::CString)).as_str().to_string(),
        }
    })
}
/// A script String argument, kept while the call runs.
pub(super) struct Str(red4ext_abi::cstring::CString, #[allow(dead_code)] std::ffi::CString);
impl Str {
    pub(super) fn new(text: &str) -> Str {
        let c = std::ffi::CString::new(text.replace('\0', "")).unwrap_or_default();
        let e = red4ext_abi::cstring::CString::new(&c).unwrap_or_else(|| red4ext_abi::cstring::CString::new(c"").expect("empty string"));
        Str(e, c)
    }
    pub(super) fn v(&self) -> Value {
        Value::Ptr((&self.0 as *const red4ext_abi::cstring::CString).cast())
    }
}
pub(super) fn cn(s: &str) -> Value {
    Value::U64(CName::hashed(s).0)
}

// ---------------------------------------------------------------- spawner.lua
/// The CName array argument of VRSpawnTemplate's tags, made by the parameter's own type.
fn tags(func: &str, names: &[&str]) -> Option<Owned> {
    let f = RttiSystem::get()?.get_function(CName::hashed(func))?;
    // SAFETY: the function's own fifth parameter type.
    let ty: &'static red4ext_abi::rtti::IType = unsafe { &*(**f.base.params.as_slice().get(4)?).ty };
    let mut owned = Owned::of_type(ty)?;
    // SAFETY: the array value of this type, resized by its own type and filled element by element.
    unsafe {
        let at = owned.as_mut_ptr().cast();
        if !ty.array_resize(at, names.len() as u32) {
            return None;
        }
        for (i, n) in names.iter().enumerate() {
            *(ty.array_element(at, i as u32) as *mut u64) = CName::hashed(n).0;
        }
    }
    Some(owned)
}
/// Spawner.spawn: the template at the pose, tagged VRLobby and `kind`; its entity id (None when the spawn failed).
pub(super) fn spawn(path: &str, p: V3, q: Q, kind: &str) -> Option<u64> {
    spawn_tagged(path, "", p, q, &[TAG, kind])
}
/// VRSpawnTemplate: the template in an appearance ("" none) at the pose with the tags; its entity id.
pub(super) fn spawn_tagged(path: &str, appearance: &str, p: V3, q: Q, names: &[&str]) -> Option<u64> {
    let t = tags("VRSpawnTemplate", names)?;
    let s = Str::new(path);
    let pos = [p[0] as f32, p[1] as f32, p[2] as f32, 1.0];
    let rot = [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32];
    let id = native::<u64>("VRSpawnTemplate", &[s.v(), cn(appearance), Value::Ptr(pos.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast()), Value::Ptr(t.as_ptr().cast())])?;
    (id != 0).then_some(id)
}
/// VRSpawnRecord: a character record spawned at the pose, tagged VRLobby and `kind`.
pub(super) fn spawn_record(record: &str, p: V3, q: Q, kind: &str) -> Option<u64> {
    let t = tags("VRSpawnRecord", &[TAG, kind])?;
    let pos = [p[0] as f32, p[1] as f32, p[2] as f32, 1.0];
    let rot = [q[0] as f32, q[1] as f32, q[2] as f32, q[3] as f32];
    let id = native::<u64>(
        "VRSpawnRecord",
        &[Value::U64(red4ext_abi::tweakdbid::from_name(record)), cn(""), Value::Ptr(pos.as_ptr().cast()), Value::Ptr(rot.as_ptr().cast()), Value::Ptr(t.as_ptr().cast())],
    )?;
    (id != 0).then_some(id)
}
pub(super) fn delete(id: Option<u64>) {
    if let Some(id) = id {
        native::<bool>("VRSpawnDelete", &[Value::U64(id)]);
    }
}
pub(super) fn clear(kind: &str) {
    native::<i32>("VRSpawnDeleteTagged", &[cn(kind)]);
}
pub(super) fn spawned(id: Option<u64>) -> bool {
    id.is_some_and(|id| !native_handle("VRSpawnedEntity", &[Value::U64(id)]).is_null())
}
pub(super) fn alive(id: Option<u64>) -> bool {
    id.is_some_and(|id| native::<bool>("VRSpawnManaged", &[Value::U64(id)]) == Some(true))
}
pub(super) fn find_entity(id: Option<u64>) -> Handle {
    id.and_then(|id| super::find_entity(id).ok()).unwrap_or_else(Handle::null)
}
pub(super) fn find_comp(e: &Handle, name: &str) -> Handle {
    ink::child(e, "FindComponentByName", &[cn(name)])
}
pub(super) fn set_world(e: &Handle, p: V3, q: Q) -> bool {
    !e.is_null() && crate::engine_objects::entity_set_world_transform(e, &super::reload_core::world_bytes(p, q))
}
/// The UI quad spawned for a panel: its entity, the UI component and the component's root widget.
#[derive(Default, Clone)]
pub(super) struct Surface {
    pub id: Option<u64>,
    pub entity: Handle,
    pub comp: Handle,
    pub root: Handle,
}
impl Surface {
    /// resolve: the entity, its UI component and the widget once spawned.
    pub(super) fn resolve(&mut self, ui: &str) -> bool {
        if self.id.is_none() {
            return false;
        }
        if !self.root.is_null() {
            return true;
        }
        self.entity = find_entity(self.id);
        if !self.entity.is_null() {
            self.comp = find_comp(&self.entity, ui);
        }
        if !self.comp.is_null() {
            self.root = ink::child(&self.comp, "GetWidget", &[]);
        }
        !self.root.is_null()
    }
    pub(super) fn painted(&self) -> bool {
        native::<bool>("VRMenuSurfaceReady", &[Value::Handle(&self.comp), Value::Handle(&self.root)]) == Some(true)
    }
    /// The quad's display component switched on.
    pub(super) fn display_on(&self) -> bool {
        let d = find_comp(&self.entity, "display");
        !d.is_null() && super::belt::toggle(&d, true)
    }
}

// ---------------------------------------------------------------- frames in the world
pub(super) fn norm(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l > 1e-6 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        [0.0, 0.0, 1.0]
    }
}
pub(super) fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(super) fn mul(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
pub(super) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
pub(super) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
pub(super) fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
/// quatFromBasis: the rotation whose columns are X, Y, Z.
pub(super) fn quat_from_basis(x: V3, y: V3, z: V3) -> Q {
    let (m00, m01, m02) = (x[0], y[0], z[0]);
    let (m10, m11, m12) = (x[1], y[1], z[1]);
    let (m20, m21, m22) = (x[2], y[2], z[2]);
    let tr = m00 + m11 + m22;
    if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [(m21 - m12) / s, (m02 - m20) / s, (m10 - m01) / s, 0.25 * s]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
        [0.25 * s, (m01 + m10) / s, (m02 + m20) / s, (m21 - m12) / s]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
        [(m01 + m10) / s, 0.25 * s, (m12 + m21) / s, (m02 - m20) / s]
    } else {
        let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
        [(m02 + m20) / s, (m12 + m21) / s, 0.25 * s, (m10 - m01) / s]
    }
}
/// A quad's pose with its centre at `c`, right `r` and up `u` (the UI quads face back along r x u).
pub(super) fn quad_pose(r: V3, u: V3) -> Q {
    quat_from_basis(mul(r, -1.0), mul(u, -1.0), cross(r, u))
}
pub(super) fn view_world() -> Option<(V3, Q)> {
    let wp = native::<[f32; 4]>("VRViewWorldPos", &[]).filter(|v| v[3] > 0.5)?;
    let q = native::<[f32; 4]>("VRViewWorldRot", &[])?;
    Some(([f64::from(wp[0]), f64::from(wp[1]), f64::from(wp[2])], q.map(f64::from)))
}
pub(super) fn haptic(hand: usize, kind: i32) {
    native::<()>("VRHapticEvent", &[Value::Int32(kind), Value::Int32(if hand == 2 { 2 } else { 1 })]);
}

// ---------------------------------------------------------------- sound.lua
const DIR: &str = "CyberpunkVRPort_Lobby/sounds/";
const MENU_DIR: &str = "CyberpunkVRPort_Menu/sounds/";
struct Snd {
    file: String,
    volume: f64,
    lowpass: f64,
    loop_from: f64,
    loop_to: f64,
    crossfade: f64,
    music: bool,
}
fn snd(file: String, volume: f64) -> Snd {
    Snd { file, volume, lowpass: 0.0, loop_from: -1.0, loop_to: 0.0, crossfade: 0.0, music: false }
}
const HOVER_GAP: f64 = 0.06;
const AMBIENT_IN: f64 = 4.0;
const AMBIENT_OUT: f64 = 2.0;
const GLITCH_OUT: f64 = 0.35;
#[derive(Default)]
pub(super) struct Sound {
    hum: bool,
    glitch: bool,
    ambient: bool,
    sliders: Option<(f64, f64)>,
    clock: f64,
    hover_at: Option<std::time::Instant>,
}
fn slider(name: &str) -> f64 {
    // the volumes are ConfigVarInt: read by the result's own type (as a Float its 100 was 1.4e-43, every lobby sound
    // played silent)
    let v = ink::setting_var("/audio/volume", name);
    super::lobby_rtti::num(&v, "GetValue", &[]).map_or(1.0, |n| n.clamp(0.0, 100.0) / 100.0)
}
fn read_sliders() -> (f64, f64) {
    let master = slider("MasterVolume");
    let (music, sfx) = (master * slider("MusicVolume"), master * slider("SfxVolume"));
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| cvr_host::log!("[VRLobby] sound volumes from the game's settings: master {master:.2}, music {music:.2}, effects {sfx:.2} (from Rust)\n"));
    (music, sfx)
}
fn ambient_snd() -> Snd {
    Snd { file: format!("{DIR}ambient.ogg"), volume: 0.3, lowpass: 0.0, loop_from: 30.0, loop_to: 172.0, crossfade: 3.0, music: true }
}
impl Sound {
    fn volume_of(&mut self, s: &Snd) -> f64 {
        let (music, fx) = *self.sliders.get_or_insert_with(read_sliders);
        s.volume * if s.music { music } else { fx }
    }
    fn channel(&mut self, name: &str, s: Option<&Snd>, fade: f64) -> bool {
        let ch = Str::new(name);
        let r = match s {
            None => {
                let e = Str::new("");
                native::<bool>(
                    "VRSoundChannel",
                    &[ch.v(), e.v(), Value::Float(0.0), Value::Float(0.0), Value::Float(-1.0), Value::Float(0.0), Value::Float(0.0), Value::Float(fade as f32)],
                )
            }
            Some(s) => {
                let v = self.volume_of(s);
                let f = Str::new(&s.file);
                native::<bool>(
                    "VRSoundChannel",
                    &[
                        ch.v(),
                        f.v(),
                        Value::Float(v as f32),
                        Value::Float(s.lowpass as f32),
                        Value::Float(s.loop_from as f32),
                        Value::Float(s.loop_to as f32),
                        Value::Float(s.crossfade as f32),
                        Value::Float(fade as f32),
                    ],
                )
            }
        };
        r == Some(true)
    }
    fn once(&mut self, s: &Snd) {
        let v = self.volume_of(s);
        let f = Str::new(&s.file);
        native::<bool>("VRMenuPlayWav", &[f.v(), Value::Float(v as f32)]);
    }
    fn hum_raw(&mut self, on: bool) {
        let (_, fx) = *self.sliders.get_or_insert_with(read_sliders);
        native::<()>("VRIntroHum", &[Value::Bool(on), Value::Float(fx as f32)]);
    }
    pub(super) fn hum(&mut self, on: bool) {
        if on == self.hum {
            return;
        }
        self.hum = on;
        self.hum_raw(on);
    }
    pub(super) fn logo(&mut self) {
        self.once(&snd(format!("{DIR}logo_whoosh.ogg"), 0.8));
        self.hum(false);
    }
    pub(super) fn glitch(&mut self, on: bool) {
        if on == self.glitch {
            return;
        }
        self.glitch = on;
        let g = snd(format!("{DIR}logo_glitch.ogg"), 0.6);
        self.channel("lobby_glitch", on.then_some(&g), if on { 0.0 } else { GLITCH_OUT });
    }
    /// M.menu: the panel's sounds (appear, hover -- not faster than 60 ms -- click, open, close).
    pub(super) fn menu(&mut self, kind: &str) {
        let s = match kind {
            "appear" => snd(format!("{DIR}menu_appear.ogg"), 0.7),
            "hover" => snd(format!("{DIR}menu_hover.ogg"), 1.0),
            "click" => snd(format!("{MENU_DIR}click.wav"), 0.8),
            "open" => snd(format!("{MENU_DIR}open.wav"), 0.8),
            "close" => snd(format!("{MENU_DIR}close.wav"), 0.8),
            _ => return,
        };
        if kind == "hover" {
            let now = std::time::Instant::now();
            if self.hover_at.is_some_and(|t| now.duration_since(t).as_secs_f64() < HOVER_GAP) {
                return;
            }
            self.hover_at = Some(now);
        }
        self.once(&s);
    }
    pub(super) fn ambient_on() -> bool {
        native::<bool>("VRLobbyAmbient", &[]) != Some(false)
    }
    pub(super) fn set_ambient_on(on: bool) {
        native::<()>("VRSetLobbyAmbient", &[Value::Bool(on)]);
    }
    pub(super) fn ambient(&mut self, wanted: bool) {
        let on = wanted && Sound::ambient_on();
        if on == self.ambient {
            return;
        }
        self.ambient = on;
        let a = ambient_snd();
        self.channel("lobby_ambient", on.then_some(&a), if on { AMBIENT_IN } else { AMBIENT_OUT });
    }
    pub(super) fn update(&mut self, dt: f64) {
        self.clock -= dt;
        if self.clock > 0.0 {
            return;
        }
        self.clock = 1.0;
        let now = read_sliders();
        let was = self.sliders.replace(now);
        if was.is_some_and(|w| w != now) {
            if self.hum {
                self.hum_raw(true);
            }
            if self.ambient {
                self.channel("lobby_ambient", Some(&ambient_snd()), 0.3);
            }
        }
    }
}

// ---------------------------------------------------------------- beams.lua
const BEAM_ENT: &str = "vrport\\ui\\vr_menu_beam.ent";
const BEAM_TAG: &str = "VRLobbyBeam";
const MENU_UI: &str = "vrp_menu_ui";
const LEN: f64 = 1.5;
const BCW: f64 = 16.0;
const BCH: f64 = 2048.0;
const HIDE: f64 = 0.12;
const FADE: f64 = 0.12;
const STEPS: usize = 6;
#[derive(Clone, Copy)]
pub(super) struct BeamLook {
    pub glow: C3,
    pub glow_op: f64,
    pub core: C3,
    pub core_op: f64,
}
struct Piece {
    k: usize,
    x: f64,
    w: f64,
    rect: Handle,
}
#[derive(Default)]
struct Beam {
    s: Surface,
    pieces: Vec<Piece>,
    px: f64,
    ready: bool,
    shown: Option<bool>,
}
pub(super) struct Beams {
    beams: [Beam; 2],
    look: BeamLook,
}
impl Default for Beams {
    fn default() -> Beams {
        Beams { beams: Default::default(), look: BeamLook { glow: [1.0; 3], glow_op: 0.2, core: [1.0; 3], core_op: 0.6 } }
    }
}
fn beam_resolve(h: &mut Beam, look: BeamLook) -> bool {
    if h.s.id.is_none() {
        return false;
    }
    if h.s.root.is_null() {
        if !h.s.resolve(MENU_UI) {
            return false;
        }
        // SAFETY: the UI component's Float property.
        if let Some(p) = unsafe { crate::script_call::property(h.s.comp.instance, "glitchValue") } {
            unsafe { *(p as *mut f32) = 0.0 };
        }
        ink::remove_all(&h.s.root);
        (h.pieces, h.px) = (Vec::new(), -1.0);
        for k in 1..=STEPS + 1 {
            let f = if k <= STEPS { k as f64 / (STEPS as f64 + 1.0) } else { 1.0 };
            h.pieces.push(Piece { k, x: 0.0, w: BCW, rect: ink::rect(&h.s.root, 0.0, 0.0, BCW, 0.0, look.glow, look.glow_op * f) });
            h.pieces.push(Piece { k, x: (BCW - 6.0) / 2.0, w: 6.0, rect: ink::rect(&h.s.root, (BCW - 6.0) / 2.0, 0.0, 6.0, 0.0, look.core, look.core_op * f) });
        }
    }
    if !h.ready {
        if !h.s.painted() {
            return false;
        }
        h.ready = h.s.display_on();
    }
    h.ready
}
fn beam_lay(h: &mut Beam, length: f64) {
    let per_m = BCH / LEN;
    let px = length.clamp(0.0, LEN) * per_m;
    if (px - h.px).abs() < 1.0 {
        return;
    }
    h.px = px;
    let (hide, step) = (HIDE * per_m, FADE * per_m / STEPS as f64);
    for p in &h.pieces {
        let top = hide + (p.k as f64 - 1.0) * step;
        let bottom = if p.k <= STEPS { top + step } else { px };
        ink::move_to(&p.rect, p.x, top);
        ink::size(&p.rect, p.w, (bottom.min(px) - top).max(0.0));
    }
}
impl Beams {
    pub(super) fn open(&mut self, at: V3, look: Option<BeamLook>) {
        if let Some(l) = look {
            self.look = l;
        }
        for b in self.beams.iter_mut() {
            if b.s.id.is_none() {
                *b = Beam { s: Surface { id: spawn(BEAM_ENT, at, quad_pose([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), BEAM_TAG), ..Default::default() }, ..Default::default() };
            }
        }
    }
    /// M.set: beam `i` (1, 2) from the origin to the hit, its strip turned to the eye; hidden when not shown.
    pub(super) fn set(&mut self, i: usize, show: bool, origin: Option<V3>, hit: Option<V3>, eye: Option<V3>) {
        let look = self.look;
        let Some(h) = self.beams.get_mut(i - 1) else { return };
        if !beam_resolve(h, look) {
            return;
        }
        let (true, Some(origin), Some(hit), Some(eye)) = (show, origin, hit, eye) else {
            if h.shown != Some(false) {
                for p in &h.pieces {
                    ink::visible(&p.rect, false);
                }
                h.shown = Some(false);
            }
            return;
        };
        let ray = sub(hit, origin);
        let length = dot(ray, ray).sqrt();
        let dir = norm(ray);
        let to_eye = sub(eye, origin);
        let n0 = sub(to_eye, mul(dir, dot(to_eye, dir)));
        let l = dot(n0, n0).sqrt();
        if length < 1e-3 || l <= 1e-6 {
            return;
        }
        let n = mul(n0, 1.0 / l);
        set_world(&h.s.entity, add(origin, mul(dir, LEN / 2.0)), quad_pose(cross(n, dir), mul(dir, -1.0)));
        beam_lay(h, length);
        if h.shown != Some(true) {
            for p in &h.pieces {
                ink::visible(&p.rect, true);
            }
            h.shown = Some(true);
        }
    }
    pub(super) fn close(&mut self) {
        clear(BEAM_TAG);
        self.beams = Default::default();
    }
}
