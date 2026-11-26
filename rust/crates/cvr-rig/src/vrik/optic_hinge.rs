//! The Osprey's screen hinge in the pose pass (main's src/Anim/OpticHinge.cpp, 0aa233e0a; the checks are cvr-body's
//! optic_hinge.rs). A request (cvr-ffi's VRPhysicalOpticsHinge, every frame CyberpunkVRPort_Optics aims the Osprey
//! through its screen) resolves the weapon's Barrel, its graph and the update bucket's entry of its animated object;
//! the pass that object's update runs (anim_pose_hook.rs's object scope) re-reads that entry and, when it is still the
//! request's, writes the hinge bone open after the native pose copy and the weapon rig's own writes. The request's
//! state is guarded by the rig tables' lock (pose_rig.rs); the engine's strong handles are taken and released outside it.
use super::pose_rig::RigPoseStateGuard;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::*};
use cvr_body::optic_hinge::{self as hinge, Identity, Pass, Target};
use cvr_engine::code::read;
use red4ext_abi::cname::CName;
use red4ext_abi::handle::WeakHandle;

/// requests, accepted, applied, invalidated, conflict.
#[no_mangle]
pub static CyberpunkVR_OpticsHingeStats: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
/// The diagnostic stages' counters (Stage) and the last one marked.
#[no_mangle]
pub static CyberpunkVR_OpticsHingeStages: [AtomicU64; 22] = [const { AtomicU64::new(0) }; 22];
#[no_mangle]
pub static CyberpunkVR_OpticsHingeLastStage: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
#[repr(u32)]
pub enum Stage {
    None,
    Disabled,
    Context,
    Player,
    Signature,
    Query,
    ActiveMismatch,
    Barrel,
    Control,
    Graph,
    Binding,
    Accepted,
    OwnerScope,
    Descriptor,
    CurrentBinding,
    ContextMeta,
    SourceRig,
    Track,
    Remap,
    Conflict,
    Applied,
    Expired,
}
pub fn mark(stage: Stage) {
    let i = stage as u32;
    CyberpunkVR_OpticsHingeStages[i as usize].fetch_add(1, Relaxed);
    CyberpunkVR_OpticsHingeLastStage.store(i, Relaxed);
}
pub fn stat(i: usize) {
    CyberpunkVR_OpticsHingeStats[i].fetch_add(1, Relaxed);
}

// entIComponent: name +0x40, owner +0x50; entEntity: runtimeScene +0xB8.
const COMPONENT_NAME: usize = 0x40;
const COMPONENT_OWNER: usize = 0x50;
const ENTITY_RUNTIME_SCENE: usize = 0xB8;
// entAnimatedComponent: rig (Ref<animRig>: its path +0, its token +8) +0x138, controlBinding (a handle) +0x288.
pub const ANIMATED_RIG: usize = 0x138;
pub const ANIMATED_CONTROL_BINDING: usize = 0x288;
// entIBinding: enabled +0x30, bindName +0x68.
pub const BINDING_ENABLED: usize = 0x30;
pub const BINDING_NAME: usize = 0x68;
// ResourceToken: the resource's handle +0x28, finished +0x58, error +0x5C.
const TOKEN_RESOURCE: usize = 0x28;
const TOKEN_FINISHED: usize = 0x58;
const TOKEN_ERROR: usize = 0x5C;
// animRig: parentIndices +0x40, referencePoseLS (QsTransform, 0x30 each: the rotation +0x10) +0x48, boneNames +0x50.
const RIG_PARENTS: usize = 0x40;
const RIG_REFERENCE_POSE: usize = 0x48;
const RIG_BONE_NAMES: usize = 0x50;
// animAnimatedObject: metaRigID +0, metaRig +8, the pose descriptor +0x18 (its [3] the track buffer).
const OBJECT_META_RIG_ID: usize = 0x00;
const OBJECT_META_RIG: usize = 0x08;
const OBJECT_DESCRIPTOR: usize = 0x18;
// animMetaRig: parentIndeces +0x10, boneNames +0x20 (DynArrays: the entries, then the size at +0xC).
const META_PARENTS: usize = 0x10;
const META_BONE_NAMES: usize = 0x20;
const ARRAY_SIZE: usize = 0xC;

