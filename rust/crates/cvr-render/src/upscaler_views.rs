//! One temporal-upscaler history per eye for the upscalers that keep a single engine-wide context (formerly
//! src/Stereo/UpscalerViews.cpp; the file keeps the exported switch and counters, the FidelityFX providers detour's
//! registration, and the services that read the node dispatcher's view side and hand framegen its inputs).
//!
//! DLSS gets its second history from a second Streamline viewport; FSR 2 is the engine's own compute passes with
//! per-view history. The other three keep ONE context in a renderer-wide manager, and both eyes dispatch into it:
//!
//!   FSR 3.0   ffx_fsr3_x64.dll          FfxFsr3Context at manager+44, manager = renderer+18032
//!   FSR 3.1+  FidelityFX API            ffxContext of the manager at renderer+18024 (FSR 4 goes here too)
//!   XeSS      libxess.dll               xess_context_handle_t at manager+32
//!
//! With one context each eye's frame is accumulated onto the other eye's history -- an IPD-wide ghost on everything
//! near. So a second context is made next to the engine's, inside the engine's own create call from the description
//! it passes, and the second eye's dispatches (the node dispatcher's view side 1) are pointed at it; destroy and every
//! setting call follow the engine's context. All of these run under the engine's manager lock. Framegen reads its
//! depth and motion inputs here as well, before the library changes their state.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, AtomicU64, AtomicUsize, Ordering::*};
use std::sync::Mutex;

#[allow(non_upper_case_globals)]
extern "C" {
    /// 1 = the second eye uses its own context (default); 0 = both eyes share the engine's.
    static CyberpunkVR_UpscalerPerEye: AtomicI32;
    /// Per library (0 FSR 3.0, 1 FFX API, 2 XeSS): second contexts created, first-eye dispatches, second-eye
    /// dispatches given their own context, failures.
    static CyberpunkVR_UpscalerPerEyeStats: [[AtomicU64; 4]; 3];
    fn CyberpunkVR_GetGameDevice() -> *mut c_void;
    /// Framegen's capture of the upscaler's depth and motion inputs (rust/crates/cvr-render/src/framegen_inputs.rs).
    fn cvr_framegen_capture_upscaler_inputs(list: *mut c_void);
}
#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn GetModuleFileNameW(module: *mut c_void, name: *mut u16, size: u32) -> u32;
    fn GetFileAttributesW(name: *const u16) -> u32;
}
extern "system" {
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
    fn MH_RemoveHook(target: *mut c_void) -> i32;
}

const FSR3: usize = 0;
const FFX_API: usize = 1;
const XESS: usize = 2;
const CREATED: usize = 0;
const FIRST_EYE: usize = 1;
const SECOND_EYE: usize = 2;
const FAILED: usize = 3;

