//! THE WORKER THREAD (formerly src/Core/WorkerThread.cpp) and the guarded engine-memory dumps of src/Core/Log.cpp.
//!
//! ONE THREAD, NOT ONE PER JOB: it polls the live-control file, the hotkeys, the resolution overrides, the resting
//! finger and two-hand files, and recovers the camera fast path -- file and registry work that must not happen on a
//! render thread. Each poll is idempotent and none depends on another's timing, so one loop with a sleep is both
//! sufficient and the only shape in which their order is obvious. With the verbose log on, every tenth pass dumps the
//! camera telemetry the patch sites write.
//!
//! THE DUMP HELPERS GUARD EVERY READ: they take an address from somewhere in the engine and print what is there, so a
//! mistyped offset in a diagnostic produces a line saying so instead of taking the game down. `Log` itself stays the
//! C++ half's (a C-variadic function); the state other files read (g_gameModuleBase/Size, the telemetry buffers,
//! g_verboseLog) stays there too, reached through the runtime.
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicPtr, Ordering::*};

use cvr_crt::{format, Arg};
use cvr_engine::code;

extern "C" {
    static CyberpunkVR_DebugPatchCamMain: core::sync::atomic::AtomicU64;
    static CyberpunkVR_DebugPatchCamVrcam: core::sync::atomic::AtomicU64;
    static g_unifixHits: u64;
    static g_unifixRenderObj: usize;
    static g_unifixProjDump: [f32; 9];
    static g_projStageHits: u64;
    static g_projStageFov: f32;
    static g_projStageAspect: f32;
    static g_projStageExtra: f32;
    static g_projStagePatched: bool;
    fn InitStereoOnce();
}

/// Core/Telemetry.hpp's TelemetryData (the patch sites write it; its offsets are baked into their machine code).
#[repr(C)]
pub struct TelemetryData {
    locate_hits: u32,
    _pad1: u32,
    locate_rbx: usize,
    locate_xmm0: f32,
    _pad2: [u32; 3],
    patch_hits: u32,
    _pad3: u32,
    patch_rdx: usize,
    patch_rsi: usize,
    patch_xmm0: [f32; 4],
    final_hits: u32,
    _pad4: u32,
    final_rsi: usize,
    delta_head_hits: u32,
    _pad5: u32,
    delta_head_rcx: usize,
    delta_head_xmm0: f32,
    _pad6: [u32; 3],
    move_xy_hits: u32,
    _pad7: u32,
    move_xy_rsi: usize,
    move_xy_xmm0: f32,
    _pad8: [u32; 3],
    free_delta_hits: u32,
    _pad9: u32,
    free_delta_rsi: usize,
    free_delta_xmm3: f32,
    _pad10: [u32; 3],
}
/// Core/Telemetry.hpp's SetterTraceData.
#[repr(C)]
pub struct SetterTraceData {
    meta_write_hits: u32,
    _pad1: u32,
    meta_write_temp: usize,
    meta_write_meta: usize,
    meta_write_rsp: usize,
    meta_consume_hits: u32,
    _pad2: u32,
    meta_consume_temp: usize,
    meta_consume_meta: usize,
    meta_consume_rsp: usize,
    clear_hits: u32,
    _pad3: u32,
    clear_temp: usize,
    clear_return: usize,
}
const _: () = assert!(
    core::mem::size_of::<TelemetryData>() == 184
        && core::mem::offset_of!(TelemetryData, patch_hits) == 32
        && core::mem::offset_of!(TelemetryData, free_delta_hits) == 152
        && core::mem::size_of::<SetterTraceData>() == 88
);

