//! THE PORT'S ARCHIVE, WITHOUT ARCHIVEXL (formerly src/Core/ArchiveExtensions.cpp's logic; Core/ArchiveExtensions.hpp
//! tells the whole of it). The port's archives are mounted in a mod group of their own as the resource depot
//! initializes its archives, and the manifest (config\archive.yaml, read by cvr_settings::archive_manifest) applied:
//! item factories after the game's own, animation sets on an entity's animated components, and resource patches -- a
//! streaming block merged into a streaming world, mesh appearances added to meshes. And two engine changes ArchiveXL
//! makes for every mod: world widget components past a layer's tenth are updated, and an entity spawned from a record
//! takes the record's appearanceName.
//!
//! The SDK's containers and resource types stay behind the C++ adapter in src/Core/ArchiveExtensions.cpp
//! (`cvr_cpp_depot_*`, `cvr_cpp_mesh_*`, `cvr_cpp_token_*` ...); what is mounted, prepared, loaded, merged and when is
//! here.
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::ffi::CStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};
use std::time::Instant;

use cvr_host::log;
use cvr_host::paths::{self, Folder};
use cvr_settings::archive_manifest::{self, Manifest};
use red4ext_abi::api::{PluginHandle, Sdk};
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::cstring::CString as EngineString;
use red4ext_abi::reloc;
use red4ext_abi::resource::resource_path;
use red4ext_abi::tweakdbid;

use crate::native_tweaks::{
    rt_rtti_type, rt_tdb_flats_lock_shared, rt_tdb_flats_unlock_shared,
    rt_tdb_get,
};
use crate::tweak_records::{cstr, flat};

/// SharedPtr<ResourceToken<CResource>>: the instance and its reference count (asserted in the C++ adapter).
pub(crate) type Token = [usize; 2];


// The depot, token, resource, component and mesh reads (formerly ArchiveExtensions.cpp's adapters), by the SDK's
// layouts: ResourceDepot (ResourceExists at vtable +0x18, after GetAllocator and the destructor), ResourceToken (the resource's handle at +0x28, the job at
// +0x50, finished at +0x58, error at +0x5C), CResource::path (+0x30), IComponent (name +0x40, owner +0x50),
// Entity::templatePath (+0x60), CMesh (forceLoadAllAppearances +0x6D, materialEntries +0xE0, externalMaterials +0xF0,
// appearances +0x1E0; a DynArray's size at +0xC), CMeshMaterialEntry (0x30 bytes: name +0x20, index +0x28,
// isLocalInstance +0x2A), MeshAppearance::name (+0x30).
static DEPOT: reloc::Reloc = reloc::Reloc::new(red4ext_abi::hashes::ResourceDepot);
static JOB_JOIN: reloc::Reloc = reloc::Reloc::new(red4ext_abi::hashes::JobHandle_Join);
/// A field of an engine object.
///
/// # Safety
/// `object + offset` holds a `T`.
unsafe fn field<T: Copy>(object: *const c_void, offset: usize) -> T {
    // SAFETY: as documented.
    unsafe { core::ptr::read_unaligned(object.cast::<u8>().add(offset).cast::<T>()) }
}
/// `ResourceDepot::Get()`: the global the relocation names, read.
pub(crate) unsafe fn rt_depot_get() -> *mut c_void {
    // SAFETY: the resolved global holds the depot's pointer.
    DEPOT.get().map_or(core::ptr::null_mut(), |at| unsafe { *(at as *const *mut c_void) })
}
/// `depot->ResourceExists(path)`.
///
/// # Safety
/// `depot` is the engine's depot.
pub(crate) unsafe fn rt_depot_exists(depot: *mut c_void, path: u64) -> bool {
    type F = unsafe extern "C" fn(*mut c_void, u64) -> bool;
    // SAFETY: ResourceDepot's vtable (GetAllocator +0x00, the destructor +0x08, RequestResource +0x10).
    unsafe { red4ext_abi::rtti::vfunc::<F>(depot, 0x18)(depot, path) }
}
/// # Safety
/// A live token.
unsafe fn token_instance(token: *const Token) -> *const c_void {
    // SAFETY: as documented.
    unsafe { (*token)[0] as *const c_void }
}
/// `IsLoaded()`: finished and no error.
///
/// # Safety
/// A live token.
pub(crate) unsafe fn rt_token_loaded(token: *const Token) -> bool {
    // SAFETY: as documented (finished is volatile).
    unsafe {
        let t = token_instance(token);
        core::ptr::read_volatile(t.cast::<u8>().add(0x58).cast::<i32>()) != 0 && field::<u8>(t, 0x5C) == 0
    }
}
/// `IsFailed()`: the error flag.
///
/// # Safety
/// A live token.
pub(crate) unsafe fn rt_token_failed(token: *const Token) -> bool {
    // SAFETY: as documented.
    unsafe { field::<u8>(token_instance(token), 0x5C) != 0 }
}
/// The loaded resource's instance.
///
/// # Safety
/// A live token.
pub(crate) unsafe fn rt_token_resource(token: *const Token) -> *mut c_void {
    // SAFETY: as documented.
    unsafe { field::<*mut c_void>(token_instance(token), 0x28) }
}
/// `job->Join(token->job)`.
///
/// # Safety
/// A live job handle and token.
unsafe fn rt_job_join(job: *mut c_void, token: *const Token) {
    type F = unsafe extern "C" fn(*mut c_void, *const c_void);
    // SAFETY: JobHandle_Join's signature per JobQueue-inl.hpp.
    if let Some(join) = unsafe { JOB_JOIN.func::<F>() } {
        // SAFETY: as documented.
        unsafe { join(job, token_instance(token).cast::<u8>().add(0x50).cast()) };
    }
}
/// `GetNativeType()->GetName()`.
///
/// # Safety
/// A live ISerializable.
unsafe fn rt_serializable_type(serializable: *mut c_void) -> u64 {
    type F = unsafe extern "C" fn(*mut c_void) -> *const red4ext_abi::rtti::IType;
    // SAFETY: ISerializable::GetNativeType (+0x00); a class is an IType.
    unsafe { (*red4ext_abi::rtti::vfunc::<F>(serializable, 0)(serializable)).name().0 }
}
/// # Safety
/// A live CResource.
unsafe fn rt_resource_path(resource: *mut c_void) -> u64 {
    // SAFETY: as documented.
    unsafe { field::<u64>(resource, 0x30) }
}
/// The component's entity's template path, or 0 without an entity.
///
/// # Safety
/// A live IComponent.
unsafe fn rt_component_template(component: *mut c_void) -> u64 {
    // SAFETY: as documented.
    unsafe {
        let owner = field::<*const c_void>(component, 0x50);
        if owner.is_null() { 0 } else { field::<u64>(owner, 0x60) }
    }
}
/// `name.ToString()`.
///
/// # Safety
/// A live IComponent.
unsafe fn rt_component_name(component: *mut c_void) -> *const c_char {
    // SAFETY: as documented.
    red4ext_abi::cname::CName(unsafe { field::<u64>(component, 0x40) }).raw_text()
}
/// # Safety
/// A live CMesh.
unsafe fn rt_mesh_material_count(mesh: *mut c_void) -> u32 {
    // SAFETY: as documented.
    unsafe { field::<u32>(mesh, 0xE0 + 0xC) }
}
/// The material entry's name, local flag and index.
///
/// # Safety
/// A live CMesh; `index` within its entries.
unsafe fn rt_mesh_material(mesh: *mut c_void, index: u32, name: *mut u64, local: *mut bool) -> u32 {
    // SAFETY: as documented.
    unsafe {
        let entry = field::<*const u8>(mesh, 0xE0).add(index as usize * 0x30).cast::<c_void>();
        *name = field::<u64>(entry, 0x20);
        *local = field::<u8>(entry, 0x2A) != 0;
        u32::from(field::<u16>(entry, 0x28))
    }
}
/// # Safety
/// A live CMesh.
unsafe fn rt_mesh_external_count(mesh: *mut c_void) -> u32 {
    // SAFETY: as documented.
    unsafe { field::<u32>(mesh, 0xF0 + 0xC) }
}
/// # Safety
/// A live CMesh.
unsafe fn rt_mesh_appearance_count(mesh: *mut c_void) -> u32 {
    // SAFETY: as documented.
    unsafe { field::<u32>(mesh, 0x1E0 + 0xC) }
}
/// The appearance's name, or u64::MAX for an empty handle.
///
/// # Safety
/// A live CMesh; `index` within its appearances.
unsafe fn rt_mesh_appearance_name(mesh: *mut c_void, index: u32) -> u64 {
    // SAFETY: as documented (a handle is the instance, then its counts).
    unsafe {
        let appearance = field::<*const c_void>(field::<*const u8>(mesh, 0x1E0).add(index as usize * 0x10).cast(), 0);
        if appearance.is_null() { u64::MAX } else { field::<u64>(appearance, 0x30) }
    }
}
/// `forceLoadAllAppearances = false`.
///
/// # Safety
/// A live CMesh.
unsafe fn rt_mesh_settle(mesh: *mut c_void) {
    // SAFETY: as documented.
    unsafe { *mesh.cast::<u8>().add(0x6D) = 0 };
}

