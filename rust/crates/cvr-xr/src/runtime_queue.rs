//! THE RUNTIME'S OWN D3D12 QUEUE (main's 0.2.4: OpenXRManager::m_xrQueue, m_xrOrderFence, RuntimeQueue,
//! OrderRuntimeAfter). The session is bound to a high-priority DIRECT queue of the port's own, so the runtime's copies
//! and the port's eye copies cut in front of the game's long GPU work instead of waiting behind it. Not under SteamVR on
//! Radeon (its compositor reads the projection images on the bound queue: the game's queue orders the inline and
//! generated eye copies with those reads) nor with the test marker cvr_xr_game_queue.txt beside the game's exe.
//!
//! Work recorded on the game's queue for a swapchain image (or anything the runtime's queue reads next) is ordered on
//! the GPU with order_runtime_after: the game's queue signals the order fence, the runtime's waits for it.
use core::ffi::c_void;
use core::sync::atomic::{AtomicPtr, Ordering::*};
use std::sync::Mutex;

use cvr_host::log;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    ID3D12CommandQueue, ID3D12Device, ID3D12Fence, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC,
    D3D12_COMMAND_QUEUE_PRIORITY_HIGH, D3D12_FENCE_FLAG_NONE,
};

/// The runtime's queue (one reference of ours), or null when the runtime works on the game's.
static QUEUE: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The game's queue the session was made with (borrowed).
static GAME: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// The order fence and its last value.
static ORDER: Mutex<(Option<ID3D12Fence>, u64)> = Mutex::new((None, 0));

/// The queue the session binds: the runtime's own when it can be made (and is wanted), else the game's. Logs which.
///
/// # Safety
/// `device` and `game` are the game's live device and DIRECT queue.
pub unsafe fn create(device: *mut c_void, game: *mut c_void, radeon_steamvr: bool, game_queue_marker: bool) -> *mut c_void {
    GAME.store(game, Release);
    if radeon_steamvr {
        log!("OpenXRManager: SteamVR/Radeon uses the game DIRECT queue for the projection swapchains\n");
        return game;
    }
    if game_queue_marker || !QUEUE.load(Acquire).is_null() {
        return if QUEUE.load(Acquire).is_null() { game } else { QUEUE.load(Acquire) };
    }
    // SAFETY: the caller's live device.
    let Some(device) = (unsafe { ID3D12Device::from_raw_borrowed(&device) }) else {
        return game;
    };
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        Priority: D3D12_COMMAND_QUEUE_PRIORITY_HIGH.0,
        ..Default::default()
    };
    // SAFETY: plain object creation on the live device.
    let made = unsafe {
        device.CreateCommandQueue::<ID3D12CommandQueue>(&desc).ok().zip(device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE).ok())
    };
    let Some((queue, fence)) = made else {
        log!("OpenXRManager: no queue of its own for the runtime; it works on the game's\n");
        return game;
    };
    // SAFETY: our new objects.
    unsafe {
        let _ = queue.SetName(windows::core::w!("CVR_OpenXR_runtime_queue"));
        let _ = fence.SetName(windows::core::w!("CVR_OpenXR_runtime_order_fence"));
    }
    *ORDER.lock().unwrap_or_else(|e| e.into_inner()) = (Some(fence), 0);
    let raw = queue.into_raw();
    QUEUE.store(raw, Release);
    log!("OpenXRManager: the runtime works on a queue of its own ({:016X}; the game's {:016X})\n", raw as usize, game as usize);
    raw
}

/// RuntimeQueue(): the runtime's own queue, else the game's (null before the session).
pub fn runtime_queue() -> *mut c_void {
    let own = QUEUE.load(Acquire);
    if own.is_null() {
        GAME.load(Acquire)
    } else {
        own
    }
}
/// The runtime works on a queue of its own (main's `m_xrQueue != nullptr`).
pub fn has_own() -> bool {
    !QUEUE.load(Acquire).is_null()
}
/// The game's queue the session was made with.
pub fn game_queue() -> *mut c_void {
    GAME.load(Acquire)
}

/// OrderRuntimeAfter: the runtime's queue waits on the GPU for what `used` has queued so far (nothing when the runtime
/// has no queue of its own or `used` is it).
///
/// # Safety
/// `used` is a live queue of the device, or null.
pub unsafe fn order_runtime_after(used: *mut c_void) {
    let own = QUEUE.load(Acquire);
    if own.is_null() || used.is_null() || used == own {
        return;
    }
    let mut order = ORDER.lock().unwrap_or_else(|e| e.into_inner());
    let (fence, value) = &mut *order;
    let Some(fence) = fence.as_ref() else { return };
    // SAFETY: the caller's live queue and our own.
    unsafe {
        let (Some(used), Some(runtime)) =
            (ID3D12CommandQueue::from_raw_borrowed(&used), ID3D12CommandQueue::from_raw_borrowed(&own))
        else {
            return;
        };
        *value += 1;
        if used.Signal(fence, *value).is_ok() {
            let _ = runtime.Wait(fence, *value);
        }
    }
}

/// The session's end: the runtime's queue and the order fence released.
pub fn release() {
    let own = QUEUE.swap(core::ptr::null_mut(), AcqRel);
    if !own.is_null() {
        // SAFETY: our reference, taken in create.
        drop(unsafe { ID3D12CommandQueue::from_raw(own) });
    }
    *ORDER.lock().unwrap_or_else(|e| e.into_inner()) = (None, 0);
    GAME.store(core::ptr::null_mut(), Release);
}

/// C names for the render crate's and the C++ half's callers.
#[no_mangle]
pub extern "C" fn cvr_xr_runtime_queue() -> *mut c_void {
    runtime_queue()
}
#[no_mangle]
pub extern "C" fn cvr_xr_has_runtime_queue() -> bool {
    has_own()
}
/// # Safety
/// As order_runtime_after.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_order_runtime_after(used: *mut c_void) {
    // SAFETY: as the caller vouches.
    unsafe { order_runtime_after(used) }
}
