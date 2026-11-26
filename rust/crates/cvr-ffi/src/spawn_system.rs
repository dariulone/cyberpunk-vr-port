//! ENTITIES THE PORT SPAWNS, WITHOUT CODEWARE (formerly src/Natives/SpawnSystem.cpp's logic).
//!
//! The lobby's props, lights and figures and the basketball, spawned the way the game spawns its own dynamic entities
//! and the way Codeware's dynamic system does it: an entity stub is created for a record (a template is given a
//! SpawnableObject record of its own first), and once the stub is in, the population system is asked to keep the
//! entity spawned, in view, with its appearance. Nothing of it goes into a save.
//!
//! VRPortSpawnSystem, the game system of the plugin's own that the world's attach and detach reach, stays a native
//! class of the C++ half (src/Natives/SpawnSystem.cpp), with the adapter over the SDK's game systems and their
//! callbacks (`cvr_cpp_spawn_*`, `cvr_cpp_population_*`, `cvr_cpp_stub_*`); what is spawned, kept, entered in the
//! world's entity registry and let go is here, with the natives (all VRSpawn*).
//!
//! THE REGISTRY. worldRuntimeEntityRegistry is a table by entity id that keeps what it has: entering an id it knows
//! changes nothing (sub_14074AD78), and it outlives the session. The game takes its game objects out as they detach;
//! a prop entered here has to be taken out here, or its dead entry stays. The dynamic ids start over with every
//! session, so a prop of a later session could get the id of an earlier one and FindEntityByID answered null for an
//! entity standing in the world. Back from a save the lobby's menu panel was one (2026-11-02): it never built, and
//! with it nothing could be done any more.
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void};
use std::ffi::CStr;
use std::sync::{Mutex, MutexGuard};

use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::fnv::fnv1a32;
use red4ext_abi::handle::Handle;
use red4ext_abi::reloc;
use red4ext_abi::resource::resource_path;
use red4ext_abi::stack::StackFrame;
use red4ext_abi::tweakdbid;

use crate::native_args::args;
use crate::native_tweaks::{rt_rtti_type, rt_tdb_get};
use crate::tweak_records::{rt_rtti_class, rt_tdb_create_record, FlatWrites};

// src/Natives/SpawnSystem.cpp, src/Core/TweakRecords.cpp, src/Core/ArchiveExtensions.cpp: the adapter.
extern "C" {
    /// The game system's name when it, the dynamic id system or the stub system is missing; null when all are there.
    fn cvr_cpp_spawn_missing_system() -> *const c_char;
    fn cvr_cpp_spawn_system_present() -> bool;
}

// ---- the spawner events' listener (formerly SpawnSystem.cpp's, through the SDK's EntitySpawnerCallback)

/// The spawner's event callback as Rust hands it over (unsafe extern "C" fn(kind, id)).
type SpawnerEvent = unsafe extern "C" fn(kind: i32, id: u64);
/// RED4ext::CallbackHandler<void, EntitySpawnerEventType, EntityID, EntityID, EntityStub*>: the invoke takes the
/// arguments by rvalue reference (pointers), then copy, move and destroy the inline target.
#[repr(C)]
struct SpawnerHandler {
    invoke: unsafe extern "C" fn(target: *const usize, kind: *const i32, id: *const u64, other: *const u64, stub: *const *mut c_void),
    copy: unsafe extern "C" fn(dst: *mut usize, src: *const usize),
    moved: unsafe extern "C" fn(dst: *mut usize, src: *mut usize),
    destruct: unsafe extern "C" fn(target: *mut usize),
}
/// EntitySpawnerCallback = Callback<void (*)(EntitySpawnerEventType, EntityID, EntityID, EntityStub*), 32>: the
/// inline target (here: the Rust callback in its first word) and the handler after it.
#[repr(C)]
struct SpawnerCallback {
    target: [usize; 4],
    handler: *const SpawnerHandler,
}
const _: () = assert!(core::mem::size_of::<SpawnerCallback>() == 0x28);

unsafe extern "C" fn spawner_invoke(target: *const usize, kind: *const i32, id: *const u64, _: *const u64, _: *const *mut c_void) {
    // SAFETY: the target's first word is the callback listen stored; the engine's arguments.
    unsafe {
        let callback: SpawnerEvent = core::mem::transmute::<usize, SpawnerEvent>(*target);
        callback(*kind, *id);
    }
}
unsafe extern "C" fn spawner_copy(dst: *mut usize, src: *const usize) {
    // SAFETY: two inline targets.
    unsafe { core::ptr::copy_nonoverlapping(src, dst, 4) };
}
unsafe extern "C" fn spawner_move(dst: *mut usize, src: *mut usize) {
    // SAFETY: as copy (a plain function pointer).
    unsafe { core::ptr::copy_nonoverlapping(src, dst, 4) };
}
unsafe extern "C" fn spawner_destruct(_: *mut usize) {}
static SPAWNER_HANDLER: SpawnerHandler =
    SpawnerHandler { invoke: spawner_invoke, copy: spawner_copy, moved: spawner_move, destruct: spawner_destruct };