// ---- the writes (formerly ArchiveExtensions.cpp's adapters): engine arrays grown through DynArray_Realloc as the SDK's
// PushBack / Emplace do, their elements moved bitwise (none of them points into itself); ArchiveGroup (0x38: the
// archives +0x00, basePath +0x10, scope +0x30, Mod = 6), AnimSetupEntry (0x20: the set's path, variableNames +0x08,
// priority +0x18; AnimatedComponent::animations.gameplay +0x1A0), CMeshMaterialEntry (0x30), MeshAppearance (name +0x30,
// chunkMaterials +0x38, tags +0x58), StreamingWorld::blockRefs (+0x70; a reference is its path and a SharedPtr).
use red4ext_abi::dynarray::DynArray;
static LOADER: reloc::Reloc = reloc::Reloc::new(red4ext_abi::hashes::ResourceLoader);
static LOAD_BY_PATH: reloc::Reloc = reloc::Reloc::new(red4ext_abi::hashes::ResourceLoader_IssueLoadingRequestByPath);
/// `PushBack` of an element given as its bytes onto the engine array at `array`.
///
/// # Safety
/// `array` is a DynArray of `N`-byte elements aligned to 8 that may be moved bitwise.
unsafe fn push_bytes<const N: usize>(array: *mut c_void, element: [u8; N]) -> bool {
    // SAFETY: as documented.
    unsafe { (*(array as *mut DynArray<[u8; N]>)).push(element) }
}
/// A DynArray's buffer and size.
///
/// # Safety
/// `array` is a DynArray.
unsafe fn array_parts(array: *const c_void) -> (*mut u8, u32) {
    // SAFETY: as documented.
    unsafe { (field::<*mut u8>(array, 0), field::<u32>(array, 0xC)) }
}
/// A DynArray<CName>'s names pushed onto another (`operator=` onto an empty array).
///
/// # Safety
/// Both are DynArray<CName>s.
unsafe fn copy_names(target: *mut c_void, source: *const c_void) {
    // SAFETY: as documented.
    unsafe {
        let (names, count) = array_parts(source);
        for i in 0..count as usize {
            (*(target as *mut DynArray<u64>)).push(*(names as *const u64).add(i));
        }
    }
}
/// The depot's group for `base` (a new Mod group before the first that is not one, when there is none), its archives
/// mounted by LoadArchives.
///
/// # Safety
/// The engine's depot and its LoadArchives; NUL-terminated texts.
unsafe fn rt_depot_mount(depot: *mut c_void, load_archives: usize, base: *const c_char, paths: *const *const c_char, count: usize) -> bool {
    type LoadArchives = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *mut c_void, i32, u8) -> bool;
    const GROUP: usize = 0x38;
    const MOD: u32 = 6;
    // SAFETY: as documented.
    unsafe {
        let mut archives = DynArray::<[u8; 0x20]>::default();
        for i in 0..count {
            if let Some(text) = EngineString::new(CStr::from_ptr(*paths.add(i))) {
                archives.push(core::mem::transmute::<EngineString, [u8; 0x20]>(text));
            }
        }
        let groups = depot.cast::<u8>().add(0x10).cast::<c_void>();
        let wanted = CStr::from_ptr(base);
        let (entries, size) = array_parts(groups);
        let found = (0..size as usize).find(|&i| {
            let text = (*(entries.add(i * GROUP + 0x10) as *const EngineString)).c_str();
            !text.is_null() && CStr::from_ptr(text) == wanted
        });
        let index = match found {
            Some(i) => i,
            None => {
                let first_other = (0..size as usize).find(|&i| field::<u32>(entries.add(i * GROUP).cast(), 0x30) != MOD).unwrap_or(size as usize);
                push_bytes::<GROUP>(groups, [0; GROUP]);
                let (entries, size) = array_parts(groups);
                let at = entries.add(first_other * GROUP);
                core::ptr::copy(at, at.add(GROUP), (size as usize - 1 - first_other) * GROUP);
                core::ptr::write_bytes(at, 0, GROUP);
                if let Some(text) = EngineString::new(wanted) {
                    core::ptr::write(at.add(0x10) as *mut EngineString, text);
                }
                *(at.add(0x30) as *mut u32) = MOD;
                first_other
            }
        };
        let group = array_parts(groups).0.add(index * GROUP).cast::<c_void>();
        let mut loaded = DynArray::<u64>::default();
        let result = core::mem::transmute::<usize, LoadArchives>(load_archives)(
            core::ptr::null_mut(),
            group,
            (&archives as *const DynArray<[u8; 0x20]>).cast(),
            (&mut loaded as *mut DynArray<u64>).cast(),
            0,
            0,
        );
        let (names, named) = array_parts((&archives as *const DynArray<[u8; 0x20]>).cast());
        for i in 0..named as usize {
            drop(core::ptr::read(names.add(i * 0x20) as *const EngineString));
        }
        archives.free();
        loaded.free();
        result
    }
}
/// `ResourceLoader::Get()->LoadAsync<CResource>(path)` into `out`.
///
/// # Safety
/// `out` is writable.
pub(crate) unsafe fn rt_resource_load_async(path: u64, out: *mut Token) {
    type F = unsafe extern "C" fn(*mut c_void, *mut Token, u64) -> usize;
    // SAFETY: as documented; the loader's global and IssueLoadingRequestByPath's signature per ResourceLoader.hpp.
    unsafe {
        out.write([0, 0]);
        let Some(at) = LOADER.get() else { return };
        if let Some(load) = LOAD_BY_PATH.func::<F>() {
            load(*(at as *const *mut c_void), out, path);
        }
    }
}
/// A gameplay animation setup (the set, its priority and variable names) appended to the component's.
///
/// # Safety
/// A live entAnimatedComponent; `vars` holds `count` names.
unsafe fn rt_anim_push_setup(component: *mut c_void, set: u64, priority: u8, vars: *const u64, count: usize) {
    // SAFETY: as documented.
    unsafe {
        let mut names = DynArray::<u64>::default();
        for i in 0..count {
            names.push(*vars.add(i));
        }
        let mut entry = [0u8; 0x20];
        core::ptr::write_unaligned(entry.as_mut_ptr() as *mut u64, set);
        core::ptr::write_unaligned(entry.as_mut_ptr().add(8) as *mut DynArray<u64>, names);
        entry[0x18] = priority;
        push_bytes::<0x20>(component.cast::<u8>().add(0x1A0).cast(), entry);
    }
}
/// The patch's external material `external` appended to the target's, with a material entry naming it.
///
/// # Safety
/// Live CMeshes; `external` within the patch's.
unsafe fn rt_mesh_add_material(target: *mut c_void, patch: *mut c_void, name: u64, external: u32) {
    // SAFETY: as documented.
    unsafe {
        let index = field::<u32>(target, 0xF0 + 0xC) as u16;
        let material = *(array_parts(patch.cast::<u8>().add(0xF0).cast()).0 as *const u64).add(external as usize);
        push_bytes::<8>(target.cast::<u8>().add(0xF0).cast(), material.to_le_bytes());
        let mut entry = [0u8; 0x30];
        core::ptr::write_unaligned(entry.as_mut_ptr().add(0x20) as *mut u64, name);
        core::ptr::write_unaligned(entry.as_mut_ptr().add(0x28) as *mut u16, index);
        push_bytes::<0x30>(target.cast::<u8>().add(0xE0).cast(), entry);
    }
}
/// A new meshMeshAppearance with the patch's appearance `index`'s name, chunk materials and tags, replacing the
/// target's appearance `replace` (or appended when it is negative).
///
/// # Safety
/// Live CMeshes; `index` within the patch's appearances, `replace` within the target's.
unsafe fn rt_mesh_put_appearance(target: *mut c_void, patch: *mut c_void, index: u32, replace: i32) {
    static CLASS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let class = *CLASS.get_or_init(|| {
        red4ext_abi::rtti::RttiSystem::get()
            .and_then(|rtti| rtti.get_class(red4ext_abi::cname::CName::hashed("meshMeshAppearance")))
            .map_or(0, |class| class as *const red4ext_abi::rtti::Class as usize)
    });
    // SAFETY: as documented; the class is the engine's.
    unsafe {
        let appearance = field::<*const c_void>(array_parts(patch.cast::<u8>().add(0x1E0).cast()).0.add(index as usize * 0x10).cast(), 0);
        let clone = (*(class as *const red4ext_abi::rtti::Class)).create_instance();
        *(clone.cast::<u8>().add(0x30) as *mut u64) = field::<u64>(appearance, 0x30);
        copy_names(clone.cast::<u8>().add(0x38).cast(), appearance.cast::<u8>().add(0x38).cast());
        copy_names(clone.cast::<u8>().add(0x58).cast(), appearance.cast::<u8>().add(0x58).cast());
        let handle = red4ext_abi::handle::Handle::from_instance(clone);
        let appearances = target.cast::<u8>().add(0x1E0).cast::<c_void>();
        if replace >= 0 {
            let slot = array_parts(appearances).0.add(replace as usize * 0x10) as *mut red4ext_abi::handle::Handle;
            drop(core::ptr::replace(slot, handle));
        } else {
            push_bytes::<0x10>(appearances, core::mem::transmute::<red4ext_abi::handle::Handle, [u8; 0x10]>(handle));
        }
    }
}
/// A block reference (the path and the token, its strong count raised) appended to the streaming world's.
///
/// # Safety
/// A live worldStreamingWorld and token.
unsafe fn rt_world_add_block(world: *mut c_void, path: u64, token: *const Token) {
    // SAFETY: as documented; RefCnt's strong count is its first word.
    unsafe {
        let [instance, counts] = *token;
        if counts != 0 {
            (*(counts as *const core::sync::atomic::AtomicU32)).fetch_add(1, Ordering::SeqCst);
        }
        let mut entry = [0u8; 0x18];
        core::ptr::write_unaligned(entry.as_mut_ptr() as *mut u64, path);
        core::ptr::write_unaligned(entry.as_mut_ptr().add(8) as *mut [usize; 2], [instance, counts]);
        push_bytes::<0x18>(world.cast::<u8>().add(0x70).cast(), entry);
    }
}

