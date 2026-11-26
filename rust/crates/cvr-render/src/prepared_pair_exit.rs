//! The native prepared pair's exit ownership (src/Render/NativePreparedPair.cpp, `prepared_pair::NativeExitMatchesSource`):
//! a native exit capture belongs to a source when it has the work's size, the asked eye with the other source side,
//! the work's source frame (the source's original and owned draws' frame too, on that side), and the source recording's
//! list (the draws' list too) and generation.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the check forwards to `cvr_npp_exit_matches`; the unit test below covers each
//! field.

/// The exit's fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Exit {
    pub width: u32,
    pub height: u32,
    pub eye: u32,
    pub source_side: u32,
    pub frame: u32,
    pub list: u64,
    pub generation: u64,
}
/// The source's fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Owner {
    pub width: u32,
    pub height: u32,
    pub source_frame: u32,
    pub original_frame: u32,
    pub owned_frame: u32,
    pub original_side: u32,
    pub owned_side: u32,
    pub recording_list: u64,
    pub original_list: u64,
    pub owned_list: u64,
    pub recording_generation: u64,
}

const _: () = assert!(core::mem::size_of::<Exit>() == 40 && core::mem::size_of::<Owner>() == 64);

/// NativeExitMatchesSource.
pub fn exit_matches(e: &Exit, o: &Owner, eye: u32) -> bool {
    e.width == o.width
        && e.height == o.height
        && e.eye == eye
        && e.source_side == 1u32.wrapping_sub(eye)
        && e.frame == o.source_frame
        && e.frame == o.original_frame
        && e.frame == o.owned_frame
        && o.original_side == e.source_side
        && o.owned_side == e.source_side
        && e.list != 0
        && e.list == o.recording_list
        && e.list == o.original_list
        && e.list == o.owned_list
        && e.generation != 0
        && e.generation == o.recording_generation
}

// ---- the C ABI

/// # Safety
/// Both records are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_exit_matches(exit: *const Exit, owner: *const Owner, eye: u32) -> bool {
    exit_matches(unsafe { &*exit }, unsafe { &*owner }, eye)
}

#[cfg(test)]
mod tests {
    //! Each exit field.
    use super::*;

    #[test]
    fn ownership() {
        let e = Exit { width: 8, height: 4, eye: 0, source_side: 1, frame: 5, list: 0x10, generation: 2 };
        let o = Owner { width: 8, height: 4, source_frame: 5, original_frame: 5, owned_frame: 5, original_side: 1, owned_side: 1, recording_list: 0x10, original_list: 0x10, owned_list: 0x10, recording_generation: 2 };
        assert!(exit_matches(&e, &o, 0));
        assert!(!exit_matches(&e, &o, 1));
        assert!(!exit_matches(&Exit { source_side: 0, ..e }, &o, 0));
        assert!(!exit_matches(&e, &Owner { owned_frame: 6, ..o }, 0));
        assert!(!exit_matches(&e, &Owner { owned_side: 0, ..o }, 0));
        assert!(!exit_matches(&e, &Owner { original_list: 0x11, ..o }, 0));
        assert!(!exit_matches(&Exit { generation: 0, ..e }, &Owner { recording_generation: 0, ..o }, 0));
        assert!(!exit_matches(&Exit { list: 0, ..e }, &Owner { recording_list: 0, original_list: 0, owned_list: 0, ..o }, 0));
        assert!(!exit_matches(&e, &Owner { width: 9, ..o }, 0));
    }
}