/// IEntitySpawnerEventsBroadcaster::RegisterListener(callback&&, "") (vtable +0x1B0): the listener's id, 0 without the
/// broadcaster. The local callback is destroyed after the call, as the C++ temporary was.
unsafe fn cvr_cpp_spawn_listen(callback: SpawnerEvent) -> u32 {
    let events = system("gameIEntitySpawnerEventsBroadcaster");
    if events.is_null() {
        return 0;
    }
    let mut cb = SpawnerCallback { target: [callback as usize, 0, 0, 0], handler: &SPAWNER_HANDLER };
    type F = unsafe extern "C" fn(*mut c_void, *mut SpawnerCallback, *const c_char) -> u32;
    // SAFETY: the live broadcaster's virtual function over a callback of ours.
    unsafe {
        let id = red4ext_abi::rtti::vfunc::<F>(events, 0x1B0)(events, &mut cb, c"".as_ptr());
        (SPAWNER_HANDLER.destruct)(cb.target.as_mut_ptr());
        id
    }
}

/// IEntitySpawnerEventsBroadcaster::UnregisterListener (+0x1B8).
unsafe fn cvr_cpp_spawn_unlisten(listener: u32) {
    let events = system("gameIEntitySpawnerEventsBroadcaster");
    if events.is_null() {
        return;
    }
    type F = unsafe extern "C" fn(*mut c_void, u32) -> u64;
    // SAFETY: the live broadcaster's virtual function.
    unsafe { red4ext_abi::rtti::vfunc::<F>(events, 0x1B8)(events, listener) };
}

// ---- the stub's creation (formerly SpawnSystem.cpp's cvr_cpp_spawn_create_stub)

/// The Rust callback the stub system's answer reaches: the id, and the token (an EntityStubTokenPtr*).
type StubMade = unsafe extern "C" fn(id: u64, token: *mut c_void);
/// RED4ext::CallbackHandler<void, SharedPtr<EntityStubToken>&>: the invoke takes the token by reference.
#[repr(C)]
struct StubHandler {
    invoke: unsafe extern "C" fn(target: *const usize, token: *mut c_void),
    copy: unsafe extern "C" fn(dst: *mut usize, src: *const usize),
    moved: unsafe extern "C" fn(dst: *mut usize, src: *mut usize),
    destruct: unsafe extern "C" fn(target: *mut usize),
}
/// EntityStubCallback = Callback<void (*)(EntityStubTokenPtr&), 128>: the inline target (here: the Rust callback and
/// the id, as the C++ lambda captured them) and the handler at +0x80.
#[repr(C)]
struct StubCallback {
    target: [usize; 16],
    handler: *const StubHandler,
}
const _: () = assert!(core::mem::size_of::<StubCallback>() == 0x88);

unsafe extern "C" fn stub_invoke(target: *const usize, token: *mut c_void) {
    // SAFETY: the target create_stub stored: the callback, then the id.
    unsafe {
        let made: StubMade = core::mem::transmute::<usize, StubMade>(*target);
        made(*target.add(1) as u64, token);
    }
}
unsafe extern "C" fn stub_copy(dst: *mut usize, src: *const usize) {
    // SAFETY: two inline targets.
    unsafe { core::ptr::copy_nonoverlapping(src, dst, 16) };
}
unsafe extern "C" fn stub_move(dst: *mut usize, src: *mut usize) {
    // SAFETY: as copy (plain words).
    unsafe { core::ptr::copy_nonoverlapping(src, dst, 16) };
}
unsafe extern "C" fn stub_destruct(_: *mut usize) {}
static STUB_HANDLER: StubHandler = StubHandler { invoke: stub_invoke, copy: stub_copy, moved: stub_move, destruct: stub_destruct };

/// The pool EntityStubToken is allocated from (Memory::GMPL_SpawnAllocator).
static POOL_GMPL_SPAWN: red4ext_abi::memory::Pool = red4ext_abi::memory::Pool::new("PoolGMPL_Spawn");
static DEC_WEAK_REF: reloc::Reloc = reloc::Reloc::new(red4ext_abi::hashes::Handle_DecWeakRef);

