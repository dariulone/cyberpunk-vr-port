//! WHAT THE DEVICE CREATES, AND THE SLOTS IT IS WATCHED THROUGH (formerly src/Stereo/DeviceHooks.cpp, part): the
//! device's vtable patched once -- the creation hooks here, the pipeline-state, command-list, queue, CBV and RTV hooks
//! of the C++ half handed in with the slots for their originals --; the descriptor heaps (the shader-visible heap's
//! size constant and enlargement, both off), the committed and placed resources and the root signatures created, each
//! announced to the readers that keep tables of them; the creation counts and the time inside the runtime, the busiest
//! creation stacks (CyberpunkVR_CreationStacks), the video memory against its budget, and streamed resources placed
//! into the allocator's heaps (CyberpunkVR_StreamingPlaced).
//!
//! Everything here is a creation-time hook: it runs before the frame loop and none of it is on a per-frame path.
#![allow(non_upper_case_globals)]
use cvr_host::fast::FastMap;
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering::*};
use std::ffi::CString;
use std::sync::Mutex;

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12Device2, ID3D12Device4, ID3D12Device5, ID3D12Heap,
    D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
    D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_HEAP_PROPERTIES,
    D3D12_RESOURCE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};

/// A hook of the C++ half and the slot its original goes into.
#[repr(C)]
pub struct CppHook {
    hook: *const c_void,
    original: *mut *const c_void,
}

/// The hooks, slots, flags and readers the device's creations are told to (built in `table`).
#[repr(C)]
pub struct Runtime {
    /// Slot 8 (only with the execute probe), 12, 10, 11, ID3D12Device2 47, ID3D12Device5 62, 17, 20.
    create_command_queue: CppHook,
    create_command_list: CppHook,
    create_graphics_pso: CppHook,
    create_compute_pso: CppHook,
    create_pipeline_state: CppHook,
    create_state_object: CppHook,
    create_cbv: CppHook,
    create_rtv: CppHook,
    /// g_orig_CreateDescriptorHeap: the C++ half's, other files call through it.
    original_create_descriptor_heap: *mut *const c_void,
    rtv_hook_installed: *const AtomicBool,
    enable_exec_probe: *const bool,
    desc_vtable_patched: *const AtomicBool,
    enable_desc_heap_enlarge: *const bool,
    desc_heap_target: *const u32,
    enable_desc_heap_resize: *const bool,
    desc_heap_resized: *const AtomicBool,
    /// constants::SetReader and InstallDepthCaptureHooks, before a slot is patched.
    install_begin: extern "C" fn(*mut c_void),
    read_observation_unavailable: extern "C" fn(),
    /// descriptors::BorrowedRangeObserverReady when the audit is on, with whether the CBV and heap slots are ours.
    descriptors_ready: extern "C" fn(*mut c_void, bool),
    /// descriptors::MutationScope, entered and left around a heap's creation.
    mutation_enter: extern "C" fn() -> *mut c_void,
    mutation_leave: extern "C" fn(*mut c_void),
    /// The readers of a committed resource (cascades, constants, texture and buffer reads, geometry, the buffer map,
    /// the diagnostics) and of a placed one.
    committed_created: extern "C" fn(*mut c_void, *const D3D12_RESOURCE_DESC, i32),
    placed_created:
        extern "C" fn(*mut c_void, *mut c_void, u64, u64, *const D3D12_RESOURCE_DESC, i32),
    /// families, the native probe and single pass, on a root signature.
    root_created: extern "C" fn(*mut c_void, *mut c_void, u32, *const c_void, usize),
}
// SAFETY: immutable once built; its pointers name process-lifetime hooks, slots and flags.
unsafe impl Sync for Runtime {}
// SAFETY: as above.
unsafe impl Send for Runtime {}
fn rt() -> &'static Runtime {
    &table::RT
}
fn r<T>(p: *const T) -> &'static T {
    // SAFETY: the C++ half's globals, alive for the process.
    unsafe { &*p }
}

extern "C" {
    static CyberpunkVR_CreationCounts: [AtomicU64; 15];
    static CyberpunkVR_VideoMemoryMB: [AtomicU64; 2];
    static CyberpunkVR_StreamingPlaced: i32;
    static mut CyberpunkVR_StreamingPlacedState: u32;
    static CyberpunkVR_CreationStacks: i32;
    static mut CyberpunkVR_DebugDescHeapResized: u32;
    static CyberpunkVR_DebugDescHeapCreates: AtomicU64;
    static mut CyberpunkVR_DebugDescHeapSVNum: u32;
    static mut CyberpunkVR_DebugDescHeapSVRetRva: usize;
    static mut CyberpunkVR_DebugDescHeapSVRetAbs: usize;
    static mut CyberpunkVR_DebugDescHeapSVFlags: u32;
    static CyberpunkVR_DebugDescHeapEnlarged: AtomicU64;
    static CyberpunkVR_RuntimeDiagnostics: i32;
    /// `cvr::log("%s", text)`: "[stereo] " before and a newline after.
    fn cvr_cpp_stereo_log(text: *const c_char);
}
fn log(text: &str) {
    if let Ok(text) = CString::new(text) {
        // SAFETY: a C++ service taking a NUL-terminated string.
        unsafe { cvr_cpp_stereo_log(text.as_ptr()) };
    }
}
fn diagnostics() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_RuntimeDiagnostics) != 0 }
}
fn counts() -> &'static [AtomicU64; 15] {
    // SAFETY: an export of the C++ half.
    unsafe { &*(&raw const CyberpunkVR_CreationCounts) }
}
fn p(v: usize) -> String {
    format!("{v:016X}")
}
fn exe_base() -> usize {
    // SAFETY: the process's own module.
    unsafe {
        windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as usize
    }
}
/// The address the hook returns to (`_ReturnAddress()`): frame 0 is this hook, frame 1 its caller.
#[inline(always)]
fn return_address() -> usize {
    let mut frames = [core::ptr::null_mut::<c_void>(); 1];
    // SAFETY: a back trace into a local buffer.
    let n = unsafe {
        windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, &mut frames, None)
    };
    if n == 0 {
        0
    } else {
        frames[0] as usize
    }
}

// ---- the counts -------------------------------------------------------------------------------------------------------

fn clock() -> u64 {
    let mut t = 0i64;
    // SAFETY: a plain counter read.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut t) };
    t as u64
}
fn count_time(i: usize, start: u64) {
    static FREQUENCY: AtomicU64 = AtomicU64::new(0);
    let mut f = FREQUENCY.load(Relaxed);
    if f == 0 {
        let mut q = 0i64;
        // SAFETY: a plain frequency read.
        unsafe { windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut q) };
        f = q.max(1) as u64;
        FREQUENCY.store(f, Relaxed);
    }
    counts()[i].fetch_add(
        (clock().wrapping_sub(start)).wrapping_mul(10_000_000) / f,
        Relaxed,
    );
}
fn module_of(address: usize) -> usize {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
        GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    let mut m = core::ptr::null_mut();
    // SAFETY: an address query.
    unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address as *const u16,
            &mut m,
        )
    };
    m as usize
}
/// CountCreation: the count, the bytes after it, and for a committed or placed resource who asked (the port, the
/// game's exe, or someone else: drivers, Streamline, DLSS).
#[no_mangle]
pub extern "C" fn cvr_count_creation(i: usize, bytes: u64, caller: *const c_void) {
    let c = counts();
    c[i].fetch_add(1, Relaxed);
    if bytes != 0 && i + 1 < 8 {
        c[i + 1].fetch_add(bytes, Relaxed);
    }
    if !caller.is_null() && (i == 0 || i == 2) {
        static PORT: AtomicUsize = AtomicUsize::new(0);
        let mut port = PORT.load(Relaxed);
        if port == 0 {
            port = module_of(counts() as *const _ as usize);
            PORT.store(port, Relaxed);
        }
        let m = module_of(caller as usize);
        if m == port {
            c[if i == 0 { 8 } else { 10 }].fetch_add(1, Relaxed);
        } else if m == exe_base() {
            c[if i == 0 { 9 } else { 11 }].fetch_add(1, Relaxed);
        }
    }
}

/// The busiest creation stacks: the first exe frames of the call stack plus the resource's dimension, heap type and
/// size class; the 16 busiest to the log every 4096 creations.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
struct CreationKey {
    rva: [u32; 4],
    kind: u32,
}
#[derive(Clone, Copy, Default)]
struct CreationStat {
    count: u64,
    bytes: u64,
}
struct Stacks {
    map: FastMap<CreationKey, CreationStat>,
    logged: u64,
}
static STACKS: Mutex<Option<Stacks>> = Mutex::new(None);

