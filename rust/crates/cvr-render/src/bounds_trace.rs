//! The bounds and particle trace (formerly src/Stereo/NativeBoundsTrace.cpp; include/Render/NativeBoundsTrace.hpp
//! declares it): with CVR_STEREO_BOUNDS_TRACE set and the code as written against, while the CPU collector probe
//! traces a scene's pool, every candidate bounds write to that pool (its key, slot and box from the three known
//! callers, the caller's return and the stack's RVAs) and every particle emitter's instances the pool's scene lists
//! (their view, state, stamp, accumulated time and deltas), each a row of the exports'.
//!
//! The .cpp keeps the exports' arrays, the detours' registration over the thunks and the stereo state it reads (the
//! view side, the node, the VRCAM key, the main view), lent through the attached runtime.

use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Mutex, MutexGuard};

pub const CAPACITY: u32 = 4096;
/// bounds_trace::Kind.
pub mod kind {
    pub const BEGIN: u32 = 0;
    pub const WRITE_BEGIN: u32 = 1;
    pub const END: u32 = 7;
}

/// bounds_trace::Record.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Record {
    pub sequence: u64,
    pub qpc: u64,
    pub id: u64,
    pub pool: u64,
    pub span: u64,
    pub kind: u32,
    pub thread: u32,
    pub caller: u32,
    pub node: u32,
    pub side: i32,
    pub slot: u32,
    pub key: u32,
    pub flags: u32,
    pub bounds: [f32; 6],
    pub stack: [u32; 12],
    pub result: u32,
    pub stack_count: u32,
}
impl Default for Record {
    fn default() -> Self {
        Record { sequence: 0, qpc: 0, id: 0, pool: 0, span: 0, kind: 0, thread: 0, caller: 0, node: 0, side: -1, slot: 0, key: 0, flags: 0, bounds: [0.0; 6], stack: [0; 12], result: 0, stack_count: 0 }
    }
}
/// bounds_trace::ParticleRecord.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleRecord {
    pub sequence: u64,
    pub qpc: u64,
    pub id: u64,
    pub instance: u64,
    pub state: u64,
    pub view: u64,
    pub kind: u32,
    pub thread: u32,
    pub caller: u32,
    pub node: u32,
    pub epoch: u32,
    pub stamp: u32,
    pub scene_counter: u32,
    pub instance_index: u32,
    pub side: i32,
    pub onscreen: u32,
    pub valid: u32,
    pub total_instances: u32,
    pub accumulated: f32,
    pub anchor: u32,
    pub system_delta: f32,
    pub parameter_delta: f32,
}
impl Default for ParticleRecord {
    fn default() -> Self {
        ParticleRecord {
            sequence: 0,
            qpc: 0,
            id: 0,
            instance: 0,
            state: 0,
            view: 0,
            kind: 0,
            thread: 0,
            caller: 0,
            node: 0,
            epoch: 0,
            stamp: u32::MAX,
            scene_counter: 0,
            instance_index: 0,
            side: -1,
            onscreen: u32::MAX,
            valid: 0,
            total_instances: 0,
            accumulated: 0.0,
            anchor: 0,
            system_delta: 0.0,
            parameter_delta: 0.0,
        }
    }
}
const _: () = assert!(core::mem::size_of::<Record>() == 152 && core::mem::size_of::<ParticleRecord>() == 112);