// Address-library hashes (red4ext's cyberpunk2077_addresses.json), the ones ArchiveXL resolves for the same purposes.
const INITIALIZE_ARCHIVES_HASH: u32 = 2892763485; // ResourceDepot: mounts its archive groups
const LOAD_ARCHIVES_HASH: u32 = 3573367139; // ResourceDepot: mounts archives into a group (2.5 form)
const LOAD_FACTORY_ASYNC_HASH: u32 = 1931943018; // FactoryIndex: loads one factory CSV
const INITIALIZE_ANIMATIONS_HASH: u32 = 2855474741; // entAnimatedComponent: builds its animation setup
const MESH_POST_LOAD_HASH: u32 = 2272530537; // CMesh::PostLoad
const DEPENDENCIES_READY_HASH: u32 = 3892979844; // ResourceSerializer: a resource's dependencies are in
const SERIALIZER_LOAD_HASH: u32 = 2577814646; // ResourceSerializer: a resource is requested
const DESERIALIZE_HASH: u32 = 2901686778; // ResourceSerializer: a resource's deserialization job
const UPDATE_WORLD_LAYER_HASH: u32 = 2254060789; // inkWorldLayer: updates its world widget components
const SPAWN_FROM_TEMPLATE_HASH: u32 = 2509382878; // EntitySpawner: spawns an entity from a template
const MESH_APPEARANCE_DTOR_HASH: u32 = 2901936808; // meshMeshAppearance: destructor (not the deleting one)
const RELEASE_WEAK_HASH: u32 = 1073024731; // HandleSharedStorage::ReleaseWeak

/// The game's own last factory: the port's are loaded right after it, into the same index.
fn last_game_factory() -> u64 {
    resource_path(r"base\gameplay\factories\vehicles\vehicles.csv")
}

type InitializeArchivesFn = unsafe extern "C" fn(depot: *mut c_void);
type LoadFactoryAsyncFn = unsafe extern "C" fn(index: usize, path: u64, context: usize);
type InitializeAnimationsFn = unsafe extern "C" fn(component: *mut c_void);
type MeshPostLoadFn = unsafe extern "C" fn(mesh: *mut c_void, params: *mut c_void);
type DependenciesReadyFn = unsafe extern "C" fn(context: *mut u8);
// The request carries the resource's path at +0x28 (2.5, as ArchiveXL has it).
type SerializerLoadFn =
    unsafe extern "C" fn(serializer: *mut c_void, a2: u64, request: *mut u8, a4: u64, a5: u64);