/// A SharedPtr<EntityStubToken> released (its destructor): the strong count down; at zero the weak count through the
/// engine's DecWeakRef, then the token destroyed -- its stub (a UniquePtr) destroyed and freed -- and freed.
///
/// # Safety
/// `token` is the pair, holding a strong reference or empty.
unsafe fn release_token(token: &mut [usize; 2]) {
    let counts = token[1] as *const core::sync::atomic::AtomicU32;
    if counts.is_null() {
        return;
    }
    // SAFETY: as documented; the token's counts, then its instance once the last strong reference is gone.
    unsafe {
        if (*counts).fetch_sub(1, core::sync::atomic::Ordering::AcqRel) == 1 {
            if let Some(dec) = DEC_WEAK_REF.func::<unsafe extern "C" fn(*mut [usize; 2])>() {
                dec(token);
            }
            let instance = token[0] as *mut *mut c_void;
            if !instance.is_null() {
                destroy_stub(*instance);
                POOL_GMPL_SPAWN.free(instance.cast());
            }
        }
    }
    *token = [0, 0];
}

/// IEntityStubSystem::CreateStub(token, request, callback) (vtable +0x1B0) for `id` at `position` / `orientation` from
/// the record: the request (0x40 bytes: the id, the transform -- its position fixed-point, x * 131072 truncated --, the
/// record at +0x30), the callback over `made` and the id, the call's token released after it and the callback destroyed.
///
/// # Safety
/// `position` and `orientation` hold four floats.
unsafe fn cvr_cpp_spawn_create_stub(id: u64, position: *const [f32; 4], orientation: *const [f32; 4], record: u64, made: StubMade) {
    let stubs = system("gameIEntityStubSystem");
    if stubs.is_null() {
        return;
    }
    let mut request = [0u8; 0x40];
    // SAFETY: the caller's vectors; a local request.
    unsafe {
        let p = *position;
        request[0x00..0x08].copy_from_slice(&id.to_le_bytes());
        request[0x10..0x14].copy_from_slice(&((p[0] * 131072.0) as i32).to_le_bytes());
        request[0x14..0x18].copy_from_slice(&((p[1] * 131072.0) as i32).to_le_bytes());
        request[0x18..0x1C].copy_from_slice(&((p[2] * 131072.0) as i32).to_le_bytes());
        for (k, v) in (*orientation).iter().enumerate() {
            request[0x20 + 4 * k..0x24 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        request[0x30..0x38].copy_from_slice(&record.to_le_bytes());
    }
    let mut target = [0usize; 16];
    target[0] = made as usize;
    target[1] = id as usize;
    let mut callback = StubCallback { target, handler: &STUB_HANDLER };
    let mut token = [0usize; 2];
    type F = unsafe extern "C" fn(*mut c_void, *mut [usize; 2], *mut u8, *mut StubCallback) -> *mut c_void;
    // SAFETY: the live system's virtual function over our token, request and callback.
    unsafe {
        red4ext_abi::rtti::vfunc::<F>(stubs, 0x1B0)(stubs, &mut token, request.as_mut_ptr(), &mut callback);
        release_token(&mut token);
        (STUB_HANDLER.destruct)(callback.target.as_mut_ptr());
    }
}

// ---- the entity stubs (formerly SpawnSystem.cpp's services over EntityStubToken and UniquePtr<EntityStub>)

/// The pool EntityStub is allocated from (Memory::Allocator<PoolGMPL_PS>).
static POOL_GMPL_PS: red4ext_abi::memory::Pool = red4ext_abi::memory::Pool::new("PoolGMPL_PS");

/// The token's status (EntityStubToken::status, a byte at +0x10), -1 for an empty token.
///
/// # Safety
/// `token` is an EntityStubTokenPtr (SharedPtr<EntityStubToken>: its instance first).
unsafe fn cvr_cpp_stub_token_status(token: *mut c_void) -> i32 {
    // SAFETY: as documented.
    unsafe {
        let instance = *token.cast::<*const u8>();
        if instance.is_null() { -1 } else { i32::from(*instance.add(0x10)) }
    }
}

/// EntityStubToken::ExtractStub: the token's stub (+0x00), taken out of it.
///
/// # Safety
/// As cvr_cpp_stub_token_status's, the token not empty.
unsafe fn cvr_cpp_stub_token_extract(token: *mut c_void) -> *mut c_void {
    // SAFETY: as documented.
    unsafe {
        let instance = *token.cast::<*mut *mut c_void>();
        core::ptr::replace(instance, core::ptr::null_mut())
    }
}

/// ~EntityStub then the pool's Free (UniquePtr<EntityStub>'s destructor): stubState (+0x30, a Handle) released, then
/// componentStates (+0x08, StaticArray<Handle<ComponentPS>, 2>, its size at +0x28) cleared.
///
/// # Safety
/// `stub` is an EntityStub of the GMPL_PS pool nobody else holds, or null.
unsafe fn destroy_stub(stub: *mut c_void) {
    if stub.is_null() {
        return;
    }
    let bytes = stub.cast::<u8>();
    // SAFETY: as documented; the handles are read out and dropped once.
    unsafe {
        drop(core::ptr::read(bytes.add(0x30).cast::<Handle>()));
        let size = (*bytes.add(0x28).cast::<u32>()).min(2) as usize;
        for i in 0..size {
            drop(core::ptr::read(bytes.add(0x08 + 16 * i).cast::<Handle>()));
        }
        POOL_GMPL_PS.free(stub);
    }
}

/// IEntityStubSystem::DeleteStub(EntityStub*&) (+0x1C8); the stub destroyed here if the system left it.
///
/// # Safety
/// As destroy_stub's.
unsafe fn cvr_cpp_stub_delete(stub: *mut c_void) {
    let mut held = stub;
    let stubs = system("gameIEntityStubSystem");
    if !stubs.is_null() {
        type F = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void;
        // SAFETY: the live system's virtual function over our pointer.
        unsafe { red4ext_abi::rtti::vfunc::<F>(stubs, 0x1C8)(stubs, &mut held) };
    }
    // SAFETY: what the system did not take.
    unsafe { destroy_stub(held) };
}

/// A stub dropped (UniquePtr<EntityStub>'s destructor).
///
/// # Safety
/// As destroy_stub's.
unsafe fn cvr_cpp_stub_free(stub: *mut c_void) {
    // SAFETY: as documented.
    unsafe { destroy_stub(stub) };
}

// ---- the population and the ids (formerly src/Natives/SpawnSystem.cpp's services over the SDK's game systems: the
// vtable slots are those the C++ was compiled with -- the SDK's declaration order, one past its comments)

/// GameInstance::GetSystem of the class `name` (SpawnSystem.cpp's System<T>()), or null.
fn system(name: &str) -> *mut c_void {
    red4ext_abi::engine::system_named(name)
}

/// IDynamicEntityIDSystem::GetNextTransientID (vtable +0x1B0, the id returned through the caller's slot); 0 without
/// the system.
unsafe fn cvr_cpp_spawn_next_id() -> u64 {
    let ids = system("gameIDynamicEntityIDSystem");
    if ids.is_null() {
        return 0;
    }
    type F = unsafe extern "C" fn(*mut c_void, *mut u64) -> *mut u64;
    let mut id = 0u64;
    // SAFETY: the live system's virtual function.
    unsafe { red4ext_abi::rtti::vfunc::<F>(ids, 0x1B0)(ids, &mut id) };
    id
}

/// PopulationEntityRegisterRequest (gameIPopulationSystem.hpp, 0x28 bytes).
#[repr(C)]
struct PopulationRequest {
    entity: u64,
    appearance: u64,
    priority: u8,
    unk11: u8,
    always_spawned: bool,
    spawn_in_view: bool,
    prefetch_appearance: bool,
    unk15: bool,
    unk18: usize,
    unk20: usize,
}
const _: () = assert!(core::mem::size_of::<PopulationRequest>() == 0x28);

/// IPopulationSystem::RegisterEntity (+0x1F0) of a request always spawned, in view too, with the appearance (prefetched)
/// when one is given; whether there was a population system.
unsafe fn cvr_cpp_population_register(id: u64, appearance: u64) -> bool {
    let mut request = PopulationRequest {
        entity: id,
        appearance,
        priority: 0,
        unk11: 0,
        always_spawned: true,
        spawn_in_view: true,
        prefetch_appearance: appearance != 0,
        unk15: false,
        unk18: 0,
        unk20: 0,
    };
    let population = system("gameIPopulationSystem");
    if !population.is_null() {
        type F = unsafe extern "C" fn(*mut c_void, *mut PopulationRequest) -> bool;
        // SAFETY: the live system's virtual function over a request of ours.
        unsafe { red4ext_abi::rtti::vfunc::<F>(population, 0x1F0)(population, &mut request) };
    }
    !population.is_null()
}

/// IPopulationSystem::RemoveEntity(id, 0) (+0x1F8).
unsafe fn cvr_cpp_population_remove(id: u32) {
    let population = system("gameIPopulationSystem");
    if population.is_null() {
        return;
    }
    type F = unsafe extern "C" fn(*mut c_void, u32, u64);
    // SAFETY: the live system's virtual function.
    unsafe { red4ext_abi::rtti::vfunc::<F>(population, 0x1F8)(population, id, 0) };
}

/// IPopulationSystem::FindEntity (+0x1B0) into a handle of ours: the entity, or a null handle.
fn population_entity(id: u32) -> Handle {
    let mut entity = Handle::null();
    let population = system("gameIPopulationSystem");
    if !population.is_null() {
        type F = unsafe extern "C" fn(*mut c_void, *mut Handle, u32) -> *mut c_void;
        // SAFETY: the live system's virtual function filling a null handle.
        unsafe { red4ext_abi::rtti::vfunc::<F>(population, 0x1B0)(population, &mut entity, id) };
    }
    entity
}

/// `*out = FindEntity(id)` (the old value released).
unsafe fn cvr_cpp_population_find(id: u64, out: *mut Handle) {
    // SAFETY: the caller's handle slot.
    drop(unsafe { core::ptr::replace(out, population_entity(id as u32)) });
}

/// The entity, when the population has it, appended to `out` (a DynArray<Handle<entEntity>>).
unsafe fn cvr_cpp_spawn_push_entity(out: *mut c_void, id: u64) {
    if system("gameIPopulationSystem").is_null() {
        return;
    }
    let entity = population_entity(id as u32);
    if !entity.instance.is_null() {
        // SAFETY: the caller's result array.
        unsafe { crate::native_args::push_handle(out, &entity) };
    }
}

/// The entity is a gameObject (its class or an ancestor).
unsafe fn cvr_cpp_entity_is_game_object(entity: *mut c_void) -> bool {
    // SAFETY: a live entity's class.
    unsafe { red4ext_abi::rtti::object_class(entity) }.is_some_and(|c| c.is_a(CName::hashed("gameObject")))
}

/// framework->unk18: the world's runtime scene.
unsafe fn cvr_cpp_runtime_scene() -> usize {
    red4ext_abi::engine::world_runtime_scene()
}

const REGISTER_ENTITY_HASH: u32 = 2840271332; // enters an entity
const UNREGISTER_ENTITY_HASH: u32 = 3878623943; // takes it out (sub_14074A848)
const ENTITY_REGISTRY_SLOT: usize = 13; // the registry among the runtime scene's systems
const SPAWN_EVENT: i32 = 2; // gameEntitySpawnerEventType::Spawn
const DESPAWN_EVENT: i32 = 3; // ::Despawn
const STUB_CREATED: i32 = 1; // EntityStubStatus::Created

type RegistryFn = unsafe extern "C" fn(registry: *mut c_void, entity: *mut c_void);

struct Spawned {
    record: u64,
    appearance: u64,
    tags: Vec<u64>,
    /// The EntityStub, owned (a UniquePtr's pointer) once the stub system made it.
    stub: *mut c_void,
    deleted: bool,
    // What was entered in the registry, and the registry: held until it is out again.
    entered: Handle,
    registry: Handle,
}

#[derive(Default)]
struct State {
    by_id: FastMap<u64, Spawned>,
    listener: u32,
}
// SAFETY: the engine objects held (a stub, two handles) are only handed back to the engine's own thread-safe systems.
unsafe impl Send for State {}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn state() -> MutexGuard<'static, Option<State>> {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(State::default);
    guard
}
fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = state();
    f(guard.as_mut().expect("made by state()"))
}

