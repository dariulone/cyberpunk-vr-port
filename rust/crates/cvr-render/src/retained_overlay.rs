//! The retained geometry's t5 overlay B4 association (src/Render/NativeRetainedGeometry.cpp, `OverlayB4`): a source's
//! b4 selector input row is its own (source and side) with owned bytes equal to their copy and the copy's snapshot row
//! recorded (14, inside the snapshot rows); and that CBV snapshot row is the source's private b4 copy -- the same source
//! and side, pixel register 4 on parameter 19 at offset 0, status 1, a recording and a payload in the blob, the 228 b4
//! bytes, the private copy's resource, GPU address, private offset, copied and padded bytes and owner, a held resource
//! and owner reflecting 228 bytes, and the source's copies prepared on its row's list and generation at its
//! occurrence.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_b4_input` and `cvr_nrg_b4_row` (the private copy's and
//! the prepared copies' fields as the .cpp hands them); the unit tests below cover each field.

/// multilayer_selectors::B4Bytes.
pub const B4_BYTES: u64 = 228;

/// The private b4 copy's and the prepared copies' fields.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct B4Copy {
    pub resource: u64,
    pub owner: u64,
    pub gpu: u64,
    pub private_offset: u64,
    pub copied: u64,
    pub padded: u64,
    pub reflected: u64,
    pub prepared_owner: u64,
    pub prepared_list: u64,
    pub prepared_generation: u64,
    pub prepared_occurrence: u64,
}
const _: () = assert!(core::mem::size_of::<B4Copy>() == 88);

/// The selector input row's checks.
pub fn b4_input(matched: &[u64; 24], index: u64, side: u64, snapshots: u64) -> bool {
    matched[0] == index && matched[1] == side && matched[13] == 1 && matched[14] != 0 && matched[14] <= snapshots
}
/// The snapshot row's checks; `saved` the source row's list (6) and generation (7).
pub fn b4_row(row: &[u64; 32], index: u64, side: u64, c: &B4Copy, list: u64, generation: u64, blob: u64) -> bool {
    row[0] == index
        && row[1] == side
        && row[2] == 5
        && row[3] == 4
        && row[4] == 19
        && row[5] == 0
        && row[8] == 1
        && row[29] != 0
        && row[30] != 0
        && row[31] != 0
        && row[20] == B4_BYTES
        && row[17] == c.resource
        && row[18] == c.gpu
        && row[19] == c.private_offset
        && row[20] == c.copied
        && row[21] == c.padded
        && row[22] == c.owner
        && c.resource != 0
        && c.owner != 0
        && c.reflected == B4_BYTES
        && c.prepared_owner != 0
        && c.prepared_list == list
        && c.prepared_generation == generation
        && c.prepared_occurrence == index.wrapping_add(1)
        && row[31] - 1 <= blob
        && row[20] <= blob - (row[31] - 1)
}

// ---- the C ABI

/// # Safety
/// `matched` is a selector input row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_b4_input(matched: *const [u64; 24], index: u64, side: u64, snapshots: u64) -> bool {
    b4_input(unsafe { &*matched }, index, side, snapshots)
}
/// # Safety
/// `row` is a CBV snapshot row; `copy` the mirrored fields.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_b4_row(row: *const [u64; 32], index: u64, side: u64, copy: *const B4Copy, list: u64, generation: u64, blob: u64) -> bool {
    b4_row(unsafe { &*row }, index, side, unsafe { &*copy }, list, generation, blob)
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;

    fn copy() -> B4Copy {
        B4Copy {
            resource: 1,
            owner: 2,
            gpu: 3,
            private_offset: 4,
            copied: 228,
            padded: 256,
            reflected: 228,
            prepared_owner: 5,
            prepared_list: 6,
            prepared_generation: 7,
            prepared_occurrence: 9,
        }
    }
    fn row() -> [u64; 32] {
        let mut r = [0u64; 32];
        r[..6].copy_from_slice(&[8, 1, 5, 4, 19, 0]);
        r[8] = 1;
        r[17..23].copy_from_slice(&[1, 3, 4, 228, 256, 2]);
        r[29] = 1;
        r[30] = 1;
        r[31] = 11;
        r
    }
    #[test]
    fn inputs() {
        let mut m = [0u64; 24];
        m[0] = 8;
        m[1] = 1;
        m[13] = 1;
        m[14] = 3;
        assert!(b4_input(&m, 8, 1, 3));
        assert!(!b4_input(&m, 8, 1, 2));
        assert!(!b4_input(&m, 7, 1, 3));
        assert!(!b4_input(&m, 8, 0, 3));
        let mut n = m;
        n[13] = 2;
        assert!(!b4_input(&n, 8, 1, 3));
        let mut n = m;
        n[14] = 0;
        assert!(!b4_input(&n, 8, 1, 3));
    }
    #[test]
    fn rows() {
        let run = |r: [u64; 32], c: B4Copy, blob: u64| b4_row(&r, 8, 1, &c, 6, 7, blob);
        assert!(run(row(), copy(), 238));
        assert!(!run(row(), copy(), 237));
        for (at, value) in [(0, 9), (1, 0), (2, 1), (3, 5), (4, 18), (5, 1), (8, 2), (29, 0), (30, 0), (31, 0), (17, 9), (18, 9), (19, 9), (21, 9), (22, 9)] {
            let mut r = row();
            r[at] = value;
            assert!(!run(r, copy(), 1 << 20), "{at}");
        }
        let mut r = row();
        r[20] = 229;
        assert!(!run(r, B4Copy { copied: 229, ..copy() }, 1 << 20));
        let changes: [fn(&mut B4Copy); 8] = [
            |c| c.resource = 0,
            |c| c.owner = 0,
            |c| c.reflected = 227,
            |c| c.prepared_owner = 0,
            |c| c.prepared_list = 0,
            |c| c.prepared_generation = 0,
            |c| c.prepared_occurrence = 8,
            |c| c.copied = 227,
        ];
        for (i, change) in changes.into_iter().enumerate() {
            let mut c = copy();
            change(&mut c);
            assert!(!run(row(), c, 1 << 20), "{i}");
        }
        // resource 0 with a row naming it: the held resource is still required.
        let mut r = row();
        r[17] = 0;
        assert!(!run(r, B4Copy { resource: 0, ..copy() }, 1 << 20));
    }
}
