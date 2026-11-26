//! What killed the device, when D3D12 will not say (formerly src/Hooks/Dred.cpp and ListAudit.cpp).
//!
//! DRED (Device Removed Extended Data) records auto-breadcrumbs of the last GPU work and the page fault that ended
//! it -- if it is enabled BEFORE any device exists (`cvr_dred_enable_once`, from each CreateDXGIFactory entry). The
//! first device removal is dumped once: the lists the GPU had not finished, the faulting address and the allocations
//! around it. With CVR_GPU_DIAG ("dred"/"1", "debug" for the debug layer too, "lists" for the audit) the device also
//! gets a removal watch (a fence a removed device completes to UINT64_MAX), the debug layer's errors with their
//! callers' stacks, and the command-list audit: every allocator Reset (own hook), list Reset/Close and submission is
//! recorded with its thread and stack, and a list submitted open, or whose allocator was reset after it was recorded,
//! is reported with the three stacks. src/Hooks/Dred.cpp and ListAudit.cpp keep the C++ names their callers use.
#![allow(unsafe_op_in_unsafe_fn)]
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, AtomicUsize, Ordering};
use cvr_host::log;
use std::sync::{Mutex, Once, OnceLock};
use windows::core::{Interface, PCSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HMODULE};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameA, GetModuleHandleA, GetModuleHandleExA, GetProcAddress, LoadLibraryA, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
    GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentThreadId, WaitForSingleObject, INFINITE};

fn p(v: *const c_void) -> String {
    format!("{:016X}", v as usize)
}
/// A D3D12 object's debug name: the ANSI one, else the wide one as UTF-8 (src/Hooks/SwapChain.cpp's DebugName).
unsafe fn debug_name(ansi: *const u8, wide: *const u16) -> String {
    if !ansi.is_null() && *ansi != 0 {
        return CStr::from_ptr(ansi as *const c_char).to_string_lossy().into_owned();
    }
    if wide.is_null() {
        return "<unnamed>".into();
    }
    let mut n = 0;
    while *wide.add(n) != 0 {
        n += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(wide, n))
}

// ---------------------------------------------------------------- the device DRED is read from

struct Tracked {
    device: Option<ID3D12Device>,
    dumped: bool,
}
// SAFETY: a D3D12 device is free-threaded.
unsafe impl Send for Tracked {}
static TRACKED: Mutex<Tracked> = Mutex::new(Tracked { device: None, dumped: false });

/// RememberDredDevice: the device a later dump reads (a new one re-arms the dump).
#[no_mangle]
pub unsafe extern "C" fn cvr_dred_remember_device(device: *mut c_void) {
    let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return };
    let mut t = TRACKED.lock().unwrap_or_else(|p| p.into_inner());
    if t.device.as_ref().map(|d| d.as_raw()) != Some(device.as_raw()) {
        t.device = Some(device.clone());
        t.dumped = false;
        log!("[DRED] Tracking D3D12 device {}\n", p(device.as_raw()));
    }
}
/// RememberDredDeviceFromSwapChain: the swapchain's device, or its back buffer's.
#[no_mangle]
pub unsafe extern "C" fn cvr_dred_remember_device_from_swapchain(swap_chain: *mut c_void) {
    let Some(swap_chain) = IDXGISwapChain::from_raw_borrowed(&swap_chain) else {
        return;
    };
    let device = swap_chain.GetDevice::<ID3D12Device>().or_else(|_| {
        let buffer: ID3D12Resource = swap_chain.GetBuffer(0)?;
        let mut device: Option<ID3D12Device> = None;
        buffer.GetDevice(&mut device)?;
        device.ok_or_else(windows::core::Error::empty)
    });
    if let Ok(device) = device {
        cvr_dred_remember_device(device.as_raw());
    }
}
type GetDebugInterface = unsafe extern "system" fn(*const windows::core::GUID, *mut *mut c_void) -> windows::core::HRESULT;
unsafe fn get_debug_interface<T: Interface>(f: GetDebugInterface) -> Option<T> {
    let mut out: *mut c_void = core::ptr::null_mut();
    f(&T::IID, &mut out).ok().ok()?;
    (!out.is_null()).then(|| T::from_raw(out))
}
/// EnableDredOnce: auto-breadcrumbs and page faults (and breadcrumb context on Windows 10 2004+) forced on.
#[no_mangle]
pub extern "C" fn cvr_dred_enable_once() {
    static ONCE: Once = Once::new();
    // SAFETY: d3d12's own entry point and the DRED settings interfaces it returns.
    ONCE.call_once(|| unsafe {
        let Ok(d3d12) = LoadLibraryA(PCSTR(c"d3d12.dll".as_ptr() as *const u8)) else {
            log!("[DRED] LoadLibrary(d3d12.dll) failed; DRED not enabled\n");
            return;
        };
        let Some(f) = GetProcAddress(d3d12, PCSTR(c"D3D12GetDebugInterface".as_ptr() as *const u8)) else {
            log!("[DRED] D3D12GetDebugInterface export missing; DRED not enabled\n");
            return;
        };
        let f: GetDebugInterface = core::mem::transmute(f);
        if let Some(s1) = get_debug_interface::<ID3D12DeviceRemovedExtendedDataSettings1>(f) {
            s1.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            s1.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            s1.SetBreadcrumbContextEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            log!("[DRED] Enabled (settings1): AutoBreadcrumbs+PageFault+BreadcrumbContext\n");
            return;
        }
        if let Some(s) = get_debug_interface::<ID3D12DeviceRemovedExtendedDataSettings>(f) {
            s.SetAutoBreadcrumbsEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            s.SetPageFaultEnablement(D3D12_DRED_ENABLEMENT_FORCED_ON);
            log!("[DRED] Enabled (settings): AutoBreadcrumbs+PageFault\n");
            return;
        }
        log!("[DRED] D3D12GetDebugInterface QI for DRED settings failed; DRED not enabled\n");
    });
}
/// IsDeviceRemovedHr: DXGI_ERROR_DEVICE_REMOVED, _HUNG or _RESET.
#[no_mangle]
pub extern "C" fn cvr_dred_is_device_removed_hr(hr: i32) -> bool {
    hr == 0x887A0005u32 as i32 || hr == 0x887A0006u32 as i32 || hr == 0x887A0007u32 as i32
}

