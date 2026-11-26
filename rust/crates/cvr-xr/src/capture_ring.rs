//! The capture's command ring, owned here (formerly OpenXRManager's m_capture* members): three allocators and their
//! lists, the fence and its event, the fence value, the allocators' fence values and the current allocator's index.
//! The C++ half's capture lock (m_captureMutex) guards it, as it guarded the members; include/Runtimes/OpenXRManager.hpp's
//! members are references to it. cvr-render's xr_capture_state.rs makes its objects (EnsureMonoCaptureResource) and
//! xr_capture.rs records on it; Shutdown releases it here.
use core::cell::UnsafeCell;
use core::ffi::c_void;

#[repr(C)]
pub struct CaptureRing {
    /// ID3D12CommandAllocator*, ID3D12GraphicsCommandList* (each holding the reference made for it).
    pub allocators: [*mut c_void; 3],
    pub lists: [*mut c_void; 3],
    /// ID3D12Fence*, the fence's event (HANDLE).
    pub fence: *mut c_void,
    pub fence_event: *mut c_void,
    pub fence_value: u64,
    pub allocator_fences: [u64; 3],
    pub allocator_index: u32,
}
const _: () = assert!(
    core::mem::size_of::<CaptureRing>() == 104
        && core::mem::offset_of!(CaptureRing, fence_value) == 64
        && core::mem::offset_of!(CaptureRing, allocator_index) == 96,
    "mirrored by include/Runtimes/OpenXRManager.hpp"
);
impl CaptureRing {
    pub const EMPTY: CaptureRing = CaptureRing {
        allocators: [core::ptr::null_mut(); 3],
        lists: [core::ptr::null_mut(); 3],
        fence: core::ptr::null_mut(),
        fence_event: core::ptr::null_mut(),
        fence_value: 0,
        allocator_fences: [0; 3],
        allocator_index: 0,
    };
    /// OpenXRManager::Shutdown's part for the ring: its lists released, its fence's event closed, then `between` (the
    /// submission's events, closed there as the manager had it), then its fence and its allocators released. The fence
    /// values and the index stay.
    pub fn release(&mut self, between: impl FnOnce()) {
        self.lists.iter_mut().for_each(release_slot);
        if !self.fence_event.is_null() {
            // SAFETY: the ring's event, closed once.
            unsafe { windows_sys::Win32::Foundation::CloseHandle(self.fence_event) };
            self.fence_event = core::ptr::null_mut();
        }
        between();
        release_slot(&mut self.fence);
        self.allocators.iter_mut().for_each(release_slot);
    }
}
/// A slot's reference released and the slot emptied.
fn release_slot(slot: &mut *mut c_void) {
    use windows::core::Interface as _;
    if !slot.is_null() {
        // SAFETY: the reference the ring took when it made the object, released once.
        drop(unsafe { windows::core::IUnknown::from_raw(*slot) });
        *slot = core::ptr::null_mut();
    }
}

struct RingCell(UnsafeCell<CaptureRing>);
// SAFETY: every access is under the C++ half's capture lock (or at shutdown, with the capture stopped).
unsafe impl Sync for RingCell {}
static RING: RingCell = RingCell(UnsafeCell::new(CaptureRing::EMPTY));
#[no_mangle]
pub extern "C" fn cvr_xr_capture_ring() -> *mut CaptureRing {
    RING.0.get()
}
/// Shutdown: the ring released, the submission's events closed in between (crate::submit_state).
#[no_mangle]
pub extern "C" fn cvr_xr_capture_release_ring() {
    // SAFETY: shutdown, with the capture stopped.
    unsafe { &mut *RING.0.get() }.release(|| crate::submit_state::SUBMIT.close_events());
}

#[cfg(test)]
mod tests {
    //! The release's order: the fence's event is closed before the submission's events, the fence after them.
    use super::*;

    #[test]
    fn released_in_the_managers_order() {
        let mut ring = CaptureRing::EMPTY;
        // SAFETY: a plain event of the test's.
        ring.fence_event = unsafe { windows_sys::Win32::System::Threading::CreateEventW(core::ptr::null(), 0, 0, core::ptr::null()) };
        ring.fence_value = 9;
        ring.allocator_index = 2;
        assert!(!ring.fence_event.is_null());
        let event = ring.fence_event;
        let mut between = 0;
        ring.release(|| {
            let mut flags = 0u32;
            // SAFETY: a query of a handle value; a closed one fails.
            let open = unsafe { windows_sys::Win32::Foundation::GetHandleInformation(event, &mut flags) } != 0;
            assert!(!open, "the fence's event closed first");
            between += 1;
        });
        assert_eq!(between, 1);
        assert!(ring.fence_event.is_null() && ring.fence.is_null() && ring.lists.iter().all(|l| l.is_null()));
        assert_eq!((ring.fence_value, ring.allocator_index), (9, 2));
    }
}
