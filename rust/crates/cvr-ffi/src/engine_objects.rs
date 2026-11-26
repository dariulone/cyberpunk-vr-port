//! ENGINE OBJECTS THE SCRIPTS REACH, WITHOUT CODEWARE (formerly src/Natives/EngineObjects.cpp's logic).
//!
//! Codeware added script methods to engine classes the port's scripts and Lua mods call: an entity's components and
//! world transform, a mesh component's resource and appearance, the ink system's layers, the weather. These are the
//! same operations as natives of the plugin under the port's own names (VR*), so nothing collides with Codeware when
//! it is installed for other mods. What 2.5 already has natively (inkLayerWrapper ...) is used as the game has it: the
//! layer natives hand out the game's own inkLayerWrapper.
//!
//! The lobby's exposure (mods/redscript/CyberpunkVRPort_EngineStereo/vrport_lobby_exposure.reds) is here too: the
//! script registers for the environment resources as they load and hands them over; the settings themselves are
//! native classes no script declares without Codeware.
//!
//! The SDK's job queue (a component's reload), the world widget components' virtual getters and the script call to the
//! weather system stay behind the C++ adapter in src/Natives/EngineObjects.cpp.
use core::ffi::c_void;
use core::mem::ManuallyDrop;
use std::sync::Mutex;

use cvr_host::log;
use red4ext_abi::cname::CName;
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::handle::Handle;
use red4ext_abi::reloc;
use red4ext_abi::resource::resource_path;
use red4ext_abi::rtti::{object_class, vfunc, Class, RttiSystem};
use red4ext_abi::stack::StackFrame;

use crate::native_args::{args, borrowed, handles, push_handle, push_owned, set_handle};

// src/Natives/EngineObjects.cpp: the adapter.
extern "C" {
    /// The component's resource loaded anew and its appearance rebuilt once it is in; with `wait`, once that is done
    /// (5 s at most).
    fn cvr_cpp_component_reload(component: *const Handle, wait: bool) -> bool;
}

/// The world widget components' game controllers (those with a window) of an inkWorldLayer, as weak handles appended
/// to `out` (formerly EngineObjects.cpp's): the layer's DynArray<SharedPtr<inkIWidgetComponentWrapper>> at +0x170,
/// each wrapper's GetWindow (vtable +0x30) and GetGameController (+0x90) -- the slots of the SDK's declaration order,
/// as the C++ was compiled -- returning handles, released here as the C++ temporaries were.
///
/// # Safety
/// `layer` is a live inkWorldLayer; `out` a DynArray<WeakHandle<IScriptable>> result slot.
unsafe fn ink_world_controllers(layer: *mut c_void, out: *mut c_void) {
    use red4ext_abi::handle::WeakHandle;
    // SAFETY: the caller's layer; each controller pushed as the result array's weak handle.
    unsafe {
        each_world_controller(layer, &mut |controller| {
            let weak = ManuallyDrop::new(WeakHandle::from_handle(&controller));
            // SAFETY: the caller's result array; its element is the weak handle's two words.
            let array = &mut *(out as *mut red4ext_abi::dynarray::DynArray<[usize; 2]>);
            if !array.push([weak.instance as usize, weak.ref_count as usize]) {
                drop(ManuallyDrop::into_inner(weak));
            }
        })
    }
}
/// Each of a world layer's widget components with a window: its game controller.
unsafe fn each_world_controller(layer: *mut c_void, sink: &mut dyn FnMut(Handle)) {
    type Getter = unsafe extern "C" fn(*mut c_void, *mut Handle) -> *mut Handle;
    // SAFETY: the layer's component array.
    let components = unsafe { handles((layer as *const u8).add(0x170).cast()) };
    for component in components {
        let wrapper = component[0] as *mut c_void;
        if wrapper.is_null() {
            continue;
        }
        let mut window = core::mem::MaybeUninit::<Handle>::uninit();
        // SAFETY: the wrapper's getter constructs the returned handle in the slot given; dropped at once.
        let has_window = unsafe {
            vfunc::<Getter>(wrapper, 0x30)(wrapper, window.as_mut_ptr());
            !window.assume_init().instance.is_null()
        };
        if !has_window {
            continue;
        }
        let mut controller = core::mem::MaybeUninit::<Handle>::uninit();
        // SAFETY: as above; the controller's handle is ours until the end of this turn.
        let controller = unsafe {
            vfunc::<Getter>(wrapper, 0x90)(wrapper, controller.as_mut_ptr());
            controller.assume_init()
        };
        if !controller.instance.is_null() {
            sink(controller);
        }
    }
}
/// Game.VRInkWorldControllers() for Rust: the world screens' game controllers, each held.
pub(crate) fn ink_world_controller_handles() -> Vec<Handle> {
    let mut out = Vec::new();
    if class("inkWorldLayer").is_none() {
        return out;
    }
    let world = name("inkWorldLayer");
    for layer in layers() {
        let layer = unsafe { borrowed(layer) };
        // SAFETY: a live layer of the ink system.
        if layer.is_null() || !unsafe { native_class(layer.instance) }.is_some_and(|c| c.is_a(world)) {
            continue;
        }
        // SAFETY: the world layer's widget components.
        unsafe { each_world_controller(layer.instance, &mut |c| out.push(c)) };
        break;
    }
    out
}
/// Game.VREntityComponentsOf(entity, class) for Rust: its components of the class (or derived), each held.
pub(crate) fn entity_components_of(entity: &Handle, kind: CName) -> Vec<Handle> {
    let mut out = Vec::new();
    if entity.is_null() {
        return out;
    }
    let Some(wanted) = RttiSystem::get().and_then(|r| r.get_class(kind)) else { return out };
    // SAFETY: the entity, held; its live components.
    for component in unsafe { components(entity.instance) } {
        let component = unsafe { borrowed(component) };
        if !component.is_null() && unsafe { object_class(component.instance) }.is_some_and(|c| c.is_a(wanted.class_name())) {
            out.push((*component).clone());
        }
    }
    out
}