fn present() -> bool {
    // SAFETY: whether the game instance holds VRPortSpawnSystem.
    unsafe { cvr_cpp_spawn_system_present() }
}

fn record_text(record: u64) -> String {
    format!("{:08X}:{:02X}", record as u32, (record >> 32) as u8)
}

/// Listen: the spawner's events, once.
#[no_mangle]
pub extern "C" fn cvr_spawn_listen() {
    if with(|s| s.listener) != 0 {
        return;
    }
    // SAFETY: the broadcaster's RegisterListener, with a callback that only looks the id up.
    let listener = unsafe { cvr_cpp_spawn_listen(on_spawner_event) };
    with(|s| {
        if s.listener == 0 {
            s.listener = listener;
        }
    });
}

/// StopListening (the world goes).
#[no_mangle]
pub extern "C" fn cvr_spawn_stop_listening() {
    let listener = with(|s| core::mem::take(&mut s.listener));
    if listener != 0 {
        // SAFETY: the id RegisterListener gave.
        unsafe { cvr_cpp_spawn_unlisten(listener) };
    }
}

/// The world went: its stubs and entities with it, and what the registry was told of them.
#[no_mangle]
pub extern "C" fn cvr_spawn_world_detached() {
    let gone = with(|s| core::mem::take(&mut s.by_id));
    for (_, mut spawned) in gone {
        leave_registry(&mut spawned.entered, &mut spawned.registry);
        if !spawned.stub.is_null() {
            // SAFETY: the stub this held, freed as its UniquePtr would free it.
            unsafe { cvr_cpp_stub_free(spawned.stub) };
        }
    }
}

