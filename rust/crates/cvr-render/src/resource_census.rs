//! THE D3D12 RESOURCE LIFETIME CENSUS (formerly main's src/Render/ResourceLifetimeCensus.cpp, 1f8f77cf8): what the
//! game and every module in the process create through CreateCommittedResource(1) and CreatePlacedResource while it
//! is switched on, grouped by description, heap, allocation size and caller (and with mode 2 a short stack), each
//! group's created / released / live counts and bytes. The tools ask for a report (tools/resource_census).
//!
//! A resource is followed by a cookie of this module's set on it as private data (SetPrivateDataInterface): the
//! driver releases the cookie with the resource, and the cookie's release counts it out. The cookie holds no
//! resource, heap or device; its release takes no lock, calls no driver method, walks no stack and writes no file.
//! Groups are fixed storage, so a live cookie never points at a moved group; past MaxGroups - 2 a resource lands in
//! its kind's overflow group. The module is pinned once the census is first used: cookies can outlive the plugin.
//!
//! Exports: CyberpunkVR_ResourceCensusEnable (0 off, 1 the API caller, 2 the caller and a stack),
//! CyberpunkVR_ResourceCensusRequest / Completed (a report asked for and published, by number),
//! CyberpunkVR_ResourceCensusReportSeq (odd while it is written), ReportBytes and the report itself (1 MB of JSON).
#![allow(non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::*};
use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};

use windows::core::{Interface, GUID, HRESULT, IUnknown};
use windows::Win32::Graphics::Direct3D12::*;

#[no_mangle]
pub static CyberpunkVR_ResourceCensusEnable: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ResourceCensusRequest: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ResourceCensusCompleted: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ResourceCensusReportSeq: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ResourceCensusReportBytes: AtomicU32 = AtomicU32::new(0);
const REPORT_CAPACITY: usize = 1024 * 1024;
#[no_mangle]
pub static mut CyberpunkVR_ResourceCensusReport: [u8; REPORT_CAPACITY] = [0; REPORT_CAPACITY];

pub const MAX_GROUPS: usize = 512;
pub const STACK_DEPTH: usize = 16;
const NORMAL_GROUPS: usize = MAX_GROUPS - 2;
const MAX_COOKIES: u64 = 131_072;
/// The private data's name on each followed resource.
const LIFETIME_GUID: GUID = GUID::from_values(0x7a6d_d297, 0xf4ab, 0x43c1, [0xa2, 0x8f, 0x66, 0x12, 0xbe, 0x73, 0xe7, 0x09]);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    Committed = 0,
    Placed = 1,
}