fn count(library: usize, stat: usize) {
    // SAFETY: the plugin's exported counters.
    unsafe { CyberpunkVR_UpscalerPerEyeStats[library][stat].fetch_add(1, Relaxed) };
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
unsafe fn second_eye() -> bool {
    CyberpunkVR_UpscalerPerEye.load(Relaxed) != 0 && crate::render_tls::view_side() == 1
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
unsafe fn module(name: &str) -> *mut c_void {
    GetModuleHandleW(wide(name).as_ptr())
}
/// The export `name` of `module` hooked; `original` receives MinHook's trampoline (null again when enabling fails).
unsafe fn hook_export(
    module: *mut c_void,
    name: &core::ffi::CStr,
    detour: *const (),
    original: *mut usize,
) -> bool {
    let target = if module.is_null() {
        core::ptr::null_mut()
    } else {
        GetProcAddress(module, name.as_ptr())
    };
    if target.is_null() {
        return false;
    }
    if MH_CreateHook(target, detour as *mut c_void, original as *mut *mut c_void) != 0 {
        return false;
    }
    if MH_EnableHook(target) != 0 {
        MH_RemoveHook(target);
        *original = 0;
        return false;
    }
    true
}
unsafe fn first_member(description: *const c_void, at: usize) -> *mut c_void {
    core::ptr::read_unaligned((description as *const u8).add(at) as *const *mut c_void)
}

// ---- FSR 3.0 ------------------------------------------------------------------------------------
// FidelityFX SDK 1.0 as the game ships it. The context description is 0x208 bytes: flags and three sizes, then the
// shared-resource, upscaling and frame-interpolation backend interfaces of 0x98 bytes each (sub_141D4EE90 fills them
// at +0x20, +0xB8 and +0x150), the message callback and the format. The engine builds all three interfaces on ONE
// scratch buffer sized for four backend contexts; the second context gets its own scratch and interfaces instead of
// the engine's spare slots, so it cannot run the engine out of them.
mod fsr3 {
    use super::*;
    pub const DESCRIPTION_BYTES: usize = 0x208;
    pub const MAX_CONTEXTS: usize = 4;
    pub const INTERFACES: [usize; 3] = [0x20, 0xB8, 0x150];
    /// The engine reserves 1,639,012 bytes for its FfxFsr3Context (manager+44 to manager+1639056).
    pub const CONTEXT_BYTES: usize = 0x200000;
    pub type Create = unsafe extern "C" fn(*mut c_void, *mut c_void) -> i32;
    pub type Destroy = unsafe extern "C" fn(*mut c_void) -> i32;
    pub type Dispatch = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;
    pub type ScratchSize = unsafe extern "C" fn(usize) -> usize;
    pub type Interface =
        unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, usize, usize) -> i32;
    pub static mut ORIGINAL_CREATE: usize = 0;
    pub static mut ORIGINAL_DESTROY: usize = 0;
    pub static mut ORIGINAL_DISPATCH: usize = 0;
    pub static SCRATCH_SIZE: AtomicUsize = AtomicUsize::new(0);
    pub static MAKE_INTERFACE: AtomicUsize = AtomicUsize::new(0);
    pub static BACKEND: AtomicUsize = AtomicUsize::new(0);
    pub struct Own {
        pub context: Vec<u32>,
        pub scratch: Vec<u8>,
    }
    pub static OWN: Mutex<Own> = Mutex::new(Own {
        context: Vec::new(),
        scratch: Vec::new(),
    });
    pub static ENGINE: AtomicUsize = AtomicUsize::new(0);
    pub static SECOND: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    pub static DONE: AtomicBool = AtomicBool::new(false);

    pub unsafe fn create() -> Create {
        core::mem::transmute(ORIGINAL_CREATE)
    }
    pub unsafe fn destroy() -> Destroy {
        core::mem::transmute(ORIGINAL_DESTROY)
    }
    pub unsafe fn dispatch() -> Dispatch {
        core::mem::transmute(ORIGINAL_DISPATCH)
    }

    unsafe fn in_backend(p: *mut c_void) -> bool {
        let base = BACKEND.load(Relaxed);
        if base == 0 {
            return false;
        }
        let lfanew = core::ptr::read_unaligned((base + 0x3C) as *const i32);
        let size = core::ptr::read_unaligned((base as isize + lfanew as isize + 0x50) as *const u32)
            as usize;
        let at = p as usize;
        at >= base && at < base + size
    }
    /// Under OWN's lock.
    unsafe fn release(own: &mut Own) {
        let second = SECOND.swap(core::ptr::null_mut(), SeqCst);
        if !second.is_null() {
            destroy()(second);
        }
        own.context = Vec::new();
        own.scratch = Vec::new();
        ENGINE.store(0, SeqCst);
    }
    fn zeroed<T: Default + Clone>(n: usize) -> Option<Vec<T>> {
        let mut v = Vec::new();
        v.try_reserve_exact(n).ok()?;
        v.resize(n, T::default());
        Some(v)
    }

    pub unsafe extern "C" fn hook_create(
        engine_context: *mut c_void,
        description: *mut c_void,
    ) -> i32 {
        let result = create()(engine_context, description);
        let (scratch_size, make_interface) =
            (SCRATCH_SIZE.load(Relaxed), MAKE_INTERFACE.load(Relaxed));
        if result != 0 || description.is_null() || scratch_size == 0 || make_interface == 0 {
            return result;
        }
        let scratch_size: ScratchSize = core::mem::transmute(scratch_size);
        let make_interface: Interface = core::mem::transmute(make_interface);
        let mut own = OWN.lock().unwrap_or_else(|p| p.into_inner());
        release(&mut own);
        // Only a layout this was written against: every interface starts with a function of the game's
        // ffx_backend_dx12_x64.dll.
        for at in INTERFACES {
            if !in_backend(first_member(description, at)) {
                log("[upscaler] FSR 3.0: unexpected context description, the second eye keeps the shared history\n");
                count(FSR3, FAILED);
                return result;
            }
        }
        let device = CyberpunkVR_GetGameDevice();
        let bytes = scratch_size(MAX_CONTEXTS);
        let mut copy = [0u8; DESCRIPTION_BYTES];
        core::ptr::copy_nonoverlapping(
            description as *const u8,
            copy.as_mut_ptr(),
            DESCRIPTION_BYTES,
        );
        let scratch = zeroed::<u8>(bytes);
        let context = zeroed::<u32>(CONTEXT_BYTES / 4);
        let mut ok = !device.is_null() && scratch.is_some() && context.is_some();
        let (mut scratch, mut context) = (scratch.unwrap_or_default(), context.unwrap_or_default());
        for at in INTERFACES {
            ok = ok
                && make_interface(
                    copy.as_mut_ptr().add(at).cast(),
                    device,
                    scratch.as_mut_ptr().cast(),
                    bytes,
                    MAX_CONTEXTS,
                ) == 0;
        }
        if ok && create()(context.as_mut_ptr().cast(), copy.as_mut_ptr().cast()) == 0 {
            ENGINE.store(engine_context as usize, SeqCst);
            SECOND.store(context.as_mut_ptr().cast(), Release);
            own.context = context;
            own.scratch = scratch;
            count(FSR3, CREATED);
            log(&format!(
                "[upscaler] FSR 3.0: second-eye context created (scratch {bytes} bytes)\n"
            ));
        } else {
            count(FSR3, FAILED);
            log("[upscaler] FSR 3.0: second-eye context failed, the second eye keeps the shared history\n");
        }
        result
    }
    pub unsafe extern "C" fn hook_destroy(engine_context: *mut c_void) -> i32 {
        {
            let mut own = OWN.lock().unwrap_or_else(|p| p.into_inner());
            if engine_context as usize == ENGINE.load(SeqCst) {
                release(&mut own);
            }
        }
        destroy()(engine_context)
    }
    pub unsafe extern "C" fn hook_dispatch(
        engine_context: *mut c_void,
        description: *const c_void,
    ) -> i32 {
        // FfxCommandList is the first member.
        let list = if description.is_null() {
            core::ptr::null_mut()
        } else {
            first_member(description, 0)
        };
        if !list.is_null() {
            cvr_framegen_capture_upscaler_inputs(list);
        }
        let engine = ENGINE.load(Relaxed);
        if engine_context as usize == engine && second_eye() {
            let own = SECOND.load(Acquire);
            if !own.is_null() {
                count(FSR3, SECOND_EYE);
                return dispatch()(own, description);
            }
        }
        if engine_context as usize == engine {
            count(FSR3, FIRST_EYE);
        }
        dispatch()(engine_context, description)
    }

    pub unsafe fn install() {
        if DONE.load(Relaxed) {
            return;
        }
        let library = module("ffx_fsr3_x64.dll");
        let backend = module("ffx_backend_dx12_x64.dll");
        BACKEND.store(backend as usize, Relaxed);
        if library.is_null() || backend.is_null() {
            return;
        }
        DONE.store(true, Relaxed);
        SCRATCH_SIZE.store(
            GetProcAddress(backend, c"ffxGetScratchMemorySizeDX12".as_ptr()) as usize,
            Relaxed,
        );
        MAKE_INTERFACE.store(
            GetProcAddress(backend, c"ffxGetInterfaceDX12".as_ptr()) as usize,
            Relaxed,
        );
        let ok = SCRATCH_SIZE.load(Relaxed) != 0
            && MAKE_INTERFACE.load(Relaxed) != 0
            && hook_export(
                library,
                c"ffxFsr3ContextCreate",
                hook_create as *const (),
                &raw mut ORIGINAL_CREATE,
            )
            && hook_export(
                library,
                c"ffxFsr3ContextDestroy",
                hook_destroy as *const (),
                &raw mut ORIGINAL_DESTROY,
            )
            && hook_export(
                library,
                c"ffxFsr3ContextDispatchUpscale",
                hook_dispatch as *const (),
                &raw mut ORIGINAL_DISPATCH,
            );
        if !ok {
            count(FSR3, FAILED);
        }
        log(&format!(
            "[upscaler] FSR 3.0 per-eye hooks {}\n",
            if ok { "installed" } else { "FAILED" }
        ));
    }
}

// ---- FSR 3.1 / FSR 4 (FidelityFX API) -----------------------------------------------------------
// Descriptions are chains of {uint64 type, next}. Only the upscaler is doubled; frame generation and the swap chain
// stay single. ffxDispatch's upscale description has the command list right after the header.
mod ffx_api {
    use super::*;
    pub const CREATE_UPSCALE: u64 = 0x0001_0000;
    pub const DISPATCH_UPSCALE: u64 = 0x0001_0001;
    pub const REACTIVE_MASK: u64 = 0x0001_0006;
    pub type Create = unsafe extern "C" fn(*mut *mut c_void, *mut u64, *const c_void) -> u32;
    pub type Destroy = unsafe extern "C" fn(*mut *mut c_void, *const c_void) -> u32;
    pub type Configure = unsafe extern "C" fn(*mut *mut c_void, *const u64) -> u32;
    pub type Dispatch = unsafe extern "C" fn(*mut *mut c_void, *const u64) -> u32;
    pub static mut ORIGINAL_CREATE: usize = 0;
    pub static mut ORIGINAL_DESTROY: usize = 0;
    pub static mut ORIGINAL_CONFIGURE: usize = 0;
    pub static mut ORIGINAL_DISPATCH: usize = 0;
    pub static LOCK: Mutex<()> = Mutex::new(());
    /// The engine's upscale context handle.
    pub static ENGINE: AtomicUsize = AtomicUsize::new(0);
    /// Ours (under LOCK; its address is what the library is handed).
    pub static mut SECOND: *mut c_void = core::ptr::null_mut();
    pub static SECOND_SLOT: AtomicPtr<*mut c_void> = AtomicPtr::new(core::ptr::null_mut());
    pub static INSTALLED: AtomicBool = AtomicBool::new(false);

    unsafe fn create() -> Create {
        core::mem::transmute(ORIGINAL_CREATE)
    }
    unsafe fn destroy() -> Destroy {
        core::mem::transmute(ORIGINAL_DESTROY)
    }
    unsafe fn configure() -> Configure {
        core::mem::transmute(ORIGINAL_CONFIGURE)
    }
    unsafe fn dispatch() -> Dispatch {
        core::mem::transmute(ORIGINAL_DISPATCH)
    }

    pub unsafe extern "C" fn hook_create(
        context: *mut *mut c_void,
        description: *mut u64,
        allocation: *const c_void,
    ) -> u32 {
        let result = create()(context, description, allocation);
        if result != 0
            || context.is_null()
            || (*context).is_null()
            || description.is_null()
            || *description != CREATE_UPSCALE
        {
            return result;
        }
        let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if !SECOND.is_null() {
            SECOND_SLOT.store(core::ptr::null_mut(), SeqCst);
            destroy()(&raw mut SECOND, allocation);
            SECOND = core::ptr::null_mut();
        }
        let mut own: *mut c_void = core::ptr::null_mut();
        ENGINE.store(*context as usize, SeqCst);
        if create()(&mut own, description, allocation) == 0 && !own.is_null() {
            SECOND = own;
            SECOND_SLOT.store(&raw mut SECOND, Release);
            count(FFX_API, CREATED);
            log("[upscaler] FidelityFX API: second-eye upscale context created\n");
        } else {
            count(FFX_API, FAILED);
            log("[upscaler] FidelityFX API: second-eye upscale context failed, the second eye keeps the shared history\n");
        }
        result
    }
    pub unsafe extern "C" fn hook_destroy(
        context: *mut *mut c_void,
        allocation: *const c_void,
    ) -> u32 {
        {
            let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            if !context.is_null()
                && !(*context).is_null()
                && *context as usize == ENGINE.load(SeqCst)
            {
                SECOND_SLOT.store(core::ptr::null_mut(), SeqCst);
                if !SECOND.is_null() {
                    destroy()(&raw mut SECOND, allocation);
                }
                SECOND = core::ptr::null_mut();
                ENGINE.store(0, SeqCst);
            }
        }
        destroy()(context, allocation)
    }
    pub unsafe extern "C" fn hook_configure(
        context: *mut *mut c_void,
        description: *const u64,
    ) -> u32 {
        let result = configure()(context, description);
        if !context.is_null() && !(*context).is_null() && *context as usize == ENGINE.load(SeqCst) {
            let own = SECOND_SLOT.load(Acquire);
            if !own.is_null() {
                configure()(own, description);
            }
        }
        result
    }
    pub unsafe extern "C" fn hook_dispatch(
        context: *mut *mut c_void,
        description: *const u64,
    ) -> u32 {
        let kind = if description.is_null() {
            0
        } else {
            *description
        };
        let upscale = kind == DISPATCH_UPSCALE || kind == REACTIVE_MASK;
        if !upscale
            || context.is_null()
            || (*context).is_null()
            || *context as usize != ENGINE.load(SeqCst)
        {
            return dispatch()(context, description);
        }
        if kind == DISPATCH_UPSCALE {
            let list = first_member(description.cast(), 16); // after the {type, next} header
            if !list.is_null() {
                cvr_framegen_capture_upscaler_inputs(list);
            }
        }
        if second_eye() {
            let own = SECOND_SLOT.load(Acquire);
            if !own.is_null() {
                if kind == DISPATCH_UPSCALE {
                    count(FFX_API, SECOND_EYE);
                }
                return dispatch()(own, description);
            }
        }
        if kind == DISPATCH_UPSCALE {
            count(FFX_API, FIRST_EYE);
        }
        dispatch()(context, description)
    }

    pub unsafe fn install(library: *mut c_void) {
        if library.is_null() || INSTALLED.swap(true, SeqCst) {
            return;
        }
        let ok = hook_export(
            library,
            c"ffxCreateContext",
            hook_create as *const (),
            &raw mut ORIGINAL_CREATE,
        ) && hook_export(
            library,
            c"ffxDestroyContext",
            hook_destroy as *const (),
            &raw mut ORIGINAL_DESTROY,
        ) && hook_export(
            library,
            c"ffxConfigure",
            hook_configure as *const (),
            &raw mut ORIGINAL_CONFIGURE,
        ) && hook_export(
            library,
            c"ffxDispatch",
            hook_dispatch as *const (),
            &raw mut ORIGINAL_DISPATCH,
        );
        if !ok {
            count(FFX_API, FAILED);
        }
        log(&format!(
            "[upscaler] FidelityFX API per-eye hooks {}\n",
            if ok { "installed" } else { "FAILED" }
        ));
    }
}

// ---- XeSS ---------------------------------------------------------------------------------------
// The engine creates the context, sets four context parameters, then on every mode change sets the velocity scale
// and calls xessD3D12Init with library-owned heaps (no heap of the engine is named, so two contexts cannot alias each
// other's memory). Each of those is repeated on the second context.
mod xess {
    use super::*;
    pub type Create = unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32;
    pub type Destroy = unsafe extern "C" fn(*mut c_void) -> i32;
    pub type Init = unsafe extern "C" fn(*mut c_void, *const c_void) -> i32;
    pub type Execute = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void) -> i32;
    pub type VelocityScale = unsafe extern "C" fn(*mut c_void, f32, f32) -> i32;
    pub type ParameterF = unsafe extern "C" fn(*mut c_void, u32, f32) -> i32;
    pub static mut ORIGINAL_CREATE: usize = 0;
    pub static mut ORIGINAL_DESTROY: usize = 0;
    pub static mut ORIGINAL_INIT: usize = 0;
    pub static mut ORIGINAL_EXECUTE: usize = 0;
    pub static mut ORIGINAL_VELOCITY_SCALE: usize = 0;
    pub static mut ORIGINAL_PARAMETER_F: usize = 0;
    pub static LOCK: Mutex<()> = Mutex::new(());
    pub static ENGINE: AtomicUsize = AtomicUsize::new(0);
    pub static SECOND: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    pub static DONE: AtomicBool = AtomicBool::new(false);

    unsafe fn destroy() -> Destroy {
        core::mem::transmute(ORIGINAL_DESTROY)
    }
    unsafe fn mirror(context: *mut c_void) -> *mut c_void {
        if !context.is_null() && context as usize == ENGINE.load(SeqCst) {
            SECOND.load(Acquire)
        } else {
            core::ptr::null_mut()
        }
    }

    pub unsafe extern "C" fn hook_create(device: *mut c_void, context: *mut *mut c_void) -> i32 {
        let create: Create = core::mem::transmute(ORIGINAL_CREATE);
        let result = create(device, context);
        if result != 0 || context.is_null() || (*context).is_null() {
            return result;
        }
        let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let old = SECOND.swap(core::ptr::null_mut(), SeqCst);
        if !old.is_null() {
            destroy()(old);
        }
        let mut own: *mut c_void = core::ptr::null_mut();
        ENGINE.store(*context as usize, SeqCst);
        if create(device, &mut own) == 0 && !own.is_null() {
            SECOND.store(own, Release);
            count(XESS, CREATED);
            log("[upscaler] XeSS: second-eye context created\n");
        } else {
            count(XESS, FAILED);
            log("[upscaler] XeSS: second-eye context failed, the second eye keeps the shared history\n");
        }
        result
    }
    pub unsafe extern "C" fn hook_destroy(context: *mut c_void) -> i32 {
        {
            let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            if !context.is_null() && context as usize == ENGINE.load(SeqCst) {
                let own = SECOND.swap(core::ptr::null_mut(), SeqCst);
                if !own.is_null() {
                    destroy()(own);
                }
                ENGINE.store(0, SeqCst);
            }
        }
        destroy()(context)
    }
    pub unsafe extern "C" fn hook_init(context: *mut c_void, parameters: *const c_void) -> i32 {
        let init: Init = core::mem::transmute(ORIGINAL_INIT);
        let result = init(context, parameters);
        let own = mirror(context);
        if !own.is_null() && init(own, parameters) != 0 {
            count(XESS, FAILED);
            log("[upscaler] XeSS: second-eye init failed, the second eye keeps the shared history\n");
            let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let failed = SECOND.swap(core::ptr::null_mut(), SeqCst);
            if !failed.is_null() {
                destroy()(failed);
            }
        }
        result
    }
    pub unsafe extern "C" fn hook_velocity_scale(context: *mut c_void, x: f32, y: f32) -> i32 {
        let set: VelocityScale = core::mem::transmute(ORIGINAL_VELOCITY_SCALE);
        let result = set(context, x, y);
        let own = mirror(context);
        if !own.is_null() {
            set(own, x, y);
        }
        result
    }
    pub unsafe extern "C" fn hook_parameter_f(
        context: *mut c_void,
        parameter: u32,
        value: f32,
    ) -> i32 {
        let set: ParameterF = core::mem::transmute(ORIGINAL_PARAMETER_F);
        let result = set(context, parameter, value);
        let own = mirror(context);
        if !own.is_null() {
            set(own, parameter, value);
        }
        result
    }
    pub unsafe extern "C" fn hook_execute(
        context: *mut c_void,
        list: *mut c_void,
        parameters: *const c_void,
    ) -> i32 {
        let execute: Execute = core::mem::transmute(ORIGINAL_EXECUTE);
        if !list.is_null() {
            cvr_framegen_capture_upscaler_inputs(list);
        }
        let engine = ENGINE.load(SeqCst);
        if !context.is_null() && context as usize == engine && second_eye() {
            let own = SECOND.load(Acquire);
            if !own.is_null() {
                count(XESS, SECOND_EYE);
                return execute(own, list, parameters);
            }
        }
        if !context.is_null() && context as usize == engine {
            count(XESS, FIRST_EYE);
        }
        execute(context, list, parameters)
    }

    pub unsafe fn install() {
        if DONE.load(Relaxed) {
            return;
        }
        let library = module("libxess.dll");
        if library.is_null() {
            return;
        }
        DONE.store(true, Relaxed);
        let ok = hook_export(
            library,
            c"xessD3D12CreateContext",
            hook_create as *const (),
            &raw mut ORIGINAL_CREATE,
        ) && hook_export(
            library,
            c"xessDestroyContext",
            hook_destroy as *const (),
            &raw mut ORIGINAL_DESTROY,
        ) && hook_export(
            library,
            c"xessD3D12Init",
            hook_init as *const (),
            &raw mut ORIGINAL_INIT,
        ) && hook_export(
            library,
            c"xessD3D12Execute",
            hook_execute as *const (),
            &raw mut ORIGINAL_EXECUTE,
        ) && hook_export(
            library,
            c"xessSetVelocityScale",
            hook_velocity_scale as *const (),
            &raw mut ORIGINAL_VELOCITY_SCALE,
        ) && hook_export(
            library,
            c"xessSetContextParameterF",
            hook_parameter_f as *const (),
            &raw mut ORIGINAL_PARAMETER_F,
        );
        if !ok {
            count(XESS, FAILED);
        }
        log(&format!(
            "[upscaler] XeSS per-eye hooks {}\n",
            if ok { "installed" } else { "FAILED" }
        ));
    }
}