fn leave_registry(entered: &mut Handle, registry: &mut Handle) {
    let (entity, registry) = (core::mem::take(entered), core::mem::take(registry));
    if entity.is_null() || registry.is_null() {
        return;
    }
    // SAFETY: the registry's own removal of the entity it was given.
    if let Some(unregister) = reloc::resolve(UNREGISTER_ENTITY_HASH) {
        unsafe {
            core::mem::transmute::<usize, RegistryFn>(unregister)(
                registry.instance,
                entity.instance,
            )
        };
    }
}

fn find_entity(id: u64) -> Handle {
    let mut entity = Handle::null();
    // SAFETY: IPopulationSystem::FindEntity into a handle of ours.
    unsafe { cvr_cpp_population_find(id, &mut entity) };
    entity
}

/// The world's entity registry (the runtime scene's system at slot 13).
fn registry() -> Handle {
    // SAFETY: the engine framework's runtime scene, whose systems are handles.
    unsafe {
        let scene = cvr_cpp_runtime_scene();
        if scene == 0 {
            return Handle::null();
        }
        (*((scene as *const Handle).add(ENTITY_REGISTRY_SLOT))).clone()
    }
}

unsafe extern "C" fn on_spawner_event(kind: i32, id: u64) {
    if !with(|s| s.by_id.contains_key(&id)) {
        return;
    }
    if kind == SPAWN_EVENT {
        let entity = find_entity(id);
        if entity.is_null() {
            log!("[spawn] {id:016X} spawned, but the population system has no entity for it\n");
            return;
        }
        cvr_host::vlog!("[spawn] {id:016X} in the world\n");
        // A prop is no game object and the registry would not know it: entered here, FindEntityByID finds it.
        // SAFETY: the entity the population system just gave.
        if unsafe { !cvr_cpp_entity_is_game_object(entity.instance) } {
            let registry = registry();
            match reloc::resolve(REGISTER_ENTITY_HASH).filter(|_| !registry.is_null()) {
                Some(register) => {
                    // SAFETY: the registry's own entry of an entity.
                    unsafe {
                        core::mem::transmute::<usize, RegistryFn>(register)(
                            registry.instance,
                            entity.instance,
                        )
                    };
                    with(|s| {
                        if let Some(spawned) = s.by_id.get_mut(&id) {
                            spawned.entered = entity;
                            spawned.registry = registry;
                        }
                    });
                }
                None => log!("[spawn] {id:016X}: no runtime entity registry to enter it in\n"),
            }
        }
    } else if kind == DESPAWN_EVENT {
        let held = with(|s| {
            s.by_id.get_mut(&id).map(|sp| {
                (
                    core::mem::take(&mut sp.entered),
                    core::mem::take(&mut sp.registry),
                )
            })
        });
        if let Some((mut entered, mut registry)) = held {
            leave_registry(&mut entered, &mut registry);
        }
        with(|s| {
            if s.by_id.get(&id).is_some_and(|sp| sp.deleted) {
                s.by_id.remove(&id);
            }
        });
    }
}

