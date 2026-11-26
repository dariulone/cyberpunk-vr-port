//! The submission's state, owned here (formerly OpenXRManager's members): whether the mono frame is submitted, the
//! runtime's predicted display period (the frame loop stores it, the present buckets against it), and the game's D3D12
//! device and queue the session was made on (references the manager took at InitGraphics and releases at shutdown), and
//! the two events the submission paces on: the mono frame's (the capture sets it when a frame is published, the frame
//! loop's old pacing waits on it) and the frame sync's (set when a cycle ends). The manager makes both at InitGraphics
//! and closes them at shutdown. include/Runtimes/OpenXRManager.hpp's members are references to them; the present
//! (present.rs), the frame loop and the capture read them here.
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicBool, AtomicI64};

#[repr(C)]
pub struct SubmitShared {
    pub mono_submit_enabled: AtomicBool,
    pub predicted_display_period_ns: AtomicI64,
    /// ID3D12Device* (plain, as the C++ half reads it).
    pub device: UnsafeCell<*mut c_void>,
    /// ID3D12CommandQueue*.
    pub queue: UnsafeCell<*mut c_void>,
    /// m_monoPresentEvent, m_frameSyncEvent (HANDLE; null before InitGraphics).
    pub mono_present_event: UnsafeCell<*mut c_void>,
    pub frame_sync_event: UnsafeCell<*mut c_void>,
}
// SAFETY: the flag and period are atomic; the device, queue and events are set once at InitGraphics and cleared at
// shutdown, as the C++ members were.
unsafe impl Sync for SubmitShared {}
const _: () = {
    assert!(core::mem::size_of::<SubmitShared>() == 48, "mirrored by include/Runtimes/OpenXRManager.hpp");
    assert!(core::mem::offset_of!(SubmitShared, mono_present_event) == 32);
    assert!(core::mem::offset_of!(SubmitShared, predicted_display_period_ns) == 8);
};

pub static SUBMIT: SubmitShared = SubmitShared {
    mono_submit_enabled: AtomicBool::new(false),
    predicted_display_period_ns: AtomicI64::new(0),
    device: UnsafeCell::new(core::ptr::null_mut()),
    queue: UnsafeCell::new(core::ptr::null_mut()),
    mono_present_event: UnsafeCell::new(core::ptr::null_mut()),
    frame_sync_event: UnsafeCell::new(core::ptr::null_mut()),
};
impl SubmitShared {
    /// InitGraphics: the game's device and queue kept for the session (a reference taken on each), then the mono
    /// frame's event made when there is none -- false, logged, when it cannot be -- and the frame sync's (unchecked, as
    /// the C++ half had it).
    ///
    /// # Safety
    /// `device` and `queue` are null or live COM objects; the manager's InitGraphics thread.
    pub unsafe fn take(&self, device: *mut c_void, queue: *mut c_void) -> bool {
        use windows_sys::Win32::System::Threading::CreateEventA;
        use windows::core::Interface as _;
        // SAFETY (whole body): the caller's objects, a reference taken on each; plain Win32 events.
        unsafe {
            *self.device.get() = device;
            *self.queue.get() = queue;
            for object in [device, queue] {
                if let Some(unknown) = windows::core::IUnknown::from_raw_borrowed(&object) {
                    core::mem::forget(unknown.clone());
                }
            }
            if (*self.mono_present_event.get()).is_null() {
                let event = CreateEventA(core::ptr::null(), 0, 0, core::ptr::null());
                *self.mono_present_event.get() = event;
                if event.is_null() {
                    cvr_host::log::write("OpenXRManager: Failed to create mono present event\n");
                    return false;
                }
            }
            if (*self.frame_sync_event.get()).is_null() {
                *self.frame_sync_event.get() = CreateEventA(core::ptr::null(), 0, 0, core::ptr::null());
            }
        }
        true
    }
    /// Shutdown: the mono frame's event closed, then the frame sync's.
    pub fn close_events(&self) {
        for event in [&self.mono_present_event, &self.frame_sync_event] {
            // SAFETY: an event of ours, closed once.
            unsafe {
                if !(*event.get()).is_null() {
                    windows_sys::Win32::Foundation::CloseHandle(*event.get());
                    *event.get() = core::ptr::null_mut();
                }
            }
        }
    }
    /// Shutdown, last: the queue's reference released, then the device's.
    ///
    /// # Safety
    /// The references are the ones `take` took; nothing uses them after.
    pub unsafe fn release_device(&self) {
        use windows::core::Interface as _;
        for object in [&self.queue, &self.device] {
            // SAFETY: the reference taken at InitGraphics, released once.
            unsafe {
                let raw = *object.get();
                if !raw.is_null() {
                    drop(windows::core::IUnknown::from_raw(raw));
                    *object.get() = core::ptr::null_mut();
                }
            }
        }
    }
    /// The mono frame's event, as the frame loop and the capture read it.
    pub fn mono_present_event(&self) -> *mut c_void {
        // SAFETY: a plain pointer the manager sets at InitGraphics.
        unsafe { core::ptr::read_volatile(self.mono_present_event.get()) }
    }
    /// The frame sync's event.
    pub fn frame_sync_event(&self) -> *mut c_void {
        // SAFETY: as above.
        unsafe { core::ptr::read_volatile(self.frame_sync_event.get()) }
    }
}
#[no_mangle]
pub extern "C" fn cvr_xr_submit_shared() -> *const SubmitShared {
    &SUBMIT
}
/// OpenXRManager::InitGraphics' device, queue and events (SubmitShared::take).
///
/// # Safety
/// As `take`.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_submit_take(device: *mut c_void, queue: *mut c_void) -> bool {
    // SAFETY: as the caller promises.
    unsafe { SUBMIT.take(device, queue) }
}
/// Shutdown's events (SubmitShared::close_events).
#[no_mangle]
pub extern "C" fn cvr_xr_submit_close_events() {
    SUBMIT.close_events();
}
/// Shutdown's device and queue (SubmitShared::release_device).
///
/// # Safety
/// As `release_device`.
#[no_mangle]
pub unsafe extern "C" fn cvr_xr_submit_release_device() {
    // SAFETY: as the caller promises.
    unsafe { SUBMIT.release_device() }
}