/// A group's identity: what was made, on what heap, how large, and by whom.
#[derive(Clone)]
pub struct Key {
    pub kind: Kind,
    pub desc: D3D12_RESOURCE_DESC,
    pub heap: D3D12_HEAP_PROPERTIES,
    pub heap_flags: i32,
    pub allocation_bytes: u64,
    pub caller: u64,
    pub module_base: u64,
    pub module_rva: u64,
    pub module_timestamp: u32,
    pub stack_count: u32,
    pub stack: [u64; STACK_DEPTH],
    pub module: String,
}
impl Default for Key {
    fn default() -> Self {
        Key {
            kind: Kind::Committed,
            desc: D3D12_RESOURCE_DESC::default(),
            heap: D3D12_HEAP_PROPERTIES::default(),
            heap_flags: 0,
            allocation_bytes: 0,
            caller: 0,
            module_base: 0,
            module_rva: 0,
            module_timestamp: 0,
            stack_count: 0,
            stack: [0; STACK_DEPTH],
            module: String::new(),
        }
    }
}
impl Key {
    fn same(&self, b: &Key) -> bool {
        let (x, y) = (&self.desc, &b.desc);
        self.kind == b.kind
            && self.allocation_bytes == b.allocation_bytes
            && self.caller == b.caller
            && self.module_base == b.module_base
            && self.module_timestamp == b.module_timestamp
            && self.stack_count == b.stack_count
            && self.stack == b.stack
            && x.Dimension == y.Dimension
            && x.Alignment == y.Alignment
            && x.Width == y.Width
            && x.Height == y.Height
            && x.DepthOrArraySize == y.DepthOrArraySize
            && x.MipLevels == y.MipLevels
            && x.Format == y.Format
            && x.SampleDesc.Count == y.SampleDesc.Count
            && x.SampleDesc.Quality == y.SampleDesc.Quality
            && x.Layout == y.Layout
            && x.Flags == y.Flags
            && self.heap.Type == b.heap.Type
            && self.heap.CPUPageProperty == b.heap.CPUPageProperty
            && self.heap.MemoryPoolPreference == b.heap.MemoryPoolPreference
            && self.heap.CreationNodeMask == b.heap.CreationNodeMask
            && self.heap.VisibleNodeMask == b.heap.VisibleNodeMask
            && self.heap_flags == b.heap_flags
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Counts {
    pub created: u64,
    pub released: u64,
    pub live: u64,
    pub created_bytes: u64,
    pub released_bytes: u64,
    pub live_bytes: u64,
    pub peak_live_bytes: u64,
}

/// A group's counters: atomics only, a revision and a writer count for a consistent read.
#[derive(Default)]
struct Counters {
    created: AtomicU64,
    released: AtomicU64,
    live: AtomicU64,
    created_bytes: AtomicU64,
    released_bytes: AtomicU64,
    live_bytes: AtomicU64,
    peak: AtomicU64,
    revision: AtomicU64,
    writers: AtomicU64,
}
impl Counters {
    fn created(&self, bytes: u64) {
        self.writers.fetch_add(1, AcqRel);
        self.created.fetch_add(1, Relaxed);
        self.created_bytes.fetch_add(bytes, Relaxed);
        self.live.fetch_add(1, Relaxed);
        let current = self.live_bytes.fetch_add(bytes, Relaxed) + bytes;
        self.peak.fetch_max(current, Relaxed);
        self.revision.fetch_add(1, Release);
        self.writers.fetch_sub(1, Release);
    }
    /// From the driver's private-data destruction: no locks, resource methods, stack walks or file I/O here.
    fn released(&self, bytes: u64) {
        self.writers.fetch_add(1, AcqRel);
        self.released.fetch_add(1, Relaxed);
        self.released_bytes.fetch_add(bytes, Relaxed);
        self.live.fetch_sub(1, Relaxed);
        self.live_bytes.fetch_sub(bytes, Relaxed);
        self.revision.fetch_add(1, Release);
        self.writers.fetch_sub(1, Release);
    }
    fn read(&self) -> (Counts, bool) {
        let mut out = Counts::default();
        for _ in 0..8 {
            let idle = self.writers.load(Acquire) == 0;
            let before = self.revision.load(Acquire);
            out = Counts {
                created: self.created.load(Relaxed),
                released: self.released.load(Relaxed),
                live: self.live.load(Relaxed),
                created_bytes: self.created_bytes.load(Relaxed),
                released_bytes: self.released_bytes.load(Relaxed),
                live_bytes: self.live_bytes.load(Relaxed),
                peak_live_bytes: self.peak.load(Relaxed),
            };
            if idle && before == self.revision.load(Acquire) && self.writers.load(Acquire) == 0 {
                return (out, true);
            }
        }
        (out, false)
    }
}

pub struct Row {
    pub id: u32,
    pub key: Key,
    pub counts: Counts,
    pub overflow: bool,
    pub consistent: bool,
}
#[derive(Default)]
pub struct Snapshot {
    pub groups: Vec<Row>,
    pub live_cookies: u64,
    pub duplicates: u64,
    pub attach_failures: u64,
    pub cookie_limit_drops: u64,
    pub group_overflows: u64,
    pub reentrant_drops: u64,
    pub invalid_allocations: u64,
    pub module_pin_failures: u64,
}

/// One census: fixed group storage, for the process (cookies may be released after the plugin's teardown).
pub struct Census {
    counters: Box<[Counters]>,
    keys: Mutex<Vec<Key>>,
    attachment: Mutex<()>,
    report: Mutex<()>,
    live_cookies: AtomicU64,
    duplicates: AtomicU64,
    attach_failures: AtomicU64,
    cookie_limit_drops: AtomicU64,
    group_overflows: AtomicU64,
    reentrant_drops: AtomicU64,
    invalid_allocations: AtomicU64,
    module_pin_failures: AtomicU64,
}

/// The cookie set on a followed resource: an IUnknown of this module's.
#[repr(C)]
struct Cookie {
    vtbl: *const CookieVtbl,
    refs: AtomicU32,
    census: &'static Census,
    group: usize,
    bytes: u64,
    armed: AtomicBool,
}
#[repr(C)]
struct CookieVtbl {
    query_interface: unsafe extern "system" fn(*mut Cookie, *const GUID, *mut *mut c_void) -> HRESULT,
    add_ref: unsafe extern "system" fn(*mut Cookie) -> u32,
    release: unsafe extern "system" fn(*mut Cookie) -> u32,
}
static COOKIE_VTBL: CookieVtbl = CookieVtbl { query_interface: cookie_query, add_ref: cookie_add_ref, release: cookie_release };
unsafe extern "system" fn cookie_query(this: *mut Cookie, id: *const GUID, out: *mut *mut c_void) -> HRESULT {
    const E_POINTER: HRESULT = HRESULT(0x8000_4003_u32 as i32);
    const E_NOINTERFACE: HRESULT = HRESULT(0x8000_4002_u32 as i32);
    if out.is_null() {
        return E_POINTER;
    }
    *out = core::ptr::null_mut();
    if id.is_null() || *id != IUnknown::IID {
        return E_NOINTERFACE;
    }
    *out = this.cast();
    cookie_add_ref(this);
    HRESULT(0)
}
unsafe extern "system" fn cookie_add_ref(this: *mut Cookie) -> u32 {
    (*this).refs.fetch_add(1, Relaxed) + 1
}
unsafe extern "system" fn cookie_release(this: *mut Cookie) -> u32 {
    let left = (*this).refs.fetch_sub(1, AcqRel) - 1;
    if left == 0 {
        let cookie = Box::from_raw(this);
        if cookie.armed.load(Acquire) {
            cookie.census.counters[cookie.group].released(cookie.bytes);
        }
        cookie.census.live_cookies.fetch_sub(1, Relaxed);
    }
    left
}

thread_local! {
    static OBSERVING: Cell<bool> = const { Cell::new(false) };
}

/// The plugin manager can unload before graphics objects die; their private data still calls the cookie's release, so
/// the code is pinned once the census is first used. This retains no resource, heap, device or queue.
fn keep_observer_code_alive() -> bool {
    static PINNED: OnceLock<bool> = OnceLock::new();
    *PINNED.get_or_init(|| {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN};
        let mut module = core::ptr::null_mut();
        // SAFETY: an address in this module; the out pointer local.
        unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
                cookie_release as *const () as *const u16,
                &mut module,
            ) != 0
        }
    })
}