/// D3D12_AUTO_BREADCRUMB_OP in its declaration order (d3d12.h).
fn op_name(op: u32) -> &'static str {
    const NAMES: [&str; 50] = [
        "SetMarker",
        "BeginEvent",
        "EndEvent",
        "DrawInstanced",
        "DrawIndexedInstanced",
        "ExecuteIndirect",
        "Dispatch",
        "CopyBufferRegion",
        "CopyTextureRegion",
        "CopyResource",
        "CopyTiles",
        "ResolveSubresource",
        "ClearRenderTargetView",
        "ClearUnorderedAccessView",
        "ClearDepthStencilView",
        "ResourceBarrier",
        "ExecuteBundle",
        "Present",
        "ResolveQueryData",
        "BeginSubmission",
        "EndSubmission",
        "DecodeFrame",
        "ProcessFrames",
        "AtomicCopyBufferUint",
        "AtomicCopyBufferUint64",
        "ResolveSubresourceRegion",
        "WriteBufferImmediate",
        "DecodeFrame1",
        "SetProtectedResourceSession",
        "DecodeFrame2",
        "ProcessFrames1",
        "BuildRaytracingAccelerationStructure",
        "EmitRaytracingAccelerationStructurePostbuildInfo",
        "CopyRaytracingAccelerationStructure",
        "DispatchRays",
        "InitializeMetaCommand",
        "ExecuteMetaCommand",
        "EstimateMotion",
        "ResolveMotionVectorHeap",
        "SetPipelineState1",
        "InitializeExtensionCommand",
        "ExecuteExtensionCommand",
        "DispatchMesh",
        "EncodeFrame",
        "ResolveEncoderOutputMetadata",
        "Barrier",
        "BeginCommandList",
        "DispatchGraph",
        "SetProgram",
        "ProcessFrames2",
    ];
    NAMES.get(op as usize).copied().unwrap_or("-")
}
/// D3D12_AUTO_BREADCRUMB_NODE, read as raw pointers.
#[repr(C)]
struct BreadcrumbNode {
    list_name_a: *const u8,
    list_name_w: *const u16,
    queue_name_a: *const u8,
    queue_name_w: *const u16,
    list: *mut c_void,
    queue: *mut c_void,
    count: u32,
    last: *const u32,
    history: *const i32,
    next: *const BreadcrumbNode,
}
const _: () = assert!(core::mem::size_of::<BreadcrumbNode>() == core::mem::size_of::<D3D12_AUTO_BREADCRUMB_NODE>());
/// D3D12_DRED_ALLOCATION_NODE.
#[repr(C)]
struct AllocationNode {
    name_a: *const u8,
    name_w: *const u16,
    kind: i32,
    next: *const AllocationNode,
}
const _: () = assert!(core::mem::size_of::<AllocationNode>() == core::mem::size_of::<D3D12_DRED_ALLOCATION_NODE>());

