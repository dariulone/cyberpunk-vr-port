//! The machine's load for framegen's statistics overlay (formerly src/Framegen/HardwareTelemetry.cpp; the header keeps
//! `HardwareStatistics` and its four functions, which forward here with framegen's metrics switch as a callback).
//!
//! A monitor thread samples once a second while the metrics are on and sleeps on a condition variable while they are
//! off: total and per-core CPU load (GetSystemTimes, NtQuerySystemInformation's processor performance table), RAM and
//! the process's working set and commit, the GPU's process budget (IDXGIAdapter3::QueryVideoMemoryInfo) and, on
//! NVIDIA, its load, memory and temperature through NVML. NVML's device is the one at the adapter's PCI address
//! (D3DKMT); when that lookup fails a unique NVIDIA device is still unambiguous, but an arbitrary one of several is
//! never taken.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::Interface;
use windows::Win32::Foundation::LUID;
use windows::Win32::Graphics::Dxgi::*;

/// include/Framegen/HardwareTelemetry.hpp's HardwareStatistics.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct HardwareStatistics {
    pub sequence: u64,
    pub cpu_usage: f64,
    pub gpu_usage: f64,
    pub gpu_temperature: f64,
    pub core_maximum: f64,
    pub sample_ms: f64,
    pub ram_used: u64,
    pub ram_total: u64,
    pub app_working_set: u64,
    pub app_commit: u64,
    pub gpu_memory_used: u64,
    pub gpu_memory_total: u64,
    pub gpu_process_used: u64,
    pub gpu_process_budget: u64,
    pub gpu_memory_valid: bool,
    pub gpu_process_memory_valid: bool,
    pub core_count: u32,
    pub cores: [f32; 128],
    pub gpu_name: [u8; 96],
    pub cpu_name: [u8; 96],
}
const _: () = assert!(core::mem::size_of::<HardwareStatistics>() == 824);
impl Default for HardwareStatistics {
    fn default() -> Self {
        HardwareStatistics {
            sequence: 0,
            cpu_usage: -1.0,
            gpu_usage: -1.0,
            gpu_temperature: -1.0,
            core_maximum: -1.0,
            sample_ms: 0.0,
            ram_used: 0,
            ram_total: 0,
            app_working_set: 0,
            app_commit: 0,
            gpu_memory_used: 0,
            gpu_memory_total: 0,
            gpu_process_used: 0,
            gpu_process_budget: 0,
            gpu_memory_valid: false,
            gpu_process_memory_valid: false,
            core_count: 0,
            cores: [0.0; 128],
            gpu_name: [0; 96],
            cpu_name: [0; 96],
        }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}
#[repr(C)]
struct MemoryStatus {
    length: u32,
    load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page: u64,
    avail_page: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended: u64,
}
#[repr(C)]
#[derive(Default)]
struct ProcessMemoryCounters {
    cb: u32,
    page_faults: u32,
    peak_working_set: usize,
    working_set: usize,
    quota_peak_paged: usize,
    quota_paged: usize,
    quota_peak_non_paged: usize,
    quota_non_paged: usize,
    pagefile: usize,
    peak_pagefile: usize,
    private_usage: usize,
}
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct CoreTimes {
    idle: i64,
    kernel: i64,
    user: i64,
    reserved: [i64; 2],
    reserved2: u32,
}
const _: () = assert!(core::mem::size_of::<CoreTimes>() == 48);
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemTimes(idle: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    fn GlobalMemoryStatusEx(status: *mut MemoryStatus) -> i32;
    fn K32GetProcessMemoryInfo(
        process: *mut c_void,
        counters: *mut ProcessMemoryCounters,
        cb: u32,
    ) -> i32;
    fn GetCurrentProcess() -> *mut c_void;
    fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(frequency: *mut i64) -> i32;
    fn WideCharToMultiByte(
        page: u32,
        flags: u32,
        wide: *const u16,
        n: i32,
        out: *mut u8,
        size: i32,
        default: *const u8,
        used: *mut i32,
    ) -> i32;
}
#[link(name = "advapi32")]
extern "system" {
    fn RegGetValueW(
        key: isize,
        sub: *const u16,
        value: *const u16,
        flags: u32,
        kind: *mut u32,
        data: *mut c_void,
        size: *mut u32,
    ) -> i32;
}
const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
const RRF_RT_REG_SZ: u32 = 2;
const LOAD_LIBRARY_SEARCH_SYSTEM32: u32 = 0x800;
const CP_UTF8: u32 = 65001;
/// SystemProcessorPerformanceInformation.
const PROCESSOR_PERFORMANCE: u32 = 8;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
/// framegen's NowMs: QPC in milliseconds.
fn now_ms() -> f64 {
    let (mut f, mut v) = (0i64, 0i64);
    // SAFETY: plain Win32 calls.
    unsafe {
        QueryPerformanceFrequency(&mut f);
        QueryPerformanceCounter(&mut v);
    }
    v as f64 * (1000.0 / f as f64)
}
/// A NUL-terminated wide string into a 96-byte UTF-8 field (WideCharToMultiByte as the C++ called it).
unsafe fn utf8_into(wide: *const u16, out: &mut [u8; 96]) {
    WideCharToMultiByte(
        CP_UTF8,
        0,
        wide,
        -1,
        out.as_mut_ptr(),
        96,
        core::ptr::null(),
        core::ptr::null_mut(),
    );
}
/// snprintf(out, 96, "%s", text).
fn copy_name(out: &mut [u8; 96], text: &[u8; 96]) {
    let n = text.iter().position(|&b| b == 0).unwrap_or(96).min(95);
    *out = [0; 96];
    out[..n].copy_from_slice(&text[..n]);
}
fn ticks(t: FileTime) -> u64 {
    ((t.high as u64) << 32) | t.low as u64
}

type NvDevice = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct NvMemory {
    total: u64,
    free: u64,
    used: u64,
}
#[repr(C)]
#[derive(Default)]
struct NvUsage {
    gpu: u32,
    memory: u32,
}
struct Nvml {
    module: *mut c_void,
    initialized: bool,
    shutdown: Option<unsafe extern "C" fn() -> i32>,
    by_pci: Option<unsafe extern "C" fn(*const c_char, *mut NvDevice) -> i32>,
    count: Option<unsafe extern "C" fn(*mut u32) -> i32>,
    by_index: Option<unsafe extern "C" fn(u32, *mut NvDevice) -> i32>,
    usage: Option<unsafe extern "C" fn(NvDevice, *mut NvUsage) -> i32>,
    memory: Option<unsafe extern "C" fn(NvDevice, *mut NvMemory) -> i32>,
    temperature: Option<unsafe extern "C" fn(NvDevice, u32, *mut u32) -> i32>,
}
unsafe fn symbol<T>(module: *mut c_void, name: &core::ffi::CStr) -> Option<T> {
    let p = GetProcAddress(module, name.as_ptr());
    (!p.is_null()).then(|| core::mem::transmute_copy(&p))
}
#[repr(C)]
struct OpenAdapterFromLuid {
    luid: LUID,
    adapter: u32,
}
#[repr(C)]
struct QueryAdapterInfo {
    adapter: u32,
    kind: u32,
    data: *mut c_void,
    size: u32,
}
#[repr(C)]
#[derive(Default)]
struct AdapterAddress {
    bus: u32,
    device: u32,
    function: u32,
}
/// KMTQAITYPE_ADAPTERADDRESS.
const ADAPTER_ADDRESS: u32 = 6;
impl Nvml {
    unsafe fn load() -> Nvml {
        let module = LoadLibraryExW(
            wide("nvml.dll").as_ptr(),
            core::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        );
        let mut n = Nvml {
            module,
            initialized: false,
            shutdown: None,
            by_pci: None,
            count: None,
            by_index: None,
            usage: None,
            memory: None,
            temperature: None,
        };
        if module.is_null() {
            return n;
        }
        let init: Option<unsafe extern "C" fn() -> i32> = symbol(module, c"nvmlInit_v2");
        n.shutdown = symbol(module, c"nvmlShutdown");
        n.by_pci = symbol(module, c"nvmlDeviceGetHandleByPciBusId_v2");
        n.count = symbol(module, c"nvmlDeviceGetCount_v2");
        n.by_index = symbol(module, c"nvmlDeviceGetHandleByIndex_v2");
        n.usage = symbol(module, c"nvmlDeviceGetUtilizationRates");
        n.memory = symbol(module, c"nvmlDeviceGetMemoryInfo");
        n.temperature = symbol(module, c"nvmlDeviceGetTemperature");
        n.initialized = init.is_some_and(|init| init() == 0);
        n
    }
    unsafe fn select(&self, luid: LUID) -> NvDevice {
        if !self.initialized {
            return core::ptr::null_mut();
        }
        let gdi = GetModuleHandleW(wide("gdi32.dll").as_ptr());
        let open: Option<unsafe extern "system" fn(*mut OpenAdapterFromLuid) -> i32> =
            symbol(gdi, c"D3DKMTOpenAdapterFromLuid");
        let query: Option<unsafe extern "system" fn(*mut QueryAdapterInfo) -> i32> =
            symbol(gdi, c"D3DKMTQueryAdapterInfo");
        let close: Option<unsafe extern "system" fn(*mut u32) -> i32> =
            symbol(gdi, c"D3DKMTCloseAdapter");
        if let (Some(open), Some(query), Some(close), Some(by_pci)) =
            (open, query, close, self.by_pci)
        {
            let mut handle = OpenAdapterFromLuid { luid, adapter: 0 };
            if open(&mut handle) >= 0 {
                let mut address = AdapterAddress::default();
                let mut info = QueryAdapterInfo {
                    adapter: handle.adapter,
                    kind: ADAPTER_ADDRESS,
                    data: (&mut address as *mut AdapterAddress).cast(),
                    size: core::mem::size_of::<AdapterAddress>() as u32,
                };
                let result = query(&mut info);
                let mut cleanup = handle.adapter;
                close(&mut cleanup);
                if result >= 0 {
                    let pci = format!(
                        "00000000:{:02X}:{:02X}.{:X}\0",
                        address.bus, address.device, address.function
                    );
                    let mut selected: NvDevice = core::ptr::null_mut();
                    if by_pci(pci.as_ptr().cast(), &mut selected) == 0 {
                        return selected;
                    }
                }
            }
        }
        // A unique NVIDIA device is unambiguous; never an arbitrary GPU of several.
        let (mut n, mut selected) = (0u32, core::ptr::null_mut());
        match (self.count, self.by_index) {
            (Some(count), Some(by_index))
                if count(&mut n) == 0 && n == 1 && by_index(0, &mut selected) == 0 =>
            {
                selected
            }
            _ => core::ptr::null_mut(),
        }
    }
}
impl Drop for Nvml {
    fn drop(&mut self) {
        // SAFETY: the library this loaded.
        unsafe {
            if self.initialized {
                if let Some(shutdown) = self.shutdown {
                    shutdown();
                }
            }
            if !self.module.is_null() {
                FreeLibrary(self.module);
            }
        }
    }
}

struct Sampler {
    nvml: Nvml,
    nv: NvDevice,
    adapter: Option<IDXGIAdapter3>,
    luid: u64,
    last_idle: u64,
    last_total: u64,
    cpu_known: bool,
    previous_cores: [CoreTimes; 128],
    previous_core_count: u32,
    cpu_name: [u8; 96],
    gpu_name: [u8; 96],
    gpu_total: u64,
}
impl Sampler {
    unsafe fn new() -> Sampler {
        let mut s = Sampler {
            nvml: Nvml::load(),
            nv: core::ptr::null_mut(),
            adapter: None,
            luid: 0,
            last_idle: 0,
            last_total: 0,
            cpu_known: false,
            previous_cores: [CoreTimes::default(); 128],
            previous_core_count: 0,
            cpu_name: [0; 96],
            gpu_name: [0; 96],
            gpu_total: 0,
        };
        let mut name = [0u16; 128];
        let mut size = core::mem::size_of_val(&name) as u32;
        if RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0").as_ptr(),
            wide("ProcessorNameString").as_ptr(),
            RRF_RT_REG_SZ,
            core::ptr::null_mut(),
            name.as_mut_ptr().cast(),
            &mut size,
        ) == 0
        {
            utf8_into(name.as_ptr(), &mut s.cpu_name);
        }
        s
    }
    unsafe fn read(&mut self) -> HardwareStatistics {
        let start = now_ms();
        let mut out = HardwareStatistics::default();
        copy_name(&mut out.cpu_name, &self.cpu_name);
        let wanted = REQUESTED_ADAPTER.load(Acquire);
        if wanted != self.luid {
            self.adapter = None;
            self.nv = core::ptr::null_mut();
            self.gpu_total = 0;
            self.gpu_name[0] = 0;
            self.luid = wanted;
            if wanted != 0 {
                let luid = LUID {
                    LowPart: wanted as u32,
                    HighPart: (wanted >> 32) as i32,
                };
                let adapter = CreateDXGIFactory1::<IDXGIFactory4>()
                    .and_then(|factory| factory.EnumAdapterByLuid::<IDXGIAdapter3>(luid));
                if let Ok(adapter) = adapter {
                    if let Ok(desc) = adapter.GetDesc() {
                        self.gpu_total = desc.DedicatedVideoMemory as u64;
                        utf8_into(desc.Description.as_ptr(), &mut self.gpu_name);
                        if desc.VendorId == 0x10de {
                            self.nv = self.nvml.select(desc.AdapterLuid);
                        }
                    }
                    self.adapter = Some(adapter);
                }
            }
        }
        copy_name(&mut out.gpu_name, &self.gpu_name);
        out.gpu_memory_total = self.gpu_total;
        let (mut idle, mut kernel, mut user) = (
            FileTime::default(),
            FileTime::default(),
            FileTime::default(),
        );
        if GetSystemTimes(&mut idle, &mut kernel, &mut user) != 0 {
            let (i, total) = (ticks(idle), ticks(kernel) + ticks(user));
            if self.cpu_known && total > self.last_total && i >= self.last_idle {
                out.cpu_usage = (100.0
                    * (1.0 - (i - self.last_idle) as f64 / (total - self.last_total) as f64))
                    .clamp(0.0, 100.0);
            }
            self.last_idle = i;
            self.last_total = total;
            self.cpu_known = true;
        }
        type NtQuery = unsafe extern "system" fn(u32, *mut c_void, u32, *mut u32) -> i32;
        let query: Option<NtQuery> = symbol(
            GetModuleHandleW(wide("ntdll.dll").as_ptr()),
            c"NtQuerySystemInformation",
        );
        let mut current = [CoreTimes::default(); 128];
        let mut bytes = 0u32;
        if let Some(query) = query {
            if query(
                PROCESSOR_PERFORMANCE,
                current.as_mut_ptr().cast(),
                core::mem::size_of_val(&current) as u32,
                &mut bytes,
            ) >= 0
            {
                let n = (bytes / core::mem::size_of::<CoreTimes>() as u32).min(128);
                out.core_count = n;
                if n == self.previous_core_count {
                    out.core_maximum = 0.0;
                    for index in 0..n as usize {
                        let (a, b) = (&self.previous_cores[index], &current[index]);
                        let total = b.kernel - a.kernel + b.user - a.user;
                        let idle_ticks = b.idle - a.idle;
                        out.cores[index] = if total > 0 {
                            (100.0 * (1.0 - idle_ticks as f64 / total as f64)).clamp(0.0, 100.0)
                                as f32
                        } else {
                            0.0
                        };
                        out.core_maximum = out.core_maximum.max(out.cores[index] as f64);
                    }
                }
                self.previous_cores = current;
                self.previous_core_count = n;
            }
        }
        let mut memory: MemoryStatus = core::mem::zeroed();
        memory.length = core::mem::size_of::<MemoryStatus>() as u32;
        if GlobalMemoryStatusEx(&mut memory) != 0 {
            out.ram_total = memory.total_phys;
            out.ram_used = memory.total_phys - memory.avail_phys;
        }
        let mut process = ProcessMemoryCounters {
            cb: core::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        if K32GetProcessMemoryInfo(GetCurrentProcess(), &mut process, process.cb) != 0 {
            out.app_working_set = process.working_set as u64;
            out.app_commit = process.private_usage as u64;
        }
        if let Some(adapter) = &self.adapter {
            let mut video = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
            if adapter
                .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut video)
                .is_ok()
            {
                out.gpu_process_memory_valid = true;
                out.gpu_process_used = video.CurrentUsage;
                out.gpu_process_budget = video.Budget;
            }
        }
        if !self.nv.is_null() {
            let (mut usage, mut memory, mut temperature) =
                (NvUsage::default(), NvMemory::default(), 0u32);
            if self.nvml.usage.is_some_and(|f| f(self.nv, &mut usage) == 0) {
                out.gpu_usage = usage.gpu as f64;
            }
            if self
                .nvml
                .memory
                .is_some_and(|f| f(self.nv, &mut memory) == 0)
            {
                out.gpu_memory_valid = true;
                out.gpu_memory_used = memory.used;
                out.gpu_memory_total = memory.total;
            }
            if self
                .nvml
                .temperature
                .is_some_and(|f| f(self.nv, 0, &mut temperature) == 0)
            {
                out.gpu_temperature = temperature as f64;
            }
        }
        out.sample_ms = now_ms() - start;
        out
    }
}

