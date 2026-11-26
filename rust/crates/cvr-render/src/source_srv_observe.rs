//! THE SOURCE SRV BINDING'S OBSERVER (formerly src/Render/NativeSourceSrvBinding.cpp): the candidates the engine's two
//! SRV setters announce (the individual buffer SRV 0x338FC0 and the cached SRV group 0x338ED4, through the thunks in
//! asm_thunks/native_source_srv_binding_thunk.rs that call CvrSourceSrvBefore with the setter's registers), kept per
//! list, generation and register under an epoch; and the capture at a native draw that proves a held SRV is the
//! candidate's (the root binding, the native slot and record, the CPU descriptor, the default-heap buffer and its view
//! range, the native read state, the allocation and its allocator), read twice. The rules each step applies are
//! source_srv.rs'.
//!
//! The thunks' globals (the originals and the extended state the thunks save) are defined here; the C++ API
//! (include/Render/NativeSourceSrvBinding.hpp) forwards.
use core::cell::Cell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12GraphicsCommandList, ID3D12Resource, D3D12_HEAP_FLAGS, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE_DEFAULT,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_SHADER_RESOURCE_VIEW_DESC, D3D12_SRV_DIMENSION_BUFFER,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;

use crate::buffer_read::NativeBuffer;
#[cfg(any(feature = "rust-build", feature = "probe-exports"))]
use crate::declared_moved_exports::CyberpunkVR_SourceSrvBindingEnable;
/// The unit tests' switch (the plugin's and the probe tests' is declared_moved_exports').
#[cfg(not(any(feature = "rust-build", feature = "probe-exports")))]
static CyberpunkVR_SourceSrvBindingEnable: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
use crate::native_recording::{Context, Lease};
use crate::source_cbv_binding::DescriptorStamp;
use crate::source_cbv_details::{read_allocation, read_allocator, read_context, Allocation, Allocator, ReadState};
use crate::source_srv::{
    cached_group, decode_candidate, matches_mode, read_record, read_slot, relevant, tracked_register, valid_xstate, view_range, CandidateInput,
    FrameRegisters, Reader, Slot, CACHED, INDIVIDUAL,
};

// The thunks' globals.
#[no_mangle]
pub static mut CvrSourceSrvIndividualOriginal: *mut c_void = core::ptr::null_mut();
#[no_mangle]
pub static mut CvrSourceSrvCachedOriginal: *mut c_void = core::ptr::null_mut();
#[no_mangle]
pub static mut CvrSourceSrvXstateMask: u64 = 0;
#[no_mangle]
pub static mut CvrSourceSrvXstateBytes: u32 = 512;
#[no_mangle]
pub static mut CvrSourceSrvXstateMode: u32 = 0;
#[no_mangle]
pub static mut CvrSourceSrvXstateReady: u32 = 0;

/// source_srv::Status.
mod status {
    pub const IGNORED: u32 = 0;
    pub const CAPTURED: u32 = 1;
    pub const UNSUPPORTED_EXECUTABLE: u32 = 2;
    pub const MISSING_CONTEXT: u32 = 3;
    pub const MISSING_OBSERVATION: u32 = 4;
    pub const INVALID_BINDING: u32 = 5;
    pub const DESCRIPTOR_MISMATCH: u32 = 6;
    pub const CHANGED: u32 = 7;
    pub const CAPACITY: u32 = 8;
    pub const COVERAGE_LOST: u32 = 9;
    pub const FAILURE: u32 = 10;
    pub const CANDIDATE: u32 = 11;
}
const ROOT_PARAMETER: u32 = 23;
const STAGE_INDEX: u32 = 1;
const CAPACITY: usize = 8192;
const SRV_RANGE: u32 = 0;
const PIXEL_VISIBILITY: u32 = 5;
/// The setters' contract: the header's rows (rva, bytes, FNV-1a).
const CONTRACT: [(u32, u32, u64); 9] = [
    (0x338FC0, 400, 0x51B6659A1B7482CA),
    (0x338ED4, 224, 0x7C30C7D822B3C01D),
    (0x339E8C, 147, 0x28D70ABB4E1F3623),
    (0x3393A0, 103, 0x892D112D7E8F25F9),
    (0x3364A8, 791, 0xA8C2FE51FFB12DA9),
    (0x337230, 51, 0x0396F3E29AEB7926),
    (0x337DA0, 384, 0xB36B2D868965D718),
    (0x338390, 223, 0x262EAAB2CEC6F5E0),
    (0x1EAF30, 310, 0xE80917A196B71137),
];