/// The weather system behind GameInstance.GetWeatherSystem, or null (no runtime scene between two sessions)
/// (formerly EngineObjects.cpp's, through the SDK's ExecuteFunction(CClass*, CName, out, ScriptGameInstance())):
/// ScriptGameInstance's class looked up once (kept even when missing, as the C++ static was); no call while the game
/// instance has no scene (its word at +0x78); the function's argument typed from its parameter; the interface's
/// handle released after its system (+0x40) is read.
///
/// # Safety
/// Called on a thread that may run script calls.
unsafe fn weather_system() -> *mut u8 {
    use red4ext_abi::stack::{CStack, StackType};
    static GAME: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let game = *GAME.get_or_init(|| {
        RttiSystem::get().and_then(|r| r.get_class(CName::hashed("ScriptGameInstance"))).map_or(0, |c| c as *const Class as usize)
    });
    let instance = red4ext_abi::engine::game_instance();
    // SAFETY: a live game instance's word at +0x78 (GameInstance::unk78[0]).
    if game == 0 || instance.is_null() || unsafe { *(instance as *const u8).add(0x78).cast::<usize>() } == 0 {
        return core::ptr::null_mut();
    }
    let Some(rtti) = RttiSystem::get() else { return core::ptr::null_mut() };
    // SAFETY: the class RTTI registered, for the process.
    let class = unsafe { &*(game as *const Class) };
    let Some(function) = class.function(CName::hashed("GetWeatherSystem")) else { return core::ptr::null_mut() };
    #[repr(C)]
    struct ScriptGameInstance {
        instance: *mut c_void,
        unk8: i8,
        unk10: i64,
    }
    let game_arg = ScriptGameInstance { instance, unk8: 1, unk10: 0 };
    let f = function;
    let mut args = [StackType {
        // SAFETY: the function's own parameters.
        ty: f.params.as_slice().first().and_then(|p| unsafe { p.as_ref() }).map_or(core::ptr::null(), |p| p.ty),
        value: (&game_arg as *const ScriptGameInstance).cast_mut().cast(),
    }];
    let mut iface = Handle::null();
    let mut result = StackType {
        // SAFETY: the function's own return property, or none.
        ty: unsafe { f.return_type.as_ref() }.map_or(core::ptr::null(), |r| r.ty),
        value: (&raw mut iface).cast(),
    };
    // SAFETY: as above.
    let result_ptr = if unsafe { f.return_type.as_ref() }.is_none() { core::ptr::null_mut() } else { &mut result as *mut StackType };
    // The context ExecuteFunction(CClass*, ...) passes: the game instance's system of that class (none for this one).
    let context = red4ext_abi::engine::system_named("ScriptGameInstance");
    let Some(mut stack) = CStack::new(rtti, context, &mut args, result_ptr) else { return core::ptr::null_mut() };
    let Some(address) = f.execute_address() else { return core::ptr::null_mut() };
    type Execute = unsafe extern "C" fn(*mut c_void, *mut CStack, *mut c_void) -> bool;
    // SAFETY: CBaseFunction::Execute(stack, none) over the function and the stack built above.
    let ran = unsafe {
        core::mem::transmute::<usize, Execute>(address)((f as *const red4ext_abi::function::BaseFunction).cast_mut().cast(), &mut stack, core::ptr::null_mut())
    };
    if !ran || iface.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: the weather interface's system pointer at +0x40.
    unsafe { *(iface.instance as *const u8).add(0x40).cast::<*mut u8>() }
}

