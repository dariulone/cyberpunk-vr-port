//! The blade's natives (main's src/Anim/BladeContactNative.cpp): what a blade meets of a person, read from their hit
//! representation (gameHitRepresentationComponent: its appearance overrides, its own shapes, and the resource's when it
//! uses it), each capsule taken in the frame of the slot it is bound to; and the lodged blade's D6 joint, the physics
//! hands' (cvr-rig physhands::blade). The measures are cvr-physhands' blade_contact.
//!
//! The mechanics call these directly (the [`Shapes`] visitor, the sweeps); the natives are registered with main's
//! signatures for any script that calls them.
use crate::native_args::args;
use crate::script_call::{method, Value};
use core::ffi::c_void;
use cvr_physhands::blade_contact as bc;
use cvr_rig::vrik::physhands::blade as pb;
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::resource::TokenRef;
use red4ext_abi::rtti::{object_class, Class, IType, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::sync::OnceLock;

pub(crate) type V3 = [f32; 3];
pub(crate) type V4 = [f32; 4];

// ---------------------------------------------------------------- the layouts, from the classes' properties
#[derive(Clone, Copy)]
struct Layout {
    // gameHitRepresentationComponent
    resource: usize,
    overrides: usize,
    representations: usize,
    material: usize,
    use_resource: usize,
    // gameHitRepresentationOverride / gameHitRepresentationVisualTaggedOverride: the container in it, their sizes
    override_container: usize,
    override_size: usize,
    tagged_container: usize,
    tagged_size: usize,
    // gameHitRepresentationResource
    res_representations: usize,
    res_overrides: usize,
    // gameHitShapeContainer
    name: usize,
    slot: usize,
    shape: usize,
    user_data: usize,
    shape_material: usize,
    container_size: usize,
    // gameHitShapeBase, the capsule and the sphere
    local_transform: usize,
    capsule_radius: usize,
    capsule_height: usize,
    sphere_radius: usize,
    capsule: CName,
    sphere: CName,
    /// gameHitShape_OBB's dimensions (a mech's shapes are boxes) and its class name; none when the class is not there
    obb_dimensions: Option<usize>,
    obb: CName,
}
pub(crate) fn class(n: &str) -> Option<&'static Class> {
    RttiSystem::get()?.get_class(CName::hashed(n))
}
pub(crate) fn prop(c: &Class, n: &str) -> Option<usize> {
    c.property_named(n).map(|p| p.value_offset as usize)
}
fn size_of(c: &Class) -> usize {
    // SAFETY: a class is an IType.
    unsafe { (*(c as *const Class as *const IType)).size() as usize }
}
fn layout() -> Option<Layout> {
    static L: OnceLock<Option<Layout>> = OnceLock::new();
    *L.get_or_init(|| {
        let comp = class("gameHitRepresentationComponent")?;
        let ov = class("gameHitRepresentationOverride")?;
        let tagged = class("gameHitRepresentationVisualTaggedOverride")?;
        let res = class("gameHitRepresentationResource")?;
        let cont = class("gameHitShapeContainer")?;
        let base = class("gameHitShapeBase")?;
        let cap = class("gameHitShape_Capsule")?;
        let sph = class("gameHitShape_Sphere")?;
        Some(Layout {
            resource: prop(comp, "resource")?,
            overrides: prop(comp, "appearanceOverrides")?,
            representations: prop(comp, "representations")?,
            material: prop(comp, "physicsMaterial")?,
            use_resource: prop(comp, "useResourceData")?,
            override_container: prop(ov, "represenationOverride")?,
            override_size: size_of(ov),
            tagged_container: prop(tagged, "represenationOverride")?,
            tagged_size: size_of(tagged),
            res_representations: prop(res, "representations")?,
            res_overrides: prop(res, "overrides")?,
            name: prop(cont, "name")?,
            slot: prop(cont, "slotName")?,
            shape: prop(cont, "shape")?,
            user_data: prop(cont, "userData")?,
            shape_material: prop(cont, "physicsMaterial")?,
            container_size: size_of(cont),
            local_transform: prop(base, "localTransform")?,
            capsule_radius: prop(cap, "radius")?,
            capsule_height: prop(cap, "height")?,
            sphere_radius: prop(sph, "radius")?,
            capsule: cap.class_name(),
            sphere: sph.class_name(),
            obb_dimensions: class("gameHitShape_OBB").and_then(|c| prop(c, "dimensions")),
            obb: CName::hashed("gameHitShape_OBB"),
        })
    })
}
unsafe fn rd<T: Copy>(at: usize) -> T {
    // SAFETY: the caller's live object.
    unsafe { core::ptr::read_unaligned(at as *const T) }
}
/// A DynArray at `at`: its entries and size.
unsafe fn array(at: usize) -> (usize, usize) {
    // SAFETY: as above.
    unsafe { (rd::<usize>(at), rd::<u32>(at + 0xC) as usize) }
}