const fn name(text: &str) -> u64 {
    CName::hashed(text).0
}
const BARREL: u64 = name("Barrel");
const RECEIVER: u64 = name("Receiver");
fn osprey_rig() -> u64 {
    static RIG: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *RIG.get_or_init(|| {
        red4ext_abi::resource::resource_path("ep1\\weapons\\firearms\\rifle_sniper\\nokota_osprey\\rig\\w_rifle_sniper__nokota_osprey__base1.rig")
    })
}

/// Where the animation system's update buckets keep their entries (cvr-ffi's anim_lookup layout, from the SDK).
#[derive(Clone, Copy, Default, Debug)]
pub struct Buckets {
    pub buckets: usize,
    pub stride: usize,
    pub count: u32,
    pub entities: usize,
    pub components: usize,
    pub objects: usize,
    pub entities_size: usize,
    pub components_size: usize,
    pub objects_size: usize,
    pub max_entries: u32,
}
/// A resolved request: the animation system, the weapon's scene, the bucket entry and what it held.
#[derive(Clone, Copy, Default, Debug)]
pub struct Binding {
    pub system: usize,
    pub scene: usize,
    pub bucket: u32,
    pub entry: u32,
    pub identity: Identity,
    pub layout: Buckets,
}

/// The request in force and the engine objects it resolved, weakly held.
#[derive(Default, Clone)]
pub struct Handles {
    pub player: WeakHandle,
    pub weapon: WeakHandle,
    pub component: WeakHandle,
    pub graph: WeakHandle,
    pub control: WeakHandle,
}
#[derive(Default, Clone)]
struct State {
    target: Target,
    binding: Binding,
    handles: Handles,
}
/// Under the rig tables' lock (RigPoseStateGuard).
static mut CURRENT: Option<State> = None;
static ACTIVE: AtomicBool = AtomicBool::new(false);
static ACTIVE_OBJECT: AtomicUsize = AtomicUsize::new(0);
static ACTIVE_DESCRIPTOR: AtomicUsize = AtomicUsize::new(0);

/// # Safety
/// The rig tables' lock is held.
unsafe fn current() -> &'static mut State {
    // SAFETY: the caller holds the lock.
    unsafe { (*(&raw mut CURRENT)).get_or_insert_with(State::default) }
}

