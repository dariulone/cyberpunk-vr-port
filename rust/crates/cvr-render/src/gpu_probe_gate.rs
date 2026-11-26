//! The stereo GPU probe's preparation gate and readback (src/Render/StereoGpuProbe.cpp): the gate that holds the
//! producer's queue on a fence of its device while MAIN's CPU preparation runs (created at the producer's submission:
//! the queue waits on its fence, a timer thread releases it with reason 2 after the requested interval at the latest,
//! a failed wait releases it with reason 3); the publication of a matched draw's predicate only while the gate is
//! closed; the release (once, by its first reason: the commit word written and published when given, the write-combined
//! bytes fenced before the GPU wakes, the release time and reason exported, the fence signalled); and, once the probe's
//! fence completed, the readback mapped, its comparison (gpu_probe_compare), the scene's occlusion samples and the
//! timed scopes (gpu_probe_timings), unmapped with nothing written.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp holds the gate through a small owner and keeps the probe's state, the
//! surfaces' layouts, the VRS read and the results; the unit test below runs the gate and the readback on WARP, and
//! tools/single_pass_tests/scene_probe_test runs every probe mode (the gate's timeout and late-visibility ones among
//! them) through the .cpp in its Rust variant.

use crate::gpu_probe_compare::{compare, Difference, Exports, Layout, Totals};
use crate::gpu_probe_timings::{timings, Timed};
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Performance::QueryPerformanceCounter;

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_OUTOFMEMORY: i32 = 0x8007_000Eu32 as i32;
const DXGI_ERROR_DEVICE_REMOVED: i32 = 0x887A_0005u32 as i32;

fn now() -> u64 {
    let mut q = 0i64;
    // SAFETY: a local output.
    let _ = unsafe { QueryPerformanceCounter(&mut q) };
    q as u64
}
fn borrowed<T: Interface>(raw: &*mut c_void) -> Option<&T> {
    // SAFETY: a live COM object the caller holds for the call (or null).
    unsafe { T::from_raw_borrowed(raw) }
}