type DeserializeFn = unsafe extern "C" fn(
    serializer: *mut c_void,
    a2: u64,
    a3: u64,
    job: *mut c_void,
    request: *mut u8,
    a6: u64,
    results: *mut c_void,
    a8: u64,
);
type SpawnFromTemplateFn = unsafe extern "C" fn(
    spawner: *mut c_void,
    out: *mut c_void,
    request: *mut u8,
    template: u64,
) -> *mut c_void;
pub type Fixup = unsafe extern "C" fn(resource: *mut c_void);

// The originals, written by RED4ext's attach before the detour can run.
static LOAD_ARCHIVES: AtomicUsize = AtomicUsize::new(0);
static INITIALIZE_ARCHIVES: AtomicUsize = AtomicUsize::new(0);
static LOAD_FACTORY_ASYNC: AtomicUsize = AtomicUsize::new(0);
static INITIALIZE_ANIMATIONS: AtomicUsize = AtomicUsize::new(0);
static MESH_POST_LOAD: AtomicUsize = AtomicUsize::new(0);
static DEPENDENCIES_READY: AtomicUsize = AtomicUsize::new(0);
static SERIALIZER_LOAD: AtomicUsize = AtomicUsize::new(0);
static DESERIALIZE: AtomicUsize = AtomicUsize::new(0);
static SPAWN_FROM_TEMPLATE: AtomicUsize = AtomicUsize::new(0);
static MESH_APPEARANCE_DTOR: AtomicUsize = AtomicUsize::new(0);
/// The depot's read of a resource's data and its existence check (main's private mesh data / lookup hooks): a blade
/// mesh copy's private path reads its source's.
const DEPOT_REQUEST_HASH: u32 = 2450934495;
const DEPOT_CHECK_HASH: u32 = 43194193;
static DEPOT_REQUEST: AtomicUsize = AtomicUsize::new(0);
static DEPOT_CHECK: AtomicUsize = AtomicUsize::new(0);
type DepotRequestFn = unsafe extern "C" fn(*mut c_void, *const usize, u64, *const i32) -> *mut usize;
type DepotCheckFn = unsafe extern "C" fn(*mut c_void, u64) -> bool;
unsafe extern "C" fn detour_depot_request(depot: *mut c_void, out: *const usize, path: u64, archive: *const i32) -> *mut usize {
    // SAFETY: the original with the remapped path.
    unsafe { original::<DepotRequestFn>(&DEPOT_REQUEST)(depot, out, crate::blade_mesh::depot_source(path), archive) }
}
unsafe extern "C" fn detour_depot_check(depot: *mut c_void, path: u64) -> bool {
    // SAFETY: as above.
    unsafe { original::<DepotCheckFn>(&DEPOT_CHECK)(depot, crate::blade_mesh::depot_source(path)) }
}
static RELEASE_WEAK: AtomicUsize = AtomicUsize::new(0);
static DANGLING_APPEARANCE_HANDLES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// # Safety
/// `slot` holds an original of type `F` (a function pointer), written by the attach.
unsafe fn original<F: Copy>(slot: &AtomicUsize) -> F {
    let address = slot.load(Ordering::Acquire);
    // SAFETY: the caller's: the slot holds a function of this type.
    unsafe { core::mem::transmute_copy::<usize, F>(&address) }
}

// ---- the manifest, and what it becomes once the archives are mounted

static MANIFEST: Mutex<Option<Manifest>> = Mutex::new(None);

struct Setup {
    set: u64,
    priority: u8,
    vars: Vec<u64>,
}

/// A patch resource, loaded when one of its targets is first requested and held for the session: a few small
/// resources, needed again by every load of their targets.
struct Patch {
    source: u64,
    name: String,
    /// A mesh patch: its appearances are added to the target.
    appearances: bool,
    token: Mutex<Token>,
}

struct Prepared {
    factories: Vec<u64>,
    /// By the entity template path, or by FNV1a64(component name, template path) for one component.
    animations: FastMap<u64, Vec<Setup>>,
    patches: Vec<Patch>,
    by_target: FastMap<u64, Vec<usize>>,
}
static PREPARED: OnceLock<Prepared> = OnceLock::new();

/// The features' fixes (cvr_archive_add_fixup), by type name.
static FIXUPS: RwLock<Vec<(u64, Fixup)>> = RwLock::new(Vec::new());

fn manifest_path() -> PathBuf {
    // Not beside the archive: ArchiveXL applies every *.xl under a mod group's folder, the plugin's archive folder too.
    paths::file(Folder::Config, "archive.yaml")
}

/// RED4ext's FNV1a64(text, seed): the text continuing a hash.
fn fnv1a64_from(text: &[u8], seed: u64) -> u64 {
    text.iter().fold(seed, |h, &b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

fn read_manifest() -> bool {
    let Ok(bytes) = std::fs::read(manifest_path()) else {
        return false;
    };
    let (manifest, error) = archive_manifest::parse(&String::from_utf8_lossy(&bytes));
    *MANIFEST.lock().unwrap_or_else(|e| e.into_inner()) = Some(manifest);
    if let Some(error) = error {
        log!("[archive] {}: {}\n", manifest_path().display(), error);
        return false;
    }
    true
}

fn exists(depot: *mut c_void, path: u64) -> bool {
    // SAFETY: ResourceDepot::ResourceExists on the depot the engine handed the detour.
    unsafe { rt_depot_exists(depot, path) }
}

fn prepare(depot: *mut c_void) {
    let manifest = MANIFEST
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
        .unwrap_or_default();
    let mut missing = 0;
    let mut factories = Vec::new();
    for factory in &manifest.factories {
        let path = resource_path(factory);
        if exists(depot, path) {
            factories.push(path);
        } else {
            log!("[archive] factory {factory} is in no archive\n");
            missing += 1;
        }
    }
    let mut setups = 0;
    let mut animations: FastMap<u64, Vec<Setup>> = cvr_host::fast::fast_map();
    for entry in &manifest.animations {
        let set = resource_path(&entry.set);
        if !exists(depot, set) {
            log!("[archive] animation set {} is in no archive\n", entry.set);
            missing += 1;
            continue;
        }
        for entity in &entry.entities {
            let target = resource_path(entity);
            if !exists(depot, target) {
                continue;
            }
            let vars = entry
                .vars
                .iter()
                .map(|v| red4ext_abi::cname::CName::add(&cstr(v)).0)
                .collect();
            let key = if entry.component.is_empty() {
                target
            } else {
                fnv1a64_from(entry.component.as_bytes(), target)
            };
            animations.entry(key).or_default().push(Setup {
                set,
                priority: entry.priority as u8,
                vars,
            });
            setups += 1;
        }
    }
    let mut patches = Vec::new();
    let mut by_target: FastMap<u64, Vec<usize>> = cvr_host::fast::fast_map();
    for entry in &manifest.patches {
        let source = resource_path(&entry.source);
        if !exists(depot, source) {
            log!("[archive] patch {} is in no archive\n", entry.source);
            missing += 1;
            continue;
        }
        for target in &entry.targets {
            by_target
                .entry(resource_path(target))
                .or_default()
                .push(patches.len());
        }
        patches.push(Patch {
            source,
            name: entry.source.clone(),
            appearances: entry.props.iter().any(|p| p == "appearances"),
            token: Mutex::new([0; 2]),
        });
    }
    log!(
        "[archive] manifest: {} factories, {} animation setups on {} targets, {} patches, {} missing\n",
        factories.len(),
        setups,
        animations.len(),
        patches.len(),
        missing
    );
    let _ = PREPARED.set(Prepared {
        factories,
        animations,
        patches,
        by_target,
    });
}

fn patches_of(path: u64) -> impl Iterator<Item = &'static Patch> {
    let prepared = PREPARED.get();
    prepared
        .and_then(|p| p.by_target.get(&path).map(|list| (p, list)))
        .into_iter()
        .flat_map(|(p, list)| list.iter().map(move |&i| &p.patches[i]))
}
/// The patches of a serializer request's resource.
///
/// # Safety
/// `request` is null or the serializer's request (its path at +0x28).
unsafe fn patches_of_request(request: *const u8) -> impl Iterator<Item = &'static Patch> {
    let path = if request.is_null() || PREPARED.get().is_none_or(|p| p.by_target.is_empty()) {
        0
    } else {
        // SAFETY: the caller's.
        unsafe { (request.add(0x28) as *const u64).read_unaligned() }
    };
    patches_of(path).filter(move |_| path != 0)
}

