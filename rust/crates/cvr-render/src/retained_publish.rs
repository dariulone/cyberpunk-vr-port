//! The retained geometry's published summaries (src/Render/NativeRetainedGeometry.cpp, `PublishSources`): the counts each
//! diagnostic stats row derives from its recorded rows -- the source stream (rows of 48: kept or refused, the side,
//! the four flag bits of word 43), the indirect bindings (56: kept or refused, an argument resource, kind 20, mode 2),
//! the source CBVs (24: owned or not, vertex or other visibility, owned kinds 1..3 or the first other kind), the
//! indirect snapshots (24: kept or refused, the copies, bytes and versions, the first failure), the source tables (the
//! commands of 24 -- kept or refused, the descriptors and tables, the first refusal -- and the rows of 16 with their
//! descriptors), the source details (128: complete or not, the backing kinds 1, 2 and 5, the four evidence words, the
//! first failure), the CBV snapshots (rows of 32 by status 1, 2 or other, the copies and bytes, the first status-3
//! reason; commands of 24 with their two evidence words and first reason), the selectors (commands of 32 by kind 1, 2 or
//! more, their slots and evidence; inputs of 24 with two evidence words), the source assembly (32: kept or refused,
//! the evidence, the tables, slots and banks, the first refusal) and the T5 overlay rows (32: word 28 set).
//!
//! In the Rust build (`CVR_RUST_VRIK`) each loop forwards to `cvr_nrg_summarize` with its kind; the unit tests below
//! cover each counter of each kind.

/// The summarized row kinds and their row widths (first rows, second rows).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Stream = 0,
    Indirect = 1,
    Cbv = 2,
    Snapshots = 3,
    Tables = 4,
    Details = 5,
    CbvSnapshots = 6,
    Selectors = 7,
    Assembly = 8,
    Overlay = 9,
}
impl Kind {
    pub fn from(value: u32) -> Option<Kind> {
        use Kind::*;
        [Stream, Indirect, Cbv, Snapshots, Tables, Details, CbvSnapshots, Selectors, Assembly, Overlay].get(value as usize).copied()
    }
    pub fn widths(self) -> (usize, usize) {
        match self {
            Kind::Stream => (48, 0),
            Kind::Indirect => (56, 0),
            Kind::Cbv | Kind::Snapshots => (24, 0),
            Kind::Tables => (24, 16),
            Kind::Details => (128, 0),
            Kind::CbvSnapshots => (32, 24),
            Kind::Selectors => (32, 24),
            Kind::Assembly | Kind::Overlay => (32, 0),
        }
    }
}

fn add(stat: &mut u64, value: u64) {
    *stat = stat.wrapping_add(value);
}
fn count(stat: &mut u64, yes: bool) {
    add(stat, u64::from(yes));
}
fn first(stat: &mut u64, value: u64) {
    if *stat == 0 {
        *stat = value;
    }
}

