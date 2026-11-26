//! Finding the objects the animation natives and the VRIK solve operate on (formerly src/Natives/AnimLookup.cpp's
//! lookups and the qword helpers of src/Natives/AnimGraph.cpp): the player and its animation controller, the world's
//! animation system, and an entity's animated objects in that system's update buckets.
//!
//! The buckets' layout is the SDK's, handed over by the C++ adapter (`cvr_cpp_anim_layout`, from offsetof and sizeof
//! of `world::AnimationSystem`), and every read of them goes through the SEH adapter as the C++ read them under
//! __try.
//!
//! WINE PUTS THE HEAP SOMEWHERE ELSE, and that one fact stopped VRIK from ever arming on Linux (issue #14). Wine
//! exports wine_get_version from ntdll and real Windows does not; under Wine the heap floor of `classify_qword` is
//! 4 GiB instead of 1 TiB, so Windows behaviour is bit-for-bit what it was -- there the narrow floor is what keeps the
//! memory-scan paths from calling GetType on arbitrary qwords (REDEngine's vectored handler takes an access violation
//! before any __try does).
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::ffi::CStr;
use std::fmt::Write as _;
use std::sync::OnceLock;

use cvr_engine::code::{guarded_call4, read};
use red4ext_abi::cname::CName;
use red4ext_abi::fnv::fnv1a64;
use red4ext_abi::handle::Handle;
use red4ext_abi::rtti::{object_class, Class};
use red4ext_abi::stack::StackFrame;

use crate::native_args::{args, borrowed, handles};

/// Where the update buckets keep what is read here (src/Natives/AnimLookup.cpp fills it from the SDK's types).
#[repr(C)]
#[derive(Default)]
pub(crate) struct Layout {
    pub(crate) buckets: u32,
    pub(crate) bucket_stride: u32,
    pub(crate) bucket_count: u32,
    pub(crate) objects: u32,
    pub(crate) components: u32,
    pub(crate) entities: u32,
    pub(crate) objects_size: u32,
    pub(crate) components_size: u32,
    pub(crate) entities_size: u32,
    /// componentBindings (the dumps): its entries, its size and an entry's size.
    pub(crate) bindings: u32,
    pub(crate) bindings_size: u32,
    pub(crate) binding_stride: u32,
}

extern "C" {
    fn cvr_cpp_anim_layout(out: *mut Layout);
}

pub(crate) fn layout() -> &'static Layout {
    static LAYOUT: OnceLock<Layout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        let mut l = Layout::default();
        // SAFETY: the adapter fills the structure.
        unsafe { cvr_cpp_anim_layout(&mut l) };
        l
    })
}

const ENTITY_COMPONENTS: usize = 0xA0;
const ENTITY_RUNTIME_SCENE: usize = 0xB8;
const COMPONENT_NAME: usize = 0x40;
const SCRIPT_INTERFACE_SYSTEM: usize = 0x40; // worldAnimationSystemScriptInterface::animationSystem

fn name(text: &str) -> CName {
    CName(fnv1a64(text.as_bytes()))
}

// ---- qwords

fn running_under_wine() -> bool {
    static WINE: OnceLock<bool> = OnceLock::new();
    *WINE.get_or_init(|| {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        // SAFETY: ntdll is always loaded; a lookup of an export by name.
        unsafe {
            let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr().cast());
            !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
        }
    })
}

/// ClassifyQword: what kind of thing a qword looks like, by address range ("HEAP", "CODE/VTBL" or "").
pub(crate) fn classify_qword(v: u64) -> &'static str {
    let heap_floor = if running_under_wine() {
        0x0000_0001_0000_0000
    } else {
        0x0000_0100_0000_0000
    };
    if v >= heap_floor && v < 0x0000_7000_0000_0000 {
        "HEAP"
    } else if (0x0000_7F00_0000_0000..0x0000_8000_0000_0000).contains(&v) {
        "CODE/VTBL"
    } else {
        ""
    }
}
pub(crate) fn is_heap(v: u64) -> bool {
    classify_qword(v) == "HEAP"
}