/// What the .cpp lends.
#[repr(C)]
pub struct Runtime {
    pub records: *mut Record,
    pub particles: *mut ParticleRecord,
    pub compatible: *const AtomicU32,
    pub count: *const AtomicU32,
    pub lost: *const AtomicU32,
    pub particle_compatible: *const AtomicU32,
    pub particle_count: *const AtomicU32,
    pub particle_lost: *const AtomicU32,
    /// g_exe_base, t_view_side, t_current_node_work, g_vrcam_ctx_key, is_main_view.
    pub exe_base: extern "C" fn() -> u64,
    pub vrcam_key: extern "C" fn() -> u64,
    pub is_main_view: extern "C" fn(u64) -> bool,
}
impl Runtime {
    // SAFETY (all): the .cpp's static, its pointers to live exports.
    fn compatible(&self) -> &AtomicU32 {
        unsafe { &*self.compatible }
    }
    fn count(&self) -> &AtomicU32 {
        unsafe { &*self.count }
    }
    fn lost(&self) -> &AtomicU32 {
        unsafe { &*self.lost }
    }
    fn particle_compatible(&self) -> &AtomicU32 {
        unsafe { &*self.particle_compatible }
    }
    fn particle_count(&self) -> &AtomicU32 {
        unsafe { &*self.particle_count }
    }
    fn particle_lost(&self) -> &AtomicU32 {
        unsafe { &*self.particle_lost }
    }
    fn rva(&self, p: u64) -> u32 {
        let base = (self.exe_base)();
        if p >= base && p - base <= u64::from(u32::MAX) {
            (p - base) as u32
        } else {
            0
        }
    }
}
/// The plugin's table and the exports and originals it names, defined here (formerly the .cpp's); a test
/// program attaches its own.
#[cfg(feature = "rust-build")]
mod plugin {
    use super::{ParticleRecord, Record, Runtime, CAPACITY};
    use core::sync::atomic::Ordering::{Acquire, SeqCst};
    extern "C" fn exe_base() -> u64 {
        // SAFETY: the game module base, set once at the stereo install.
        unsafe { core::ptr::read_volatile(&raw const crate::stereo_shared::g_exe_base) as u64 }
    }
    extern "C" fn vrcam_key() -> u64 {
        crate::moved_globals::g_vrcam_ctx_key.load(SeqCst)
    }
    use crate::declared_moved_exports as x;
    use core::ffi::c_void;
    /// The trace's records (formerly src/Stereo/NativeBoundsTrace.cpp's).
    #[no_mangle]
    pub static mut CyberpunkVR_BoundsTraceRecords: [Record; CAPACITY as usize] = unsafe { core::mem::zeroed() };
    #[no_mangle]
    pub static mut CyberpunkVR_ParticleTraceRecords: [ParticleRecord; CAPACITY as usize] = unsafe { core::mem::zeroed() };
    /// The thunks' originals (asm_thunks/native_bounds_thunk.rs jumps through them; engine_detours.rs installs them).
    #[no_mangle]
    pub static mut CvrBoundsOriginal: *mut c_void = core::ptr::null_mut();
    #[no_mangle]
    pub static mut CvrParticleOriginal: *mut c_void = core::ptr::null_mut();
    /// cvr::detail::is_main_view.
    extern "C" fn is_main_view(view: u64) -> bool {
        let cached = crate::moved_globals::g_main_view_ctx.load(Acquire);
        // SAFETY: an engine view the trace found.
        unsafe { crate::main_view_check::cvr_ss_is_main_view(view as *const u8, cached) }
    }
    // SAFETY: process-lifetime exports and plain functions.
    unsafe impl Sync for Runtime {}
    pub(super) static RT: Runtime = Runtime {
        records: (&raw mut CyberpunkVR_BoundsTraceRecords).cast(),
        particles: (&raw mut CyberpunkVR_ParticleTraceRecords).cast(),
        compatible: &raw const x::CyberpunkVR_BoundsTraceCompatible,
        count: &raw const x::CyberpunkVR_BoundsTraceCount,
        lost: &raw const x::CyberpunkVR_BoundsTraceLost,
        particle_compatible: &raw const x::CyberpunkVR_ParticleTraceCompatible,
        particle_count: &raw const x::CyberpunkVR_ParticleTraceCount,
        particle_lost: &raw const x::CyberpunkVR_ParticleTraceLost,
        exe_base,
        vrcam_key,
        is_main_view,
    };
}
#[cfg(feature = "rust-build")]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new((&raw const plugin::RT).cast_mut());
#[cfg(not(feature = "rust-build"))]
static RUNTIME: AtomicPtr<Runtime> = AtomicPtr::new(core::ptr::null_mut());
fn runtime() -> Option<&'static Runtime> {
    // SAFETY: the .cpp's static.
    unsafe { RUNTIME.load(Acquire).as_ref() }
}

