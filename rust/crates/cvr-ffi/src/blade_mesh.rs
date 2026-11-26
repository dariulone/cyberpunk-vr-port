//! main's blade mesh marks (src/Anim/BladeMeshInstance.cpp, with BladeMeshBlob.cpp and BladeMeshCut.cpp in
//! cvr-physhands): the struck person's skinned mesh copied under a private depot path (the depot reads the source's
//! bytes for it: archive_ext's depot hooks ask [`depot_source`]), the copy's render blob cut and painted before its
//! upload ([`before_private_upload`], from the mesh post-load), and the component switched to the copy once it is on
//! the GPU (its ink chunks shown with their faces' chunks, the component reloaded). At most nine copies live -- eight
//! owners and one to replace a resident mesh; a new person takes the least recently marked one, given back its own
//! mesh first. Strokes queued while a copy loads go into the next one.
use core::ffi::c_void;
use core::mem::ManuallyDrop;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use cvr_host::log;
use cvr_physhands::blade_mesh_blob as blob;
use cvr_physhands::blade_mesh_cut as cut;
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::{Handle, RefCnt, WeakHandle};
use red4ext_abi::resource::TokenRef;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

type V4 = [f32; 4];

const PREFIX: u64 = 0xd3a6_c500_0000_0000;
const CAPACITY: usize = 9;
const MAX_SEGMENTS: usize = 12;

// ---- the engine's layouts (2.5; RED4ext.SDK's)
const COMPONENT_NAME: usize = 0x40;
const COMPONENT_OWNER: usize = 0x50;
const COMPONENT_ENABLED: usize = 0x8B;
const ENTITY_COMPONENTS: usize = 0xA0;
const SKIN_SKINNING: usize = 0x188; // entISkinTargetComponent: Handle<entSkinningBinding>
const SKIN_RENDER_PROXY: usize = 0x1E0; // entSkinnedMeshComponent: SharedPtr<IRenderProxy>
const SKIN_APPEARANCE: usize = 0x1F0; // Handle<meshMeshAppearance>
const SKIN_MESH_HANDLE: usize = 0x200; // Handle<CMesh>
const SKIN_MESH_PATH: usize = 0x228; // RaRef<CMesh>
const SKIN_CHUNK_MASK: usize = 0x248;
const BINDING_ENABLED: usize = 0x30;
const BINDING_NAME: usize = 0x68;
const ANIMATED_RIG: usize = 0x138; // entAnimatedComponent: Ref<animRig> (path, token)
const ANIMATED_CONTROL: usize = 0x288; // Handle<entAnimationControlBinding>
const RIG_PARENTS: usize = 0x40;
const RIG_BONE_NAMES: usize = 0x50;
const SLOTS: usize = 0x120; // entSlotComponent: DynArray<entSlot> (0x30 each)
const SLOT_SIZE: usize = 0x30;
const RESOURCE_PATH: usize = 0x30; // CResource
const MESH_BONE_NAMES: usize = 0x80;
const MESH_BONE_MATRICES: usize = 0x90;
const MESH_MATERIAL_ENTRIES: usize = 0xE0;
const MESH_EXTERNAL_MATERIALS: usize = 0xF0;
const MESH_APPEARANCES: usize = 0x1E0;
const MESH_RENDER_RESOURCE: usize = 0x1F0;
const MESH_BLOB: usize = 0x1F8; // Handle<IRenderResourceBlob>
const RENDER_CHUNKS: usize = 0xA0; // CRenderMesh: DynArray<rendChunk>
const APPEARANCE_CHUNK_MATERIALS: usize = 0x38;
const BLOB_HEADER: usize = 0x30; // rendRenderMeshBlob
const BLOB_BUFFER: usize = 0x110; // DataBuffer: RawBuffer first
// rendRenderMeshBlobHeader
const H_RENDER_CHUNKS: usize = 0x28;
const H_CHUNK_INFOS: usize = 0x38;
const H_SPEED_TREE: usize = 0x48;
const H_OPACITY: usize = 0x58;
const H_CUSTOM: usize = 0x68;
const H_TOPOLOGY_DATA: usize = 0x78;
const H_TOPOLOGY_META: usize = 0x88;
const H_TOPOLOGY: usize = 0x98;
const H_VB_SIZE: usize = 0xB4;
const H_IB_SIZE: usize = 0xB8;
const H_IB_OFFSET: usize = 0xBC;
const H_SCALE: usize = 0xC0;
const H_OFFSET: usize = 0xD0;
// rendChunk (0xF8)
const CHUNK_SIZE: usize = 0xF8;
const C_ELEMENTS: usize = 0x00; // StaticArray<PackingElement(5), 32>, its size at 0xA0
const C_ELEMENT_COUNT: usize = 0xA0;
const C_SLOT_STRIDES: usize = 0xA4; // StaticArray<u8, 8>, its size at 0xAC
const C_SLOT_STRIDE_COUNT: usize = 0xAC;
const C_BYTE_OFFSETS: usize = 0xB8; // StaticArray<u32, 5>, its size at 0xCC
const C_BYTE_OFFSET_COUNT: usize = 0xCC;
const C_INDEX_TYPE: usize = 0xD0;
const C_INDEX_OFFSET: usize = 0xD4;
const C_MATERIAL_ID: usize = 0xD8; // DynArray<CName>
const C_NUM_INDICES: usize = 0xE8;
const C_NUM_VERTICES: usize = 0xEC;
const C_LOD_MASK: usize = 0xF5;
const TOPOLOGY_SIZE: usize = 0x20; // rendTopologyData (two DynArray<u8>)