// Address-library hashes (red4ext's cyberpunk2077_addresses.json), the functions Codeware calls for the same.
const INK_SYSTEM_INSTANCE_HASH: u32 = 2635337807; // the ink system singleton (a pointer variable)
const PLACED_SET_TRANSFORM_HASH: u32 = 1828854026; // entIPlacedComponent: sets its world transform
const WEATHER_SET_BY_NAME_HASH: u32 = 2334794340; // worldRuntimeSystemWeather: blends to a state by name
const WEATHER_STATE_HASH: u32 = 2679119876; // worldRuntimeSystemWeather: the current state
/// A visual component's dissolve (2.5: 0x1403D1CD8).
const VISUAL_DISSOLVE_HASH: u32 = 3960219843;
const PORT_DISSOLVE_REASON: u8 = 6;

// Fields (2.5, asserted against the SDK in src/Natives/EngineObjects.cpp).
const ENTITY_COMPONENTS: usize = 0xA0; // entEntity: DynArray<Handle<entIComponent>>
const ENTITY_TRANSFORM_COMPONENT: usize = 0xB0; // entEntity: entIPlacedComponent*
const COMPONENT_NAME: usize = 0x40; // entIComponent: CName
const COMPONENT_OWNER: usize = 0x50; // entIComponent: entEntity*
const MESH_PATH: usize = 0x150; // entMeshComponent: RaRef<CMesh>
const MESH_APPEARANCE: usize = 0x190;
const MORPH_PATH: usize = 0x200; // entMorphTargetSkinnedMeshComponent: RaRef<MorphTargetMesh>
const MORPH_APPEARANCE: usize = 0x238;
const SKINNED_PATH: usize = 0x228; // entSkinnedMeshComponent: RaRef<CMesh>
const SKINNED_APPEARANCE: usize = 0x230;
const VISUAL_DISSOLVE_REASONS: usize = 300; // entIVisualComponent: a byte of dissolve reasons
const VISUAL_ATTACHED: usize = 136; // bit 1: attached (the test 0x1403D1CD8 makes)
                                    // A mesh component's appearance rebuild (2.5 vtable, as Codeware has it).
const REFRESH_APPEARANCE_SLOT: usize = 0x280;

fn name(text: &str) -> CName {
    CName(fnv1a64(text.as_bytes()))
}
fn class(text: &str) -> Option<&'static Class> {
    RttiSystem::get()?.get_class(name(text))
}
/// `ISerializable::GetNativeType` (vtable +0x00).
///
/// # Safety
/// `object` is a live serializable.
unsafe fn native_class(object: *const c_void) -> Option<&'static Class> {
    type F = unsafe extern "C" fn(*const c_void) -> *const Class;
    // SAFETY: the caller's.
    unsafe { vfunc::<F>(object, 0x00)(object).as_ref() }
}
/// `object->GetType()->IsA(className)`.
fn is_a(object: *const c_void, class_name: &str) -> bool {
    // SAFETY: a live scriptable object (the handle of a native's argument holds it).
    !object.is_null() && unsafe { object_class(object) }.is_some_and(|c| c.is_a(name(class_name)))
}
/// # Safety
/// `object` is live and `at` one of its fields of type `T`.
unsafe fn field<T>(object: *const c_void, at: usize) -> *mut T {
    (object as *mut u8).wrapping_add(at) as *mut T
}
fn has_owner(component: *const c_void) -> bool {
    // SAFETY: entIComponent's owner.
    !component.is_null()
        && unsafe { !(*field::<*const c_void>(component, COMPONENT_OWNER)).is_null() }
}

// ---- entities

/// The components of the entity of a handle argument, borrowed.
///
/// # Safety
/// `entity` is a live entity.
unsafe fn components<'a>(entity: *const c_void) -> &'a [[usize; 2]] {
    // SAFETY: the caller's.
    unsafe { handles(field::<c_void>(entity, ENTITY_COMPONENTS)) }
}

/// VREntityComponents(entity): its components.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREntityComponents(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    a.finish();
    if out.is_null() || entity.is_null() {
        return;
    }
    for component in unsafe { components(entity.instance) } {
        unsafe { push_handle(out, &borrowed(component)) };
    }
}

