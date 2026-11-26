//! HAIR THAT THE HANDS MOVE (main's src/Natives/HairDangle.cpp). A person's hair (and a coat's tails, a pair of earrings)
//! swings by the animation's dangle simulations: an animDangleConstraint_SimulationDyng (particles on the hair's bones,
//! linked and coned, against rounded collision shapes on the head and shoulders) or a pendulum per bone, in the anim graph
//! of an animated component of the person's (Judy's hair: hh_051_wa__judy_dangle.animgraph, 26 particles, 19 of them
//! free). The hands are no bone of that rig, so they are given to it as what the simulation already takes from outside:
//! its external force, world space, which every particle feels (gravity's company). The hand collision decides when a
//! hand goes through the hair and how hard; this file only finds the simulations and sets the force.
//!
//! The simulations are the graph's, which is a resource: every person who wears that hair shares it. A force given for
//! one is felt by all who wear the same at that moment, so it is given only while a hand is in someone's hair, and taken
//! back the moment it is not.
//!
//! Every read of the engine's objects is guarded (cvr_engine::code::read, main's __try): the field offsets come from the
//! classes' own properties, the rig's from its resource (optic_hinge's: parentIndices +0x40, referencePoseLS +0x48,
//! boneNames +0x50).
use core::ffi::c_void;
use cvr_engine::code::{read, write};
use red4ext_abi::cname::CName;
use red4ext_abi::cstring::CString;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class, IType, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type Frame = StackFrame;
type V3 = [f32; 3];
type Q = [f32; 4];

// entEntity::components (+0xA0, DynArray<Handle<entIComponent>>); entIComponent::name (+0x40).
const ENTITY_COMPONENTS: usize = 0xA0;
const COMPONENT_NAME: usize = 0x40;
// A Ref<T>: its path +0, its token (a SharedPtr: the instance) +8. ResourceToken: the resource's handle +0x28, the path
// +0x48, finished +0x58, error +0x5C.
const REF_TOKEN: usize = 8;
const TOKEN_RESOURCE: usize = 0x28;
const TOKEN_PATH: usize = 0x48;
const TOKEN_FINISHED: usize = 0x58;
const TOKEN_ERROR: usize = 0x5C;
// animRig: parentIndices (Int16 *) +0x40, referencePoseLS (QsTransform *, 0x30 each: translation +0, rotation +0x10)
// +0x48, boneNames (DynArray<CName>) +0x50.
const RIG_PARENTS: usize = 0x40;
const RIG_REFERENCE_POSE: usize = 0x48;
const RIG_BONE_NAMES: usize = 0x50;
// A DynArray: its entries +0, its size +0xC.
const ARRAY_SIZE: usize = 0xC;

const DYNG: CName = CName::hashed("animDangleConstraint_SimulationDyng");
const PENDULUM: CName = CName::hashed("animDangleConstraint_SimulationPendulum");

/// Where the fields are, from the classes' properties (once).
#[derive(Clone, Copy)]
struct Layout {
    animated: &'static Class,
    dangle_node: &'static Class,
    graph: usize,
    rig: usize,
    nodes_to_init: usize,
    dangle_constraint: usize,
    rounded_shapes: usize,
    container: usize,
    particles: usize,
    external_dyng: usize,
    gravity: usize,
    external_pendulum: usize,
    particle_size: usize,
    particle_bone_name: usize,
    particle_mass: usize,
}
// SAFETY: classes are the RTTI system's, alive for the process.
unsafe impl Send for Layout {}
unsafe impl Sync for Layout {}