#[cfg(test)]
mod tests {
    //! The defaults the C++ members had.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    fn blank() -> SubmitShared {
        SubmitShared {
            mono_submit_enabled: AtomicBool::new(false),
            predicted_display_period_ns: AtomicI64::new(0),
            device: UnsafeCell::new(core::ptr::null_mut()),
            queue: UnsafeCell::new(core::ptr::null_mut()),
            mono_present_event: UnsafeCell::new(core::ptr::null_mut()),
            frame_sync_event: UnsafeCell::new(core::ptr::null_mut()),
        }
    }

    #[test]
    fn the_events_made_once_and_closed() {
        let s = blank();
        // SAFETY: no device or queue; plain events.
        assert!(unsafe { s.take(core::ptr::null_mut(), core::ptr::null_mut()) });
        let (mono, sync) = (s.mono_present_event(), s.frame_sync_event());
        assert!(!mono.is_null() && !sync.is_null() && mono != sync);
        // A second InitGraphics keeps the events it has.
        assert!(unsafe { s.take(core::ptr::null_mut(), core::ptr::null_mut()) });
        assert_eq!((s.mono_present_event(), s.frame_sync_event()), (mono, sync));
        s.close_events();
        assert!(s.mono_present_event().is_null() && s.frame_sync_event().is_null());
        // SAFETY: nothing was referenced.
        unsafe { s.release_device() };
    }

    #[test]
    fn defaults() {
        assert!(!SUBMIT.mono_submit_enabled.load(Relaxed));
        assert_eq!(SUBMIT.predicted_display_period_ns.load(Relaxed), 0);
        // SAFETY: read once, nothing sets them in the tests.
        unsafe { assert!((*SUBMIT.device.get()).is_null() && (*SUBMIT.queue.get()).is_null()) };
        assert_eq!(core::mem::offset_of!(SubmitShared, device), 16);
        assert!(SUBMIT.mono_present_event().is_null() && SUBMIT.frame_sync_event().is_null());
        assert_eq!(core::mem::offset_of!(SubmitShared, frame_sync_event), 40);
    }
}