struct State {
    next_span: u64,
    closing: bool,
    emitters: Vec<u64>,
}
static STATE: Mutex<State> = Mutex::new(State { next_span: 0, closing: false, emitters: Vec::new() });
static ACTIVE_POOL: AtomicU64 = AtomicU64::new(0);
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn read(address: u64, out: &mut [u8]) -> bool {
    address != 0 && cvr_engine::stack::read(address, out)
}
fn read_u64(address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn read_u32(address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
fn now() -> u64 {
    let mut value = 0i64;
    // SAFETY: one output.
    unsafe { windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut value) };
    value as u64
}
fn thread() -> u32 {
    // SAFETY: a plain call.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}
fn append(rt: &Runtime, mut row: Record) {
    let count = rt.count().load(SeqCst);
    if count == CAPACITY {
        rt.lost().fetch_add(1, SeqCst);
        return;
    }
    row.sequence = u64::from(count) + 1;
    row.qpc = now();
    row.thread = thread();
    // SAFETY: inside the records' array.
    unsafe { *rt.records.add(count as usize) = row };
    rt.count().store(count + 1, Release);
}
fn traced() -> bool {
    std::env::var_os("CVR_STEREO_BOUNDS_TRACE").is_some()
}
/// Compatible: asked for, and the bounds writer as written against (stereo_code.rs table 1).
pub fn compatible(rt: &Runtime) -> bool {
    if !traced() {
        return false;
    }
    // SAFETY: the executable's own module.
    let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
    let valid = crate::stereo_code::cvr_stereo_code(1, base);
    rt.compatible().store(if valid { 1 } else { 2 }, SeqCst);
    valid
}
/// ParticleCompatible: asked for, and the particle preparation as written against (table 2).
pub fn particle_compatible(rt: &Runtime) -> bool {
    if !traced() {
        return false;
    }
    // SAFETY: the executable's own module.
    let base = unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 };
    let valid = crate::stereo_code::cvr_stereo_code(2, base);
    rt.particle_compatible().store(if valid { 1 } else { 2 }, SeqCst);
    valid
}
/// ParticleSnapshot: a traced emitter's instances (at most 128): its scene's counter, the particle epoch, each instance's
/// state, stamp, accumulated time and the deltas; the view's side from its name (the VRCAM's or the main view).
fn particle_snapshot(rt: &Runtime, state: &State, emitter: u64, context: u64, onscreen: u32, kind: u32, caller: u64, system: u64) {
    if state.emitters.binary_search(&emitter).is_err() {
        return;
    }
    let base = (rt.exe_base)();
    let header = (|| {
        let instances = read_u64(emitter.wrapping_add(0xB8))?;
        let count = read_u32(emitter.wrapping_add(0xC4))?;
        if count > 128 || (count != 0 && instances == 0) {
            return None;
        }
        let scene = read_u64(emitter.wrapping_add(0x68))?;
        let scene_counter = read_u32(scene.wrapping_add(0x28))?;
        let epoch = read_u32(base.wrapping_add(0x3BCD478))?;
        Some((instances, count, scene_counter, epoch))
    })();
    let Some((instances, count, scene_counter, epoch)) = header else {
        rt.particle_lost().fetch_add(1, SeqCst);
        return;
    };
    let view = if context != 0 { read_u64(context.wrapping_add(0x18)).unwrap_or(0) } else { 0 };
    for i in 0..count {
        let at = rt.particle_count().load(SeqCst);
        if at == CAPACITY {
            rt.particle_lost().fetch_add(1, SeqCst);
            return;
        }
        let mut row = ParticleRecord {
            id: emitter + 16,
            view,
            kind,
            thread: thread(),
            caller: rt.rva(caller),
            node: rt.rva(crate::render_tls::current_node_work() as u64),
            side: crate::render_tls::view_side(),
            epoch,
            scene_counter,
            instance_index: i,
            total_instances: count,
            onscreen,
            anchor: rt.count().load(SeqCst),
            ..ParticleRecord::default()
        };
        if view != 0 {
            if let Some(name) = read_u64(view.wrapping_add(0x28)) {
                if name == (rt.vrcam_key)() {
                    row.side = 1;
                } else if (rt.is_main_view)(view) {
                    row.side = 0;
                }
            }
        }
        let mut valid = (|| {
            row.instance = read_u64(instances.wrapping_add(u64::from(i) * 8))?;
            if row.instance == 0 {
                return None;
            }
            row.state = read_u64(row.instance.wrapping_add(0x18))?;
            row.accumulated = f32::from_bits(read_u32(row.instance.wrapping_add(0x24))?);
            Some(())
        })()
        .is_some();
        if valid && row.state != 0 {
            match read_u32(row.state.wrapping_add(0x7C)) {
                Some(stamp) => row.stamp = stamp,
                None => valid = false,
            }
        }
        if system != 0 {
            valid = valid && read_u32(system.wrapping_add(0x50)).map(|d| row.system_delta = f32::from_bits(d)).is_some();
        }
        valid = valid && read_u32(emitter.wrapping_add(0x1C0)).map(|d| row.parameter_delta = f32::from_bits(d)).is_some();
        row.valid = valid as u32;
        row.sequence = u64::from(at) + 1;
        row.qpc = now();
        // SAFETY: inside the particle records' array.
        unsafe { *rt.particles.add(at as usize) = row };
        rt.particle_count().store(at + 1, Release);
    }
}
/// Observe: a bounds write to the traced pool -- from the three known callers (indices 0x74 or more past the scene,
/// the scene's pool exactly) its key, slot and box -- with the stack's RVAs; the written emitter's particles after it.
pub fn observe(rt: &Runtime, pool: u64, indices: u64, bounds: u64, caller: u64) {
    crate::scene_update::cvr_su_bounds_write(pool, indices, caller);
    if pool == 0 || ACTIVE_POOL.load(Relaxed) != pool {
        return;
    }
    let state = lock();
    if ACTIVE_POOL.load(SeqCst) != pool {
        return;
    }
    let mut state = state;
    state.next_span += 1;
    let mut row = Record { span: state.next_span, pool, kind: kind::WRITE_BEGIN, result: u32::MAX, caller: rt.rva(caller), node: rt.rva(crate::render_tls::current_node_work() as u64), side: crate::render_tls::view_side(), ..Record::default() };
    if matches!(row.caller, 0x32C04B | 0x29ADE0 | 0x2EA47CD) && indices >= 0x74 {
        let mut key = [0u8; 8];
        let mut boxed = [0u8; 32];
        if read_u64(indices - 12).is_some_and(|scene| scene.wrapping_add(0x7E0F0) == pool) && read(indices, &mut key) && read(bounds, &mut boxed) {
            row.id = indices - 0x64;
            row.key = u32::from_le_bytes(key[0..4].try_into().unwrap());
            row.slot = u32::from_le_bytes(key[4..8].try_into().unwrap());
            row.flags = 1;
            let f = |i: usize| f32::from_le_bytes(boxed[i * 4..i * 4 + 4].try_into().unwrap());
            for i in 0..3 {
                row.bounds[i] = f(i);
                row.bounds[i + 3] = f(i + 4);
            }
        }
    }
    let mut frames = [core::ptr::null_mut::<c_void>(); 12];
    // SAFETY: twelve slots.
    row.stack_count = u32::from(unsafe { windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, 12, frames.as_mut_ptr(), core::ptr::null_mut()) });
    for i in 0..row.stack_count as usize {
        row.stack[i] = rt.rva(frames[i] as u64);
    }
    append(rt, row);
    if row.flags != 0 && row.id >= 16 {
        particle_snapshot(rt, &state, row.id - 16, 0, u32::MAX, 1, caller, 0);
    }
}
/// Begin: the trace of `pool` (its scene's emitters, sorted) started when none runs and the trace is compatible.
pub fn begin(rt: &Runtime, pool: u64) -> bool {
    let mut state = lock();
    if pool == 0 || ACTIVE_POOL.load(SeqCst) != 0 || rt.compatible().load(SeqCst) != 1 {
        return false;
    }
    let scene = pool.wrapping_sub(0x7E0F0);
    if pool < 0x7E0F0 {
        return false;
    }
    let (Some(array), Some(count)) = (read_u64(scene + 0x7E190), read_u32(scene + 0x7E19C)) else { return false };
    if count > 65536 || (count != 0 && array == 0) {
        return false;
    }
    let mut bytes = vec![0u8; count as usize * 8];
    if count != 0 && !read(array, &mut bytes) {
        return false;
    }
    state.emitters = bytes.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
    state.emitters.sort_unstable();
    state.next_span = 0;
    state.closing = false;
    rt.count().store(0, SeqCst);
    rt.lost().store(0, SeqCst);
    rt.particle_count().store(0, SeqCst);
    rt.particle_lost().store(0, SeqCst);
    ACTIVE_POOL.store(pool, SeqCst);
    append(rt, Record { kind: kind::BEGIN, pool, ..Record::default() });
    true
}
pub fn mark(rt: &Runtime, kind: u32, side: i32, native_counter: u32) {
    let _state = lock();
    let pool = ACTIVE_POOL.load(SeqCst);
    if pool == 0 {
        return;
    }
    append(rt, Record { kind, side, key: native_counter, pool, node: rt.rva(crate::render_tls::current_node_work() as u64), ..Record::default() });
}
pub fn end(rt: &Runtime) -> bool {
    let mut state = lock();
    if !state.closing {
        let pool = ACTIVE_POOL.swap(0, SeqCst);
        append(rt, Record { kind: kind::END, pool, ..Record::default() });
        state.closing = true;
    }
    true
}
/// BeforeParticle: an emitter's instances prepared while a pool is traced (the caller and its onscreen flag at the
/// thunk's stack).
pub fn before_particle(rt: &Runtime, system: u64, context: u64, emitter: u64, stack: u64) {
    if ACTIVE_POOL.load(Relaxed) == 0 {
        return;
    }
    let state = lock();
    if ACTIVE_POOL.load(SeqCst) == 0 {
        return;
    }
    let mut onscreen = [0u8; 1];
    let Some(caller) = read_u64(stack).filter(|_| read(stack.wrapping_add(0x28), &mut onscreen)) else {
        rt.particle_lost().fetch_add(1, SeqCst);
        return;
    };
    particle_snapshot(rt, &state, emitter, context, u32::from(onscreen[0]), 0, caller, system);
}

