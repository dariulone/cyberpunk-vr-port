//! The retained geometry's native instance copies (src/Render/NativeRetainedGeometry.cpp, `PrepareInstanceOverlay`): a
//! compared instance copy row (24 words) carries its original submission evidence -- recorded, readable and observed
//! (2, 3, 4), checked (21), its eye's diagnostic readback mapped, its range (10, 11) inside that readback and its
//! original GPU payload (18, plus one) inside the GPU blob -- and the private copy equals the original GPU bytes (the
//! view's copy size and every byte). `BuildPrivateGeometry`'s view identity reuses retained_staged's
//! `cvr_nrg_instance_view`.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nrg_instance_evidence` and `cvr_nrg_instance_payload`; the
//! unit tests below cover each field.

/// The copy row's evidence.
pub fn instance_evidence(row: &[u64; 24], mapped: bool, readback: u64, blob: u64) -> bool {
    row[2] != 0
        && row[3] != 0
        && row[4] != 0
        && row[21] != 0
        && mapped
        && row[10] <= readback
        && row[11] <= readback - row[10]
        && row[18] != 0
        && row[18] - 1 <= blob
        && row[11] <= blob - (row[18] - 1)
}
/// The private copy against the original GPU bytes; `private` the copy's bytes (row[11] of them).
pub fn instance_payload(copy_bytes: u64, row: &[u64; 24], private: &[u8], blob: &[u8]) -> bool {
    let n = row[11] as usize;
    let start = row[18].wrapping_sub(1) as usize;
    copy_bytes == row[11] && private.len() >= n && blob.len() >= start && blob.len() - start >= n && private[..n] == blob[start..start + n]
}

// ---- the C ABI

/// # Safety
/// `row` is an instance copy row.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_evidence(row: *const [u64; 24], mapped: bool, readback: u64, blob: u64) -> bool {
    instance_evidence(unsafe { &*row }, mapped, readback, blob)
}
/// # Safety
/// `row` is an instance copy row whose evidence held: `private` holds row[11] bytes and `blob` `blob_bytes`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_payload(copy_bytes: u64, row: *const [u64; 24], private: *const u8, blob: *const u8, blob_bytes: usize) -> bool {
    let row = unsafe { &*row };
    let bytes = |p: *const u8, n: usize| if p.is_null() || n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(p, n) } };
    instance_payload(copy_bytes, row, bytes(private, row[11] as usize), bytes(blob, blob_bytes))
}

#[cfg(test)]
mod tests {
    //! Each field.
    use super::*;

    fn row() -> [u64; 24] {
        let mut r = [0u64; 24];
        r[2] = 1;
        r[3] = 1;
        r[4] = 1;
        r[21] = 1;
        r[10] = 16;
        r[11] = 8;
        r[18] = 5;
        r
    }
    #[test]
    fn evidence() {
        assert!(instance_evidence(&row(), true, 24, 12));
        assert!(!instance_evidence(&row(), true, 23, 12));
        assert!(!instance_evidence(&row(), true, 24, 11));
        assert!(!instance_evidence(&row(), false, 24, 12));
        for at in [2, 3, 4, 21, 18] {
            let mut r = row();
            r[at] = 0;
            assert!(!instance_evidence(&r, true, 1 << 20, 1 << 20), "{at}");
        }
        let mut r = row();
        r[10] = 25;
        assert!(!instance_evidence(&r, true, 24, 1 << 20));
        let mut r = row();
        r[18] = 14;
        assert!(!instance_evidence(&r, true, 1 << 20, 12));
    }
    #[test]
    fn payloads() {
        let blob: Vec<u8> = (0..16).collect();
        let private: Vec<u8> = (4..12).collect();
        assert!(instance_payload(8, &row(), &private, &blob));
        assert!(!instance_payload(7, &row(), &private, &blob));
        let mut other = private.clone();
        other[7] = 0;
        assert!(!instance_payload(8, &row(), &other, &blob));
        assert!(!instance_payload(8, &row(), &private[..7], &blob));
    }
}