/// Hooks the libraries that are already loaded. Idempotent; called with the early engine detours.
///
/// # Safety
/// Called by the stereo module's init, with MinHook initialised.
#[no_mangle]
pub unsafe extern "C" fn cvr_upscaler_views_install() {
    fsr3::install();
    xess::install();
    // 2.5 links the FidelityFX loader statically, so it is loaded before any plugin: its exports are the ones the game
    // calls (2.31 loaded amd_fidelityfx_dx12 itself).
    let mut ffx = module("amd_fidelityfx_loader_dx12.dll");
    if ffx.is_null() {
        ffx = module("amd_fidelityfx_dx12.dll");
    }
    ffx_api::install(ffx);
}

/// A test switch: a file of this name next to the game executable.
unsafe fn test_marker(name: &str) -> bool {
    let mut path = [0u16; 260];
    let n = GetModuleFileNameW(core::ptr::null_mut(), path.as_mut_ptr(), 260) as usize;
    if n == 0 {
        return false;
    }
    let path = String::from_utf16_lossy(&path[..n]);
    let folder = path.rfind('\\').map_or(path.as_str(), |cut| &path[..=cut]);
    GetFileAttributesW(wide(&format!("{folder}{name}")).as_ptr()) != u32::MAX
}

/// The trampoline MinHook writes (src/Stereo/UpscalerViews.cpp's CVR_DETOUR line names it).
#[no_mangle]
pub static mut cvr_ffx_api_providers_original: usize = 0;