/// The caller's module: its base, the caller's RVA, its file and its link time stamp.
fn caller(key: &mut Key, address: u64) {
    use windows_sys::Win32::System::LibraryLoader::{
        GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    };
    key.caller = address;
    let mut module = core::ptr::null_mut();
    // SAFETY: a lookup by address; the out pointer local.
    let found = address != 0
        && unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                address as *const u16,
                &mut module,
            ) != 0
        };
    if !found {
        key.module_rva = key.caller;
        key.module = "<unknown caller module>".to_string();
        return;
    }
    key.module_base = module as u64;
    key.module_rva = key.caller - key.module_base;
    let mut name = [0u16; 512];
    // SAFETY: the buffer and its size.
    let n = unsafe { GetModuleFileNameW(module, name.as_mut_ptr(), 512) } as usize;
    key.module = String::from_utf16_lossy(&name[..n.min(511)]);
    // the module's image: its PE header's time stamp
    let base = key.module_base as usize;
    if cvr_engine::code::read::<u16>(base) == Some(0x5A4D) {
        if let Some(lfanew) = cvr_engine::code::read::<u32>(base + 0x3C) {
            let nt = base + lfanew as usize;
            if cvr_engine::code::read::<u32>(nt) == Some(0x4550) {
                key.module_timestamp = cvr_engine::code::read::<u32>(nt + 8).unwrap_or(0);
            }
        }
    }
}

#[inline(never)]
fn trace(key: &mut Key) {
    let mut frames = [core::ptr::null_mut::<c_void>(); STACK_DEPTH];
    // SAFETY: a back trace into a local buffer.
    let count = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(3, &mut frames, None) } as usize;
    key.stack_count = count as u32;
    for (slot, frame) in key.stack.iter_mut().zip(&frames[..count]) {
        *slot = *frame as u64;
    }
}