unsafe fn log_breadcrumbs(dred: &ID3D12DeviceRemovedExtendedData) {
    let breadcrumbs = match dred.GetAutoBreadcrumbsOutput() {
        Ok(b) => b,
        Err(e) => {
            log!("[DRED] GetAutoBreadcrumbsOutput failed: hr=0x{:08X}\n", e.code().0 as u32);
            return;
        }
    };
    // The lists the GPU had not finished say where it stopped: those are logged, the finished ones counted.
    let (mut index, mut logged, mut finished) = (0u32, 0u32, 0u32);
    let mut node = breadcrumbs.pHeadAutoBreadcrumbNode as *const BreadcrumbNode;
    while let Some(n) = node.as_ref() {
        if index >= 8192 {
            break;
        }
        let last = if n.last.is_null() { u32::MAX } else { *n.last };
        if last != u32::MAX && last >= n.count {
            finished += 1;
        } else {
            logged += 1;
            if logged <= 64 {
                let op = if !n.history.is_null() && last < n.count {
                    *n.history.add(last as usize) as u32
                } else {
                    u32::MAX
                };
                let before = if !n.history.is_null() && last != u32::MAX && last > 0 && last <= n.count {
                    *n.history.add(last as usize - 1) as u32
                } else {
                    u32::MAX
                };
                log!(
                    "[DRED] Breadcrumb[{index}] queue={} list={} completed={last}/{} op={op}({}) after={before}({}) queuePtr={} listPtr={}\n",
                    debug_name(n.queue_name_a, n.queue_name_w),
                    debug_name(n.list_name_a, n.list_name_w),
                    n.count,
                    op_name(op),
                    op_name(before),
                    p(n.queue),
                    p(n.list)
                );
            }
        }
        node = n.next;
        index += 1;
    }
    log!(
        "[DRED] Breadcrumbs: {index} lists, {finished} finished, {logged} not finished ({} logged)\n",
        logged.min(64)
    );
}
unsafe fn log_allocations(label: &str, head: *const AllocationNode) {
    let mut index = 0u32;
    let mut node = head;
    while let Some(n) = node.as_ref() {
        if index >= 128 {
            break;
        }
        log!("[DRED] {label}[{index}] type={} name={}\n", n.kind as u32, debug_name(n.name_a, n.name_w));
        node = n.next;
        index += 1;
    }
}
unsafe fn log_page_fault(dred: &ID3D12DeviceRemovedExtendedData) {
    let fault = match dred.GetPageFaultAllocationOutput() {
        Ok(f) => f,
        Err(e) => {
            log!("[DRED] GetPageFaultAllocationOutput failed: hr=0x{:08X}\n", e.code().0 as u32);
            return;
        }
    };
    log!("[DRED] PageFaultVA=0x{:016X}\n", fault.PageFaultVA);
    log_allocations("ExistingAllocation", fault.pHeadExistingAllocationNode as *const AllocationNode);
    log_allocations("RecentFreedAllocation", fault.pHeadRecentFreedAllocationNode as *const AllocationNode);
}
unsafe fn dump(device: &ID3D12Device, present_hr: i32) {
    let reason = device.GetDeviceRemovedReason().map_or_else(|e| e.code().0, |()| 0);
    log!(
        "[DRED] Device removed detected. presentHr=0x{:08X} reason=0x{:08X} device={}\n",
        present_hr as u32,
        reason as u32,
        p(device.as_raw())
    );
    match device.cast::<ID3D12DeviceRemovedExtendedData>() {
        Ok(dred) => {
            log_breadcrumbs(&dred);
            log_page_fault(&dred);
        }
        Err(e) => log!("[DRED] ID3D12DeviceRemovedExtendedData unavailable: hr=0x{:08X}\n", e.code().0 as u32),
    }
}
/// DumpDredOnce: the first removal of the session, the one with a cause.
#[no_mangle]
pub unsafe extern "C" fn cvr_dred_dump_once(swap_chain: *mut c_void, present_hr: i32) {
    let device = {
        let mut t = TRACKED.lock().unwrap_or_else(|p| p.into_inner());
        if t.dumped {
            return;
        }
        t.dumped = true;
        t.device.clone()
    };
    let device = device.or_else(|| IDXGISwapChain::from_raw_borrowed(&swap_chain).and_then(|s| s.GetDevice::<ID3D12Device>().ok()));
    match device {
        Some(device) => dump(&device, present_hr),
        None => log!("[DRED] Present failed with hr=0x{:08X} but no D3D12 device is tracked\n", present_hr as u32),
    }
}

// ---------------------------------------------------------------- GPU diagnostics on request

