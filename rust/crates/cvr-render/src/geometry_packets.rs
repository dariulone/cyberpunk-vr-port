//! The native packets' capture (formerly src/Render/NativeGeometryPackets.cpp; include/Render/NativeGeometryPackets.hpp
//! and NativePacketInputs.hpp declare it): a bounded, opt-in snapshot of the native sorted packets before the consumer
//! (sub_14033DDA4) takes them, for one to four frames on request -- each consumer call's row (its arguments, the view and
//! storage behind them, the span's packets copied while the caller owns them, the section's 48-byte definition and the
//! plane bucket found up the stack) and, in resolve mode, each packet's inputs (packet_inputs.rs) with the native
//! instance uploads watched (upload_shadow.rs); the draw origins of the native flushes (draw_origin.rs) and the draw
//! geometry they resolve -- through the scoped thunk's origin, the unwound flush frame, or both compared until 128 agree.
//!
//! Stored addresses are diagnostic identities only; no game pointer is reused. The engine's memory is read with
//! ReadProcessMemory and the stack walked with cvr-engine's unwinder (its SEH step), so a stale pointer reads as
//! unreadable instead of faulting. The .cpp keeps the exports' definitions (the records' and packets' arrays) and the
//! preparation's pairing; it attaches them here with `cvr_gp_attach`.

use crate::draw_origin::{self, DrawOrigin, DrawOriginStack};
use crate::packet_inputs::{self, Blob, Inputs, MappedInstances, Packet, MAX_INPUT_BLOB_BYTES};
use crate::upload_shadow;
use core::cell::RefCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// packets::Record: one consumer call.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub sequence: u64,
    pub begin_qpc: u64,
    pub end_qpc: u64,
    pub renderer: u64,
    pub arguments: u64,
    pub context: u64,
    pub view: u64,
    pub storage: u64,
    pub begin: u64,
    pub end: u64,
    pub frame: u32,
    pub node: u32,
    pub side: u32,
    pub thread: u32,
    pub offset: u32,
    pub count: u32,
    pub status: u32,
    pub metadata_flags: u32,
    pub argument_bytes: [u8; 32],
    pub node_definition: u64,
    pub entry: u64,
    pub plane: u64,
    pub group_bytes: [u8; 48],
}
impl Record {
    pub const ZERO: Record = Record {
        sequence: 0,
        begin_qpc: 0,
        end_qpc: 0,
        renderer: 0,
        arguments: 0,
        context: 0,
        view: 0,
        storage: 0,
        begin: 0,
        end: 0,
        frame: 0,
        node: 0,
        side: 0,
        thread: 0,
        offset: 0,
        count: 0,
        status: 0,
        metadata_flags: 0,
        argument_bytes: [0; 32],
        node_definition: 0,
        entry: 0,
        plane: 0,
        group_bytes: [0; 48],
    };
}
/// packets::Ticket: an index of the capture epoch (u32::MAX none).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub epoch: u64,
    pub index: u32,
}
const _: () = assert!(
    core::mem::size_of::<Record>() == 216
        && core::mem::offset_of!(Record, argument_bytes) == 112
        && core::mem::offset_of!(Record, group_bytes) == 168
        && core::mem::size_of::<Ticket>() == 16
);

pub const RECORD_CAPACITY: u32 = 256;
pub const PACKET_CAPACITY: u32 = 262_144;
pub const MAX_SPAN_PACKETS: u64 = 32_768;
const MAX_INPUT_PACKETS: u32 = 65_536;
/// packets::Status.
pub const COPIED: u32 = 0;
pub const EMPTY: u32 = 1;
pub const BAD_ARGUMENTS: u32 = 2;
pub const BAD_SPAN: u32 = 3;
pub const TOO_LARGE: u32 = 4;
pub const UNREADABLE: u32 = 5;
/// upload_shadow::AddressTag: an instance address in the watched upload buffer's shadow.
pub const ADDRESS_TAG: u64 = 0xFFFF_FFFF_0000_0000;
/// The consumer (sub_14033DDA4, 2.31: sub_14023A938) and the flushes' return addresses (2.5).
const CONSUMER: (u64, u64) = (0x33DDA4, 0x33E356);
const ORDINARY_FLUSH: u64 = 0x335496;
const SKINNED_FLUSH: u64 = 0x335512;
/// The renderer's global (its +0x80 the renderer, +0x4628 the packet pool) and the instance buffer registry.
const RENDERER_GLOBAL: u64 = 0x3BBCAE8;
const BUFFER_REGISTRY: u64 = 0x3BCDCA8;
/// The walks start below the .cpp's (this module's frames and the C++ forwarder): the same frames above are searched.
const OWN_FRAMES: u32 = 4;