/// Game.VREntityComponents(entity) for Rust: the entity's components, each held.
pub(crate) fn entity_components(entity: &Handle) -> Vec<Handle> {
    if entity.is_null() {
        return Vec::new();
    }
    // SAFETY: the entity, held; its components' handles cloned.
    unsafe { components(entity.instance) }.iter().map(|c| (*unsafe { borrowed(c) }).clone()).collect()
}

/// VREntityComponentsOf(entity, className): its components of one class (or derived from it): the list a script would
/// otherwise walk with an IsA call on every component, every frame (world touch: ~20 components a device).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREntityComponentsOf(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    let kind = a.get::<u64>();
    a.finish();
    if out.is_null() || entity.is_null() {
        return;
    }
    let Some(wanted) = RttiSystem::get().and_then(|r| r.get_class(CName(kind))) else {
        return;
    };
    for component in unsafe { components(entity.instance) } {
        let component = unsafe { borrowed(component) };
        // SAFETY: a live component of the entity.
        if !component.is_null()
            && unsafe { object_class(component.instance) }
                .is_some_and(|c| c.is_a(wanted.class_name()))
        {
            unsafe { push_handle(out, &component) };
        }
    }
}

/// VREntitySkinnedMeshes(entity): its skinned meshes (the three classes the lobby's construct handles,
/// modules/construct.lua). Whether each is on is left to the script's IsEnabled: filtered here on the component's own
/// flag, a mesh the system had just switched on showed for frames before the lobby saw it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREntitySkinnedMeshes(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    a.finish();
    if out.is_null() || entity.is_null() {
        return;
    }
    let kinds = [
        name("entSkinnedMeshComponent"),
        name("entGarmentSkinnedMeshComponent"),
        name("entMorphTargetSkinnedMeshComponent"),
    ];
    for component in unsafe { components(entity.instance) } {
        let component = unsafe { borrowed(component) };
        if component.is_null() {
            continue;
        }
        // SAFETY: a live component of the entity.
        let ty = unsafe { object_class(component.instance) }.map(Class::class_name);
        if ty.is_some_and(|t| kinds.contains(&t)) {
            unsafe { push_handle(out, &component) };
        }
    }
}