/// What the C++ half provides.
#[repr(C)]
pub struct Runtime {
    verbose_log: *const i32,
    backend_module_path: *const c_char,
    game_module_base: *mut usize,
    game_module_size: *mut usize,
    telemetry: *mut *mut TelemetryData,
    setter_trace: *mut *mut SetterTraceData,
    ensure_live_control_file_exists: extern "C" fn(),
    poll_live_controls: extern "C" fn(),
    install_stage_post_stereo: extern "C" fn(),
    install_stage_boot: extern "C" fn(),
    report_installed: extern "C" fn(),
    patch_fast_disarm: extern "C" fn(),
    poll_hotkeys: extern "C" fn(),
    apply_known_resolution_overrides: extern "C" fn(),
    rest_finger_tick: extern "C" fn(),
    rest_finger_right_tick: extern "C" fn(),
    two_hand_tick: extern "C" fn(),
}
// ---- VrCore.cpp's objects the worker reads (formerly C++ definitions), under their MSVC names
/// char g_backendModulePath[MAX_PATH]: the backend module the worker loads.
#[export_name = "?g_backendModulePath@@3PADA"]
#[allow(non_upper_case_globals)]
pub static mut g_backendModulePath: [u8; 260] = [0; 260];
/// size_t g_gameModuleSize.
#[export_name = "?g_gameModuleSize@@3_KA"]
#[allow(non_upper_case_globals)]
pub static mut g_gameModuleSize: usize = 0;
/// TelemetryData* g_telemetry; SetterTraceData* g_setterTrace (Core/Telemetry.hpp: one object each).
#[export_name = "?g_telemetry@@3PEAUTelemetryData@@EA"]
#[allow(non_upper_case_globals)]
pub static mut g_telemetry: *mut TelemetryData = core::ptr::null_mut();
#[export_name = "?g_setterTrace@@3PEAUSetterTraceData@@EA"]
#[allow(non_upper_case_globals)]
pub static mut g_setterTrace: *mut SetterTraceData = core::ptr::null_mut();

unsafe extern "C" {
    /// cvr-rig's: the resting fingers and the two-hand grip's ticks.
    safe fn cvr_anim_rest_finger_tick();
    safe fn cvr_anim_rest_finger_right_tick();
    safe fn cvr_anim_two_hand_tick();
}
extern "C" fn install_stage_post_stereo() {
    cvr_engine::stages::cvr_hooks_install_stage(2);
}
extern "C" fn install_stage_boot() {
    cvr_engine::stages::cvr_hooks_install_stage(1);
}
// SAFETY: the table holds addresses of statics and plain functions.
unsafe impl Sync for Runtime {}
/// The table, built here (formerly WorkerThread.cpp's, attached at its static initialisation).
static RT: Runtime = Runtime {
    verbose_log: (&raw const crate::vrcore_exports::g_verboseLog).cast(),
    backend_module_path: (&raw const g_backendModulePath).cast(),
    game_module_base: (&raw const crate::vrcore_exports::g_gameModuleBase).cast_mut().cast(),
    game_module_size: &raw mut g_gameModuleSize,
    telemetry: &raw mut g_telemetry,
    setter_trace: &raw mut g_setterTrace,
    ensure_live_control_file_exists: crate::runtime_paths::cvr_ensure_live_control_file_exists,
    poll_live_controls: crate::live_controls_poll::cvr_poll_live_controls,
    install_stage_post_stereo,
    install_stage_boot,
    report_installed: cvr_engine::stages::cvr_hooks_report_installed,
    patch_fast_disarm: crate::camera_owner::cvr_patch_fast_disarm,
    poll_hotkeys: crate::live_controls_poll::cvr_poll_hotkeys,
    apply_known_resolution_overrides: crate::vr_core_settings::cvr_apply_known_resolution_overrides,
    rest_finger_tick: cvr_anim_rest_finger_tick,
    rest_finger_right_tick: cvr_anim_rest_finger_right_tick,
    two_hand_tick: cvr_anim_two_hand_tick,
};
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const RT).cast_mut());
fn rt() -> &'static Runtime {
    // SAFETY: attached at the C++ half's static initialisation.
    unsafe { &*RUNTIME.load(Acquire) }
}
/// g_setterTrace: the tracers' counters (allocated at the worker's start).
pub fn setter_trace() -> usize {
    // SAFETY: the C++ half's pointer, read like the C++ installers read it.
    unsafe { *rt().setter_trace as usize }
}
fn verbose() -> bool {
    // SAFETY: the C++ half's volatile int.
    unsafe { core::ptr::read_volatile(rt().verbose_log) != 0 }
}
fn log(text: &str) {
    cvr_host::log::write(text);
}
/// A line formatted as the C runtime formats it (the C++ `Log` format strings, kept verbatim).
fn logf(fmt: &CStr, args: &[Arg]) {
    log(&format(fmt, args).to_string_lossy());
}
fn label_arg(label: &CStr) -> Arg {
    Arg::S(label.as_ptr())
}
fn sleep(ms: u32) {
    // SAFETY: a plain system call.
    unsafe { windows_sys::Win32::System::Threading::Sleep(ms) };
}

// ---- the guarded helpers (Log.cpp) ------------------------------------------------------------------------------

