//! The Odyssey 3D SDK, in the plugin: engine stereo without Samsung's SDK, its registry key or a cvar ini (formerly
//! src/Stereo/EngineStereoSdk.cpp; the file keeps the exported counters and the detour's registration, and its
//! `NoteMainMenu` / `Armed` / ... forward here).
//!
//! 2.5 renders the two-eye frame (render feature 98) only through its Samsung Odyssey 3D wrapper (platform+0x4680,
//! built by sub_142282E60 for every launch): the wrapper loads Odyssey3DSDK.dll from the path under
//! HKCU\SOFTWARE\Samsung\Odyssey3DHub, makes the SDK's factory and, from it, the interface the frame gate asks every
//! frame (sub_14227E5D0: [Odyssey3D] cvar && wrapper && interface Available()). Here the wrapper's constructor is
//! followed and, in engine stereo, handed the plugin's own factory and interface -- the contract the old stand-in DLL
//! implemented (tools/odyssey3d_fake_sdk/Odyssey3DSDK.cpp) -- and the three cvars are set in memory.
//!
//! STEREO STARTS AT THE MAIN MENU. Until the main menu has been shown (the redscript controller calls
//! VREngineStereoMenuShown) the interface answers "not available" and StereoEnabled is off: splash screens, intro
//! videos and the first loading render mono, and the startup window path (sub_1408F36D8, which with stereo on would
//! take the SDK's display size and go fullscreen) never fires. From the main menu on, stereo stays on.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering::*};

#[allow(non_upper_case_globals)]
extern "C" {
    /// Calls into the plugin's SDK: [0] factory, [1] interface, by slot (8 bytes each).
    static CyberpunkVR_DebugEngineStereoSdkCalls: [[AtomicU64; 16]; 2];
    /// 1 once the main menu has been shown.
    static CyberpunkVR_EngineStereoArmed: AtomicI32;
    /// Main menu controllers initialised and not yet uninitialised.
    static CyberpunkVR_EngineStereoMainMenus: AtomicI32;
    /// One eye's render size (src/Core/VrCore.cpp's launcher globals).
    fn cvr_cpp_launcher_size(width: *mut i32, height: *mut i32);
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
}

/// The trampoline MinHook writes (src/Stereo/EngineStereoSdk.cpp's CVR_DETOUR_IF line names it).
#[no_mangle]
pub static mut cvr_engine_stereo_wrapper_ctor_original: usize = 0;

/// The [Odyssey3D] cvars' values (ConfigVar<bool>, value byte at +0x30 of each 0x38-byte var in .data), read by the
/// frame gate, the camera pose builder sub_1402A59B8, the startup window path and sub_142083400.
const OVERRIDE_ENABLED_RVA: usize = 0x3929C98; // use StereoEnabled instead of the user setting
const STEREO_ENABLED_RVA: usize = 0x3929CD0;
const SBS_DEBUG_RVA: usize = 0x3929D08; // MapStereo3D (sub_142284580) keeps the raw side-by-side
/// The wrapper: +0 loaded, +8 factory, +24 create_odyssey3d_factory_instance, +32 destroy_odyssey3d_factory_instance;
/// sub_142284C50 then makes the interface (+16) from the factory.
const LOADED: usize = 0;
const FACTORY: usize = 8;
const CREATE: usize = 24;
const DESTROY: usize = 32;
const SLOTS: usize = 16;

static ARMED: AtomicBool = AtomicBool::new(false);

fn log(text: &str) {
    cvr_host::log::write(text);
}
fn exe_base() -> usize {
    static BASE: AtomicUsize = AtomicUsize::new(0);
    let mut base = BASE.load(Relaxed);
    if base == 0 {
        // SAFETY: the process image's own handle.
        base = unsafe { GetModuleHandleW(core::ptr::null()) } as usize;
        BASE.store(base, Relaxed);
    }
    base
}
fn count(table: usize, slot: usize) {
    // SAFETY: the plugin's exported counters.
    unsafe { CyberpunkVR_DebugEngineStereoSdkCalls[table][slot].fetch_add(1, Relaxed) };
}
fn write_cvars(stereo: bool) {
    let base = exe_base();
    if base == 0 {
        return;
    }
    // SAFETY: the cvars' value bytes in the game's .data.
    unsafe {
        core::ptr::write_volatile((base + OVERRIDE_ENABLED_RVA) as *mut u8, 1);
        core::ptr::write_volatile((base + STEREO_ENABLED_RVA) as *mut u8, stereo as u8);
        core::ptr::write_volatile((base + SBS_DEBUG_RVA) as *mut u8, 1);
    }
}

