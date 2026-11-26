//! The opt-in CPU trace of single pass's common geometry preparation (formerly src/Render/SinglePassTrace.cpp): asked
//! for through CyberpunkVR_SinglePassTraceRequest (a number of frames), every camera upload and command list of the
//! traced nodes is written into CyberpunkVR_SinglePassTraceRecords (512 rows) with the camera bytes, until the frames
//! are done or the table is full. No GPU work, files or resource references; camera bytes are copied only for the
//! requested frames.
//!
//! The camera builder's inputs are read by walking up the current thread's stack (cvr_engine::stack: the OS unwinder
//! under SEH, memory through ReadProcessMemory). The exports stay in C++; the C++ half attaches them at its static
//! initialisation.
use core::ffi::c_void;
use std::cell::Cell;
use std::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

use windows::Win32::System::Performance::QueryPerformanceCounter;
use windows::Win32::System::Threading::GetCurrentThreadId;

/// cvr::stereo::trace::Record (4616 bytes).
#[repr(C)]
pub struct Record {
    pub sequence: u64,
    pub qpc: u64,
    pub object: u64,
    pub frame: u32,
    pub node: u32,
    pub kind: u32,
    pub index: u32,
    pub side: i32,
    pub thread: u32,
    pub camera: [u8; 848],
    pub context: u64,
    pub view: u64,
    pub camera_address: u64,
    pub width: u32,
    pub height: u32,
    pub input_valid: u32,
    pub reserved: u32,
    pub input: [u8; 904],
    pub clip: [u8; 16],
    pub prediction_valid: u32,
    pub prediction_counter: u32,
    pub prediction: [u8; 904],
    pub shader_prediction_valid: u32,
    pub previous_input_valid: u32,
    pub shader_prediction: [u8; 848],
    pub previous_input: [u8; 904],
    pub valid_shader_words: [u64; 4],
    pub peer_source_pose: [u8; 32],
    pub twin_cpu_descriptor: u64,
    pub twin_gpu_address: u64,
    pub twin_bytes: u32,
    pub prediction_peer_side: u32,
}
const _: () = assert!(core::mem::size_of::<Record>() == 4616);
const CAPACITY: u32 = 512;

/// CvrSinglePassTraceAttach: the C++ half's exports.
#[repr(C)]
pub struct Attach {
    request: *const AtomicU32,
    state: *const AtomicU32,
    seq: *const AtomicU32,
    count: *const AtomicU32,
    node_filter: *const AtomicU32,
    records: *mut Record,
}
static ATTACH: AtomicPtr<Attach> = AtomicPtr::new(core::ptr::null_mut());
/// The plugin's own exports and table (formerly src/Render/SinglePassTrace.cpp's): the records and their size here, the
/// switches declared_moved_exports.rs'. The Rust test variants still attach their reference definitions.
#[cfg(feature = "rust-build")]
mod plugin {
    #![allow(non_upper_case_globals)]
    use super::{Attach, Record, CAPACITY};
    use crate::declared_moved_exports::*;
    // SAFETY: immutable; its pointers name process-lifetime exports.
    unsafe impl Sync for Attach {}
    // SAFETY: a record is plain data; all zeroes is its initial state.
    const ZERO: Record = unsafe { core::mem::zeroed() };
    #[no_mangle]
    pub static mut CyberpunkVR_SinglePassTraceRecordBytes: u32 = core::mem::size_of::<Record>() as u32;
    #[no_mangle]
    pub static mut CyberpunkVR_SinglePassTraceRecords: [Record; CAPACITY as usize] = [ZERO; CAPACITY as usize];
    pub static ATTACH: Attach = Attach {
        request: &raw const CyberpunkVR_SinglePassTraceRequest,
        state: &raw const CyberpunkVR_SinglePassTraceState,
        seq: &raw const CyberpunkVR_SinglePassTraceSeq,
        count: &raw const CyberpunkVR_SinglePassTraceCount,
        node_filter: &raw const CyberpunkVR_SinglePassTraceNodeFilter,
        records: (&raw mut CyberpunkVR_SinglePassTraceRecords).cast(),
    };
}
fn attach() -> Option<&'static Attach> {
    // SAFETY: the C++ half's table, alive for the process.
    let attached = unsafe { ATTACH.load(Ordering::Acquire).as_ref() };
    #[cfg(feature = "rust-build")]
    let attached = attached.or(Some(&plugin::ATTACH));
    attached
}
/// # Safety
/// `table` lives for the process; its records point at 512 rows.
#[no_mangle]
pub unsafe extern "C" fn cvr_sp_trace_attach(table: *const Attach) {
    ATTACH.store(table as *mut Attach, Ordering::Release);
}
fn a(p: *const AtomicU32) -> &'static AtomicU32 {
    // SAFETY: an exported std::atomic<uint32_t>.
    unsafe { &*p }
}