impl Census {
    pub fn new() -> &'static Census {
        Box::leak(Box::new(Census {
            counters: (0..MAX_GROUPS).map(|_| Counters::default()).collect(),
            keys: Mutex::new(Vec::with_capacity(NORMAL_GROUPS)),
            attachment: Mutex::new(()),
            report: Mutex::new(()),
            live_cookies: AtomicU64::new(0),
            duplicates: AtomicU64::new(0),
            attach_failures: AtomicU64::new(0),
            cookie_limit_drops: AtomicU64::new(0),
            group_overflows: AtomicU64::new(0),
            reentrant_drops: AtomicU64::new(0),
            invalid_allocations: AtomicU64::new(0),
            module_pin_failures: AtomicU64::new(0),
        }))
    }

    fn group(&self, key: Key) -> usize {
        let mut keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(i) = keys.iter().position(|k| k.same(&key)) {
            return i;
        }
        if keys.len() < NORMAL_GROUPS {
            keys.push(key);
            return keys.len() - 1;
        }
        self.group_overflows.fetch_add(1, Relaxed);
        NORMAL_GROUPS + key.kind as usize
    }

    /// Follows `resource` (mode 0: nothing). False when it is not followed: off, already followed (a base/API1
    /// wrapper's second creation call), over the cookie limit, or the private data refused.
    #[allow(clippy::too_many_arguments)]
    pub fn observe(
        &'static self,
        mode: u32,
        resource: &ID3D12Resource,
        kind: Kind,
        desc: &D3D12_RESOURCE_DESC,
        heap: &D3D12_HEAP_PROPERTIES,
        heap_flags: i32,
        bytes: u64,
        api_caller: u64,
    ) -> bool {
        if mode == 0 {
            return false;
        }
        if OBSERVING.with(Cell::get) {
            self.reentrant_drops.fetch_add(1, Relaxed);
            return false;
        }
        OBSERVING.with(|o| o.set(true));
        let followed = self.observe_unguarded(mode, resource, kind, desc, heap, heap_flags, bytes, api_caller);
        OBSERVING.with(|o| o.set(false));
        followed
    }
    #[allow(clippy::too_many_arguments)]
    fn observe_unguarded(
        &'static self,
        mode: u32,
        resource: &ID3D12Resource,
        kind: Kind,
        desc: &D3D12_RESOURCE_DESC,
        heap: &D3D12_HEAP_PROPERTIES,
        heap_flags: i32,
        bytes: u64,
        api_caller: u64,
    ) -> bool {
        if !keep_observer_code_alive() {
            self.module_pin_failures.fetch_add(1, Relaxed);
            return false;
        }
        if bytes == 0 || bytes == u64::MAX {
            self.invalid_allocations.fetch_add(1, Relaxed);
            return false;
        }
        let mut key = Key { kind, desc: *desc, heap: *heap, heap_flags, allocation_bytes: bytes, ..Default::default() };
        caller(&mut key, api_caller);
        if mode >= 2 {
            trace(&mut key);
        }
        // Duplicate observations of one object serialized; the registry's lock never held over a driver call.
        let _attach = self.attachment.lock().unwrap_or_else(|e| e.into_inner());
        let mut existing: *mut c_void = core::ptr::null_mut();
        let mut size = core::mem::size_of::<*mut c_void>() as u32;
        // SAFETY: the resource's own private data, an interface pointer (a reference given to us) when present.
        let found = unsafe { resource.GetPrivateData(&LIFETIME_GUID, &mut size, Some((&raw mut existing).cast())) }.is_ok();
        if found && size as usize == core::mem::size_of::<*mut c_void>() && !existing.is_null() {
            // SAFETY: the reference GetPrivateData gave.
            drop(unsafe { IUnknown::from_raw(existing) });
            self.duplicates.fetch_add(1, Relaxed);
            return false;
        }
        if self.live_cookies.fetch_add(1, Relaxed) >= MAX_COOKIES {
            self.live_cookies.fetch_sub(1, Relaxed);
            self.cookie_limit_drops.fetch_add(1, Relaxed);
            return false;
        }
        let group = self.group(key);
        let cookie = Box::into_raw(Box::new(Cookie {
            vtbl: &COOKIE_VTBL,
            refs: AtomicU32::new(1),
            census: self,
            group,
            bytes,
            armed: AtomicBool::new(false),
        }));
        let pointer = cookie.cast::<c_void>();
        // SAFETY: the cookie is an IUnknown of ours, borrowed for the call (the resource takes its own reference).
        let attached = unsafe {
            let unknown = IUnknown::from_raw_borrowed(&pointer).expect("a cookie");
            resource.SetPrivateDataInterface(&LIFETIME_GUID, unknown).is_ok()
        };
        if attached {
            self.counters[group].created(bytes);
            // SAFETY: the cookie lives while the resource (or our reference below) holds it.
            unsafe { (*cookie).armed.store(true, Release) };
        } else {
            self.attach_failures.fetch_add(1, Relaxed);
        }
        // SAFETY: our own first reference.
        unsafe { cookie_release(cookie) };
        attached
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut out = Snapshot::default();
        {
            let keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
            for i in 0..MAX_GROUPS {
                if i >= keys.len() && i < NORMAL_GROUPS {
                    continue;
                }
                let (counts, consistent) = self.counters[i].read();
                if counts.created == 0 {
                    continue;
                }
                let key = if i < NORMAL_GROUPS {
                    keys[i].clone()
                } else {
                    Key {
                        kind: if i == NORMAL_GROUPS { Kind::Committed } else { Kind::Placed },
                        module: "<group capacity overflow>".to_string(),
                        ..Default::default()
                    }
                };
                out.groups.push(Row { id: i as u32, key, counts, overflow: i >= NORMAL_GROUPS, consistent });
            }
        }
        out.live_cookies = self.live_cookies.load(SeqCst);
        out.duplicates = self.duplicates.load(SeqCst);
        out.attach_failures = self.attach_failures.load(SeqCst);
        out.cookie_limit_drops = self.cookie_limit_drops.load(SeqCst);
        out.group_overflows = self.group_overflows.load(SeqCst);
        out.reentrant_drops = self.reentrant_drops.load(SeqCst);
        out.invalid_allocations = self.invalid_allocations.load(SeqCst);
        out.module_pin_failures = self.module_pin_failures.load(SeqCst);
        out
    }

    /// The report (schema 1, main's): the scope, the census's own counts, then every group.
    pub fn json(&self, mode: u32) -> String {
        let s = self.snapshot();
        let mut out = String::with_capacity(256 * 1024);
        out.push_str(
            "{\"schema\":1,\"scope\":\"resources created while enabled; committed allocations are not residency; placed footprints are separate\",\
             \"coveredApis\":[\"CreateCommittedResource\",\"CreateCommittedResource1\",\"CreatePlacedResource\"]",
        );
        let number = |out: &mut String, key: &str, value: u64| {
            out.push(',');
            quoted(out, key);
            let _ = write!(out, ":{value}");
        };
        let hex = |out: &mut String, key: &str, value: u64| {
            out.push(',');
            quoted(out, key);
            out.push(':');
            quoted(out, &format!("0x{value:x}"));
        };
        number(&mut out, "pid", u64::from(std::process::id()));
        number(&mut out, "mode", u64::from(mode));
        // SAFETY: a plain Win32 call.
        number(&mut out, "tickMs", unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() });
        number(&mut out, "maxGroups", MAX_GROUPS as u64);
        number(&mut out, "maxStackDepth", STACK_DEPTH as u64);
        number(&mut out, "liveCookies", s.live_cookies);
        number(&mut out, "duplicates", s.duplicates);
        number(&mut out, "attachFailures", s.attach_failures);
        number(&mut out, "cookieLimitDrops", s.cookie_limit_drops);
        number(&mut out, "groupOverflows", s.group_overflows);
        number(&mut out, "reentrantDrops", s.reentrant_drops);
        number(&mut out, "invalidAllocations", s.invalid_allocations);
        number(&mut out, "modulePinFailures", s.module_pin_failures);
        out.push_str(",\"groups\":[");
        for (i, row) in s.groups.iter().enumerate() {
            if i != 0 {
                out.push(',');
            }
            let (k, d, c) = (&row.key, &row.key.desc, &row.counts);
            out.push('{');
            quoted(&mut out, "kind");
            out.push(':');
            quoted(&mut out, if k.kind == Kind::Committed { "committed" } else { "placed-footprint" });
            number(&mut out, "groupId", u64::from(row.id));
            number(&mut out, "overflow", u64::from(row.overflow));
            number(&mut out, "consistent", u64::from(row.consistent));
            number(&mut out, "heapType", k.heap.Type.0 as u32 as u64);
            number(&mut out, "cpuPageProperty", k.heap.CPUPageProperty.0 as u32 as u64);
            number(&mut out, "memoryPoolPreference", k.heap.MemoryPoolPreference.0 as u32 as u64);
            number(&mut out, "creationNodeMask", u64::from(k.heap.CreationNodeMask));
            number(&mut out, "visibleNodeMask", u64::from(k.heap.VisibleNodeMask));
            number(&mut out, "heapFlags", k.heap_flags as u32 as u64);
            number(&mut out, "dimension", d.Dimension.0 as u32 as u64);
            number(&mut out, "width", d.Width);
            number(&mut out, "height", u64::from(d.Height));
            number(&mut out, "depthOrArray", u64::from(d.DepthOrArraySize));
            number(&mut out, "mips", u64::from(d.MipLevels));
            number(&mut out, "format", d.Format.0 as u32 as u64);
            number(&mut out, "flags", d.Flags.0 as u32 as u64);
            number(&mut out, "samples", u64::from(d.SampleDesc.Count));
            number(&mut out, "sampleQuality", u64::from(d.SampleDesc.Quality));
            number(&mut out, "alignment", d.Alignment);
            number(&mut out, "layout", d.Layout.0 as u32 as u64);
            number(&mut out, "allocationBytes", k.allocation_bytes);
            out.push_str(",\"callerModule\":");
            quoted(&mut out, &k.module);
            hex(&mut out, "caller", k.caller);
            hex(&mut out, "moduleBase", k.module_base);
            hex(&mut out, "callerRva", k.module_rva);
            number(&mut out, "moduleTimestamp", u64::from(k.module_timestamp));
            number(&mut out, "created", c.created);
            number(&mut out, "released", c.released);
            number(&mut out, "live", c.live);
            number(&mut out, "createdBytes", c.created_bytes);
            number(&mut out, "releasedBytes", c.released_bytes);
            number(&mut out, "liveBytes", c.live_bytes);
            number(&mut out, "peakLiveBytes", c.peak_live_bytes);
            out.push_str(",\"stack\":[");
            for (j, frame) in k.stack[..k.stack_count as usize].iter().enumerate() {
                if j != 0 {
                    out.push(',');
                }
                let _ = write!(out, "\"0x{frame:x}\"");
            }
            out.push_str("]}");
        }
        out.push_str("]}");
        out
    }
}

