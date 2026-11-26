//! THE IMAGE SUBMISSION LOCK (cvr::camera::ImageSubmissionMutex, formerly a C++ std::recursive_mutex in
//! src/Camera/ImagePoseIdentity.cpp): held around a queue submission and the image labels it publishes, so a label is
//! selected at the actual submission, never while another producer's submission is half published. Recursive: the inner
//! submission hook takes it again under the XR submit and the capture. A Win32 critical section, which is recursive and
//! is taken and released by separate calls, as the C++ half's lock_guard and its explicit lock()/unlock() do.
use core::cell::UnsafeCell;
use std::sync::OnceLock;

use windows_sys::Win32::System::Threading::{
    EnterCriticalSection, InitializeCriticalSection, LeaveCriticalSection, CRITICAL_SECTION,
};

struct Section(Box<UnsafeCell<CRITICAL_SECTION>>);
// SAFETY: a critical section is made to be shared between threads; it never moves once initialised (boxed).
unsafe impl Send for Section {}
// SAFETY: as above.
unsafe impl Sync for Section {}

static SECTION: OnceLock<Section> = OnceLock::new();

fn section() -> *mut CRITICAL_SECTION {
    SECTION
        .get_or_init(|| {
            // SAFETY: zero is the critical section's storage before its initialisation.
            let cell = Box::new(UnsafeCell::new(unsafe { core::mem::zeroed::<CRITICAL_SECTION>() }));
            // SAFETY: initialised once, in place, before any use.
            unsafe { InitializeCriticalSection(cell.get()) };
            Section(cell)
        })
        .0
        .get()
}

/// The lock taken (recursively).
#[no_mangle]
pub extern "C" fn cvr_image_submission_lock() {
    // SAFETY: the initialised section.
    unsafe { EnterCriticalSection(section()) }
}
/// The lock released once (by the thread holding it).
#[no_mangle]
pub extern "C" fn cvr_image_submission_unlock() {
    // SAFETY: the initialised section, held by this thread.
    unsafe { LeaveCriticalSection(section()) }
}

/// The lock held until dropped.
pub struct Guard(());
impl Drop for Guard {
    fn drop(&mut self) {
        cvr_image_submission_unlock();
    }
}
/// The lock taken; released when the guard drops.
pub fn lock() -> Guard {
    cvr_image_submission_lock();
    Guard(())
}
