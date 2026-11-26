//! The car's panels the hand moves (the doors, the trunk, the hood): a panel's bone held at the angle the mechanics
//! want, written into the vehicle's completed pose (the animated object's final local-to-model, anim_pose_hook.rs),
//! after every graph controller. Its hinge is the graph's own: the rotation the door animation makes from the closed
//! pose (the axis, in the bone's parent frame, once it has turned 2 degrees) and the furthest it has gone (the open
//! angle); the pivot and the axis in the model's frame are published for the hand's angle about the hinge.
//!
//! The pose passes run on the animation's workers: a pass touches a door only when its descriptor is one of the
//! registered ones (an atomic list), under a lock it never waits for.
use core::sync::atomic::{AtomicUsize, Ordering::*};
use cvr_engine::code::read;
use std::sync::Mutex;

type Q = [f32; 4];
type V3 = [f32; 3];
const BONE: usize = 48;
const MAX_BONES: u32 = 4096;
const SLOTS: usize = 12;
// animMetaRig: parentIndeces +0x10, boneNames +0x20 (DynArrays: the entries, then the size at +0xC)
const META_PARENTS: usize = 0x10;
const META_NAMES: usize = 0x20;
const ARRAY_SIZE: usize = 0xC;

/// One panel's bone in one vehicle's pose.
#[derive(Clone, Copy, Default)]
pub struct Door {
    pub object: usize,
    pub descriptor: usize,
    pub bone: i32,
    /// the closed pose's local rotation (the graph's, read while the door was shut)
    pub closed: Q,
    pub have_closed: bool,
    /// the hinge axis in the bone's parent frame, the open angle seen (rad)
    pub axis: V3,
    pub have_axis: bool,
    pub open_max: f32,
    /// the graph's own angle this pass, the angle wanted, whether it is written
    pub graph_angle: f32,
    pub angle: f32,
    pub drive: bool,
    /// the hinge in the model's frame: its pivot and axis (from the bone's parent chain)
    pub pivot: V3,
    pub model_axis: V3,
    pub have_model: bool,
    pub seen: u64,
    pub passes: u32,
    /// the slot's bone and its ancestors (the hinge is the one the graph turns), their reference rotations
    pub chain: [i32; 4],
    pub chain_closed: [Q; 4],
}
static ACTIVE: [AtomicUsize; SLOTS] = [const { AtomicUsize::new(0) }; SLOTS];
/// CVR_DOOR_OBSERVE=1: the passes only observe (a development switch).
fn observe_only() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("CVR_DOOR_OBSERVE").is_ok_and(|v| v == "1"))
}
/// The pass's whole bone buffer is committed and writable.
fn writable(buf: usize, bytes: usize) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_EXECUTE_READWRITE, PAGE_READWRITE};
    let mut at = buf;
    let end = buf + bytes;
    while at < end {
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
        // SAFETY: a query of the address space.
        if unsafe { VirtualQuery(at as *const core::ffi::c_void, &mut info, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            return false;
        }
        if info.State != MEM_COMMIT || (info.Protect != PAGE_READWRITE && info.Protect != PAGE_EXECUTE_READWRITE) {
            return false;
        }
        at = info.BaseAddress as usize + info.RegionSize;
    }
    true
}
static DOORS: Mutex<[Option<Door>; SLOTS]> = Mutex::new([None; SLOTS]);

fn qmul(a: &Q, b: &Q) -> Q {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] + a[1] * b[3] + a[2] * b[0] - a[0] * b[2],
        a[3] * b[2] + a[2] * b[3] + a[0] * b[1] - a[1] * b[0],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}
fn conj(q: &Q) -> Q {
    [-q[0], -q[1], -q[2], q[3]]
}
fn rot(q: &Q, v: &V3) -> V3 {
    let tx = 2.0 * (q[1] * v[2] - q[2] * v[1]);
    let ty = 2.0 * (q[2] * v[0] - q[0] * v[2]);
    let tz = 2.0 * (q[0] * v[1] - q[1] * v[0]);
    [v[0] + q[3] * tx + (q[1] * tz - q[2] * ty), v[1] + q[3] * ty + (q[2] * tx - q[0] * tz), v[2] + q[3] * tz + (q[0] * ty - q[1] * tx)]
}
fn axis_angle(axis: &V3, angle: f32) -> Q {
    let (s, c) = (angle * 0.5).sin_cos();
    [axis[0] * s, axis[1] * s, axis[2] * s, c]
}
fn normalized(q: Q) -> Option<Q> {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    (n.is_finite() && n > 0.5).then(|| [q[0] / n, q[1] / n, q[2] / n, q[3] / n])
}

