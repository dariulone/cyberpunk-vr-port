//! The presented frame, owned here (formerly OpenXRManager's members): the present lock (an SRW lock both halves take;
//! the C++ half's cvr::SrwMutex), the frame last held, the presented frame's size, format, buffer index and serial, and
//! the present counter. include/Runtimes/OpenXRManager.hpp's members are references to them; the present (present.rs)
//! records the frame here.
use crate::srw::SrwLock;
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::sync::atomic::AtomicU64;

#[repr(C)]
pub struct PresentedShared {
    pub lock: SrwLock,
    /// The frame last held (an ID3D12Resource*, released when the next is recorded and at shutdown).
    pub back_buffer: UnsafeCell<*mut c_void>,
    pub width: UnsafeCell<u32>,
    pub height: UnsafeCell<u32>,
    pub format: UnsafeCell<u32>,
    pub buffer_index: UnsafeCell<u32>,
    pub serial: UnsafeCell<u64>,
    pub present_count: AtomicU64,
}
// SAFETY: the cells are written and read under the lock (the counter is atomic), by both halves.
unsafe impl Sync for PresentedShared {}
const _: () = {
    assert!(core::mem::size_of::<PresentedShared>() == 48, "mirrored by include/Runtimes/OpenXRManager.hpp");
    assert!(core::mem::offset_of!(PresentedShared, present_count) == 40);
};

pub static PRESENTED: PresentedShared = PresentedShared {
    lock: SrwLock::new(),
    back_buffer: UnsafeCell::new(core::ptr::null_mut()),
    width: UnsafeCell::new(0),
    height: UnsafeCell::new(0),
    format: UnsafeCell::new(0),
    buffer_index: UnsafeCell::new(0),
    serial: UnsafeCell::new(0),
    present_count: AtomicU64::new(0),
};
#[no_mangle]
pub extern "C" fn cvr_xr_presented_shared() -> *const PresentedShared {
    &PRESENTED
}
impl PresentedShared {
    /// Shutdown: the frame last held released (under the present lock), its size, format and serial kept.
    pub fn release_held(&self) {
        use windows::core::Interface as _;
        let _held = self.lock.lock();
        // SAFETY: under the lock; the reference the present took, released once.
        unsafe {
            let last = *self.back_buffer.get();
            if !last.is_null() {
                drop(windows::core::IUnknown::from_raw(last));
                *self.back_buffer.get() = core::ptr::null_mut();
            }
        }
    }
}
/// OpenXRManager::Shutdown's release of the frame last held.
#[no_mangle]
pub extern "C" fn cvr_xr_presented_release() {
    PRESENTED.release_held();
}

#[cfg(test)]
mod tests {
    //! The layout and the empty start.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn starts_empty() {
        let _held = PRESENTED.lock.lock();
        // SAFETY: under the lock.
        unsafe {
            assert!((*PRESENTED.back_buffer.get()).is_null());
            assert_eq!((*PRESENTED.width.get(), *PRESENTED.height.get(), *PRESENTED.serial.get()), (0, 0, 0));
        }
        assert_eq!(PRESENTED.present_count.load(Relaxed), 0);
        assert_eq!(core::mem::offset_of!(PresentedShared, width), 16);
    }

    #[test]
    fn nothing_held_nothing_released() {
        let p = PresentedShared {
            lock: SrwLock::new(),
            back_buffer: UnsafeCell::new(core::ptr::null_mut()),
            width: UnsafeCell::new(1864),
            height: UnsafeCell::new(2016),
            format: UnsafeCell::new(28),
            buffer_index: UnsafeCell::new(2),
            serial: UnsafeCell::new(7),
            present_count: AtomicU64::new(7),
        };
        p.release_held();
        let _held = p.lock.lock();
        // SAFETY: under the lock.
        unsafe {
            assert!((*p.back_buffer.get()).is_null());
            assert_eq!((*p.width.get(), *p.height.get(), *p.format.get(), *p.serial.get()), (1864, 2016, 28, 7));
        }
    }
}