/// The gate's exports.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GateExports {
    pub committed: *const AtomicU32,
    pub release: *const AtomicU64,
    pub reason: *const AtomicU32,
    pub start: *const AtomicU64,
}
/// The preparation gate.
pub struct Gate {
    fence: ID3D12Fence,
    armed: Mutex<bool>,
    wake: Condvar,
    reason: AtomicU32,
    publication: Mutex<()>,
    exports: GateExports,
}
// SAFETY: the fence is free-threaded; the exports are process-lifetime atomics.
unsafe impl Send for Gate {}
unsafe impl Sync for Gate {}
impl Gate {
    pub fn reason(&self) -> u32 {
        self.reason.load(Ordering::SeqCst)
    }
    /// Publish: a matched draw's predicate word, only while the gate is closed.
    pub fn publish(&self, address: *mut u64) -> bool {
        let _held = self.publication.lock().unwrap_or_else(|e| e.into_inner());
        if self.reason.load(Ordering::SeqCst) != 0 {
            return false;
        }
        // SAFETY: the predicate word, mapped for the probe's lifetime.
        unsafe { core::ptr::write_volatile(address, 1) };
        true
    }
    /// Release: once, by its first reason.
    pub fn release(&self, why: u32, commit: *mut u64) -> bool {
        let _held = self.publication.lock().unwrap_or_else(|e| e.into_inner());
        if self.reason.compare_exchange(0, why, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            return false;
        }
        // SAFETY: the commit word mapped for the probe's lifetime (or null); the exports live atomics.
        unsafe {
            if !commit.is_null() {
                core::ptr::write_volatile(commit, 1);
                (*self.exports.committed).store(1, Ordering::SeqCst);
            }
            // Publish write-combined upload bytes before waking the GPU.
            core::arch::x86_64::_mm_sfence();
            (*self.exports.release).store(now(), Ordering::SeqCst);
            (*self.exports.reason).store(why, Ordering::SeqCst);
            let _ = self.fence.Signal(1);
        }
        true
    }
    fn arm(&self) {
        *self.armed.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_one();
    }
    fn wait_armed(&self) {
        let mut armed = self.armed.lock().unwrap_or_else(|e| e.into_inner());
        while !*armed {
            armed = self.wake.wait(armed).unwrap_or_else(|e| e.into_inner());
        }
    }
}
/// The gate at the producer's submission: its fence on the queue's device, the timer thread (released with reason 2
/// `ms` after arming), the queue's wait (a failure releases it with reason 3). The gate comes back whenever the timer
/// thread started, with the wait's result.
pub fn begin(queue: &ID3D12CommandQueue, ms: u32, exports: GateExports) -> (Option<Arc<Gate>>, i32) {
    let mut device: Option<ID3D12Device> = None;
    // SAFETY: a live queue; the output a local.
    if let Err(e) = unsafe { queue.GetDevice(&mut device) } {
        return (None, e.code().0);
    }
    let Some(device) = device else { return (None, E_FAIL) };
    // SAFETY: a live device.
    let fence: ID3D12Fence = match unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) } {
        Ok(fence) => fence,
        Err(e) => return (None, e.code().0),
    };
    let gate = Arc::new(Gate { fence, armed: Mutex::new(false), wake: Condvar::new(), reason: AtomicU32::new(0), publication: Mutex::new(()), exports });
    let timer = gate.clone();
    let spawned = std::thread::Builder::new().spawn(move || {
        timer.wait_armed();
        std::thread::sleep(std::time::Duration::from_millis(u64::from(ms)));
        timer.release(2, core::ptr::null_mut());
    });
    if spawned.is_err() {
        return (None, E_OUTOFMEMORY);
    }
    // SAFETY: the exports are live atomics.
    unsafe { (*exports.start).store(now(), Ordering::SeqCst) };
    // SAFETY: a live queue and fence.
    let waited = unsafe { queue.Wait(&gate.fence, 1) }.map_or_else(|e| e.code().0, |_| S_OK);
    gate.arm();
    if waited < 0 {
        gate.release(3, core::ptr::null_mut());
    }
    (Some(gate), waited)
}