/// A template's own SpawnableObject record, named by its path (the name Codeware gives it too, so either makes the
/// same record).
fn template_record(path: &str) -> u64 {
    let resource = resource_path(path);
    let record = fnv1a32(&resource.to_le_bytes()) as u64 | 1 << 32;
    // SAFETY: TweakDB::Get, and the adapter over the TweakDB's records.
    let db = unsafe { rt_tdb_get() };
    if db.is_null() {
        return 0;
    }
    if unsafe { red4ext_abi::tweakdb::record_exists(db, record) } {
        return record;
    }
    // The name goes into the pool first: RTTI makes a composite type from its name's text.
    CName::add(c"raRef:CResource");
    // SAFETY: RTTI by name.
    let (ref_type, record_type) = unsafe {
        (
            rt_rtti_type(c"raRef:CResource".as_ptr()),
            rt_rtti_class(c"gamedataSpawnableObject_Record".as_ptr()),
        )
    };
    if ref_type.is_null() || record_type.is_null() {
        let what = if ref_type.is_null() {
            "raRef:CResource"
        } else {
            "SpawnableObject record"
        };
        log!("[spawn] no {what} type: {path} gets no record\n");
        return 0;
    }
    {
        let mut writes = FlatWrites::new(db);
        let mut value = resource;
        writes.set(
            tweakdbid::derive(record, ".entityTemplatePath"),
            ref_type,
            (&mut value as *mut u64).cast(),
        );
    }
    // SAFETY: TweakDB::CreateRecord of the record type.
    if unsafe { rt_tdb_create_record(db, record, record_type) } {
        log!("[spawn] record {} made for {path}\n", record_text(record));
        return record;
    }
    log!("[spawn] the TweakDB made no record for {path}\n");
    0
}

fn record_exists(record: u64) -> bool {
    // SAFETY: TweakDB::Get and its records by id, under their lock.
    let db = unsafe { rt_tdb_get() };
    !db.is_null() && unsafe { red4ext_abi::tweakdb::record_exists(db, record) }
}