/// VREntitySetWorldTransform(entity, transform): its root component placed there.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VREntitySetWorldTransform(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    #[repr(C, align(16))]
    #[derive(Default)]
    struct WorldTransform([u8; 0x20]);
    let mut a = unsafe { args(frame) };
    let entity = a.get::<Handle>();
    let transform = a.get::<WorldTransform>();
    a.finish();
    type SetTransformFn =
        unsafe extern "C" fn(placed: *mut c_void, transform: *const WorldTransform);
    let root = if entity.is_null() {
        core::ptr::null_mut()
    } else {
        // SAFETY: the entity's transform component.
        unsafe { *field::<*mut c_void>(entity.instance, ENTITY_TRANSFORM_COMPONENT) }
    };
    let set = reloc::resolve(PLACED_SET_TRANSFORM_HASH);
    let done = !root.is_null() && set.is_some();
    if let (true, Some(set)) = (done, set) {
        // SAFETY: entIPlacedComponent's world transform setter.
        unsafe { core::mem::transmute::<usize, SetTransformFn>(set)(root, &transform) };
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// Game.VREntitySetWorldTransform(entity, transform) for Rust (a WorldTransform's 0x20 bytes); false when it could not.
pub(crate) fn entity_set_world_transform(entity: &Handle, transform: &[u8; 0x20]) -> bool {
    #[repr(C, align(16))]
    struct WorldTransform([u8; 0x20]);
    let transform = WorldTransform(*transform);
    type SetTransformFn = unsafe extern "C" fn(placed: *mut c_void, transform: *const WorldTransform);
    if entity.is_null() {
        return false;
    }
    // SAFETY: the entity's transform component.
    let root = unsafe { *field::<*mut c_void>(entity.instance, ENTITY_TRANSFORM_COMPONENT) };
    match reloc::resolve(PLACED_SET_TRANSFORM_HASH) {
        Some(set) if !root.is_null() => {
            // SAFETY: entIPlacedComponent's world transform setter.
            unsafe { core::mem::transmute::<usize, SetTransformFn>(set)(root, &transform) };
            true
        }
        _ => false,
    }
}
/// Game.VRComponentChangeResource(component, path, wait) for Rust.
pub(crate) fn component_change_resource(component: &Handle, path: &str, wait: bool) -> bool {
    match fields_of(component.instance) {
        Some((resource, _)) => {
            // SAFETY: the component's resource path.
            unsafe { *resource = resource_path(path) };
            reload(component, wait)
        }
        None => false,
    }
}
/// component_change_resource by the resource path's hash (a mesh known only by its hash: a partner's carried prop).
pub(crate) fn component_change_resource_hash(component: &Handle, hash: u64, wait: bool) -> bool {
    match fields_of(component.instance) {
        Some((resource, _)) => {
            // SAFETY: the component's resource path.
            unsafe { *resource = hash };
            reload(component, wait)
        }
        None => false,
    }
}
/// The mesh component's resource path (the hash its `mesh` reference holds), or None for another component.
pub(crate) fn component_resource(component: &Handle) -> Option<u64> {
    // SAFETY: the class's field, read.
    fields_of(component.instance).map(|(resource, _)| unsafe { *resource })
}
/// Game.VRComponentChangeAppearance(component, appearance, wait) for Rust.
pub(crate) fn component_change_appearance(component: &Handle, appearance: u64, wait: bool) -> bool {
    match fields_of(component.instance) {
        Some((_, current)) => {
            // SAFETY: the component's appearance name.
            unsafe { *current = appearance };
            reload(component, wait)
        }
        None => false,
    }
}

// ---- mesh components

/// Where a mesh component keeps its resource and appearance: the mesh components, the skinned ones (garments
/// included) and the morph target ones, each at its own offsets.
fn fields_of(component: *const c_void) -> Option<(*mut u64, *mut u64)> {
    let (path, appearance) = if is_a(component, "entMeshComponent") {
        (MESH_PATH, MESH_APPEARANCE)
    } else if is_a(component, "entMorphTargetSkinnedMeshComponent") {
        (MORPH_PATH, MORPH_APPEARANCE)
    } else if is_a(component, "entSkinnedMeshComponent") {
        (SKINNED_PATH, SKINNED_APPEARANCE)
    } else {
        return None;
    };
    // SAFETY: the class's fields.
    Some(unsafe { (field(component, path), field(component, appearance)) })
}

fn refresh_appearance(component: *mut c_void) {
    type F = unsafe extern "C" fn(*mut c_void);
    // SAFETY: the component's appearance rebuild.
    unsafe { vfunc::<F>(component, REFRESH_APPEARANCE_SLOT)(component) };
}

/// ReloadCutMesh: the component reloaded without waiting (the blade mesh switched to its copy).
pub(crate) fn reload_cut_mesh(component: &Handle) -> bool {
    reload(component, false)
}
fn reload(component: &Handle, wait: bool) -> bool {
    // SAFETY: the job queue's reload of a live component.
    !component.is_null()
        && has_owner(component.instance)
        && unsafe { cvr_cpp_component_reload(component, wait) }
}

/// VRComponentChangeResource(component, path, wait).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentChangeResource(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let path = a.text();
    let wait = a.get::<bool>();
    a.finish();
    let mut done = false;
    if let Some((resource, _)) = fields_of(component.instance) {
        // SAFETY: the component's resource path.
        unsafe { *resource = resource_path(&path) };
        done = reload(&component, wait);
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRComponentChangeAppearance(component, appearance, wait).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentChangeAppearance(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let appearance = a.get::<u64>();
    let wait = a.get::<bool>();
    a.finish();
    let mut done = false;
    if let Some((_, current)) = fields_of(component.instance) {
        // SAFETY: the component's appearance name.
        unsafe { *current = appearance };
        done = reload(&component, wait);
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRComponentDissolve(component, hidden, mode): A VISUAL COMPONENT DISSOLVED OUT OR IN -- the engine's dithered fade
/// (Rendering/DissolveTime, 0.5 s), the one an entity streaming in or an appearance change shows. IVisualComponent
/// keeps a byte of dissolve reasons (+300): drawn while none is set, each change between none and some fades the render
/// proxy. The port sets its own reason bit, so the game's own dissolves (scanner, streaming) are left as they are.
/// `mode` is the engine's last argument (0..2): measured 2026-11-05 on the player's body, 0 fades out and in over
/// ~0.45 s, 1 and 2 fade out the same but 2 comes back in at once. On a component that is switched off only the bit
/// changes, and the proxy it gets when it is switched on again keeps it.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentDissolve(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    let hidden = a.get::<bool>();
    let mode = a.get::<i32>();
    a.finish();
    type DissolveFn =
        unsafe extern "C" fn(component: *mut c_void, hidden: bool, reason: u8, mode: u8) -> u8;
    let dissolve = reloc::resolve(VISUAL_DISSOLVE_HASH);
    let done = dissolve.is_some()
        && has_owner(component.instance)
        && is_a(component.instance, "entIVisualComponent")
        && (0..=2).contains(&mode);
    if let (true, Some(dissolve)) = (done, dissolve) {
        // SAFETY: the engine's dissolve of a visual component.
        unsafe {
            core::mem::transmute::<usize, DissolveFn>(dissolve)(
                component.instance,
                hidden,
                PORT_DISSOLVE_REASON,
                mode as u8,
            )
        };
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRComponentDissolved(component): whether the port's reason holds it dissolved out (the byte the call above writes).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentDissolved(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    a.finish();
    let visual = is_a(component.instance, "entIVisualComponent");
    if !out.is_null() {
        // SAFETY: the visual component's dissolve reasons.
        let held = visual
            && unsafe { *field::<u8>(component.instance, VISUAL_DISSOLVE_REASONS) }
                & (1 << PORT_DISSOLVE_REASON)
                != 0;
        unsafe { *(out as *mut bool) = held };
    }
}

/// VRComponentDissolveReady(component): whether a dissolve reaches its render proxy now -- the engine passes it on only
/// while the component is attached. A component just switched on needs a few frames for that; a fade asked before
/// then only clears the bit, and the proxy then comes in at once (measured 2026-11-05).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentDissolveReady(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    a.finish();
    let visual = has_owner(component.instance) && is_a(component.instance, "entIVisualComponent");
    if !out.is_null() {
        // SAFETY: the visual component's attachment flags.
        let ready = visual && unsafe { *field::<u8>(component.instance, VISUAL_ATTACHED) } & 2 != 0;
        unsafe { *(out as *mut bool) = ready };
    }
}

/// Game.VRComponentRefreshAppearance(component) for Rust.
pub(crate) fn component_refresh_appearance(component: &Handle) -> bool {
    let done = has_owner(component.instance) && fields_of(component.instance).is_some();
    if done {
        refresh_appearance(component.instance);
    }
    done
}

/// VRComponentRefreshAppearance(component).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentRefreshAppearance(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    a.finish();
    let done = has_owner(component.instance) && fields_of(component.instance).is_some();
    if done {
        refresh_appearance(component.instance);
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRComponentKey(component): a component told apart from every other -- its address, mixed with its name (a
/// component gone, the next one may be given its address). Its id is its template's, so the copies of one component
/// the character creation system adds to a puppet all carry the id of the puppet's own.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRComponentKey(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let component = a.get::<Handle>();
    a.finish();
    if out.is_null() {
        return;
    }
    let key = if component.is_null() {
        0
    } else {
        // SAFETY: the component's name.
        component.instance as u64 ^ unsafe { *field::<u64>(component.instance, COMPONENT_NAME) }
    };
    unsafe { *(out as *mut u64) = key };
}

// ---- the ink system's layers

/// The ink system's layers: its first layer manager holds them (2.5, as Codeware has it).
fn layers<'a>() -> &'a [[usize; 2]] {
    let Some(instance) = reloc::resolve(INK_SYSTEM_INSTANCE_HASH) else {
        return &[];
    };
    // SAFETY: the ink system singleton (a pointer variable); its managers (DynArray<SharedPtr> at +0x380), the first
    // manager's layers (DynArray<Handle<inkLayer>> at +0x38).
    unsafe {
        let system = *(instance as *const *const u8);
        if system.is_null() {
            return &[];
        }
        let managers = handles(system.add(0x380).cast());
        match managers.first() {
            Some(&[manager, _]) if manager != 0 => handles((manager as *const u8).add(0x38).cast()),
            _ => &[],
        }
    }
}

/// Every layer of the ink system in the game's own wrapper (Game.VRInkLayers() for Rust).
pub(crate) fn layer_wrappers() -> Vec<Handle> {
    layers()
        .iter()
        .map(|l| unsafe { borrowed(l) })
        .filter(|layer| !layer.is_null())
        .map(|layer| wrap(&layer))
        .filter(|wrapped| !wrapped.is_null())
        .collect()
}

/// The game's own wrapper of a layer (2.5 has it as a native class, with GetVirtualWindow, GetGameControllers ...): its
/// one field, the layer, right after IScriptable.
fn wrap(layer: &Handle) -> Handle {
    static TOLD: Mutex<bool> = Mutex::new(false);
    let ty = class("inkLayerWrapper");
    // SAFETY: a registered class's size.
    let size = ty.map_or(0, |t| unsafe {
        (*(t as *const Class as *const red4ext_abi::rtti::IType)).size()
    });
    let Some(ty) = ty.filter(|_| size >= 0x50) else {
        let mut told = TOLD.lock().unwrap_or_else(|e| e.into_inner());
        if !*told {
            log!("[engine] inkLayerWrapper is not the 2.5 class (size {size}): no layers handed out\n");
            *told = true;
        }
        return Handle::null();
    };
    let wrapper = ty.create_instance();
    if wrapper.is_null() {
        return Handle::null();
    }
    // SAFETY: the new wrapper's layer slot (zeroed by CreateInstance) takes a reference of its own.
    unsafe {
        let slot = field::<Handle>(wrapper, 0x40);
        core::ptr::write(slot, layer.clone());
        Handle::from_instance(wrapper)
    }
}

/// VRInkLayers(): every layer, wrapped.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRInkLayers(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    if out.is_null() {
        return;
    }
    for layer in layers() {
        let layer = unsafe { borrowed(layer) };
        if layer.is_null() {
            continue;
        }
        let wrapped = wrap(&layer);
        if !wrapped.is_null() {
            unsafe { push_owned(out, wrapped) };
        }
    }
}

/// The ink system's layer of this native type (inkMenuLayer, inkHUDLayer ...).
fn find_layer(type_name: u64) -> Option<ManuallyDrop<Handle>> {
    layers()
        .iter()
        .map(|l| unsafe { borrowed(l) })
        .find(|layer| {
            // SAFETY: a live layer of the ink system.
            !layer.is_null()
                && unsafe { native_class(layer.instance) }
                    .is_some_and(|c| c.class_name().0 == type_name)
        })
}

/// Game.VRInkLayer(name) for Rust: the layer of this native type in the game's wrapper, or null.
pub(crate) fn layer_wrapper(type_name: u64) -> Handle {
    find_layer(type_name).map_or(Handle::null(), |layer| wrap(&layer))
}

/// FindInkLayer (Lobby/MenuLobby.cpp, cvr-render menu_lobby): the layer of this native type, or null.
#[no_mangle]
pub extern "C" fn cvr_find_ink_layer(type_name: u64) -> *mut c_void {
    find_layer(type_name).map_or(core::ptr::null_mut(), |l| l.instance)
}

/// VRInkLayer(name): the layer of this native type, wrapped.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRInkLayer(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let type_name = a.get::<u64>();
    a.finish();
    if out.is_null() {
        return;
    }
    if let Some(layer) = find_layer(type_name) {
        unsafe { set_handle(out, wrap(&layer)) };
    }
}

/// VRInkWorldControllers(): the game controllers of the world's widget components that have a window (the screens of
/// terminals, doors ...).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRInkWorldControllers(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    if out.is_null() || class("inkWorldLayer").is_none() {
        return;
    }
    let world = name("inkWorldLayer");
    for layer in layers() {
        let layer = unsafe { borrowed(layer) };
        // SAFETY: a live layer of the ink system.
        if layer.is_null()
            || !unsafe { native_class(layer.instance) }.is_some_and(|c| c.is_a(world))
        {
            continue;
        }
        // SAFETY: the world layer's widget components, through their virtual getters.
        unsafe { ink_world_controllers(layer.instance, out) };
        return;
    }
}