fn offset(class: &Class, name: &str) -> Option<usize> {
    class.property_named(name).map(|p| p.value_offset as usize)
}
fn layout() -> Option<Layout> {
    static LAYOUT: OnceLock<Option<Layout>> = OnceLock::new();
    *LAYOUT.get_or_init(|| {
        let rtti = RttiSystem::get()?;
        let class = |n: &str| rtti.get_class(CName::hashed(n));
        let animated = class("entAnimatedComponent")?;
        let dangle_node = class("animAnimNode_Dangle")?;
        let graph_class = class("animAnimGraph")?;
        let sim = class("animDangleConstraint_Simulation")?;
        let dyng = class("animDangleConstraint_SimulationDyng")?;
        let pendulum = class("animDangleConstraint_SimulationPendulum")?;
        let container = class("animDyngParticlesContainer")?;
        let particle = class("animDyngParticle")?;
        let transform_index = class("animTransformIndex")?;
        // SAFETY: a class is an IType.
        let particle_size = unsafe { (*(particle as *const Class as *const IType)).size() } as usize;
        let l = Layout {
            animated,
            dangle_node,
            graph: offset(animated, "graph")?,
            rig: offset(animated, "rig")?,
            nodes_to_init: offset(graph_class, "nodesToInit")?,
            dangle_constraint: offset(dangle_node, "dangleConstraint")?,
            rounded_shapes: offset(sim, "collisionRoundedShapes")?,
            container: offset(dyng, "particlesContainer")?,
            particles: offset(container, "particles")?,
            external_dyng: offset(container, "externalForceWS")?,
            gravity: offset(container, "gravityWS")?,
            external_pendulum: offset(pendulum, "externalForceWS")?,
            particle_size,
            particle_bone_name: offset(particle, "bone")? + offset(transform_index, "name")?,
            particle_mass: offset(particle, "mass")?,
        };
        (particle_size > 0).then_some(l)
    })
}

/// One dangle simulation found: the animated component, the simulation, its class's name, the graph's path hash.
#[derive(Clone, Copy)]
struct Dangle {
    component: usize,
    sim: usize,
    kind: CName,
    graph: u64,
}
fn name_text(n: CName) -> String {
    n.text().to_string()
}
/// A loaded resource behind a Ref<T> at `at`: (the resource, the token's path hash).
fn resource_of(at: usize) -> Option<(usize, u64)> {
    let token = read::<usize>(at + REF_TOKEN)?;
    if token == 0 || read::<i32>(token + TOKEN_FINISHED)? == 0 || read::<u8>(token + TOKEN_ERROR)? != 0 {
        return None;
    }
    let resource = read::<usize>(token + TOKEN_RESOURCE)?;
    (resource != 0).then(|| (resource, read::<u64>(token + TOKEN_PATH).unwrap_or(0)))
}
/// A DynArray at `at`: (its entries, its size).
fn array(at: usize) -> Option<(usize, u32)> {
    let entries = read::<usize>(at)?;
    let size = read::<u32>(at + ARRAY_SIZE)?;
    Some((entries, if entries == 0 { 0 } else { size }))
}
/// Each dangle simulation in the entity's animated components' graphs (main's EachDangle).
fn each_dangle(entity: usize) -> Vec<Dangle> {
    let mut out = Vec::new();
    let Some(l) = layout() else { return out };
    if entity == 0 {
        return out;
    }
    let Some((comps, n)) = array(entity + ENTITY_COMPONENTS) else { return out };
    for i in 0..n.min(4096) as usize {
        let Some(component) = read::<usize>(comps + i * 16) else { break };
        if component == 0 {
            continue;
        }
        // SAFETY: a live component's class (read through its vtable, as the SDK's GetType).
        let Some(class) = (unsafe { object_class(component as *const c_void) }) else { continue };
        if !class.is_a(l.animated.class_name()) {
            continue;
        }
        let Some((graph, path)) = resource_of(component + l.graph) else { continue };
        let Some((nodes, count)) = array(graph + l.nodes_to_init) else { continue };
        for k in 0..count.min(65536) as usize {
            let Some(node) = read::<usize>(nodes + k * 16) else { break };
            if node == 0 {
                continue;
            }
            // SAFETY: as above.
            let Some(nc) = (unsafe { object_class(node as *const c_void) }) else { continue };
            if !nc.is_a(l.dangle_node.class_name()) {
                continue;
            }
            let Some(sim) = read::<usize>(node + l.dangle_constraint).filter(|s| *s != 0) else { continue };
            // SAFETY: as above.
            let Some(sc) = (unsafe { object_class(sim as *const c_void) }) else { continue };
            out.push(Dangle { component, sim, kind: sc.class_name(), graph: path });
        }
    }
    out
}
fn component_name(component: usize) -> CName {
    CName(read::<u64>(component + COMPONENT_NAME).unwrap_or(0))
}
/// THE HAIR ALONE, where the entity's components say which is hair: a name with "hh_" or "hair" in it (the game's hair
/// meshes and their dangles are hh_...).
fn hair_name(component: usize) -> bool {
    let s = name_text(component_name(component)).to_ascii_lowercase();
    s.contains("hh_") || s.contains("hair")
}
/// THE BREASTS: a component named so ("breast", "boob").
fn breast_name(component: usize) -> bool {
    let s = name_text(component_name(component)).to_ascii_lowercase();
    s.contains("breast") || s.contains("boob")
}
fn force_field(l: &Layout, d: &Dangle) -> Option<usize> {
    if d.kind == DYNG {
        Some(d.sim + l.container + l.external_dyng)
    } else if d.kind == PENDULUM {
        Some(d.sim + l.external_pendulum)
    } else {
        None
    }
}