/// One of a hit representation's shape containers.
#[derive(Clone, Copy)]
pub(crate) struct Container(usize);
impl Container {
    pub fn name(&self) -> u64 {
        let l = layout().expect("visited");
        // SAFETY: a live container.
        unsafe { rd(self.0 + l.name) }
    }
    /// The slot it is bound to; its name when it has none.
    pub fn binding(&self) -> u64 {
        let l = layout().expect("visited");
        // SAFETY: as above.
        let slot: u64 = unsafe { rd(self.0 + l.slot) };
        if slot != 0 {
            slot
        } else {
            self.name()
        }
    }
    pub fn slot_name(&self) -> u64 {
        let l = layout().expect("visited");
        // SAFETY: as above.
        unsafe { rd(self.0 + l.slot) }
    }
    pub fn shape(&self) -> *mut c_void {
        let l = layout().expect("visited");
        // SAFETY: as above (a Handle's instance first).
        unsafe { rd(self.0 + l.shape) }
    }
    pub fn user_data(&self) -> *mut c_void {
        let l = layout().expect("visited");
        // SAFETY: as above.
        unsafe { rd(self.0 + l.user_data) }
    }
    /// The handle of the shape (a new strong reference).
    pub fn shape_handle(&self) -> Handle {
        let l = layout().expect("visited");
        // SAFETY: a Handle in place, cloned.
        unsafe { (*((self.0 + l.shape) as *const Handle)).clone() }
    }
    pub fn user_data_handle(&self) -> Handle {
        let l = layout().expect("visited");
        // SAFETY: as above.
        unsafe { (*((self.0 + l.user_data) as *const Handle)).clone() }
    }
    pub fn material(&self) -> u64 {
        let l = layout().expect("visited");
        // SAFETY: a MaterialReference: its name first.
        unsafe { rd(self.0 + l.shape_material) }
    }
    /// Its capsule (a sphere is one of no height), local to its slot: the centre with the radius in w, the axis with
    /// half the height in w. None for another shape or an implausible size.
    pub fn capsule(&self) -> Option<(V4, V4)> {
        let l = layout()?;
        let shape = self.shape();
        if shape.is_null() {
            return None;
        }
        // SAFETY: the shape's live class.
        let kind = unsafe { object_class(shape) }?.class_name();
        let s = shape as usize;
        // SAFETY: the shape's own fields.
        let (radius, height) = unsafe {
            if kind == l.capsule {
                (rd::<f32>(s + l.capsule_radius), rd::<f32>(s + l.capsule_height))
            } else if kind == l.sphere {
                (rd::<f32>(s + l.sphere_radius), 0.0)
            } else {
                return None;
            }
        };
        if !(radius > 0.0 && radius <= 1.0 && (0.0..=2.0).contains(&height)) {
            return None;
        }
        // SAFETY: the shape's local transform: a Matrix, its X row at +0 and its W row at +0x30.
        let (x, w) = unsafe { (rd::<V4>(s + l.local_transform), rd::<V4>(s + l.local_transform + 0x30)) };
        Some(([w[0], w[1], w[2], radius], [x[0], x[1], x[2], height * 0.5]))
    }
    /// A box shape (gameHitShape_OBB, a mech's): its centre and its three axes (unit) in the slot's frame, and its half
    /// extents -- `dimensions` ARE the half extents: read so, a ray from the player enters the Minotaur's Belly box
    /// exactly where the Melee ray meets its physical body (0.49 / 0.73 of two rays); read as the full size, no box is
    /// entered at all (CVR_HITBOX_OBB_SCALE overrides the factor, 1).
    pub fn obb(&self) -> Option<(V3, [V3; 3], V3)> {
        let l = layout()?;
        let at = l.obb_dimensions?;
        let shape = self.shape();
        if shape.is_null() {
            return None;
        }
        // SAFETY: the shape's live class.
        if unsafe { object_class(shape) }?.class_name() != l.obb {
            return None;
        }
        let s = shape as usize;
        let scale = obb_scale();
        // SAFETY: the shape's own fields: the dimensions and the local transform (a Matrix, rows X, Y, Z, W).
        let (d, rows) = unsafe { (rd::<V3>(s + at), [0, 1, 2, 3].map(|r| rd::<V4>(s + l.local_transform + r * 0x10))) };
        let half = d.map(|v| v * scale);
        if !half.iter().all(|h| *h > 0.0 && *h <= 4.0) {
            return None;
        }
        let mut axes = [[0.0f32; 3]; 3];
        for (k, row) in rows[..3].iter().enumerate() {
            let n = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
            if !(n > 1e-4) {
                return None;
            }
            axes[k] = [row[0] / n, row[1] / n, row[2] / n];
        }
        Some(([rows[3][0], rows[3][1], rows[3][2]], axes, half))
    }
}
fn obb_scale() -> f32 {
    static S: OnceLock<f32> = OnceLock::new();
    *S.get_or_init(|| std::env::var("CVR_HITBOX_OBB_SCALE").ok().and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.1..=2.0).contains(v)).unwrap_or(1.0))
}
/// A point of a slot's frame in a box's own frame (centre, unit axes).
fn in_box(p: &V3, center: &V3, axes: &[V3; 3]) -> V3 {
    let d = sub(p, center);
    axes.map(|a| d[0] * a[0] + d[1] * a[1] + d[2] * a[2])
}
/// The material of the component (a shape without one has it).
pub(crate) fn component_material(component: *mut c_void) -> u64 {
    let Some(l) = layout() else { return 0 };
    // SAFETY: a live component.
    unsafe { rd(component as usize + l.material) }
}
/// ShapeMaterial: the container's own physics material, else its component's.
pub(crate) fn shape_material(component: *mut c_void, c: &Container) -> u64 {
    let m = c.material();
    if m != 0 {
        m
    } else {
        component_material(component)
    }
}
/// VisitShapes: the appearance overrides first, then the component's own shapes, then -- an ordinary puppet may have
/// only a resource reference -- the resource's overrides and shapes when the component uses it or has none of its
/// own (found among the loader's tokens, else asked for; never waited on). The visitor's true stops the walk.
pub(crate) fn visit_shapes(component: *mut c_void, mut visit: impl FnMut(Container) -> bool) {
    let Some(l) = layout() else { return };
    if component.is_null() {
        return;
    }
    let c = component as usize;
    // SAFETY: a live component's arrays.
    unsafe {
        let (ov, n) = array(c + l.overrides);
        for i in 0..n {
            if visit(Container(ov + i * l.override_size + l.override_container)) {
                return;
            }
        }
        let (rep, n_local) = array(c + l.representations);
        for i in 0..n_local {
            if visit(Container(rep + i * l.container_size)) {
                return;
            }
        }
        let path: u64 = rd(c + l.resource);
        let use_resource: bool = rd::<u8>(c + l.use_resource) != 0;
        if path == 0 || !bc::uses_shape_resource(use_resource, n_local != 0) {
            return;
        }
        let mut token = TokenRef::find(path);
        if token.is_null() {
            token = TokenRef::load_async(path);
        }
        if !token.loaded() || token.resource().is_null() {
            return;
        }
        let r = token.resource() as usize;
        let (ov, n) = array(r + l.res_overrides);
        for i in 0..n {
            if visit(Container(ov + i * l.tagged_size + l.tagged_container)) {
                return;
            }
        }
        let (rep, n) = array(r + l.res_representations);
        for i in 0..n {
            if visit(Container(rep + i * l.container_size)) {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------- slots and frames
/// WorldTransform: a WorldPosition (three 1/2^17 fixed points) and its orientation at +0x10.
#[repr(C, align(16))]
#[derive(Clone, Copy, Default)]
pub(crate) struct WorldTransform(pub [u32; 8]);
impl WorldTransform {
    pub fn position(&self) -> V3 {
        [0, 1, 2].map(|i| self.0[i] as i32 as f32 / 131072.0)
    }
    pub fn orientation(&self) -> V4 {
        [4, 5, 6, 7].map(|i| f32::from_bits(self.0[i]))
    }
}
/// `component:GetSlotTransform(slot)`: the slot's world frame, None when it does not answer.
pub(crate) fn slot_frame(component: *mut c_void, slot: u64) -> Option<WorldTransform> {
    if component.is_null() {
        return None;
    }
    let mut wt = WorldTransform::default();
    // SAFETY: the component, alive for the caller's frame; the transform's storage for the out parameter.
    let found = unsafe { method::<bool>(component, "GetSlotTransform", &[Value::U64(slot), Value::Out((&mut wt as *mut WorldTransform).cast())]) };
    (found == Some(true)).then_some(wt)
}
pub(crate) fn qrot(q: &V4, v: &V3) -> V3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
pub(crate) fn conj(q: &V4) -> V4 {
    [-q[0], -q[1], -q[2], q[3]]
}
fn sub(a: &V3, b: &V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn f3(v: &V4) -> V3 {
    [v[0], v[1], v[2]]
}
/// A point in a slot's frame.
pub(crate) fn to_local(frame: &WorldTransform, p: &V3) -> V3 {
    qrot(&conj(&frame.orientation()), &sub(p, &frame.position()))
}

/// A shape the sweep crossed: its container, the slot it is bound to and that slot's frame, its capsule, the segment's
/// end pose in that frame, and the contact.
pub(crate) struct Observation {
    pub item: Container,
    pub binding: u64,
    pub frame: WorldTransform,
    pub center: V4,
    pub axis: V4,
    pub a: V4,
    pub b: V4,
    pub hit: bc::SweepContact,
}
/// VisitSweepContacts: the blade from (ends[0], ends[1]) to (ends[2], ends[3]) swept through each capsule, in its slot's
/// frame (the pinned slot's given one, `observation`); `end_grip` asks whether the blade's whole length at its end pose,
/// from the grip, is still in any. The slots that did not answer, -1 without the component.
pub(crate) fn visit_sweep_contacts(
    component: *mut c_void,
    ends: &[V3; 4],
    mut visit: impl FnMut(&Observation),
    observation: Option<(u64, WorldTransform)>,
    mut end_grip: Option<(&V3, &mut bool)>,
) -> i32 {
    if component.is_null() {
        return -1;
    }
    let mut missing = 0;
    visit_shapes(component, |item| {
        // a capsule (a person's shapes), else a box (a mech's): its centre and largest half extent, its first axis and
        // that axis' half extent stand where a capsule's centre / radius and axis / half height do
        let obb = if item.capsule().is_none() { item.obb() } else { None };
        let Some((center, axis)) = item.capsule().or_else(|| {
            obb.map(|(c, ax, h)| ([c[0], c[1], c[2], h[0].max(h[1]).max(h[2])], [ax[0][0], ax[0][1], ax[0][2], h[0]]))
        }) else {
            return false;
        };
        let binding = item.binding();
        let frame = match observation {
            Some((b, f)) if binding != 0 && binding == b => f,
            _ => match slot_frame(component, binding) {
                Some(f) => f,
                None => {
                    missing += 1;
                    return false;
                }
            },
        };
        let local = ends.map(|e| to_local(&frame, &e));
        let (c, ax) = (f3(&center), f3(&axis));
        if let Some((grip, occupied)) = end_grip.as_mut() {
            if !**occupied {
                // the heel of the shaft and every resolved region, even a neighbour the striking portion never crossed
                let g = to_local(&frame, grip);
                **occupied = match &obb {
                    Some((bc_, bax, h)) => {
                        let (g2, e2) = (in_box(&g, bc_, bax), in_box(&local[3], bc_, bax));
                        bc::sweep_box(&g2, &e2, &g2, &e2, h).depth > 0.0
                    }
                    None => bc::sweep_capsule(&g, &local[3], &g, &local[3], &c, &ax, center[3], axis[3]).depth > 0.0,
                };
            }
        }
        let hit = match &obb {
            Some((bc_, bax, h)) => {
                let b = local.map(|p| in_box(&p, bc_, bax));
                bc::sweep_box(&b[0], &b[1], &b[2], &b[3], h)
            }
            None => bc::sweep_capsule(&local[0], &local[1], &local[2], &local[3], &c, &ax, center[3], axis[3]),
        };
        if hit.depth > 0.0 {
            visit(&Observation {
                item,
                binding,
                frame,
                center,
                axis,
                a: [local[2][0], local[2][1], local[2][2], 1.0],
                b: [local[3][0], local[3][1], local[3][2], 1.0],
                hit,
            });
        }
        false
    });
    missing
}
/// ReadCapsule: the capsule a name (and the binding it must have) selects, preferring the exact user data; none when
/// clones disagree. `binding` None: the name's own (a Neck shape bound to the Head slot).
pub(crate) fn read_capsule(component: *mut c_void, name: u64, data: *mut c_void, binding: Option<u64>) -> Option<(V4, V4)> {
    let with_binding = binding.is_some();
    let binding = binding.unwrap_or(if name == CName::hashed("Neck").0 { CName::hashed("Head").0 } else { name });
    let mut lookup = bc::CapsuleLookup::default();
    visit_shapes(component, |item| {
        if item.name() != name {
            return false;
        }
        let slot = if item.slot_name() == 0 { item.name() } else { item.slot_name() };
        if slot != binding && (with_binding || slot != name) {
            return false;
        }
        let Some((c, a)) = item.capsule() else { return false };
        lookup.consider([c[0], c[1], c[2], c[3], a[0], a[1], a[2], a[3]], !data.is_null() && item.user_data() == data)
    });
    (lookup.found && !lookup.ambiguous).then(|| {
        let g = lookup.geometry;
        ([g[0], g[1], g[2], g[3]], [g[4], g[5], g[6], g[7]])
    })
}
/// SweptContact: the earliest crossing of any shape (time, where along the blade, depth, w 1); with `end_grip`, w 2
/// when every slot answered and the end pose is clear of them all.
/// Whether the hit representation has box shapes (a mech's): the melee attack's sweep is widened for them.
pub(crate) fn has_box_shapes(component: *mut c_void) -> bool {
    let mut found = false;
    visit_shapes(component, |item| {
        found = item.obb().is_some();
        found
    });
    found
}
/// The hit representation's shapes in words (tests): each container's name, slot, shape class, whether the sweep can
/// use it (capsule()), and the shape's float and vector fields; at most `max` of them.
pub(crate) fn describe_shapes(component: *mut c_void, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    visit_shapes(component, |item| {
        let shape = item.shape();
        // SAFETY: a live shape's class and its own fields, read by their RTTI types.
        let (kind, fields) = unsafe {
            match object_class(shape) {
                None => (String::from("none"), String::new()),
                Some(c) => {
                    let mut f = Vec::new();
                    let mut cls = Some(c);
                    while let Some(k) = cls {
                        for p in k.own_properties() {
                            let Some(ty) = p.ty.as_ref() else { continue };
                            let at = shape as usize + p.value_offset as usize;
                            let t = ty.name().text().to_string();
                            match t.as_str() {
                                "Float" => f.push(format!("{}={:.3}", p.name.text(), rd::<f32>(at))),
                                "Vector3" => {
                                    let v = rd::<[f32; 3]>(at);
                                    f.push(format!("{}=({:.3},{:.3},{:.3})", p.name.text(), v[0], v[1], v[2]));
                                }
                                "Vector4" => {
                                    let v = rd::<[f32; 4]>(at);
                                    f.push(format!("{}=({:.3},{:.3},{:.3},{:.3})", p.name.text(), v[0], v[1], v[2], v[3]));
                                }
                                _ => {}
                            }
                        }
                        cls = k.parent();
                    }
                    (c.class_name().text().to_string(), f.join(" "))
                }
            }
        };
        out.push(format!(
            "{} slot {} {kind} usable {} {fields}",
            CName(item.name()).text(),
            CName(item.slot_name()).text(),
            item.capsule().is_some() || item.obb().is_some()
        ));
        out.len() >= max
    });
    out
}
/// Each box shape's entry along the world segment from..to (tests): its slot's world centre and where the segment
/// enters it with `dimensions` read as the full size (x0.5) and as the half extents (x1).
pub(crate) fn obb_entries(component: *mut c_void, from: &V3, to: &V3) -> Vec<String> {
    let mut out = Vec::new();
    visit_shapes(component, |item| {
        let Some((c, axes, half)) = item.obb() else { return false };
        let Some(frame) = slot_frame(component, item.binding()) else { return false };
        let (a, b) = (in_box(&to_local(&frame, from), &c, &axes), in_box(&to_local(&frame, to), &c, &axes));
        let full = half.map(|h| h / obb_scale());
        let at = |h: &V3| bc::segment_in_box(&a, &b, h).map_or(String::from("-"), |(e, x)| format!("{e:.2}..{x:.2}"));
        let p = frame.position();
        out.push(format!(
            "{} at ({:.2},{:.2},{:.2}): entry x0.5 {} x1 {}",
            CName(item.name()).text(),
            p[0],
            p[1],
            p[2],
            at(&full.map(|h| h * 0.5)),
            at(&full)
        ));
        false
    });
    out
}
pub(crate) fn swept_contact(component: *mut c_void, ends: &[V3; 4], end_grip: Option<&V3>) -> V4 {
    let mut result = [0f32; 4];
    let mut occupied = false;
    let missing = visit_sweep_contacts(
        component,
        ends,
        |obs| {
            let h = obs.hit;
            if result[3] == 0.0 || h.time < result[0] || (h.time == result[0] && h.depth > result[2]) {
                result = [h.time, h.fraction, h.depth, 1.0];
            }
        },
        None,
        end_grip.map(|g| (g, &mut occupied)),
    );
    if end_grip.is_some() && result[3] != 0.0 && missing == 0 && !occupied {
        result[3] = 2.0;
    }
    result
}
/// EquivalentHitData: the same reflected fields (the authored reaction zone, shape type, tag, protection and weakspot
/// flags, damage modifier, dismemberment part), subclass fields included, up to gameHitShapeUserData.
pub(crate) fn equivalent_hit_data(a: *mut c_void, b: *mut c_void) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    if a == b {
        return true;
    }
    // SAFETY: live script objects' classes.
    let (Some(ta), Some(tb)) = (unsafe { object_class(a) }, unsafe { object_class(b) }) else { return false };
    if !core::ptr::eq(ta, tb) {
        return false;
    }
    let stop = CName::hashed("gameHitShapeUserData");
    let base = CName::hashed("HitShapeUserDataBase");
    let mut saw_region = false;
    let mut cls: Option<&Class> = Some(ta);
    while let Some(c) = cls {
        if c.class_name() == stop {
            break;
        }
        let props = c.own_properties();
        if c.class_name() == base && !props.is_empty() {
            saw_region = true;
        }
        for p in props {
            // SAFETY: the property's own type; its value in each instance (by its own value pointer).
            unsafe {
                let Some(ty) = p.ty.as_ref() else { return false };
                let (va, vb) = (p.value_ptr::<c_void>(a), p.value_ptr::<c_void>(b));
                if va.is_null() || vb.is_null() || !ty.is_equal(va, vb) {
                    return false;
                }
            }
        }
        cls = c.parent();
    }
    saw_region
}
/// MatchesOccupied: whether a hit's shape is the one observed in the pose: by name, binding, material, capsule and
/// equivalent data, the exact data deciding among clones.
#[allow(clippy::too_many_arguments)]
pub(crate) fn matches_occupied(component: *mut c_void, hit_name: u64, hit_data: *mut c_void, hit_material: u64, binding: u64, geometry: *mut c_void, data: *mut c_void, material: u64, center: &V4, axis: &V4) -> bool {
    if component.is_null() || geometry.is_null() || data.is_null() || hit_data.is_null() || hit_name == 0 || binding == 0 {
        return false;
    }
    let mut m = bc::CallbackShapeMatch::default();
    visit_shapes(component, |item| {
        if item.name() != hit_name {
            return false;
        }
        let slot = if item.slot_name() != 0 { item.slot_name() } else { item.name() };
        let same = slot == binding && shape_material(component, &item) == material && item.capsule().is_some_and(|(c, a)| c == *center && a == *axis) && equivalent_hit_data(item.user_data(), data);
        m.consider(same, item.user_data() == hit_data);
        false
    });
    m.matches() && (m.exact_observed || (equivalent_hit_data(hit_data, data) && hit_material != 0 && (material == 0 || hit_material == material)))
}
/// CapsuleResult: a blade (start to end, its motion, its edge) against a capsule (centre with radius, axis with half
/// height): interval -- where along it, how deep, valid in w; else depth, incidence, edge alignment, resistance.
pub(crate) fn capsule_result(start: &V4, end: &V4, motion: &V4, edge: &V4, center: &V4, axis: &V4, material: i32, interval: bool) -> V4 {
    let input = bc::ContactInput {
        blade_start: f3(start),
        blade_end: f3(end),
        motion_end: f3(motion),
        edge_direction: f3(edge),
        material: bc::Material::from_i32(material),
        ..bc::ContactInput::default()
    };
    let m = bc::evaluate_capsule(&input, &f3(center), &f3(axis), center[3], axis[3]);
    if interval {
        [m.center_fraction, m.penetration_depth, 0.0, if m.valid { 1.0 } else { 0.0 }]
    } else {
        [m.penetration_depth, m.incidence, m.edge_alignment, m.resistance]
    }
}
/// physicsTraceResult's hidden ids: the proxy (+0x48), the actor's index (+0x4C), the proxy's kind (+0x59).
pub(crate) fn trace_ids(trace: &[u8; 0x60]) -> (u32, u32, u8) {
    let u = |o: usize| u32::from_le_bytes(trace[o..o + 4].try_into().expect("4 bytes"));
    (u(0x48), u(0x4C), trace[0x59])
}
/// The entity a trace hit (Codeware's TraceResult.GetHitEntity, which the game itself does not have): the hit
/// proxy's owner (the engine's proxy owner lookup), an entity or a component's entity; null for the world's statics.
pub(crate) fn trace_entity(trace: &[u8; 0x60]) -> Handle {
    const OWNER_HASH: u32 = 2394822845;
    static OWNER: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(OWNER_HASH);
    type Owner = unsafe extern "C" fn(u32, u32) -> Handle;
    let (proxy, _, _) = trace_ids(trace);
    if proxy == 0 || proxy == u32::MAX {
        return Handle::null();
    }
    // SAFETY: the engine's proxy owner lookup (a Handle<ISerializable> by value).
    let Some(get_owner) = (unsafe { OWNER.func::<Owner>() }) else { return Handle::null() };
    let is = |h: &Handle, c: &str| !h.is_null() && unsafe { object_class(h.instance.cast()) }.is_some_and(|k| k.is_a(CName::hashed(c)));
    for index in 0..2 {
        // SAFETY: as above.
        let owner = unsafe { get_owner(proxy, index) };
        if is(&owner, "entEntity") {
            return owner;
        }
        if is(&owner, "entIComponent") {
            // entIComponent::owner (+0x50)
            let entity = cvr_engine::code::read::<usize>(owner.instance as usize + 0x50).unwrap_or(0);
            if entity != 0 {
                // SAFETY: the component's live entity.
                return unsafe { Handle::from_instance(entity as *mut c_void) };
            }
        }
    }
    Handle::null()
}
/// PROXY kinds: 1 a physical system, 12 a ragdoll.
const PROXY_PHYSICAL_SYSTEM: u8 = 1;
const PROXY_RAGDOLL: u8 = 12;
/// SubmitJoint: the D6 joint asked for at the traced body (a dynamic one -- a ragdoll or a physical system -- keeps the
/// serial; another clears it), bound already.
pub(crate) fn submit_joint(side: i32, serial: u32, trace: &[u8; 0x60], point: &V3, edge: &V3) -> bool {
    let (proxy, body, kind) = trace_ids(trace);
    let dynamic = kind == PROXY_RAGDOLL || kind == PROXY_PHYSICAL_SYSTEM;
    pb::request_blade_joint(side, if dynamic { serial } else { 0 }, proxy, body, Some(point), Some(edge)) && pb::blade_joint_bound(side, serial)
}
pub(crate) fn clear_joint(side: i32) {
    pb::request_blade_joint(side, 0, 0, 0, None, None);
}
/// TraceKind: the traced proxy's kind, 0 for none.
pub(crate) fn trace_kind(trace: &[u8; 0x60]) -> i32 {
    let (proxy, body, kind) = trace_ids(trace);
    if proxy != 0 && proxy != u32::MAX && body < 65536 {
        i32::from(kind)
    } else {
        0
    }
}
/// FindPuppetJoint: the target's own ragdoll body under the blade (among PhysX's bodies within 8 cm, a ragdoll's whose
/// owner is the target, the nearest by its own shapes within 4.5 cm), the joint asked for there. A cached choice is
/// asked again while it is bound or for 125 ms; nothing found clears the request.
pub(crate) fn find_puppet_joint(target: &Handle, side: i32, serial: u32, point: &V3, edge: &V3) -> bool {
    use std::cell::RefCell;
    const OWNER_HASH: u32 = 2394822845;
    static OWNER: red4ext_abi::reloc::Reloc = red4ext_abi::reloc::Reloc::new(OWNER_HASH);
    if target.is_null() || !(0..=1).contains(&side) || serial == 0 {
        return false;
    }
    // entEntity::status +0x156, 4 Attached
    if cvr_engine::code::read::<u8>(target.instance as usize + 0x156) != Some(4) {
        return false;
    }
    type Owner = unsafe extern "C" fn(u32, u32) -> Handle;
    // SAFETY: the engine's proxy owner lookup (a Handle<ISerializable> by value).
    let Some(get_owner) = (unsafe { OWNER.func::<Owner>() }) else { return false };
    let owned = |proxy: u32| -> bool {
        if proxy == 0 || proxy == u32::MAX || cvr_rig::vrik::debug_view::pick::proxy_kind(proxy) != 12 {
            return false;
        }
        // SAFETY: as above.
        (0..2).any(|index| unsafe { get_owner(proxy, index) }.instance == target.instance)
    };
    #[derive(Default)]
    struct Cached {
        target: usize,
        serial: u32,
        proxy: u32,
        body: u32,
        retry: u64,
    }
    thread_local! {
        static REQUESTS: RefCell<[Cached; 2]> = RefCell::new(Default::default());
    }
    let now = cvr_rig::vrik::physhands::tick_ms();
    REQUESTS.with(|r| {
        let mut r = r.borrow_mut();
        let request = &mut r[side as usize];
        if request.target != target.instance as usize || request.serial != serial {
            *request = Cached { target: target.instance as usize, serial, ..Cached::default() };
        }
        let bound = pb::blade_joint_bound(side, serial);
        if (bound || now < request.retry) && request.proxy != 0 && owned(request.proxy) {
            return pb::request_blade_joint(side, serial, request.proxy, request.body, Some(point), Some(edge)) && bound;
        }
        if now < request.retry {
            return false;
        }
        request.retry = now + 125;
        request.proxy = 0;
        let nearby = cvr_rig::vrik::debug_view::pick::find_near(point, 0.08, 16);
        let mut checked: Vec<u32> = Vec::with_capacity(16);
        for n in &nearby {
            if checked.contains(&n.proxy) {
                continue;
            }
            checked.push(n.proxy);
            if !owned(n.proxy) {
                continue;
            }
            let Some(found) = cvr_rig::vrik::debug_view::pick::nearest_body_of(n.proxy, point, 0.045) else { continue };
            request.proxy = n.proxy;
            request.body = found.body;
            return pb::request_blade_joint(side, serial, n.proxy, found.body, Some(point), Some(edge)) && pb::blade_joint_bound(side, serial);
        }
        pb::request_blade_joint(side, 0, 0, 0, None, None);
        false
    })
}
unsafe extern "C" {
    fn VRIK_ReadCurrentBodyFrame(p: *mut f32, q: *mut f32, tracking_yaw: *mut f32) -> bool;
}
/// TrackingBasis: the tracking's yaw as a direction (cos, sin, 0, 1), zero without a body frame.
pub(crate) fn tracking_basis() -> V4 {
    let (mut p, mut q, mut yaw) = ([0f32; 3], [0f32; 4], 0f32);
    // SAFETY: the body frame's reader with its outputs.
    if unsafe { VRIK_ReadCurrentBodyFrame(p.as_mut_ptr(), q.as_mut_ptr(), &mut yaw) } && yaw.is_finite() {
        [yaw.cos(), yaw.sin(), 0.0, 1.0]
    } else {
        [0.0; 4]
    }
}

// ---------------------------------------------------------------- the natives
fn out<T>(o: *mut T, v: T) {
    if !o.is_null() {
        // SAFETY: the VM's result slot.
        unsafe { *o = v };
    }
}
fn v3(v: &V4) -> V3 {
    [v[0], v[1], v[2]]
}
/// VRBladeReadCapsule(component, name, data) -> array<Vector4>.
#[no_mangle]
pub unsafe extern "C" fn VRBladeReadCapsule(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<V4>, _: i64) {
    let mut a = unsafe { args(frame) };
    let (component, name, data) = (a.get::<Handle>(), a.get::<u64>(), a.get::<Handle>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    if let Some((c, x)) = read_capsule(component.instance.cast(), name, data.instance.cast(), None) {
        r.push(c);
        r.push(x);
    }
}
/// VRBladeReadSlotCapsule(component, name, data, binding) -> array<Vector4>.
#[no_mangle]
pub unsafe extern "C" fn VRBladeReadSlotCapsule(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<V4>, _: i64) {
    let mut a = unsafe { args(frame) };
    let (component, name, data, binding) = (a.get::<Handle>(), a.get::<u64>(), a.get::<Handle>(), a.get::<u64>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    if let Some((c, x)) = read_capsule(component.instance.cast(), name, data.instance.cast(), Some(binding)) {
        r.push(c);
        r.push(x);
    }
}
unsafe fn sweep_native(frame: *mut StackFrame, result: *mut V4, check_exit: bool) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let ends = [a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>()];
    let grip = if check_exit { Some(a.get::<V4>()) } else { None };
    a.finish();
    let g = grip.map(|g| v3(&g));
    out(result, swept_contact(component.instance.cast(), &ends.map(|e| v3(&e)), g.as_ref()));
}
/// VRBladeSweptContact(component, oldA, oldB, a, b) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeSweptContact(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    unsafe { sweep_native(frame, result, false) }
}
/// VRBladeEntrySweep(component, oldA, oldB, a, b, endGrip) -> Vector4: w 2 when the end pose is clear.
#[no_mangle]
pub unsafe extern "C" fn VRBladeEntrySweep(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    unsafe { sweep_native(frame, result, true) }
}
/// VRBladeEdgeShapes(component, from, to) -> array<CName>: the (name, binding) pairs the segment is in.
#[no_mangle]
pub unsafe extern "C" fn VRBladeEdgeShapes(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<u64>, _: i64) {
    let mut a = unsafe { args(frame) };
    let (component, from, to) = (a.get::<Handle>(), a.get::<V4>(), a.get::<V4>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    let path = [v3(&from), v3(&to), v3(&from), v3(&to)];
    let mut pairs: Vec<(u64, u64)> = Vec::new();
    visit_sweep_contacts(
        component.instance.cast(),
        &path,
        |obs| {
            let p = (obs.item.name(), obs.binding);
            if !pairs.contains(&p) {
                pairs.push(p);
            }
        },
        None,
        None,
    );
    for (n, b) in pairs {
        r.push(n);
        r.push(b);
    }
}
/// VRBladeCollectOccupied(component, from, to, edge, pinSlot, pinFrame, sink) -> Vector4: each shape the shaft is in
/// handed to the sink's VRBladeKeepOccupied (a VrBladeVisiblePose, main's redscript class); x the shapes seen, y the
/// calls made, z those kept, w the slots that did not answer (-1: no sink).
#[no_mangle]
pub unsafe extern "C" fn VRBladeCollectOccupied(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let (from, to, world_edge) = (a.get::<V4>(), a.get::<V4>(), a.get::<V4>());
    let pin_binding = a.get::<u64>();
    let pin_frame = a.get::<[u32; 8]>();
    let sink = a.get::<Handle>();
    a.finish();
    out(result, [0.0, 0.0, 0.0, -1.0]);
    // SAFETY: the sink's class.
    if component.is_null() || sink.is_null() || unsafe { object_class(sink.instance.cast()) }.map(|c| c.class_name()) != Some(CName::hashed("VrBladeVisiblePose")) {
        return;
    }
    let r = collect_occupied(component.instance.cast(), &from, &to, &world_edge, pin_binding, &WorldTransform(pin_frame), |o| {
        let values = [
            Value::U64(o.name),
            Value::U64(o.slot),
            Value::Handle(&o.geometry),
            Value::Handle(&o.data),
            Value::U64(o.material),
            Value::Ptr(o.center.as_ptr().cast()),
            Value::Ptr(o.axis.as_ptr().cast()),
            Value::Ptr((&o.frame as *const WorldTransform).cast()),
            Value::Ptr(o.a.as_ptr().cast()),
            Value::Ptr(o.b.as_ptr().cast()),
            Value::Ptr(o.interval.as_ptr().cast()),
        ];
        // SAFETY: the sink, held; its own method with the eleven arguments main's passes.
        unsafe { method::<bool>(sink.instance.cast(), "VRBladeKeepOccupied", &values) }
    });
    out(result, r);
}
/// A shape the shaft is in, as CollectOccupied hands it on: its name and slot, its geometry and user data, material,
/// capsule, the slot's frame, the shaft's ends in it and the interval (fraction, depth, 0, an edge 1).
pub(crate) struct Occupied {
    pub name: u64,
    pub slot: u64,
    pub geometry: Handle,
    pub data: Handle,
    pub material: u64,
    pub center: V4,
    pub axis: V4,
    pub frame: WorldTransform,
    pub a: V4,
    pub b: V4,
    pub interval: V4,
}
/// CollectOccupied: each shape the shaft from..to is in (the pinned slot's frame given) handed to `keep` (None: not
/// called; Some(kept)); x the shapes seen, y the calls made, z those kept, w the slots that did not answer.
pub(crate) fn collect_occupied(comp: *mut c_void, from: &V4, to: &V4, world_edge: &V4, pin_binding: u64, pin_frame: &WorldTransform, mut keep: impl FnMut(Occupied) -> Option<bool>) -> V4 {
    let mut r = [0f32; 4];
    let shaft = [v3(from), v3(to), v3(from), v3(to)];
    let missing = visit_sweep_contacts(
        comp,
        &shaft,
        |obs| {
            // the blade's cut stations work on capsules: a box (a mech's shape) gates a hit but holds no cut
            if obs.item.capsule().is_none() {
                return;
            }
            r[0] += 1.0;
            if obs.item.name() == 0 || obs.binding == 0 || obs.item.user_data().is_null() {
                return;
            }
            let edge = qrot(&conj(&obs.frame.orientation()), &v3(world_edge));
            let has = bc::has_contact_edge(&v3(&obs.a), &v3(&obs.b), &edge);
            let o = Occupied {
                name: obs.item.name(),
                slot: obs.binding,
                geometry: obs.item.shape_handle(),
                data: obs.item.user_data_handle(),
                material: shape_material(comp, &obs.item),
                center: obs.center,
                axis: obs.axis,
                frame: obs.frame,
                a: obs.a,
                b: obs.b,
                interval: [obs.hit.fraction, obs.hit.depth, 0.0, if has { 1.0 } else { 0.0 }],
            };
            if let Some(kept) = keep(o) {
                r[1] += 1.0;
                if kept {
                    r[2] += 1.0;
                }
            }
        },
        Some((pin_binding, *pin_frame)),
        None,
    );
    r[3] = missing as f32;
    r
}
/// VRBladeSameHitData(first, second) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeSameHitData(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (x, y) = (a.get::<Handle>(), a.get::<Handle>());
    a.finish();
    out(result, equivalent_hit_data(x.instance.cast(), y.instance.cast()));
}
/// VRBladeSameGeometry(first, second) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeSameGeometry(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (x, y) = (a.get::<Handle>(), a.get::<Handle>());
    a.finish();
    out(result, !x.is_null() && x.instance == y.instance);
}
/// gameShapeData: the hit's shape name, its user data, its physics material, by the struct's properties.
pub(crate) fn shape_data_fields(hit: *const u8) -> Option<(u64, *mut c_void, u64)> {
    static AT: OnceLock<Option<(usize, usize, usize)>> = OnceLock::new();
    let (n, d, m) = (*AT.get_or_init(|| {
        let c = class("gameShapeData")?;
        Some((prop(c, "hitShapeName")?, prop(c, "userData")?, prop(c, "physicsMaterial")?))
    }))?;
    // SAFETY: the caller's struct.
    unsafe { Some((rd(hit as usize + n), rd(hit as usize + d), rd(hit as usize + m))) }
}
/// VRBladeMatchesOccupied(component, hit, slot, geometry, data, material, center, axis) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeMatchesOccupied(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let hit = a.owned("gameShapeData");
    let binding = a.get::<u64>();
    let (geometry, data) = (a.get::<Handle>(), a.get::<Handle>());
    let material = a.get::<u64>();
    let (center, axis) = (a.get::<V4>(), a.get::<V4>());
    a.finish();
    let Some((name, hit_data, hit_material)) = hit.as_ref().and_then(|h| shape_data_fields(h.as_ptr())) else {
        out(result, false);
        return;
    };
    out(result, matches_occupied(component.instance.cast(), name, hit_data, hit_material, binding, geometry.instance.cast(), data.instance.cast(), material, &center, &axis));
}
unsafe fn capsule_native(frame: *mut StackFrame, result: *mut V4, interval: bool) {
    let mut a = unsafe { args(frame) };
    let v = [a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>()];
    let material = a.get::<i32>();
    a.finish();
    out(result, capsule_result(&v[0], &v[1], &v[2], &v[3], &v[4], &v[5], material, interval));
}
/// VRBladeCapsuleMetrics(start, end, motion, edge, center, axis, material) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeCapsuleMetrics(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    unsafe { capsule_native(frame, result, false) }
}
/// VRBladeCapsuleInterval(start, end, motion, edge, center, axis, material) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeCapsuleInterval(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    unsafe { capsule_native(frame, result, true) }
}
/// VRBladeSubmitJoint(side, serial, trace, point, edge) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeSubmitJoint(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial) = (a.get::<i32>(), a.get::<u32>());
    let trace = a.get::<Trace>();
    let (point, edge) = (a.get::<V4>(), a.get::<V4>());
    a.finish();
    out(result, submit_joint(side, serial, &trace.0, &v3(&point), &v3(&edge)));
}
/// physicsTraceResult by value.
#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(crate) struct Trace(pub [u8; 0x60]);
impl Default for Trace {
    fn default() -> Trace {
        Trace([0; 0x60])
    }
}
/// VRBladeClearJoint(side).
#[no_mangle]
pub unsafe extern "C" fn VRBladeClearJoint(_c: *mut c_void, frame: *mut StackFrame, _result: *mut c_void, _: i64) {
    let mut a = unsafe { args(frame) };
    let side = a.get::<i32>();
    a.finish();
    clear_joint(side);
}
/// VRBladeTraceKind(trace) -> Int32.
#[no_mangle]
pub unsafe extern "C" fn VRBladeTraceKind(_c: *mut c_void, frame: *mut StackFrame, result: *mut i32, _: i64) {
    let mut a = unsafe { args(frame) };
    let trace = a.get::<Trace>();
    a.finish();
    out(result, trace_kind(&trace.0));
}
/// The joint's point (w 1), zero without a live joint.
pub(crate) fn joint_point(side: i32, serial: u32) -> V4 {
    pb::read_blade_joint_point(side, serial).map_or([0.0; 4], |p| [p[0], p[1], p[2], 1.0])
}
/// The joint kept with a load: held at its depth, slid inwards, or loaded (VRBladeHoldDepth / InsertSlide /
/// ApplySlideLoad).
pub(crate) fn joint_load(side: i32, serial: u32, resistance: f32, hold: bool, inserting: bool) -> bool {
    pb::renew_blade_joint(side, serial, resistance, hold, None, inserting, 0.0)
}
/// SlotEdgeSample: the cut's offset (stroke in w) and axis in the bound slot.
pub(crate) fn slot_edge_sample(side: i32, key: u32) -> Vec<V4> {
    match pb::read_blade_slot_edge(side, key) {
        Some((stroke, offset, axis)) if stroke != 0 => vec![[offset[0], offset[1], offset[2], stroke as f32], [axis[0], axis[1], axis[2], 0.0]],
        _ => Vec::new(),
    }
}
pub(crate) fn edge_stroke_live(side: i32, serial: u32, stroke: u32) -> bool {
    pb::blade_edge_stroke_live(side, serial, stroke)
}
/// VRBladeJointPoint(side, serial) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeJointPoint(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial) = (a.get::<i32>(), a.get::<u32>());
    a.finish();
    out(result, pb::read_blade_joint_point(side, serial).map_or([0.0; 4], |p| [p[0], p[1], p[2], 1.0]));
}
/// VRBladeRenewJoint(side, serial) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeRenewJoint(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial) = (a.get::<i32>(), a.get::<u32>());
    a.finish();
    out(result, pb::renew_blade_joint(side, serial, 0.0, false, None, false, 0.0));
}
unsafe fn slide_load(frame: *mut StackFrame, result: *mut bool, hold: bool, inserting: bool) {
    let mut a = unsafe { args(frame) };
    let (side, serial, resistance) = (a.get::<i32>(), a.get::<u32>(), a.get::<f32>());
    a.finish();
    out(result, pb::renew_blade_joint(side, serial, resistance, hold, None, inserting, 0.0));
}
/// VRBladeApplySlideLoad(side, serial, resistance) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeApplySlideLoad(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    unsafe { slide_load(frame, result, false, false) }
}
/// VRBladeHoldDepth(side, serial, resistance) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeHoldDepth(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    unsafe { slide_load(frame, result, true, false) }
}
/// VRBladeInsertSlide(side, serial, resistance) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeInsertSlide(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    unsafe { slide_load(frame, result, false, true) }
}
/// VRBladeTrackingBasis() -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeTrackingBasis(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    let a = unsafe { args(frame) };
    a.finish();
    out(result, tracking_basis());
}
/// VRBladeRequestEdgeSlide(side, serial, edge, load) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeRequestEdgeSlide(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial, edge, load) = (a.get::<i32>(), a.get::<u32>(), a.get::<V4>(), a.get::<f32>());
    a.finish();
    out(result, pb::renew_blade_joint(side, serial, load, false, Some(&v3(&edge)), false, 0.0));
}
/// DriveEdge: the cut along the item's cutting edge (the local shaft's edge), advanced at most 35 cm.
pub(crate) fn drive_edge(side: i32, serial: u32, local_shaft: &V3, advance: f32, load: f32) -> bool {
    if !(0.0..=0.35).contains(&advance) {
        return false;
    }
    let Some(edge) = cvr_physhands::blade::local_cut_edge(cvr_physhands::handspace::Vector::new(local_shaft[0], local_shaft[1], local_shaft[2])) else { return false };
    pb::renew_blade_joint(side, serial, load, false, Some(&edge.to_array()), false, advance)
}
/// VRBladeDriveEdge(side, serial, localShaft, advance, load) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeDriveEdge(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial, shaft, advance, load) = (a.get::<i32>(), a.get::<u32>(), a.get::<V4>(), a.get::<f32>(), a.get::<f32>());
    a.finish();
    out(result, drive_edge(side, serial, &v3(&shaft), advance, load));
}
/// EdgePath: the cut's start (stroke in w) and current point, and with the axis its stage.
pub(crate) fn edge_path(side: i32, serial: u32, with_axis: bool) -> Vec<V4> {
    let mut rows = Vec::new();
    if let Some((stroke, from, to, axis, stage)) = pb::read_blade_edge_path(side, serial) {
        if stroke != 0 || (with_axis && stage != 0) {
            rows.push([from[0], from[1], from[2], stroke as f32]);
            rows.push([to[0], to[1], to[2], 1.0]);
            if with_axis {
                rows.push([axis[0], axis[1], axis[2], stage as f32]);
            }
        }
    }
    rows
}
unsafe fn edge_path_native(frame: *mut StackFrame, result: *mut DynArray<V4>, with_axis: bool) {
    let mut a = unsafe { args(frame) };
    let (side, serial) = (a.get::<i32>(), a.get::<u32>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    for row in edge_path(side, serial, with_axis) {
        r.push(row);
    }
}
/// VRBladeEdgePath(side, serial) -> array<Vector4>.
#[no_mangle]
pub unsafe extern "C" fn VRBladeEdgePath(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<V4>, _: i64) {
    unsafe { edge_path_native(frame, result, false) }
}
/// VRBladeEdgeSample(side, serial) -> array<Vector4>.
#[no_mangle]
pub unsafe extern "C" fn VRBladeEdgeSample(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<V4>, _: i64) {
    unsafe { edge_path_native(frame, result, true) }
}
/// AnimatedAttachment: the blade pinned to the animated slot (the goal in xyz, the status in w).
pub(crate) fn animated_attachment(side: i32, key: u32, animated: &V3, rotation: &V4, station: &V3, bind: bool) -> V4 {
    let mut goal = [0f32; 3];
    let status = pb::update_blade_attachment(side, key, animated, rotation, station, bind, &mut goal);
    [goal[0], goal[1], goal[2], status as f32]
}
/// VRBladeAnimatedAttachment(side, key, animated, rotation, station, bind) -> Vector4.
#[no_mangle]
pub unsafe extern "C" fn VRBladeAnimatedAttachment(_c: *mut c_void, frame: *mut StackFrame, result: *mut V4, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, key, animated, rotation, station, bind) = (a.get::<i32>(), a.get::<u32>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<bool>());
    a.finish();
    out(result, animated_attachment(side, key, &v3(&animated), &rotation, &v3(&station), bind));
}
/// VRBladeSlotEdgeSample(side, key) -> array<Vector4>: the cut's offset (stroke in w) and axis in the bound slot.
#[no_mangle]
pub unsafe extern "C" fn VRBladeSlotEdgeSample(_c: *mut c_void, frame: *mut StackFrame, result: *mut DynArray<V4>, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, key) = (a.get::<i32>(), a.get::<u32>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    if let Some((stroke, offset, axis)) = pb::read_blade_slot_edge(side, key) {
        if stroke != 0 {
            r.push([offset[0], offset[1], offset[2], stroke as f32]);
            r.push([axis[0], axis[1], axis[2], 0.0]);
        }
    }
}
/// VRBladeEdgeStrokeLive(side, serial, stroke) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeEdgeStrokeLive(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (side, serial, stroke) = (a.get::<i32>(), a.get::<u32>(), a.get::<u32>());
    a.finish();
    out(result, pb::blade_edge_stroke_live(side, serial, stroke));
}
/// VRBladeFindPuppetJoint(target, side, serial, point, edge) -> Bool.
#[no_mangle]
pub unsafe extern "C" fn VRBladeFindPuppetJoint(_c: *mut c_void, frame: *mut StackFrame, result: *mut bool, _: i64) {
    let mut a = unsafe { args(frame) };
    let (target, side, serial, point, edge) = (a.get::<Handle>(), a.get::<i32>(), a.get::<u32>(), a.get::<V4>(), a.get::<V4>());
    a.finish();
    out(result, find_puppet_joint(&target, side, serial, &v3(&point), &v3(&edge)));
}