const READER: Reader = Some(crate::ia_binding::cvr_iab_read);
fn read(address: u64, out: &mut [u8]) -> bool {
    crate::geometry_packets::read(address, out)
}
fn mem(address: u64, out: &mut [u8]) -> bool {
    read(address, out)
}
fn base() -> u64 {
    // SAFETY: the game's own module.
    unsafe { GetModuleHandleW(None) }.map_or(0, |m| m.0 as u64)
}
fn thread() -> u32 {
    // SAFETY: no preconditions.
    unsafe { GetCurrentThreadId() }
}
fn field_u64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn tls() -> u64 {
    let tls: u64;
    // SAFETY: the thread's TEB slot array pointer.
    unsafe { core::arch::asm!("mov {}, gs:[0x58]", out(reg) tls, options(nostack, readonly, preserves_flags)) };
    tls
}
fn current() -> Context {
    match crate::source_cbv_reads::current(&read, tls()) {
        Some((context, state, list)) => Context { context, state, list },
        None => Context::default(),
    }
}
fn valid(context: &Context) -> bool {
    context.context != 0 && context.state != 0 && context.list != 0
}
fn read_registry() -> Option<u64> {
    let game = base();
    if game == 0 || game > u64::MAX - 0x3BCDCB0 {
        return None;
    }
    let mut b = [0u8; 8];
    read(game + 0x3BCDCA8, &mut b).then(|| u64::from_le_bytes(b)).filter(|&r| r != 0)
}
fn same_recording(a: &Lease, b: &Lease) -> bool {
    !a.token.is_null() && a.token == b.token && a.list == b.list && a.context == b.context && a.registry == b.registry && a.generation == b.generation
}

// ---- references

fn add_ref(object: *mut c_void) -> *mut c_void {
    // SAFETY: a live COM object or null.
    if let Some(o) = unsafe { windows::core::IUnknown::from_raw_borrowed(&object) } {
        core::mem::forget(o.clone());
    }
    object
}
fn release(object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: a reference this module owns, released once.
        drop(unsafe { windows::core::IUnknown::from_raw(object) });
    }
}
/// A native recording lease whose token reference is owned.
struct OwnedLease(Lease);
// SAFETY: the token is a free-threaded COM object (the C++ shared its ComPtr across threads).
unsafe impl Send for OwnedLease {}
impl Clone for OwnedLease {
    fn clone(&self) -> Self {
        add_ref(self.0.token);
        OwnedLease(Lease { ..self.0 })
    }
}
impl Drop for OwnedLease {
    fn drop(&mut self) {
        release(self.0.token);
    }
}

// ---- the code guards and the extended state