// ---- weather

/// VRWeatherSet(weather, blendTime, priority): the weather blends to a state by name and stays there (the weather cycle
/// stops, +0x8C) until the game sets one.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWeatherSet(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let weather = a.get::<u64>();
    let blend = a.get::<f32>();
    let priority = a.get::<u32>();
    a.finish();
    type SetFn = unsafe extern "C" fn(
        system: *mut u8,
        weather: u64,
        transition: u64,
        blend: f32,
        a5: u32,
        priority: u32,
    ) -> bool;
    // SAFETY: the weather system, when there is one, and its blend by name.
    let system = unsafe { weather_system() };
    let done = match reloc::resolve(WEATHER_SET_BY_NAME_HASH) {
        Some(set) if !system.is_null() => unsafe {
            core::mem::transmute::<usize, SetFn>(set)(system, weather, 0, blend, 0, priority)
        },
        _ => false,
    };
    if done {
        // SAFETY: the weather system's cycle flag.
        unsafe { *(system.add(0x8C) as *mut bool) = false };
    }
    if !out.is_null() {
        unsafe { *(out as *mut bool) = done };
    }
}

/// VRWeatherState(): the current state's name.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRWeatherState(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    type StateFn = unsafe extern "C" fn(system: *mut u8, out: *mut Handle) -> *mut c_void;
    let mut current = Handle::null();
    // SAFETY: the weather system, when there is one, and its current state into a handle of ours.
    let system = unsafe { weather_system() };
    if let (false, Some(state)) = (system.is_null(), reloc::resolve(WEATHER_STATE_HASH)) {
        unsafe { core::mem::transmute::<usize, StateFn>(state)(system, &mut current) };
    }
    let property = class("worldWeatherState").and_then(|c| c.property(name("name")));
    if !out.is_null() {
        let value = match property {
            // SAFETY: the state's name property.
            Some(p) if !current.is_null() => unsafe { *p.value_ptr::<u64>(current.instance) },
            _ => 0,
        };
        unsafe { *(out as *mut u64) = value };
    }
}