/// CVR_GPU_DIAG: 0 off, 1 DRED (any value but "0..."), 2 with the debug layer ("debug").
fn diag_level() -> i32 {
    static LEVEL: OnceLock<i32> = OnceLock::new();
    *LEVEL.get_or_init(|| match std::env::var("CVR_GPU_DIAG") {
        Ok(v) if !v.is_empty() && v.len() < 32 && !v.starts_with('0') => {
            if v.contains("debug") {
                2
            } else {
                1
            }
        }
        _ => 0,
    })
}
/// " module+0xRVA" per frame (or the bare address), the way the C++ printed a captured stack.
unsafe fn format_stack(frames: &[*mut c_void]) -> String {
    let mut out = String::new();
    for &frame in frames {
        if out.len() + 64 >= 1024 {
            break;
        }
        let mut module = HMODULE::default();
        let mut name = [0u8; 260];
        if GetModuleHandleExA(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            PCSTR(frame as *const u8),
            &mut module,
        )
        .is_ok()
            && !module.is_invalid()
        {
            let n = GetModuleFileNameA(Some(module), &mut name) as usize;
            if n > 0 {
                let full = &name[..n];
                let base = full.iter().rposition(|&c| c == b'\\').map_or(full, |i| &full[i + 1..]);
                out.push_str(&format!(" {}+0x{:X}", String::from_utf8_lossy(base), frame as usize - module.0 as usize));
                continue;
            }
        }
        out.push_str(&format!(" {}", p(frame)));
    }
    out
}
unsafe extern "system" fn on_d3d12_message(
    category: D3D12_MESSAGE_CATEGORY,
    severity: D3D12_MESSAGE_SEVERITY,
    id: D3D12_MESSAGE_ID,
    description: PCSTR,
    _context: *mut c_void,
) {
    if severity.0 > D3D12_MESSAGE_SEVERITY_ERROR.0 {
        return;
    }
    // Each message the first few times; a resource released while the GPU still uses it every time.
    static SEEN: [AtomicU16; 4096] = [const { AtomicU16::new(0) }; 4096];
    let count = SEEN[(id.0 as u32 & 4095) as usize].fetch_add(1, Ordering::Relaxed);
    if count >= 4 && id != D3D12_MESSAGE_ID_OBJECT_DELETED_WHILE_STILL_IN_USE {
        return;
    }
    let text = if description.is_null() {
        String::new()
    } else {
        CStr::from_ptr(description.0 as *const c_char).to_string_lossy().into_owned()
    };
    log!(
        "[D3D12] {} id={} cat={} thread={}: {}\n",
        if severity == D3D12_MESSAGE_SEVERITY_CORRUPTION {
            "CORRUPTION"
        } else {
            "ERROR"
        },
        id.0 as u32,
        category.0 as u32,
        GetCurrentThreadId(),
        text
    );
    // The callback runs inside the call that made the message: its caller is the stack here.
    if count >= 2 {
        return;
    }
    let mut frames = [core::ptr::null_mut::<c_void>(); 32];
    let n = RtlCaptureStackBackTrace(1, &mut frames, None) as usize;
    log!("[D3D12]   stack:{}\n", format_stack(&frames[..n]));
}
/// CyberpunkVRPort_GpuDiagBeforeDevice: DRED, and with "debug" the debug layer, before the game's device is made.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_diag_before_device() {
    let level = diag_level();
    if level == 0 {
        return;
    }
    cvr_dred_enable_once();
    if level < 2 {
        return;
    }
    let d3d12 = GetModuleHandleA(PCSTR(c"d3d12.dll".as_ptr() as *const u8)).ok();
    let f = d3d12.and_then(|m| GetProcAddress(m, PCSTR(c"D3D12GetDebugInterface".as_ptr() as *const u8)));
    let debug = f.and_then(|f| get_debug_interface::<ID3D12Debug>(core::mem::transmute::<_, GetDebugInterface>(f)));
    match debug {
        Some(debug) => {
            debug.EnableDebugLayer();
            log!("[GPU diag] D3D12 debug layer enabled\n");
        }
        None => log!("[GPU diag] D3D12 debug layer unavailable\n"),
    }
}
struct Watch {
    device: ID3D12Device,
    _fence: ID3D12Fence,
    event: HANDLE,
}
// SAFETY: D3D12 objects are free-threaded; the event handle is the thread's to close.
unsafe impl Send for Watch {}
/// CyberpunkVRPort_GpuDiagDevice: the game's device, once: tracked for DRED, the list audit's allocator hook, the debug
/// layer's messages, and a removal watch (a removed device completes every fence to UINT64_MAX).
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_diag_device(raw: *mut c_void) {
    let level = diag_level();
    if level == 0 || raw.is_null() {
        return;
    }
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Some(device) = ID3D12Device::from_raw_borrowed(&raw).cloned() else {
        return;
    };
    cvr_dred_remember_device(raw);
    cvr_list_audit_install_allocator_hook(raw);
    if level >= 2 {
        let mut cookie = 0u32;
        let registered = device.cast::<ID3D12InfoQueue1>().ok().is_some_and(|q| {
            q.RegisterMessageCallback(Some(on_d3d12_message), D3D12_MESSAGE_CALLBACK_FLAG_NONE, core::ptr::null_mut(), &mut cookie)
                .is_ok()
        });
        if registered {
            log!("[GPU diag] debug messages logged (cookie {cookie})\n");
        } else {
            log!("[GPU diag] ID3D12InfoQueue1 unavailable: debug messages not logged\n");
        }
    }
    let event = CreateEventW(None, false, false, None).ok();
    let fence = device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE).ok();
    let armed = match (event, fence.as_ref()) {
        (Some(e), Some(f)) => f.SetEventOnCompletion(u64::MAX, e).is_ok(),
        _ => false,
    };
    let (Some(event), Some(fence), true) = (event, fence, armed) else {
        log!("[GPU diag] removal watch not armed\n");
        if let Some(e) = event {
            let _ = CloseHandle(e);
        }
        return;
    };
    let watch = Watch { device, _fence: fence, event };
    std::thread::spawn(move || {
        let watch = watch;
        WaitForSingleObject(watch.event, INFINITE);
        let _ = CloseHandle(watch.event);
        {
            let mut t = TRACKED.lock().unwrap_or_else(|p| p.into_inner());
            if t.dumped {
                return;
            }
            t.dumped = true;
        }
        dump(&watch.device, 0);
    });
    log!("[GPU diag] DRED level {level}, removal watch armed on device {}\n", p(raw));
}