/// SafeReadQword and the like: 0 where the read faults.
pub(crate) fn safe<T: Copy + Default>(address: usize) -> T {
    read::<T>(address).unwrap_or_default()
}

/// SafeGetObjectType: `GetType()` of what may be no object at all, under the SEH adapter.
pub(crate) fn safe_object_type(object: usize) -> Option<&'static Class> {
    let vtable = read::<usize>(object)?;
    let get_type = read::<usize>(vtable.checked_add(8)?)?;
    // SAFETY: a virtual call on a candidate object, caught by the adapter when it is none.
    let class = unsafe {
        guarded_call4(
            get_type,
            object as *mut c_void,
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
        )
    }?;
    // SAFETY: what GetType returns is a registered class or null.
    unsafe { (class as *const Class).as_ref() }
}

fn type_name_of(object: usize) -> Option<CName> {
    safe_object_type(object).map(Class::class_name)
}

/// ScanForAnimationSystemInBlock: a worldAnimationSystem (or its script interface's) among the heap pointers of a
/// block, or one level into them; what looks like one is written to `log`.
pub(crate) fn scan_for_animation_system(
    base: usize,
    size: usize,
    mut log: Option<&mut String>,
) -> usize {
    if base == 0 {
        return 0;
    }
    let system = name("worldAnimationSystem");
    let interface = name("worldAnimationSystemScriptInterface");
    let check = |p: u64,
                 log: &mut Option<&mut String>,
                 describe: &dyn Fn(&str) -> String|
     -> Option<usize> {
        let ty = type_name_of(p as usize)?;
        let text = ty.text();
        if let Some(log) = log.as_deref_mut() {
            if text.to_ascii_lowercase().contains("animationsystem") {
                log.push_str(&describe(text));
            }
        }
        if ty == system {
            return Some(p as usize);
        }
        if ty == interface {
            let inner = safe::<usize>(p as usize + SCRIPT_INTERFACE_SYSTEM);
            if inner != 0 {
                return Some(inner);
            }
        }
        None
    };
    let mut off = 0;
    while off + 8 <= size {
        let p: u64 = safe(base + off);
        if is_heap(p) {
            if let Some(found) = check(p, &mut log, &|t| {
                format!("direct off=0x{off:x} ptr=0x{p:x} type={t}\n")
            }) {
                return found;
            }
            let mut inner = 0;
            while inner + 8 <= 0x200 {
                let q: u64 = safe(p as usize + inner);
                if is_heap(q) {
                    let describe = |t: &str| {
                        format!("nested parentOff=0x{off:x} innerOff=0x{inner:x} ptr=0x{q:x} type={t}\n")
                    };
                    if let Some(found) = check(q, &mut log, &describe) {
                        return found;
                    }
                }
                inner += 8;
            }
        }
        off += 8;
    }
    0
}

// ---- the player

/// `ExecuteGlobalFunction("GetPlayer;GameInstance", out, ScriptGameInstance())` (formerly Natives.cpp's
/// cvr_cpp_get_player): the global function called in cpPlayerSystem's context with the framework's game instance
/// (`unk8` 1, `unk10` 0, as ScriptGameInstance's constructor sets them), its handle result into `out`.
///
/// # Safety
/// `out` is a handle slot of the caller's (null, it is assigned). True when the script call ran (the SDK's
/// ExecuteGlobalFunction's result).
pub(crate) unsafe fn get_player(out: *mut Handle) -> bool {
    use red4ext_abi::stack::{CStack, StackType};
    let Some(rtti) = red4ext_abi::rtti::RttiSystem::get() else { return false };
    let Some(function) = rtti.get_function(CName::hashed("GetPlayer;GameInstance")) else { return false };
    if rtti.get_class(CName::hashed("cpPlayerSystem")).is_none() {
        return false;
    }
    // The context the SDK passes: the game instance's cpPlayerSystem (a global function does not use it).
    let context = red4ext_abi::engine::system_named("cpPlayerSystem");
    #[repr(C)]
    struct ScriptGameInstance {
        instance: *mut c_void,
        unk8: i8,
        unk10: i64,
    }
    let game = ScriptGameInstance { instance: red4ext_abi::engine::game_instance(), unk8: 1, unk10: 0 };
    let f = &function.base;
    let mut args = [StackType {
        ty: f.params.as_slice().first().and_then(|p| unsafe { p.as_ref() }).map_or(core::ptr::null(), |p| p.ty),
        value: &game as *const ScriptGameInstance as *mut c_void,
    }];
    let mut result = StackType {
        // SAFETY: the function's own return property, or none.
        ty: unsafe { f.return_type.as_ref() }.map_or(core::ptr::null(), |r| r.ty),
        value: out.cast(),
    };
    // SAFETY: as above.
    let result_ptr = if unsafe { f.return_type.as_ref() }.is_none() { core::ptr::null_mut() } else { &mut result as *mut StackType };
    let Some(mut stack) = CStack::new(rtti, context, &mut args, result_ptr) else { return false };
    let Some(address) = f.execute_address() else { return false };
    type Execute = unsafe extern "C" fn(*mut c_void, *mut CStack, *mut c_void) -> bool;
    // SAFETY: CBaseFunction::Execute(stack, none) over the function and the stack built above.
    unsafe { core::mem::transmute::<usize, Execute>(address)(f as *const _ as *mut c_void, &mut stack, core::ptr::null_mut()) }
}