fn quoted(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' | '\\' => {
                out.push('\\');
                out.push(ch);
            }
            c if (c as u32) < 32 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The process's census (allocated on first use).
fn census() -> &'static Census {
    static CENSUS: OnceLock<&'static Census> = OnceLock::new();
    CENSUS.get_or_init(Census::new)
}

/// Whether resources are followed now.
pub fn enabled() -> bool {
    CyberpunkVR_ResourceCensusEnable.load(Relaxed) != 0
}
/// The device hooks' call after a successful creation (device_creation.rs).
///
/// # Safety
/// `resource` is the creation's out pointer: a live ID3D12Resource.
#[allow(clippy::too_many_arguments)]
pub unsafe fn observe(
    resource: *mut c_void,
    kind: Kind,
    desc: *const D3D12_RESOURCE_DESC,
    heap: &D3D12_HEAP_PROPERTIES,
    heap_flags: i32,
    bytes: u64,
    caller: u64,
) {
    let mode = CyberpunkVR_ResourceCensusEnable.load(Relaxed);
    let (Some(resource), Some(desc)) = (ID3D12Resource::from_raw_borrowed(&resource), desc.as_ref()) else { return };
    if mode != 0 {
        census().observe(mode, resource, kind, desc, heap, heap_flags, bytes, caller);
    }
}

