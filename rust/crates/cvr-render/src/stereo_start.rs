//! The stereo module's start (formerly src/Core/VrCore.cpp's InitStereoOnce, InitOpenXREarly and
//! CyberpunkVRPort_InitStereo, src/Stereo/SyncStereo.cpp's sync_stereo_init, sync_stereo_install_early_hooks,
//! sync_stereo_ensure_descriptor_probe, its D3D12CreateDevice detour and install_desc_ring_probe, and
//! src/Stereo/DetourRegistry.cpp's InstallEngineDetours with the code guards it prepares first).
//!
//! The order is the C++ one: the kill switch and the opt-out file, the launcher's DEBUG gate, the vrcam selection and
//! its watcher thread, the module base and the profiler's tick scale, then -- before the game creates its device --
//! the descriptor heap patch, the D3D12CreateDevice hook (through RenderDoc's serializer when it is loaded), and the
//! engine pass: MinHook, the registered detours by RVA (the single pass modules' code guards and observers before any
//! of them is live), the instance witness, the source selectors' switch, the FSR 3.0 / XeSS second-eye contexts, the
//! handle assign and the direct hooks. Every log line is the C++ one.

use core::cell::Cell;
use core::ffi::{c_char, c_void};
use core::ptr::{addr_of, addr_of_mut, null_mut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering::*};

use crate::declared_moved_exports::{CyberpunkVR_SourceSrvBindingEnable, CyberpunkVR_StereoModuleLoaded};
use crate::used_moved_exports_rest::CyberpunkVR_StereoModuleEnable;

const HANDLE_ASSIGN_RVA: usize = 0x21_309C; // sub_1407CDAE4(dst, src)
/// The instance observer's completed writer and stamp publication (NativeInstanceObserver.hpp).
const INSTANCE_WRITER_RVA: usize = 0x33_7144;
const INSTANCE_PUBLISH_RVA: usize = 0x4F_34FC;
/// The source SRV binding's two detours.
const SOURCE_SRV_RVAS: [usize; 2] = [0x33_8FC0, 0x33_8ED4];
const MH_OK: i32 = 0;
const MH_ERROR_ALREADY_INITIALIZED: i32 = 1;

/// double g_qpc_to_ms: 1000 / the performance counter's frequency (the stereo profiler's scale), set at init.
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static mut g_qpc_to_ms: f64 = 0.0;

#[allow(non_snake_case)]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
    fn GetEnvironmentVariableW(name: *const u16, buffer: *mut u16, size: u32) -> u32;
    fn MH_Initialize() -> i32;
    fn MH_CreateHook(target: *mut c_void, detour: *mut c_void, original: *mut *mut c_void) -> i32;
    fn MH_EnableHook(target: *mut c_void) -> i32;
}
#[allow(improper_ctypes)]
extern "C" {
    /// Profiler.cpp's: the stereo module's log.
    fn cvr_cpp_stereo_log(text: *const c_char);
    /// cvr-ffi's host.rs: the plugin's main log.
    fn cvr_log_write(text: *const c_char, len: usize);
    /// OpenXRManager.cpp's: OpenXRManager::Get().Init().
    fn cvr_cpp_xr_init();
    /// DeviceHooks.cpp's: the game's device, taken once under its lock, with its "[mirror]" line.
    fn cvr_cpp_capture_game_device(device: *mut c_void);
    fn ApplyLauncherDebugGate();
    /// instance_witness_runtime.rs's (its private abi module).
    fn cvr_iw_rt_prepare(base: usize) -> bool;
    fn cvr_iw_rt_install(base: usize, threads_quiescent: bool) -> bool;
    fn cvr_iw_rt_installed() -> bool;
}

fn stereo_log(text: &str) {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(0);
    // SAFETY: a terminated string.
    unsafe { cvr_cpp_stereo_log(bytes.as_ptr().cast()) };
}
/// `Log(text)`: the plugin's main log.
fn main_log(text: &str) {
    // SAFETY: the bytes and their length.
    unsafe { cvr_log_write(text.as_ptr().cast(), text.len()) };
}
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn environment_present(name: &str) -> bool {
    let name = wide(name);
    // SAFETY: a terminated name, no buffer (the call returns the size the value needs, zero when it is absent).
    unsafe { GetEnvironmentVariableW(name.as_ptr(), null_mut(), 0) != 0 }
}
fn exe_base() -> usize {
    // SAFETY: the module base the stereo init publishes.
    unsafe { core::ptr::read_volatile(addr_of!(crate::stereo_shared::g_exe_base)) as usize }
}

