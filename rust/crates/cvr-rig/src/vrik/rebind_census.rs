//! RebindCensus (formerly src/Hooks/RebindCensus.cpp): how many times per frame the engine re-establishes world
//! transforms from their bindings, and nothing else.
//!
//! sub_140329460 walks a component's bindings, asks each provider for its slot transform and calls SetWorldTransform
//! for it -- the very function the camera write detours. Everything the camera inherits, heading included, arrives
//! through this loop. The census answers one question: does it run once per rendered frame, or more? Twice would place
//! the skeleton with two different world transforms inside one image -- what a body double on a fast mouse turn looks
//! like. Cost: one increment on a call the engine already makes (diagnostics only); the rate is printed by the census
//! in OpenXRPresent and the counter is live-readable.
use core::ffi::c_void;
use cvr_engine::hooks::{install_chain, Detour};

extern "C" {
    static CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_DebugRebindLoopCalls: u64;
}

/// sub_140329460: the binding -> SetWorldTransform loop (identified by call site).
const REBIND_LOOP_RVA: usize = 0x329460;

type RebindLoopFn = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
static mut ORIG_REBIND_LOOP: *mut c_void = core::ptr::null_mut();

unsafe extern "C" fn rebind_loop(a1: *mut c_void, a2: *mut c_void, a3: *mut c_void, a4: *mut c_void) -> *mut c_void {
    if CyberpunkVR_RuntimeDiagnostics != 0 {
        // SAFETY: the C++ half's export, a plain counter written with a locked increment as InterlockedIncrement64 did.
        (*core::ptr::addr_of!(CyberpunkVR_DebugRebindLoopCalls).cast::<core::sync::atomic::AtomicU64>()).fetch_add(1, core::sync::atomic::Ordering::SeqCst);
    }
    let original: RebindLoopFn = core::mem::transmute(ORIG_REBIND_LOOP);
    original(a1, a2, a3, a4)
}

/// Installed from src/Hooks/RebindCensus.cpp's registration (Boot, 60).
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_rebind_census_install() -> bool {
    let detours = [Detour {
        name: "rebind loop sub_140329460",
        target: REBIND_LOOP_RVA,
        absolute: false,
        detour: rebind_loop as *const c_void,
        original: core::ptr::addr_of_mut!(ORIG_REBIND_LOOP),
        expect: None,
    }];
    install_chain("rebind-census", &detours).is_ok()
}