unsafe extern "C" {
    /// cvr-render's vrcore_exports.rs (volatile int).
    static g_menuModeValue: i32;
    fn cvr_cpp_device_cam_active() -> bool;
}
/// The VR body bound, no menu, no device camera, no head-aimed weapon.
pub fn context() -> bool {
    // SAFETY: plain reads of the globals.
    unsafe {
        super::env::g_VRBind > 0
            && core::ptr::read_volatile(&raw const g_menuModeValue) == 0
            && !cvr_cpp_device_cam_active()
            && !super::weapon_arm::cvr_anim_is_head_aim_weapon_active()
    }
}
fn now() -> u64 {
    // SAFETY: plain Win32 call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}

fn clear_locked() {
    ACTIVE.store(false, Release);
    ACTIVE_OBJECT.store(0, Relaxed);
    ACTIVE_DESCRIPTOR.store(0, Relaxed);
    // SAFETY: under the lock; the weak handles' release only drops their counts.
    unsafe { *(&raw mut CURRENT) = None };
}
/// Clear: the request gone (disabled, holstered, stale, expired).
pub fn clear() {
    let _lock = RigPoseStateGuard::new();
    clear_locked();
}
/// The binding the last request resolved (Resolve reuses it while it still reads the same).
pub fn current_binding() -> Binding {
    let _lock = RigPoseStateGuard::new();
    // SAFETY: under the lock.
    unsafe { current().binding }
}
/// Accepted: the request in force from now (the binding and the handles it resolved).
pub fn accept(binding: Binding, handles: Handles) {
    let _lock = RigPoseStateGuard::new();
    // SAFETY: under the lock.
    let s = unsafe { current() };
    s.binding = binding;
    s.handles = handles;
    s.target = Target { identity: binding.identity, stamp: now(), enabled: true };
    ACTIVE_OBJECT.store(binding.identity.object, Relaxed);
    ACTIVE_DESCRIPTOR.store(binding.identity.descriptor, Relaxed);
    ACTIVE.store(true, Release);
    stat(1);
    mark(Stage::Accepted);
}

fn rd<T: Copy>(address: usize) -> Option<T> {
    if address < 0x10000 {
        return None;
    }
    read::<T>(address)
}
/// ReadBinding: the bucket entry read again -- still the weapon's, its Barrel the Osprey's rig (13 bones, the hinge at
/// 8 under the receiver, its rest the 180-degree fold), the graph and the control binding as resolved, the object's
/// pose descriptor and track buffer; every read guarded (a fault is a mismatch).
pub fn read_binding(b: &Binding) -> Option<Identity> {
    let l = &b.layout;
    if b.system == 0 || b.bucket >= l.count {
        return None;
    }
    let bucket = b.system + l.buckets + b.bucket as usize * l.stride;
    let (ne, nc, no) = (rd::<u32>(bucket + l.entities_size)?, rd::<u32>(bucket + l.components_size)?, rd::<u32>(bucket + l.objects_size)?);
    if ne > l.max_entries || nc > l.max_entries || no > l.max_entries {
        return None;
    }
    let i = b.entry as usize;
    if b.entry >= ne || b.entry >= nc || b.entry >= no {
        return None;
    }
    let weapon = rd::<usize>(bucket + l.entities + i * 16)?;
    let graph = rd::<usize>(bucket + l.components + i * 8)?;
    let object = rd::<usize>(bucket + l.objects + i * 8)?;
    let component = b.identity.component;
    if weapon == 0 || component == 0 || graph == 0 || object == 0 || weapon != b.identity.weapon || graph != b.identity.graph_component {
        return None;
    }
    if rd::<usize>(weapon + ENTITY_RUNTIME_SCENE)? != b.scene
        || rd::<usize>(component + COMPONENT_OWNER)? != weapon
        || rd::<usize>(graph + COMPONENT_OWNER)? != weapon
        || rd::<u64>(component + COMPONENT_NAME)? != BARREL
    {
        return None;
    }
    // the rig loaded: its token finished without an error
    let token = rd::<usize>(component + ANIMATED_RIG + 8)?;
    if token == 0 || rd::<i32>(token + TOKEN_FINISHED)? == 0 || rd::<u8>(token + TOKEN_ERROR)? != 0 {
        return None;
    }
    let control = rd::<usize>(component + ANIMATED_CONTROL_BINDING)?;
    if b.identity.control_binding != 0 {
        if control != b.identity.control_binding
            || rd::<u8>(control + BINDING_ENABLED)? == 0
            || rd::<u64>(control + BINDING_NAME)? != RECEIVER
            || rd::<u64>(graph + COMPONENT_NAME)? != rd::<u64>(control + BINDING_NAME)?
        {
            return None;
        }
    } else if graph != component || (control != 0 && rd::<u8>(control + BINDING_ENABLED)? != 0) {
        return None;
    }
    let rig = rd::<usize>(token + TOKEN_RESOURCE)?;
    let meta = rd::<usize>(object + OBJECT_META_RIG)?;
    if rig == 0 || meta == 0 || rd::<u64>(component + ANIMATED_RIG)? != osprey_rig() {
        return None;
    }
    let names = rd::<usize>(rig + RIG_BONE_NAMES)?;
    let parents = rd::<usize>(rig + RIG_PARENTS)?;
    if rd::<u32>(rig + RIG_BONE_NAMES + ARRAY_SIZE)? != hinge::BONE_COUNT || parents == 0 || names == 0 {
        return None;
    }
    if rd::<i16>(parents + 8 * 2)? != 1
        || rd::<i16>(parents + 2)? != 0
        || rd::<u64>(names + 8 * 8)? != hinge::BONE_NAME_HASH
        || rd::<u64>(names + 5 * 8)? != name("cocking_mechanism")
        || rd::<u64>(names + 12 * 8)? != name("handle")
    {
        return None;
    }
    let meta_names = rd::<u32>(meta + META_BONE_NAMES + ARRAY_SIZE)?;
    if meta_names == 0 || meta_names as usize > super::MAX_BONES || rd::<u32>(meta + META_PARENTS + ARRAY_SIZE)? != meta_names {
        return None;
    }
    let rest_pose = rd::<usize>(rig + RIG_REFERENCE_POSE)?;
    if rest_pose == 0 {
        return None;
    }
    let [ri, rj, rk, rr] = rd::<[f32; 4]>(rest_pose + 8 * 0x30 + 0x10)?;
    if !rk.is_finite() || (rk.abs() - 1.0).abs() > 0.0001 || !(ri + rj + rr).is_finite() || ri.abs() + rj.abs() + rr.abs() > 0.0001 {
        return None;
    }
    // The object's own pose descriptor (the final LocalToModel's object+0x18) and its track buffer: referenceTracks is
    // shared by equal rig assets and is never an owner key.
    let descriptor = rd::<usize>(object + OBJECT_DESCRIPTOR)?;
    let track = if descriptor != 0 { rd::<usize>(descriptor + 3 * 8)? } else { 0 };
    if descriptor == 0 || track == 0 {
        return None;
    }
    let result = Identity {
        player: b.identity.player,
        weapon,
        component,
        object,
        descriptor,
        rig,
        meta_rig: meta,
        track,
        meta_rig_id: rd::<u64>(object + OBJECT_META_RIG_ID)?,
        graph_component: graph,
        control_binding: b.identity.control_binding,
    };
    let same = rd::<usize>(bucket + l.entities + i * 16)? == weapon
        && rd::<usize>(bucket + l.components + i * 8)? == graph
        && rd::<usize>(bucket + l.objects + i * 8)? == object
        && (b.identity.control_binding == 0
            || (rd::<usize>(component + ANIMATED_CONTROL_BINDING)? == control
                && rd::<u8>(control + BINDING_ENABLED)? != 0
                && rd::<u64>(control + BINDING_NAME)? == rd::<u64>(graph + COMPONENT_NAME)?));
    same.then_some(result)
}
/// Scan: the system's buckets searched for the weapon's entry with the identity's graph, read as a binding.
pub fn scan(system: usize, scene: usize, layout: Buckets, identity: Identity) -> Option<Binding> {
    for b in 0..layout.count {
        let bucket = system + layout.buckets + b as usize * layout.stride;
        let Some(count) = rd::<u32>(bucket + layout.entities_size) else { continue };
        if count > layout.max_entries {
            continue;
        }
        let components = rd::<u32>(bucket + layout.components_size).unwrap_or(0);
        for i in 0..count {
            if rd::<usize>(bucket + layout.entities + i as usize * 16) != Some(identity.weapon)
                || i >= components
                || rd::<usize>(bucket + layout.components + i as usize * 8) != Some(identity.graph_component)
            {
                continue;
            }
            let mut candidate = Binding { system, scene, bucket: b, entry: i, identity, layout };
            if let Some(read) = read_binding(&candidate) {
                candidate.identity = read;
                return Some(candidate);
            }
        }
    }
    None
}

/// TryApply: the pass checked against the binding read now and the request, then the hinge's rotation written into the
/// slot the LOD's remap gives it. 1 applied, 0 not this pass, -1 the binding gone.
unsafe fn try_apply(binding: &Binding, target: &Target, object: usize, a1: *mut *mut u8, a2: *mut *mut u8, lod: u32, pose: *mut u8, bones: *mut u8, track: usize, now: u64) -> i32 {
    let Some(current) = read_binding(binding).filter(|c| *c == target.identity) else {
        mark(Stage::CurrentBinding);
        return -1;
    };
    let (Some(meta_now), Some(rig_now)) = (rd::<usize>(a2 as usize + 5 * 8), rd::<usize>(a1 as usize + 8 * 8)) else { return -1 };
    if meta_now != current.meta_rig {
        // an old in-flight graph context
        mark(Stage::ContextMeta);
        return 0;
    }
    if rig_now != current.rig {
        mark(Stage::SourceRig);
        return 0;
    }
    if pose as usize != current.descriptor {
        mark(Stage::Descriptor);
        return 0;
    }
    if track != current.track {
        mark(Stage::Track);
        return 0;
    }
    let player = super::roomscale_move::cvr_roomscale_player_identity();
    let pass = Pass {
        current,
        object,
        descriptor: pose as usize,
        rig: rig_now,
        track,
        player,
        meta_rig: meta_now,
        now,
        context: true,
        weak_alive: true,
        legacy_conflict: false,
    };
    if !hinge::matches(target, &pass) {
        mark(Stage::Expired);
        return 0;
    }
    let six = rd::<usize>(a2 as usize + 6 * 8).unwrap_or(0);
    let (map, counts) = if six != 0 { (rd::<usize>(six + 0x40).unwrap_or(0), rd::<usize>(six + 0x50).unwrap_or(0)) } else { (0, 0) };
    if map == 0 || counts == 0 {
        mark(Stage::Remap);
        return 0;
    }
    let (Some(count), Some(slots)) = (rd::<u16>(counts + lod as usize * 2), rd::<u32>(pose as usize + 8)) else { return -1 };
    let count = u32::from(count);
    let names_at = rd::<usize>(current.meta_rig + META_BONE_NAMES).unwrap_or(0);
    let names_n = rd::<u32>(current.meta_rig + META_BONE_NAMES + ARRAY_SIZE).unwrap_or(0);
    let mut applied = false;
    if count <= hinge::BONE_COUNT && names_at != 0 && !bones.is_null() {
        let mut pairs = [0u32; hinge::BONE_COUNT as usize * 2];
        let mut ok = true;
        for (k, p) in pairs.iter_mut().take(count as usize * 2).enumerate() {
            match rd::<u32>(map + k * 4) {
                Some(v) => *p = v,
                None => ok = false,
            }
        }
        if !ok {
            return -1;
        }
        let name_of = |i: u32| rd::<u64>(names_at + i as usize * 8);
        if let Some(d) = hinge::destination(target, &pass, &pairs[..count as usize * 2], slots, names_n, name_of) {
            // SAFETY: the pose's bone buffer, `slots` QsTransforms of 0x30 bytes; the rotation at +0x10.
            unsafe { core::ptr::copy_nonoverlapping(hinge::HIP_ROTATION.as_ptr(), bones.add(d as usize * 0x30 + 0x10) as *mut f32, 4) };
            applied = true;
        }
    }
    mark(if applied { Stage::Applied } else { Stage::Remap });
    i32::from(applied)
}

unsafe extern "C" {
    static mut g_RigWriteN: i32;
    static mut g_RigWriteWhich: [i32; 32];
    static mut g_RigWriteBone: [i32; 32];
    static mut g_RigWriteEnabled: [i32; 32];
}

/// ApplyOpticHinge: after the weapon rig's pass released the lock, in the pass of the requested object's update (its
/// scope from anim_pose_hook.rs): the request's handles locked outside the rig lock, then -- under it -- the request
/// expired (context, liveness, 200 ms, the player), a script rig write owning the hinge (a conflict), or the hinge
/// written.
///
/// # Safety
/// The pose pass's own buffers.
pub unsafe fn apply(object: usize, object_descriptor: usize, a1: *mut *mut u8, a2: *mut *mut u8, lod: u32, pose: *mut u8, bones: *mut u8, track: usize) {
    if !ACTIVE.load(Acquire) || object == 0 || object != ACTIVE_OBJECT.load(Relaxed) {
        return;
    }
    mark(Stage::OwnerScope);
    if object_descriptor != ACTIVE_DESCRIPTOR.load(Relaxed) {
        mark(Stage::Descriptor);
        return;
    }
    let snapshot = {
        let _lock = RigPoseStateGuard::new();
        // SAFETY: under the lock.
        let s = unsafe { current() };
        if !s.target.enabled {
            return;
        }
        s.clone()
    };
    let (player, weapon, component, graph, control) = (
        snapshot.handles.player.lock(),
        snapshot.handles.weapon.lock(),
        snapshot.handles.component.lock(),
        snapshot.handles.graph.lock(),
        snapshot.handles.control.lock(),
    );
    {
        let _lock = RigPoseStateGuard::new();
        // SAFETY: under the lock.
        let s = unsafe { current() };
        if !s.target.enabled || s.target.identity != snapshot.target.identity {
            return;
        }
        let now = now();
        let alive = !player.is_null() && !weapon.is_null() && !component.is_null() && !graph.is_null() && (s.target.identity.control_binding == 0 || !control.is_null());
        let owner = super::roomscale_move::cvr_roomscale_player_identity();
        if !context() || !alive || now < s.target.stamp || now - s.target.stamp >= hinge::FRESH_MS || owner != s.target.identity.player {
            mark(Stage::Expired);
            stat(3);
            clear_locked();
            return;
        }
        // SAFETY: the rig write tables, under their lock.
        let conflict = unsafe {
            let n = g_RigWriteN.clamp(0, 32) as usize;
            (0..n).any(|i| g_RigWriteEnabled[i] != 0 && g_RigWriteWhich[i] == 1 && g_RigWriteBone[i] == hinge::BONE as i32)
        };
        if conflict {
            mark(Stage::Conflict);
            stat(4);
            return;
        }
        let (binding, target) = (s.binding, s.target);
        // SAFETY: the pass's buffers; every engine read guarded.
        let result = unsafe { try_apply(&binding, &target, object, a1, a2, lod, pose, bones, track, now) };
        if result > 0 {
            stat(2);
        } else if result < 0 {
            stat(3);
            clear_locked();
        }
    }
    // the engine's strong handles released after the rig lock is gone
    drop((player, weapon, component, graph, control));
}