/// The readback's exports.
#[repr(C)]
pub struct ReadExports {
    pub eyes: *mut [u64; 2],
    pub targets: *mut [u64; 4],
    pub samples: *mut [Difference; 128],
    pub count: *mut u32,
    pub scene: *mut [u64; 2],
    pub drops: *const AtomicU32,
    pub ticks: *mut [[u64; 2]; 7],
    pub timed: *mut [[u32; 2]; 7],
}
const _: () = assert!(core::mem::size_of::<ReadExports>() == 64 && core::mem::size_of::<GateExports>() == 32);
/// What Poll reads back.
pub struct Readback<'a> {
    pub bytes: u64,
    pub surfaces: &'a [[Layout; 4]; 2],
    pub width: u32,
    pub depth_only: bool,
    pub scene_query: Option<u64>,
    pub timed: Option<(&'a [Timed], u64)>,
}
/// Poll's readback: S_FALSE until the probe's fence completed; the comparison, the scene's samples and the timings.
///
/// # Safety
/// The exports are live and writable.
pub unsafe fn read(fence: &ID3D12Fence, readback: &ID3D12Resource, r: &Readback<'_>, e: &ReadExports) -> Result<Totals, i32> {
    // SAFETY: a live fence.
    let completed = unsafe { fence.GetCompletedValue() };
    if completed == u64::MAX {
        return Err(DXGI_ERROR_DEVICE_REMOVED);
    }
    if completed < 1 {
        return Err(S_FALSE);
    }
    let mut data = core::ptr::null_mut();
    // SAFETY: a live READBACK buffer of `bytes`.
    if let Err(error) = unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: r.bytes as usize }), Some(&mut data)) } {
        return Err(error.code().0);
    }
    let unmap = || unsafe { readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    // SAFETY: the mapping holds `bytes` readable bytes.
    let image = if data.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(data as *const u8, r.bytes as usize) } };
    // SAFETY: as documented.
    let exports = unsafe { Exports { eyes: &mut *e.eyes, targets: &mut *e.targets, samples: &mut *e.samples, count: &mut *e.count } };
    let Some(totals) = compare(image, r.surfaces, r.width, r.depth_only, exports) else {
        unmap();
        return Err(E_FAIL);
    };
    if let Some(at) = r.scene_query {
        if let Some(samples) = image.get(at as usize..at as usize + 16) {
            // SAFETY: as documented.
            let scene = unsafe { &mut *e.scene };
            scene[0] = u64::from_le_bytes(samples[..8].try_into().unwrap());
            scene[1] = u64::from_le_bytes(samples[8..].try_into().unwrap());
        }
    }
    if let Some((scopes, offset)) = r.timed {
        // SAFETY: as documented.
        let dropped = unsafe { timings(image, scopes, offset, &mut *e.ticks, &mut *e.timed) };
        unsafe { (*e.drops).fetch_add(dropped, Ordering::SeqCst) };
    }
    unmap();
    Ok(totals)
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// The gate at the producer's submission into `out` (null when it never started), with the wait's result.
///
/// # Safety
/// A live queue; the exports live atomics; `out` a pointer.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpg_begin(queue: *mut c_void, ms: u32, exports: *const GateExports, out: *mut *const Gate) -> i32 {
    let Some(queue) = borrowed::<ID3D12CommandQueue>(&queue) else { return E_INVALIDARG };
    // SAFETY: as documented.
    let (gate, hr) = begin(queue, ms, unsafe { *exports });
    unsafe { *out = gate.map_or(core::ptr::null(), Arc::into_raw) };
    hr
}
/// Releases the owner's reference (null: nothing).
///
/// # Safety
/// From cvr_sgpg_begin, once.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpg_free(gate: *const Gate) {
    if !gate.is_null() {
        // SAFETY: as documented.
        drop(unsafe { Arc::from_raw(gate) });
    }
}
/// The gate's release reason (0 while closed).
///
/// # Safety
/// A live gate.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpg_reason(gate: *const Gate) -> u32 {
    // SAFETY: as documented.
    unsafe { (*gate).reason() }
}
/// Publish.
///
/// # Safety
/// A live gate; `address` a mapped predicate word.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpg_publish(gate: *const Gate, address: *mut u64) -> bool {
    // SAFETY: as documented.
    unsafe { (*gate).publish(address) }
}
/// Release.
///
/// # Safety
/// A live gate; `commit` null or a mapped word.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sgpg_release(gate: *const Gate, why: u32, commit: *mut u64) -> bool {
    // SAFETY: as documented.
    unsafe { (*gate).release(why, commit) }
}
/// Poll's readback (`scene_query` UINT64_MAX: none; `timestamps` false: no timed scopes): S_OK with the totals, S_FALSE
/// while pending, or the failure.
///
/// # Safety
/// A live fence and readback; `surfaces` two modes of four layouts; `timed` `count` scopes; the exports live.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_sgpr_read(
    fence: *mut c_void,
    readback: *mut c_void,
    bytes: u64,
    surfaces: *const [[Layout; 4]; 2],
    width: u32,
    depth_only: bool,
    scene_query: u64,
    timed: *const Timed,
    count: usize,
    timestamps: bool,
    timestamp_offset: u64,
    exports: *const ReadExports,
    totals: *mut Totals,
) -> i32 {
    let (Some(fence), Some(readback)) = (borrowed::<ID3D12Fence>(&fence), borrowed::<ID3D12Resource>(&readback)) else { return E_INVALIDARG };
    let scopes = if timed.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(timed, count) } };
    let r = Readback {
        bytes,
        surfaces: unsafe { &*surfaces },
        width,
        depth_only,
        scene_query: (scene_query != u64::MAX).then_some(scene_query),
        timed: timestamps.then_some((scopes, timestamp_offset)),
    };
    // SAFETY: as documented.
    match unsafe { read(fence, readback, &r, &*exports) } {
        Ok(found) => {
            unsafe { *totals = found };
            S_OK
        }
        Err(hr) => hr,
    }
}