unsafe fn at<T: core::marker::Copy>(base: *const c_void, offset: usize) -> T {
    unsafe { core::ptr::read_unaligned((base as *const u8).add(offset) as *const T) }
}
unsafe fn put<T>(base: *mut c_void, offset: usize, v: T) {
    unsafe { core::ptr::write_unaligned((base as *mut u8).add(offset) as *mut T, v) }
}
unsafe fn arr<'a, T>(base: *const c_void, offset: usize) -> &'a mut DynArray<T> {
    unsafe { &mut *((base as *mut u8).add(offset) as *mut DynArray<T>) }
}
fn is_a(object: *const c_void, class: &str) -> bool {
    // SAFETY: a live object.
    !object.is_null() && unsafe { red4ext_abi::rtti::object_class(object) }.is_some_and(|c| c.is_a(CName::hashed(class)))
}
fn instance_at(base: *const c_void, offset: usize) -> *mut c_void {
    // SAFETY: a Handle field's instance.
    unsafe { at::<*mut c_void>(base, offset) }
}
/// A component's entity's components (their instances).
fn components(entity: *const c_void) -> Vec<*mut c_void> {
    if entity.is_null() {
        return Vec::new();
    }
    // SAFETY: entEntity's components: handles (instance, counts).
    unsafe { arr::<[usize; 2]>(entity, ENTITY_COMPONENTS) }.as_slice().iter().map(|h| h[0] as *mut c_void).filter(|p| !p.is_null()).collect()
}
fn token_resource(token: *const c_void) -> *mut c_void {
    let t = ManuallyDrop::new(TokenRef { instance: token as *mut c_void, ref_count: core::ptr::null_mut::<RefCnt>() });
    if t.loaded() {
        t.resource()
    } else {
        core::ptr::null_mut()
    }
}