/// The camera builder's inputs, read from its live frame on this thread: the current EXE's builder keeps
/// context/camera/width/height in nonvolatile registers. 2.5: the builder body is sub_14228C9A0 (sub_140297904 only adds
/// an eighth argument and calls it), with context in R15, camera in RBX, width in R14D, height in ESI, none rewritten
/// before the epilogue; the fifth argument (the clip pointer) sits at RBP+0x8F0 (2.31: RSI, RDI, R15D, R14D,
/// RBP+0x8A0). The 2.5 camera is read up to +0x399, past the 904 bytes copied here. `input_valid` only when all of it
/// was read.
pub fn native_inputs(row: &mut Record) {
    use cvr_engine::stack::{read, read_value, walk};
    let game = cvr_engine::code::image_base() as u64;
    walk(24, |c, low, high| {
        if c.Rip < game.wrapping_add(0x228C9A0) || c.Rip >= game.wrapping_add(0x228D5E4) {
            return None;
        }
        row.context = c.R15;
        row.camera_address = c.Rbx;
        row.width = c.R14 as u32;
        row.height = c.Rsi as u32;
        if row.context == 0 || row.camera_address == 0 || row.width > 16384 || row.height > 16384 {
            return Some(());
        }
        let Some(view) = read_value::<u64>(row.context.wrapping_add(0x18)) else {
            return Some(());
        };
        row.view = view;
        if !read(row.camera_address, &mut row.input) {
            return Some(());
        }
        if c.Rbp.wrapping_add(0x8F8) <= high && c.Rbp.wrapping_add(0x8F0) >= low {
            let Some(clip) = read_value::<u64>(c.Rbp.wrapping_add(0x8F0)) else {
                return Some(());
            };
            if clip != 0 && !read(clip, &mut row.clip) {
                return Some(());
            }
        }
        row.input_valid = 1;
        Some(())
    });
}
/// trace::CaptureNativeInputs: [`native_inputs`] for the C++ callers that fill a row themselves.
///
/// # Safety
/// `row` is a valid record or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_sp_trace_native_inputs(row: *mut Record) {
    // SAFETY: as the caller promises.
    if let Some(row) = unsafe { row.as_mut() } {
        native_inputs(row);
    }
}

/// The trace's own state: the frame being traced and the last one asked for.
struct Frames {
    frame: u32,
    limit: u32,
}
static FRAMES: Mutex<Frames> = Mutex::new(Frames { frame: 0, limit: 0 });
static EPOCH: AtomicU64 = AtomicU64::new(0);
const RECORDING: u32 = 1;
const DONE: u32 = 2;
const FULL: u32 = 3;

type Predict = Option<extern "C" fn(*mut Record)>;

#[allow(clippy::too_many_arguments)]
fn append(
    t: &Attach,
    kind: u32,
    node: u32,
    side: i32,
    index: u32,
    object: usize,
    data: Option<&[u8; 848]>,
    predict: Predict,
) {
    let frames = FRAMES.lock().unwrap_or_else(|p| p.into_inner());
    if a(t.state).load(Ordering::Relaxed) != RECORDING {
        return;
    }
    let count = a(t.count).load(Ordering::Relaxed);
    if count == CAPACITY {
        a(t.state).store(FULL, Ordering::Release);
        return;
    }
    a(t.seq).fetch_add(1, Ordering::AcqRel);
    // SAFETY: row `count` of the exported table (count < 512), written only under FRAMES.
    let row = unsafe { &mut *t.records.add(count as usize) };
    // SAFETY: Record is plain old data; all-zero is its default.
    unsafe { core::ptr::write_bytes(row as *mut Record, 0, 1) };
    let mut now = 0i64;
    // SAFETY: plain Win32 calls.
    unsafe {
        let _ = QueryPerformanceCounter(&mut now);
        row.thread = GetCurrentThreadId();
    }
    row.sequence = count as u64 + 1;
    row.qpc = now as u64;
    row.object = object as u64;
    row.frame = frames.frame;
    row.node = node;
    row.kind = kind;
    row.index = index;
    row.side = side;
    if let Some(bytes) = data {
        row.camera.copy_from_slice(bytes);
        native_inputs(row);
        if let (Some(p), true) = (predict, row.input_valid != 0) {
            p(row);
        }
    }
    a(t.count).store(count + 1, Ordering::Relaxed);
    a(t.seq).fetch_add(1, Ordering::Release);
}