#[cfg(test)]
mod tests {
    //! On WARP: the gate released once (the commit word and exports), its timer releasing a gate left closed; the
    //! readback pending, then compared, its samples and timings read.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    fn warp() -> ID3D12Device {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    #[test]
    fn gate_and_readback() {
        let device = warp();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let (committed, release, reason, start) = (AtomicU32::new(0), AtomicU64::new(0), AtomicU32::new(0), AtomicU64::new(0));
        let exports = GateExports { committed: &committed, release: &release, reason: &reason, start: &start };
        let (gate, hr) = begin(&queue, 5000, exports);
        let gate = gate.unwrap();
        assert_eq!((hr, gate.reason(), start.load(Ordering::SeqCst) != 0), (S_OK, 0, true));
        let mut predicate = 0u64;
        assert!(gate.publish(&mut predicate) && predicate == 1, "published while closed");
        let mut commit = 0u64;
        assert!(gate.release(1, &mut commit));
        assert_eq!((commit, committed.load(Ordering::SeqCst), reason.load(Ordering::SeqCst)), (1, 1, 1));
        assert!(!gate.release(4, core::ptr::null_mut()) && gate.reason() == 1, "released once, by its first reason");
        let mut late = 0u64;
        assert!(!gate.publish(&mut late) && late == 0, "nothing published after the release");
        let (timed_out, _) = begin(&queue, 1, exports);
        let timed_out = timed_out.unwrap();
        while timed_out.reason() == 0 {
            std::thread::yield_now();
        }
        assert_eq!(timed_out.reason(), 2, "the timer releases a closed gate");
        // The readback: one-plane surfaces of one 16-byte row, every mode's copy equal, and the scene's samples.
        let readback = crate::readback_buffers::buffer(&device, None, 512, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        let mut zeroed = core::ptr::null_mut();
        unsafe { readback.Map(0, None, Some(&mut zeroed)) }.unwrap();
        unsafe { core::ptr::write_bytes(zeroed as *mut u8, 0, 512) };
        unsafe { readback.Unmap(0, None) };
        let surfaces: [[Layout; 4]; 2] = core::array::from_fn(|_| {
            core::array::from_fn(|_| {
                let mut l = Layout { planes: 1, ..Default::default() };
                for sub in 0..2 {
                    l.footprints[sub].Footprint.RowPitch = 16;
                    l.row_bytes[sub] = 16;
                    l.rows[sub] = 1;
                }
                l
            })
        });
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        let (mut eyes, mut targets, mut samples, mut count, mut scene) = ([0u64; 2], [0u64; 4], [Difference::default(); 128], 0u32, [0u64; 2]);
        let (drops, mut ticks, mut timed) = (AtomicU32::new(0), [[0u64; 2]; 7], [[0u32; 2]; 7]);
        let e = ReadExports { eyes: &mut eyes, targets: &mut targets, samples: &mut samples, count: &mut count, scene: &mut scene, drops: &drops, ticks: &mut ticks, timed: &mut timed };
        let r = Readback { bytes: 512, surfaces: &surfaces, width: 4, depth_only: false, scene_query: Some(256), timed: Some((&[Timed { stage: 1, eye: 0, index: 36 }], 0)) };
        assert_eq!(unsafe { read(&fence, &readback, &r, &e) }.err(), Some(S_FALSE), "pending");
        unsafe { fence.Signal(1) }.unwrap();
        let totals = unsafe { read(&fence, &readback, &r, &e) }.unwrap();
        assert_eq!((totals.different, totals.compared), (0, 4 * 2 * 16), "every byte compared, none different");
        assert_eq!((drops.load(Ordering::SeqCst), timed[1][0]), (0, 1), "a zeroed timed pair counted");
    }
}