/// The bone of a vehicle's animated object by name (its rig's names), -1 for none.
pub fn bone_of(object: usize, name: u64) -> i32 {
    let Some(rig) = read::<usize>(object + 8).filter(|r| *r != 0) else { return -1 };
    let (Some(names), Some(count)) = (read::<usize>(rig + META_NAMES), read::<u32>(rig + META_NAMES + ARRAY_SIZE)) else { return -1 };
    if count > MAX_BONES {
        return -1;
    }
    (0..count as usize).find(|&i| read::<u64>(names + i * 8) == Some(name)).map_or(-1, |i| i as i32)
}
/// A panel registered (or found again): its slot. The descriptor is the object's (+0x18).
pub fn register(object: usize, bone: i32) -> Option<usize> {
    let descriptor = read::<usize>(object + 0x18).filter(|d| *d != 0)?;
    if bone < 0 {
        return None;
    }
    let mut g = DOORS.lock().ok()?;
    if let Some(i) = g.iter().position(|d| d.is_some_and(|d| d.object == object && d.bone == bone)) {
        return Some(i);
    }
    let i = g.iter().position(|d| d.is_none()).or_else(|| g.iter().enumerate().filter(|(_, d)| d.is_some_and(|d| !d.drive)).min_by_key(|(_, d)| d.map_or(0, |d| d.seen)).map(|(i, _)| i))?;
    // the closed pose: the rig's reference (bind) rotation of the bone -- and of its ancestors, one of which may be the
    // hinge the animation turns (a slot's bone is often a socket under it)
    let rig = read::<usize>(object + 8).unwrap_or(0);
    let reference_of = |b: i32| read::<usize>(rig).and_then(|t| read::<Q>(t + b as usize * BONE + 16)).and_then(normalized);
    let reference = reference_of(bone);
    let mut chain = [-1i32; 4];
    let mut chain_closed = [[0.0, 0.0, 0.0, 1.0]; 4];
    let parents = read::<usize>(rig + META_PARENTS).unwrap_or(0);
    let mut b = bone;
    for k in 0..4 {
        if b < 0 {
            break;
        }
        chain[k] = b;
        chain_closed[k] = reference_of(b).unwrap_or([0.0, 0.0, 0.0, 1.0]);
        b = if parents != 0 { read::<i16>(parents + b as usize * 2).map_or(-1, i32::from) } else { -1 };
        // the chain stops at the root (the body itself does not swing)
        if b == 0 {
            break;
        }
    }
    g[i] = Some(Door { object, descriptor, bone, closed: reference.unwrap_or([0.0, 0.0, 0.0, 1.0]), have_closed: reference.is_some(), chain, chain_closed, ..Door::default() });
    ACTIVE[i].store(descriptor, Release);
    let (buf, capacity, slots) = (read::<usize>(descriptor).unwrap_or(0), read::<u32>(descriptor + 8).unwrap_or(0), read::<u32>(descriptor + 12).unwrap_or(0));
    let names = read::<usize>(object + 8).and_then(|rig| read::<u32>(rig + META_NAMES + ARRAY_SIZE)).unwrap_or(0);
    crate::vrik::env::log(&format!(
        "[vehicle-doors] bone {bone} of object {object:#x} registered: descriptor {descriptor:#x} (buffer {buf:#x}, {slots} of {capacity} slots, the rig's {names} names), reference {:?}\n",
        reference
    ));
    Some(i)
}
/// The panel's state, as the last pass left it.
pub fn get(slot: usize) -> Option<Door> {
    DOORS.lock().ok()?.get(slot).copied().flatten()
}
/// The angle the panel is held at (`drive` off: the graph's own pose shown).
pub fn set(slot: usize, angle: f32, drive: bool) {
    if let Ok(mut g) = DOORS.lock() {
        if let Some(Some(d)) = g.get_mut(slot) {
            d.angle = angle.max(0.0);
            d.drive = drive;
        }
    }
}
/// The panel forgotten.
pub fn release(slot: usize) {
    if slot < SLOTS {
        ACTIVE[slot].store(0, Release);
        if let Ok(mut g) = DOORS.lock() {
            g[slot] = None;
        }
    }
}
/// The model-space transform of a bone's parent chain from the local pose (translation, rotation).
fn parent_model(buf: usize, parents: usize, bone: i32) -> Option<(V3, Q)> {
    let mut chain = [0i32; 64];
    let mut n = 0;
    let mut b = read::<i16>(parents + bone as usize * 2)? as i32;
    while b >= 0 && n < 64 {
        chain[n] = b;
        n += 1;
        b = read::<i16>(parents + b as usize * 2)? as i32;
    }
    let (mut t, mut q): (V3, Q) = ([0.0; 3], [0.0, 0.0, 0.0, 1.0]);
    for &i in chain[..n].iter().rev() {
        let lt = read::<V3>(buf + i as usize * BONE)?;
        let lq = read::<Q>(buf + i as usize * BONE + 16)?;
        let r = rot(&q, &lt);
        t = [t[0] + r[0], t[1] + r[1], t[2] + r[2]];
        q = qmul(&q, &lq);
    }
    Some((t, q))
}
/// The completed pose of an object's descriptor: each registered panel of it observed (the graph's angle, the hinge)
/// and, when driven, its bone turned to the wanted angle about the hinge.
pub fn pose_completed(descriptor: usize) {
    if descriptor == 0 || !ACTIVE.iter().any(|a| a.load(Acquire) == descriptor) {
        return;
    }
    let Ok(mut g) = DOORS.try_lock() else { return };
    let Some(buf) = read::<usize>(descriptor).filter(|b| *b != 0) else { return };
    let (Some(capacity), Some(slots)) = (read::<u32>(descriptor + 8), read::<u32>(descriptor + 12)) else { return };
    if slots > capacity || slots > MAX_BONES {
        return;
    }
    let write = !observe_only() && writable(buf, slots as usize * BONE);
    // SAFETY: a plain OS call.
    let now = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    for d in g.iter_mut().flatten().filter(|d| d.descriptor == descriptor) {
        if d.bone < 0 || d.bone as u32 >= slots {
            continue;
        }
        // the hinge not known yet: whichever bone of the chain the graph turns first
        if !d.have_axis {
            for k in 0..4 {
                let b = d.chain[k];
                if b < 0 || b as u32 >= slots {
                    continue;
                }
                let Some(g) = read::<Q>(buf + b as usize * BONE + 16).and_then(normalized) else { continue };
                let delta = qmul(&g, &conj(&d.chain_closed[k]));
                if 2.0 * delta[3].abs().min(1.0).acos() > 2f32.to_radians() {
                    if d.bone != b {
                        crate::vrik::env::log(&format!("[vehicle-doors] the hinge is bone {b} (the slot's bone {} is {} below it)\n", d.chain[0], k));
                    }
                    d.bone = b;
                    d.closed = d.chain_closed[k];
                    d.have_closed = true;
                    break;
                }
            }
        }
        let at = buf + d.bone as usize * BONE + 16;
        let Some(graph) = read::<Q>(at).and_then(normalized) else { continue };
        d.seen = now;
        if !d.have_closed {
            // no reference rotation: the door as the graph has it on the first pass (registered shut)
            d.closed = graph;
            d.have_closed = true;
        }
        let delta = qmul(&graph, &conj(&d.closed));
        let half = delta[3].abs().min(1.0).acos();
        let angle = 2.0 * half;
        if angle > 2f32.to_radians() && half.sin() > 1e-4 {
            let s = if delta[3] < 0.0 { -1.0 } else { 1.0 } / half.sin();
            let axis = [delta[0] * s, delta[1] * s, delta[2] * s];
            if !d.have_axis {
                d.axis = axis;
                d.have_axis = true;
            }
        }
        d.graph_angle = angle;
        d.passes = d.passes.wrapping_add(1);
        if d.passes % 120 == 1 {
            crate::vrik::env::log(&format!("[vehicle-doors] bone {}: the graph's angle {:.1} deg (open {:.1}), hinge {}, driven {} at {:.1} deg
", d.bone, angle.to_degrees(), d.open_max.to_degrees(), d.have_axis, d.drive, d.angle.to_degrees()));
        }
        d.open_max = d.open_max.max(angle);
        if let Some(rig) = read::<usize>(d.object + 8) {
            if let Some(parents) = read::<usize>(rig + META_PARENTS) {
                if let (Some((pt, pq)), Some(lt)) = (parent_model(buf, parents, d.bone), read::<V3>(buf + d.bone as usize * BONE)) {
                    let r = rot(&pq, &lt);
                    d.pivot = [pt[0] + r[0], pt[1] + r[1], pt[2] + r[2]];
                    if d.have_axis {
                        d.model_axis = rot(&pq, &d.axis);
                        d.have_model = true;
                    }
                }
            }
        }
        if !write || d.bone as u32 >= slots {
            continue;
        }
        if d.drive && d.have_axis {
            let a = d.angle.min(d.open_max.max(0.0));
            let q = qmul(&axis_angle(&d.axis, a), &d.closed);
            // SAFETY: the pass's own bone buffer, the bone within its slots.
            unsafe { core::ptr::write_unaligned(at as *mut Q, q) };
        } else if d.drive {
            // SAFETY: as above: held shut until the hinge is known.
            unsafe { core::ptr::write_unaligned(at as *mut Q, d.closed) };
        }
    }
}