/// FindPlayerEntity: GetPlayer(GameInstance)'s entity (the handle is not kept, as the C++ did not keep it).
pub(crate) fn player_entity() -> usize {
    let mut player = Handle::null();
    // SAFETY: the script call into a handle of ours.
    unsafe { get_player(&mut player) };
    player.instance as usize
}

/// The components of an entity, borrowed.
///
/// # Safety
/// `entity` is a live entity.
pub(crate) unsafe fn components<'a>(entity: usize) -> &'a [[usize; 2]] {
    unsafe { handles((entity + ENTITY_COMPONENTS) as *const c_void) }
}

/// FindPlayerAnimationController: the player's entAnimationControllerComponent.
pub(crate) fn player_animation_controller() -> usize {
    let entity = player_entity();
    if entity == 0 {
        return 0;
    }
    let wanted = name("entAnimationControllerComponent");
    // SAFETY: the player's components.
    for component in unsafe { components(entity) } {
        let component = unsafe { borrowed(component) };
        // SAFETY: a live component.
        if !component.is_null()
            && unsafe { object_class(component.instance) }.is_some_and(|c| c.class_name() == wanted)
        {
            return component.instance as usize;
        }
    }
    0
}

// ---- the world's animation system

fn world_animation_system() -> usize {
    // The game instance's system of that class (formerly AnimLookup.cpp's cvr_cpp_game_system).
    let iface = red4ext_abi::engine::system_named("worldAnimationSystemScriptInterface") as usize;
    if iface == 0 {
        return 0;
    }
    // SAFETY: the script interface's system pointer.
    unsafe { *((iface + SCRIPT_INTERFACE_SYSTEM) as *const usize) }
}

fn is_valid_animation_system(p: usize) -> bool {
    is_heap(p as u64) && type_name_of(p) == Some(name("worldAnimationSystem"))
}

/// The worldAnimationSystem consistently sits at runtimeScene+0x8 -> +0x38: one GetType call instead of a scan.
fn try_known_chain(scene: usize) -> usize {
    if scene == 0 {
        return 0;
    }
    let parent: u64 = safe(scene + 0x8);
    if !is_heap(parent) {
        return 0;
    }
    let candidate: u64 = safe(parent as usize + 0x38);
    if !is_heap(candidate) {
        return 0;
    }
    if is_valid_animation_system(candidate as usize) {
        candidate as usize
    } else {
        0
    }
}

static CACHED_SYSTEM: AtomicUsize = AtomicUsize::new(0);

