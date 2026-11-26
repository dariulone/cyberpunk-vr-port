//! The target access query's checks (src/Render/NativeTargetAccess.cpp, `Query` and `ReportCheck`): the native
//! frontier (the boundary on this queue, with a batch, an invocation and a graph epoch, its two source indices adjacent
//! and inside the submission, the after-index right after the later one); each side's own window then compared with
//! the supplied final states (the first differing plane rejects with State at the row count, naming its target); the
//! cross window watched from the earlier side's supplied final states and merged into the earlier side's check (its
//! counts added, phase 1, a failure copied); and the exported check row.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_taq_frontier`, `cvr_taq_final_states`, `cvr_taq_seed`,
//! `cvr_taq_merge` and `cvr_taq_row` (target_access.rs's mirrors; a source's final states read at the target stride);
//! the unit tests below cover each.

use crate::target_access::{failure, Boundary, Check, Watch};

/// The Query's frontier check.
pub fn frontier(b: &Boundary, queue: u64) -> bool {
    let (low, high) = (b.source_indices[0].min(b.source_indices[1]), b.source_indices[0].max(b.source_indices[1]));
    queue != 0
        && b.queue == queue
        && b.batch != 0
        && b.invocation != 0
        && b.graph_epoch != 0
        && high < b.count
        && low.wrapping_add(1) == high
        && b.after_index == high.wrapping_add(1)
        && b.after_index < b.count
}
/// A side's own window against the supplied final states; returns the success.
pub fn final_states(check: &mut Check, watched: &[Watch; 4], supplied: &[[u32; 2]; 4], rows: u32, success: bool) -> bool {
    let mut success = success;
    if success {
        for t in 0..4 {
            for p in 0..(watched[t].count as usize).min(2) {
                if check.final_states[t][p] != supplied[t][p] {
                    success = check.reject(failure::STATE, rows, t as u32);
                }
            }
        }
    }
    success
}
/// The cross window's watch: the earlier side's supplied final states.
pub fn seed(watched: &mut [Watch; 4], supplied: &[[u32; 2]; 4]) {
    for t in 0..4 {
        for p in 0..(watched[t].count as usize).min(2) {
            watched[t].states[p] = supplied[t][p];
        }
    }
}
/// The cross check merged into the earlier side's.
pub fn merge(earlier: &mut Check, cross: &Check, success: bool) {
    earlier.rows = earlier.rows.wrapping_add(cross.rows);
    earlier.reads = earlier.reads.wrapping_add(cross.reads);
    earlier.writes = earlier.writes.wrapping_add(cross.writes);
    earlier.transitions = earlier.transitions.wrapping_add(cross.transitions);
    earlier.aliases = earlier.aliases.wrapping_add(cross.aliases);
    earlier.shaders = earlier.shaders.wrapping_add(cross.shaders);
    earlier.excluded_shaders = earlier.excluded_shaders.wrapping_add(cross.excluded_shaders);
    earlier.proof_reads = earlier.proof_reads.wrapping_add(cross.proof_reads);
    earlier.proof_scopes = earlier.proof_scopes.wrapping_add(cross.proof_scopes);
    earlier.phase = 1;
    if !success {
        earlier.failure = cross.failure;
        earlier.failure_row = cross.failure_row;
        earlier.resource = cross.resource;
    }
}
/// ReportCheck's row.
pub fn row(side: u32, c: &Check, success: bool) -> [u64; 16] {
    [
        side.into(),
        c.phase.into(),
        c.rows,
        c.reads,
        c.writes,
        c.transitions,
        c.aliases,
        c.shaders,
        c.excluded_shaders,
        c.proof_reads,
        c.proof_scopes,
        c.failure.into(),
        c.failure_row.into(),
        c.resource.into(),
        success.into(),
        0,
    ]
}

/// A source's four targets' final states, read at the target stride from the first target's.
///
/// # Safety
/// `first` points at the first target's two states and each next target's lies `stride` bytes on.
unsafe fn supplied(first: *const u8, stride: usize) -> [[u32; 2]; 4] {
    let mut out = [[0u32; 2]; 4];
    for (t, states) in out.iter_mut().enumerate() {
        *states = unsafe { first.add(t * stride).cast::<[u32; 2]>().read_unaligned() };
    }
    out
}

// ---- the C ABI