/// What the .cpp lends: the exports (its definitions or Rust's) and the preparation's pairing.
#[repr(C)]
pub struct Runtime {
    pub records: *mut Record,
    pub data: *mut Packet,
    pub request: *const AtomicU32,
    pub state: *const AtomicU32,
    pub seq: *const AtomicU32,
    pub count: *const AtomicU32,
    pub data_count: *const AtomicU32,
    pub resolve: *const AtomicU32,
    pub input_count: *const AtomicU32,
    pub input_failure: *const AtomicU32,
    pub inputs: *mut u64,
    pub input_blob: *mut u64,
    pub input_blob_bytes: *const AtomicU32,
    pub origin_mode: *const AtomicU32,
    pub origin_compatible: *const AtomicU32,
    pub origin_verified: *const AtomicU32,
    pub origin_stats: *const AtomicU64,
}
impl Runtime {
    // SAFETY (all): attached once from the .cpp's static, its pointers to live exports.
    fn state(&self) -> &AtomicU32 {
        unsafe { &*self.state }
    }
    fn request(&self) -> &AtomicU32 {
        unsafe { &*self.request }
    }
    fn seq(&self) -> &AtomicU32 {
        unsafe { &*self.seq }
    }
    fn count(&self) -> &AtomicU32 {
        unsafe { &*self.count }
    }
    fn data_count(&self) -> &AtomicU32 {
        unsafe { &*self.data_count }
    }
    fn resolve(&self) -> &AtomicU32 {
        unsafe { &*self.resolve }
    }
    fn input_count(&self) -> &AtomicU32 {
        unsafe { &*self.input_count }
    }
    fn input_failure(&self) -> &AtomicU32 {
        unsafe { &*self.input_failure }
    }
    fn input_blob_bytes(&self) -> &AtomicU32 {
        unsafe { &*self.input_blob_bytes }
    }
    fn origin_mode(&self) -> &AtomicU32 {
        unsafe { &*self.origin_mode }
    }
    fn origin_compatible(&self) -> &AtomicU32 {
        unsafe { &*self.origin_compatible }
    }
    fn origin_verified(&self) -> &AtomicU32 {
        unsafe { &*self.origin_verified }
    }
    fn origin_stat(&self, i: usize) -> &AtomicU64 {
        unsafe { &*self.origin_stats.add(i) }
    }
    fn packets(&self, offset: u32, count: usize) -> &mut [u8] {
        // SAFETY: inside the packets' array (offset + count <= PACKET_CAPACITY), written under the capture's mutex.
        unsafe { core::slice::from_raw_parts_mut(self.data.add(offset as usize).cast::<u8>(), count * 16) }
    }
    fn packet(&self, index: u32) -> Packet {
        unsafe { *self.data.add(index as usize) }
    }
    fn set_record(&self, index: u32, row: Record) {
        unsafe { *self.records.add(index as usize) = row };
    }
    fn set_end(&self, index: u32, qpc: u64) {
        unsafe { (*self.records.add(index as usize)).end_qpc = qpc };
    }
    fn publish(&self, inputs: u64, blob: u64) {
        unsafe {
            core::ptr::write_volatile(self.inputs, inputs);
            core::ptr::write_volatile(self.input_blob, blob);
        }
    }
}
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own records, data and runtime (formerly src/Render/NativeGeometryPackets.cpp's definitions and runtime).
/// The Rust test variants define and attach theirs.
#[cfg(all(feature = "rust-build", not(test)))]
mod plugin {
    #![allow(non_upper_case_globals)]
    use super::{Packet, Record, Runtime, PACKET_CAPACITY, RECORD_CAPACITY};
    use crate::declared_moved_exports::*;
    // SAFETY: plain records; all zeroes their initial state.
    const NO_RECORD: Record = unsafe { core::mem::zeroed() };
    // SAFETY: as above.
    const NO_PACKET: Packet = unsafe { core::mem::zeroed() };
    #[no_mangle]
    pub static mut CyberpunkVR_GeometryPacketRecordBytes: u32 = core::mem::size_of::<Record>() as u32;
    #[no_mangle]
    pub static mut CyberpunkVR_GeometryPacketRecords: [Record; RECORD_CAPACITY as usize] = [NO_RECORD; RECORD_CAPACITY as usize];
    #[no_mangle]
    pub static mut CyberpunkVR_GeometryPacketData: [Packet; PACKET_CAPACITY as usize] = [NO_PACKET; PACKET_CAPACITY as usize];
    #[no_mangle]
    pub static mut CyberpunkVR_GeometryPacketInputBytes: u32 = core::mem::size_of::<crate::packet_inputs::Inputs>() as u32;
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Runtime {}
    pub static RUNTIME: Runtime = Runtime {
        records: (&raw mut CyberpunkVR_GeometryPacketRecords).cast(),
        data: (&raw mut CyberpunkVR_GeometryPacketData).cast(),
        request: &raw const CyberpunkVR_GeometryPacketRequest,
        state: &raw const CyberpunkVR_GeometryPacketState,
        seq: &raw const CyberpunkVR_GeometryPacketSeq,
        count: &raw const CyberpunkVR_GeometryPacketCount,
        data_count: &raw const CyberpunkVR_GeometryPacketDataCount,
        resolve: &raw const CyberpunkVR_GeometryPacketResolve,
        input_count: &raw const CyberpunkVR_GeometryPacketInputCount,
        input_failure: &raw const CyberpunkVR_GeometryPacketInputFailure,
        inputs: &raw mut CyberpunkVR_GeometryPacketInputs,
        input_blob: &raw mut CyberpunkVR_GeometryPacketInputBlob,
        input_blob_bytes: &raw const CyberpunkVR_GeometryPacketInputBlobBytes,
        origin_mode: &raw const CyberpunkVR_GeometryOriginMode,
        origin_compatible: &raw const CyberpunkVR_GeometryOriginCompatible,
        origin_verified: &raw const CyberpunkVR_GeometryOriginVerified,
        origin_stats: (&raw const CyberpunkVR_GeometryOriginStats).cast(),
    };
}
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: the .cpp's static.
    let attached = unsafe { RUNTIME.load(Acquire).as_ref() };
    #[cfg(all(feature = "rust-build", not(test)))]
    let attached = attached.or(Some(&plugin::RUNTIME));
    attached
}