/// Adds the rows' counts to `stats`; `first` and `second` hold whole rows of the kind's widths.
pub fn summarize(kind: Kind, first_rows: &[u64], second_rows: &[u64], s: &mut [u64; 16]) {
    let (a, b) = kind.widths();
    let rows = first_rows.chunks_exact(a);
    let seconds = if b == 0 { [].chunks_exact(1) } else { second_rows.chunks_exact(b) };
    match kind {
        Kind::Stream => {
            for r in rows {
                count(&mut s[if r[2] != 0 { 4 } else { 3 }], true);
                if r[1] < 2 {
                    count(&mut s[5 + r[1] as usize], true);
                }
                for bit in 0..4 {
                    count(&mut s[7 + bit], r[43] & (1 << bit) != 0);
                }
            }
        }
        Kind::Indirect => {
            for r in rows {
                count(&mut s[if r[3] != 0 { 3 } else { 4 }], true);
                count(&mut s[5], r[26] != 0);
                count(&mut s[6], r[15] == 20);
                count(&mut s[7], r[8] == 2);
            }
        }
        Kind::Cbv => {
            for r in rows {
                count(&mut s[if r[23] != 0 { 4 } else { 5 }], true);
                // D3D12_SHADER_VISIBILITY_VERTEX.
                count(&mut s[if r[2] == 1 { 6 } else { 7 }], true);
                if r[23] != 0 && (1..=3).contains(&r[7]) {
                    count(&mut s[7 + r[7] as usize], true);
                } else {
                    first(&mut s[11], r[7]);
                }
            }
        }
        Kind::Snapshots => {
            for r in rows {
                count(&mut s[if r[2] != 0 { 4 } else { 5 }], true);
                add(&mut s[6], r[13]);
                add(&mut s[7], r[21]);
                add(&mut s[8], r[20]);
                if r[5] != 0 {
                    first(&mut s[9], r[5]);
                }
            }
        }
        Kind::Tables => {
            for r in rows {
                count(&mut s[if r[2] != 0 { 4 } else { 5 }], true);
                add(&mut s[10], r[9]);
                add(&mut s[11], r[10]);
                if r[2] == 0 {
                    first(&mut s[12], r[3]);
                }
            }
            s[6] = (second_rows.len() / 16) as u64;
            for r in seconds {
                add(&mut s[7], r[3]);
            }
        }
        Kind::Details => {
            for r in rows {
                count(&mut s[if r[7] != 0 { 4 } else { 5 }], true);
                if r[7] != 0 {
                    match r[8] {
                        1 => count(&mut s[6], true),
                        2 => count(&mut s[7], true),
                        5 => count(&mut s[8], true),
                        _ => {}
                    }
                    for (stat, word) in [(9, 18), (10, 31), (11, 64), (12, 84)] {
                        count(&mut s[stat], r[word] != 0);
                    }
                } else {
                    first(&mut s[13], r[39]);
                }
            }
        }
        Kind::CbvSnapshots => {
            for r in rows {
                count(&mut s[match r[8] {
                    1 => 4,
                    2 => 5,
                    _ => 6,
                }], true);
                add(&mut s[7], r[20]);
                add(&mut s[8], r[21]);
                if r[8] == 3 {
                    first(&mut s[11], r[9]);
                }
            }
            for r in seconds {
                count(&mut s[10], r[18] != 0);
                count(&mut s[12], r[19] != 0);
                if r[3] != 0 {
                    first(&mut s[11], r[3]);
                }
            }
        }
        Kind::Selectors => {
            for r in rows {
                count(&mut s[6], r[2] == 1);
                count(&mut s[7], r[2] == 2);
                count(&mut s[8], r[2] >= 3);
                add(&mut s[9], r[8]);
                count(&mut s[10], r[11] != 0);
            }
            for r in seconds {
                count(&mut s[11], r[7] != 0);
                count(&mut s[12], r[13] != 0);
            }
        }
        Kind::Assembly => {
            for r in rows {
                count(&mut s[if r[2] != 0 { 3 } else { 4 }], true);
                count(&mut s[5], r[4] != 0);
                add(&mut s[6], r[5]);
                add(&mut s[8], r[7]);
                count(&mut s[9], r[21] != 0);
                count(&mut s[10], r[22] != 0);
                if r[3] != 0 {
                    first(&mut s[11], r[3]);
                }
            }
        }
        Kind::Overlay => {
            for r in rows {
                count(&mut s[12], r[28] != 0);
            }
        }
    }
}

// ---- the C ABI

/// # Safety
/// `first` holds `first_count` rows and `second` `second_count` rows of the kind's widths (either null when empty);
/// `stats` holds 16 values. An unknown kind changes nothing.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_summarize(kind: u32, first: *const u64, first_count: usize, second: *const u64, second_count: usize, stats: *mut [u64; 16]) {
    let Some(kind) = Kind::from(kind) else { return };
    let (a, b) = kind.widths();
    let slice = |rows: *const u64, n: usize, width: usize| if rows.is_null() || n == 0 || width == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(rows, n * width) } };
    summarize(kind, slice(first, first_count, a), slice(second, second_count, b), unsafe { &mut *stats });
}

#[cfg(test)]
mod tests {
    //! Each counter of each kind.
    use super::*;