/// # Safety
/// `boundary` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_taq_frontier(boundary: *const Boundary, queue: u64) -> bool {
    frontier(unsafe { &*boundary }, queue)
}
/// # Safety
/// `check` and `watched` (four) are valid; `first`/`stride` as for the supplied states.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_taq_final_states(check: *mut Check, watched: *const [Watch; 4], first: *const u8, stride: usize, rows: u32, success: bool) -> bool {
    unsafe { final_states(&mut *check, &*watched, &supplied(first, stride), rows, success) }
}
/// # Safety
/// `watched` holds four watches; `first`/`stride` as for the supplied states.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_taq_seed(watched: *mut [Watch; 4], first: *const u8, stride: usize) {
    unsafe { seed(&mut *watched, &supplied(first, stride)) }
}
/// # Safety
/// Both checks are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_taq_merge(earlier: *mut Check, cross: *const Check, success: bool) {
    unsafe { merge(&mut *earlier, &*cross, success) }
}
/// # Safety
/// `check` is valid; `out` holds sixteen values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_taq_row(side: u32, check: *const Check, success: bool, out: *mut [u64; 16]) {
    unsafe { *out = row(side, &*check, success) };
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    fn boundary() -> Boundary {
        Boundary { queue: 7, batch: 1, invocation: 2, graph_epoch: 3, count: 6, after_index: 4, source_indices: [3, 2], ..Default::default() }
    }
    #[test]
    fn frontiers() {
        assert!(frontier(&boundary(), 7));
        assert!(!frontier(&boundary(), 0));
        assert!(!frontier(&boundary(), 8));
        let changes: [fn(&mut Boundary); 8] = [
            |b| b.batch = 0,
            |b| b.invocation = 0,
            |b| b.graph_epoch = 0,
            |b| b.source_indices = [1, 3],
            |b| b.after_index = 5,
            |b| b.count = 4,
            |b| b.count = 3,
            |b| b.source_indices = [3, 3],
        ];
        for change in changes {
            let mut b = boundary();
            change(&mut b);
            assert!(!frontier(&b, 7), "{b:?}");
        }
        // The after-index must lie inside the submission.
        let mut b = boundary();
        b.count = 5;
        assert!(frontier(&b, 7));
    }
    fn watches(counts: [u32; 4]) -> [Watch; 4] {
        counts.map(|count| Watch { count, ..Default::default() })
    }
    #[test]
    fn finals() {
        let supplied = [[1, 2], [3, 4], [5, 6], [7, 8]];
        let watched = watches([2, 1, 0, 2]);
        let mut check = Check { final_states: [[1, 2], [3, 9], [0, 0], [7, 8]], ..Default::default() };
        assert!(final_states(&mut check, &watched, &supplied, 40, true));
        // A plane past the target's count does not count; one inside does, and the first failure is kept.
        let mut check = Check { final_states: [[1, 2], [9, 4], [0, 0], [0, 8]], ..Default::default() };
        assert!(!final_states(&mut check, &watched, &supplied, 40, true));
        assert_eq!((check.failure, check.failure_row, check.resource), (failure::STATE, 40, 1));
        // An earlier failure is not re-checked.
        let mut check = Check::default();
        assert!(!final_states(&mut check, &watched, &supplied, 40, false));
        assert_eq!(check.failure, failure::NONE);
    }
    #[test]
    fn seeds_and_merges() {
        let mut watched = watches([1, 2, 0, 2]);
        seed(&mut watched, &[[1, 2], [3, 4], [5, 6], [7, 8]]);
        assert_eq!(watched.map(|w| w.states), [[1, 0], [3, 4], [0, 0], [7, 8]]);
        let mut earlier = Check { rows: 1, reads: 2, writes: 3, transitions: 4, aliases: 5, shaders: 6, excluded_shaders: 7, proof_reads: 8, proof_scopes: 9, ..Default::default() };
        let cross = Check { rows: 10, reads: 20, writes: 30, transitions: 40, aliases: 50, shaders: 60, excluded_shaders: 70, proof_reads: 80, proof_scopes: 90, failure: 13, failure_row: 5, resource: 2, ..Default::default() };
        let mut ok = earlier;
        merge(&mut ok, &cross, true);
        assert_eq!((ok.rows, ok.reads, ok.writes, ok.transitions, ok.aliases, ok.shaders, ok.excluded_shaders, ok.proof_reads, ok.proof_scopes), (11, 22, 33, 44, 55, 66, 77, 88, 99));
        assert_eq!((ok.phase, ok.failure, ok.failure_row, ok.resource), (1, failure::NONE, u32::MAX, u32::MAX));
        merge(&mut earlier, &cross, false);
        assert_eq!((earlier.failure, earlier.failure_row, earlier.resource), (13, 5, 2));
    }
    #[test]
    fn rows() {
        let c = Check { rows: 1, reads: 2, writes: 3, transitions: 4, aliases: 5, shaders: 6, excluded_shaders: 7, proof_reads: 8, proof_scopes: 9, failure: 10, failure_row: 11, resource: 12, phase: 1, ..Default::default() };
        assert_eq!(row(1, &c, true), [1, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 1, 0]);
        assert_eq!(row(0, &c, false)[14], 0);
    }
    #[test]
    fn strided() {
        let data: [u32; 12] = [1, 2, 0, 3, 4, 0, 5, 6, 0, 7, 8, 0];
        assert_eq!(unsafe { supplied(data.as_ptr().cast(), 12) }, [[1, 2], [3, 4], [5, 6], [7, 8]]);
    }
}