#[repr(C)]
struct Object {
    vtable: *const *const c_void,
}

/// A slot outside the contract: logged once, answered 0.
unsafe extern "C" fn unknown<const TABLE: usize, const SLOT: usize>(
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
) -> u64 {
    if CyberpunkVR_DebugEngineStereoSdkCalls[TABLE][SLOT].fetch_add(1, Relaxed) == 0 {
        log(&format!(
            "[engine-stereo-sdk] {} slot +{} called -- not in the contract, answered 0\n",
            if TABLE != 0 { "interface" } else { "factory" },
            SLOT * 8
        ));
    }
    0
}

// ---- interface (MSVC returns the structs through a hidden pointer: each takes `ret` and returns it) ----------

/// The monitor's head-tracked viewer, millimetres (sub_142284AA0 scales by 0.001); the stand-in's values.
unsafe extern "C" fn eye_position(_: *mut Object, ret: *mut f32, right: bool) -> *mut f32 {
    count(1, 1);
    *ret = if right { 32.0 } else { -32.0 };
    *ret.add(1) = 0.0;
    *ret.add(2) = 600.0;
    ret
}
/// The display: the window the port made, both eyes wide.
unsafe extern "C" fn resolution(_: *mut Object, ret: *mut f32) -> *mut f32 {
    count(1, 2);
    let (mut w, mut h) = (0, 0);
    cvr_cpp_launcher_size(&mut w, &mut h);
    *ret = (2 * w) as f32;
    *ret.add(1) = h as f32;
    ret
}
/// A 27" 16:9 panel, the Odyssey 3D's size, in centimetres (the wrapper keeps x * 0.01).
unsafe extern "C" fn physical_size(_: *mut Object, ret: *mut f32) -> *mut f32 {
    count(1, 3);
    *ret = 59.77;
    *ret.add(1) = 33.62;
    ret
}
/// Compared by the user-settings path only (sub_14226F850): zero never matches a monitor, no mode switch.
unsafe extern "C" fn monitor_rect(_: *mut Object, ret: *mut i64) -> *mut i64 {
    count(1, 6);
    core::ptr::write_bytes(ret, 0, 4);
    ret
}
/// Once a frame (sub_1422854F0), result unused.
unsafe extern "C" fn frame_tick(_: *mut Object) -> u64 {
    count(1, 9);
    0
}
/// The gate: stereo from the main menu on.
unsafe extern "C" fn available(_: *mut Object) -> bool {
    count(1, 10);
    ARMED.load(Relaxed)
}
unsafe extern "C" fn delete_interface(this: *mut Object, flags: u32) -> *mut Object {
    count(1, 12);
    if flags & 1 != 0 {
        drop(Box::from_raw(this));
    }
    this
}

// ---- factory -------------------------------------------------------------------------------------------------

unsafe extern "C" fn create_interface(_: *mut Object, ret: *mut *mut Object) -> *mut *mut Object {
    count(0, 0);
    *ret = Box::into_raw(Box::new(Object {
        vtable: INTERFACE.0.as_ptr(),
    }));
    ret
}
/// MapStereo3D asks for Samsung's weaver only without SBSDebug: an empty pair, which the engine checks before use.
#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn create_weaver(
    _: *mut Object,
    ret: *mut *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
    _: *mut c_void,
) -> *mut *mut c_void {
    count(0, 2);
    *ret = core::ptr::null_mut();
    *ret.add(1) = core::ptr::null_mut();
    ret
}

#[repr(transparent)]
struct Vtable([*const c_void; SLOTS]);
// SAFETY: function addresses, never written.
unsafe impl Sync for Vtable {}