/// The stub system's answer for `id`: kept and handed to the population system, or given back.
unsafe extern "C" fn on_stub_made(id: u64, token: *mut c_void) {
    // SAFETY: the token the stub system passes its callback.
    let status = unsafe { cvr_cpp_stub_token_status(token) };
    let created = status == STUB_CREATED;
    let mut appearance = None;
    let mut record = 0;
    if created {
        with(|s| {
            // Deleted while its stub was made: the stub goes back at once.
            if let Some(spawned) = s.by_id.get_mut(&id).filter(|sp| !sp.deleted) {
                // SAFETY: the token's stub, owned from here.
                spawned.stub = unsafe { cvr_cpp_stub_token_extract(token) };
                appearance = Some(spawned.appearance);
                record = spawned.record;
            }
        });
    }
    let Some(appearance) = appearance else {
        if !created {
            log!("[spawn] the stub system made no stub (status {status})\n");
        } else {
            // SAFETY: the token's stub, back to the stub system.
            unsafe { cvr_cpp_stub_delete(cvr_cpp_stub_token_extract(token)) };
        }
        with(|s| s.by_id.remove(&id));
        return;
    };
    // SAFETY: IPopulationSystem::RegisterEntity: always spawned (a place a kilometre wide: nothing of it streams out
    // with distance), in view, with its appearance.
    let handed = unsafe { cvr_cpp_population_register(id, appearance) };
    let to = if handed {
        "handed to the population system"
    } else {
        "NO population system"
    };
    cvr_host::vlog!(
        "[spawn] {id:016X}: stub made (record {}), {to}\n",
        record_text(record)
    );
}

/// One entity of a SpawnableObject (or Character ...) record; its id, or none.
fn spawn(
    record: u64,
    appearance: u64,
    position: &[f32; 4],
    orientation: &[f32; 4],
    tags: &[u64],
) -> u64 {
    // SAFETY: which of the game's systems is missing, if one is.
    let missing = unsafe { cvr_cpp_spawn_missing_system() };
    if !missing.is_null() {
        // SAFETY: a NUL-terminated name.
        let name = unsafe { CStr::from_ptr(missing) }.to_string_lossy();
        log!("[spawn] no spawn: the game's {name} is not there\n");
        return 0;
    }
    if record == 0 || !record_exists(record) {
        log!(
            "[spawn] no spawn: record {} is not in the TweakDB\n",
            record_text(record)
        );
        return 0;
    }
    cvr_spawn_listen();
    // SAFETY: IDynamicEntityIDSystem::GetNextTransientID.
    let id = unsafe { cvr_cpp_spawn_next_id() };
    if id == 0 {
        return 0;
    }
    with(|s| {
        s.by_id.insert(
            id,
            Spawned {
                record,
                appearance,
                tags: tags.to_vec(),
                stub: core::ptr::null_mut(),
                deleted: false,
                entered: Handle::null(),
                registry: Handle::null(),
            },
        )
    });
    // SAFETY: IEntityStubSystem::CreateStub for this id, transform and record; the callback runs once.
    unsafe { cvr_cpp_spawn_create_stub(id, position, orientation, record, on_stub_made) };
    id
}

fn delete(id: u64) -> bool {
    let stub = with(|s| {
        let spawned = s.by_id.get_mut(&id).filter(|sp| !sp.deleted)?;
        spawned.deleted = true;
        let stub = core::mem::replace(&mut spawned.stub, core::ptr::null_mut());
        // Never spawned (or not yet): nothing will report its despawn.
        if stub.is_null() {
            s.by_id.remove(&id);
        }
        Some(stub)
    });
    let Some(stub) = stub else { return false };
    if !stub.is_null() {
        // SAFETY: the population system lets the entity go, the stub system takes the stub back.
        unsafe {
            cvr_cpp_population_remove(id as u32);
            cvr_cpp_stub_delete(stub);
        }
    }
    true
}

fn tagged(tag: u64) -> Vec<u64> {
    with(|s| {
        s.by_id
            .iter()
            .filter(|(_, sp)| !sp.deleted && sp.tags.contains(&tag))
            .map(|(&id, _)| id)
            .collect()
    })
}

// ---- the natives

/// # Safety
/// The VM's call of a native: `frame` is its frame, `out` its result slot or null.
unsafe fn spawn_native(frame: *mut StackFrame, out: *mut c_void, from_template: bool) {
    // SAFETY: the caller's.
    let mut args = unsafe { args(frame) };
    let (path, record) = if from_template {
        (Some(args.text()), 0)
    } else {
        (None, args.get::<u64>())
    };
    let appearance = args.get::<u64>();
    let position = args.get::<[f32; 4]>();
    let orientation = args.get::<[f32; 4]>();
    let tags = args.names();
    args.finish();
    let id = match path {
        Some(path) => {
            // SAFETY: ResourceDepot::Get()->ResourceExists.
            if unsafe { !crate::archive_ext::rt_depot_exists(crate::archive_ext::rt_depot_get(), resource_path(&path)) } {
                log!("[spawn] {path} is in no archive\n");
                0
            } else {
                spawn(
                    template_record(&path),
                    appearance,
                    &position,
                    &orientation,
                    &tags,
                )
            }
        }
        None => spawn(record, appearance, &position, &orientation, &tags),
    };
    if !out.is_null() {
        // SAFETY: an entEntityID result.
        unsafe { *(out as *mut u64) = id };
    }
}