/// Framegen's metrics switch (src/Framegen/Framegen.cpp MetricsEnabled), handed over by every call that can start the
/// monitor.
pub type MetricsEnabled = unsafe extern "C" fn() -> bool;

static REQUESTED_ADAPTER: AtomicU64 = AtomicU64::new(0);
static MONITOR_RUNNING: AtomicBool = AtomicBool::new(false);
struct State {
    stopping: bool,
    snapshot: HardwareStatistics,
    enabled: Option<MetricsEnabled>,
}
// SAFETY: the snapshot is plain data; the callback is a C function.
unsafe impl Send for State {}
static STATE: Mutex<Option<State>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();
static THREAD: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

fn enabled(state: &State) -> bool {
    // SAFETY: framegen's switch.
    state.enabled.is_some_and(|f| unsafe { f() })
}
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let state = guard.get_or_insert_with(|| State {
        stopping: false,
        snapshot: HardwareStatistics::default(),
        enabled: None,
    });
    f(state)
}
fn run() {
    cvr_host::name_this_thread("CVR hardware telemetry");
    // SAFETY: the sampler's Win32/DXGI/NVML calls, all on this thread.
    let mut sampler = unsafe { Sampler::new() };
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        let state = guard
            .as_mut()
            .expect("the state exists while the monitor runs");
        if state.stopping {
            break;
        }
        if !enabled(state) {
            guard = WAKE
                .wait_while(guard, |s| {
                    s.as_ref().is_some_and(|s| !s.stopping && !enabled(s))
                })
                .unwrap_or_else(|p| p.into_inner());
            continue;
        }
        drop(guard);
        // SAFETY: as above.
        let mut next = unsafe { sampler.read() };
        guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
        let state = guard
            .as_mut()
            .expect("the state exists while the monitor runs");
        next.sequence = state.snapshot.sequence + 1;
        state.snapshot = next;
        guard = WAKE
            .wait_timeout_while(guard, Duration::from_secs(1), |s| {
                s.as_ref().is_some_and(|s| !s.stopping && enabled(s))
            })
            .unwrap_or_else(|p| p.into_inner())
            .0;
    }
}
fn wake(metrics: MetricsEnabled) {
    let mut thread = THREAD.lock().unwrap_or_else(|p| p.into_inner());
    let start = with_state(|state| {
        state.enabled = Some(metrics);
        if thread.is_none() && enabled(state) {
            state.stopping = false;
            MONITOR_RUNNING.store(true, SeqCst);
            true
        } else {
            false
        }
    });
    if start {
        *thread = Some(std::thread::spawn(run));
    }
    WAKE.notify_all();
}