unsafe extern "C" fn note_providers(manager: *mut c_void, _: *mut c_void, _: *mut c_void) -> bool {
    let m = manager as *mut i64;
    log(&format!(
        "[upscaler] FidelityFX API upscalers: 2.x={:X} 3.x={:X} 4.x={:X} default={:X}\n",
        *m.add(18) as u64,
        *m.add(19) as u64,
        *m.add(20) as u64,
        *m.add(21) as u64
    ));
    if test_marker("cvr_simulate_fsr4.txt") && *m.add(20) == -1 && *m.add(19) != -1 {
        *m.add(20) = *m.add(19);
        log("[upscaler] TEST: FSR 4 simulated with the FSR 3.1 provider (cvr_simulate_fsr4.txt)\n");
    }
    true
}

/// sub_142287A20's detour: the FidelityFX API's upscaler providers (+0x90 "2.x", +0x98 "3.x", +0xA0 "4.x", -1 when
/// absent) logged, and with cvr_simulate_fsr4.txt (TEST ONLY) the 3.x provider put in the 4.x slot, so any GPU runs
/// the FSR 4 path with FSR 3.1 shaders.
///
/// # Safety
/// Called by the engine in place of the function, with its argument.
#[no_mangle]
pub unsafe extern "C" fn cvr_ffx_api_providers(manager: *mut i64) -> i64 {
    let original: unsafe extern "C" fn(*mut i64) -> i64 =
        core::mem::transmute(cvr_ffx_api_providers_original);
    let result = original(manager);
    let _ = cvr_engine::code::guarded_call3(
        note_providers as *const () as usize,
        manager.cast(),
        core::ptr::null_mut(),
        core::ptr::null_mut(),
    );
    result
}