// ---------------------------------------------------------------- the command-list audit

#[derive(Clone, Copy, Default)]
struct Stack {
    frames: [usize; 20],
    count: usize,
    thread: u32,
}
impl Stack {
    fn capture() -> Stack {
        let mut frames = [core::ptr::null_mut::<c_void>(); 20];
        // SAFETY: fills the array; two frames skipped as the C++ skipped its own.
        let count = unsafe { RtlCaptureStackBackTrace(2, &mut frames, None) } as usize;
        let mut s = Stack {
            count,
            thread: unsafe { GetCurrentThreadId() },
            ..Stack::default()
        };
        for i in 0..count {
            s.frames[i] = frames[i] as usize;
        }
        s
    }
    fn text(&self) -> String {
        let frames: Vec<*mut c_void> = self.frames[..self.count].iter().map(|&f| f as *mut c_void).collect();
        // SAFETY: module lookups by address.
        unsafe { format_stack(&frames) }
    }
}
#[derive(Default)]
struct AllocatorRecord {
    epoch: u64,
    reset: Stack,
}
#[derive(Default)]
struct ListRecord {
    allocator: usize,
    epoch: u64,
    open: bool,
    recorded: Stack,
}
#[derive(Default)]
struct Audit {
    allocators: FastMap<usize, AllocatorRecord>,
    lists: FastMap<usize, ListRecord>,
}
static AUDIT: Mutex<Option<Audit>> = Mutex::new(None);
fn with_audit<R>(f: impl FnOnce(&mut Audit) -> R) -> R {
    let mut guard = AUDIT.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(Audit::default))
}
static REPORTS: AtomicI32 = AtomicI32::new(0);
static mut ALLOCATOR_RESET_ORIGINAL: *mut c_void = core::ptr::null_mut();
static ALLOCATOR_HOOKED: AtomicUsize = AtomicUsize::new(0);