/// FindWorldAnimationSystemFromScene: the one found before while it still looks valid, the game system's, the fixed
/// chain, and only then a scan of the runtime scene and the framework's scene.
pub(crate) fn find_world_animation_system(
    scene: usize,
    framework_scene: usize,
    mut log: Option<&mut String>,
) -> usize {
    let cached = CACHED_SYSTEM.load(Ordering::Relaxed);
    if cached != 0 && is_valid_animation_system(cached) {
        return cached;
    }
    CACHED_SYSTEM.store(0, Ordering::Relaxed);
    let found = 'found: {
        let system = world_animation_system();
        if system != 0 {
            break 'found system;
        }
        let system = try_known_chain(scene);
        if system != 0 {
            if let Some(log) = log.as_deref_mut() {
                log.push_str("found via fixed chain runtimeScene+0x8->+0x38\n");
            }
            break 'found system;
        }
        if let Some(log) = log.as_deref_mut() {
            log.push_str(
                "GetWorldAnimationSystem() returned null, scanning runtime scene memory...\n",
            );
        }
        let system = scan_for_animation_system(scene, 0x4B8, log.as_deref_mut());
        if system != 0 {
            break 'found system;
        }
        if framework_scene != 0 {
            break 'found scan_for_animation_system(framework_scene, 0x800, log.as_deref_mut());
        }
        0
    };
    if found != 0 {
        CACHED_SYSTEM.store(found, Ordering::Relaxed);
    }
    found
}

/// The engine, its framework, the framework's game instance and its runtime scene (+0x18) (formerly AnimLookup.cpp's
/// cvr_cpp_engine_objects).
pub(crate) fn engine_objects() -> [usize; 4] {
    use red4ext_abi::engine::{framework, game_engine, game_instance, world_runtime_scene};
    [game_engine() as usize, framework() as usize, game_instance() as usize, world_runtime_scene()]
}
/// A class by native name and by script name, and the game instance's systems of each (formerly AnimLookup.cpp's
/// cvr_cpp_anim_lookup_row).
fn lookup_row(class_name: &str) -> [usize; 4] {
    let game = red4ext_abi::engine::game_instance();
    let rtti = red4ext_abi::rtti::RttiSystem::get();
    let name = CName::hashed(class_name);
    let native = rtti.and_then(|r| r.get_class(name)).map_or(0, |c| c as *const Class as usize);
    let script = rtti.and_then(|r| r.get_class_by_script_name(name)).map_or(0, |c| c as *const Class as usize);
    // SAFETY: the instance read just now; the classes from RTTI (null when absent).
    let system = |class: usize| unsafe { red4ext_abi::engine::get_system(game, class as *const c_void) } as usize;
    [native, script, system(native), system(script)]
}

/// The animation system an entity's scene has.
fn system_of(entity: usize) -> usize {
    if entity == 0 {
        return 0;
    }
    let scene: usize = safe(entity + ENTITY_RUNTIME_SCENE);
    find_world_animation_system(scene, engine_objects()[3], None)
}

/// One bucket entry: the entity, its animated component and its animated object.
struct Entry {
    entity: usize,
    component: usize,
    object: usize,
}

/// The entries of every update bucket, read as the C++ read them under __try: a bucket whose read faults ends there.
fn for_each_entry(system: usize, mut f: impl FnMut(u32, u32, &Entry) -> bool) {
    let l = layout();
    for bucket_index in 0..l.bucket_count {
        let bucket = system + l.buckets as usize + bucket_index as usize * l.bucket_stride as usize;
        let Some(count) = read::<u32>(bucket + l.entities_size as usize) else {
            continue;
        };
        for entry_index in 0..count {
            let i = entry_index as usize;
            let (Some(entity), Some(component), Some(object)) = (
                read::<usize>(bucket + l.entities as usize + i * 16),
                read::<usize>(bucket + l.components as usize + i * 8),
                read::<usize>(bucket + l.objects as usize + i * 8),
            ) else {
                break;
            };
            if !f(
                bucket_index,
                entry_index,
                &Entry {
                    entity,
                    component,
                    object,
                },
            ) {
                return;
            }
        }
    }
}

fn component_name(component: usize) -> Option<CName> {
    (component != 0)
        .then(|| read::<u64>(component + COMPONENT_NAME).map(CName))
        .flatten()
}