// ---- InitOpenXREarly / InitStereoOnce

thread_local! {
    static XR_INIT_REENTRY: Cell<bool> = const { Cell::new(false) };
}
/// InitOpenXREarly: the XR instance and session (OpenXRManager::Init), not re-entered from its own thread.
#[no_mangle]
pub extern "C" fn InitOpenXREarly() {
    if XR_INIT_REENTRY.get() {
        return;
    }
    XR_INIT_REENTRY.set(true);
    // SAFETY: a C++ service.
    unsafe { cvr_cpp_xr_init() };
    XR_INIT_REENTRY.set(false);
}

static STEREO_STARTED: AtomicBool = AtomicBool::new(false);
/// InitStereoOnce: the engine hooks, once, before the game's D3D12CreateDevice (the descriptor heap the second view
/// needs is sized at device creation); out with CyberpunkVR_StereoModuleEnable=0 or config\vrport_nostereo.txt.
#[no_mangle]
pub extern "C" fn InitStereoOnce() {
    if STEREO_STARTED.swap(true, Relaxed) {
        return;
    }
    // SAFETY: an exported control.
    if unsafe { core::ptr::read_volatile(addr_of!(CyberpunkVR_StereoModuleEnable)) } == 0 {
        main_log("Stereo: module disabled by CyberpunkVR_StereoModuleEnable=0\n");
        return;
    }
    if cvr_host::paths::file(cvr_host::paths::Folder::Config, "vrport_nostereo.txt").exists() {
        main_log("Stereo: config\\vrport_nostereo.txt present -- engine hooks not installed\n");
        return;
    }
    // SAFETY: Rust's (debug_gate_table.rs), before a single hook is installed.
    unsafe { ApplyLauncherDebugGate() };
    sync_stereo_init();
    sync_stereo_install_early_hooks();
    // SAFETY: an exported state.
    unsafe { core::ptr::write_volatile(addr_of_mut!(CyberpunkVR_StereoModuleLoaded), 1) };
    main_log("Stereo: sync_stereo engine hooks installed\n");
}
/// CyberpunkVRPort_InitStereo: the RED4ext plugin's entry point into the stereo module.
#[no_mangle]
pub extern "C" fn CyberpunkVRPort_InitStereo() {
    InitStereoOnce();
}

// ---- sync_stereo_init / sync_stereo_install_early_hooks

fn sync_stereo_init() {
    // before any hook can observe a view
    // SAFETY: Rust's (vrcam_selection.rs).
    unsafe { crate::vrcam_selection::cvr_load_vrcam_selection() };
    // adopts the enabled component's real virtualCameraName once CET reports it
    // SAFETY: as above; the watcher runs for the process's life.
    std::thread::spawn(|| unsafe { crate::vrcam_selection::cvr_vrcam_active_watcher() });
    // SAFETY: the process's own module; the globals are the stereo module's, written once here.
    unsafe {
        core::ptr::write_volatile(addr_of_mut!(crate::stereo_shared::g_exe_base), GetModuleHandleW(core::ptr::null()).cast());
        let mut frequency = 0i64;
        QueryPerformanceFrequency(&mut frequency);
        if frequency != 0 {
            core::ptr::write_volatile(addr_of_mut!(g_qpc_to_ms), 1000.0 / frequency as f64);
        }
    }
}
fn sync_stereo_install_early_hooks() {
    crate::device_creation::cvr_patch_descriptor_heap_size();
    // before the game's D3D12CreateDevice, so the real device, DIRECT queue and command-list vtables are captured
    sync_stereo_ensure_descriptor_probe();
    install_desc_ring_probe();
}

// ---- the D3D12CreateDevice hook