fn start_loading(patch: &Patch) -> Token {
    let mut token = patch.token.lock().unwrap_or_else(|e| e.into_inner());
    if token[0] == 0 {
        // SAFETY: ResourceLoader::LoadAsync<CResource>, its SharedPtr written into the token.
        unsafe { rt_resource_load_async(patch.source, &mut *token) };
    }
    *token
}

/// A patch resource ready to merge: its load is part of its target's deserialization job (detour_deserialize), so
/// the target is never done before it.
fn ready(patch: &Patch) -> Option<*mut c_void> {
    let token = start_loading(patch);
    // SAFETY: a live token of the resource loader's (held for the session).
    unsafe {
        if !rt_token_loaded(&token) {
            let state = if rt_token_failed(&token) {
                "missing"
            } else {
                "STILL LOADING"
            };
            log!("[archive] patch {} is {}\n", patch.name, state);
            return None;
        }
        Some(rt_token_resource(&token))
    }
}

fn type_is(serializable: *mut c_void, name: &str) -> bool {
    // SAFETY: GetNativeType()->GetName() of a live serializable.
    !serializable.is_null()
        && unsafe { rt_serializable_type(serializable) } == fnv1a64(name.as_bytes())
}

// ---- mounting

/// The port's one archive (main's f9d8ad19e: the blade marks, the scope housings and the lobby's control icons are in
/// it; the HUD hints' glyph atlas since 0.2.6) in a mod group of its own, placed with the game's mod groups (before the
/// first other one).
fn mount(depot: *mut c_void) {
    let dir = paths::dir(Folder::Archive);
    // The folder as the C++ half names it (paths::Dir): with its trailing separator -- the group's base path.
    let shown = format!("{}\\", dir.display());
    let archive = dir.join("cyberpunkvrport.archive");
    if !archive.is_file() {
        log!("[archive] missing {}\n", archive.display());
        return;
    }
    let archives: Vec<std::ffi::CString> = vec![cstr(&archive.to_string_lossy())];
    let pointers: Vec<*const c_char> = archives.iter().map(|a| a.as_ptr()).collect();
    let base = cstr(&shown);
    // SAFETY: the adapter finds or makes the group and calls the engine's mounting with these paths.
    let failed = unsafe {
        rt_depot_mount(
            depot,
            LOAD_ARCHIVES.load(Ordering::Acquire),
            base.as_ptr(),
            pointers.as_ptr(),
            pointers.len(),
        )
    };
    log!(
        "[archive] mounted {} archive(s) from {}{}\n",
        archives.len(),
        shown,
        if failed { " -- ONE FAILED" } else { "" }
    );
}

unsafe extern "C" fn detour_initialize_archives(depot: *mut c_void) {
    // SAFETY: the original, attached before this could run.
    unsafe { original::<InitializeArchivesFn>(&INITIALIZE_ARCHIVES)(depot) };
    let started = Instant::now();
    mount(depot);
    prepare(depot);
    log!(
        "[archive] mount and manifest: {:.1} ms\n",
        started.elapsed().as_secs_f64() * 1000.0
    );
}

// ---- factories, animations, patches

/// A patched resource is requested: its patches start loading alongside it.
unsafe extern "C" fn detour_serializer_load(
    serializer: *mut c_void,
    a2: u64,
    request: *mut u8,
    a4: u64,
    a5: u64,
) {
    // SAFETY: the serializer's request.
    for patch in unsafe { patches_of_request(request) } {
        start_loading(patch);
    }
    // SAFETY: the original.
    unsafe { original::<SerializerLoadFn>(&SERIALIZER_LOAD)(serializer, a2, request, a4, a5) };
}

/// ... and its deserialization waits for them: their load jobs are joined to its job.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn detour_deserialize(
    serializer: *mut c_void,
    a2: u64,
    a3: u64,
    job: *mut c_void,
    request: *mut u8,
    a6: u64,
    results: *mut c_void,
    a8: u64,
) {
    // SAFETY: the serializer's request; the job handle it is given.
    for patch in unsafe { patches_of_request(request) } {
        let token = start_loading(patch);
        unsafe { rt_job_join(job, &token) };
    }
    // SAFETY: the original.
    unsafe {
        original::<DeserializeFn>(&DESERIALIZE)(serializer, a2, a3, job, request, a6, results, a8)
    };
}

unsafe extern "C" fn detour_load_factory_async(index: usize, path: u64, context: usize) {
    // SAFETY: the original.
    let load = unsafe { original::<LoadFactoryAsyncFn>(&LOAD_FACTORY_ASYNC) };
    unsafe { load(index, path, context) };
    if path != last_game_factory() {
        return;
    }
    let factories = PREPARED.get().map_or(&[][..], |p| &p.factories[..]);
    for &factory in factories {
        // SAFETY: the same index and context the game loads its own into.
        unsafe { load(index, factory, context) };
    }
    log!(
        "[archive] {} item factories loaded after the game's\n",
        factories.len()
    );
}

unsafe extern "C" fn detour_initialize_animations(component: *mut c_void) {
    if let Some(prepared) = PREPARED.get().filter(|p| !p.animations.is_empty()) {
        // SAFETY: the component's owner's template path, and its name.
        let template = unsafe { rt_component_template(component) };
        if template != 0 {
            let setups = prepared.animations.get(&template).or_else(|| {
                // SAFETY: the component's own name, NUL-terminated (null: a name the pool does not have).
                let name = unsafe { rt_component_name(component) };
                let name = if name.is_null() {
                    &[][..]
                } else {
                    unsafe { CStr::from_ptr(name) }.to_bytes()
                };
                prepared.animations.get(&fnv1a64_from(name, template))
            });
            for setup in setups.into_iter().flatten() {
                // SAFETY: an AnimSetupEntry pushed onto the component's gameplay animations.
                unsafe {
                    rt_anim_push_setup(
                        component,
                        setup.set,
                        setup.priority,
                        setup.vars.as_ptr(),
                        setup.vars.len(),
                    )
                };
            }
        }
    }
    // SAFETY: the original.
    unsafe { original::<InitializeAnimationsFn>(&INITIALIZE_ANIMATIONS)(component) };
}