/// FindAnimatedObjectForEntity: the animated object of an entity's component by name, or of any of its components.
pub(crate) fn animated_object_for_entity(
    entity: usize,
    component: Option<&CStr>,
    any: bool,
) -> usize {
    let system = system_of(entity);
    if entity == 0 || system == 0 {
        return 0;
    }
    let wanted = component.map(|c| CName(fnv1a64(c.to_bytes())));
    let mut found = 0;
    for_each_entry(system, |_, _, e| {
        if e.entity != entity {
            return true;
        }
        if any || (wanted.is_some() && component_name(e.component) == wanted) {
            found = e.object;
            return false;
        }
        true
    });
    found
}

/// ListAnimatedComponents: an entity's animated components' names into `out` (space-separated), how many there are
/// (-1 without the system), and the first that is not "root".
pub(crate) fn list_animated_components(
    entity: usize,
    out: &mut Vec<u8>,
    cap: usize,
    first_non_root: &mut usize,
) -> i32 {
    out.clear();
    *first_non_root = 0;
    let system = system_of(entity);
    if entity == 0 || system == 0 {
        return -1;
    }
    let mut count = 0;
    let root = name("root");
    for_each_entry(system, |_, _, e| {
        if e.entity != entity {
            return true;
        }
        count += 1;
        let n = component_name(e.component);
        let text = n.map(CName::text).filter(|t| !t.is_empty()).unwrap_or("?");
        if out.len() + 2 < cap {
            if !out.is_empty() {
                out.push(b' ');
            }
            let room = (cap - 1).saturating_sub(out.len());
            out.extend_from_slice(&text.as_bytes()[..text.len().min(room)]);
        }
        if *first_non_root == 0 && n != Some(root) {
            *first_non_root = e.object;
        }
        true
    });
    count
}

static ROOT_BUCKET: AtomicU32 = AtomicU32::new(0);
static ROOT_ENTRY: AtomicU32 = AtomicU32::new(0);

/// FindPlayerAnimatedObjectByComponentName: the player's animated object of a component. The root's bucket entry is
/// remembered and validated first -- the root binding is checked every game tick without walking every animated
/// entity in the scene.
pub(crate) fn player_animated_object(component: &CStr) -> usize {
    let entity = player_entity();
    let system = system_of(entity);
    if entity == 0 || system == 0 {
        return 0;
    }
    let wanted = CName(fnv1a64(component.to_bytes()));
    let root = wanted == name("root");
    let l = layout();
    if root {
        let bucket = system
            + l.buckets as usize
            + ROOT_BUCKET.load(Ordering::Relaxed) as usize * l.bucket_stride as usize;
        let entry = ROOT_ENTRY.load(Ordering::Relaxed) as usize;
        let sizes = (
            read::<u32>(bucket + l.entities_size as usize),
            read::<u32>(bucket + l.components_size as usize),
            read::<u32>(bucket + l.objects_size as usize),
        );
        if let (Some(e), Some(c), Some(o)) = sizes {
            if (entry as u32) < e
                && (entry as u32) < c
                && (entry as u32) < o
                && read::<usize>(bucket + l.entities as usize + entry * 16) == Some(entity)
            {
                let comp = read::<usize>(bucket + l.components as usize + entry * 8).unwrap_or(0);
                if comp != 0 && component_name(comp) == Some(wanted) {
                    if let Some(object) = read::<usize>(bucket + l.objects as usize + entry * 8) {
                        return object;
                    }
                }
            }
        }
    }
    let mut found = 0;
    for_each_entry(system, |b, i, e| {
        if e.entity == entity && component_name(e.component) == Some(wanted) {
            if root {
                ROOT_BUCKET.store(b, Ordering::Relaxed);
                ROOT_ENTRY.store(i, Ordering::Relaxed);
            }
            found = e.object;
            return false;
        }
        true
    });
    found
}

// ---- the C ABI (cvr-rig's VRIK and the C++ natives' forwarders in src/Natives/AnimLookup.cpp)