/// cvr::list_audit::Enabled: CVR_GPU_DIAG contains "lists".
#[no_mangle]
pub extern "C" fn cvr_list_audit_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("CVR_GPU_DIAG").is_ok_and(|v| !v.is_empty() && v.len() < 64 && v.contains("lists")))
}
unsafe extern "system" fn allocator_reset(this: *mut c_void) -> windows::core::HRESULT {
    type F = unsafe extern "system" fn(*mut c_void) -> windows::core::HRESULT;
    let result = core::mem::transmute::<*mut c_void, F>(ALLOCATOR_RESET_ORIGINAL)(this);
    if result.is_ok() {
        with_audit(|a| {
            let record = a.allocators.entry(this as usize).or_default();
            record.epoch += 1;
            record.reset = Stack::capture();
        });
    }
    result
}
/// cvr::list_audit::InstallAllocatorHook: ID3D12CommandAllocator::Reset (vtable slot 8), hooked once.
#[no_mangle]
pub unsafe extern "C" fn cvr_list_audit_install_allocator_hook(device: *mut c_void) {
    if !cvr_list_audit_enabled() || device.is_null() || ALLOCATOR_HOOKED.load(Ordering::SeqCst) != 0 {
        return;
    }
    let Some(device) = ID3D12Device::from_raw_borrowed(&device) else { return };
    let Ok(probe) = device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) else {
        log!("[list audit] no allocator to hook\n");
        return;
    };
    let target = *(*(probe.as_raw() as *const *const usize)).add(8);
    let detours = [cvr_engine_detour(target)];
    if cvr_engine::hooks::install_chain_on_any_build("list-audit", &detours).is_err() {
        log!("[list audit] allocator Reset hook failed at {}\n", p(target as *const c_void));
        ALLOCATOR_RESET_ORIGINAL = core::ptr::null_mut();
        return;
    }
    ALLOCATOR_HOOKED.store(target, Ordering::SeqCst);
    log!("[list audit] on: allocator Reset hooked at {}\n", p(target as *const c_void));
}
unsafe fn cvr_engine_detour(target: usize) -> cvr_engine::hooks::Detour {
    cvr_engine::hooks::Detour {
        name: "ID3D12CommandAllocator::Reset",
        target,
        absolute: true,
        detour: allocator_reset as *const c_void,
        original: core::ptr::addr_of_mut!(ALLOCATOR_RESET_ORIGINAL),
        expect: None,
    }
}
/// cvr::list_audit::ListReset: the list is recorded against its allocator's current epoch.
#[no_mangle]
pub extern "C" fn cvr_list_audit_list_reset(list: *mut c_void, allocator: *mut c_void) {
    if !cvr_list_audit_enabled() || list.is_null() {
        return;
    }
    with_audit(|a| {
        let epoch = if allocator.is_null() {
            0
        } else {
            a.allocators.entry(allocator as usize).or_default().epoch
        };
        let record = a.lists.entry(list as usize).or_default();
        record.allocator = allocator as usize;
        record.epoch = epoch;
        record.open = true;
        record.recorded = Stack::capture();
    });
}
/// cvr::list_audit::ListClose.
#[no_mangle]
pub extern "C" fn cvr_list_audit_list_close(list: *mut c_void) {
    if !cvr_list_audit_enabled() || list.is_null() {
        return;
    }
    with_audit(|a| {
        if let Some(record) = a.lists.get_mut(&(list as usize)) {
            record.open = false;
        }
    });
}
/// cvr::list_audit::Execute: a submitted list that is open, or whose allocator was reset after it was recorded.
#[no_mangle]
pub unsafe extern "C" fn cvr_list_audit_execute(queue: *mut c_void, count: u32, submitted: *const *mut c_void) {
    if !cvr_list_audit_enabled() || submitted.is_null() || ALLOCATOR_RESET_ORIGINAL.is_null() {
        return;
    }
    let lists = core::slice::from_raw_parts(submitted, count as usize);
    let mut submit: Option<Stack> = None;
    with_audit(|a| {
        for &list in lists {
            let Some(record) = a.lists.get(&(list as usize)) else { continue };
            let allocator = if record.allocator != 0 { a.allocators.get(&record.allocator) } else { None };
            let reset = allocator.is_some_and(|al| al.epoch != record.epoch);
            if !record.open && !reset {
                continue;
            }
            let submit = *submit.get_or_insert_with(Stack::capture);
            if REPORTS.fetch_add(1, Ordering::Relaxed) >= 8 {
                continue;
            }
            let what = if record.open {
                "list submitted while open"
            } else {
                "allocator reset after the list was recorded"
            };
            log!(
                "[list audit] {what}: queue={} list={} allocator={} list epoch={} allocator epoch={}\n",
                p(queue),
                p(list),
                p(record.allocator as *const c_void),
                record.epoch,
                allocator.map_or(0, |al| al.epoch)
            );
            log!("[list audit]   submitted by thread {}:{}\n", submit.thread, submit.text());
            log!(
                "[list audit]   recorded (Reset) by thread {}:{}\n",
                record.recorded.thread,
                record.recorded.text()
            );
            if let Some(al) = allocator {
                log!("[list audit]   allocator last reset by thread {}:{}\n", al.reset.thread, al.reset.text());
            }
        }
    });
}

// ---------------------------------------------------------------- the engine's Aftermath, on request

/// The diagnostic mode vrport_aftermath_debug.txt asks for (1, or 2 for the debug flag), and the resource trace.
static AFTERMATH_MODE: AtomicI32 = AtomicI32::new(1);
static RESOURCE_TRACE: AtomicBool = AtomicBool::new(false);
static mut AFTERMATH_INITIALIZE: *mut c_void = core::ptr::null_mut();
static mut AFTERMATH_CONFIGURE: *mut c_void = core::ptr::null_mut();