macro_rules! unknown_table {
    ($t:literal) => {
        [
            unknown::<$t, 0> as *const c_void,
            unknown::<$t, 1> as *const c_void,
            unknown::<$t, 2> as *const c_void,
            unknown::<$t, 3> as *const c_void,
            unknown::<$t, 4> as *const c_void,
            unknown::<$t, 5> as *const c_void,
            unknown::<$t, 6> as *const c_void,
            unknown::<$t, 7> as *const c_void,
            unknown::<$t, 8> as *const c_void,
            unknown::<$t, 9> as *const c_void,
            unknown::<$t, 10> as *const c_void,
            unknown::<$t, 11> as *const c_void,
            unknown::<$t, 12> as *const c_void,
            unknown::<$t, 13> as *const c_void,
            unknown::<$t, 14> as *const c_void,
            unknown::<$t, 15> as *const c_void,
        ]
    };
}
const fn interface_table() -> Vtable {
    let mut slots = unknown_table!(1);
    slots[1] = eye_position as *const c_void;
    slots[2] = resolution as *const c_void;
    slots[3] = physical_size as *const c_void;
    slots[6] = monitor_rect as *const c_void;
    slots[9] = frame_tick as *const c_void;
    slots[10] = available as *const c_void;
    slots[12] = delete_interface as *const c_void;
    Vtable(slots)
}
const fn factory_table() -> Vtable {
    let mut slots = unknown_table!(0);
    slots[0] = create_interface as *const c_void;
    slots[2] = create_weaver as *const c_void;
    Vtable(slots)
}
static INTERFACE: Vtable = interface_table();
static FACTORY_VTABLE: Vtable = factory_table();

extern "C" fn create_factory() -> *mut c_void {
    Box::into_raw(Box::new(Object {
        vtable: FACTORY_VTABLE.0.as_ptr(),
    })) as *mut c_void
}
unsafe extern "C" fn destroy_factory(factory: *mut c_void) {
    if !factory.is_null() {
        drop(Box::from_raw(factory as *mut Object));
    }
}

/// The wrapper's constructor sub_142282E60's detour: the wrapper handed the plugin's SDK.
///
/// # Safety
/// Called by the engine in place of the constructor, with its argument.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_wrapper_ctor(wrapper: *mut u8) -> *mut u8 {
    let original: unsafe extern "C" fn(*mut u8) -> *mut u8 =
        core::mem::transmute(cvr_engine_stereo_wrapper_ctor_original);
    let result = original(wrapper);
    if wrapper.is_null() {
        return result;
    }
    // An SDK found through the registry (Samsung's, or the old stand-in) is let go: the headset's stereo is the
    // port's, never a monitor's.
    if *wrapper.add(LOADED) != 0 {
        let destroy = core::ptr::read_unaligned(wrapper.add(DESTROY) as *const usize);
        if destroy != 0 {
            let destroy: unsafe extern "C" fn(*mut c_void) = core::mem::transmute(destroy);
            destroy(core::ptr::read_unaligned(
                wrapper.add(FACTORY) as *const *mut c_void
            ));
        }
        log("[engine-stereo-sdk] an Odyssey 3D SDK from the registry was loaded; replaced by the plugin's\n");
    }
    core::ptr::write_unaligned(
        wrapper.add(CREATE) as *mut usize,
        create_factory as *const () as usize,
    );
    core::ptr::write_unaligned(
        wrapper.add(DESTROY) as *mut usize,
        destroy_factory as *const () as usize,
    );
    core::ptr::write_unaligned(wrapper.add(FACTORY) as *mut *mut c_void, create_factory());
    *wrapper.add(LOADED) = 1;
    write_cvars(ARMED.load(Relaxed));
    log(&format!(
        "[engine-stereo-sdk] Odyssey 3D wrapper {:016X} given the plugin's SDK; stereo from the main menu on\n",
        wrapper as usize
    ));
    result
}

/// The main menu has been shown (engine stereo only: the C++ forwarder checks it): stereo on from now on.
///
/// # Safety
/// Called from the main menu controller's native.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_sdk_note_main_menu() {
    CyberpunkVR_EngineStereoMainMenus.fetch_add(1, Relaxed);
    if ARMED.swap(true, Relaxed) {
        return;
    }
    CyberpunkVR_EngineStereoArmed.store(1, Relaxed);
    write_cvars(true);
    log("[engine-stereo-sdk] main menu shown: the engine renders both eyes from now on\n");
}
/// A main menu controller uninitialised.
///
/// # Safety
/// Called from the main menu controller's native.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_sdk_note_main_menu_closed() {
    let menus = &CyberpunkVR_EngineStereoMainMenus;
    let mut open = menus.load(Relaxed);
    while open > 0 {
        match menus.compare_exchange_weak(open, open - 1, Relaxed, Relaxed) {
            Ok(_) => break,
            Err(now) => open = now,
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_engine_stereo_sdk_armed() -> bool {
    ARMED.load(Relaxed)
}
/// Main menu controllers up now.
///
/// # Safety
/// Reads the plugin's exported counter.
#[no_mangle]
pub unsafe extern "C" fn cvr_engine_stereo_sdk_main_menus_up() -> i32 {
    CyberpunkVR_EngineStereoMainMenus.load(Relaxed)
}