/// ReadFloatArraySafe: every float or false (the ones before the first unreadable one are written).
///
/// # Safety
/// `out` holds `count` floats (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_read_float_array_safe(
    src: *const f32,
    out: *mut f32,
    count: usize,
) -> bool {
    if src.is_null() || out.is_null() {
        return false;
    }
    for i in 0..count {
        let Some(v) = code::read::<f32>(src as usize + i * 4) else {
            return false;
        };
        // SAFETY: the caller's buffer.
        unsafe { *out.add(i) = v };
    }
    true
}
/// WriteFloatArraySafe.
///
/// # Safety
/// `values` holds `count` floats (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_write_float_array_safe(
    dst: *mut f32,
    values: *const f32,
    count: usize,
) -> bool {
    if dst.is_null() || values.is_null() {
        return false;
    }
    for i in 0..count {
        // SAFETY: the caller's values.
        if !code::write::<f32>(dst as usize + i * 4, unsafe { *values.add(i) }) {
            return false;
        }
    }
    true
}
/// WriteU32Safe.
#[no_mangle]
pub extern "C" fn cvr_write_u32_safe(addr: usize, value: u32) -> bool {
    code::write::<u32>(addr, value)
}
/// LooksProjectionLike: a 4x4 whose diagonal, translation column and last row look like a projection's.
///
/// # Safety
/// `values` holds `count` floats (or is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_looks_projection_like(values: *const f32, count: usize) -> bool {
    if values.is_null() || count < 16 {
        return false;
    }
    // SAFETY: 16 floats, checked above.
    let v = unsafe { core::slice::from_raw_parts(values, 16) };
    let (m00, m11, m03, m13, m23, m33) = (v[0], v[5], v[3], v[7], v[11], v[15]);
    if !(m00 > 0.2 && m00 < 8.0 && m11 > 0.2 && m11 < 8.0) {
        return false;
    }
    if m03.abs() > 0.1 || m13.abs() > 0.1 {
        return false;
    }
    m23.abs() > 0.1 || m33.abs() < 0.1 || (m33 - 1.0).abs() < 0.1
}
/// LogMatrix4x4.
///
/// # Safety
/// `prefix` is a C string and `values` 16 floats (or either is null).
#[no_mangle]
pub unsafe extern "C" fn cvr_log_matrix4x4(prefix: *const c_char, values: *const f32) {
    if prefix.is_null() || values.is_null() {
        return;
    }
    logf(c"%s\n", &[Arg::S(prefix)]);
    for row in 0..4 {
        // SAFETY: the caller's 16 floats.
        let v = |i: usize| Arg::F(f64::from(unsafe { *values.add(row * 4 + i) }));
        logf(c"  [%.6f %.6f %.6f %.6f]\n", &[v(0), v(1), v(2), v(3)]);
    }
}
/// InitGameModuleInfo: Cyberpunk2077.exe's base and size, once.
#[no_mangle]
pub extern "C" fn cvr_init_game_module_info() {
    let r = rt();
    // SAFETY: the C++ half's globals; module queries.
    unsafe {
        if *r.game_module_base != 0 && *r.game_module_size != 0 {
            return;
        }
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleA;
        use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let module = GetModuleHandleA(c"Cyberpunk2077.exe".as_ptr() as *const u8);
        if module.is_null() {
            return;
        }
        let mut info: MODULEINFO = core::mem::zeroed();
        if GetModuleInformation(
            GetCurrentProcess(),
            module,
            &mut info,
            size_of::<MODULEINFO>() as u32,
        ) == 0
        {
            return;
        }
        *r.game_module_base = info.lpBaseOfDll as usize;
        *r.game_module_size = info.SizeOfImage as usize;
    }
}
/// CvrLogText: one line for code that cannot name `Log` (the Lua host, where CET's logging namespace is called Log).
///
/// # Safety
/// `text` is a C string or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_log_text(text: *const c_char) {
    if !text.is_null() {
        // SAFETY: the caller's string.
        log(&unsafe { CStr::from_ptr(text) }.to_string_lossy());
    }
}