    fn rows(width: usize, set: &[&[(usize, u64)]]) -> Vec<u64> {
        let mut out = vec![0u64; width * set.len()];
        for (i, values) in set.iter().enumerate() {
            for &(at, value) in *values {
                out[i * width + at] = value;
            }
        }
        out
    }
    fn run(kind: Kind, first: &[&[(usize, u64)]], second: &[&[(usize, u64)]]) -> [u64; 16] {
        let (a, b) = kind.widths();
        let mut s = [0u64; 16];
        summarize(kind, &rows(a, first), &rows(b.max(1), second), &mut s);
        s
    }
    #[test]
    fn streams() {
        let s = run(Kind::Stream, &[&[(2, 1), (1, 0), (43, 5)], &[(1, 1), (43, 10)], &[(1, 2), (43, 16)]], &[]);
        assert_eq!(s[3..11], [2, 1, 1, 1, 1, 1, 1, 1]);
    }
    #[test]
    fn indirects() {
        let s = run(Kind::Indirect, &[&[(3, 1), (26, 4), (15, 20), (8, 2)], &[(15, 21), (8, 1)]], &[]);
        assert_eq!(s[3..8], [1, 1, 1, 1, 1]);
    }
    #[test]
    fn cbvs() {
        let s = run(Kind::Cbv, &[&[(23, 1), (2, 1), (7, 2)], &[(23, 1), (2, 2), (7, 4)], &[(7, 3)], &[(7, 6)]], &[]);
        assert_eq!(s[4..12], [2, 2, 1, 3, 0, 1, 0, 4]);
        let s = run(Kind::Cbv, &[&[(23, 1), (7, 1)], &[(23, 1), (7, 3)]], &[]);
        assert_eq!((s[8], s[10], s[11]), (1, 1, 0));
    }
    #[test]
    fn snapshots() {
        let s = run(Kind::Snapshots, &[&[(2, 1), (13, 2), (21, 3), (20, 4)], &[(5, 7), (13, 1)], &[(5, 8)]], &[]);
        assert_eq!(s[4..10], [1, 2, 3, 3, 4, 7]);
    }
    #[test]
    fn tables() {
        let s = run(Kind::Tables, &[&[(2, 1), (9, 2), (10, 3), (3, 9)], &[(3, 5), (9, 1)], &[(3, 6)]], &[&[(3, 4)], &[(3, 6)]]);
        assert_eq!((s[4], s[5], s[6], s[7], s[10], s[11], s[12]), (1, 2, 2, 10, 3, 3, 5));
        assert_eq!(run(Kind::Tables, &[], &[])[6], 0);
    }
    #[test]
    fn details() {
        let s = run(
            Kind::Details,
            &[&[(7, 1), (8, 1), (18, 1), (31, 1), (64, 1), (84, 1)], &[(7, 1), (8, 2)], &[(7, 1), (8, 5), (39, 3)], &[(7, 1), (8, 3)], &[(39, 6)], &[(39, 7)]],
            &[],
        );
        assert_eq!(s[4..14], [4, 2, 1, 1, 1, 1, 1, 1, 1, 6]);
    }
    #[test]
    fn cbv_snapshots() {
        let s = run(Kind::CbvSnapshots, &[&[(8, 1), (20, 2), (21, 3)], &[(8, 2)], &[(8, 3), (9, 8)], &[(8, 3), (9, 9)]], &[&[(18, 1), (3, 5)], &[(19, 1)]]);
        assert_eq!((s[4], s[5], s[6], s[7], s[8], s[10], s[11], s[12]), (1, 1, 2, 2, 3, 1, 8, 1));
        let s = run(Kind::CbvSnapshots, &[&[(8, 1)]], &[&[(3, 5)], &[(3, 6)]]);
        assert_eq!(s[11], 5);
    }
    #[test]
    fn selectors() {
        let s = run(Kind::Selectors, &[&[(2, 1), (8, 2), (11, 1)], &[(2, 2), (8, 3)], &[(2, 3)], &[(2, 4)], &[(2, 0)]], &[&[(7, 1)], &[(13, 1)], &[]]);
        assert_eq!(s[6..13], [1, 1, 2, 5, 1, 1, 1]);
    }
    #[test]
    fn assemblies() {
        let s = run(Kind::Assembly, &[&[(2, 1), (4, 1), (5, 2), (7, 3), (21, 1), (22, 1)], &[(3, 6), (5, 1)], &[(3, 7)]], &[]);
        assert_eq!((s[3], s[4], s[5], s[6], s[8], s[9], s[10], s[11]), (1, 2, 1, 3, 3, 1, 1, 6));
        assert_eq!(s[7], 0);
    }
    #[test]
    fn overlays() {
        let s = run(Kind::Overlay, &[&[(28, 1)], &[], &[(28, 3)]], &[]);
        assert_eq!(s[12], 2);
        assert_eq!(s.iter().filter(|&&v| v != 0).count(), 1);
    }
    #[test]
    fn abi() {
        let r = rows(48, &[&[(2, 1)]]);
        let mut s = [0u64; 16];
        unsafe { cvr_nrg_summarize(0, r.as_ptr(), 1, core::ptr::null(), 0, &mut s) };
        assert_eq!(s[4], 1);
        unsafe { cvr_nrg_summarize(10, r.as_ptr(), 1, core::ptr::null(), 0, &mut s) };
        assert_eq!(s.iter().sum::<u64>(), 2);
    }
}