type CreateDevice = unsafe extern "system" fn(*mut c_void, i32, *const c_void, *mut *mut c_void) -> i32;
/// g_orig_D3D12CreateDevice.
static CREATE_DEVICE_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
unsafe extern "system" fn hook_d3d12_create_device(adapter: *mut c_void, level: i32, riid: *const c_void, out: *mut *mut c_void) -> i32 {
    if !out.is_null() {
        crate::cpp_services::CyberpunkVRPort_GpuDiagBeforeDevice();
    }
    // SAFETY: the original D3D12CreateDevice, installed before this detour could run.
    let original: CreateDevice = unsafe { core::mem::transmute(CREATE_DEVICE_ORIGINAL.load(Acquire)) };
    // SAFETY: the game's arguments.
    let hr = unsafe { original(adapter, level, riid, out) };
    // SAFETY: the created device when the call succeeded.
    if hr >= 0 && !out.is_null() && unsafe { !(*out).is_null() } {
        let device = unsafe { *out };
        // SAFETY: a live device.
        unsafe {
            crate::cpp_services::CyberpunkVRPort_GpuDiagDevice(device);
            cvr_cpp_capture_game_device(device);
        }
        crate::device_creation::cvr_patch_device_descriptor_slot(device);
    }
    hr
}
fn sync_stereo_ensure_descriptor_probe() {
    crate::device_creation::cvr_patch_descriptor_heap_size();
    if crate::moved_globals::g_desc_probe_installed.compare_exchange(false, true, SeqCst, SeqCst).is_err() {
        return;
    }
    if exe_base() == 0 {
        sync_stereo_init();
    }
    let name = wide("d3d12.dll");
    // SAFETY: a terminated module name.
    let mut d3d12 = unsafe { GetModuleHandleW(name.as_ptr()) };
    if d3d12.is_null() {
        // SAFETY: as above.
        d3d12 = unsafe { LoadLibraryW(name.as_ptr()) };
    }
    if d3d12.is_null() {
        stereo_log("[descheap] d3d12.dll not present");
        return;
    }
    // captures route device creation through RenderDoc's serializer: hooking only the loader export misses that path
    // SAFETY: a loaded module and a terminated name.
    let target = unsafe { cvr_gpu::renderdoc_bridge::cvr_renderdoc_resolve_hook_target(d3d12, c"D3D12CreateDevice".as_ptr()) };
    if target.is_null() {
        stereo_log("[descheap] D3D12CreateDevice not found");
        return;
    }
    // SAFETY: MinHook's initialisation (idempotent).
    let status = unsafe { MH_Initialize() };
    if status != MH_OK && status != MH_ERROR_ALREADY_INITIALIZED {
        stereo_log(&format!("[descheap] MH_Initialize failed: {status}"));
        return;
    }
    // SAFETY: the export D3D12CreateDevice (or RenderDoc's), a detour of its signature and the original's slot.
    let hooked = unsafe {
        MH_CreateHook(target, hook_d3d12_create_device as *mut c_void, CREATE_DEVICE_ORIGINAL.as_ptr().cast()) == MH_OK
            && MH_EnableHook(target) == MH_OK
    };
    if !hooked {
        stereo_log(&format!("[descheap] failed to hook D3D12CreateDevice @{:016X}", target as usize));
        return;
    }
    stereo_log(&format!(
        "[descheap] D3D12CreateDevice hook installed @{:016X} (enlarge={} target={})",
        target as usize,
        crate::moved_globals::g_enable_desc_heap_enlarge.load(Relaxed) as i32,
        crate::moved_globals::g_desc_heap_target.load(Relaxed)
    ));
}

// ---- the engine pass