/// HairForce: every dyng and pendulum of the entity's dangles given that external force (world space, on each particle;
/// zero gives the hair back to itself); the hair alone where its components say which is hair (an earring's and a
/// coat's tails swung with the hair at a hand in it), every dangle when none is so named and for a zero force. The count
/// of them, -1 when reading them failed.
pub fn hair_force(entity: *mut c_void, force: &V3) -> i32 {
    let Some(l) = layout() else { return -1 };
    let all = each_dangle(entity as usize);
    let zero = force[0] == 0.0 && force[1] == 0.0 && force[2] == 0.0;
    let any_hair = !zero && all.iter().any(|d| hair_name(d.component));
    for d in &all {
        if any_hair && !hair_name(d.component) {
            continue;
        }
        if let Some(f) = force_field(&l, d) {
            if !write::<V3>(f, *force) {
                return -1;
            }
        }
    }
    all.len() as i32
}
/// DangleForce: the external force on the entity's dangles of one kind -- part 0 the hair, 1 the breasts, 2 any -- all of
/// them (index < 0) or the index-th of that kind; the count set, -1 when reading them failed.
pub fn dangle_force(entity: *mut c_void, force: &V3, part: i32, index: i32) -> i32 {
    let Some(l) = layout() else { return -1 };
    let mut seen = 0;
    let mut set = 0;
    for d in each_dangle(entity as usize) {
        let matched = match part {
            0 => hair_name(d.component),
            1 => breast_name(d.component),
            _ => true,
        };
        if !matched {
            continue;
        }
        let at = seen;
        seen += 1;
        if index >= 0 && at != index {
            continue;
        }
        let Some(f) = force_field(&l, &d) else { continue };
        if !write::<V3>(f, *force) {
            return -1;
        }
        set += 1;
    }
    set
}

