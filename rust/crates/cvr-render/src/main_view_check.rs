//! Which engine view is MAIN (src/Stereo/SyncStereo.cpp, `is_main_view`): a view carrying a camera name key (+0x28,
//! VRCAM's) is never MAIN -- whatever the cache says; without this, one bad pin silently disables every VRCAM branch
//! that sits in an `else if` after an is_main_view() test. Otherwise the view the cache pinned as MAIN's context is
//! MAIN; with nothing pinned, a view whose aspect (+0x98) is over 1.3 is (this stays right when MAIN goes square only
//! once the cache is set).
//!
//! The C++ half's `is_main_view` forwards to `cvr_ss_is_main_view` with the cache's value; the view is read directly,
//! as the .cpp read it. The unit tests below use a fabricated view.

/// is_main_view.
///
/// # Safety
/// `view` is null or a live engine view (0x9C bytes readable).
pub unsafe fn is_main_view(view: *const u8, cached: usize) -> bool {
    if view.is_null() {
        return false;
    }
    // SAFETY: as the caller vouches.
    if unsafe { core::ptr::read_unaligned(view.add(0x28) as *const u64) } != 0 {
        return false;
    }
    if cached != 0 {
        return view as usize == cached;
    }
    // SAFETY: as above.
    unsafe { core::ptr::read_unaligned(view.add(0x98) as *const f32) > 1.3 }
}

/// # Safety
/// As `is_main_view`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ss_is_main_view(view: *const u8, cached: usize) -> bool {
    unsafe { is_main_view(view, cached) }
}

#[cfg(test)]
mod tests {
    //! A fabricated view.
    use super::*;

    #[test]
    fn views() {
        let mut view = [0u8; 0xA0];
        view[0x98..0x9C].copy_from_slice(&1.77f32.to_le_bytes());
        let p = view.as_ptr();
        unsafe {
            // Nothing pinned: the wide view is MAIN, a square one is not.
            assert!(is_main_view(p, 0));
            let mut square = view;
            square[0x98..0x9C].copy_from_slice(&1.0f32.to_le_bytes());
            assert!(!is_main_view(square.as_ptr(), 0));
            // Pinned: only that view, whatever its aspect.
            assert!(is_main_view(p, p as usize));
            assert!(!is_main_view(p, p as usize + 16));
            // A named view never, whatever the pin.
            let mut named = view;
            named[0x28] = 1;
            assert!(!is_main_view(named.as_ptr(), named.as_ptr() as usize));
            assert!(!is_main_view(core::ptr::null(), 0));
        }
    }
}