fn is_in_game_module(addr: usize) -> bool {
    let r = rt();
    // SAFETY: the C++ half's globals.
    let (base, size) = unsafe { (*r.game_module_base, *r.game_module_size) };
    base != 0 && size != 0 && addr >= base && addr < base + size
}
fn is_readable_address_range(addr: usize, size: usize) -> bool {
    use windows_sys::Win32::System::Memory::*;
    if addr == 0 || size == 0 || addr < 0x100_0000_0000 {
        return false; // small or tagged values like 0x0000000100000000
    }
    // SAFETY: a query about our own address space.
    let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    if unsafe {
        VirtualQuery(
            addr as *const c_void,
            &mut mbi,
            size_of::<MEMORY_BASIC_INFORMATION>(),
        )
    } != size_of::<MEMORY_BASIC_INFORMATION>()
    {
        return false;
    }
    if mbi.State != MEM_COMMIT || mbi.Protect & PAGE_GUARD != 0 || mbi.Protect & PAGE_NOACCESS != 0
    {
        return false;
    }
    let readable = PAGE_READONLY
        | PAGE_READWRITE
        | PAGE_WRITECOPY
        | PAGE_EXECUTE_READ
        | PAGE_EXECUTE_READWRITE
        | PAGE_EXECUTE_WRITECOPY;
    if mbi.Protect & readable == 0 {
        return false;
    }
    let end = mbi.BaseAddress as usize + mbi.RegionSize;
    end >= addr && addr + size <= end
}
fn log_null(label: &CStr) {
    logf(c"  %s: null\n", &[label_arg(label)]);
}
fn log_unreadable(label: &CStr, addr: usize) {
    logf(
        c"  %s @%p unreadable\n",
        &[label_arg(label), Arg::U(addr as u64)],
    );
}
fn log_vec4_at(label: &CStr, addr: usize) {
    if addr == 0 {
        return log_null(label);
    }
    let mut v = [0f32; 4];
    for (i, x) in v.iter_mut().enumerate() {
        let Some(f) = code::read::<f32>(addr + i * 4) else {
            return log_unreadable(label, addr);
        };
        *x = f;
    }
    logf(
        c"  %s @%p = (%.6f, %.6f, %.6f, %.6f)\n",
        &[
            label_arg(label),
            Arg::U(addr as u64),
            Arg::F(f64::from(v[0])),
            Arg::F(f64::from(v[1])),
            Arg::F(f64::from(v[2])),
            Arg::F(f64::from(v[3])),
        ],
    );
}
fn log_float_at(label: &CStr, addr: usize) {
    if addr == 0 {
        return log_null(label);
    }
    let Some(v) = code::read::<f32>(addr) else {
        return log_unreadable(label, addr);
    };
    logf(
        c"  %s @%p = %.6f\n",
        &[label_arg(label), Arg::U(addr as u64), Arg::F(f64::from(v))],
    );
}
fn log_u8_at(label: &CStr, addr: usize) {
    if addr == 0 {
        return log_null(label);
    }
    let Some(v) = code::read::<u8>(addr) else {
        return log_unreadable(label, addr);
    };
    logf(
        c"  %s @%p = 0x%02X (%u)\n",
        &[
            label_arg(label),
            Arg::U(addr as u64),
            Arg::U(u64::from(v)),
            Arg::U(u64::from(v)),
        ],
    );
}
fn log_ptr_at(label: &CStr, addr: usize) {
    if addr == 0 {
        return log_null(label);
    }
    let Some(v) = code::read::<usize>(addr) else {
        return log_unreadable(label, addr);
    };
    logf(
        c"  %s @%p = %p\n",
        &[label_arg(label), Arg::U(addr as u64), Arg::U(v as u64)],
    );
}
fn log_ptr_payload_vec4_at(label: &CStr, addr: usize) {
    if addr == 0 {
        return log_null(label);
    }
    let Some(v) = code::read::<usize>(addr) else {
        return log_unreadable(label, addr);
    };
    logf(
        c"  %s @%p = %p\n",
        &[label_arg(label), Arg::U(addr as u64), Arg::U(v as u64)],
    );
    if v != 0 && is_readable_address_range(v, 16) {
        // sprintf_s(payloadLabel, "%s payload", label) into 96 bytes.
        let mut payload = format(c"%s payload", &[label_arg(label)]).into_bytes();
        payload.truncate(95);
        let payload = std::ffi::CString::new(payload).unwrap_or_default();
        log_vec4_at(&payload, v);
    } else if v != 0 {
        logf(
            c"  %s payload @%p skipped (not a plausible readable pointer)\n",
            &[label_arg(label), Arg::U(v as u64)],
        );
    }
}
fn log_stack_window_at(label: &CStr, rsp: usize, slots: i32) {
    if rsp == 0 {
        return log_null(label);
    }
    logf(c"  %s @%p\n", &[label_arg(label), Arg::U(rsp as u64)]);
    let base = {
        let r = rt();
        // SAFETY: the C++ half's global.
        unsafe { *r.game_module_base }
    };
    for i in 0..slots.max(0) {
        let entry = rsp + i as usize * 8;
        let Some(value) = code::read::<usize>(entry) else {
            logf(
                c"    [%02d] @%p unreadable\n",
                &[Arg::I(i), Arg::U(entry as u64)],
            );
            return;
        };
        if is_in_game_module(value) {
            logf(
                c"    [%02d] @%p = %p (game+rva 0x%llX)\n",
                &[
                    Arg::I(i),
                    Arg::U(entry as u64),
                    Arg::U(value as u64),
                    Arg::U((value - base) as u64),
                ],
            );
        } else {
            logf(
                c"    [%02d] @%p = %p\n",
                &[Arg::I(i), Arg::U(entry as u64), Arg::U(value as u64)],
            );
        }
    }
}