/// The capture's state (the .cpp's mutex and what it guarded).
struct State {
    epoch: u64,
    frame: u32,
    limit: u32,
    inputs: Vec<Inputs>,
    blob: Vec<u8>,
    resolve_inputs: bool,
}
static STATE: Mutex<State> = Mutex::new(State { epoch: 0, frame: 0, limit: 0, inputs: Vec::new(), blob: Vec::new(), resolve_inputs: false });
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}
static LIVE: AtomicBool = AtomicBool::new(false);
thread_local! {
    static ORIGINS: RefCell<DrawOriginStack> = const { RefCell::new(DrawOriginStack { entries: [DrawOrigin { renderer: 0, draw: 0, batch: 0, flush: 0, packet: 0, instances: 0, valid: false }; 32], depth: 0 }) };
}

// ---- the engine's memory

/// Read: `out` from `address` (not 0, not wrapping), every byte or false.
pub fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && address <= u64::MAX - out.len() as u64 && cvr_engine::stack::read(address, out)
}
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_u32(address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
/// A read straight into a field (as the .cpp's: what arrived stays when the read fails).
fn read_into(address: u64, field: &mut u64) -> bool {
    let mut b = field.to_le_bytes();
    let ok = read(address, &mut b);
    *field = u64::from_le_bytes(b);
    ok
}
fn game() -> u64 {
    // SAFETY: the executable's own module.
    unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 }
}
fn now() -> u64 {
    let mut value = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value) };
    value as u64
}
/// ReadInput: an address in the watched upload buffer's shadow (tagged) from `uploaded`'s watch, else the engine's.
pub fn read_input(uploaded: Option<&MappedInstances>, address: u64, out: &mut [u8]) -> bool {
    if address >= ADDRESS_TAG && address - ADDRESS_TAG < upload_shadow::LIMIT as u64 {
        return uploaded.is_some_and(|s| upload_shadow::read(s.gpu, s.resource as usize, address - ADDRESS_TAG, out));
    }
    read(address, out)
}
/// Uploaded: the renderer's instance buffer -- its handle at +0x200 (1..0x100000) into the registry's 0xB0-byte records
/// at +0x5C0AF8: the bytes (+0), GPU address (+8), resource (+16) and CPU mapping (+64).
fn uploaded(renderer: u64) -> MappedInstances {
    let mut result = MappedInstances::default();
    if renderer == 0 || renderer > u64::MAX - 0x204 {
        return result;
    }
    let Some(handle) = read_u32(renderer + 0x200) else { return result };
    if handle == 0 || handle > 0x100000 {
        return result;
    }
    let stride = u64::from(handle - 1) * 0xB0;
    let Some(registry) = read_u64(game().wrapping_add(BUFFER_REGISTRY)) else { return result };
    if registry == 0 || registry > u64::MAX - 0x5C0B40 - stride {
        return result;
    }
    let mut record = [0u8; 72];
    if read(registry + stride + 0x5C0AF8, &mut record) {
        let word = |at: usize| u64::from_le_bytes(record[at..at + 8].try_into().unwrap());
        result.bytes = u64::from(u32::from_le_bytes(record[0..4].try_into().unwrap()));
        result.gpu = word(8);
        result.resource = word(16);
        result.cpu = word(64);
    }
    result
}
/// The renderer global's renderer and packet pool: Some((renderer, pool)) when readable.
fn renderer_global(bound: u64) -> Option<(u64, u64)> {
    let global = read_u64(game().wrapping_add(RENDERER_GLOBAL))?;
    if global == 0 || global > u64::MAX - bound {
        return None;
    }
    let renderer = read_u64(global + 0x80)?;
    Some((renderer, global))
}