// ---- WHERE A PERSON'S HAIR IS: each hair dyng particle's place in its rig's reference pose, in the frame of that rig's
// Head bone (the head's X up, Y to the back, Z to the right), and the particle it hangs from.
fn qmul(a: &Q, b: &Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn qrot(q: &Q, v: &V3) -> V3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
/// A hair rig's reference pose in its model space (each bone from its parent; the parents come first), its bones' names
/// and parents, and its Head bone.
struct RigPose {
    pos: Vec<V3>,
    rot: Vec<Q>,
    names: Vec<u64>,
    parents: Vec<i32>,
    head: usize,
}
fn build_pose(rig: usize) -> Option<RigPose> {
    const HEAD: CName = CName::hashed("Head");
    let (names_at, n) = array(rig + RIG_BONE_NAMES)?;
    let parents_at = read::<usize>(rig + RIG_PARENTS)?;
    let pose_at = read::<usize>(rig + RIG_REFERENCE_POSE)?;
    let n = n as usize;
    if n == 0 || n > 1024 || parents_at == 0 || pose_at == 0 {
        return None;
    }
    let mut out = RigPose { pos: vec![[0.0; 3]; n], rot: vec![[0.0, 0.0, 0.0, 1.0]; n], names: Vec::with_capacity(n), parents: Vec::with_capacity(n), head: usize::MAX };
    for b in 0..n {
        let t = read::<[f32; 3]>(pose_at + b * 0x30)?;
        let r = read::<Q>(pose_at + b * 0x30 + 0x10)?;
        let p = i32::from(read::<i16>(parents_at + b * 2)?);
        let name = read::<u64>(names_at + b * 8)?;
        if p >= 0 && (p as usize) < b {
            let w = qrot(&out.rot[p as usize], &t);
            let pp = out.pos[p as usize];
            out.pos[b] = [pp[0] + w[0], pp[1] + w[1], pp[2] + w[2]];
            out.rot[b] = qmul(&out.rot[p as usize], &r);
        } else {
            out.pos[b] = t;
            out.rot[b] = r;
        }
        if out.head == usize::MAX && name == HEAD.0 {
            out.head = b;
        }
        out.names.push(name);
        out.parents.push(p);
    }
    (out.head != usize::MAX).then_some(out)
}
/// The particles of a dyng: (their entries, their count).
fn particles(l: &Layout, d: &Dangle) -> Option<(usize, usize)> {
    let (at, n) = array(d.sim + l.container + l.particles)?;
    Some((at, n as usize))
}
/// HairPoints: where the entity's hair is on its head, one "x y z parent;" per hair dyng particle (metres, the Head
/// bone's frame; parent the particle it hangs from, its place in the list, -1 for one on the scalp). Empty when no hair
/// dyng has a rig with a Head bone, or reading it failed.
pub fn hair_points(entity: *mut c_void) -> String {
    let Some(l) = layout() else { return String::new() };
    let mut text = String::new();
    let mut count = 0i32;
    for d in each_dangle(entity as usize) {
        if d.kind != DYNG || !hair_name(d.component) {
            continue;
        }
        let Some((rig, _)) = resource_of(d.component + l.rig) else { continue };
        let Some(pose) = build_pose(rig) else { continue };
        let Some((parts, m)) = particles(&l, &d) else { return String::new() };
        let n = pose.names.len();
        let mut bone_of = vec![-1i32; m];
        let mut at_bone = vec![-1i32; n];
        let mut place = vec![-1i32; m];
        let mut c = 0;
        for i in 0..m {
            let Some(name) = read::<u64>(parts + i * l.particle_size + l.particle_bone_name) else { return String::new() };
            if let Some(b) = pose.names.iter().position(|x| *x == name) {
                bone_of[i] = b as i32;
                at_bone[b] = i as i32;
            }
            if bone_of[i] >= 0 {
                place[i] = count + c;
                c += 1;
            }
        }
        let h = pose.head;
        let inv = [-pose.rot[h][0], -pose.rot[h][1], -pose.rot[h][2], pose.rot[h][3]];
        for i in 0..m {
            let b = bone_of[i];
            if b < 0 {
                continue;
            }
            let b = b as usize;
            let v = [pose.pos[b][0] - pose.pos[h][0], pose.pos[b][1] - pose.pos[h][1], pose.pos[b][2] - pose.pos[h][2]];
            let lp = qrot(&inv, &v);
            // the nearest particle up its bones (none for one on the scalp)
            let mut up = pose.parents[b];
            let mut steps = 0;
            while up >= 0 && (up as usize) < n && at_bone[up as usize] < 0 && steps < n {
                up = pose.parents[up as usize];
                steps += 1;
            }
            let parent = if up >= 0 && (up as usize) < n && at_bone[up as usize] >= 0 { place[at_bone[up as usize] as usize] } else { -1 };
            text.push_str(&format!("{:.4} {:.4} {:.4} {};", lp[0], lp[1], lp[2], parent));
        }
        count += c;
    }
    text
}

/// EACH LOCK ITS OWN. The hair's force is one for the whole dyng, but the simulation divides it by each particle's mass
/// (gravity it does not: a heavy particle hangs and swings as before). So while a hand is in the hair, each particle gets
/// its own mass over its weight (1 where a hand is, down its lock from there), and the rest are made heavy enough that the
/// force leaves them be. The hair's own masses are kept here and given back when the weights are taken out (an empty
/// list). Shared, as the force: all who wear that hair have the same locks light while a hand is in one's.
struct OwnMasses {
    graph: u64,
    mass: Vec<f32>,
}
static OWN_MASSES: Mutex<Option<HashMap<usize, OwnMasses>>> = Mutex::new(None);
const LEAST_WEIGHT: f32 = 1e-4;
/// HairStrands: each hair particle (hair_points' order) as touched as its weight, 0 to 1 (a missing one 0), the force on
/// the hair moving each by its weight; empty gives the hair its own masses back. The count set, -1 when reading the hair
/// failed.
pub fn hair_strands(entity: *mut c_void, weights: &[f32]) -> i32 {
    let Some(l) = layout() else { return -1 };
    let mut guard = OWN_MASSES.lock().unwrap_or_else(|p| p.into_inner());
    let own_all = guard.get_or_insert_with(HashMap::new);
    let mut set = 0;
    let mut at = 0usize;
    for d in each_dangle(entity as usize) {
        if d.kind != DYNG || !hair_name(d.component) {
            continue;
        }
        let Some((rig, _)) = resource_of(d.component + l.rig) else { continue };
        let Some(pose) = build_pose(rig) else { continue };
        let Some((parts, m)) = particles(&l, &d) else { return -1 };
        // kept for another hair that came to the same address: dropped
        if own_all.get(&d.sim).is_some_and(|o| o.graph != d.graph || o.mass.len() != m) {
            own_all.remove(&d.sim);
        }
        let mass_at = |i: usize| parts + i * l.particle_size + l.particle_mass;
        if weights.is_empty() {
            if let Some(o) = own_all.remove(&d.sim) {
                for i in 0..m {
                    if !write::<f32>(mass_at(i), o.mass[i]) {
                        return -1;
                    }
                }
                set += m as i32;
            }
            continue;
        }
        if !own_all.contains_key(&d.sim) {
            let mut mass = Vec::with_capacity(m);
            for i in 0..m {
                let Some(v) = read::<f32>(mass_at(i)) else { return -1 };
                mass.push(v);
            }
            own_all.insert(d.sim, OwnMasses { graph: d.graph, mass });
        }
        let own = &own_all[&d.sim];
        for i in 0..m {
            let Some(name) = read::<u64>(parts + i * l.particle_size + l.particle_bone_name) else { return -1 };
            if !pose.names.contains(&name) {
                continue;
            }
            let w = weights.get(at).copied().unwrap_or(0.0);
            at += 1;
            if !write::<f32>(mass_at(i), own.mass[i] / LEAST_WEIGHT.max(w.min(1.0))) {
                return -1;
            }
            set += 1;
        }
    }
    set
}
/// The weights of VRHairStrands' text ("w0 w1 ..."): read up to the first that is not a number.
pub fn parse_weights(text: &str) -> Vec<f32> {
    let mut out = Vec::new();
    for word in text.split_ascii_whitespace() {
        match word.parse::<f32>() {
            Ok(v) => out.push(v),
            Err(_) => break,
        }
    }
    out
}

/// The dangle simulations an entity's animated components run, one line each (a test tool's look at them).
pub fn describe(entity: *mut c_void) -> String {
    let Some(l) = layout() else { return "(failed)".into() };
    let mut text = String::new();
    for d in each_dangle(entity as usize) {
        let name = name_text(component_name(d.component));
        let shapes = array(d.sim + l.rounded_shapes).map_or(0, |a| a.1);
        if d.kind == DYNG {
            let f = read::<V3>(d.sim + l.container + l.external_dyng).unwrap_or([0.0; 3]);
            let g = read::<f32>(d.sim + l.container + l.gravity).unwrap_or(0.0);
            let n = particles(&l, &d).map_or(0, |p| p.1);
            text.push_str(&format!(
                "{name}: dyng {n} particles, {shapes} shapes, force {:.2} {:.2} {:.2}, gravity {g:.2} (graph {:016x}, sim {:#x}); ",
                f[0], f[1], f[2], d.graph, d.sim
            ));
        } else if d.kind == PENDULUM {
            let f = read::<V3>(d.sim + l.external_pendulum).unwrap_or([0.0; 3]);
            text.push_str(&format!("{name}: pendulum, {shapes} shapes, force {:.2} {:.2} {:.2}; ", f[0], f[1], f[2]));
        } else {
            text.push_str(&format!("{name}: {}; ", name_text(d.kind)));
        }
    }
    text
}
/// Raw bytes of the first hair dyng (a test tool's look): its shapes list's header and first shape, the particle's, and
/// the container's first 0x38 bytes.
pub fn raw(entity: *mut c_void, index: i32) -> String {
    let Some(l) = layout() else { return "(failed)".into() };
    let hex = |at: usize, bytes: usize, text: &mut String| {
        for i in 0..bytes {
            text.push_str(&format!("{:02x}", read::<u8>(at + i).unwrap_or(0)));
            if i & 3 == 3 {
                text.push(' ');
            }
        }
        text.push_str("; ");
    };
    let mut text = String::new();
    let shape_size = RttiSystem::get()
        .and_then(|r| r.get_class(CName::hashed("animCollisionRoundedShape")))
        // SAFETY: a class is an IType.
        .map_or(0, |c| unsafe { (*(c as *const Class as *const IType)).size() } as usize);
    if let Some(d) = each_dangle(entity as usize).into_iter().find(|d| d.kind == DYNG && hair_name(d.component)) {
        let entries = read::<usize>(d.sim + l.rounded_shapes).unwrap_or(0);
        let capacity = read::<u32>(d.sim + l.rounded_shapes + 8).unwrap_or(0);
        let size = read::<u32>(d.sim + l.rounded_shapes + ARRAY_SIZE).unwrap_or(0);
        text.push_str(&format!("shapes {entries:#x} cap {capacity} size {size}; "));
        if size > 0 && capacity >= size && entries != 0 {
            text.push_str("shape0 ");
            hex(entries, shape_size, &mut text);
        }
        if let Some((parts, m)) = particles(&l, &d) {
            if index >= 0 && (index as usize) < m {
                let at = parts + index as usize * l.particle_size;
                let name = CName(read::<u64>(at + l.particle_bone_name).unwrap_or(0));
                text.push_str(&format!("particle {index} of {m} ({}) ", name_text(name)));
                hex(at, l.particle_size, &mut text);
            }
        }
        text.push_str("container ");
        hex(d.sim + l.container, 0x38, &mut text);
    }
    text
}

// ---- the natives
macro_rules! params {
    ($frame:expr $(, $value:expr)*) => {
        if let Some(frame) = $frame.as_mut() {
            $(frame.get_parameter(&mut $value);)*
            frame.finish_params();
        }
    };
}
unsafe fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        *out = value;
    }
}
/// VRHairForce(entity, force) -> Int32: see hair_force.
#[no_mangle]
pub unsafe extern "C" fn VRHairForce(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let (mut entity, mut force) = (Handle::null(), [0.0f32; 4]);
    params!(frame, entity, force);
    put(out, hair_force(entity.instance.cast(), &[force[0], force[1], force[2]]));
}
/// VRDangleForce(entity, force, part, index) -> Int32: see dangle_force.
#[no_mangle]
pub unsafe extern "C" fn VRDangleForce(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let (mut entity, mut force, mut part, mut index) = (Handle::null(), [0.0f32; 4], 0i32, -1i32);
    params!(frame, entity, force, part, index);
    put(out, dangle_force(entity.instance.cast(), &[force[0], force[1], force[2]], part, index));
}
/// VRHairPoints(entity) -> String: see hair_points.
#[no_mangle]
pub unsafe extern "C" fn VRHairPoints(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _: i64) {
    let mut entity = Handle::null();
    params!(frame, entity);
    if !out.is_null() {
        CString::assign(out, &hair_points(entity.instance.cast()));
    }
}
/// VRHairStrands(entity, weights) -> Int32: see hair_strands.
#[no_mangle]
pub unsafe extern "C" fn VRHairStrands(_this: *mut c_void, frame: *mut Frame, out: *mut i32, _: i64) {
    let (mut entity, mut data) = (Handle::null(), CString::empty());
    params!(frame, entity, data);
    let weights = parse_weights(data.as_str());
    put(out, hair_strands(entity.instance.cast(), &weights));
}
/// VRHairDangles(entity) -> String: see describe.
#[no_mangle]
pub unsafe extern "C" fn VRHairDangles(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _: i64) {
    let mut entity = Handle::null();
    params!(frame, entity);
    if !out.is_null() {
        CString::assign(out, &describe(entity.instance.cast()));
    }
}
/// VRHairRaw(entity, particle) -> String: see raw.
#[no_mangle]
pub unsafe extern "C" fn VRHairRaw(_this: *mut c_void, frame: *mut Frame, out: *mut CString, _: i64) {
    let (mut entity, mut index) = (Handle::null(), -1i32);
    params!(frame, entity, index);
    if !out.is_null() {
        CString::assign(out, &raw(entity.instance.cast(), index));
    }
}