// ---- the copies
#[derive(Clone)]
struct Segment {
    volume: cut::Volume,
    bone: u64,
    swept: bool,
    rays: [cut::Point; 4],
    serial: u32,
    mark: blob::Mark,
    visible_chunks: u64,
    shell_depth: f64,
    skin_region: [bool; 256],
}
struct Copy {
    owner: WeakHandle,
    previous: u64,
    touched: u64,
    original_mask: u64,
    managed_mask: u64,
    history: Vec<Segment>,
    pending: Vec<Segment>,
    token: TokenRef,
    counts: blob::Counts,
    ready: bool,
    finished: bool,
}
impl Copy {
    fn empty() -> Copy {
        Copy { owner: WeakHandle::null(), previous: 0, touched: 0, original_mask: 0, managed_mask: 0, history: Vec::new(), pending: Vec::new(), token: TokenRef::null(), counts: blob::Counts::default(), ready: false, finished: false }
    }
}
// SAFETY: the copies are read and written under their lock; the handles and the token are the engine's, shared.
unsafe impl Send for Copy {}
/// The copies' private paths and sources, for the depot hooks' lock-free lookup.
static PATHS: [AtomicU64; CAPACITY] = [const { AtomicU64::new(0) }; CAPACITY];
static SOURCES: [AtomicU64; CAPACITY] = [const { AtomicU64::new(0) }; CAPACITY];
static COPIES: Mutex<Vec<Copy>> = Mutex::new(Vec::new());
static ENABLED: AtomicBool = AtomicBool::new(false);
static WORKING: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn tick_ms() -> u64 {
    // SAFETY: a plain OS call.
    unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() }
}
fn with<T>(f: impl FnOnce(&mut Vec<Copy>) -> T) -> T {
    let mut g = COPIES.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_empty() {
        g.extend((0..CAPACITY).map(|_| Copy::empty()));
    }
    f(&mut g)
}
fn lookup(path: u64) -> Option<usize> {
    if path & 0xffff_ff00_0000_0000 != PREFIX {
        return None;
    }
    let slot = (path & 255) as usize;
    (slot < CAPACITY && PATHS[slot].load(Ordering::Acquire) == path).then_some(slot)
}
fn reset(copies: &mut [Copy], i: usize) {
    PATHS[i].store(0, Ordering::Release);
    copies[i] = Copy::empty();
}
fn free_copy(replacing: bool) -> Option<usize> {
    let mut first = None;
    for i in 0..CAPACITY {
        if PATHS[i].load(Ordering::Acquire) == 0 {
            if replacing {
                return Some(i);
            }
            if first.is_some() {
                return first;
            }
            first = Some(i);
        }
    }
    None
}
fn mesh_path(component: *const c_void) -> u64 {
    // SAFETY: entSkinnedMeshComponent's mesh path.
    unsafe { at::<u64>(component, SKIN_MESH_PATH) }
}
fn chunk_mask(component: *const c_void) -> u64 {
    // SAFETY: as above.
    unsafe { at::<u64>(component, SKIN_CHUNK_MASK) }
}
fn set_chunk_mask(component: *mut c_void, mask: u64) {
    // SAFETY: as above.
    unsafe { put(component, SKIN_CHUNK_MASK, mask) }
}
fn reload(component: &Handle) {
    crate::engine_objects::reload_cut_mesh(component);
}
/// StartCopy: a private path for the component's mesh, its token loading.
fn start_copy(i: usize, component: &Handle, original: u64, history: Vec<Segment>) -> bool {
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    let path = PREFIX | ((generation & 0xFFFF_FFFF) << 8) | i as u64;
    let c = component.instance;
    let (mut original_mask, mut managed_mask) = (chunk_mask(c), 0);
    let current = mesh_path(c);
    with(|copies| {
        if let Some(old) = lookup(current) {
            original_mask = copies[old].original_mask;
            managed_mask = copies[old].managed_mask;
        }
        let copy = &mut copies[i];
        *copy = Copy::empty();
        copy.owner = WeakHandle::from_handle(component);
        copy.previous = current;
        copy.history = history;
        copy.original_mask = original_mask;
        copy.managed_mask = managed_mask;
        copy.touched = tick_ms();
        SOURCES[i].store(original, Ordering::Release);
        PATHS[i].store(path, Ordering::Release);
    });
    let token = TokenRef::load_async(path);
    with(|copies| {
        if token.is_null() {
            reset(copies, i);
            return false;
        }
        copies[i].token = token;
        WORKING.store(true, Ordering::Release);
        true
    })
}
/// PaintMask: the component's chunks with each ink chunk shown with the chunk it paints.
fn paint_mask(copy: &Copy, current: u64) -> u64 {
    let mut mask = (current & !copy.managed_mask) | (copy.original_mask & copy.managed_mask);
    for &(parent, ink) in &copy.counts.ink_chunks {
        let bit = 1u64 << ink;
        if mask & (1u64 << parent) != 0 {
            mask |= bit;
        } else {
            mask &= !bit;
        }
    }
    mask
}
/// VRBladeMeshPoll: each loading copy carried on -- switched in once its private mesh is on the GPU, the strokes
/// queued meanwhile started as the next copy, a stale or orphaned one given back.
pub(crate) fn poll() {
    if !WORKING.load(Ordering::Acquire) {
        return;
    }
    let mut any = false;
    for i in 0..CAPACITY {
        if PATHS[i].load(Ordering::Acquire) == 0 {
            continue;
        }
        enum Next {
            Skip,
            Reload(Handle),
            Start(usize, Handle, u64, Vec<Segment>),
        }
        let next = with(|copies| {
            let path = PATHS[i].load(Ordering::Acquire);
            let copy = &mut copies[i];
            if copy.token.is_null() || !copy.token.finished() {
                any = true;
                return Next::Skip;
            }
            let owner = copy.owner.lock();
            // SAFETY: the component's owner field.
            if owner.is_null() || unsafe { at::<*const c_void>(owner.instance, COMPONENT_OWNER) }.is_null() || !copy.token.loaded() {
                reset(copies, i);
                return Next::Skip;
            }
            let o = owner.instance;
            if copy.finished && mesh_path(o) != path {
                reset(copies, i);
                return Next::Skip;
            }
            let mesh = copy.token.resource();
            let mesh_handle = instance_at(o, SKIN_MESH_HANDLE);
            if !copy.finished {
                // SAFETY: the private mesh's path.
                if mesh_path(o) != copy.previous || mesh.is_null() || mesh_handle.is_null() || unsafe { at::<u64>(mesh, RESOURCE_PATH) } != path || mesh_handle == mesh {
                    reset(copies, i);
                    return Next::Skip;
                }
                // SAFETY: both meshes' render resources.
                let (mine, theirs) = unsafe { (at::<*const c_void>(mesh, MESH_RENDER_RESOURCE), at::<*const c_void>(mesh_handle, MESH_RENDER_RESOURCE)) };
                if copy.ready && !mine.is_null() && mine != theirs {
                    set_chunk_mask(o, paint_mask(copy, chunk_mask(o)));
                    for &(_, ink) in &copy.counts.ink_chunks {
                        copy.managed_mask |= 1u64 << ink;
                    }
                    // SAFETY: the component's mesh path, switched to the copy.
                    unsafe { put(o, SKIN_MESH_PATH, path) };
                    copy.finished = true;
                    any = true;
                    return Next::Reload(owner);
                }
                if copy.pending.is_empty() {
                    reset(copies, i);
                    return Next::Skip;
                }
            }
            any = true;
            if copy.finished && mesh_handle == mesh {
                let mask = paint_mask(copy, chunk_mask(o));
                if mask != chunk_mask(o) {
                    set_chunk_mask(o, mask);
                    return Next::Reload(owner);
                }
            }
            if copy.pending.is_empty() || (copy.finished && mesh_handle != mesh) {
                return Next::Skip;
            }
            let target = if copy.finished { free_copy(true) } else { Some(i) };
            let Some(target) = target else { return Next::Skip };
            let mut history = copy.history.clone();
            history.append(&mut copy.pending);
            if history.len() > MAX_SEGMENTS {
                history.drain(..history.len() - MAX_SEGMENTS);
            }
            let source = SOURCES[i].load(Ordering::Acquire);
            if !copy.finished {
                reset(copies, i);
            }
            Next::Start(target, owner, source, history)
        });
        match next {
            Next::Skip => {}
            Next::Reload(owner) => reload(&owner),
            Next::Start(target, owner, source, history) => {
                start_copy(target, &owner, source, history);
            }
        }
    }
    WORKING.store(any, Ordering::Release);
}
fn append(copy: &mut Copy, stroke: &Segment) -> bool {
    let Some(last) = copy.pending.last().or(copy.history.last()) else { return false };
    if last.mark == stroke.mark && last.serial == stroke.serial && last.bone == stroke.bone && cut::segment_distance(&last.volume, &stroke.volume) < 0.018 {
        return false;
    }
    if copy.pending.len() >= MAX_SEGMENTS {
        copy.pending.remove(0);
    }
    copy.pending.push(stroke.clone());
    copy.touched = tick_ms();
    true
}
/// EvictResident: the least recently marked resident copy of another person given back its own mesh.
fn evict_resident(incoming: *const c_void) -> Option<usize> {
    let (slot, owner) = with(|copies| {
        let mut oldest: Option<usize> = None;
        for i in 0..CAPACITY {
            let c = &copies[i];
            let owner = c.owner.lock();
            // SAFETY: the component's owner.
            if c.finished && c.pending.is_empty() && !owner.is_null() && unsafe { at::<*const c_void>(owner.instance, COMPONENT_OWNER) } != incoming && mesh_path(owner.instance) == PATHS[i].load(Ordering::Acquire) && oldest.is_none_or(|o| c.touched < copies[o].touched) {
                oldest = Some(i);
            }
        }
        let i = oldest?;
        let owner = copies[i].owner.lock();
        if owner.is_null() || mesh_path(owner.instance) != PATHS[i].load(Ordering::Acquire) {
            return None;
        }
        let o = owner.instance;
        // SAFETY: the component's mesh path given back.
        unsafe { put(o, SKIN_MESH_PATH, SOURCES[i].load(Ordering::Acquire)) };
        let c = &copies[i];
        set_chunk_mask(o, (chunk_mask(o) & !c.managed_mask) | (c.original_mask & c.managed_mask));
        reset(copies, i);
        Some((i, owner))
    })?;
    reload(&owner);
    Some(slot)
}
/// Queue: the stroke appended to the component's loading or resident copy, else a new copy started.
fn queue(component: &Handle, stroke: &Segment) -> bool {
    let original = mesh_path(component.instance);
    if original == 0 {
        return false;
    }
    let me = component.instance;
    let appended = with(|copies| {
        for i in 0..CAPACITY {
            if PATHS[i].load(Ordering::Acquire) != 0 && !copies[i].finished && copies[i].previous == original && copies[i].owner.lock().instance == me {
                return Some(append(&mut copies[i], stroke));
            }
        }
        if let Some(old) = lookup(original) {
            let c = &mut copies[old];
            return Some(c.finished && c.owner.lock().instance == me && append(c, stroke));
        }
        None
    });
    if let Some(done) = appended {
        return done;
    }
    // SAFETY: the component's owner.
    let incoming = unsafe { at::<*const c_void>(me, COMPONENT_OWNER) };
    let Some(available) = free_copy(false).or_else(|| evict_resident(incoming)) else { return false };
    start_copy(available, component, original, vec![stroke.clone()])
}