/// VRSpawnTemplate(path, appearance, position, orientation, tags): an entity of a template (an .ent path), through a
/// SpawnableObject record of its own.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnTemplate(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { spawn_native(frame, out, true) };
}

/// VRSpawnRecord(record, appearance, position, orientation, tags): an entity of a record (a Character's NPC puppet ...).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnRecord(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { spawn_native(frame, out, false) };
}

/// # Safety
/// Called by the script VM.
unsafe fn one_param<T: Default>(frame: *mut StackFrame) -> T {
    // SAFETY: the caller's.
    let mut args = unsafe { args(frame) };
    let value = args.get::<T>();
    args.finish();
    value
}

/// For the mechanics: an entity of a record at a place (its id, 0 none), the entity of an id once it is in, and one let go.
pub(crate) fn spawn_record(record: u64, position: &[f32; 4], orientation: &[f32; 4]) -> u64 {
    if present() {
        spawn(record, 0, position, orientation, &[])
    } else {
        0
    }
}
/// For the mechanics (mechanics/ent_spawn.rs, without Cyber Engine Tweaks): an entity of a template (an .ent path) at a
/// place through the template's own record (its id, 0 none), and whether the spawner is there.
pub(crate) fn spawn_template(path: &str, appearance: u64, position: &[f32; 4], orientation: &[f32; 4]) -> u64 {
    if !present() {
        return 0;
    }
    // SAFETY: ResourceDepot::Get()->ResourceExists.
    if unsafe { !crate::archive_ext::rt_depot_exists(crate::archive_ext::rt_depot_get(), resource_path(path)) } {
        log!("[spawn] {path} is in no archive\n");
        return 0;
    }
    spawn(template_record(path), appearance, position, orientation, &[])
}
pub(crate) fn available() -> bool {
    present()
}
pub(crate) fn spawned(id: u64) -> Handle {
    let mut h = Handle::null();
    if id != 0 && present() {
        // SAFETY: the population system's entity written into our handle.
        unsafe { cvr_cpp_population_find(id, &mut h) };
    }
    h
}
pub(crate) fn delete_spawned(id: u64) -> bool {
    present() && delete(id)
}
/// VRSpawnDelete(id): the entity let go.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnDelete(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let id = unsafe { one_param::<u64>(frame) };
    let done = present() && delete(id);
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRSpawnDeleteTagged(tag): every one with the tag let go; how many.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnDeleteTagged(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let tag = unsafe { one_param::<u64>(frame) };
    let mut deleted = 0i32;
    if present() {
        for id in tagged(tag) {
            delete(id);
            deleted += 1;
        }
    }
    if !out.is_null() {
        unsafe { *(out as *mut i32) = deleted };
    }
}

/// VRSpawnManaged(id): spawned here and not let go.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnManaged(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let id = unsafe { one_param::<u64>(frame) };
    let known = present() && with(|s| s.by_id.get(&id).is_some_and(|sp| !sp.deleted));
    if !out.is_null() {
        unsafe { *(out as *mut bool) = known };
    }
}

/// VRSpawnedEntity(id): its entity once it is in.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnedEntity(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let id = unsafe { one_param::<u64>(frame) };
    if !out.is_null() && present() {
        // SAFETY: the population system's entity written into the handle result.
        unsafe { cvr_cpp_population_find(id, out as *mut Handle) };
    }
}

/// VRSpawnTagged(tag): the entities with the tag that are in.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnTagged(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let tag = unsafe { one_param::<u64>(frame) };
    if out.is_null() || !present() {
        return;
    }
    for id in tagged(tag) {
        // SAFETY: the entity, when it is in, pushed onto the array:handle:entEntity result.
        unsafe { cvr_cpp_spawn_push_entity(out, id) };
    }
}

/// VRSpawnTaggedIDs(tag): the ids with the tag.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRSpawnTaggedIDs(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let tag = unsafe { one_param::<u64>(frame) };
    if out.is_null() || !present() {
        return;
    }
    // SAFETY: an array:entEntityID result.
    let ids = unsafe { &mut *(out as *mut DynArray<u64>) };
    for id in tagged(tag) {
        ids.push(id);
    }
}