/// FrameBoundary: the next traced frame, or the start of a requested trace (4 frames at most, 8 for the node filter
/// 0x33DDA4).
pub fn frame_boundary() {
    let Some(t) = attach() else { return };
    if a(t.state).load(Ordering::Relaxed) != RECORDING && a(t.request).load(Ordering::Relaxed) == 0
    {
        return;
    }
    let mut f = FRAMES.lock().unwrap_or_else(|p| p.into_inner());
    if a(t.state).load(Ordering::Relaxed) == RECORDING {
        f.frame += 1;
        EPOCH.fetch_add(1, Ordering::Release);
        if f.frame > f.limit {
            a(t.state).store(DONE, Ordering::Release);
        }
        return;
    }
    let request = a(t.request).swap(0, Ordering::AcqRel);
    if request == 0 {
        return;
    }
    a(t.seq).fetch_add(1, Ordering::AcqRel);
    a(t.count).store(0, Ordering::Relaxed);
    f.frame = 1;
    f.limit = request.min(if a(t.node_filter).load(Ordering::SeqCst) == 0x33DDA4 {
        8
    } else {
        4
    });
    EPOCH.fetch_add(1, Ordering::Release);
    a(t.state).store(RECORDING, Ordering::Release);
    a(t.seq).fetch_add(1, Ordering::Release);
}
/// Camera: a camera upload of `node` (side 0 or 1) with its 848 bytes.
pub fn camera(node: u32, side: i32, index: u32, bytes: Option<&[u8; 848]>, predict: Predict) {
    let Some(t) = attach() else { return };
    let Some(bytes) = bytes else { return };
    if a(t.state).load(Ordering::Relaxed) != RECORDING || !(0..=1).contains(&side) {
        return;
    }
    let filter = a(t.node_filter).load(Ordering::Relaxed);
    if filter != 0 && node != filter {
        return;
    }
    append(t, 1, node, side, index, 0, Some(bytes), predict);
}
thread_local! {
    /// The last list this thread recorded: (epoch, list, node, side).
    static PREVIOUS: Cell<(u64, usize, u32, i32)> = const { Cell::new((0, 0, 0, -1)) };
}
/// List: a command list of `node`, once per frame, list, node and side on this thread.
pub fn list(node: u32, side: i32, list: usize) {
    let Some(t) = attach() else { return };
    if a(t.state).load(Ordering::Relaxed) != RECORDING || list == 0 || !(0..=1).contains(&side) {
        return;
    }
    let filter = a(t.node_filter).load(Ordering::Relaxed);
    if filter != 0 && node != filter {
        return;
    }
    let current = EPOCH.load(Ordering::Acquire);
    let key = (current, list, node, side);
    if PREVIOUS.get() == key {
        return;
    }
    PREVIOUS.set(key);
    append(t, 2, node, side, 0, list, None, None);
}

// ---- the C ABI (include/Render/SinglePassTrace.hpp)

#[no_mangle]
pub extern "C" fn cvr_sp_trace_frame_boundary() {
    frame_boundary()
}
/// # Safety
/// `bytes` is 848 readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_sp_trace_camera(
    node: u32,
    side: i32,
    index: u32,
    bytes: *const c_void,
    predict: Predict,
) {
    // SAFETY: the caller's camera bytes.
    camera(
        node,
        side,
        index,
        unsafe { (bytes as *const [u8; 848]).as_ref() },
        predict,
    )
}
#[no_mangle]
pub extern "C" fn cvr_sp_trace_list(node: u32, side: i32, list_ptr: *const c_void) {
    list(node, side, list_ptr as usize)
}