static CODE: Mutex<(bool, bool, u64)> = Mutex::new((false, false, 0));
fn compatible() -> bool {
    let code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    code.1 && code.2 == base()
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_prepare_code_guards(game: u64) {
    let mut code = CODE.lock().unwrap_or_else(|e| e.into_inner());
    if code.0 {
        return;
    }
    code.0 = true;
    code.2 = game;
    #[repr(C)]
    struct Row {
        rva: u32,
        bytes: u32,
        fnv: u64,
    }
    let rows: Vec<Row> = CONTRACT.iter().map(|&(rva, bytes, fnv)| Row { rva, bytes, fnv }).collect();
    // SAFETY: the rows as code_tables.rs's Row (16 bytes each).
    code.1 = game != 0 && game == base() && unsafe { crate::code_tables::cvr_code_rows(game, rows.as_ptr().cast(), rows.len(), 1024) };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_compatible() -> bool {
    compatible()
}
/// ConfigureXstate: the extended state the thunks save around CvrSourceSrvBefore (XSAVE when the OS enables it), once.
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_configure_xstate() -> bool {
    static CONFIGURED: OnceLock<bool> = OnceLock::new();
    *CONFIGURED.get_or_init(|| {
        // SAFETY: the thunks read these only once Ready is set; this runs once.
        unsafe {
            CvrSourceSrvXstateReady = 0;
            CvrSourceSrvXstateMask = 0;
            CvrSourceSrvXstateBytes = 512;
            CvrSourceSrvXstateMode = 0;
            let maximum = core::arch::x86_64::__cpuid(0).eax;
            let features = core::arch::x86_64::__cpuid(1).ecx;
            if features & (1 << 27) != 0 {
                if features & (1 << 26) == 0 || maximum < 0xD {
                    return false;
                }
                let (low, high): (u32, u32);
                core::arch::asm!("xgetbv", in("ecx") 0u32, out("eax") low, out("edx") high, options(nomem, nostack, preserves_flags));
                let enabled = (high as u64) << 32 | low as u64;
                let leaf = core::arch::x86_64::__cpuid_count(0xD, 0);
                let supported = leaf.eax as u64 | (leaf.edx as u64) << 32;
                if !valid_xstate(enabled, supported, leaf.ebx as u64) {
                    return false;
                }
                CvrSourceSrvXstateMask = enabled;
                CvrSourceSrvXstateBytes = leaf.ebx;
                CvrSourceSrvXstateMode = 1;
            }
            CvrSourceSrvXstateReady = 1;
        }
        true
    })
}

// ---- the candidates

#[derive(Clone)]
struct Candidate {
    input: CandidateInput,
    context: Context,
    recording: OwnedLease,
    registry: u64,
    serial: u64,
    epoch: u64,
    thread: u32,
}
type Key = (u64, u64, u32);
struct Candidates {
    map: BTreeMap<Key, Candidate>,
    next_serial: u64,
}
static CANDIDATES: Mutex<Candidates> = Mutex::new(Candidates { map: BTreeMap::new(), next_serial: 0 });
static LOST: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(1);
fn candidates() -> std::sync::MutexGuard<'static, Candidates> {
    CANDIDATES.lock().unwrap_or_else(|e| e.into_inner())
}
thread_local! {
    static LAST: Cell<u32> = const { Cell::new(0) };
}
fn set_last(value: u32) {
    LAST.set(value);
}
fn enabled() -> bool {
    CyberpunkVR_SourceSrvBindingEnable.load(Relaxed) != 0
}
fn reset_list(list: u64) {
    let mut c = candidates();
    c.map.retain(|k, _| k.0 != list);
}

/// CvrSourceSrvBefore: the setter's candidates (the individual route's one register, the cached route's three) recorded
/// for the current list, under the epoch observed before the reads.
///
/// # Safety
/// `frame` is the thunk's frame or null.
#[no_mangle]
pub unsafe extern "C-unwind" fn CvrSourceSrvBefore(route: u32, frame: *const FrameRegisters) {
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        set_last(status::IGNORED);
        // SAFETY: as documented (the frame's leading registers).
        let Some(frame) = (unsafe { frame.as_ref() }) else {
            return;
        };
        if route > CACHED || !enabled() || !relevant(route, frame) || cvr_host::probe::internal() {
            return;
        }
        set_last(status::UNSUPPORTED_EXECUTABLE);
        if !compatible() {
            return;
        }
        if LOST.load(Acquire) {
            set_last(status::COVERAGE_LOST);
            return;
        }
        let observed_epoch = EPOCH.load(Acquire);
        let context = current();
        set_last(status::MISSING_CONTEXT);
        if !valid(&context) {
            LOST.store(true, Release);
            return;
        }
        let mut lease = OwnedLease(Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 });
        // SAFETY: the context's list; our empty lease.
        if !unsafe { crate::native_recording::cvr_native_recording_capture(context.list as *mut c_void, &context, &mut lease.0) } {
            reset_list(context.list);
            return;
        }
        let Some(registry) = read_registry().filter(|&r| r == lease.0.registry) else {
            reset_list(context.list);
            set_last(status::INVALID_BINDING);
            return;
        };
        let first = if route == INDIVIDUAL { frame.rdx as u32 } else { 4 };
        let end = if route == INDIVIDUAL { first.wrapping_add(1) } else { 7 };
        let mut inputs = [CandidateInput::default(); 3];
        let mut valid_inputs = [false; 3];
        for reg in first..end {
            if let Some(input) = decode_candidate(route, frame, registry, READER, core::ptr::null_mut(), reg) {
                inputs[(reg - 4) as usize] = input;
                valid_inputs[(reg - 4) as usize] = true;
            }
        }
        let mut c = candidates();
        if observed_epoch != EPOCH.load(Acquire) || !enabled() {
            return;
        }
        for reg in first..end {
            let key: Key = (context.list, lease.0.generation, reg);
            let at = (reg - 4) as usize;
            if !valid_inputs[at] {
                c.map.remove(&key);
                set_last(status::INVALID_BINDING);
                continue;
            }
            if c.next_serial == u64::MAX || (!c.map.contains_key(&key) && c.map.len() >= CAPACITY) {
                LOST.store(true, Release);
                set_last(status::CAPACITY);
                return;
            }
            c.next_serial += 1;
            let candidate = Candidate {
                input: inputs[at],
                context,
                recording: lease.clone(),
                registry,
                serial: c.next_serial,
                epoch: observed_epoch,
                thread: thread(),
            };
            c.map.insert(key, candidate);
            set_last(status::CANDIDATE);
        }
    }));
    if done.is_err() {
        LOST.store(true, Release);
        set_last(status::FAILURE);
    }
}