/// The registration's gate (src/Hooks/AftermathDiagnostics.cpp, PreDevice, order 5): the request file exists; its
/// number is the mode.
#[no_mangle]
pub extern "C" fn cvr_hook_aftermath_wanted() -> bool {
    let path = cvr_host::paths::file(cvr_host::paths::Folder::Config, "vrport_aftermath_debug.txt");
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    let mode = text.split_whitespace().next().and_then(|t| t.parse::<i32>().ok()).unwrap_or(1);
    AFTERMATH_MODE.store(if mode == 2 { 2 } else { 1 }, Ordering::SeqCst);
    RESOURCE_TRACE.store(true, Ordering::Relaxed);
    true
}
unsafe extern "C" fn aftermath_initialize(device: *mut c_void, requested_debug: u8) -> u8 {
    type F = unsafe extern "C" fn(*mut c_void, u8) -> u8;
    log!("[aftermath-debug] entering native initialization device={}\n", p(device));
    let result = core::mem::transmute::<*mut c_void, F>(AFTERMATH_INITIALIZE)(device, requested_debug);
    log!(
        "[aftermath-debug] native initialization debug={requested_debug} result={result} device={}\n",
        p(device)
    );
    result
}
unsafe extern "C" fn aftermath_configure(device: *mut c_void, requested: *const u8) -> i64 {
    type F = unsafe extern "C" fn(*mut c_void, *const u8) -> i64;
    let options = [1u8, (AFTERMATH_MODE.load(Ordering::SeqCst) == 2) as u8];
    log!("[aftermath-debug] entering native device setup device={}\n", p(device));
    let result = core::mem::transmute::<*mut c_void, F>(AFTERMATH_CONFIGURE)(device, options.as_ptr());
    let (a, b) = if requested.is_null() { (0, 0) } else { (*requested, *requested.add(1)) };
    log!("[aftermath-debug] native device setup enable={a} debug={b} -> 1,{} complete\n", options[1]);
    result
}
/// The engine's Aftermath initialisation (0x2FA9008) and device setup (0x688D5C): the setup always enables it, with
/// the debug flag in mode 2.
#[no_mangle]
pub unsafe extern "C" fn cvr_hook_aftermath_install() -> bool {
    let detours = [
        cvr_engine::hooks::Detour {
            name: "Aftermath initialisation sub_142FA9008",
            target: 0x2FA9008,
            absolute: false,
            detour: aftermath_initialize as *const c_void,
            original: core::ptr::addr_of_mut!(AFTERMATH_INITIALIZE),
            expect: Some("48 83 EC 28 48 8B 05 BD 4C C2 00 48 85 C0 74 31 F6 DA 4C 8B C1 B9 09 02"),
        },
        cvr_engine::hooks::Detour {
            name: "Aftermath device setup sub_140688D5C",
            target: 0x688D5C,
            absolute: false,
            detour: aftermath_configure as *const c_void,
            original: core::ptr::addr_of_mut!(AFTERMATH_CONFIGURE),
            expect: Some("4C 8B DC 49 89 5B 18 49 89 73 20 57 48 83 EC 20 48 8B F2 48 8B F9"),
        },
    ];
    cvr_engine::hooks::install_chain_on_any_build("aftermath-diagnostics", &detours).is_ok()
}