// ---- the lobby's exposure

/// Every exposure setting of the environments loaded so far, with the compensation it came with.
struct Exposure {
    settings: Vec<(Handle, f32)>,
    offset: f32,
}
// SAFETY: the handles are only read and written under the lock, on the script thread's natives.
unsafe impl Send for Exposure {}
static EXPOSURE: Mutex<Exposure> = Mutex::new(Exposure {
    settings: Vec::new(),
    offset: 0.0,
});

fn compensation() -> Option<&'static red4ext_abi::function::Property> {
    class("ExposureAreaSettings")?.property(name("exposureCompensationOffset"))
}

/// The area parameters (array<handle:IAreaSettings>) of a struct property of `object` (renderAreaSettings of the area
/// parameters, worldRenderSettings of the environment definition).
///
/// # Safety
/// `object` is a live serializable.
unsafe fn area_parameters<'a>(object: *mut c_void, holder: &str) -> Option<&'a [[usize; 2]]> {
    const CLASS_KIND: u8 = 2; // ERTTIType::Class
                              // SAFETY: the caller's; the properties' own types and offsets.
    unsafe {
        let outer = native_class(object)?.property(name(holder))?;
        let ty = outer.ty.as_ref()?;
        if ty.kind() != CLASS_KIND {
            return None;
        }
        let inner = (*(outer.ty as *const Class)).property(name("areaParameters"))?;
        Some(handles(
            inner.value_ptr::<c_void>(outer.value_ptr::<c_void>(object)),
        ))
    }
}