/// The Present poll: a report asked for (Request beyond Completed) is written and published; nothing else.
pub fn poll() {
    let request = CyberpunkVR_ResourceCensusRequest.load(Relaxed);
    if request == 0 || request == CyberpunkVR_ResourceCensusCompleted.load(Acquire) {
        return;
    }
    let c = census();
    let Ok(_report) = c.report.try_lock() else { return };
    let mut report = c.json(CyberpunkVR_ResourceCensusEnable.load(Relaxed));
    if report.len() >= REPORT_CAPACITY {
        report = "{\"schema\":1,\"error\":\"bounded report capacity exceeded\"}".to_string();
    }
    CyberpunkVR_ResourceCensusReportSeq.fetch_add(1, AcqRel);
    // SAFETY: the export's own buffer, the report shorter than it; readers follow the sequence.
    unsafe {
        let buffer = &raw mut CyberpunkVR_ResourceCensusReport;
        core::ptr::copy_nonoverlapping(report.as_ptr(), (*buffer).as_mut_ptr(), report.len());
        (*buffer)[report.len()] = 0;
    }
    CyberpunkVR_ResourceCensusReportBytes.store(report.len() as u32, Relaxed);
    CyberpunkVR_ResourceCensusReportSeq.fetch_add(1, Release);
    CyberpunkVR_ResourceCensusCompleted.store(request, Release);
}