#[inline(never)]
fn note_creation_stack(
    placed: bool,
    d: *const D3D12_RESOURCE_DESC,
    hp: *const D3D12_HEAP_PROPERTIES,
    bytes: u64,
) {
    let exe = exe_base();
    // SAFETY: the process's own PE headers.
    let exe_size = unsafe {
        let dos = exe as *const windows_sys::Win32::System::SystemServices::IMAGE_DOS_HEADER;
        let nt = (exe + (*dos).e_lfanew as usize)
            as *const windows_sys::Win32::System::Diagnostics::Debug::IMAGE_NT_HEADERS64;
        (*nt).OptionalHeader.SizeOfImage as usize
    };
    let mut frames = [core::ptr::null_mut::<c_void>(); 24];
    // SAFETY: a back trace into a local buffer: this function and the hook skipped.
    let n = unsafe {
        windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(2, &mut frames, None)
    };
    let mut key = CreationKey::default();
    let mut k = 0;
    for f in &frames[..n as usize] {
        if k >= 4 {
            break;
        }
        let a = *f as usize;
        if a >= exe && a < exe + exe_size {
            key.rva[k] = (a - exe) as u32;
            k += 1;
        }
    }
    // SAFETY: the caller's descriptions or null.
    let (dim, heap_type, flags) = unsafe {
        (
            d.as_ref().map_or(0, |d| d.Dimension.0 as u32),
            hp.as_ref().map_or(0, |h| h.Type.0 as u32),
            d.as_ref().map_or(0, |d| (d.Flags.0 as u32) & 0xFF),
        )
    };
    let mut size_class = 0u32;
    let mut b = bytes;
    while b > 65536 && size_class < 15 {
        b >>= 2;
        size_class += 1;
    }
    key.kind = (placed as u32) | dim << 1 | heap_type << 4 | size_class << 8 | flags << 16;
    let top: Vec<(CreationKey, CreationStat)>;
    let logged;
    {
        let mut g = STACKS.lock().unwrap_or_else(|p| p.into_inner());
        let s = g.get_or_insert_with(|| Stacks {
            map: cvr_host::fast::fast_map(),
            logged: 0,
        });
        if s.map.len() < 4096 || s.map.contains_key(&key) {
            let stat = s.map.entry(key).or_default();
            stat.count += 1;
            stat.bytes += bytes;
        }
        s.logged += 1;
        if s.logged % 4096 != 0 {
            return;
        }
        logged = s.logged;
        top = s.map.iter().map(|(k, v)| (*k, *v)).collect();
    }
    let mut top = top;
    top.sort_by(|a, b| b.1.count.cmp(&a.1.count));
    log(&format!(
        "[creation] {logged} committed/placed creations, {} distinct stacks; busiest:",
        top.len()
    ));
    for (k, v) in top.iter().take(16) {
        log(&format!(
            "[creation]  {:7} x {} dim={} heap={} size<={}K flags=0x{:02X} avg={}K exe+{:X} <- {:X} <- {:X} <- {:X}",
            v.count,
            if k.kind & 1 != 0 { "placed" } else { "committed" },
            k.kind >> 1 & 7,
            k.kind >> 4 & 15,
            64u64 << (2 * (k.kind >> 8 & 15)),
            k.kind >> 16 & 0xFF,
            v.bytes / v.count / 1024,
            k.rva[0],
            k.rva[1],
            k.rva[2],
            k.rva[3]
        ));
    }
}

// ---- the video memory -----------------------------------------------------------------------------------------------

/// The process's video memory against its budget, local and non-local: every 256th committed creation and every
/// 16384th submission (~17 s).
fn log_video_memory(device: *mut c_void, when: &str, count: u64) {
    static ADAPTER: AtomicUsize = AtomicUsize::new(0);
    static TRIED: AtomicBool = AtomicBool::new(false);
    if !TRIED.swap(true, AcqRel) {
        // SAFETY: the caller's device; a DXGI factory of our own.
        unsafe {
            if let Some(d) = ID3D12Device::from_raw_borrowed(&device) {
                if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory4>() {
                    if let Ok(a) = factory.EnumAdapterByLuid::<IDXGIAdapter3>(d.GetAdapterLuid()) {
                        ADAPTER.store(a.into_raw() as usize, Release);
                    }
                }
            }
        }
    }
    let raw = ADAPTER.load(Acquire) as *mut c_void;
    // SAFETY: the adapter kept for the process.
    let Some(adapter) = (unsafe { IDXGIAdapter3::from_raw_borrowed(&raw) }) else {
        return;
    };
    let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    let mut shared = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    // SAFETY: plain queries into locals.
    unsafe {
        let _ = adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local);
        let _ = adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, &mut shared);
    }
    // SAFETY: an export of the C++ half.
    let mb = unsafe { &*(&raw const CyberpunkVR_VideoMemoryMB) };
    mb[0].store(local.CurrentUsage >> 20, Relaxed);
    mb[1].fetch_max(local.CurrentUsage >> 20, Relaxed);
    let c = counts();
    if !cvr_host::log::verbose() {
        return;
    }
    log(&format!(
        "[creation] video memory after {count} {when}: local {} / {} MB budget, non-local {} / {} MB; since launch {} graphics pipelines, {} with a mesh shader",
        local.CurrentUsage >> 20,
        local.Budget >> 20,
        shared.CurrentUsage >> 20,
        shared.Budget >> 20,
        c[5].load(Relaxed),
        c[14].load(Relaxed)
    ));
}
fn note_video_memory(device: *mut c_void) {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let n = CALLS.fetch_add(1, Relaxed) + 1;
    if n % 256 == 1 {
        log_video_memory(device, "committed", n);
    }
}
/// NoteSubmitVideoMemory, from the queue's ExecuteCommandLists.
///
/// # Safety
/// `queue` is an ID3D12CommandQueue.
#[no_mangle]
pub unsafe extern "C" fn cvr_note_submit_video_memory(queue: *mut c_void) {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let n = CALLS.fetch_add(1, Relaxed) + 1;
    if n % 16384 != 0 {
        return;
    }
    // SAFETY: the caller's queue.
    unsafe {
        if let Some(q) =
            windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue::from_raw_borrowed(&queue)
        {
            let mut device: Option<ID3D12Device> = None;
            if q.GetDevice(&mut device).is_ok() {
                if let Some(device) = device {
                    log_video_memory(device.as_raw(), "submissions", n);
                }
            }
        }
    }
}

// ---- code constants -------------------------------------------------------------------------------------------------

fn write_code_u32(address: usize, value: u32) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_EXECUTE_READWRITE};
    let mut old = 0u32;
    // SAFETY: the exe's own code, made writable for four bytes and restored.
    unsafe {
        if VirtualProtect(
            address as *const c_void,
            4,
            PAGE_EXECUTE_READWRITE,
            &mut old,
        ) == 0
        {
            return false;
        }
        core::ptr::write_unaligned(address as *mut u32, value);
        let mut junk = 0u32;
        VirtualProtect(address as *const c_void, 4, old, &mut junk);
        windows_sys::Win32::System::Diagnostics::Debug::FlushInstructionCache(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
            address as *const c_void,
            4,
        );
    }
    true
}