/// One traced resource.
#[derive(Clone, Copy, Default)]
struct ResourceSample {
    resource: usize,
    desc: D3D12_RESOURCE_DESC,
    tick: u64,
    serial: u64,
}
// SAFETY: plain data (the resource only as an address).
unsafe impl Send for ResourceSample {}
static RESOURCES: Mutex<Option<FastMap<usize, ResourceSample>>> = Mutex::new(None);
static RESOURCE_SERIAL: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// The resource vtables whose Release is patched (at most 16): the table, then its original Release.
static RELEASE_INSTALL: Mutex<()> = Mutex::new(());
static RELEASE_TABLE: [AtomicUsize; 32] = [const { AtomicUsize::new(0) }; 32];
static RELEASE_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe fn trace_resource(event: &str, sample: &ResourceSample, status: u32) {
    let mut frames = [core::ptr::null_mut::<c_void>(); 14];
    let count = RtlCaptureStackBackTrace(2, &mut frames, None) as usize;
    let base = GetModuleHandleA(PCSTR::null()).map_or(0, |m| m.0 as usize);
    let nt = base + *((base + 0x3C) as *const u32) as usize;
    let end = base + *((nt + 0x18 + 0x38) as *const u32) as usize;
    let mut stack = String::new();
    for &frame in &frames[..count] {
        if stack.len() + 32 >= 450 {
            break;
        }
        let a = frame as usize;
        if a >= base && a < end {
            stack.push_str(&format!("exe+{:X} ", a - base));
        } else {
            stack.push_str(&format!("{} ", p(frame)));
        }
    }
    let now = windows::Win32::System::SystemInformation::GetTickCount64();
    let d = &sample.desc;
    log!(
        "[resource-life] {event} id={} resource={} handle={} tick={now} age={} tid={} dim={} size={}x{}x{} mip={} fmt={} flags={:X} status={:X} stack={stack}\n",
        sample.serial,
        p(sample.resource as *const c_void),
        p(core::ptr::null()),
        now - sample.tick,
        GetCurrentThreadId(),
        d.Dimension.0 as u32,
        d.Width,
        d.Height,
        d.DepthOrArraySize as u32,
        d.MipLevels as u32,
        d.Format.0 as u32,
        d.Flags.0 as u32,
        status
    );
}
unsafe extern "system" fn release_resource(resource: *mut c_void) -> u32 {
    type F = unsafe extern "system" fn(*mut c_void) -> u32;
    let table = *(resource as *const usize);
    let count = RELEASE_COUNT.load(Ordering::Acquire);
    let mut release = 0usize;
    for i in 0..count {
        if RELEASE_TABLE[2 * i].load(Ordering::Relaxed) == table {
            release = RELEASE_TABLE[2 * i + 1].load(Ordering::Relaxed);
            break;
        }
    }
    if release == 0 {
        return 0; // never exposed before its original is published
    }
    let sample = {
        let guard = RESOURCES.lock().unwrap_or_else(|p| p.into_inner());
        guard.as_ref().and_then(|m| m.get(&(resource as usize)).copied()).unwrap_or_default()
    };
    let remaining = core::mem::transmute::<usize, F>(release)(resource);
    if remaining == 0 && sample.resource != 0 {
        {
            let mut guard = RESOURCES.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(m) = guard.as_mut() {
                if m.get(&(resource as usize)).is_some_and(|s| s.serial == sample.serial) {
                    m.remove(&(resource as usize));
                }
            }
        }
        trace_resource("final-release", &sample, remaining);
    }
    remaining
}
/// The resource's vtable Release (slot 2) replaced once per vtable, its original published first.
unsafe fn observe_final_release(resource: usize) -> bool {
    use windows::Win32::System::Memory::{VirtualProtect, PAGE_PROTECTION_FLAGS, PAGE_READWRITE};
    let table = *(resource as *const usize);
    let _guard = RELEASE_INSTALL.lock().unwrap_or_else(|p| p.into_inner());
    let count = RELEASE_COUNT.load(Ordering::Relaxed);
    for i in 0..count {
        if RELEASE_TABLE[2 * i].load(Ordering::Relaxed) == table {
            return true;
        }
    }
    if count == 16 {
        return false;
    }
    let slot = (table + 2 * 8) as *mut usize;
    let mut protection = PAGE_PROTECTION_FLAGS(0);
    if VirtualProtect(slot as *const c_void, 8, PAGE_READWRITE, &mut protection).is_err() {
        return false;
    }
    RELEASE_TABLE[2 * count].store(table, Ordering::Relaxed);
    RELEASE_TABLE[2 * count + 1].store(*slot, Ordering::Relaxed);
    RELEASE_COUNT.store(count + 1, Ordering::Release);
    (*(slot as *const AtomicUsize)).swap(release_resource as *const () as usize, Ordering::SeqCst);
    let mut ignored = PAGE_PROTECTION_FLAGS(0);
    let _ = VirtualProtect(slot as *const c_void, 8, protection, &mut ignored);
    true
}
/// cvr::diagnostics::ObserveResourceCreation: large R32 depth textures and RGBA16F volumes (the first 256) traced from
/// creation to their final release, with the creating and releasing stacks.
#[no_mangle]
pub unsafe extern "C" fn cvr_aftermath_observe_resource_creation(resource: *mut c_void, desc: *const D3D12_RESOURCE_DESC) {
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16G16B16A16_FLOAT, DXGI_FORMAT_R32_TYPELESS};
    if !RESOURCE_TRACE.load(Ordering::Relaxed) || resource.is_null() || desc.is_null() {
        return;
    }
    let d = *desc;
    let depth = d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE2D && d.Format == DXGI_FORMAT_R32_TYPELESS && d.Width >= 512 && d.Height >= 512;
    let volume = d.Dimension == D3D12_RESOURCE_DIMENSION_TEXTURE3D && d.Format == DXGI_FORMAT_R16G16B16A16_FLOAT && d.Width >= 128 && d.Width <= 1024;
    if !depth && !volume {
        return;
    }
    let serial = RESOURCE_SERIAL.fetch_add(1, Ordering::Relaxed) + 1;
    if serial > 256 || !observe_final_release(resource as usize) {
        return;
    }
    let sample = ResourceSample {
        resource: resource as usize,
        desc: d,
        tick: windows::Win32::System::SystemInformation::GetTickCount64(),
        serial,
    };
    RESOURCES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get_or_insert_with(cvr_host::fast::fast_map)
        .insert(resource as usize, sample);
    trace_resource("create", &sample, 0);
}