// ---- the capture at the source

/// descriptors::SrvBinding: the resource an owned reference.
#[repr(C)]
pub struct SrvBinding {
    pub description: [u64; 20],
    pub resource: *mut c_void,
    pub view: D3D12_SHADER_RESOURCE_VIEW_DESC,
    pub stamp: DescriptorStamp,
}
/// source_srv::SourceSrvBacking: the binding's resource and the recording's token owned references.
#[repr(C)]
pub struct Backing {
    pub binding: SrvBinding,
    pub recording: Lease,
    pub native: NativeBuffer,
    pub state: ReadState,
    pub context: Context,
    pub slot: Slot,
    pub raw_record: [u8; 0xB0],
    pub allocation: Allocation,
    pub allocator: Allocator,
    pub gpu: u64,
    pub backing_bytes: u64,
    pub source_offset: u64,
    pub view_bytes: u64,
    pub record_address: u64,
    pub cpu_descriptor: u64,
    pub serial: u64,
    pub stage: u32,
    pub shader_register: u32,
    pub route: u32,
    pub setter_rva: u32,
    pub low_type: u32,
    pub heap_type: u32,
    pub flags: u32,
}
pub const BACKING_LAYOUT: [usize; 8] = [
    core::mem::size_of::<SrvBinding>(),
    core::mem::size_of::<Backing>(),
    core::mem::offset_of!(Backing, recording),
    core::mem::offset_of!(Backing, slot),
    core::mem::offset_of!(Backing, allocation),
    core::mem::offset_of!(Backing, allocator),
    core::mem::offset_of!(Backing, gpu),
    core::mem::offset_of!(Backing, stage),
];
const _: () = assert!(
    BACKING_LAYOUT[0] == 384
        && BACKING_LAYOUT[1] == 1112
        && BACKING_LAYOUT[2] == 384
        && BACKING_LAYOUT[3] == 512
        && BACKING_LAYOUT[4] == 768
        && BACKING_LAYOUT[5] == 960
        && BACKING_LAYOUT[6] == 1024
        && BACKING_LAYOUT[7] == 1080
);

