//! Which of the engine's views is recording on this thread (formerly src/Hooks/ViewKey.cpp): the frame-graph node
//! dispatcher (sub_140332F90) is entered with a work context whose view ctx carries the view's identity hash (0 MAIN).
//! The stereo module hooks the same dispatcher and exports the identity itself; when it is loaded this hook is skipped
//! and the answers come from it (src/Hooks/ViewKey.cpp keeps the exports and the registration).
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, Ordering};
use cvr_engine::hooks::{install_chain, Detour};

extern "C" {
    static CyberpunkVR_StereoModuleLoaded: i32;
    static mut CyberpunkVR_DebugViewKeyMainNodes: u64;
    static mut CyberpunkVR_DebugViewKeyOtherNodes: u64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    fn CyberpunkVR_IsMainViewActive() -> i32;
}

const NODE_DISPATCH_RVA: usize = 0x332F90;
type NodeDispatch = unsafe extern "C" fn(*mut usize, *mut u8, *mut c_void) -> u8;
static mut ORIG_NODE_DISPATCH: *mut c_void = core::ptr::null_mut();
static OBSERVED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static VIEW_KEY: Cell<(bool, u64)> = const { Cell::new((false, 0)) };
}

unsafe extern "C" fn view_key_dispatch(node: *mut usize, work_context: *mut u8, args: *mut c_void) -> u8 {
    let (mut known, mut key) = (false, 0u64);
    if !work_context.is_null() {
        if let Some(ctx) = cvr_engine::code::read::<usize>(work_context as usize + 0x18) {
            if ctx != 0 {
                if let Some(k) = cvr_engine::code::read::<u64>(ctx + 0x28) {
                    key = k;
                    known = true;
                }
            }
        }
    }
    // Saved and restored: the dispatcher re-enters itself (one nested dispatch per pass), so a nested node must not
    // leave the parent's view mis-tagged.
    let previous = VIEW_KEY.with(|v| v.replace((known, key)));
    if known {
        OBSERVED.store(true, Ordering::Relaxed);
        if CyberpunkVR_RuntimeDiagnostics != 0 {
            if key == 0 {
                CyberpunkVR_DebugViewKeyMainNodes += 1;
            } else {
                CyberpunkVR_DebugViewKeyOtherNodes += 1;
            }
        }
    }
    let r = core::mem::transmute::<*mut c_void, NodeDispatch>(ORIG_NODE_DISPATCH)(node, work_context, args);
    VIEW_KEY.with(|v| v.set(previous));
    r
}

/// The site is the stereo module's when it is loaded (the registry then reports "skipped").
#[no_mangle]
pub extern "C" fn cvr_hook_view_key_wanted() -> bool {
    // SAFETY: a plain read of the C++ half's flag.
    unsafe { CyberpunkVR_StereoModuleLoaded == 0 }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_view_key_install() -> bool {
    if CyberpunkVR_StereoModuleLoaded != 0 {
        return true;
    }
    let detours = [Detour {
        name: "frame-graph node dispatch sub_140332F90",
        target: NODE_DISPATCH_RVA,
        absolute: false,
        detour: view_key_dispatch as *const c_void,
        original: core::ptr::addr_of_mut!(ORIG_NODE_DISPATCH),
        expect: None,
    }];
    install_chain("view-key", &detours).is_ok()
}
/// 1 while a node of the MAIN view is recording on this thread.
#[no_mangle]
pub unsafe extern "C" fn cvr_view_key_main_recording() -> i32 {
    if CyberpunkVR_StereoModuleLoaded != 0 {
        return CyberpunkVR_IsMainViewActive();
    }
    VIEW_KEY.with(|v| matches!(v.get(), (true, 0))) as i32
}
/// 0 until the dispatcher hook is in and has seen view-carrying nodes.
#[no_mangle]
pub unsafe extern "C" fn cvr_view_key_hook_active() -> i32 {
    if CyberpunkVR_StereoModuleLoaded != 0 {
        return 1;
    }
    (!ORIG_NODE_DISPATCH.is_null() && OBSERVED.load(Ordering::Relaxed)) as i32
}