// ---- the inputs

fn clear_inputs(rt: &Runtime, s: &mut State) {
    s.inputs = Vec::new();
    s.blob = Vec::new();
    rt.publish(0, 0);
    rt.input_blob_bytes().store(0, SeqCst);
    rt.input_count().store(0, Release);
    s.resolve_inputs = false;
    upload_shadow::watch(0, 0, 0);
}
/// ResizeBlob: growing reserves half again the capacity (4 KiB at first), at most the blob limit.
fn resize_blob(blob: &mut Vec<u8>, size: usize, growing: bool) -> *mut u8 {
    if growing && size > blob.capacity() {
        let capacity = blob.capacity();
        let target = MAX_INPUT_BLOB_BYTES.min(size.max(if capacity != 0 { capacity + capacity / 2 } else { 4096 }));
        blob.reserve_exact(target - blob.len());
    }
    blob.resize(size, 0);
    blob.as_mut_ptr()
}
/// CopyInputs: in resolve mode each copied packet's inputs, at its place, while the renderer global still hands out
/// the row's renderer; the inputs and blob published even after a failure (readers reject the failure).
fn copy_inputs(rt: &Runtime, s: &mut State, row: &Record) {
    if !s.resolve_inputs || row.count == 0 || rt.input_failure().load(Relaxed) != 0 {
        return;
    }
    if row.offset + row.count > MAX_INPUT_PACKETS {
        rt.input_failure().store(1, SeqCst);
        return;
    }
    let size = (row.offset + row.count) as usize;
    if s.inputs.try_reserve(size.saturating_sub(s.inputs.len())).is_err() {
        rt.input_failure().store(2, SeqCst);
    } else {
        s.inputs.resize(size, Inputs::default());
        // The consumer loads this exact pool from [global+0x4628]: [global+0x80] must be the row's renderer.
        match renderer_global(0x4630).filter(|&(renderer, _)| renderer == row.renderer).and_then(|(_, global)| read_u64(global + 0x4628)) {
            Some(pool) => {
                let mut up = uploaded(row.renderer);
                if up.cpu == 0 {
                    up.cpu = ADDRESS_TAG;
                }
                let State { inputs, blob, .. } = &mut *s;
                for i in 0..row.count {
                    let mut reader = |address: u64, out: &mut [u8]| read_input(Some(&up), address, out);
                    let (len, data) = (blob.len(), blob.as_mut_ptr());
                    let mut resize = |size: usize, growing: bool| resize_blob(blob, size, growing);
                    let mut lent = Blob { len, data, resize: &mut resize };
                    inputs[(row.offset + i) as usize] = packet_inputs::resolve_details(rt.packet(row.offset + i), pool, up, Some(&mut reader as packet_inputs::Read), &mut lent, false);
                }
            }
            None => rt.input_failure().store(3, SeqCst),
        }
    }
    rt.publish(s.inputs.as_ptr() as u64, s.blob.as_ptr() as u64);
    rt.input_blob_bytes().store(s.blob.len() as u32, Release);
    rt.input_count().store(s.inputs.len() as u32, Release);
}

// ---- the stack