// ---- the skin region
/// SkinRegion: the bone and the helpers whose nearest ancestor with a hit slot is it, from the rigs of the skin's
/// animated components (snapshotted before the private mesh loads on its worker).
fn skin_region(target: *const c_void, source: u64, slots: *const c_void, bone: u64) -> HashSet<u64> {
    let mut names = HashSet::from([bone]);
    let all = components(target);
    let slot_bones: Vec<u64> = {
        // SAFETY: the slot component's slots.
        let s = unsafe { arr::<[u8; SLOT_SIZE]>(slots, SLOTS) };
        s.as_slice().iter().map(|e| u64::from_le_bytes(e[8..16].try_into().unwrap_or([0; 8]))).collect()
    };
    let mut controller = 0u64;
    for &c in &all {
        // SAFETY: a component's name.
        if unsafe { at::<u64>(c, COMPONENT_NAME) } == source && is_a(c, "entAnimatedComponent") {
            let binding = instance_at(c, ANIMATED_CONTROL);
            // SAFETY: the binding's fields.
            if !binding.is_null() && unsafe { at::<bool>(binding, BINDING_ENABLED) } {
                controller = unsafe { at::<u64>(binding, BINDING_NAME) };
            }
            break;
        }
    }
    for &c in &all {
        if !is_a(c, "entAnimatedComponent") {
            continue;
        }
        // retail skins often bind to "root", while the rig it drives supplies the muscles and twists of their palette
        let binding = instance_at(c, ANIMATED_CONTROL);
        // SAFETY: the component's and the binding's fields.
        let (name, bound, enabled) = unsafe {
            let name = at::<u64>(c, COMPONENT_NAME);
            if binding.is_null() {
                (name, 0, false)
            } else {
                (name, at::<u64>(binding, BINDING_NAME), at::<bool>(binding, BINDING_ENABLED))
            }
        };
        if name != source && name != controller && (!enabled || (bound != source && (controller == 0 || bound != controller))) {
            continue;
        }
        // SAFETY: the rig reference's token (path +0, token +8).
        let token = unsafe { at::<*const c_void>(c, ANIMATED_RIG + 8) };
        if token.is_null() {
            continue;
        }
        let rig = token_resource(token);
        if rig.is_null() {
            continue;
        }
        // SAFETY: the rig's parents and names (the same count).
        let (parents, bones) = unsafe { (at::<*const i16>(rig, RIG_PARENTS), arr::<u64>(rig, RIG_BONE_NAMES).as_slice()) };
        if parents.is_null() || bones.len() > 32768 || !bones.contains(&bone) {
            continue;
        }
        // each helper belongs to its nearest ancestor with a hit slot: a forearm's vertices are not an upper arm's
        let roots: Vec<i8> = bones.iter().map(|n| if *n == bone { 1 } else if slot_bones.contains(n) { 0 } else { -1 }).collect();
        for i in 0..roots.len() {
            let mut parent = i as i32;
            for _ in 0..roots.len() {
                if parent < 0 || parent as usize >= roots.len() {
                    break;
                }
                if roots[parent as usize] >= 0 {
                    if roots[parent as usize] == 1 {
                        names.insert(bones[i]);
                    }
                    break;
                }
                // SAFETY: the parents' array, the bones' count long.
                parent = i32::from(unsafe { *parents.add(parent as usize) });
            }
        }
    }
    names
}
fn starts_with_priority(name: &str) -> bool {
    name.starts_with("t0_") || name.starts_with("h0_") || name.starts_with("a0_")
}
/// VRBladeQueueSurfaceMark: a stroke from `from` to `to` on the bone's skinned triangles (kind 1 a slash, 2 a
/// puncture, 3 a trace); the copies it was queued to (0 when no mesh takes it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_surface_mark(target: &Handle, bone: u64, from: &V4, to: &V4, normal: &V4, depth: f32, serial: u32, old_start: &V4, old_end: &V4, start: &V4, end: &V4, kind: i32) -> u32 {
    if !(1..=3).contains(&kind) || target.is_null() || serial == 0 || !ENABLED.load(Ordering::Acquire) || !is_a(target.instance, "NPCPuppet") {
        return 0;
    }
    let p3 = |v: &V4| [f64::from(v[0]), f64::from(v[1]), f64::from(v[2])];
    let mark = match kind {
        1 => blob::Mark::Slash,
        2 => blob::Mark::Puncture,
        _ => blob::Mark::Trail,
    };
    let c = cut::Cut { from: p3(from), to: p3(to), normal: p3(normal), half_width: 0.004, depth: f64::from(depth), outside: 0.012 };
    let mut segment = Segment { volume: cut::Volume::default(), bone, swept: true, rays: [p3(old_start), p3(old_end), p3(start), p3(end)], serial, mark, visible_chunks: u64::MAX, shell_depth: f64::from(normal[3]), skin_region: [false; 256] };
    if !cut::make_volume(&c, &mut segment.volume) {
        return 0;
    }
    let mut candidates = Vec::new();
    let mut slots: *const c_void = core::ptr::null();
    for comp in components(target.instance) {
        // SAFETY: the component's fields.
        let name = unsafe { at::<u64>(comp, COMPONENT_NAME) };
        if name == CName::hashed("HitRepresentation").0 && is_a(comp, "entSlotComponent") {
            slots = comp;
        }
        // SAFETY: as above.
        if !unsafe { at::<bool>(comp, COMPONENT_ENABLED) } || !is_a(comp, "entSkinnedMeshComponent") || name == CName::hashed("AppearanceProxyMesh").0 || is_a(comp, "entAppearanceProxyMeshComponent") {
            continue;
        }
        let mesh = instance_at(comp, SKIN_MESH_HANDLE);
        // SAFETY: the skin's render proxy and mesh's render resource and bones.
        let ok = unsafe { !mesh.is_null() && !at::<*const c_void>(comp, SKIN_RENDER_PROXY).is_null() && !at::<*const c_void>(mesh, MESH_RENDER_RESOURCE).is_null() && arr::<u64>(mesh, MESH_BONE_NAMES).as_slice().contains(&bone) };
        if ok {
            candidates.push((comp, name));
        }
    }
    // the torso, head and arms first
    candidates.sort_by_key(|(_, n)| !starts_with_priority(CName(*n).text()));
    let mut regions: HashMap<u64, HashSet<u64>> = HashMap::new();
    let mut queued = 0u32;
    for (comp, _) in candidates {
        segment.visible_chunks = chunk_mask(comp);
        if !slots.is_null() {
            segment.skin_region = [false; 256];
            let skinning = instance_at(comp, SKIN_SKINNING);
            // SAFETY: the binding's fields.
            if !skinning.is_null() && unsafe { at::<bool>(skinning, BINDING_ENABLED) } {
                let source = unsafe { at::<u64>(skinning, BINDING_NAME) };
                let region = regions.entry(source).or_insert_with(|| skin_region(target.instance, source, slots, bone));
                let mesh = instance_at(comp, SKIN_MESH_HANDLE);
                // SAFETY: the mesh's palette.
                for (i, n) in unsafe { arr::<u64>(mesh, MESH_BONE_NAMES) }.as_slice().iter().enumerate().take(256) {
                    segment.skin_region[i] = region.contains(n);
                }
            }
        }
        // SAFETY: the component, held by its entity.
        let handle = unsafe { Handle::from_instance(comp) };
        if queue(&handle, &segment) {
            queued += 1;
        }
    }
    queued
}
/// VRBladePaintPoint: the painted contact's point and normal in the bone's frame (w: the mark, the ink triangles);
/// none before the paint is shown.
pub(crate) fn paint_point(target: &Handle, bone: u64, serial: u32) -> Vec<V4> {
    if target.is_null() || serial == 0 {
        return Vec::new();
    }
    let ink_material = CName::hashed("vrport_surface_ink").0;
    with(|copies| {
        let mut nearest = f64::INFINITY;
        let mut out = Vec::new();
        for i in 0..CAPACITY {
            let path = PATHS[i].load(Ordering::Acquire);
            if path == 0 {
                continue;
            }
            let copy = &copies[i];
            let owner = copy.owner.lock();
            let c = &copy.counts;
            if !copy.finished || owner.is_null() || c.contact_edit < 0 {
                continue;
            }
            let o = owner.instance;
            let mesh = instance_at(o, SKIN_MESH_HANDLE);
            // SAFETY: the component's and its mesh's fields.
            let ok = unsafe {
                at::<*const c_void>(o, COMPONENT_OWNER) == target.instance as *const c_void
                    && at::<bool>(o, COMPONENT_ENABLED)
                    && !at::<*const c_void>(o, SKIN_RENDER_PROXY).is_null()
                    && !mesh.is_null()
                    && at::<u64>(mesh, RESOURCE_PATH) == path
                    && !at::<*const c_void>(mesh, MESH_RENDER_RESOURCE).is_null()
            };
            let mask = chunk_mask(o);
            if !ok || mask & (1u64 << c.contact_chunk) == 0 || mask & (1u64 << c.contact_ink_chunk) == 0 {
                continue;
            }
            // SAFETY: the render resource's chunks.
            let chunks = unsafe { arr::<[u8; CHUNK_SIZE]>(at::<*const c_void>(mesh, MESH_RENDER_RESOURCE), RENDER_CHUNKS) };
            let Some(chunk) = chunks.as_slice().get(c.contact_ink_chunk as usize) else { continue };
            if u32::from_le_bytes(chunk[C_NUM_INDICES..C_NUM_INDICES + 4].try_into().unwrap_or([0; 4])) < 3 {
                continue;
            }
            let appearance = instance_at(o, SKIN_APPEARANCE);
            if !appearance.is_null() {
                // SAFETY: the appearance's chunk materials.
                let materials = unsafe { arr::<u64>(appearance, APPEARANCE_CHUNK_MATERIALS) };
                if materials.as_slice().get(c.contact_ink_chunk as usize) != Some(&ink_material) {
                    continue;
                }
            }
            let Some(stroke) = copy.history.get(c.contact_edit as usize) else { continue };
            if stroke.bone != bone || stroke.serial != serial || c.entry_distance_squared >= nearest {
                continue;
            }
            nearest = c.entry_distance_squared;
            out = vec![
                [c.contact[0] as f32, c.contact[1] as f32, c.contact[2] as f32, stroke.mark as i32 as f32],
                [c.normal[0] as f32, c.normal[1] as f32, c.normal[2] as f32, c.ink_triangles as f32],
            ];
        }
        out
    })
}