/// STREAMED RESOURCES INTO THE ALLOCATOR'S HEAPS: the engine's dedicated-allocation limits (textures over 256 x 256
/// texels, buffers over 256 KB) raised to 16 M texels and 32 MB, the original bytes checked first.
fn apply_streaming_placed(on: bool) {
    struct Site {
        rva: usize,
        opcode: &'static [u8],
        original: u32,
        raised: u32,
    }
    const SITES: [Site; 2] = [
        Site {
            rva: 0x3480EC,
            opcode: &[0x81, 0xF9],
            original: 0x0001_0000,
            raised: 0x0100_0000,
        },
        Site {
            rva: 0x3448E3,
            opcode: &[0x3D],
            original: 0x0003_FFFF,
            raised: 0x01FF_FFFF,
        },
    ];
    let exe = exe_base();
    for s in &SITES {
        let code = exe + s.rva;
        // SAFETY: the exe's own code, always readable.
        let (bytes, current) = unsafe {
            (
                core::slice::from_raw_parts(code as *const u8, s.opcode.len()),
                core::ptr::read_unaligned((code + s.opcode.len()) as *const u32),
            )
        };
        if bytes != s.opcode || (current != s.original && current != s.raised) {
            // SAFETY: an export of the C++ half.
            unsafe {
                core::ptr::write_volatile(&raw mut CyberpunkVR_StreamingPlacedState, 0xFFFF_FFFF)
            };
            log(&format!(
                "[creation] streaming-placed: bytes at exe+{:X} differ (imm 0x{current:X}), not patched",
                s.rva
            ));
            return;
        }
    }
    for s in &SITES {
        write_code_u32(
            exe + s.rva + s.opcode.len(),
            if on { s.raised } else { s.original },
        );
    }
    // SAFETY: as above.
    unsafe {
        core::ptr::write_volatile(
            &raw mut CyberpunkVR_StreamingPlacedState,
            if on { 1 } else { 2 },
        )
    };
    log(&format!(
        "[creation] streaming-placed {}: texture limit 0x{:X} texels, buffer limit 0x{:X} bytes",
        if on { "on" } else { "off" },
        if on {
            SITES[0].raised
        } else {
            SITES[0].original
        },
        (if on {
            SITES[1].raised
        } else {
            SITES[1].original
        }) + 1
    ));
}
/// PollStreamingPlaced, from the queue's ExecuteCommandLists: applied when the key changes.
#[no_mangle]
pub extern "C" fn cvr_poll_streaming_placed() {
    static APPLIED: AtomicI32 = AtomicI32::new(0);
    // SAFETY: an export of the C++ half.
    let wanted =
        (unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_StreamingPlaced) } != 0) as i32;
    let seen = APPLIED.load(Relaxed);
    if seen != wanted
        && APPLIED
            .compare_exchange(seen, wanted, AcqRel, Relaxed)
            .is_ok()
    {
        apply_streaming_placed(wanted != 0);
    }
}

/// The shader-visible heap's base constant (mov eax, 0xDC240), raised so two full views fit -- OFF: the runtime
/// refuses a shader-visible heap past 1,000,000 descriptors.
const DESC_HEAP_SIZE_MOV_RVA: usize = 0x8EB88A;
const DESC_HEAP_SIZE_ORIG: u32 = 0x000D_C240;
const DESC_HEAP_SIZE_NEW: u32 = 0x001D_C240;
#[no_mangle]
pub extern "C" fn cvr_patch_descriptor_heap_size() {
    let rt = rt();
    // SAFETY: the C++ half's bool.
    if !unsafe { *rt.enable_desc_heap_resize } {
        return;
    }
    if r(rt.desc_heap_resized)
        .compare_exchange(false, true, AcqRel, Relaxed)
        .is_err()
    {
        return;
    }
    let mov = exe_base() + DESC_HEAP_SIZE_MOV_RVA;
    let (Some(op), Some(imm)) = (
        cvr_engine::code::read::<u8>(mov),
        cvr_engine::code::read::<u32>(mov + 1),
    ) else {
        // SAFETY: an export of the C++ half.
        unsafe {
            core::ptr::write_volatile(&raw mut CyberpunkVR_DebugDescHeapResized, 0xEEEE_EEEE)
        };
        return;
    };
    if op != 0xB8 || imm != DESC_HEAP_SIZE_ORIG {
        // SAFETY: as above.
        unsafe {
            core::ptr::write_volatile(&raw mut CyberpunkVR_DebugDescHeapResized, 0xFFFF_FFFF)
        };
        log(&format!(
            "[descheap] size-const validation FAILED at {} (op=0x{op:02X} imm=0x{imm:X})",
            p(mov)
        ));
        return;
    }
    if !write_code_u32(mov + 1, DESC_HEAP_SIZE_NEW) {
        return;
    }
    // SAFETY: as above.
    unsafe {
        core::ptr::write_volatile(
            &raw mut CyberpunkVR_DebugDescHeapResized,
            DESC_HEAP_SIZE_NEW,
        )
    };
    log(&format!(
        "[descheap] heap size const patched 0x{DESC_HEAP_SIZE_ORIG:X} -> 0x{DESC_HEAP_SIZE_NEW:X} at {}",
        p(mov)
    ));
}

// ---- the hooks --------------------------------------------------------------------------------------------------------