/// NativeGroup: up the stack to the consumer, whose RSI is the node definition and RBX its section's 48-byte definition
/// ([RSI+0x30] + 48 x index, index < [RSI+0x3C] and 1024); the plane bucket [RSP+0x38] + 0x1C0 x (10 x entry[+0x10] +
/// entry[+0x14]) (sub_14228E460; 2.31 kept it at [RBP-0x78]).
fn native_group(row: &mut Record) {
    let game = game();
    let _ = cvr_engine::stack::walk(24 + OWN_FRAMES, |c, _low, high| {
        if c.Rip < game.wrapping_add(CONSUMER.0) || c.Rip >= game.wrapping_add(CONSUMER.1) {
            return None;
        }
        let at = |offset: u64| c.Rsi.wrapping_add(offset);
        let found = (c.Rsp <= high.wrapping_sub(0x40))
            .then(|| Some((read_u64(at(0x30))?, read_u32(at(0x3C))?)))
            .flatten()
            .filter(|&(sections, count)| c.Rbx >= sections && (c.Rbx - sections) % 48 == 0 && (c.Rbx - sections) / 48 < u64::from(count.min(1024)));
        if found.is_some() {
            row.node_definition = c.Rsi;
            row.entry = c.Rbx;
            if let Some(base) = read_u64(c.Rsp.wrapping_add(0x38)) {
                if read(row.entry, &mut row.group_bytes) {
                    let pass = u32::from_le_bytes(row.group_bytes[0x10..0x14].try_into().unwrap());
                    row.plane = base.wrapping_add(0x1C0u64.wrapping_mul(u64::from(pass) * 10 + u64::from(row.group_bytes[0x14])));
                    row.metadata_flags |= 4;
                }
            }
        }
        Some(())
    });
}
/// OriginRead over the engine's memory.
unsafe extern "C-unwind" fn origin_read(_: *mut c_void, address: u64, out: *mut c_void, bytes: usize) -> bool {
    if bytes == 0 {
        return read(address, &mut []);
    }
    // SAFETY: draw_origin's output buffer of `bytes`.
    read(address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
/// DrawGeometryCompatible: the ordinary flush (its consumer detour excluded) and, for 64-byte instances, the skinned
/// flush and gather as the current executable has them (code_tables.rs).
fn draw_geometry_compatible(stride: u32) -> bool {
    static COMPATIBLE: OnceLock<[bool; 2]> = OnceLock::new();
    let c = COMPATIBLE.get_or_init(|| {
        let game = game();
        [crate::code_tables::cvr_code_table(0, game), crate::code_tables::cvr_code_table(1, game)]
    });
    c[0] && (stride == 48 || (stride == 64 && c[1]))
}
/// LegacyDrawGeometry: the flush frame up the stack (its return address in the ordinary or skinned flush) and the draw
/// origin its registers hold, resolved.
fn legacy_draw_geometry(renderer: u64, instances: u32, stride: u32, output: &mut [u8; 64]) -> bool {
    if renderer == 0 || instances == 0 || !draw_geometry_compatible(stride) {
        return false;
    }
    let target = game().wrapping_add(if stride == 64 { SKINNED_FLUSH } else { ORDINARY_FLUSH });
    cvr_engine::stack::walk(48 + OWN_FRAMES, |c, _, _| {
        (c.Rip == target).then(|| {
            let origin = DrawOrigin {
                renderer: c.Rsi,
                draw: c.R14,
                batch: c.Rbx,
                flush: c.Rdi,
                packet: c.R15,
                instances: if stride == 64 { u64::from(instances) } else { c.R12 },
                valid: true,
            };
            draw_origin::resolve_origin(&origin, renderer, instances, stride, Some(origin_read as draw_origin::OriginRead), core::ptr::null_mut(), output)
        })
    })
    .unwrap_or(false)
}
/// CurrentDrawGeometry. Skinned batches (stride 64) take the unwound frame: the scoped thunk covers only the ordinary
/// flush. Mode 2 uses the thunk's origin once mode 1 verified 128 agreements without a mismatch; mode 1 compares the two.
fn current_draw_geometry(rt: &Runtime, renderer: u64, instances: u32, stride: u32, output: &mut [u8; 64]) -> bool {
    if renderer == 0 || instances == 0 || !draw_geometry_compatible(stride) {
        return false;
    }
    if stride == 64 {
        rt.origin_stat(3).fetch_add(1, SeqCst);
        return legacy_draw_geometry(renderer, instances, stride, output);
    }
    let mode = rt.origin_mode().load(Relaxed);
    let origin = ORIGINS.with(|s| s.borrow().current().copied());
    let mut scoped = [0u8; 64];
    let valid = mode != 0
        && rt.origin_compatible().load(SeqCst) == 1
        && origin.is_some_and(|o| draw_origin::resolve_origin(&o, renderer, instances, stride, Some(origin_read as draw_origin::OriginRead), core::ptr::null_mut(), &mut scoped));
    if mode == 2 && rt.origin_verified().load(SeqCst) == 1 && rt.origin_stat(1).load(SeqCst) == 0 && valid {
        rt.origin_stat(2).fetch_add(1, SeqCst);
        *output = scoped;
        return true;
    }
    let legacy = legacy_draw_geometry(renderer, instances, stride, output);
    if mode == 1 && rt.origin_compatible().load(SeqCst) == 1 {
        if valid != legacy || (valid && scoped != *output) {
            rt.origin_stat(1).fetch_add(1, SeqCst);
            rt.origin_verified().store(0, SeqCst);
            return false;
        }
        if valid && rt.origin_stat(0).fetch_add(1, SeqCst) + 1 >= 128 && rt.origin_stat(1).load(SeqCst) == 0 {
            rt.origin_verified().store(1, SeqCst);
        }
    } else {
        rt.origin_stat(3).fetch_add(1, SeqCst);
    }
    legacy
}

// ---- the capture

/// FrameBoundary: with live inputs only the instance upload watched; else a request starts a capture of up to four
/// frames (and in resolve mode 1 the inputs, any resolve mode watching the uploads), a running one counts its frames,
/// and inputs left from a resolve mode now off are released.
fn frame_boundary(rt: &Runtime, live_inputs: bool) {
    if live_inputs {
        LIVE.store(true, Relaxed);
        let source = renderer_global(0x88).map(|(renderer, _)| uploaded(renderer)).unwrap_or_default();
        upload_shadow::watch(if source.cpu != 0 { 0 } else { source.gpu }, source.resource as usize, source.bytes);
        return;
    }
    if LIVE.swap(false, Relaxed) {
        upload_shadow::stop();
    }
    if rt.state().load(Relaxed) != 1 && rt.request().load(Relaxed) == 0 {
        if rt.resolve().load(Relaxed) == 0 && (rt.input_count().load(Acquire) != 0 || upload_shadow::active()) {
            let mut s = lock();
            rt.seq().fetch_add(1, AcqRel);
            clear_inputs(rt, &mut s);
            rt.seq().fetch_add(1, Release);
        }
        return;
    }
    let mut s = lock();
    if rt.state().load(Relaxed) == 1 {
        s.frame += 1;
        if s.frame > s.limit {
            upload_shadow::stop();
            rt.state().store(2, Release);
        }
        return;
    }
    let request = rt.request().swap(0, AcqRel);
    if request == 0 {
        return;
    }
    rt.seq().fetch_add(1, AcqRel);
    s.epoch += 1;
    s.frame = 1;
    s.limit = request.min(4);
    rt.count().store(0, Relaxed);
    rt.data_count().store(0, Relaxed);
    let resolve_mode = rt.resolve().load(Relaxed);
    clear_inputs(rt, &mut s);
    s.resolve_inputs = resolve_mode == 1;
    rt.input_failure().store(0, SeqCst);
    // Mode 2 only watches the native instance uploads for a bounded GPU probe: it does not resolve or copy every
    // material program and object in the scene.
    if resolve_mode != 0 {
        if let Some((renderer, _)) = renderer_global(0x88) {
            let source = uploaded(renderer);
            if source.cpu == 0 {
                upload_shadow::watch(source.gpu, source.resource as usize, source.bytes);
            }
        }
    }
    rt.seq().fetch_add(1, Release);
    rt.state().store(1, Release);
}
/// Begin: the preparation's pairing first; then, while capturing, the consumer call's row -- its 32 argument bytes, the
/// context's view (+0x18) and its storage (+0x2C40), the span (begin, end) checked and its packets copied now, before
/// the shared view scratch is reused (the caller owns the immutable span throughout the native call) -- and the
/// section up the stack.
fn begin(rt: &Runtime, node: u32, side: i32, renderer: *const c_void, arguments: *const c_void, span: *const c_void) -> Ticket {
    if crate::preparation_trace::pairing() {
        crate::preparation_trace::cvr_prep_consume(node, side, arguments as u64, span as u64);
    }
    let none = Ticket { epoch: 0, index: u32::MAX };
    if node != CONSUMER.0 as u32 || !(0..=1).contains(&side) || rt.state().load(Relaxed) != 1 {
        return none;
    }
    let mut s = lock();
    if rt.state().load(Relaxed) != 1 {
        return none;
    }
    let index = rt.count().load(Relaxed);
    let offset = rt.data_count().load(Relaxed);
    if index >= RECORD_CAPACITY {
        rt.state().store(3, Release);
        return none;
    }
    let mut row = Record::ZERO;
    row.sequence = u64::from(index) + 1;
    row.begin_qpc = now();
    row.renderer = renderer as u64;
    row.arguments = arguments as u64;
    row.frame = s.frame;
    row.node = node;
    row.side = side as u32;
    // SAFETY: a plain call.
    row.thread = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    row.offset = offset;
    let mut status = BAD_ARGUMENTS;
    let mut full = false;
    if read(row.arguments, &mut row.argument_bytes) {
        row.context = u64::from_le_bytes(row.argument_bytes[0..8].try_into().unwrap());
        if row.context != 0 && row.context <= u64::MAX - 0x18 && read_into(row.context + 0x18, &mut row.view) {
            row.metadata_flags |= 1;
            if row.view != 0 && row.view <= u64::MAX - 0x2C40 && read_into(row.view + 0x2C40, &mut row.storage) {
                row.metadata_flags |= 2;
            }
        }
        status = BAD_SPAN;
        let mut range = [0u8; 16];
        if read(span as u64, &mut range) {
            row.begin = u64::from_le_bytes(range[0..8].try_into().unwrap());
            row.end = u64::from_le_bytes(range[8..16].try_into().unwrap());
            if row.end >= row.begin && (row.end - row.begin) % 16 == 0 && row.begin % 8 == 0 {
                let count = (row.end - row.begin) / 16;
                status = if count == 0 {
                    EMPTY
                } else if row.begin == 0 {
                    BAD_SPAN
                } else if count > MAX_SPAN_PACKETS {
                    TOO_LARGE
                } else if count > u64::from(PACKET_CAPACITY - offset) {
                    full = true;
                    TOO_LARGE
                } else if read(row.begin, rt.packets(offset, count as usize)) {
                    row.count = count as u32;
                    COPIED
                } else {
                    UNREADABLE
                };
            }
        }
    }
    row.status = status;
    native_group(&mut row);
    rt.seq().fetch_add(1, AcqRel);
    copy_inputs(rt, &mut s, &row);
    rt.set_record(index, row);
    rt.data_count().store(offset + row.count, Relaxed);
    rt.count().store(index + 1, Relaxed);
    rt.seq().fetch_add(1, Release);
    if full {
        rt.state().store(3, Release);
    }
    Ticket { epoch: s.epoch, index }
}
/// End: the row's completion time, only within its own capture.
fn end(rt: &Runtime, ticket: Ticket) {
    if ticket.index == u32::MAX {
        return;
    }
    let s = lock();
    if ticket.epoch != s.epoch || ticket.index >= rt.count().load(Relaxed) {
        return;
    }
    rt.seq().fetch_add(1, AcqRel);
    rt.set_end(ticket.index, now());
    rt.seq().fetch_add(1, Release);
}

// ---- the C ABI

/// # Safety
/// `runtime` is the .cpp's static, its pointers live for the process.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
#[no_mangle]
pub extern "C-unwind" fn cvr_gp_frame_boundary(live_inputs: bool) {
    if let Some(rt) = runtime() {
        frame_boundary(rt, live_inputs);
    }
}
/// # Safety
/// `ticket` is writable; the pointers are the consumer call's (read as the engine's memory).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_begin(node: u32, side: i32, renderer: *const c_void, arguments: *const c_void, span: *const c_void, ticket: *mut Ticket) {
    let t = runtime().map_or(Ticket { epoch: 0, index: u32::MAX }, |rt| begin(rt, node, side, renderer, arguments, span));
    unsafe { *ticket = t };
}
#[no_mangle]
pub extern "C-unwind" fn cvr_gp_end(epoch: u64, index: u32) {
    if let Some(rt) = runtime() {
        end(rt, Ticket { epoch, index });
    }
}
/// ResolveLive's source: the renderer global's packet pool while it hands out `renderer`, and its instance upload (the
/// shadow's tag for its CPU mapping when it has none); false for more than MAX_SPAN_PACKETS packets.
///
/// # Safety
/// `pool` and `uploaded` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_live(renderer: u64, packets: usize, pool: *mut u64, uploaded_out: *mut MappedInstances) -> bool {
    if packets as u64 > MAX_SPAN_PACKETS {
        return false;
    }
    let Some(found) = renderer_global(0x4630).filter(|&(actual, _)| actual == renderer).and_then(|(_, global)| read_u64(global + 0x4628)) else {
        return false;
    };
    let mut up = uploaded(renderer);
    if up.cpu == 0 {
        up.cpu = ADDRESS_TAG;
    }
    unsafe {
        *pool = found;
        *uploaded_out = up;
    }
    true
}
/// ReadInput for the .cpp's ResolveDetails (`context` a MappedInstances or null).
///
/// # Safety
/// `out` holds `bytes`; `context` is null or a MappedInstances.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_read_input(context: *mut c_void, address: u64, out: *mut c_void, bytes: usize) -> bool {
    let uploaded = unsafe { context.cast::<MappedInstances>().as_ref() };
    if bytes == 0 {
        return read_input(uploaded, address, &mut []);
    }
    read_input(uploaded, address, unsafe { core::slice::from_raw_parts_mut(out.cast::<u8>(), bytes) })
}
/// # Safety
/// `output` holds 64 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_current_geometry(renderer: u64, instances: u32, stride: u32, output: *mut [u8; 64]) -> bool {
    runtime().is_some_and(|rt| current_draw_geometry(rt, renderer, instances, stride, unsafe { &mut *output }))
}
/// EnterDrawOrigin (`origin`) or LeaveDrawOrigin (null), on this thread's stack.
///
/// # Safety
/// `origin` is null or valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_gp_draw_origin(origin: *const DrawOrigin) {
    let origin = unsafe { origin.as_ref() }.copied();
    ORIGINS.with(|s| match origin {
        Some(o) => s.borrow_mut().enter(o),
        None => s.borrow_mut().leave(),
    });
}