// ---- the depot and the upload
/// MeshDepotSource: the source a private copy's path reads (any other path itself).
pub(crate) fn depot_source(requested: u64) -> u64 {
    lookup(requested).map_or(requested, |i| SOURCES[i].load(Ordering::Acquire))
}
/// EnableMeshCopies: the depot's hooks and the mesh post-load are in.
pub(crate) fn enable(on: bool) {
    ENABLED.store(on, Ordering::Release);
}
fn read_chunk(c: &[u8; CHUNK_SIZE]) -> blob::ChunkInfo {
    let n = (u32::from_le_bytes(c[C_ELEMENT_COUNT..C_ELEMENT_COUNT + 4].try_into().unwrap_or([0; 4])) as usize).min(32);
    let elements = (0..n)
        .map(|i| {
            let e = &c[C_ELEMENTS + 5 * i..C_ELEMENTS + 5 * i + 5];
            blob::Element { ty: e[0] as i8, usage: e[1] as i8, usage_index: e[2], stream_index: e[3], stream_type: e[4] as i8 }
        })
        .collect();
    let strides = (u32::from_le_bytes(c[C_SLOT_STRIDE_COUNT..C_SLOT_STRIDE_COUNT + 4].try_into().unwrap_or([0; 4])) as usize).min(8);
    let offsets = (u32::from_le_bytes(c[C_BYTE_OFFSET_COUNT..C_BYTE_OFFSET_COUNT + 4].try_into().unwrap_or([0; 4])) as usize).min(5);
    let u32_at = |o: usize| u32::from_le_bytes(c[o..o + 4].try_into().unwrap_or([0; 4]));
    blob::ChunkInfo {
        elements,
        slot_strides: c[C_SLOT_STRIDES..C_SLOT_STRIDES + strides].to_vec(),
        byte_offsets: (0..offsets).map(|i| u32_at(C_BYTE_OFFSETS + 4 * i)).collect(),
        index_type: c[C_INDEX_TYPE] as i8,
        index_offset: u32_at(C_INDEX_OFFSET),
        num_vertices: u32::from(u16::from_le_bytes([c[C_NUM_VERTICES], c[C_NUM_VERTICES + 1]])),
        num_indices: u32_at(C_NUM_INDICES),
        lod_mask: c[C_LOD_MASK],
    }
}
/// A new element at the end of a DynArray of engine structs, a copy of `source` (its own type's copy).
unsafe fn push_struct(array: *mut c_void, element: &str, source: *const c_void) -> bool {
    let Some(rtti) = red4ext_abi::rtti::RttiSystem::get() else { return false };
    let Some(ty) = rtti.get_type(CName::hashed(&format!("array:{element}"))) else { return false };
    unsafe {
        let n = ty.array_length(array);
        if !ty.array_insert_at(array, n as i32) {
            return false;
        }
        if !source.is_null() {
            let inner = &*ty.array_inner();
            inner.assign(ty.array_element(array, n), source);
        }
    }
    true
}
/// The blob's IAllocator (the RawBuffer's copy of its vtable at +0x10): AllocAligned and Free.
unsafe fn buffer_alloc(raw: *mut u8, size: u32, alignment: u32) -> *mut c_void {
    type F = unsafe extern "C" fn(*const c_void, *mut red4ext_abi::memory::AllocationResult, u64, u32) -> *mut red4ext_abi::memory::AllocationResult;
    unsafe {
        let allocator = raw.add(0x10) as *const c_void;
        if at::<usize>(allocator, 0) == 0 {
            return core::ptr::null_mut();
        }
        let f: F = red4ext_abi::rtti::vfunc(allocator, 0x08);
        let mut result = red4ext_abi::memory::AllocationResult { memory: core::ptr::null_mut(), size: 0 };
        f(allocator, &mut result, u64::from(size), alignment);
        result.memory
    }
}
unsafe fn buffer_free(raw: *mut u8, memory: *mut c_void) {
    type F = unsafe extern "C" fn(*const c_void, *mut red4ext_abi::memory::AllocationResult);
    unsafe {
        let allocator = raw.add(0x10) as *const c_void;
        if memory.is_null() || at::<usize>(allocator, 0) == 0 {
            return;
        }
        let f: F = red4ext_abi::rtti::vfunc(allocator, 0x20);
        let mut a = red4ext_abi::memory::AllocationResult { memory, size: 0 };
        f(allocator, &mut a);
    }
}
/// BeforePrivateMeshUpload: a private copy's render blob cut before its upload (the mesh post-load, on the loader's
/// worker): the history's strokes against its bones, the changed chunks and their painted twins written back.
pub(crate) fn before_private_upload(mesh: *mut c_void) {
    if mesh.is_null() {
        return;
    }
    // SAFETY: the mesh's path.
    let path = unsafe { at::<u64>(mesh, RESOURCE_PATH) };
    let Some(slot) = lookup(path) else { return };
    // SAFETY: the mesh's bone names.
    let bones = unsafe { arr::<u64>(mesh, MESH_BONE_NAMES) }.as_slice().to_vec();
    let edits: Option<Vec<blob::Edit>> = with(|copies| {
        let copy = &copies[slot];
        if PATHS[slot].load(Ordering::Acquire) != path || copy.ready {
            return None;
        }
        copy.history
            .iter()
            .map(|s| {
                let bone = bones.iter().position(|b| *b == s.bone)?;
                let mut region = s.skin_region;
                if bone < 256 {
                    region[bone] = true;
                }
                Some(blob::Edit { volume: s.volume, bone: bone as i32, swept: s.swept, rays: s.rays, mark: s.mark, visible_chunks: s.visible_chunks, shell_depth: s.shell_depth, skin_region: region })
            })
            .collect()
    });
    let Some(edits) = edits else { return };
    let mut counts = blob::Counts::default();
    // SAFETY: the mesh in its post-load; the token cannot finish until it returns.
    let ready = unsafe { cut_mesh(mesh, &edits, &mut counts) };
    with(|copies| {
        if PATHS[slot].load(Ordering::Acquire) != path {
            return;
        }
        copies[slot].ready = ready;
        copies[slot].counts = counts.clone();
    });
    if trace() {
        log!(
            "[mesh-cut] prepared source={} ready={} split={} inserted={} ink={} shellProjection={} noOverlap={} key={}\n",
            SOURCES[slot].load(Ordering::Acquire),
            ready,
            counts.split,
            counts.inserted,
            counts.ink_triangles,
            counts.projected_from_shell,
            counts.no_overlap,
            path
        );
    }
}
fn trace() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| cvr_host::paths::test_marker("cpvr_mesh_cut_trace.flag"))
}
/// CutPrivateBlob's engine side: the blob read, the cut made (cvr-physhands), its result written into the mesh.
unsafe fn cut_mesh(mesh: *mut c_void, edits: &[blob::Edit], counts: &mut blob::Counts) -> bool {
    unsafe {
        let b = instance_at(mesh, MESH_BLOB);
        if b.is_null() || !is_a(b, "rendRenderMeshBlob") {
            return false;
        }
        let h = (b as *mut u8).add(BLOB_HEADER) as *mut c_void;
        let raw = (b as *mut u8).add(BLOB_BUFFER);
        let (data, size, alignment) = (at::<*const u8>(raw.cast(), 0), at::<u32>(raw.cast(), 8), at::<u32>(raw.cast(), 12));
        let empty = |o: usize| arr::<u8>(h, o).len() == 0;
        if data.is_null() || !empty(H_RENDER_CHUNKS) || !empty(H_CUSTOM) || !empty(H_SPEED_TREE) || !empty(H_TOPOLOGY_DATA) || !empty(H_TOPOLOGY_META) {
            return false;
        }
        let topology = arr::<[u8; TOPOLOGY_SIZE]>(h, H_TOPOLOGY);
        for t in topology.as_slice() {
            // each topology's data and metadata arrays (DynArray: ptr, capacity, size at +0x0C)
            if u32::from_le_bytes(t[12..16].try_into().unwrap_or([1; 4])) != 0 || u32::from_le_bytes(t[28..32].try_into().unwrap_or([1; 4])) != 0 {
                return false;
            }
        }
        let header = blob::Header {
            vertex_buffer_size: at::<u32>(h, H_VB_SIZE),
            index_buffer_size: at::<u32>(h, H_IB_SIZE),
            index_buffer_offset: at::<u32>(h, H_IB_OFFSET),
            scale: at::<[f32; 4]>(h, H_SCALE),
            offset: at::<[f32; 4]>(h, H_OFFSET),
        };
        let infos = arr::<[u8; CHUNK_SIZE]>(h, H_CHUNK_INFOS);
        let chunks: Vec<blob::ChunkInfo> = infos.as_slice().iter().map(read_chunk).collect();
        let matrices: Vec<blob::Matrix> = arr::<blob::Matrix>(mesh, MESH_BONE_MATRICES).as_slice().to_vec();
        let bytes = core::slice::from_raw_parts(data, size as usize);
        let Some(out) = blob::cut_private_blob(bytes, &header, &chunks, &matrices, edits, counts) else { return false };
        let original_count = chunks.len();
        if topology.len() != 0 && topology.len() as usize != original_count {
            return false;
        }
        let ink_count = out.updates.iter().filter(|u| u.ink_count != 0).count();
        let appearances = arr::<[usize; 2]>(mesh, MESH_APPEARANCES);
        if ink_count != 0 {
            if original_count + ink_count > 64 || appearances.len() == 0 || arr::<[u8; 16]>(mesh, MESH_EXTERNAL_MATERIALS).len() >= 65535 {
                return false;
            }
            for app in appearances.as_slice() {
                if app[0] == 0 || arr::<u64>(app[0] as *const c_void, APPEARANCE_CHUNK_MATERIALS).len() as usize != original_count {
                    return false;
                }
            }
        }
        let replacement = buffer_alloc(raw, out.buffer.len() as u32, alignment);
        if replacement.is_null() {
            return false;
        }
        core::ptr::copy_nonoverlapping(out.buffer.as_ptr(), replacement as *mut u8, out.buffer.len());
        let ink_material = CName::hashed("vrport_surface_ink").0;
        // precomputed ray-tracing opacity indexes the original triangles: the copy uses its materials' own opacity
        arr::<u8>(h, H_OPACITY).clear();
        if ink_count != 0 {
            let externals = arr::<[u8; 16]>(mesh, MESH_EXTERNAL_MATERIALS);
            let index = externals.len() as u16;
            // RaRef<IMaterial>: its path (the token comes with the load)
            let mut reference = [0u8; 16];
            reference[..8].copy_from_slice(&red4ext_abi::resource::resource_path("vrport\\fx\\mark_core.mi").to_le_bytes());
            externals.push(reference);
            let mut entry = [0u8; 0x30];
            entry[0x20..0x28].copy_from_slice(&ink_material.to_le_bytes());
            entry[0x28..0x2A].copy_from_slice(&index.to_le_bytes());
            push_struct((mesh as *mut u8).add(MESH_MATERIAL_ENTRIES).cast(), "CMeshMaterialEntry", entry.as_ptr().cast());
        }
        let mut stats_ink = Vec::new();
        for change in &out.updates {
            let c = infos.as_slice().as_ptr().add(change.index as usize) as *mut c_void;
            put(c, C_NUM_VERTICES, change.count as u16);
            put(c, C_NUM_INDICES, change.indices);
            put(c, C_INDEX_OFFSET, change.offset);
            for s in 0..5 {
                if change.starts[s] != 0 {
                    put(c, C_BYTE_OFFSETS + 4 * s, change.starts[s]);
                }
            }
            if change.ink_count != 0 {
                let new_index = infos.len() as u32;
                if !push_struct((h as *mut u8).add(H_CHUNK_INFOS).cast(), "rendChunk", c) {
                    return false;
                }
                let infos = arr::<[u8; CHUNK_SIZE]>(h, H_CHUNK_INFOS);
                let painted = infos.as_slice().as_ptr().add(new_index as usize) as *mut c_void;
                put(painted, C_NUM_INDICES, change.ink_count);
                put(painted, C_INDEX_OFFSET, change.ink_offset);
                arr::<u64>(painted, C_MATERIAL_ID).clear();
                stats_ink.push((change.index, new_index));
                if counts.contact_edit >= 0 && change.index == counts.contact_chunk {
                    counts.contact_ink_chunk = new_index;
                }
                if topology.len() != 0 {
                    push_struct((h as *mut u8).add(H_TOPOLOGY).cast(), "rendTopologyData", core::ptr::null());
                }
                for app in arr::<[usize; 2]>(mesh, MESH_APPEARANCES).as_slice() {
                    arr::<u64>(app[0] as *const c_void, APPEARANCE_CHUNK_MATERIALS).push(ink_material);
                }
            }
        }
        counts.ink_chunks = stats_ink;
        put(h, H_VB_SIZE, out.vertex_bytes);
        put(h, H_IB_SIZE, out.index_size);
        put(h, H_IB_OFFSET, out.index_start);
        let old = at::<*mut c_void>(raw.cast(), 0);
        put(raw.cast(), 0, replacement);
        put(raw.cast(), 8, out.buffer.len() as u32);
        buffer_free(raw, old);
        true
    }
}