/// VRLobbyExposureCollect(resource): the exposure settings of an environment resource as it loads; how many are new.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyExposureCollect(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let resource = a.get::<Handle>();
    a.finish();
    let exposure_type = name("ExposureAreaSettings");
    let mut added = 0i32;
    if let (false, Some(compensation)) = (resource.is_null(), compensation()) {
        let params = unsafe { area_parameters(resource.instance, "renderAreaSettings") }
            .or_else(|| unsafe { area_parameters(resource.instance, "worldRenderSettings") });
        if let Some(params) = params {
            let mut exposure = EXPOSURE.lock().unwrap_or_else(|e| e.into_inner());
            for param in params {
                let param = unsafe { borrowed(param) };
                // SAFETY: a live setting of the resource.
                if param.is_null()
                    || !unsafe { native_class(param.instance) }
                        .is_some_and(|c| c.is_a(exposure_type))
                {
                    continue;
                }
                if exposure
                    .settings
                    .iter()
                    .any(|(s, _)| s.instance == param.instance)
                {
                    continue;
                }
                // SAFETY: the setting's compensation (a Float).
                let value = unsafe { compensation.value_ptr::<f32>(param.instance) };
                let before = unsafe { *value };
                exposure.settings.push(((*param).clone(), before));
                // One loaded while the lobby is up takes its offset at once.
                unsafe { *value += exposure.offset };
                added += 1;
            }
        }
    }
    if !out.is_null() {
        unsafe { *(out as *mut i32) = added };
    }
}

/// VRLobbyExposureSet(stops): the lobby's offset on every collected setting over what it had (0: back to what they
/// had).
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyExposureSet(
    _context: *mut c_void,
    frame: *mut StackFrame,
    _out: *mut c_void,
    _a4: i64,
) {
    let mut a = unsafe { args(frame) };
    let offset = a.get::<f32>();
    a.finish();
    let Some(compensation) = compensation() else {
        return;
    };
    let mut exposure = EXPOSURE.lock().unwrap_or_else(|e| e.into_inner());
    exposure.offset = offset;
    for (setting, before) in &exposure.settings {
        // SAFETY: a setting held since it was collected.
        unsafe { *compensation.value_ptr::<f32>(setting.instance) = before + offset };
    }
}

/// VRLobbyExposureGet(): the lobby's offset.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyExposureGet(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    let offset = EXPOSURE.lock().unwrap_or_else(|e| e.into_inner()).offset;
    if !out.is_null() {
        unsafe { *(out as *mut f32) = offset };
    }
}

/// VRLobbyExposureCount(): the settings collected.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRLobbyExposureCount(
    _context: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    let count = EXPOSURE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .settings
        .len() as i32;
    if !out.is_null() {
        unsafe { *(out as *mut i32) = count };
    }
}