/// The patch mesh's appearances added to the target, with the external materials they use. A same-named appearance
/// of the target is replaced. Local material instances are not carried over (none of the port's patches has one).
///
/// # Safety
/// Both are live CMesh resources.
unsafe fn merge_appearances(target: *mut c_void, patch: *mut c_void, source: &Patch) {
    // SAFETY: the meshes' own arrays, through the adapter.
    unsafe {
        let materials = |mesh| -> Vec<(u64, bool, u32)> {
            (0..rt_mesh_material_count(mesh))
                .map(|i| {
                    let (mut name, mut local) = (0u64, false);
                    let index = rt_mesh_material(mesh, i, &mut name, &mut local);
                    (name, local, index)
                })
                .collect()
        };
        let known = materials(target);
        for (name, local, index) in materials(patch) {
            if known.iter().any(|&(n, _, _)| n == name) {
                continue;
            }
            if local || index >= rt_mesh_external_count(patch) {
                log!(
                    "[archive] {}: material {} is local to the patch -- not carried over\n",
                    source.name,
                    red4ext_abi::cname::CName(name).text()
                );
                continue;
            }
            rt_mesh_add_material(target, patch, name, index);
        }
        for i in 0..rt_mesh_appearance_count(patch) {
            let name = rt_mesh_appearance_name(patch, i);
            if name == u64::MAX {
                continue; // a null handle
            }
            let same = (0..rt_mesh_appearance_count(target))
                .find(|&j| rt_mesh_appearance_name(target, j) == name)
                .map_or(-1, |j| j as i32);
            rt_mesh_put_appearance(target, patch, i, same);
        }
        rt_mesh_settle(target);
    }
}

unsafe extern "C" fn detour_mesh_post_load(mesh: *mut c_void, params: *mut c_void) {
    // a blade-marked private copy is cut before its upload (main's BeforePrivateMeshUpload)
    crate::blade_mesh::before_private_upload(mesh);
    if !mesh.is_null() && PREPARED.get().is_some_and(|p| !p.by_target.is_empty()) {
        // SAFETY: the mesh's path.
        for patch in patches_of(unsafe { rt_resource_path(mesh) }) {
            if !patch.appearances {
                continue;
            }
            if let Some(resource) = ready(patch).filter(|&r| type_is(r, "CMesh")) {
                // SAFETY: both meshes, the target still in its post-load.
                unsafe { merge_appearances(mesh, resource, patch) };
                log!(
                    "[archive] appearances of {} added to a mesh it patches ({} appearances now)\n",
                    patch.name,
                    unsafe { rt_mesh_appearance_count(mesh) }
                );
            }
        }
    }
    // SAFETY: the original.
    unsafe { original::<MeshPostLoadFn>(&MESH_POST_LOAD)(mesh, params) };
}

// ---- A MESH DYING WITH AN APPEARANCE WHOSE MATERIAL NEVER ARRIVED (main's 132ce0149). An appearance keeps the handles
// of the materials it loaded in a runtime array at +0x78 (2.5; not in the reflection data); one whose load did not finish
// is left there with a shared storage and no instance. The destructor releases every entry the usual way -- the last
// reference asks the instance whether it may be destroyed -- and reads 0x0 (exe+0xB0C787 from +0x1957CC). Seen with
// the belt meshes' lit appearances (external vrp_mark_*.mi) while a save loads. Such an entry gives its reference back
// here, through HandleSharedStorage::ReleaseWeak, with nothing to destroy.
#[repr(C)]
struct RawHandleEntry {
    instance: *mut c_void,
    storage: *mut i32,
}
#[repr(C)]
struct RawHandleArray {
    entries: *mut RawHandleEntry,
    capacity: u32,
    size: u32,
}
const APPEARANCE_HANDLES: usize = 0x78;
type MeshAppearanceDtorFn = unsafe extern "C" fn(appearance: *mut u8);
type ReleaseWeakFn = unsafe extern "C" fn(handle: *mut RawHandleEntry);
unsafe extern "C" fn detour_mesh_appearance_dtor(appearance: *mut u8) {
    let release = RELEASE_WEAK.load(Ordering::Acquire);
    if !appearance.is_null() && release != 0 {
        // SAFETY: the appearance being destroyed; its handle array as 2.5 lays it out (the layout checked at install).
        unsafe {
            let handles = &*(appearance.add(APPEARANCE_HANDLES) as *const RawHandleArray);
            if !handles.entries.is_null() && handles.size <= handles.capacity && handles.capacity < 0x10000 {
                let release: ReleaseWeakFn = core::mem::transmute(release);
                for i in 0..handles.size as usize {
                    let h = &mut *handles.entries.add(i);
                    if !h.instance.is_null() || h.storage.is_null() {
                        continue;
                    }
                    // the strong count first in the shared storage
                    if core::sync::atomic::AtomicI32::from_ptr(h.storage).fetch_sub(1, Ordering::AcqRel) == 1 {
                        release(h);
                    }
                    h.storage = core::ptr::null_mut();
                    let n = DANGLING_APPEARANCE_HANDLES.fetch_add(1, Ordering::Relaxed);
                    if n < 8 {
                        log!("[archive] a mesh appearance released a material handle that never loaded ({})\n", n + 1);
                    }
                }
            }
        }
    }
    // SAFETY: the original.
    unsafe { original::<MeshAppearanceDtorFn>(&MESH_APPEARANCE_DTOR)(appearance) };
}
/// The guard relies on the destructor releasing exactly that array: on 2.5 it does, `lea rcx, [rdi+78h]` and a call
/// 0x23 bytes in. Any other build leaves the destructor unhooked.
fn appearance_dtor_layout_known(code: usize) -> bool {
    const RELEASE_ARRAY: [u8; 5] = [0x48, 0x8D, 0x4F, 0x78, 0xE8];
    code != 0 && cvr_engine::code::read::<[u8; 5]>(code + 0x23) == Some(RELEASE_ARRAY)
}

/// The serializer context's serializables (DynArray<Handle<ISerializable>> at +0x230, 2.5 as ArchiveXL has it).
///
/// # Safety
/// `context` is the serializer's context.
unsafe fn serializables(context: *mut u8) -> Vec<*mut c_void> {
    // SAFETY: the caller's; a DynArray is {entries, capacity u32, size u32}, a Handle {instance, refcount}.
    unsafe {
        let entries = (context.add(0x230) as *const *const [*mut c_void; 2]).read();
        let size = (context.add(0x23C) as *const u32).read();
        (0..size as usize)
            .map(|i| (*entries.add(i))[0])
            .filter(|p| !p.is_null())
            .collect()
    }
}