/// The natives, with main's signatures: VRBladeQueueSurfaceMark, VRBladePaintPoint, VRBladeMeshPoll.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRBladeQueueSurfaceMark(_c: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, result: *mut u32, _: i64) {
    let mut a = unsafe { crate::native_args::args(frame) };
    let target = a.get::<Handle>();
    let bone = a.get::<u64>();
    let (from, to, normal) = (a.get::<V4>(), a.get::<V4>(), a.get::<V4>());
    let depth = a.get::<f32>();
    let serial = a.get::<u32>();
    let (os, oe, s, e) = (a.get::<V4>(), a.get::<V4>(), a.get::<V4>(), a.get::<V4>());
    let kind = a.get::<i32>();
    a.finish();
    let n = queue_surface_mark(&target, bone, &from, &to, &normal, depth, serial, &os, &oe, &s, &e, kind);
    if let Some(r) = unsafe { result.as_mut() } {
        *r = n;
    }
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRBladePaintPoint(_c: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, result: *mut DynArray<V4>, _: i64) {
    let mut a = unsafe { crate::native_args::args(frame) };
    let (target, bone, serial) = (a.get::<Handle>(), a.get::<u64>(), a.get::<u32>());
    a.finish();
    let Some(r) = (unsafe { result.as_mut() }) else { return };
    r.clear();
    for v in paint_point(&target, bone, serial) {
        r.push(v);
    }
}
/// VRBladeMeshInstanceProbe(target, control) -> String (main's 0.2.4 InspectPair): per enabled skinned mesh of
/// `target` with a render proxy, a line "name meshPath shared separateProxy chunks blob bufferBytes vbBytes ibBytes
/// vertices indices" -- whether `control` draws the same mesh, and through its own proxy.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRBladeMeshInstanceProbe(_c: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, result: *mut red4ext_abi::cstring::CString, _: i64) {
    let mut a = unsafe { crate::native_args::args(frame) };
    let (target, control) = (a.get::<Handle>(), a.get::<Handle>());
    a.finish();
    if result.is_null() {
        return;
    }
    let mut summary = String::new();
    if !target.instance.is_null() && !control.instance.is_null() && target.instance != control.instance {
        let skinned = |c: *const c_void| -> bool {
            // SAFETY: a live component's flag.
            let enabled = unsafe { at::<bool>(c, COMPONENT_ENABLED) };
            enabled && is_a(c, "entSkinnedMeshComponent")
        };
        // SAFETY (whole loop): the entities' live components and their meshes, read at the fields the cut uses.
        unsafe {
            for component in components(target.instance) {
                if !skinned(component) {
                    continue;
                }
                let mesh = instance_at(component, SKIN_MESH_HANDLE);
                let proxy = at::<*const c_void>(component, SKIN_RENDER_PROXY);
                let render = if mesh.is_null() { core::ptr::null() } else { at::<*const c_void>(mesh, MESH_RENDER_RESOURCE) };
                if mesh.is_null() || render.is_null() || proxy.is_null() {
                    continue;
                }
                let (mut shared, mut separate) = (false, false);
                for other in components(control.instance) {
                    if !skinned(other) || instance_at(other, SKIN_MESH_HANDLE) != mesh {
                        continue;
                    }
                    let other_proxy = at::<*const c_void>(other, SKIN_RENDER_PROXY);
                    if other_proxy.is_null() {
                        continue;
                    }
                    shared = true;
                    separate = other_proxy != proxy;
                    break;
                }
                let blob = instance_at(mesh, MESH_BLOB);
                let geometry = !blob.is_null() && is_a(blob, "rendRenderMeshBlob");
                let chunks = arr::<[u8; CHUNK_SIZE]>(render, RENDER_CHUNKS);
                let (mut vertices, mut indices) = (0u64, 0u64);
                for chunk in chunks.as_slice() {
                    vertices += u32::from_le_bytes(chunk[C_NUM_VERTICES..C_NUM_VERTICES + 4].try_into().unwrap_or([0; 4])) as u64;
                    indices += u32::from_le_bytes(chunk[C_NUM_INDICES..C_NUM_INDICES + 4].try_into().unwrap_or([0; 4])) as u64;
                }
                let (mut buffer, mut vb, mut ib) = (0u32, 0u32, 0u32);
                if geometry {
                    let raw = (blob as *const u8).add(BLOB_BUFFER) as *const c_void;
                    if !at::<*const u8>(raw, 0).is_null() {
                        buffer = at::<u32>(raw, 8);
                    }
                    let h = (blob as *const u8).add(BLOB_HEADER) as *const c_void;
                    vb = at::<u32>(h, H_VB_SIZE);
                    ib = at::<u32>(h, H_IB_SIZE);
                }
                let name = CName(at::<u64>(component, COMPONENT_NAME));
                let text = name.text();
                summary.push_str(&format!(
                    "{} {} {} {} {} {} {} {} {} {} {}\n",
                    if text.is_empty() { "?" } else { text.as_ref() },
                    at::<u64>(component, SKIN_MESH_PATH),
                    shared as i32,
                    separate as i32,
                    chunks.len(),
                    geometry as i32,
                    buffer,
                    vb,
                    ib,
                    vertices,
                    indices
                ));
            }
        }
    }
    // SAFETY: the VM's result string.
    unsafe { red4ext_abi::cstring::CString::assign(result, &summary) };
}
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRBladeMeshPoll(_c: *mut c_void, frame: *mut red4ext_abi::stack::StackFrame, _result: *mut c_void, _: i64) {
    let a = unsafe { crate::native_args::args(frame) };
    a.finish();
    poll();
}