fn same_srv(a: &SrvBinding, b: &SrvBinding) -> bool {
    let (x, y) = (&a.view, &b.view);
    // SAFETY: buffer views (the union's Buffer member is the one compared, as the .cpp's).
    let (xb, yb) = unsafe { (x.Anonymous.Buffer, y.Anonymous.Buffer) };
    a.resource == b.resource
        && a.description == b.description
        && a.description == a.stamp.value
        && a.stamp.gpu == b.stamp.gpu
        && a.stamp.heap == b.stamp.heap
        && a.stamp.value == b.stamp.value
        && x.Format == y.Format
        && x.ViewDimension == y.ViewDimension
        && x.Shader4ComponentMapping == y.Shader4ComponentMapping
        && xb.FirstElement == yb.FirstElement
        && xb.NumElements == yb.NumElements
        && xb.StructureByteStride == yb.StructureByteStride
        && xb.Flags == yb.Flags
}
/// An SrvBinding this module resolved (its resource reference released on drop).
struct Resolved(SrvBinding);
impl Drop for Resolved {
    fn drop(&mut self) {
        release(self.0.resource);
    }
}
fn resolve_srv(list: u64, root: u64, parameter: u32, offset: u32) -> Option<Resolved> {
    // SAFETY: all-zero is a valid empty SrvBinding.
    let mut out: SrvBinding = unsafe { core::mem::zeroed() };
    // SAFETY: our outputs (descriptor_audit's DescriptorStamp has this layout).
    let ok = unsafe {
        crate::descriptor_audit::cvr_sda_audit_srv(list, root, parameter, offset, (&mut out.stamp as *mut DescriptorStamp).cast(), &mut out.resource, &mut out.view)
    };
    out.description = out.stamp.value;
    let resolved = Resolved(out);
    ok.then_some(resolved)
}
fn matches_descriptor(stamp: &DescriptorStamp) -> bool {
    let mut copy = *stamp;
    // SAFETY: descriptor_audit's DescriptorStamp has this layout.
    unsafe { crate::descriptor_audit::cvr_sda_audit_stamp(1, 0, 0, 0, 0, (&mut copy as *mut DescriptorStamp).cast(), core::ptr::null_mut()) }
}
fn matches_cpu_source(cpu: u64, stamp: &DescriptorStamp) -> bool {
    // SAFETY: as above.
    unsafe { crate::audit_source_tables::cvr_sda_st_matches_cpu(cpu, (stamp as *const DescriptorStamp).cast()) }
}
fn same_device(list: u64, resource: *mut c_void) -> bool {
    let raw = list as *mut c_void;
    // SAFETY: live objects or null.
    let (Some(list), Some(resource)) =
        (unsafe { ID3D12GraphicsCommandList::from_raw_borrowed(&raw) }, unsafe { ID3D12Resource::from_raw_borrowed(&resource) })
    else {
        return false;
    };
    let (mut a, mut b): (Option<ID3D12Device>, Option<ID3D12Device>) = (None, None);
    // SAFETY: live objects; our out-parameters.
    let located = unsafe { list.GetDevice(&mut a).is_ok() && resource.GetDevice(&mut b).is_ok() };
    located && a.as_ref().map(|d| d.as_raw()) == b.as_ref().map(|d| d.as_raw())
}
/// Mode: the candidate's route matches the slot (the individual slot's CPU descriptor, or the cached group's bundle).
fn mode(candidate: &Candidate, slot: &Slot, cpu: u64) -> bool {
    if !matches_mode(&candidate.input, slot) {
        return false;
    }
    if candidate.input.route == INDIVIDUAL {
        return slot.cpu == cpu;
    }
    let (mut entry, mut again) = ([0u8; 8], [0u8; 8]);
    cached_group(READER, core::ptr::null_mut(), slot)
        && read(candidate.input.bundle_entry, &mut entry)
        && read(candidate.input.bundle_entry, &mut again)
        && entry == again
        && entry == candidate.input.bundle_raw
}
fn same_allocation(a: &Allocation, b: &Allocation) -> bool {
    a.observation == b.observation
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.bytes == b.bytes
        && a.resource == b.resource
        && a.heap == b.heap
        && a.block == b.block
        && a.handle == b.handle
        && a.kind == b.kind
        && a.association_known == b.association_known
        && a.resource_matches == b.resource_matches
        && a.extent_covers == b.extent_covers
}
fn same_allocator(a: &Allocator, b: &Allocator) -> bool {
    a.observation == b.observation
        && a.address == b.address
        && a.vtable == b.vtable
        && a.pimpl == b.pimpl
        && a.association_known == b.association_known
        && a.pimpl_matches == b.pimpl_matches
}
fn same_identity(a: &Backing, b: &Backing) -> bool {
    same_srv(&a.binding, &b.binding)
        && same_recording(&a.recording, &b.recording)
        && crate::source_cbv_binding::same_buffer(&a.native, &b.native)
        && a.context == b.context
        && a.slot == b.slot
        && a.raw_record == b.raw_record
        && a.record_address == b.record_address
        && a.cpu_descriptor == b.cpu_descriptor
        && a.serial == b.serial
        && a.gpu == b.gpu
        && a.backing_bytes == b.backing_bytes
        && a.source_offset == b.source_offset
        && a.view_bytes == b.view_bytes
        && a.route == b.route
        && a.setter_rva == b.setter_rva
        && a.stage == b.stage
        && a.shader_register == b.shader_register
        && a.low_type == b.low_type
        && a.heap_type == b.heap_type
        && a.flags == b.flags
        && same_allocation(&a.allocation, &b.allocation)
        && same_allocator(&a.allocator, &b.allocator)
}
/// ObserveMa: the buffer's own allocation (a direct one only) and its allocator, as observations.
fn observe_ma(output: &mut Backing) {
    let raw = &output.raw_record;
    let (direct, payload, control) = (field_u64(raw, 0x18), field_u64(raw, 0x20), field_u64(raw, 0x28));
    if direct != 0 && payload == 0 && control == 0 {
        output.allocation = read_allocation(&mem, base(), direct);
        if output.allocation.observation == crate::source_cbv_details::KNOWN {
            output.allocation.association_known = true;
            output.allocation.resource_matches = output.allocation.resource == output.binding.resource as u64;
            output.allocation.extent_covers = output.allocation.bytes >= output.backing_bytes;
        }
    } else {
        output.allocation.observation =
            if direct == 0 && payload == 0 && control == 0 { crate::source_cbv_details::ABSENT } else { crate::source_cbv_details::UNSUPPORTED };
    }
    output.allocator = read_allocator(&mem, base(), output.native.registry, &output.allocation);
}
/// A backing under construction (its references released unless handed out).
struct Pending(Box<Backing>);
impl Drop for Pending {
    fn drop(&mut self) {
        release(self.0.binding.resource);
        release(self.0.recording.token);
    }
}
impl Pending {
    fn take(mut self) -> Box<Backing> {
        // SAFETY: an all-zero Backing is the empty one; the references move to the result.
        let empty = Box::new(unsafe { core::mem::zeroed::<Backing>() });
        core::mem::replace(&mut self.0, empty)
    }
}