/// # Safety
/// `component` is NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_player_animated_object(component: *const c_char) -> *mut c_void {
    if component.is_null() {
        return core::ptr::null_mut();
    }
    player_animated_object(unsafe { CStr::from_ptr(component) }) as *mut c_void
}
/// # Safety
/// `component` is null or NUL-terminated.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_object_for_entity(
    entity: *mut c_void,
    component: *const c_char,
    any: bool,
) -> *mut c_void {
    let component = (!component.is_null()).then(|| unsafe { CStr::from_ptr(component) });
    animated_object_for_entity(entity as usize, component, any) as *mut c_void
}
#[no_mangle]
pub extern "C" fn cvr_anim_player_entity() -> *mut c_void {
    player_entity() as *mut c_void
}
/// # Safety
/// `out` holds `cap` bytes (or is null); `first_non_root` is null or writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_anim_list_components(
    entity: *mut c_void,
    out: *mut c_char,
    cap: usize,
    first_non_root: *mut *mut c_void,
) -> i32 {
    let mut text = Vec::new();
    let mut first = 0;
    let n = list_animated_components(
        entity as usize,
        &mut text,
        if out.is_null() { 0 } else { cap },
        &mut first,
    );
    if !out.is_null() && cap > 0 {
        let take = text.len().min(cap - 1);
        // SAFETY: the caller's buffer.
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), out as *mut u8, take);
            *out.add(take) = 0;
        }
    }
    if !first_non_root.is_null() {
        unsafe { *first_non_root = first as *mut c_void };
    }
    n
}
#[no_mangle]
pub extern "C" fn cvr_anim_qword_is_heap(value: u64) -> bool {
    is_heap(value)
}

// ---- the lookup dump

/// DumpAnimationSystemLookup(): how the world's animation system is found, into animation_system_lookup.txt.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn DumpAnimationSystemLookup(
    _c: *mut c_void,
    frame: *mut StackFrame,
    out: *mut c_void,
    _a4: i64,
) {
    unsafe { args(frame) }.finish();
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Log, "animation_system_lookup.txt");
    let entity = player_entity();
    let [engine, framework, game, framework_scene] = engine_objects();
    let scene = if entity != 0 {
        safe::<usize>(entity + ENTITY_RUNTIME_SCENE)
    } else {
        0
    };
    let mut text = String::new();
    let _ = writeln!(text, "playerEntity=0x{entity:x} runtimeScene=0x{scene:x}");
    let _ = writeln!(
        text,
        "engine=0x{engine:x} framework=0x{framework:x} gameInstance=0x{game:x} worldSceneFromFramework=0x{framework_scene:x}\n"
    );
    for class_name in [
        "worldAnimationSystem",
        "worldAnimationSystemScriptInterface",
        "AnimationSystem",
        "worldRuntimeScene",
    ] {
        let [native, script, native_system, script_system] = lookup_row(class_name);
        let class_text = |c: usize| {
            // SAFETY: a registered class or none.
            unsafe { (c as *const Class).as_ref() }
                .map_or("<null>".to_owned(), |c| c.class_name().text().to_owned())
        };
        let _ = writeln!(text, "name={class_name}");
        let _ = writeln!(text, "  nativeCls=0x{native:x} scriptCls=0x{script:x}");
        let _ = writeln!(
            text,
            "  nativeClsName={} scriptClsName={}",
            class_text(native),
            class_text(script)
        );
        let _ = writeln!(
            text,
            "  nativeSystem=0x{native_system:x} scriptSystem=0x{script_system:x}\n"
        );
    }
    let scanned = find_world_animation_system(scene, framework_scene, Some(&mut text));
    let _ = writeln!(text, "scannedAnimationSystem=0x{scanned:x}");
    let result = if std::fs::write(&path, text).is_ok() {
        1
    } else {
        -1
    };
    if !out.is_null() {
        unsafe { *(out as *mut i32) = result };
    }
}


/// Every animated object of an entity (its components' entries in the animation system).
pub(crate) fn animated_objects_for_entity(entity: usize) -> Vec<usize> {
    let system = system_of(entity);
    let mut found = Vec::new();
    if entity == 0 || system == 0 {
        return found;
    }
    for_each_entry(system, |_, _, e| {
        if e.entity == entity && e.object != 0 && !found.contains(&e.object) {
            found.push(e.object);
        }
        true
    });
    found
}
