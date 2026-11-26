//! The VR menu's native item tooltips without Codeware's undeclared-library loader (formerly main's
//! src/Natives/MenuTooltip.cpp). A controller's stock SpawnFromExternal searches only its library's declared dependencies:
//! the four tooltip libraries are loaded asynchronously here and declared, ready, as a dependency of the owner library
//! right before the stock spawn. The dependency stays on the owner library: nested tooltip controllers resolve their lazy
//! local modules through it. The engine still owns the widget's creation, initialization and attachment to the parent.
//! Two engine paths are hooked: the library's external spawn (where the dependency is declared, for the spawn this
//! module started) and its external find (behind the same lock, so no find reads the dependencies while one is added).
use super::lobby_rtti::{self as rt, A};
use crate::archive_ext::{rt_resource_load_async, rt_token_failed, rt_token_loaded, rt_token_resource, Token};
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use cvr_engine::hooks::{install_chain, Detour};
use red4ext_abi::cstring::CString;
use red4ext_abi::dynarray::DynArray;
use red4ext_abi::handle::Handle;
use red4ext_abi::resource::resource_path;
use red4ext_abi::stack::StackFrame;
use std::cell::Cell;
use std::sync::{Mutex, RwLock};

/// Address-library hashes: inkWidgetLibraryResource's external spawn and its external library find.
const SPAWN_EXTERNAL_HASH: u32 = 506278179;
const FIND_EXTERNAL_HASH: u32 = 2756653218;
/// inkWidgetLibraryResource::externalLibraries (DynArray<Ref<inkWidgetLibraryResource>>, 0x18 a reference: the path,
/// then the token's SharedPtr).
const EXTERNAL_LIBRARIES: usize = 0x58;

type SpawnFn = unsafe extern "C" fn(*mut c_void, *mut Handle, u64, u64) -> *mut c_void;
type FindFn = unsafe extern "C" fn(*mut c_void, *mut Handle, u64) -> *mut c_void;
static mut ORIGINAL_SPAWN: *mut c_void = core::ptr::null_mut();
static mut ORIGINAL_FIND: *mut c_void = core::ptr::null_mut();
static INSTALLED: AtomicBool = AtomicBool::new(false);

const PATHS: [&str; 4] = [
    "base\\gameplay\\gui\\common\\tooltip\\itemtooltip.inkwidget",
    "base\\gameplay\\gui\\common\\tooltip\\new_itemtooltip.inkwidget",
    "base\\gameplay\\gui\\common\\tooltip\\cyberdecktooltip.inkwidget",
    "base\\gameplay\\gui\\common\\tooltip\\programtooltip.inkwidget",
];
/// The bounded cache: one token a library, held for the process (no controller or widget handles, no arbitrary
/// resources a script argument names).
static LIBRARIES: Mutex<[Token; 4]> = Mutex::new([[0; 2]; 4]);
/// The owner libraries' dependencies: written under the write lock, read under the read lock.
static DEPENDENCIES: RwLock<()> = RwLock::new(());
/// The spawn this module started on this thread: the library's token, its path, the item.
#[derive(Clone, Copy)]
struct Scope {
    token: Token,
    path: u64,
    item: u64,
}
thread_local! {
    static SCOPE: Cell<Option<Scope>> = const { Cell::new(None) };
}
static DECLARED: AtomicUsize = AtomicUsize::new(0);

