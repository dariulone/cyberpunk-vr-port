//! THE NATIVE FINE VISIBILITY TEST'S CALL (include/Render/NativeFineVisibility.hpp): the candidate count of a native
//! cull candidate range (40-byte records, at most 65536), and the call of the engine's fine test over owned candidates
//! with a preallocated output -- the native test appends at most one id per candidate, so the whole output is
//! allocated up front and the native container never grows or owns it; the returned container must still be ours, of
//! the same capacity, with a count within it. The header forwards to these in the Rust build;
//! tools/single_pass_tests/cpu_collector_test runs its cases against them.
use core::ffi::c_void;

pub const MAX_FINE_CANDIDATES: usize = 65536;

/// NativeCullCandidate.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct NativeCullCandidate {
    pub id: u64,
    pub bounds: [f32; 6],
    pub include_mask: u16,
    pub exclude_mask: u16,
    pub reserved: u32,
}
/// NativeVisibleIds.
#[repr(C)]
pub struct NativeVisibleIds {
    pub data: *mut u64,
    pub capacity: u32,
    pub count: u32,
}
const _: () = assert!(
    core::mem::size_of::<NativeCullCandidate>() == 40
        && core::mem::size_of::<NativeVisibleIds>() == 16
);
/// NativeFineCull: (query, candidate range [begin, end), partial, output) -> char.
pub type NativeFineCull = unsafe extern "C-unwind" fn(*mut c_void, *mut i64, i8, *mut c_void) -> i8;

/// FineCandidateCount: the range's candidates (0 when not a whole range), bounded.
pub fn fine_candidate_count(begin: usize, end: usize) -> (bool, usize) {
    let size = core::mem::size_of::<NativeCullCandidate>();
    if begin == 0 || end < begin || (end - begin) % size != 0 {
        return (false, 0);
    }
    let count = (end - begin) / size;
    (count <= MAX_FINE_CANDIDATES, count)
}

/// RunNativeFine into `ids` (one slot per candidate): the visible count, or None.
///
/// # Safety
/// `fine` is the engine's fine test for `query`; `ids` holds `candidates.len()` slots.
pub unsafe fn run_native_fine(
    fine: Option<NativeFineCull>,
    query: *mut c_void,
    candidates: &[NativeCullCandidate],
    partial: i8,
    ids: &mut [u64],
) -> Option<u32> {
    let fine = fine?;
    if query.is_null() || candidates.len() > MAX_FINE_CANDIDATES {
        return None;
    }
    if candidates.is_empty() {
        return Some(0);
    }
    if ids.len() != candidates.len() {
        return None;
    }
    let mut output = NativeVisibleIds {
        data: ids.as_mut_ptr(),
        capacity: ids.len() as u32,
        count: 0,
    };
    let begin = candidates.as_ptr() as usize;
    let mut range = [
        begin as i64,
        (begin + core::mem::size_of_val(candidates)) as i64,
    ];
    unsafe {
        fine(
            query,
            range.as_mut_ptr(),
            partial,
            &mut output as *mut NativeVisibleIds as *mut c_void,
        )
    };
    if output.data != ids.as_mut_ptr()
        || output.capacity as usize != ids.len()
        || output.count > output.capacity
    {
        return None;
    }
    Some(output.count)
}

/// FineCandidateCount.
///
/// # Safety
/// `count` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fv_count(begin: usize, end: usize, count: *mut usize) -> bool {
    let (bounded, n) = fine_candidate_count(begin, end);
    unsafe { *count = n };
    bounded
}

/// RunNativeFine: the visible count into `count`.
///
/// # Safety
/// `candidates` holds `candidate_count`; `ids` holds `id_count`; `count` is writable; `fine` is the engine's test.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_fv_run(
    fine: Option<NativeFineCull>,
    query: *mut c_void,
    candidates: *const NativeCullCandidate,
    candidate_count: usize,
    partial: i8,
    ids: *mut u64,
    id_count: usize,
    count: *mut u32,
) -> bool {
    unsafe {
        let candidates = if candidate_count == 0 || candidates.is_null() {
            &[][..]
        } else {
            core::slice::from_raw_parts(candidates, candidate_count)
        };
        let ids = if id_count == 0 || ids.is_null() {
            &mut [][..]
        } else {
            core::slice::from_raw_parts_mut(ids, id_count)
        };
        match run_native_fine(fine, query, candidates, partial, ids) {
            Some(n) => {
                *count = n;
                true
            }
            None => false,
        }
    }
}