#[cfg(test)]
mod tests {
    //! main's tools/resource_census_tests (lifetime, overflow, concurrent, duplicate_concurrent) on WARP, each with a
    //! census of its own.
    use super::*;
    use cvr_gpu::bench::Bench;

    fn properties() -> D3D12_HEAP_PROPERTIES {
        D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, CreationNodeMask: 1, VisibleNodeMask: 1, ..Default::default() }
    }
    fn buffer(width: u64) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Width: width,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            ..Default::default()
        }
    }
    fn create(device: &ID3D12Device, desc: &D3D12_RESOURCE_DESC) -> ID3D12Resource {
        let mut r: Option<ID3D12Resource> = None;
        // SAFETY: descriptions on the stack.
        unsafe { device.CreateCommittedResource(&properties(), D3D12_HEAP_FLAG_NONE, desc, D3D12_RESOURCE_STATE_COMMON, None, &mut r) }.unwrap();
        r.unwrap()
    }
    fn bytes(device: &ID3D12Device, desc: &D3D12_RESOURCE_DESC) -> u64 {
        // SAFETY: a plain query.
        unsafe { device.GetResourceAllocationInfo(0, &[*desc]) }.SizeInBytes
    }
    fn totals(s: &Snapshot, kind: Kind) -> Counts {
        let mut t = Counts::default();
        for row in s.groups.iter().filter(|r| r.key.kind == kind) {
            assert!(row.consistent, "a quiescent snapshot inconsistent");
            t.created += row.counts.created;
            t.released += row.counts.released;
            t.live += row.counts.live;
            t.created_bytes += row.counts.created_bytes;
            t.released_bytes += row.counts.released_bytes;
            t.live_bytes += row.counts.live_bytes;
        }
        t
    }
    const CALLER: u64 = 0x1234;

    #[test]
    fn lifetime_duplicates_and_placed_footprints() {
        let b = Bench::warp();
        let device = b.device.clone();
        let census = Census::new();
        let desc = buffer(65536);
        let resource = create(&device, &desc);
        let size = bytes(&device, &desc);
        assert!(!census.observe(0, &resource, Kind::Committed, &desc, &properties(), 0, size, CALLER), "off, yet followed");
        assert!(census.snapshot().groups.is_empty());
        assert!(census.observe(1, &resource, Kind::Committed, &desc, &properties(), 0, size, CALLER));
        assert!(!census.observe(1, &resource, Kind::Committed, &desc, &properties(), 0, size, CALLER), "a duplicate counted");
        let consumer = resource.clone();
        drop(resource);
        let held = census.snapshot();
        let before = totals(&held, Kind::Committed);
        assert_eq!((before.created, before.live, before.released, before.live_bytes), (1, 1, 0, 65536));
        assert_eq!(held.duplicates, 1);
        drop(consumer);
        let after = totals(&census.snapshot(), Kind::Committed);
        assert_eq!((after.released, after.live, after.live_bytes), (1, 0, 0), "the cookie kept the resource or did not retire");
        assert_eq!(after.created_bytes, after.released_bytes);

        // placed footprints: resource identities, not separately committed memory
        let heap_desc = D3D12_HEAP_DESC { SizeInBytes: 131_072, Properties: properties(), Flags: D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS, ..Default::default() };
        let mut heap: Option<ID3D12Heap> = None;
        // SAFETY: the description on the stack.
        unsafe { device.CreateHeap(&heap_desc, &mut heap) }.unwrap();
        let heap = heap.unwrap();
        let mut placed = Vec::new();
        for i in 0..2u64 {
            let mut r: Option<ID3D12Resource> = None;
            // SAFETY: as above.
            unsafe { device.CreatePlacedResource(&heap, i * 65536, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut r) }.unwrap();
            let r = r.unwrap();
            assert!(census.observe(2, &r, Kind::Placed, &desc, &properties(), D3D12_HEAP_FLAG_ALLOW_ONLY_BUFFERS.0, size, CALLER));
            placed.push(r);
        }
        let s = census.snapshot();
        let footprints = totals(&s, Kind::Placed);
        assert_eq!((footprints.live, footprints.live_bytes), (2, 131_072));
        assert_eq!(totals(&s, Kind::Committed).live_bytes, 0);
        assert!(s.groups.iter().any(|r| r.key.stack_count > 0), "mode 2 took no stack");
        drop(heap);
        drop(placed);
        assert_eq!(census.snapshot().live_cookies, 0, "a resource, heap or cookie kept");
        let report = census.json(0);
        assert!(report.contains("placed-footprint") && report.contains("callerRva") && report.starts_with("{\"schema\":1"));
    }

    #[test]
    fn overflow_keeps_counting() {
        let b = Bench::warp();
        let device = b.device.clone();
        let census = Census::new();
        for i in 0..540u64 {
            let desc = buffer(i + 1);
            let r = create(&device, &desc);
            assert!(census.observe(1, &r, Kind::Committed, &desc, &properties(), 0, bytes(&device, &desc), CALLER));
        }
        let s = census.snapshot();
        let t = totals(&s, Kind::Committed);
        assert!(s.groups.len() <= MAX_GROUPS && s.group_overflows > 0);
        assert_eq!((t.created, t.released, t.live_bytes, s.live_cookies), (540, 540, 0, 0));
    }

    #[test]
    fn concurrent_lifetimes_and_one_cookie_per_resource() {
        let b = Bench::warp();
        let device = b.device.clone();
        let census = Census::new();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let device = device.clone();
                scope.spawn(move || {
                    for _ in 0..100 {
                        let desc = buffer(65536);
                        let r = create(&device, &desc);
                        assert!(census.observe(1, &r, Kind::Committed, &desc, &properties(), 0, bytes(&device, &desc), CALLER));
                    }
                });
            }
            for _ in 0..50 {
                for row in census.snapshot().groups.iter().filter(|r| r.consistent) {
                    let c = row.counts;
                    assert_eq!(c.created - c.released, c.live);
                    assert_eq!(c.created_bytes - c.released_bytes, c.live_bytes);
                }
                std::thread::yield_now();
            }
        });
        let s = census.snapshot();
        let t = totals(&s, Kind::Committed);
        assert_eq!((t.created, t.released, t.live, s.live_cookies), (400, 400, 0, 0));

        // one resource seen by four threads at once: one cookie
        let census = Census::new();
        let desc = buffer(65536);
        let resource = create(&device, &desc);
        let size = bytes(&device, &desc);
        let attached = AtomicU32::new(0);
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(|| {
                    for _ in 0..100 {
                        if census.observe(1, &resource, Kind::Committed, &desc, &properties(), 0, size, CALLER) {
                            attached.fetch_add(1, Relaxed);
                        }
                    }
                });
            }
        });
        let s = census.snapshot();
        let t = totals(&s, Kind::Committed);
        assert_eq!((attached.load(Relaxed), t.created, t.live, s.duplicates), (1, 1, 1, 399));
        drop(resource);
        let t = totals(&census.snapshot(), Kind::Committed);
        assert_eq!((t.released, t.live, census.snapshot().live_cookies), (1, 0, 0));
    }
}