#[cfg(test)]
mod tests {
    //! The capture against a runtime of its own: a disabled recorder, a row and its packets owned, a late completion,
    //! the spans refused, the four-frame clamp, the resolve mode without a native pool.
    use super::*;


    #[test]
    fn capture_rows_and_bounds() {
        let atom = || Box::leak(Box::new(AtomicU32::new(0))) as &'static AtomicU32;
        let records: &'static mut [Record] = Box::leak(vec![Record::ZERO; RECORD_CAPACITY as usize].into_boxed_slice());
        let data: &'static mut [Packet] = Box::leak(vec![Packet::default(); PACKET_CAPACITY as usize].into_boxed_slice());
        let (records, data) = (records.as_mut_ptr(), data.as_mut_ptr());
        let stats: &'static [AtomicU64; 4] = Box::leak(Box::new([const { AtomicU64::new(0) }; 4]));
        let rt: &'static Runtime = Box::leak(Box::new(Runtime {
            records,
            data,
            request: atom(),
            state: atom(),
            seq: atom(),
            count: atom(),
            data_count: atom(),
            resolve: atom(),
            input_count: atom(),
            input_failure: atom(),
            inputs: Box::leak(Box::new(0u64)),
            input_blob: Box::leak(Box::new(0u64)),
            input_blob_bytes: atom(),
            origin_mode: atom(),
            origin_compatible: atom(),
            origin_verified: atom(),
            origin_stats: stats.as_ptr(),
        }));
        let record = |i: usize| unsafe { *rt.records.add(i) };
        let start = |frames: u32| {
            rt.state().store(0, SeqCst);
            rt.request().store(frames, SeqCst);
            frame_boundary(rt, false);
        };
        // The arguments: the context at +0, its view at +0x18, the view's storage at +0x2C40.
        let mut view = vec![0u8; 0x2C48];
        view[0x2C40..0x2C48].copy_from_slice(&0xabc123u64.to_le_bytes());
        let mut context = [0u8; 32];
        context[0x18..0x20].copy_from_slice(&(view.as_ptr() as u64).to_le_bytes());
        let mut arguments = [0u8; 32];
        arguments[0..8].copy_from_slice(&(context.as_ptr() as u64).to_le_bytes());
        let packets = [Packet { first: 1, second: 2 }, Packet { first: 1, second: 2 }, Packet { first: 3, second: 4 }];
        let capture = |begin_at: u64, end_at: u64, side: i32| {
            let span = [begin_at, end_at];
            super::begin(rt, 0x33DDA4, side, 0x123456 as *const c_void, arguments.as_ptr().cast(), span.as_ptr().cast())
        };
        let (b, e) = (packets.as_ptr() as u64, packets.as_ptr() as u64 + 48);
        assert_eq!(capture(b, e, 1).index, u32::MAX, "disabled");
        start(1);
        assert_eq!(capture(b, e, 2).index, u32::MAX, "an unrelated view");
        let ticket = capture(b, e, 1);
        end(rt, ticket);
        let row = record(0);
        assert!(row.status == COPIED && row.count == 3 && row.storage == 0xabc123 && row.metadata_flags & 3 == 3 && row.end_qpc >= row.begin_qpc);
        assert_eq!(unsafe { (*rt.data.add(2)).first }, 3, "the packets owned");
        frame_boundary(rt, false);
        assert_eq!(rt.state().load(SeqCst), 2, "one frame");
        start(1);
        let fresh = capture(b, e, 1);
        end(rt, ticket);
        assert_eq!(record(fresh.index as usize).end_qpc, 0, "a late completion leaves the new capture");
        start(1);
        capture(0, 0, 1);
        capture(e, b, 1);
        capture(0x10000, 0x10001, 1);
        capture(0x10000, 0x10000 + 16 * (MAX_SPAN_PACKETS + 1), 1);
        let statuses: Vec<u32> = (0..4).map(|i| record(i).status).collect();
        assert_eq!(statuses, [EMPTY, BAD_SPAN, BAD_SPAN, TOO_LARGE]);
        assert_eq!(rt.data_count().load(SeqCst), 0, "no packets published");
        start(100);
        for _ in 0..3 {
            frame_boundary(rt, false);
            assert_eq!(rt.state().load(SeqCst), 1, "the request clamped to four frames");
        }
        frame_boundary(rt, false);
        assert_eq!(rt.state().load(SeqCst), 2);
        assert_eq!(rt.seq().load(SeqCst) & 1, 0, "the publication version even");
        rt.resolve().store(1, SeqCst);
        start(1);
        end(rt, capture(b, e, 1));
        assert!(rt.input_failure().load(SeqCst) == 3 && rt.input_count().load(SeqCst) == 3 && unsafe { *rt.inputs } != 0, "no native pool");
        frame_boundary(rt, false);
        rt.resolve().store(0, SeqCst);
        frame_boundary(rt, false);
        assert!(rt.input_count().load(SeqCst) == 0 && unsafe { *rt.inputs } == 0, "the inputs released");
    }
}