static ENGINE_PASS_INSTALLED: AtomicBool = AtomicBool::new(false);
fn install_desc_ring_probe() {
    if ENGINE_PASS_INSTALLED.load(Relaxed) {
        return;
    }
    if exe_base() == 0 {
        sync_stereo_init();
    }
    // SAFETY: MinHook's initialisation (idempotent).
    let status = unsafe { MH_Initialize() };
    if status != MH_OK && status != MH_ERROR_ALREADY_INITIALIZED {
        stereo_log(&format!("[install] MH_Initialize failed: {status}"));
        return;
    }
    // everything that has moved to the registry, in one pass, ascending RVA
    install_engine_detours();
    // a second-eye context for FSR 3.0 and XeSS (static imports, loaded by now)
    // SAFETY: Rust's (upscaler_views.rs).
    unsafe { crate::upscaler_views::cvr_upscaler_views_install() };
    let base = exe_base();
    // SAFETY: the stereo module's handle assign, written once here.
    unsafe { core::ptr::write_volatile(addr_of_mut!(crate::stereo_shared::g_handle_assign), base + HANDLE_ASSIGN_RVA) };
    // the clouds node, the sky work, the node dispatcher and the view capability test
    crate::engine_hooks::cvr_install_engine_hooks(base);
    ENGINE_PASS_INSTALLED.store(true, Relaxed);
    stereo_log("[install] engine hooks installed");
}

static INSTANCE_WITNESS_PREPARED: AtomicBool = AtomicBool::new(false);
/// What must run before any detour is live: the code guards and observers of the single pass modules.
extern "C" fn prepare_before_detours() {
    let base = exe_base() as u64;
    crate::preparation_trace::cvr_prep_parent_code_guards(base);
    crate::source_cbv_observe::cvr_scbo_prepare_code_guards(base);
    crate::source_srv_observe::cvr_ssrvo_prepare_code_guards(base);
    crate::source_cbv_snapshot_receipt::cvr_scsn_prepare_upload_code_guards(base);
    crate::instance_observer_native::cvr_nio2_prepare_code_guards(base);
    // SAFETY: Rust's, the game's base.
    INSTANCE_WITNESS_PREPARED.store(unsafe { cvr_iw_rt_prepare(base as usize) }, Relaxed);
    CyberpunkVR_SourceSrvBindingEnable.store(0, SeqCst);
    if environment_present("CVR_STEREO_SOURCE_SELECTORS") {
        crate::source_srv_observe::cvr_ssrvo_configure_xstate();
    }
    // SAFETY: the source CBV observer's creation-address callback, of the registry's signature.
    unsafe { crate::upload_registry::cvr_upload_set_creation_observer(crate::source_cbv_observe::cvr_scbo_creation_address as *const () as usize) };
    crate::paired_frame::cvr_npf_prepare_guards();
    crate::scene_update::cvr_su_prepare();
    crate::preparation_trace::cvr_prep_set_scene_update_poll(crate::scene_update::cvr_su_poll as *mut c_void);
    crate::definition_tracking::cvr_ndt_prepare_code_guards();
}
extern "C" fn log_line(text: *const c_char) {
    // SAFETY: a terminated line from the registry.
    unsafe { cvr_cpp_stereo_log(text) };
}
/// InstallEngineDetours: the registry's pass, then the instance witness and the source selectors' switch.
fn install_engine_detours() -> i32 {
    let base = exe_base();
    let failed = cvr_engine::detours::cvr_detours_install(base, log_line, Some(prepare_before_detours));
    if failed < 0 {
        return failed;
    }
    let mut mask = 0u32;
    // SAFETY: Rust's, the game's base (the threads are not known to be quiescent).
    let witness = INSTANCE_WITNESS_PREPARED.load(Relaxed) && unsafe { cvr_iw_rt_install(base, false) };
    // SAFETY: as above.
    if witness && unsafe { cvr_iw_rt_installed() } {
        mask |= 2;
    }
    stereo_log(&format!("[instance] successful CAS witness {}", if witness { "ok" } else { "FAILED" }));
    if cvr_engine::detours::cvr_detours_installed_at(INSTANCE_WRITER_RVA) {
        mask |= 1;
    }
    if cvr_engine::detours::cvr_detours_installed_at(INSTANCE_PUBLISH_RVA) {
        mask |= 4;
    }
    crate::instance_observer_native::cvr_nio2_set_installed(mask);
    if SOURCE_SRV_RVAS.iter().all(|&rva| cvr_engine::detours::cvr_detours_installed_at(rva))
        && environment_present("CVR_STEREO_SOURCE_SELECTORS")
        && crate::source_srv_observe::cvr_ssrvo_compatible()
        && crate::source_srv_observe::cvr_ssrvo_configure_xstate()
    {
        CyberpunkVR_SourceSrvBindingEnable.store(1, SeqCst);
    }
    failed
}