type CreateDescriptorHeapFn = unsafe extern "system" fn(
    *mut c_void,
    *const D3D12_DESCRIPTOR_HEAP_DESC,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
type CreateCommittedFn = unsafe extern "system" fn(
    *mut c_void,
    *const D3D12_HEAP_PROPERTIES,
    i32,
    *const D3D12_RESOURCE_DESC,
    i32,
    *const c_void,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
type CreateCommitted1Fn = unsafe extern "system" fn(
    *mut c_void,
    *const D3D12_HEAP_PROPERTIES,
    i32,
    *const D3D12_RESOURCE_DESC,
    i32,
    *const c_void,
    *mut c_void,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
type CreatePlacedFn = unsafe extern "system" fn(
    *mut c_void,
    *mut c_void,
    u64,
    *const D3D12_RESOURCE_DESC,
    i32,
    *const c_void,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;
type CreateRootSignatureFn = unsafe extern "system" fn(
    *mut c_void,
    u32,
    *const c_void,
    usize,
    *const GUID,
    *mut *mut c_void,
) -> HRESULT;

static ORIGINAL_COMMITTED: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_COMMITTED1: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_PLACED: AtomicUsize = AtomicUsize::new(0);
static ORIGINAL_ROOT_SIGNATURE: AtomicUsize = AtomicUsize::new(0);

/// ID3D12Device::GetResourceAllocationInfo(0, 1, desc).SizeInBytes, or 0 without a description.
fn allocation_bytes(device: *mut c_void, desc: *const D3D12_RESOURCE_DESC) -> u64 {
    // SAFETY: the hook's device and the caller's description.
    unsafe {
        match (ID3D12Device::from_raw_borrowed(&device), desc.as_ref()) {
            (Some(d), Some(desc)) => d.GetResourceAllocationInfo(0, &[*desc]).SizeInBytes,
            _ => 0,
        }
    }
}
fn stacks_on() -> bool {
    // SAFETY: an export of the C++ half.
    unsafe { core::ptr::read_volatile(&raw const CyberpunkVR_CreationStacks) != 0 }
}

unsafe extern "system" fn create_descriptor_heap(
    this: *mut c_void,
    desc: *const D3D12_DESCRIPTOR_HEAP_DESC,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let ret_abs = return_address();
    cvr_count_creation(4, 0, core::ptr::null());
    let rt = rt();
    let mutation = (rt.mutation_enter)();
    if diagnostics() {
        // SAFETY: an export of the C++ half.
        unsafe { &*(&raw const CyberpunkVR_DebugDescHeapCreates) }.fetch_add(1, Relaxed);
    }
    let mut local;
    let mut use_desc = desc;
    // SAFETY: the caller's description or null.
    if let Some(d) = unsafe { desc.as_ref() } {
        let shader_visible = (d.Flags.0 & D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE.0) != 0;
        if d.Type == D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV
            && shader_visible
            && d.NumDescriptors >= 0x40000
        {
            let rva = ret_abs.wrapping_sub(exe_base());
            // SAFETY: exports of the C++ half.
            unsafe {
                core::ptr::write_volatile(
                    &raw mut CyberpunkVR_DebugDescHeapSVNum,
                    d.NumDescriptors,
                );
                core::ptr::write_volatile(&raw mut CyberpunkVR_DebugDescHeapSVRetRva, rva);
                core::ptr::write_volatile(&raw mut CyberpunkVR_DebugDescHeapSVRetAbs, ret_abs);
                core::ptr::write_volatile(
                    &raw mut CyberpunkVR_DebugDescHeapSVFlags,
                    d.Flags.0 as u32,
                );
            }
            log(&format!(
                "[descheap] shader-visible CBV_SRV_UAV num={} flags=0x{:X} caller_abs=0x{ret_abs:X} rva=0x{rva:X}",
                d.NumDescriptors, d.Flags.0
            ));
            // SAFETY: the C++ half's settings.
            let (enlarge, target) = unsafe { (*rt.enable_desc_heap_enlarge, *rt.desc_heap_target) };
            if enlarge && d.NumDescriptors < target {
                local = *d;
                local.NumDescriptors = target;
                use_desc = &local;
                if diagnostics() {
                    // SAFETY: as above.
                    unsafe { &*(&raw const CyberpunkVR_DebugDescHeapEnlarged) }
                        .fetch_add(1, Relaxed);
                }
                log(&format!("[descheap] enlarged -> {target}"));
            }
        }
    }
    // SAFETY: the original, saved before the slot was replaced.
    let result = unsafe {
        let original: CreateDescriptorHeapFn =
            core::mem::transmute(*rt.original_create_descriptor_heap);
        original(this, use_desc, riid, out)
    };
    // SAFETY: the caller's out pointer and the description used.
    unsafe {
        if result.is_ok() && !out.is_null() && !(*out).is_null() {
            // descriptors::HeapCreated: the descriptor audit's (formerly the runtime's callback to the C++ facade).
            crate::descriptor_audit::cvr_sda_audit_write(1, this, 0, 0, 0, 0, *out, core::ptr::null_mut());
        }
        if result.is_ok() {
            if let Some(u) = use_desc.as_ref() {
                if u.Type == D3D12_DESCRIPTOR_HEAP_TYPE_RTV {
                    crate::capture_rtv::cvr_capture_rtv_heap_created(u.NumDescriptors);
                }
            }
        }
    }
    (rt.mutation_leave)(mutation);
    result
}

unsafe extern "system" fn create_committed_resource(
    this: *mut c_void,
    hp: *const D3D12_HEAP_PROPERTIES,
    hf: i32,
    d: *const D3D12_RESOURCE_DESC,
    st: i32,
    cv: *const c_void,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let caller = return_address();
    let start = clock();
    // SAFETY: the original, saved before the slot was replaced.
    let hr = unsafe {
        let original: CreateCommittedFn = core::mem::transmute(ORIGINAL_COMMITTED.load(Acquire));
        original(this, hp, hf, d, st, cv, riid, out)
    };
    count_time(12, start);
    // SAFETY: the caller's out pointer.
    if hr.is_ok() && !out.is_null() && unsafe { !(*out).is_null() } {
        let bytes = allocation_bytes(this, d);
        cvr_count_creation(0, bytes, caller as *const c_void);
        note_video_memory(this);
        if stacks_on() {
            note_creation_stack(false, d, hp, bytes);
        }
        if crate::resource_census::enabled() {
            if let Some(hp) = unsafe { hp.as_ref() } {
                // SAFETY: the creation's out pointer, a live resource.
                unsafe { crate::resource_census::observe(*out, crate::resource_census::Kind::Committed, d, hp, hf, bytes, caller as u64) };
            }
        }
        // SAFETY: as above.
        (rt().committed_created)(unsafe { *out }, d, st);
    }
    hr
}

unsafe extern "system" fn create_committed_resource1(
    this: *mut c_void,
    heap: *const D3D12_HEAP_PROPERTIES,
    flags: i32,
    desc: *const D3D12_RESOURCE_DESC,
    state: i32,
    clear: *const c_void,
    session: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let caller = return_address();
    let start = clock();
    // SAFETY: as above.
    let result = unsafe {
        let original: CreateCommitted1Fn = core::mem::transmute(ORIGINAL_COMMITTED1.load(Acquire));
        original(this, heap, flags, desc, state, clear, session, iid, out)
    };
    count_time(12, start);
    // SAFETY: the caller's out pointer.
    if result.is_ok() && !out.is_null() && unsafe { !(*out).is_null() } {
        let bytes = allocation_bytes(this, desc);
        cvr_count_creation(0, bytes, caller as *const c_void);
        note_video_memory(this);
        if stacks_on() {
            note_creation_stack(false, desc, heap, bytes);
        }
        if crate::resource_census::enabled() {
            if let Some(hp) = unsafe { heap.as_ref() } {
                // SAFETY: the creation's out pointer, a live resource.
                unsafe { crate::resource_census::observe(*out, crate::resource_census::Kind::Committed, desc, hp, flags, bytes, caller as u64) };
            }
        }
        // SAFETY: as above.
        (rt().committed_created)(unsafe { *out }, desc, state);
    }
    result
}

unsafe extern "system" fn create_placed_resource(
    this: *mut c_void,
    heap: *mut c_void,
    offset: u64,
    d: *const D3D12_RESOURCE_DESC,
    st: i32,
    cv: *const c_void,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    let caller = return_address();
    let start = clock();
    // SAFETY: as above.
    let hr = unsafe {
        let original: CreatePlacedFn = core::mem::transmute(ORIGINAL_PLACED.load(Acquire));
        original(this, heap, offset, d, st, cv, riid, out)
    };
    count_time(13, start);
    // SAFETY: the caller's out pointer.
    if hr.is_ok() && !out.is_null() && unsafe { !(*out).is_null() } {
        let bytes = allocation_bytes(this, d);
        cvr_count_creation(2, bytes, caller as *const c_void);
        if stacks_on() {
            let mut props = D3D12_HEAP_PROPERTIES::default();
            // SAFETY: the caller's heap or null.
            if let Some(h) = unsafe { ID3D12Heap::from_raw_borrowed(&heap) } {
                // SAFETY: a plain query.
                props = unsafe { h.GetDesc() }.Properties;
            }
            note_creation_stack(true, d, &props, bytes);
        }
        if crate::resource_census::enabled() {
            let mut hd = windows::Win32::Graphics::Direct3D12::D3D12_HEAP_DESC::default();
            // SAFETY: the caller's heap or null; a plain query.
            if let Some(h) = unsafe { ID3D12Heap::from_raw_borrowed(&heap) } {
                hd = unsafe { h.GetDesc() };
            }
            // SAFETY: the creation's out pointer, a live resource.
            unsafe {
                crate::resource_census::observe(*out, crate::resource_census::Kind::Placed, d, &hd.Properties, hd.Flags.0, bytes, caller as u64)
            };
        }
        // SAFETY: as above.
        (rt().placed_created)(unsafe { *out }, heap, offset, bytes, d, st);
    }
    hr
}

/// The serialised description each root signature was created from (there is no reflection back to it).
static ROOT_BLOBS: Mutex<Option<FastMap<usize, Vec<u8>>>> = Mutex::new(None);

unsafe extern "system" fn create_root_signature(
    this: *mut c_void,
    node_mask: u32,
    blob: *const c_void,
    len: usize,
    riid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    // SAFETY: as above.
    let hr = unsafe {
        let original: CreateRootSignatureFn =
            core::mem::transmute(ORIGINAL_ROOT_SIGNATURE.load(Acquire));
        original(this, node_mask, blob, len, riid, out)
    };
    // SAFETY: the caller's out pointer.
    if hr.is_ok()
        && !out.is_null()
        && unsafe { !(*out).is_null() }
        && !blob.is_null()
        && len != 0
        && len < (1 << 20)
    {
        // SAFETY: as above.
        let rs = unsafe { *out };
        (rt().root_created)(this, rs, node_mask, blob, len);
        cvr_count_creation(7, 0, core::ptr::null());
        let mut g = ROOT_BLOBS.lock().unwrap_or_else(|p| p.into_inner());
        let map = g.get_or_insert_with(cvr_host::fast::fast_map);
        if map.len() < 4096 || map.contains_key(&(rs as usize)) {
            // SAFETY: the caller's blob of `len` bytes.
            map.insert(
                rs as usize,
                unsafe { core::slice::from_raw_parts(blob as *const u8, len) }.to_vec(),
            );
        }
    }
    hr
}
/// RootSignatureBlob: the blob's length (0 when the signature predates the hook), copied when it fits.
///
/// # Safety
/// `out` is null or `capacity` writable bytes.
/// The pair probe's copy of a root signature's blob (formerly src/Stereo/DeviceHooks.cpp's cvr_cpp_rootsig_blob over
/// RootSignatureBlob): its length, 0 when unknown; copied when `capacity` holds it.
///
/// # Safety
/// `rs` is a root signature; `out` holds `capacity` bytes or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_cpp_rootsig_blob(rs: *mut c_void, out: *mut u8, capacity: usize) -> usize {
    // SAFETY: as documented.
    unsafe {
        let n = cvr_root_signature_blob(rs, core::ptr::null_mut(), 0);
        if n == 0 {
            return 0;
        }
        let mut blob = vec![0u8; n];
        if cvr_root_signature_blob(rs, blob.as_mut_ptr(), n) != n {
            return 0;
        }
        if !out.is_null() && capacity >= n {
            core::ptr::copy_nonoverlapping(blob.as_ptr(), out, n);
        }
        n
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_root_signature_blob(
    rs: *mut c_void,
    out: *mut u8,
    capacity: usize,
) -> usize {
    let g = ROOT_BLOBS.lock().unwrap_or_else(|p| p.into_inner());
    let Some(blob) = g.as_ref().and_then(|m| m.get(&(rs as usize))) else {
        return 0;
    };
    if !out.is_null() && capacity >= blob.len() {
        // SAFETY: the caller's buffer.
        unsafe { core::ptr::copy_nonoverlapping(blob.as_ptr(), out, blob.len()) };
    }
    blob.len()
}

// ---- the installer ----------------------------------------------------------------------------------------------------

/// One vtable slot replaced: the original into `original`, the hook in. False when the page could not be opened.
fn patch_slot(
    table: *mut *const c_void,
    slot: usize,
    hook: *const c_void,
    original: &AtomicUsize,
) -> bool {
    use windows_sys::Win32::System::Memory::{VirtualProtect, PAGE_READWRITE};
    // SAFETY: a COM vtable entry, made writable for one pointer and restored.
    unsafe {
        let entry = table.add(slot);
        let mut old = 0u32;
        if VirtualProtect(entry as *const c_void, 8, PAGE_READWRITE, &mut old) == 0 {
            return false;
        }
        original.store(*entry as usize, Release);
        *entry = hook;
        let mut junk = 0u32;
        VirtualProtect(entry as *const c_void, 8, old, &mut junk);
    }
    true
}
/// The same for a C++ hook, its original into the C++ half's slot.
fn patch_cpp_slot(table: *mut *const c_void, slot: usize, h: &CppHook) -> bool {
    let original = AtomicUsize::new(0);
    if !patch_slot(table, slot, h.hook, &original) {
        return false;
    }
    // SAFETY: the C++ half's original slot.
    unsafe { *h.original = original.load(Relaxed) as *const c_void };
    true
}
fn vtable(object: *mut c_void) -> *mut *const c_void {
    // SAFETY: a COM object's first word is its vtable.
    unsafe { *(object as *mut *mut *const c_void) }
}

/// patch_device_descriptor_slot: the device's vtable, once.
#[no_mangle]
pub extern "C" fn cvr_patch_device_descriptor_slot(device: *mut c_void) {
    if device.is_null() {
        return;
    }
    let rt = rt();
    if r(rt.desc_vtable_patched)
        .compare_exchange(false, true, AcqRel, Relaxed)
        .is_err()
    {
        return;
    }
    (rt.install_begin)(device);
    let vt = vtable(device);
    // Slot 14 = CreateDescriptorHeap.
    let heap_original = AtomicUsize::new(0);
    if patch_slot(
        vt,
        14,
        create_descriptor_heap as *const c_void,
        &heap_original,
    ) {
        // SAFETY: the C++ half's original slot.
        unsafe {
            *rt.original_create_descriptor_heap = heap_original.load(Relaxed) as *const c_void
        };
        log(&format!(
            "[descheap] CreateDescriptorHeap hooked (dev={} vt={} orig={})",
            p(device as usize),
            p(vt as usize),
            p(heap_original.load(Relaxed))
        ));
    } else {
        r(rt.desc_vtable_patched).store(false, Release);
        log("[descheap] FAILED to patch device vtable slot 14");
    }
    // Slot 8 = CreateCommandQueue (capture the queues, hook their execute).
    // SAFETY: the C++ half's bool.
    if unsafe { *rt.enable_exec_probe } {
        if patch_cpp_slot(vt, 8, &rt.create_command_queue) {
            log(&format!(
                "[exec] CreateCommandQueue hooked (dev={})",
                p(device as usize)
            ));
        } else {
            (rt.read_observation_unavailable)();
        }
    } else {
        (rt.read_observation_unavailable)();
    }
    // Slot 12 = CreateCommandList (slot 9 is CreateCommandAllocator: never patch it).
    if patch_cpp_slot(vt, 12, &rt.create_command_list) {
        // SAFETY: the original just stored.
        let original = unsafe { *rt.create_command_list.original } as usize;
        log(&format!(
            "[mirror] real device CreateCommandList hooked dev={} orig={}",
            p(device as usize),
            p(original)
        ));
    } else {
        (rt.read_observation_unavailable)();
    }
    // Slots 27 and 29 = CreateCommittedResource and CreatePlacedResource.
    if !patch_slot(
        vt,
        27,
        create_committed_resource as *const c_void,
        &ORIGINAL_COMMITTED,
    ) {
        (rt.read_observation_unavailable)();
    }
    if !patch_slot(
        vt,
        29,
        create_placed_resource as *const c_void,
        &ORIGINAL_PLACED,
    ) {
        (rt.read_observation_unavailable)();
    }
    log(&format!(
        "[sightaxis] buffer-address map hooked dev={}",
        p(device as usize)
    ));
    // ID3D12Device4 slot 53 = CreateCommittedResource1, only where the device implements it.
    // SAFETY: the caller's device.
    if let Some(d) = unsafe { ID3D12Device::from_raw_borrowed(&device) } {
        if let Ok(device4) = d.cast::<ID3D12Device4>() {
            let table = vtable(device4.as_raw());
            if patch_slot(
                table,
                53,
                create_committed_resource1 as *const c_void,
                &ORIGINAL_COMMITTED1,
            ) {
                log(&format!(
                    "[single-pass] CreateCommittedResource1 hooked dev={}",
                    p(device4.as_raw() as usize)
                ));
            } else {
                (rt.read_observation_unavailable)();
            }
        }
    }
    // Slot 16 = CreateRootSignature.
    if patch_slot(
        vt,
        16,
        create_root_signature as *const c_void,
        &ORIGINAL_ROOT_SIGNATURE,
    ) {
        log(&format!(
            "[rootsig] CreateRootSignature hooked dev={}",
            p(device as usize)
        ));
    }
    // Slot 10 = CreateGraphicsPipelineState, 11 = CreateComputePipelineState (where the outline shader is built).
    if patch_cpp_slot(vt, 10, &rt.create_graphics_pso) {
        log(&format!(
            "[pso] CreateGraphicsPipelineState hooked dev={}",
            p(device as usize)
        ));
    }
    if patch_cpp_slot(vt, 11, &rt.create_compute_pso) {
        log(&format!(
            "[pso] CreateComputePipelineState hooked dev={}",
            p(device as usize)
        ));
    }
    // ID3D12Device2 slot 47 = CreatePipelineState, only where the device implements it: patching a slot that may not
    // exist is how a hook corrupts the object next to it.
    // SAFETY: as above.
    if let Some(d) = unsafe { ID3D12Device::from_raw_borrowed(&device) } {
        if let Ok(device2) = d.cast::<ID3D12Device2>() {
            if patch_cpp_slot(vtable(device2.as_raw()), 47, &rt.create_pipeline_state) {
                log(&format!(
                    "[pso] ID3D12Device2::CreatePipelineState hooked dev={}",
                    p(device as usize)
                ));
            }
        }
        if let Ok(device5) = d.cast::<ID3D12Device5>() {
            let table = vtable(device5.as_raw());
            // SAFETY: an entry of a vtable the device implements.
            let current = unsafe { *table.add(62) };
            if current != rt.create_state_object.hook
                && patch_cpp_slot(table, 62, &rt.create_state_object)
            {
                log(&format!(
                    "[pso] ID3D12Device5::CreateStateObject hooked dev={}",
                    p(device as usize)
                ));
            }
        }
    }
    // Slot 17 = CreateConstantBufferView.
    if patch_cpp_slot(vt, 17, &rt.create_cbv) {
        // SAFETY: the original just stored.
        let original = unsafe { *rt.create_cbv.original } as usize;
        log(&format!(
            "[cbv] CreateConstantBufferView hooked dev={} orig={}",
            p(device as usize),
            p(original)
        ));
    }
    // SAFETY: entries of the device's vtable; the C++ half's original slots.
    let (cbv_ours, heap_ours, cbv_original, heap_original) = unsafe {
        (
            *vt.add(17) == rt.create_cbv.hook,
            *vt.add(14) == create_descriptor_heap as *const c_void,
            !(*rt.create_cbv.original).is_null(),
            !(*rt.original_create_descriptor_heap).is_null(),
        )
    };
    (rt.descriptors_ready)(
        device,
        cbv_original && heap_original && cbv_ours && heap_ours,
    );
    // Slot 20 = CreateRenderTargetView. Real game objects only: no throwaway device through sl.interposer.
    if patch_cpp_slot(vt, 20, &rt.create_rtv) {
        r(rt.rtv_hook_installed).store(true, Release);
        // SAFETY: the original just stored.
        let original = unsafe { *rt.create_rtv.original } as usize;
        log(&format!(
            "[mirror] real device CreateRTV hooked dev={} orig={}",
            p(device as usize),
            p(original)
        ));
    }
}

// ---- the runtime (formerly src/Stereo/DeviceHooks.cpp's kDeviceCreationRuntime) and the command-list creation hook
// (formerly its Hook_CreateCommandList)
mod table {
    use super::{CppHook, Runtime};
    use core::ffi::c_void;
    use core::sync::atomic::{AtomicBool, AtomicPtr, Ordering::*};
    use std::sync::LazyLock;
    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DESC;

    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        // The C++ hooks and their originals' slots (src/Stereo/Capture.cpp, SyncStereo.cpp).
        /// The CBV hook's entry (the thunk below).
        fn cvr_create_cbv_hook();
        #[link_name = "?g_orig_CreateDescriptorHeap@detail@cvr@@3P6AJPEAUID3D12Device@@PEBUD3D12_DESCRIPTOR_HEAP_DESC@@AEBU_GUID@@PEAPEAX@ZEA"]
        static mut g_orig_CreateDescriptorHeap: *const c_void;

        // Before a slot is patched: the depth capture's hooks.
        #[link_name = "?InstallDepthCaptureHooks@@YAXPEAUID3D12Device@@@Z"]
        fn install_depth_capture_hooks(device: *mut c_void);

        // The descriptor audit's borrowed ranges.
        #[link_name = "?DescriptorMutationHooksReady@@YA_NPEAUID3D12Device@@@Z"]
        fn descriptor_mutation_hooks_ready(device: *mut c_void) -> bool;

        #[link_name = "cvr_nsf2_enabled"]
        fn families_enabled() -> bool;

        // A root signature's readers.
        #[link_name = "cvr_nsf2_root_created"]
        fn families_root_created(rs: *mut c_void, node_mask: u32, blob: *const c_void, len: usize);

        // A new list's per-list state reset.
        #[link_name = "cvr_nsf2_reset"]
        fn families_reset(list: *mut c_void, pso: *mut c_void);
    }

    /// The engine's own CreateCommandList (the device's slot 12 before the hook).
    static ORIGINAL_CREATE_COMMAND_LIST: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    /// The RTV hook's installation.
    static RTV_HOOK_INSTALLED: AtomicBool = AtomicBool::new(false);
    type CreateList = unsafe extern "system" fn(*mut c_void, u32, i32, *mut c_void, *mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
    const LIST_DIRECT: i32 = 0;
    const LIST_COMPUTE: i32 = 2;
    const LIST_COPY: i32 = 3;
    /// The list's original slot 77 (RSSetShadingRate) in its vtable entry (list_vtable.rs's Entry.head).
    const SHADING_RATE_ORIGINAL: usize = 27;

    /// CreateCommandList: a direct, compute or copy list gets its vtable patched and every subsystem's per-list state
    /// reset (copy lists also mutate what stereo draws read; their writes and barriers are observed).
    unsafe extern "system" fn create_command_list(
        device: *mut c_void,
        node_mask: u32,
        ty: i32,
        allocator: *mut c_void,
        initial: *mut c_void,
        riid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        // SAFETY: the slot the installation wrote before patching the vtable.
        let original: CreateList = unsafe { core::mem::transmute(ORIGINAL_CREATE_COMMAND_LIST.load(Acquire)) };
        // SAFETY: the device call's own arguments.
        let hr = unsafe { original(device, node_mask, ty, allocator, initial, riid, out) };
        // SAFETY: the call's output, checked before use.
        let list = if hr.is_ok() && !out.is_null() { unsafe { *out } } else { core::ptr::null_mut() };
        if list.is_null() || !(ty == LIST_DIRECT || ty == LIST_COMPUTE || ty == LIST_COPY) {
            return hr;
        }
        // SAFETY: a list just created; the subsystems take it by pointer.
        unsafe {
            crate::list_vtable::cvr_lvt_patch(list);
            crate::target_handoff_observer::reset(list);
            crate::target_access_observer::events::reset(list);
            crate::texture_receipts::cvr_tr_reset(list);
            // buffer_read::Reset: the copy windows' list reset, then the buffers'
            if crate::copy_windows::cvr_cw_active_windows.load(Relaxed) != 0 {
                crate::copy_windows::cvr_cw_global(3, list as u64, 0);
            }
            crate::buffer_read::cvr_buffer_read_reset(list);
            crate::material_observe::cvr_mbind_reset(list);
            if ty == LIST_COPY {
                crate::registered_copies::entries::cvr_rc2_reset(list);
                return hr;
            }
            families_reset(list, initial);
            crate::constant_capture_state::cvr_ncc2_reset(list);
            crate::preparation_trace::cvr_prep_reset_list(list as u64);
            let entry = crate::list_vtable::cvr_lvt_entry(list);
            let rate_known = !entry.is_null() && (*entry).head[SHADING_RATE_ORIGINAL] != 0;
            crate::fixed_vrs::cvr_vrs_reset(list, rate_known, initial);
            cvr_gpu::optics::cvr_optics_list_reset(list as usize);
        }
        hr
    }

    extern "C" fn install_begin(device: *mut c_void) {
        // SAFETY: the readers' addresses (registered_copies.rs); the device being patched.
        unsafe {
            crate::constant_capture_state::cvr_ncc2_set_reader(
                crate::registered_copies::entries::cvr_rc2_read as *const c_void,
                crate::registered_copies::entries::cvr_rc2_validate as *const c_void,
                crate::registered_copies::entries::cvr_rc2_mark_read as *const c_void,
                crate::registered_copies::entries::cvr_rc2_resolve as *const c_void,
            );
            // Engine indirect signatures can predate swapchain creation.
            install_depth_capture_hooks(device);
        }
    }
    extern "C" fn read_observation_unavailable() {
        // SAFETY: plain calls.
        unsafe { crate::list_detours::cvr_lh_reads_unavailable() };
    }
    extern "C" fn descriptors_ready(device: *mut c_void, hooks_ours: bool) {
        // SAFETY: plain C++ queries on the device being patched.
        unsafe {
            // descriptors::Enabled: the probe observes bindings, outside the port's own commands.
            if crate::stereo_probe::observe_bindings() && !cvr_host::probe::internal() {
                // descriptors::BorrowedRangeObserverReady: the tracking's Ready.
                crate::descriptor_audit::tracking_call(0, u64::from(hooks_ours && descriptor_mutation_hooks_ready(device)));
            }
        }
    }
    extern "C" fn root_created(device: *mut c_void, rs: *mut c_void, node_mask: u32, blob: *const c_void, len: usize) {
        // SAFETY: the root signature just created and its blob.
        unsafe {
            families_root_created(rs, node_mask, blob, len);
            crate::stereo_probe_creation::cvr_nspc_root_created(rs, blob, len);
            crate::single_pass_pipelines::cvr_single_pass_root_created(device, rs, blob, len);
        }
    }
    /// A large buffer's GPU range noted for the census (instance and vertex streams, not small scratch).
    fn buf_note_created(resource: *mut c_void, desc: *const D3D12_RESOURCE_DESC) {
        // SAFETY: the creation's description or null.
        let Some(d) = (unsafe { desc.as_ref() }) else { return };
        if resource.is_null() || d.Dimension != windows::Win32::Graphics::Direct3D12::D3D12_RESOURCE_DIMENSION_BUFFER || d.Width < 4096 {
            return;
        }
        // SAFETY: the resource the device just created.
        let Some(r) = (unsafe { windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&resource) }) else { return };
        // SAFETY: as above.
        let va = unsafe { r.GetGPUVirtualAddress() };
        crate::census_pso::cvr_census_buf_note(resource, va, d.Width);
    }
    /// constants::RegisteredResourceCreated: the creation's receipt, while the families' bindings are observed.
    fn registered_resource_created(resource: *mut c_void, placed: bool, heap: *mut c_void, offset: u64, bytes: u64, state: u32) {
        // SAFETY: a plain C++ query.
        if cvr_host::probe::internal() || !crate::stereo_probe::cvr_observe_bindings() || !unsafe { families_enabled() } || resource.is_null() {
            return;
        }
        // SAFETY: the resource the device just created.
        unsafe { crate::creation_receipts::cvr_crc_created(resource, placed, heap, offset, bytes, true, state) };
    }
    /// A committed resource's readers: the cascades, the constants' resource API and receipts, the texture and buffer
    /// reads, the geometry's buffers, the census' buffer map and the diagnostics.
    extern "C" fn committed_created(resource: *mut c_void, desc: *const D3D12_RESOURCE_DESC, state: i32) {
        let state = state as u32;
        // SAFETY: the resource the device just created and its description.
        unsafe {
            cvr_gpu::mono_cascades::cvr_mono_cascades_resource_created(resource, core::ptr::null_mut(), 0, 0);
            crate::upload_registry::cvr_upload_observe_resource_api(resource);
            registered_resource_created(resource, false, core::ptr::null_mut(), 0, 0, state);
            crate::texture_receipts::cvr_tr_resource_created(resource, state, false, core::ptr::null_mut(), 0, 0);
            crate::buffer_read::cvr_buffer_read_resource_created(resource, state, false);
            if crate::stereo_probe::cvr_observe_bindings() && !cvr_host::probe::internal() {
                crate::ia_binding::cvr_iab_buffer_created(resource, state);
            }
            buf_note_created(resource, desc);
            if !desc.is_null() {
                cvr_gpu::gpu_diag::cvr_aftermath_observe_resource_creation(resource, desc);
            }
        }
    }
    /// A placed resource's readers, with its heap range.
    extern "C" fn placed_created(resource: *mut c_void, heap: *mut c_void, offset: u64, bytes: u64, desc: *const D3D12_RESOURCE_DESC, state: i32) {
        let state = state as u32;
        // SAFETY: as committed_created's.
        unsafe {
            if !desc.is_null() {
                cvr_gpu::mono_cascades::cvr_mono_cascades_resource_created(resource, heap, offset, bytes);
            }
            crate::upload_registry::cvr_upload_observe_resource_api(resource);
            registered_resource_created(resource, true, heap, offset, bytes, state);
            crate::texture_receipts::cvr_tr_resource_created(resource, state, true, heap, offset, bytes);
            crate::buffer_read::cvr_buffer_read_resource_created(resource, state, true);
            if crate::stereo_probe::cvr_observe_bindings() && !cvr_host::probe::internal() {
                crate::ia_binding::cvr_iab_buffer_created(resource, state);
            }
            buf_note_created(resource, desc);
            if !desc.is_null() {
                cvr_gpu::gpu_diag::cvr_aftermath_observe_resource_creation(resource, desc);
            }
        }
    }
    /// CreateRenderTargetView: the scene state's view (while bindings are observed), then -- for the engine's own views
    /// -- the crop-blit's size map (any format), the HUD's descriptor and the mirror's candidates (the view's format, or
    /// the resource's without a description).
    unsafe extern "system" fn create_rtv(
        device: *mut c_void,
        resource: *mut c_void,
        desc: *const windows::Win32::Graphics::Direct3D12::D3D12_RENDER_TARGET_VIEW_DESC,
        handle: usize,
    ) {
        type CreateRtv = unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            *const windows::Win32::Graphics::Direct3D12::D3D12_RENDER_TARGET_VIEW_DESC,
            usize,
        );
        // SAFETY: the slot the installation wrote before patching the vtable.
        let original: CreateRtv = unsafe { core::mem::transmute(crate::stereo_shared::g_orig_CreateRTV) };
        // SAFETY: the device call's own arguments.
        unsafe { original(device, resource, desc, handle) };
        // SAFETY: plain queries; the view just created.
        unsafe {
            if crate::stereo_probe::cvr_observe_bindings() || crate::stereo_probe_creation::enabled() {
                cvr_gpu::scene_state::cvr_scene_rtv_created(resource as usize, desc, handle);
            }
        }
        if cvr_host::probe::internal() {
            return;
        }
        // SAFETY: the view's resource and descriptor.
        unsafe {
            crate::capture_rtv::cvr_capture_rtv_dim_register(handle, resource);
            crate::hud::cvr_hud_register_rtv(resource, desc, handle);
            let format = match desc.as_ref() {
                Some(d) => d.Format,
                None => windows::Win32::Graphics::Direct3D12::ID3D12Resource::from_raw_borrowed(&resource)
                    .map_or(windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN, |r| r.GetDesc().Format),
            };
            crate::capture_rtv::cvr_capture_mirror_register_rtv(resource, format, handle);
        }
    }
    unsafe extern "C-unwind" {
        /// The upload ring's CPU view of a GPU address (upload_maps.rs).
        safe fn cvr_upload_cpu_for_va(va: u64, need: u64) -> *const u8;
    }
    /// The engine's own CreateConstantBufferView (the device's slot 17 before the hook).
    static ORIGINAL_CREATE_CBV: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    // The hook's entry: the caller's return address (the CBV's creator) handed to create_cbv as its fourth argument.
    // A leaf jump with no stack of its own, so it needs no unwind data.
    core::arch::global_asm!(
        ".globl cvr_create_cbv_hook",
        ".def cvr_create_cbv_hook",
        ".scl 2",
        ".type 32",
        ".endef",
        "cvr_create_cbv_hook:",
        "    mov r9, [rsp]",
        "    jmp {target}",
        target = sym create_cbv,
    );
    /// CreateConstantBufferView: inside the descriptor audit's mutation scope, the engine's own call, then the CBV's
    /// origin (its creator), the audit's write, the upload's touch, the optics material probe, the camera upload's
    /// CBV and the capture's per-view constants.
    unsafe extern "system" fn create_cbv(
        device: *mut c_void,
        desc: *const windows::Win32::Graphics::Direct3D12::D3D12_CONSTANT_BUFFER_VIEW_DESC,
        handle: usize,
        caller: usize,
    ) {
        type CreateCbv = unsafe extern "system" fn(
            *mut c_void,
            *const windows::Win32::Graphics::Direct3D12::D3D12_CONSTANT_BUFFER_VIEW_DESC,
            usize,
        );
        let scope = mutation_enter();
        // SAFETY: the slot the installation wrote before patching the vtable.
        let original: CreateCbv = unsafe { core::mem::transmute(ORIGINAL_CREATE_CBV.load(Acquire)) };
        // SAFETY: the device call's own arguments, then its view and descriptor.
        unsafe {
            original(device, desc, handle);
            let mut origin = crate::source_cbv_observe::RawOrigin { cookie: 0, id: 0, owner: core::ptr::null_mut() };
            crate::source_cbv_observe::cvr_scbo_created(device, desc, handle as u64, caller as u64, &mut origin);
            crate::descriptor_audit::cvr_sda_audit_write(
                3,
                core::ptr::null_mut(),
                desc as usize,
                handle as u64,
                origin.cookie,
                origin.id,
                origin.owner,
                core::ptr::null_mut(),
            );
            crate::upload_registry::cvr_upload_touch(desc);
            if let Some(d) = desc.as_ref() {
                if core::ptr::read_volatile(&raw const crate::used_moved_exports::CyberpunkVR_OpticsMaterialProbe) != 0
                    && d.SizeInBytes == 512
                {
                    let mut material = cvr_upload_cpu_for_va(d.BufferLocation, 324);
                    if material.is_null() {
                        material = crate::census_pso::cvr_census_filled_cpu_for_va(d.BufferLocation, 324);
                    }
                    if !material.is_null() {
                        crate::grading_uploads::cvr_grading_optics_material_snapshot(324, material.cast());
                    }
                }
                // Only the port's scoped camera uploads can publish a result for this descriptor.
                if d.SizeInBytes == 2048 {
                    crate::camera_upload::cvr_ncu_created_cbv(desc, handle as u64);
                }
                let tls = &*crate::render_tls::cvr_render_tls();
                crate::capture_cbv::cvr_capture_cbv_views(
                    d.BufferLocation,
                    d.SizeInBytes,
                    tls.vrcam_node_active,
                    tls.current_node_work,
                    crate::stereo_shared::g_exe_base as usize,
                );
            }
            // The origin's owner (an owned reference) let go, then the mutation scope.
            if !origin.owner.is_null() {
                drop(windows::core::IUnknown::from_raw(origin.owner));
            }
        }
        mutation_leave(scope);
    }
    /// descriptors::MutationScope (formerly DeviceHooks.cpp's cvr_cpp_mutation_enter/leave): the borrowed-range
    /// tracking's mutation entered for the scope. Whether it was is the token itself (non-null: entered) -- a box on
    /// the heap per descriptor write cost every CreateConstantBufferView a heap allocation and free (1.4 of its
    /// 1.8 ms a frame in ntdll).
    extern "C" fn mutation_enter() -> *mut c_void {
        if crate::descriptor_audit::tracking_call(4, 0) {
            core::ptr::dangling_mut::<u8>().cast()
        } else {
            core::ptr::null_mut()
        }
    }
    extern "C" fn mutation_leave(scope: *mut c_void) {
        if !scope.is_null() {
            crate::descriptor_audit::tracking_call(5, 0);
        }
    }
    fn hook(hook: *const c_void, original: *mut *const c_void) -> CppHook {
        CppHook { hook, original }
    }

    pub(super) static RT: LazyLock<Runtime> = LazyLock::new(|| {
        use crate::moved_globals as m;
        use crate::pso_hooks as p;
        Runtime {
            create_command_queue: hook(
                super::game_objects::create_command_queue as *const c_void,
                super::game_objects::ORIGINAL_CREATE_COMMAND_QUEUE.as_ptr().cast(),
            ),
            create_command_list: hook(create_command_list as *const c_void, ORIGINAL_CREATE_COMMAND_LIST.as_ptr().cast()),
            create_graphics_pso: hook(p::cvr_pso_create_graphics as *const c_void, p::cvr_pso_original_graphics.as_ptr().cast()),
            create_compute_pso: hook(p::cvr_pso_create_compute as *const c_void, p::cvr_pso_original_compute.as_ptr().cast()),
            create_pipeline_state: hook(p::cvr_pso_create_stream as *const c_void, p::cvr_pso_original_stream.as_ptr().cast()),
            create_state_object: hook(
                p::cvr_pso_create_state_object as *const c_void,
                p::cvr_pso_original_state_object.as_ptr().cast(),
            ),
            create_cbv: hook(cvr_create_cbv_hook as *const c_void, ORIGINAL_CREATE_CBV.as_ptr().cast()),
            create_rtv: hook(create_rtv as *const c_void, (&raw mut crate::stereo_shared::g_orig_CreateRTV).cast()),
            original_create_descriptor_heap: (&raw mut g_orig_CreateDescriptorHeap).cast(),
            rtv_hook_installed: &raw const RTV_HOOK_INSTALLED,
            enable_exec_probe: (&raw const m::g_enable_exec_probe).cast(),
            desc_vtable_patched: &raw const m::g_desc_vtable_patched,
            enable_desc_heap_enlarge: (&raw const m::g_enable_desc_heap_enlarge).cast(),
            desc_heap_target: (&raw const m::g_desc_heap_target).cast(),
            enable_desc_heap_resize: (&raw const m::g_enable_desc_heap_resize).cast(),
            desc_heap_resized: &raw const m::g_desc_heap_resized,
            install_begin,
            read_observation_unavailable,
            descriptors_ready,
            mutation_enter,
            mutation_leave,
            committed_created,
            placed_created,
            root_created,
        }
    });
}

// ---- the game's device and DIRECT queue (formerly src/Stereo/DeviceHooks.cpp's g_game_device, g_game_queue, their lock,
// the accessors, cvr_cpp_capture_game_device and Hook_CreateCommandQueue)
pub mod game_objects {
    use core::ffi::{c_char, c_void};
    use core::sync::atomic::{AtomicPtr, Ordering::*};
    use std::sync::Mutex;
    use windows::core::{Interface, GUID, HRESULT};
    use windows::Win32::Graphics::Direct3D12::{ID3D12CommandQueue, ID3D12Device, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC};

    /// ID3D12Device* g_game_device (C linkage): the game's device, taken once (the D3D12CreateDevice detour).
    #[no_mangle]
    pub static mut g_game_device: *mut c_void = core::ptr::null_mut();
    /// ID3D12CommandQueue* cvr::detail::g_game_queue: the game's first DIRECT queue.
    #[export_name = "?g_game_queue@detail@cvr@@3PEAUID3D12CommandQueue@@EA"]
    pub static mut g_game_queue: *mut c_void = core::ptr::null_mut();
    /// g_game_object_mtx: both are taken under it.
    static GAME_OBJECTS: Mutex<()> = Mutex::new(());
    /// The engine's own CreateCommandQueue (the device's slot 8 before the hook).
    pub(crate) static ORIGINAL_CREATE_COMMAND_QUEUE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
    type CreateQueue = unsafe extern "system" fn(*mut c_void, *const D3D12_COMMAND_QUEUE_DESC, *const GUID, *mut *mut c_void) -> HRESULT;

    #[allow(non_upper_case_globals)]
    unsafe extern "C" {
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    fn log(text: String) {
        let line = std::ffi::CString::new(text).unwrap_or_default();
        // SAFETY: a terminated line.
        unsafe { cvr_cpp_stereo_log(line.as_ptr()) };
    }
    fn lock() -> std::sync::MutexGuard<'static, ()> {
        GAME_OBJECTS.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The game's device (null before the D3D12CreateDevice detour took it).
    pub fn game_device() -> *mut c_void {
        // SAFETY: a pointer written once under the lock.
        unsafe { core::ptr::read_volatile(&raw const g_game_device) }
    }
    /// For the game-view ImGui overlay.
    #[no_mangle]
    pub extern "C" fn CyberpunkVR_GetGameDevice() -> *mut c_void {
        game_device()
    }
    #[no_mangle]
    pub extern "C" fn CyberpunkVR_GetGameQueue() -> *mut c_void {
        // SAFETY: as game_device's.
        unsafe { core::ptr::read_volatile(&raw const g_game_queue) }
    }
    /// The D3D12CreateDevice detour's (stereo_start.rs): the game's device, taken once (and held).
    ///
    /// # Safety
    /// `device` is a live device.
    #[no_mangle]
    pub unsafe extern "C" fn cvr_cpp_capture_game_device(device: *mut c_void) {
        let _held = lock();
        if !game_device().is_null() {
            return;
        }
        // SAFETY: the caller's device; the reference taken is kept for the process.
        if let Some(d) = unsafe { ID3D12Device::from_raw_borrowed(&device) } {
            core::mem::forget(d.clone());
        }
        // SAFETY: under the lock.
        unsafe { core::ptr::write_volatile(&raw mut g_game_device, device) };
        log(format!("[mirror] captured real game device={:016X}", device as usize));
    }
    /// CreateCommandQueue: the queue observed by the registered constants, the game's first DIRECT queue taken (and
    /// held), the queue's ExecuteCommandLists patched.
    pub(crate) unsafe extern "system" fn create_command_queue(
        device: *mut c_void,
        desc: *const D3D12_COMMAND_QUEUE_DESC,
        riid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        // SAFETY: the slot the installation wrote before patching the vtable.
        let original: CreateQueue = unsafe { core::mem::transmute(ORIGINAL_CREATE_COMMAND_QUEUE.load(Acquire)) };
        // SAFETY: the device call's own arguments.
        let hr = unsafe { original(device, desc, riid, out) };
        // SAFETY: the call's output, checked before use.
        let queue = if hr.is_ok() && !out.is_null() { unsafe { *out } } else { core::ptr::null_mut() };
        if queue.is_null() {
            return hr;
        }
        // SAFETY: a queue just created.
        unsafe { crate::queue_api_observer::observe_queue_api(queue) };
        // SAFETY: the caller's description or null.
        if unsafe { desc.as_ref() }.is_some_and(|d| d.Type == D3D12_COMMAND_LIST_TYPE_DIRECT) {
            let _held = lock();
            // SAFETY: under the lock.
            if unsafe { core::ptr::read_volatile(&raw const g_game_queue) }.is_null() {
                // SAFETY: the queue just created; the reference taken is kept for the process.
                if let Some(q) = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue) } {
                    core::mem::forget(q.clone());
                }
                // SAFETY: under the lock; the export is the diagnostics' copy.
                unsafe {
                    core::ptr::write_volatile(&raw mut g_game_queue, queue);
                    core::ptr::write_volatile(&raw mut crate::declared_moved_exports::CyberpunkVR_DebugMirrorGameQueue, queue as u64);
                }
                log(format!("[mirror] captured game DIRECT queue={:016X} device={:016X}", queue as usize, device as usize));
            }
        }
        crate::ecl_table::attach();
        // SAFETY: a live queue.
        unsafe { crate::queue_dispatch::cvr_ecl_patch(queue.cast()) };
        hr
    }
}