/// The adapter of `device` sampled from now on (the monitor woken when it changes or is not running).
///
/// # Safety
/// `device` is a live D3D12 device or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_hw_telemetry_configure(
    device: *mut c_void,
    metrics: MetricsEnabled,
) {
    if device.is_null() || !metrics() {
        return;
    }
    let Some(device) =
        windows::Win32::Graphics::Direct3D12::ID3D12Device::from_raw_borrowed(&device)
    else {
        return;
    };
    let luid = device.GetAdapterLuid();
    let luid = ((luid.HighPart as u32 as u64) << 32) | luid.LowPart as u64;
    if REQUESTED_ADAPTER.swap(luid, AcqRel) != luid || !MONITOR_RUNNING.load(Acquire) {
        wake(metrics);
    }
}
/// The monitor started (when the metrics are on) or woken.
///
/// # Safety
/// `metrics` is framegen's switch.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_hw_telemetry_wake(metrics: MetricsEnabled) {
    wake(metrics);
}
/// The monitor stopped and joined; the snapshot cleared, keeping its sequence.
#[no_mangle]
pub extern "C" fn cvr_gpu_hw_telemetry_stop() {
    let mut thread = THREAD.lock().unwrap_or_else(|p| p.into_inner());
    with_state(|state| state.stopping = true);
    WAKE.notify_all();
    if let Some(handle) = thread.take() {
        let _ = handle.join();
    }
    with_state(|state| {
        let sequence = state.snapshot.sequence;
        state.snapshot = HardwareStatistics {
            sequence,
            ..Default::default()
        };
    });
    MONITOR_RUNNING.store(false, Release);
}
/// The last snapshot.
///
/// # Safety
/// `out` points to a HardwareStatistics.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_hw_telemetry_read(out: *mut HardwareStatistics) {
    *out = with_state(|state| state.snapshot);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_reads_the_machine() {
        // SAFETY: the sampler's own calls.
        let (first, second) = unsafe {
            let mut sampler = Sampler::new();
            let first = sampler.read();
            std::thread::sleep(Duration::from_millis(200));
            (first, sampler.read())
        };
        assert!(first.cpu_name[0] != 0);
        assert!(first.ram_total > 0 && first.ram_used <= first.ram_total);
        assert!(first.app_working_set > 0);
        assert!(first.core_count > 0 && first.cpu_usage == -1.0);
        assert!(second.cpu_usage >= 0.0 && second.core_maximum >= 0.0);
    }
}