/// The features' fixes, each on the resources of its type; and a streaming block patched into a streaming world: one
/// more block reference, already holding its loaded block.
unsafe extern "C" fn detour_dependencies_ready(context: *mut u8) {
    world_widget_limit_check();
    if !context.is_null() {
        let fixups = FIXUPS.read().unwrap_or_else(|e| e.into_inner());
        let patched = PREPARED.get().is_some_and(|p| !p.by_target.is_empty());
        if !fixups.is_empty() || patched {
            // SAFETY: the context the serializer hands its dependencies-ready step.
            let list = unsafe { serializables(context) };
            if !fixups.is_empty() {
                for &serializable in &list {
                    // SAFETY: a live serializable of this context.
                    let ty = unsafe { rt_serializable_type(serializable) };
                    for &(name, fix) in fixups.iter() {
                        if name == ty {
                            unsafe { fix(serializable) };
                        }
                    }
                }
            }
            if patched {
                for &world in list.iter().filter(|&&s| type_is(s, "worldStreamingWorld")) {
                    // SAFETY: a streaming world of this context.
                    for patch in patches_of(unsafe { rt_resource_path(world) }) {
                        if ready(patch).is_none() {
                            continue;
                        }
                        let token = *patch.token.lock().unwrap_or_else(|e| e.into_inner());
                        // SAFETY: one more block reference holding the loaded block's token.
                        unsafe { rt_world_add_block(world, patch.source, &token) };
                        log!(
                            "[archive] streaming block {} merged into its world\n",
                            patch.name
                        );
                    }
                }
            }
        }
    }
    // SAFETY: the original.
    unsafe { original::<DependenciesReadyFn>(&DEPENDENCIES_READY)(context) };
}

// ---- what ArchiveXL changes for every mod
// Two engine behaviours ArchiveXL changes whatever the manifests say; the port's assets were made with both in place.
// Either is the same change made twice when ArchiveXL is installed too.

/// Set while the world widget limit is left to ArchiveXL: installed, it lifts the limit itself as RED4ext loads it --
/// after the port -- and looks for the game's own code to do it (lifted by the port first, ArchiveXL warned that its
/// patch was not applied). Checked again at a resource load once ArchiveXL is in, or half a minute on.
static WIDGET_LIMIT_DEFERRED: AtomicBool = AtomicBool::new(false);
static WIDGET_LIMIT_SINCE: OnceLock<Instant> = OnceLock::new();

fn archivexl_installed() -> bool {
    let Some(root) = paths::game_root() else { return false };
    root.join("bin").join("x64").join("winmm.dll").is_file()
        && root.join("red4ext").join("plugins").join("ArchiveXL").join("ArchiveXL.dll").is_file()
}
fn archivexl_loaded() -> bool {
    let name: Vec<u16> = "ArchiveXL.dll\0".encode_utf16().collect();
    // SAFETY: a plain module query.
    !unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(name.as_ptr()) }.is_null()
}
/// At the plugin's start: the limit lifted now, or left to an installed ArchiveXL.
fn world_widget_limit() {
    if archivexl_installed() {
        WIDGET_LIMIT_SINCE.get_or_init(Instant::now);
        WIDGET_LIMIT_DEFERRED.store(true, Ordering::Release);
        log!("[archive] world widget limit: left to ArchiveXL (installed), checked once it is in\n");
    } else {
        lift_world_widget_limit();
    }
}
/// On a resource's dependencies: the limit left to ArchiveXL checked once ArchiveXL is in (or half a minute on).
fn world_widget_limit_check() {
    if !WIDGET_LIMIT_DEFERRED.load(Ordering::Relaxed) {
        return;
    }
    let late = WIDGET_LIMIT_SINCE.get().is_some_and(|t| t.elapsed().as_secs() >= 30);
    if (archivexl_loaded() || late) && WIDGET_LIMIT_DEFERRED.swap(false, Ordering::AcqRel) {
        lift_world_widget_limit();
    }
}

/// An ink world layer updates its first ten world widget components only (cmp ebx, 10; jae out). The port's menu, its
/// glass, beam and icons, the world markers and the weapons' own screens are more than ten with the game's. The
/// branch goes.
fn lift_world_widget_limit() {
    let Some(code) = reloc::resolve(UPDATE_WORLD_LAYER_HASH) else {
        log!("[archive] world widget limit: the layer update is not in the address library\n");
        return;
    };
    const LIMIT: [u8; 4] = [0x83, 0xFB, 0x0A, 0x73]; // cmp ebx, 10; jae rel8
    const LIFTED: [u8; 5] = [0x83, 0xFB, 0x0A, 0x90, 0x90];
    // SAFETY: the function's first 0x100 bytes, executable code of the game's image.
    let bytes = unsafe { core::slice::from_raw_parts(code as *const u8, 0x100) };
    if bytes.windows(LIFTED.len()).any(|w| w == LIFTED) {
        log!("[archive] world widget limit: already lifted (ArchiveXL)\n");
        return;
    }
    let Some(at) = bytes.windows(LIMIT.len()).position(|w| w == LIMIT) else {
        log!("[archive] world widget limit: the compare is not where 2.5 has it -- NOT lifted\n");
        return;
    };
    let branch = (code + at + 3) as *mut u8;
    // SAFETY: two bytes of the game's code made writable, written and restored.
    unsafe {
        use windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache;
        use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut protection = 0;
        VirtualProtect(branch.cast(), 2, PAGE_EXECUTE_READWRITE, &mut protection);
        branch.write(0x90);
        branch.add(1).write(0x90);
        VirtualProtect(branch.cast(), 2, protection, &mut protection);
        FlushInstructionCache(GetCurrentProcess(), branch.cast(), 2);
    }
    log!("[archive] world widget limit lifted\n");
}

/// An entity spawned from a record (DynamicEntitySpec.recordID: the lobby's figures) with no appearance, or "default",
/// takes the record's appearanceName -- not the template's first appearance.
unsafe extern "C" fn detour_spawn_from_template(
    spawner: *mut c_void,
    out: *mut c_void,
    request: *mut u8,
    template: u64,
) -> *mut c_void {
    // SAFETY: the request's appearance (+0xC0) and record (+0xE0), 2.5 as ArchiveXL has it.
    unsafe {
        let appearance = request.add(0xC0) as *mut u64;
        let record = (request.add(0xE0) as *const u64).read();
        if record != 0 && (*appearance == 0 || *appearance == fnv1a64(b"default")) {
            let name_type = rt_rtti_type(c"CName".as_ptr());
            let db = rt_tdb_get();
            if !db.is_null() {
                rt_tdb_flats_lock_shared(db);
                if let Some((ty, value)) = flat(db, tweakdbid::derive(record, ".appearanceName")) {
                    let name = *(value as *const u64);
                    if ty == name_type && name != 0 {
                        *appearance = name;
                    }
                }
                rt_tdb_flats_unlock_shared(db);
            }
        }
        original::<SpawnFromTemplateFn>(&SPAWN_FROM_TEMPLATE)(spawner, out, request, template)
    }
}

// ---- install

fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else {
        return false;
    };
    if fa.metadata().map(|m| m.len()).ok() != fb.metadata().map(|m| m.len()).ok() {
        return false;
    }
    let (mut ra, mut rb) = (std::io::BufReader::new(fa), std::io::BufReader::new(fb));
    let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    loop {
        let (Ok(n), Ok(m)) = (read_full(&mut ra, &mut x), read_full(&mut rb, &mut y)) else {
            return false;
        };
        if n != m || x[..n] != y[..m] {
            return false;
        }
        if n == 0 {
            return true;
        }
    }
}
fn read_full(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// An earlier release installed the archive and its manifest in archive\pc\mod, where the game would mount them over
/// the plugin's and ArchiveXL apply the manifest a second time: the copies that are the port's are taken out (same
/// bytes: deleted; different: renamed .legacy, which neither reads).
fn retire_legacy_copies() {
    let Some(legacy) = std::env::current_exe().ok().and_then(|e| {
        Some(
            e.parent()?
                .parent()?
                .parent()?
                .join("archive")
                .join("pc")
                .join("mod"),
        )
    }) else {
        return;
    };
    // The archives the one archive replaced (main's f9d8ad19e; the HUD hints' since 0.2.6), set aside where they are:
    // beside it or in archive\pc\mod.
    let archive_dir = paths::dir(Folder::Archive);
    if archive_dir.join("cyberpunkvrport.archive").is_file() {
        for dir in [archive_dir.to_path_buf(), legacy.clone()] {
            for name in ["cpvr_blade_mark.archive", "cpvr_scope_housings.archive", "cpvr_lobby_controls.archive", "cpvr_hud_hints.archive"] {
                let old = dir.join(name);
                if !old.is_file() {
                    continue;
                }
                let mut aside = old.clone().into_os_string();
                aside.push(".legacy");
                let failed = std::fs::rename(&old, PathBuf::from(aside)).is_err();
                log!("[archive] retired {}{}\n", old.display(), if failed { " (FAILED)" } else { "" });
            }
        }
    }
    let copies = [
        (
            "cyberpunkvrport.archive",
            paths::file(Folder::Archive, "cyberpunkvrport.archive"),
        ),
        ("cyberpunkvrport.archive.xl", manifest_path()),
    ];
    for (name, now) in copies {
        let old = legacy.join(name);
        if !old.exists() || !now.exists() {
            continue;
        }
        if same_bytes(&old, &now) {
            let failed = std::fs::remove_file(&old).is_err();
            log!(
                "[archive] removed the earlier release's copy {}{}\n",
                old.display(),
                if failed { " (FAILED)" } else { "" }
            );
        } else {
            let mut aside = old.clone().into_os_string();
            aside.push(".legacy");
            let aside = PathBuf::from(aside);
            let failed = std::fs::rename(&old, &aside).is_err();
            log!(
                "[archive] set the earlier release's copy aside: {}{}\n",
                aside.display(),
                if failed { " (FAILED)" } else { "" }
            );
        }
    }
}

/// AddFixup: a loaded resource of a native type set right as it loads (Core/ArchiveExtensions.hpp). Added at load,
/// before Install; the fix runs on the resource serializer's jobs.
#[no_mangle]
pub extern "C" fn cvr_archive_add_fixup(type_name: *const c_char, fix: Option<Fixup>) {
    let Some(fix) = fix.filter(|_| !type_name.is_null()) else {
        return;
    };
    // SAFETY: a NUL-terminated type name.
    let name = fnv1a64(unsafe { CStr::from_ptr(type_name) }.to_bytes());
    FIXUPS
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .push((name, fix));
}

/// Install: earlier releases' copies out of archive\pc\mod, the world widget limit lifted, the manifest read and the
/// engine paths hooked.
///
/// # Safety
/// `handle` and `sdk` are what RED4ext handed the plugin's Main.
#[no_mangle]
pub unsafe extern "C" fn cvr_archive_install(handle: PluginHandle, sdk: *const Sdk) {
    retire_legacy_copies();
    world_widget_limit();
    let started = Instant::now();
    if read_manifest() {
        log!(
            "[archive] manifest read: {:.1} ms\n",
            started.elapsed().as_secs_f64() * 1000.0
        );
    } else {
        log!(
            "[archive] no manifest at {}: only the archive is mounted\n",
            manifest_path().display()
        );
    }
    // SAFETY: RED4ext's SDK table, valid for the plugin's lifetime.
    let hooking = unsafe { sdk.as_ref().and_then(|s| s.hooking.as_ref()) };
    let hook = |hash: u32, detour: usize, slot: &'static AtomicUsize, what: &str| -> i32 {
        let ok = match (reloc::resolve(hash), hooking) {
            // SAFETY: RED4ext's attach writes the trampoline into the slot before the detour can run.
            (Some(target), Some(h)) => unsafe {
                (h.attach)(
                    handle,
                    target as *mut c_void,
                    detour as *mut c_void,
                    slot.as_ptr() as *mut *mut c_void,
                )
            },
            _ => false,
        };
        if !ok {
            log!("[archive] {what} NOT hooked\n");
        }
        ok as i32
    };
    let mut hooked = 0;
    if let Some(load) = reloc::resolve(LOAD_ARCHIVES_HASH) {
        LOAD_ARCHIVES.store(load, Ordering::Release);
        hooked += hook(
            INITIALIZE_ARCHIVES_HASH,
            detour_initialize_archives as *const () as usize,
            &INITIALIZE_ARCHIVES,
            "archive mounting",
        );
    }
    hooked += hook(
        LOAD_FACTORY_ASYNC_HASH,
        detour_load_factory_async as *const () as usize,
        &LOAD_FACTORY_ASYNC,
        "item factories",
    );
    hooked += hook(
        INITIALIZE_ANIMATIONS_HASH,
        detour_initialize_animations as *const () as usize,
        &INITIALIZE_ANIMATIONS,
        "animation sets",
    );
    hooked += hook(
        SERIALIZER_LOAD_HASH,
        detour_serializer_load as *const () as usize,
        &SERIALIZER_LOAD,
        "patch preloading",
    );
    hooked += hook(
        DESERIALIZE_HASH,
        detour_deserialize as *const () as usize,
        &DESERIALIZE,
        "patch load order",
    );
    let post_load = hook(
        MESH_POST_LOAD_HASH,
        detour_mesh_post_load as *const () as usize,
        &MESH_POST_LOAD,
        "mesh patches",
    );
    hooked += post_load;
    let copy_request = hook(DEPOT_REQUEST_HASH, detour_depot_request as *const () as usize, &DEPOT_REQUEST, "private mesh data");
    let copy_check = hook(DEPOT_CHECK_HASH, detour_depot_check as *const () as usize, &DEPOT_CHECK, "private mesh lookup");
    crate::blade_mesh::enable(copy_request != 0 && copy_check != 0 && post_load != 0);
    hooked += hook(
        DEPENDENCIES_READY_HASH,
        detour_dependencies_ready as *const () as usize,
        &DEPENDENCIES_READY,
        "streaming block patches and fixes",
    );
    hooked += hook(
        SPAWN_FROM_TEMPLATE_HASH,
        detour_spawn_from_template as *const () as usize,
        &SPAWN_FROM_TEMPLATE,
        "record appearances",
    );
    let release = reloc::resolve(RELEASE_WEAK_HASH).unwrap_or(0);
    RELEASE_WEAK.store(release, Ordering::Release);
    if release != 0 && appearance_dtor_layout_known(reloc::resolve(MESH_APPEARANCE_DTOR_HASH).unwrap_or(0)) {
        hooked += hook(
            MESH_APPEARANCE_DTOR_HASH,
            detour_mesh_appearance_dtor as *const () as usize,
            &MESH_APPEARANCE_DTOR,
            "mesh appearance release guard",
        );
    } else {
        log!("[archive] mesh appearance release guard NOT hooked: unknown destructor layout\n");
    }
    log!("[archive] {hooked} of 9 engine paths hooked\n");
}