/// Prepare: -1 failed or unsupported, 0 loading, 1 ready (with its token). No waits, no fetching on the game thread.
fn prepare(path: u64) -> (i32, Token) {
    if !INSTALLED.load(Ordering::Acquire) {
        return (-1, [0; 2]);
    }
    let Some(i) = PATHS.iter().position(|p| resource_path(p) == path) else { return (-1, [0; 2]) };
    let mut libraries = LIBRARIES.lock().unwrap_or_else(|p| p.into_inner());
    let token = &mut libraries[i];
    if token[0] == 0 {
        // SAFETY: the loader writes its SharedPtr into the slot.
        unsafe { rt_resource_load_async(path, token) };
        if token[0] == 0 {
            return (-1, [0; 2]);
        }
    }
    // SAFETY: a live token, held by the cache.
    unsafe {
        if rt_token_failed(token) {
            return (-1, [0; 2]);
        }
        if !rt_token_loaded(token) {
            return (0, [0; 2]);
        }
        if rt_token_resource(token).is_null() {
            return (-1, [0; 2]);
        }
    }
    (1, *token)
}
/// The owner's dependency on the ready library: an entry of the path given the token (its strong count raised), or a
/// new entry. An entry that already holds a token keeps it -- the same path, the same library (the owner declared it,
/// or an earlier spawn here did).
unsafe fn declare_ready_library(owner: *mut c_void, path: u64, token: Token) {
    // SAFETY: the caller's live library resource; its dependency array as the SDK lays it out.
    unsafe {
        let libraries = &mut *((owner as *mut u8).add(EXTERNAL_LIBRARIES) as *mut DynArray<[usize; 3]>);
        let raise = || {
            if token[1] != 0 {
                (*(token[1] as *const core::sync::atomic::AtomicU32)).fetch_add(1, Ordering::SeqCst);
            }
        };
        let entries = libraries.as_slice().as_ptr() as *mut [usize; 3];
        for i in 0..libraries.len() {
            let entry = &mut *entries.add(i);
            if entry[0] as u64 != path {
                continue;
            }
            if entry[1] == 0 {
                raise();
                (entry[1], entry[2]) = (token[0], token[1]);
            }
            return;
        }
        raise();
        libraries.push([path as usize, token[0], token[1]]);
        let n = DECLARED.fetch_add(1, Ordering::Relaxed);
        if n < 4 {
            cvr_host::log!("[menu-tooltip] a tooltip library declared on its owner (from Rust)\n");
        }
    }
}
unsafe extern "C" fn find_external(owner: *mut c_void, out: *mut Handle, path: u64) -> *mut c_void {
    let _read = DEPENDENCIES.read().unwrap_or_else(|p| p.into_inner());
    // SAFETY: the original, with the engine's own arguments.
    unsafe { core::mem::transmute::<*mut c_void, FindFn>(ORIGINAL_FIND)(owner, out, path) }
}
unsafe extern "C" fn spawn_external(owner: *mut c_void, out: *mut Handle, path: u64, item: u64) -> *mut c_void {
    if let Some(scope) = SCOPE.with(Cell::get) {
        // SAFETY: the scope's token is held by the cache.
        let library = unsafe { rt_token_resource(&scope.token) };
        if !owner.is_null() && scope.path == path && scope.item == item && owner != library {
            // A bare SpawnFromLocal on the external resource makes the shell, but its child controllers cannot find
            // itemHeader / itemDetails later: the owner's dependency must exist during the stock setup.
            let _write = DEPENDENCIES.write().unwrap_or_else(|p| p.into_inner());
            // SAFETY: the engine's live owner library.
            unsafe { declare_ready_library(owner, path, scope.token) };
        }
    }
    // the stock external spawn, with the originating library's association for lazy modules
    // SAFETY: the original, with the engine's own arguments.
    unsafe { core::mem::transmute::<*mut c_void, SpawnFn>(ORIGINAL_SPAWN)(owner, out, path, item) }
}
/// The hook (registered at Boot, order 84, in cvr-render's hook table, as main's CVR_HOOK("MenuTooltip")).
#[no_mangle]
pub extern "C" fn cvr_hook_menu_tooltip_install() -> bool {
    if !cvr_engine::game_build::supported() {
        return false;
    }
    let (Some(spawn), Some(find)) = (red4ext_abi::reloc::resolve(SPAWN_EXTERNAL_HASH), red4ext_abi::reloc::resolve(FIND_EXTERNAL_HASH)) else {
        cvr_host::log!("[menu-tooltip] the widget library's external spawn or find did not resolve\n");
        return false;
    };
    let detours = [
        Detour { name: "inkWidgetLibraryResource external find", target: find, absolute: true, detour: find_external as *const c_void, original: core::ptr::addr_of_mut!(ORIGINAL_FIND), expect: None },
        Detour { name: "inkWidgetLibraryResource external spawn", target: spawn, absolute: true, detour: spawn_external as *const c_void, original: core::ptr::addr_of_mut!(ORIGINAL_SPAWN), expect: None },
    ];
    // SAFETY: the two resolved engine functions; their trampolines written before the detours can run.
    if let Err(e) = unsafe { install_chain("menu-tooltip", &detours) } {
        cvr_host::log!("[menu-tooltip] hook failed {e:?}\n");
        return false;
    }
    INSTALLED.store(true, Ordering::Release);
    true
}
/// The tooltip library's state for `path`: -1 unavailable, 0 loading, 1 ready.
pub(super) fn prepare_path(path: &str) -> i32 {
    prepare(resource_path(path)).0
}
/// The stock SpawnFromExternal of `controller`, the ready library declared on its owner meanwhile; null unless ready.
pub(super) fn spawn(controller: &Handle, parent: &Handle, path: &str, item: u64) -> Handle {
    if controller.is_null() || parent.is_null() {
        return Handle::null();
    }
    let hash = resource_path(path);
    let (state, token) = prepare(hash);
    if state != 1 {
        return Handle::null();
    }
    let previous = SCOPE.with(|s| s.replace(Some(Scope { token, path: hash, item })));
    let widget = rt::obj(controller, "SpawnFromExternal", &[A::H(parent), A::P((&hash as *const u64).cast()), A::Name(item)]);
    SCOPE.with(|s| s.set(previous));
    if widget.is_null() {
        cvr_host::log!("[menu-tooltip] native SpawnFromExternal invocation failed\n");
    }
    widget
}

/// VRMenuTooltipPrepare(path) -> Int32.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuTooltipPrepare(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let mut path = CString::empty();
    // SAFETY: the VM's frame for this native, its one String parameter.
    if let Some(frame) = unsafe { frame.as_mut() } {
        unsafe { frame.get_parameter(&mut path) };
        frame.finish_params();
    }
    let result = prepare_path(path.as_str());
    // SAFETY: the Int32 result slot.
    if let Some(out) = unsafe { out.cast::<i32>().as_mut() } {
        *out = result;
    }
}
/// VRMenuTooltipSpawn(controller, parent, path, libraryItem) -> handle:inkWidget.
///
/// # Safety
/// Called by the script VM.
#[no_mangle]
pub unsafe extern "C" fn VRMenuTooltipSpawn(_this: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _unused: i64) {
    let (mut controller, mut parent, mut path, mut item) = (Handle::null(), Handle::null(), CString::empty(), 0u64);
    // SAFETY: the VM's frame for this native, its four parameters in order.
    if let Some(frame) = unsafe { frame.as_mut() } {
        unsafe {
            frame.get_parameter(&mut controller);
            frame.get_parameter(&mut parent);
            frame.get_parameter(&mut path);
            frame.get_parameter(&mut item);
        }
        frame.finish_params();
    }
    let widget = spawn(&controller, &parent, path.as_str(), item);
    let out = out.cast::<Handle>();
    if !out.is_null() {
        // SAFETY: the handle result slot, its old value dropped.
        drop(unsafe { core::ptr::replace(out, widget) });
    }
}