// ---- the C ABI

/// # Safety
/// `runtime` is the .cpp's static.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_bt_attach(runtime: *const Runtime) {
    RUNTIME.store(runtime as *mut Runtime, Release);
}
/// 0 Compatible, 1 ParticleCompatible.
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_compatible(which: u32) -> bool {
    runtime().is_some_and(|rt| if which == 0 { compatible(rt) } else { particle_compatible(rt) })
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_observe(pool: u64, indices: u64, bounds: u64, caller: u64) {
    if let Some(rt) = runtime() {
        observe(rt, pool, indices, bounds, caller);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_before_particle(system: u64, context: u64, emitter: u64, stack: u64) {
    if let Some(rt) = runtime() {
        before_particle(rt, system, context, emitter, stack);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_begin(pool: u64) -> bool {
    runtime().is_some_and(|rt| begin(rt, pool))
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_mark(kind: u32, side: i32, native_counter: u32) {
    if let Some(rt) = runtime() {
        mark(rt, kind, side, native_counter);
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_bt_end() -> bool {
    runtime().is_some_and(end)
}

#[cfg(test)]
mod tests {
    //! A trace over fabricated memory: a pool's emitters, a bounds write from a known caller with its particles, the
    //! marks, the capacity.
    use super::*;

    static BASE: AtomicU64 = AtomicU64::new(0);
    extern "C" fn base() -> u64 {
        BASE.load(SeqCst)
    }
    extern "C" fn key() -> u64 {
        0x5151
    }
    extern "C" fn main_view(_: u64) -> bool {
        false
    }

    #[test]
    fn trace_writes_particles_and_marks() {
        let atom = || Box::leak(Box::new(AtomicU32::new(0))) as &'static AtomicU32;
        let records: &'static mut [Record] = Box::leak(vec![Record::default(); CAPACITY as usize].into_boxed_slice());
        let particles: &'static mut [ParticleRecord] = Box::leak(vec![ParticleRecord::default(); CAPACITY as usize].into_boxed_slice());
        // The render thread's state: the second view, no node.
        // SAFETY: this thread's block.
        unsafe {
            (*crate::render_tls::cvr_render_tls()).view_side = 1;
            (*crate::render_tls::cvr_render_tls()).current_node_work = 0;
        }
        let rt: &'static Runtime = Box::leak(Box::new(Runtime {
            records: records.as_mut_ptr(),
            particles: particles.as_mut_ptr(),
            compatible: atom(),
            count: atom(),
            lost: atom(),
            particle_compatible: atom(),
            particle_count: atom(),
            particle_lost: atom(),
            exe_base: base,
            vrcam_key: key,
            is_main_view: main_view,
        }));
        // A scene with one emitter; the emitter with two instances.
        let mut scene = vec![0u8; 0x7E1A0];
        let mut emitter = vec![0u8; 0x200];
        let mut instances = [0u64; 2];
        let mut instance = vec![0u8; 0x30];
        let mut particle_state = vec![0u8; 0x80];
        let scene_at = scene.as_mut_ptr() as u64;
        let emitter_at = emitter.as_mut_ptr() as u64;
        let list = [emitter_at];
        scene[0x7E190..0x7E198].copy_from_slice(&(list.as_ptr() as u64).to_le_bytes());
        scene[0x7E19C..0x7E1A0].copy_from_slice(&1u32.to_le_bytes());
        scene[0x28..0x2C].copy_from_slice(&9u32.to_le_bytes());
        particle_state[0x7C..0x80].copy_from_slice(&55u32.to_le_bytes());
        instance[0x18..0x20].copy_from_slice(&(particle_state.as_ptr() as u64).to_le_bytes());
        instance[0x24..0x28].copy_from_slice(&1.5f32.to_bits().to_le_bytes());
        instances[0] = instance.as_ptr() as u64;
        instances[1] = instance.as_ptr() as u64;
        emitter[0xB8..0xC0].copy_from_slice(&(instances.as_ptr() as u64).to_le_bytes());
        emitter[0xC4..0xC8].copy_from_slice(&2u32.to_le_bytes());
        emitter[0x68..0x70].copy_from_slice(&scene_at.to_le_bytes());
        let epoch = Box::leak(Box::new(4u32)) as *const u32 as u64;
        BASE.store(epoch - 0x3BCD478, SeqCst);
        let game = BASE.load(SeqCst);
        let pool = scene_at + 0x7E0F0;
        assert!(!begin(rt, pool), "not compatible");
        rt.compatible().store(1, SeqCst);
        assert!(begin(rt, pool) && !begin(rt, pool), "one trace at a time");
        // A write whose indices sit 0x64 past the emitter's id + 16: the scene pointer at indices - 12.
        let mut holder = vec![0u8; 0x100];
        let indices = emitter_at + 16 + 0x64;
        let _ = &mut holder;
        let bounds = [1.0f32, 2.0, 3.0, 0.0, 4.0, 5.0, 6.0, 0.0];
        // The emitter's own bytes hold the scene pointer at indices - 12 and the key at indices.
        let at = (indices - emitter_at) as usize;
        emitter[at - 12..at - 4].copy_from_slice(&scene_at.to_le_bytes());
        emitter[at..at + 4].copy_from_slice(&7u32.to_le_bytes());
        emitter[at + 4..at + 8].copy_from_slice(&3u32.to_le_bytes());
        observe(rt, pool, indices, bounds.as_ptr() as u64, game + 0x32C04B);
        let row = records[1];
        assert!(row.kind == kind::WRITE_BEGIN && row.flags == 1 && row.key == 7 && row.slot == 3 && row.bounds == [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] && row.side == 1);
        assert_eq!(rt.particle_count().load(SeqCst), 2, "the emitter's two instances");
        let p = particles[0];
        assert!(p.valid == 1 && p.stamp == 55 && p.accumulated == 1.5 && p.scene_counter == 9 && p.epoch == 4 && p.total_instances == 2 && p.id == emitter_at + 16);
        observe(rt, pool + 8, indices, 0, game + 0x32C04B);
        assert_eq!(rt.count().load(SeqCst), 2, "another pool is not traced");
        mark(rt, 3, 1, 42);
        assert!(end(rt) && end(rt));
        assert_eq!(records[2].key, 42);
        assert_eq!((records[3].kind, rt.count().load(SeqCst)), (kind::END, 4), "one end");
    }
}