// ---- the thread -------------------------------------------------------------------------------------------------

fn alloc<T>() -> *mut T {
    use windows_sys::Win32::System::Memory::{
        VirtualAlloc, MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE,
    };
    // SAFETY: a fresh zeroed allocation (VirtualAlloc commits zero pages; the C++ zeroed it again).
    let p = unsafe {
        VirtualAlloc(
            core::ptr::null(),
            size_of::<T>(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    } as *mut T;
    if !p.is_null() {
        // SAFETY: as above.
        unsafe { core::ptr::write_bytes(p as *mut u8, 0, size_of::<T>()) };
    }
    p
}

/// The values the telemetry dump compares against its previous pass.
#[derive(Default)]
struct Previous {
    patch_rdx: usize,
    patch_rsi: usize,
    final_rsi: usize,
    delta_head_rcx: usize,
    delta_head_hits: u32,
    move_xy_rsi: usize,
    move_xy_hits: u32,
    free_delta_rsi: usize,
    free_delta_hits: u32,
    meta_write_hits: u32,
    meta_write_temp: usize,
    meta_write_meta: usize,
    meta_write_rsp: usize,
    meta_consume_hits: u32,
    meta_consume_temp: usize,
    meta_consume_meta: usize,
    meta_consume_rsp: usize,
    clear_hits: u32,
    clear_temp: usize,
    clear_return: usize,
}

/// WorkerThread: eight seconds after the plugin loads, the live controls, the stereo module's node dispatcher (FIRST:
/// both want RVA 0x332F90 and MinHook allows one hook per address), the PostStereo and Boot hook stages; then the loop.
#[no_mangle]
pub extern "C" fn cvr_worker_thread_main() -> u32 {
    let r = rt();
    if verbose() {
        log("Worker thread started, waiting 8 seconds...\n");
    }
    // SAFETY: the C++ half's char[MAX_PATH].
    if unsafe { *r.backend_module_path } != 0 && verbose() {
        logf(
            c"Backend module loaded from: %s\n",
            &[Arg::S(r.backend_module_path)],
        );
    }
    sleep(8000);

    (r.ensure_live_control_file_exists)();
    (r.poll_live_controls)();
    cvr_init_game_module_info();
    // SAFETY: the C++ half's pointers, filled before any patch site installs (their addresses are baked in).
    unsafe {
        *r.telemetry = alloc::<TelemetryData>();
        *r.setter_trace = alloc::<SetterTraceData>();
        InitStereoOnce();
    }
    (r.install_stage_post_stereo)();
    (r.install_stage_boot)();
    (r.report_installed)();

    let mut prev = Previous::default();
    let mut loop_counter = 0u32;
    let (mut prev_cam_main, mut prev_cam_vrcam, mut prev_cam_valid) = (0u64, 0u64, false);
    loop {
        (r.poll_live_controls)();
        // THE FAST PATH'S RECOVERY, free while nothing is wrong: a tick in which a camera's recognition counter did not
        // advance is exactly the state that needs a re-identification.
        // SAFETY: exports of the C++ half.
        let (m, v) = unsafe {
            (
                CyberpunkVR_DebugPatchCamMain.load(Relaxed),
                CyberpunkVR_DebugPatchCamVrcam.load(Relaxed),
            )
        };
        // Without a second view (engine stereo, a mono launch) no second camera is latched to advance.
        let second_view_wanted =
            !crate::engine_stereo::enabled() && !crate::engine_stereo::mono_launch();
        let vrcam_stalled = second_view_wanted && v == prev_cam_vrcam;
        if prev_cam_valid && (m == prev_cam_main || vrcam_stalled) {
            (r.patch_fast_disarm)();
        }
        prev_cam_main = m;
        prev_cam_vrcam = v;
        prev_cam_valid = true;
        (r.poll_hotkeys)();
        (r.apply_known_resolution_overrides)();
        // The resting left-hand pose's DISK half and the two-hand holds: file work, here rather than in the pose path.
        (r.rest_finger_tick)();
        (r.rest_finger_right_tick)();
        (r.two_hand_tick)();

        let pass = loop_counter;
        loop_counter = loop_counter.wrapping_add(1);
        if pass % 10 != 0 || !verbose() {
            sleep(200);
            continue;
        }
        // SAFETY: the buffers allocated above.
        unsafe { telemetry_dump(&**r.telemetry, &**r.setter_trace, &mut prev) };
        sleep(200);
    }
}

unsafe fn telemetry_dump(t: &TelemetryData, s: &SetterTraceData, prev: &mut Previous) {
    // Reads of fields the patch sites write from other threads (volatile in the C++).
    macro_rules! rv {
        ($e:expr) => {
            // SAFETY: a field of the live buffer.
            unsafe { core::ptr::read_volatile(&raw const $e) }
        };
    }
    let p = |v: usize| Arg::U(v as u64);
    let f = |v: f32| Arg::F(f64::from(v));
    log("--- TELEMETRY SAMPLE ---\n");
    logf(
        c"LocateCamera: hits=%u, rbx=%p, xmm0(f32)=%.6f\n",
        &[
            Arg::U(u64::from(rv!(t.locate_hits))),
            p(rv!(t.locate_rbx)),
            f(rv!(t.locate_xmm0)),
        ],
    );
    let px = rv!(t.patch_xmm0);
    logf(
        c"PatchCamera:  hits=%u, rdx=%p, xmm0=(%.6f, %.6f, %.6f, %.6f)\n",
        &[
            Arg::U(u64::from(rv!(t.patch_hits))),
            p(rv!(t.patch_rdx)),
            f(px[0]),
            f(px[1]),
            f(px[2]),
            f(px[3]),
        ],
    );
    logf(c"PatchCamera:  rsi=%p\n", &[p(rv!(t.patch_rsi))]);
    logf(
        c"FinalCamera:  hits=%u, rsi=%p\n",
        &[Arg::U(u64::from(rv!(t.final_hits))), p(rv!(t.final_rsi))],
    );
    logf(
        c"DeltaHead:    hits=%u, rcx=%p, xmm0=%.6f\n",
        &[
            Arg::U(u64::from(rv!(t.delta_head_hits))),
            p(rv!(t.delta_head_rcx)),
            f(rv!(t.delta_head_xmm0)),
        ],
    );
    logf(
        c"MoveXY:       hits=%u, rsi=%p, xmm0=%.6f\n",
        &[
            Arg::U(u64::from(rv!(t.move_xy_hits))),
            p(rv!(t.move_xy_rsi)),
            f(rv!(t.move_xy_xmm0)),
        ],
    );
    logf(
        c"FreeDelta:    hits=%u, rsi=%p, xmm3=%.6f\n",
        &[
            Arg::U(u64::from(rv!(t.free_delta_hits))),
            p(rv!(t.free_delta_rsi)),
            f(rv!(t.free_delta_xmm3)),
        ],
    );
    // SAFETY: extern "C" globals of the C++ half.
    unsafe {
        let d = core::ptr::read_volatile(&raw const g_unifixProjDump);
        logf(
            c"Unifix:       hits=%llu, proj=[%.4f %.4f %.4f %.4f %.4f %.4f %.4f %.4f] fov=%.4f\n",
            &[
                Arg::U(core::ptr::read_volatile(&raw const g_unifixHits)),
                f(d[0]),
                f(d[1]),
                f(d[2]),
                f(d[3]),
                f(d[4]),
                f(d[5]),
                f(d[6]),
                f(d[7]),
                f(d[8]),
            ],
        );
        logf(
            c"ProjStage:    hits=%llu fov=%.4f aspect=%.4f extra=%.4f patched=%d\n",
            &[
                Arg::U(core::ptr::read_volatile(&raw const g_projStageHits)),
                f(core::ptr::read_volatile(&raw const g_projStageFov)),
                f(core::ptr::read_volatile(&raw const g_projStageAspect)),
                f(core::ptr::read_volatile(&raw const g_projStageExtra)),
                Arg::I(core::ptr::read_volatile(&raw const g_projStagePatched) as i32),
            ],
        );
        // The render object's projection, read directly (filled by the game during gameplay).
        let rbx = core::ptr::read_volatile(&raw const g_unifixRenderObj);
        if rbx != 0 {
            let mut proj = [0f32; 9];
            if proj.iter_mut().enumerate().all(|(i, x)| {
                code::read::<f32>(rbx + 0x3030 + i * 4)
                    .map(|v| *x = v)
                    .is_some()
            }) {
                logf(
                    c"UnifixRender: rbx=%p p0-3=[%.6f %.6f %.6f %.6f] p4-7=[%.6f %.6f %.6f %.6f] fov=%.6f\n",
                    &[
                        p(rbx),
                        f(proj[0]),
                        f(proj[1]),
                        f(proj[2]),
                        f(proj[3]),
                        f(proj[4]),
                        f(proj[5]),
                        f(proj[6]),
                        f(proj[7]),
                        f(proj[8]),
                    ],
                );
            }
        }
    }
    let off = |a: usize, o: usize| if a != 0 { a + o } else { 0 };
    let (patch_rdx, patch_rsi) = (rv!(t.patch_rdx), rv!(t.patch_rsi));
    if patch_rdx != prev.patch_rdx || patch_rsi != prev.patch_rsi {
        log_vec4_at(
            c"patch rdx-0x10",
            if patch_rdx != 0 { patch_rdx - 0x10 } else { 0 },
        );
        log_vec4_at(c"patch rdx+0x00", patch_rdx);
        log_vec4_at(c"patch rdx+0x10", off(patch_rdx, 0x10));
        log_vec4_at(c"patch rdx+0x20", off(patch_rdx, 0x20));
        log_u8_at(c"patch rsi+0xB0", off(patch_rsi, 0xB0));
        log_u8_at(c"patch rsi+0xB1", off(patch_rsi, 0xB1));
        log_vec4_at(c"patch rsi+0x90", off(patch_rsi, 0x90));
        log_vec4_at(c"patch rsi+0xA0", off(patch_rsi, 0xA0));
    }
    let final_rsi = rv!(t.final_rsi);
    if final_rsi != prev.final_rsi {
        log_float_at(c"final rsi+0x40", off(final_rsi, 0x40));
        log_float_at(c"final rsi+0x44", off(final_rsi, 0x44));
        log_vec4_at(c"final rsi+0x30", off(final_rsi, 0x30));
        log_vec4_at(c"final rsi+0x40", off(final_rsi, 0x40));
    }
    let (dh_rcx, dh_hits) = (rv!(t.delta_head_rcx), rv!(t.delta_head_hits));
    if dh_rcx != prev.delta_head_rcx || dh_hits != prev.delta_head_hits {
        for (l, o) in [
            (c"delta rcx+0x98", 0x98),
            (c"delta rcx+0x9C", 0x9C),
            (c"delta rcx+0xA0", 0xA0),
            (c"delta rcx+0xA4", 0xA4),
            (c"delta rcx+0xA8", 0xA8),
        ] {
            log_float_at(l, off(dh_rcx, o));
        }
    }
    let (mx_rsi, mx_hits) = (rv!(t.move_xy_rsi), rv!(t.move_xy_hits));
    if mx_rsi != prev.move_xy_rsi || mx_hits != prev.move_xy_hits {
        for (l, o) in [
            (c"moveXY rsi+0x90", 0x90),
            (c"moveXY rsi+0x94", 0x94),
            (c"moveXY rsi+0x98", 0x98),
            (c"moveXY rsi+0x9C", 0x9C),
        ] {
            log_float_at(l, off(mx_rsi, o));
        }
    }
    let (fd_rsi, fd_hits) = (rv!(t.free_delta_rsi), rv!(t.free_delta_hits));
    if fd_rsi != prev.free_delta_rsi || fd_hits != prev.free_delta_hits {
        for (l, o) in [
            (c"freeDelta rsi+0xCC8", 0xCC8),
            (c"freeDelta rsi+0xCCC", 0xCCC),
            (c"freeDelta rsi+0x208", 0x208),
            (c"freeDelta rsi+0x20C", 0x20C),
        ] {
            log_float_at(l, off(fd_rsi, o));
        }
    }
    let (mw_hits, mw_temp, mw_meta, mw_rsp) = (
        rv!(s.meta_write_hits),
        rv!(s.meta_write_temp),
        rv!(s.meta_write_meta),
        rv!(s.meta_write_rsp),
    );
    if (mw_hits != 0 && prev.meta_write_hits == 0)
        || mw_temp != prev.meta_write_temp
        || mw_meta != prev.meta_write_meta
        || mw_rsp != prev.meta_write_rsp
    {
        logf(
            c"NativeSetMetaWrite: hits=%u, temp=%p, meta=%p, rsp=%p\n",
            &[
                Arg::U(u64::from(mw_hits)),
                p(mw_temp),
                p(mw_meta),
                p(mw_rsp),
            ],
        );
        log_vec4_at(c"metaWrite temp+0x00", mw_temp);
        log_ptr_at(c"metaWrite temp+0x08", off(mw_temp, 8));
        log_ptr_at(c"metaWrite meta+0x00", mw_meta);
        log_ptr_at(c"metaWrite meta+0x08", off(mw_meta, 8));
        log_ptr_payload_vec4_at(c"metaWrite meta+0x10", off(mw_meta, 0x10));
        log_stack_window_at(c"metaWrite stack", mw_rsp, 12);
    }
    let (mc_hits, mc_temp, mc_meta, mc_rsp) = (
        rv!(s.meta_consume_hits),
        rv!(s.meta_consume_temp),
        rv!(s.meta_consume_meta),
        rv!(s.meta_consume_rsp),
    );
    if (mc_hits != 0 && prev.meta_consume_hits == 0)
        || mc_temp != prev.meta_consume_temp
        || mc_meta != prev.meta_consume_meta
        || mc_rsp != prev.meta_consume_rsp
    {
        logf(
            c"NativeSetMetaConsume: hits=%u, temp=%p, meta=%p, rsp=%p\n",
            &[
                Arg::U(u64::from(mc_hits)),
                p(mc_temp),
                p(mc_meta),
                p(mc_rsp),
            ],
        );
        log_vec4_at(c"metaConsume temp+0x00", mc_temp);
        log_ptr_at(c"metaConsume temp+0x08", off(mc_temp, 8));
        log_ptr_at(c"metaConsume meta+0x00", mc_meta);
        log_ptr_at(c"metaConsume meta+0x08", off(mc_meta, 8));
        log_ptr_payload_vec4_at(c"metaConsume meta+0x10", off(mc_meta, 0x10));
        log_stack_window_at(c"metaConsume stack", mc_rsp, 12);
    }
    let (cl_hits, cl_temp, cl_return) = (rv!(s.clear_hits), rv!(s.clear_temp), rv!(s.clear_return));
    if (cl_hits != 0 && prev.clear_hits == 0)
        || cl_temp != prev.clear_temp
        || cl_return != prev.clear_return
    {
        logf(
            c"NativeSetClear: hits=%u, temp=%p, return=%p\n",
            &[Arg::U(u64::from(cl_hits)), p(cl_temp), p(cl_return)],
        );
        log_vec4_at(c"clear temp+0x00", cl_temp);
        log_ptr_at(c"clear temp+0x08", off(cl_temp, 8));
    }
    // What the next pass compares against: re-read, as the C++ did at the end of its pass.
    *prev = Previous {
        patch_rdx: rv!(t.patch_rdx),
        patch_rsi: rv!(t.patch_rsi),
        final_rsi: rv!(t.final_rsi),
        delta_head_rcx: rv!(t.delta_head_rcx),
        delta_head_hits: rv!(t.delta_head_hits),
        move_xy_rsi: rv!(t.move_xy_rsi),
        move_xy_hits: rv!(t.move_xy_hits),
        free_delta_rsi: rv!(t.free_delta_rsi),
        free_delta_hits: rv!(t.free_delta_hits),
        meta_write_hits: rv!(s.meta_write_hits),
        meta_write_temp: rv!(s.meta_write_temp),
        meta_write_meta: rv!(s.meta_write_meta),
        meta_write_rsp: rv!(s.meta_write_rsp),
        meta_consume_hits: rv!(s.meta_consume_hits),
        meta_consume_temp: rv!(s.meta_consume_temp),
        meta_consume_meta: rv!(s.meta_consume_meta),
        meta_consume_rsp: rv!(s.meta_consume_rsp),
        clear_hits: rv!(s.clear_hits),
        clear_temp: rv!(s.clear_temp),
        clear_return: rv!(s.clear_return),
    };
}