fn capture(list: u64, root: u64, parameter: u32, offset: u32, held: &SrvBinding, recording: &Lease) -> Result<Box<Backing>, u32> {
    use status::*;
    if !enabled() {
        return Err(IGNORED);
    }
    if !compatible() {
        return Err(UNSUPPORTED_EXECUTABLE);
    }
    if LOST.load(Acquire) {
        return Err(COVERAGE_LOST);
    }
    if list == 0
        || root == 0
        || parameter != ROOT_PARAMETER
        || !tracked_register(offset)
        || held.resource.is_null()
        || recording.token.is_null()
        || recording.generation == 0
        || held.view.ViewDimension != D3D12_SRV_DIMENSION_BUFFER
    {
        return Err(INVALID_BINDING);
    }
    let context = current();
    if !valid(&context)
        || context.list != list
        || context.context != recording.context
        // SAFETY: a valid lease and context.
        || !unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list as *mut c_void, &context) }
    {
        return Err(MISSING_CONTEXT);
    }
    let key: Key = (context.list, recording.generation, offset);
    let Some(candidate) = candidates().map.get(&key).cloned() else {
        return Err(MISSING_OBSERVATION);
    };
    if candidate.serial == 0
        || candidate.epoch != EPOCH.load(Acquire)
        || candidate.thread != thread()
        || candidate.context != context
        || !same_recording(&candidate.recording.0, recording)
    {
        return Err(CHANGED);
    }
    let Some(registry) = read_registry().filter(|&r| r == candidate.registry && r == recording.registry) else {
        return Err(CHANGED);
    };
    let native_context = read_context(&mem, context.context, context.list);
    if native_context.observation != crate::source_cbv_details::KNOWN
        || native_context.state != context.state
        || native_context.pending != 0
        || native_context.submitted != 0
    {
        return Err(MISSING_CONTEXT);
    }
    let (mut actual_parameter, mut actual_offset) = (0u32, 0u32);
    // SAFETY: our outputs.
    let found =
        unsafe { crate::descriptor_audit::cvr_sda_audit_find_binding(root, SRV_RANGE, 0, offset, PIXEL_VISIBILITY, &mut actual_parameter, &mut actual_offset) };
    if !found || actual_parameter != parameter || actual_offset != offset || !matches_descriptor(&held.stamp) {
        return Err(DESCRIPTOR_MISMATCH);
    }
    if !resolve_srv(list, root, parameter, offset).is_some_and(|actual| same_srv(held, &actual.0)) {
        return Err(DESCRIPTOR_MISMATCH);
    }
    // SAFETY: an all-zero Backing is the empty one.
    let mut next = Pending(Box::new(unsafe { core::mem::zeroed::<Backing>() }));
    let n = &mut *next.0;
    // SAFETY: a bitwise copy of the held binding with a reference of its own.
    n.binding = unsafe { core::ptr::read(held) };
    add_ref(n.binding.resource);
    n.recording = Lease { ..*recording };
    add_ref(n.recording.token);
    n.context = context;
    let Some(slot) = read_slot(READER, core::ptr::null_mut(), context.state, offset) else {
        return Err(INVALID_BINDING);
    };
    n.slot = slot;
    let (ok, raw, address, native) = read_record(READER, core::ptr::null_mut(), registry, context.context, candidate.input.handle);
    if !ok {
        return Err(INVALID_BINDING);
    }
    (n.raw_record, n.record_address, n.native) = (raw, address, native);
    n.cpu_descriptor = field_u64(&n.raw_record, 0x48);
    if n.native.resource != held.resource as u64
        || n.cpu_descriptor == 0
        || !mode(&candidate, &n.slot, n.cpu_descriptor)
        || !matches_cpu_source(n.cpu_descriptor, &held.stamp)
    {
        return Err(DESCRIPTOR_MISMATCH);
    }
    if !same_device(list, held.resource) {
        return Err(INVALID_BINDING);
    }
    // SAFETY: the held live resource.
    let resource = unsafe { ID3D12Resource::from_raw_borrowed(&held.resource) }.unwrap();
    // SAFETY: a live resource; our outputs.
    let desc = unsafe { resource.GetDesc() };
    let mut heap = D3D12_HEAP_PROPERTIES::default();
    let mut heap_flags = D3D12_HEAP_FLAGS::default();
    // SAFETY: as above.
    if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER || unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut heap_flags)) }.is_err() {
        return Err(INVALID_BINDING);
    }
    // SAFETY: as above.
    n.gpu = unsafe { resource.GetGPUVirtualAddress() };
    n.backing_bytes = desc.Width;
    n.heap_type = heap.Type.0 as u32;
    n.flags = desc.Flags.0 as u32;
    if heap.Type != D3D12_HEAP_TYPE_DEFAULT {
        return Err(INVALID_BINDING);
    }
    let Some((source_offset, view_bytes)) = view_range(&n.native, n.gpu, n.backing_bytes, &held.view, offset) else {
        return Err(INVALID_BINDING);
    };
    (n.source_offset, n.view_bytes) = (source_offset, view_bytes);
    n.low_type = (n.raw_record[6] & 15) as u32;
    n.stage = STAGE_INDEX;
    n.shader_register = offset;
    n.route = candidate.input.route;
    n.setter_rva = if candidate.input.route == INDIVIDUAL { 0x338FC0 } else { 0x338ED4 };
    n.serial = candidate.serial;
    let state = crate::ia_binding::read_native_state(&n.native, context.list, READER, core::ptr::null_mut());
    n.state = ReadState { value: state.value, status: state.status, kind: state.kind };
    observe_ma(n);
    let (ok_again, raw_again, record_again, native_again) = read_record(READER, core::ptr::null_mut(), registry, context.context, candidate.input.handle);
    if current() != context
        || read_registry() != Some(registry)
        // SAFETY: a valid lease and context.
        || !unsafe { crate::native_recording::cvr_native_recording_validate_recording(recording, list as *mut c_void, &context) }
        || !read_slot(READER, core::ptr::null_mut(), context.state, offset).is_some_and(|again| again == n.slot && mode(&candidate, &again, n.cpu_descriptor))
        || !ok_again
        || raw_again != n.raw_record
        || record_again != n.record_address
        || !crate::source_cbv_binding::same_buffer(&native_again, &n.native)
        || !matches_cpu_source(n.cpu_descriptor, &held.stamp)
        || !matches_descriptor(&held.stamp)
    {
        return Err(CHANGED);
    }
    let context_again = read_context(&mem, context.context, context.list);
    let state_again = crate::ia_binding::read_native_state(&n.native, context.list, READER, core::ptr::null_mut());
    if !resolve_srv(list, root, parameter, offset).is_some_and(|again| same_srv(held, &again.0)) {
        return Err(CHANGED);
    }
    if context_again != native_context || state_again.value != n.state.value || state_again.status != n.state.status || state_again.kind != n.state.kind {
        return Err(CHANGED);
    }
    {
        let c = candidates();
        if LOST.load(Acquire) || candidate.epoch != EPOCH.load(Acquire) {
            return Err(COVERAGE_LOST);
        }
        if !c.map.get(&key).is_some_and(|at| at.serial == candidate.serial && at.input == candidate.input) {
            return Err(CHANGED);
        }
    }
    Ok(next.take())
}
fn read_at_source(list: u64, root: u64, parameter: u32, offset: u32, held: &SrvBinding, recording: &Lease, out: &mut Backing) -> u32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| capture(list, root, parameter, offset, held, recording)));
    let status = match result {
        Ok(Ok(backing)) => {
            // SAFETY: `out` is the empty backing; the references move into it.
            unsafe { core::ptr::write(out, *backing) };
            status::CAPTURED
        }
        Ok(Err(status)) => status,
        Err(_) => status::FAILURE,
    };
    set_last(status);
    status
}
/// ReadAtSource: into the empty `out` the held SRV's backing; the status returned.
///
/// # Safety
/// `held` and `recording` are valid; `out` an empty SourceSrvBacking.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssrvo_read_at_source(
    list: u64,
    root: u64,
    parameter: u32,
    offset: u32,
    held: *const SrvBinding,
    recording: *const Lease,
    out: *mut Backing,
) -> u32 {
    // SAFETY: as documented.
    unsafe { read_at_source(list, root, parameter, offset, &*held, &*recording, &mut *out) }
}
/// RevalidateAtSource: `current` (empty) read again for `saved`'s binding and recording, the same identity required
/// (else `current` is emptied again and Changed returned).
///
/// # Safety
/// `saved` is valid; `current` an empty SourceSrvBacking; `clear` empties a SourceSrvBacking (the C++ assignment).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ssrvo_revalidate_at_source(
    list: u64,
    root: u64,
    parameter: u32,
    offset: u32,
    saved: *const Backing,
    current: *mut Backing,
    clear: unsafe extern "C" fn(*mut Backing),
) -> u32 {
    // SAFETY: as documented.
    let (saved, current) = unsafe { (&*saved, &mut *current) };
    let status = read_at_source(list, root, parameter, offset, &saved.binding, &saved.recording, current);
    if status != status::CAPTURED {
        return status;
    }
    if !same_identity(saved, current) {
        // SAFETY: as documented.
        unsafe { clear(current) };
        set_last(status::CHANGED);
        return status::CHANGED;
    }
    set_last(status::CAPTURED);
    status::CAPTURED
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_reset(list: u64) {
    reset_list(list);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_clear() {
    let mut c = candidates();
    let retired = core::mem::take(&mut c.map);
    let old = EPOCH.load(Relaxed);
    if old == u64::MAX || c.next_serial == u64::MAX {
        LOST.store(true, Release);
    } else {
        EPOCH.store(old + 1, Release);
        LOST.store(false, Release);
    }
    drop(c);
    drop(retired);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ssrvo_last_status() -> u32 {
    LAST.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_dormant_paths() {
        assert_eq!(BACKING_LAYOUT[1], 1112);
        let frame = FrameRegisters { rdx: 5, r8: 1, ..Default::default() };
        // SAFETY: a local frame.
        unsafe { CvrSourceSrvBefore(INDIVIDUAL, &frame) };
        assert_eq!(cvr_ssrvo_last_status(), status::IGNORED, "the switch is off");
        // SAFETY: all-zero records are the empty ones.
        let held: SrvBinding = unsafe { core::mem::zeroed() };
        let lease = Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 };
        // SAFETY: as above.
        let mut out: Backing = unsafe { core::mem::zeroed() };
        // SAFETY: valid records.
        assert_eq!(unsafe { cvr_ssrvo_read_at_source(1, 1, ROOT_PARAMETER, 5, &held, &lease, &mut out) }, status::IGNORED);
        assert!(out.binding.resource.is_null());
    }

    #[test]
    fn clear_advances_the_epoch_and_reset_drops_one_list() {
        let input = CandidateInput::default();
        let candidate = |serial| Candidate {
            input,
            context: Context::default(),
            recording: OwnedLease(Lease { token: core::ptr::null_mut(), generation: 0, context: 0, list: 0, registry: 0 }),
            registry: 0,
            serial,
            epoch: 1,
            thread: 0,
        };
        {
            let mut c = candidates();
            c.map.insert((0x10, 1, 4), candidate(1));
            c.map.insert((0x20, 1, 5), candidate(2));
        }
        cvr_ssrvo_reset(0x10);
        assert!(candidates().map.keys().all(|k| k.0 == 0x20));
        let before = EPOCH.load(Relaxed);
        cvr_ssrvo_clear();
        assert!(candidates().map.is_empty() && EPOCH.load(Relaxed) == before + 1 && !LOST.load(Relaxed));
    }

    #[test]
    fn xstate_configures_once() {
        let first = cvr_ssrvo_configure_xstate();
        assert_eq!(first, cvr_ssrvo_configure_xstate());
        // SAFETY: read after the one-time configuration.
        unsafe {
            if first {
                assert_eq!(CvrSourceSrvXstateReady, 1);
                assert!(CvrSourceSrvXstateBytes >= 512);
            }
        }
    }
}
