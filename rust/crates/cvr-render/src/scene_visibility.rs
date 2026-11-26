//! A native scene pool's visibility input, owned (include/Render/NativeSceneVisibility.hpp's NativeSceneVisibility, as
//! the CPU collector probe uses it): one bounded, complete traversal of the pool's hash slots into buckets -- each
//! node's AABB as the native coarse test loads it (unaligned at +0x10 and +0x1C, unused W lanes included) and its
//! candidates --, the pool's header re-read unchanged after; its evaluation through the native coarse and fine tests;
//! and two captures' differences in exactly the bytes those tests consume.
//!
//! The header keeps the class for the C++ build and the single-pass tests (tools/single_pass_tests/cpu_collector_test,
//! with its tagged batches).

use crate::fine_visibility::{run_native_fine, NativeCullCandidate, NativeFineCull, MAX_FINE_CANDIDATES};
use core::ffi::c_void;

/// NativeCoarseCull: (query, bounds, 1) -> 0 hidden, 1 partial, 2 inside.
pub type NativeCoarseCull = unsafe extern "C-unwind" fn(*mut c_void, *const c_void, u32) -> i8;

const HEADER_BYTES: usize = 32;
const NODE_BYTES: usize = 56;
const LIMIT: u32 = 1_048_576;

/// NativeSceneBucket's bounds: the two 16-byte loads.
#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds(pub [u8; 32]);
#[derive(Clone, Debug, Default)]
pub struct Bucket {
    pub bounds: Bounds,
    pub candidates: Vec<NativeCullCandidate>,
    pub source_begin: u64,
}
#[derive(Debug, Default)]
pub struct SceneVisibility {
    pub failure: u32,
    pub declared: u32,
    pub candidate_count: u32,
    pub bytes_owned: u32,
    pub buckets: Vec<Bucket>,
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn f32_at(b: &[u8], at: usize) -> f32 {
    f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
impl SceneVisibility {
    fn fail(&mut self, code: u32) -> bool {
        self.failure = code;
        false
    }
    /// Capture: the pool at `address` (its header: slots, size, slot count, nodes, capacity, stride).
    pub fn capture(&mut self, address: u64, read: &impl Fn(u64, &mut [u8]) -> bool) -> bool {
        (self.failure, self.declared, self.candidate_count, self.bytes_owned) = (0, 0, 0, 0);
        self.buckets.clear();
        let mut pool = [0u8; HEADER_BYTES];
        if !read(address, &mut pool) {
            return self.fail(1);
        }
        let (slots, size, slot_count, nodes, capacity, stride) = (u64_at(&pool, 0), u32_at(&pool, 8), u32_at(&pool, 12), u64_at(&pool, 16), u32_at(&pool, 24), u32_at(&pool, 28));
        self.declared = size;
        if size > capacity || capacity > LIMIT || slot_count > LIMIT || (stride as usize) < NODE_BYTES || stride > 4096 {
            return self.fail(2);
        }
        if size != 0 && (slots == 0 || nodes == 0 || slot_count == 0) {
            return self.fail(2);
        }
        if size == 0 {
            return true;
        }
        let mut heads = vec![0u8; slot_count as usize * 4];
        let mut seen = vec![false; capacity as usize];
        if !read(slots, &mut heads) {
            return self.fail(3);
        }
        for head in heads.chunks_exact(4) {
            let mut index = u32::from_le_bytes(head.try_into().unwrap());
            while index != u32::MAX {
                if index >= capacity || seen[index as usize] || self.buckets.len() >= size as usize {
                    return self.fail(4);
                }
                seen[index as usize] = true;
                let offset = u64::from(index) * u64::from(stride);
                if nodes > u64::MAX - offset {
                    return self.fail(4);
                }
                let mut node = [0u8; NODE_BYTES];
                if !read(nodes + offset, &mut node) {
                    return self.fail(5);
                }
                let (next, candidates, node_capacity, count) = (u32_at(&node, 0), u64_at(&node, 0x28), u32_at(&node, 0x30), u32_at(&node, 0x34));
                if count > node_capacity || count as usize > MAX_FINE_CANDIDATES || self.candidate_count + count > LIMIT || (count != 0 && candidates == 0) {
                    return self.fail(6);
                }
                for axis in 0..3 {
                    let (low, high) = (f32_at(&node, 0x10 + axis * 4), f32_at(&node, 0x1C + axis * 4));
                    if !low.is_finite() || !high.is_finite() || low > high {
                        return self.fail(7);
                    }
                }
                // The native unaligned loads at node+0x10 and node+0x1C, unused W lanes included; culling consumes XYZ.
                let mut bounds = Bounds::default();
                bounds.0[..16].copy_from_slice(&node[0x10..0x20]);
                bounds.0[16..].copy_from_slice(&node[0x1C..0x2C]);
                let mut owned = vec![NativeCullCandidate::default(); count as usize];
                // SAFETY: the candidates' bytes.
                let bytes = unsafe { core::slice::from_raw_parts_mut(owned.as_mut_ptr().cast::<u8>(), count as usize * 40) };
                if count != 0 && !read(candidates, bytes) {
                    return self.fail(8);
                }
                self.candidate_count += count;
                self.bytes_owned += 32 + count * 40;
                self.buckets.push(Bucket { bounds, candidates: owned, source_begin: candidates });
                index = next;
            }
        }
        if self.buckets.len() != size as usize {
            return self.fail(9);
        }
        let mut after = [0u8; HEADER_BYTES];
        if !read(address, &mut after) || after != pool {
            return self.fail(10);
        }
        true
    }
    /// Evaluate: the visible ids of every bucket the coarse test accepts, through the fine test (partial when the
    /// coarse answer was 1).
    ///
    /// # Safety
    /// `coarse` and `fine` are the engine's tests for `query`.
    pub unsafe fn evaluate(&self, coarse: Option<NativeCoarseCull>, fine: Option<NativeFineCull>, query: *mut c_void, ids: &mut Vec<u64>, visible_buckets: &mut u32) -> bool {
        ids.clear();
        *visible_buckets = 0;
        let (Some(coarse), Some(_)) = (coarse, fine) else { return false };
        if self.failure != 0 || query.is_null() {
            return false;
        }
        for bucket in &self.buckets {
            // SAFETY: as documented.
            let result = unsafe { coarse(query, bucket.bounds.0.as_ptr().cast(), 1) } as u8;
            if result > 2 {
                return false;
            }
            if result == 0 {
                continue;
            }
            *visible_buckets += 1;
            let Some(batch) = (unsafe { run_fine(fine, query, &bucket.candidates, (result == 1).into()) }) else { return false };
            ids.extend_from_slice(&batch);
        }
        true
    }
    /// CandidateDifferences: [either failed or bucket counts differ, candidate counts differ, sources, XYZ lanes,
    /// W lanes, bucket sizes, ids, bounds, masks, reserved words].
    pub fn candidate_differences(&self, other: &SceneVisibility) -> [u32; 10] {
        let mut diff = [0u32; 10];
        diff[0] = (self.failure != 0 || other.failure != 0 || self.buckets.len() != other.buckets.len()).into();
        diff[1] = (self.candidate_count != other.candidate_count).into();
        for (a, b) in self.buckets.iter().zip(&other.buckets) {
            diff[2] += u32::from(a.source_begin != b.source_begin);
            for lane in 0..8 {
                if a.bounds.0[lane * 4..lane * 4 + 4] != b.bounds.0[lane * 4..lane * 4 + 4] {
                    diff[if lane == 3 || lane == 7 { 4 } else { 3 }] += 1;
                }
            }
            diff[5] += u32::from(a.candidates.len() != b.candidates.len());
            for (x, y) in a.candidates.iter().zip(&b.candidates) {
                diff[6] += u32::from(x.id != y.id);
                diff[7] += u32::from(x.bounds.map(f32::to_bits) != y.bounds.map(f32::to_bits));
                diff[8] += u32::from(x.include_mask != y.include_mask || x.exclude_mask != y.exclude_mask);
                diff[9] += u32::from(x.reserved != y.reserved);
            }
        }
        diff
    }
    pub fn same_candidates(&self, other: &SceneVisibility) -> bool {
        let diff = self.candidate_differences(other);
        [0, 1, 3, 5, 6, 7, 8].iter().all(|&field| diff[field] == 0)
    }
}
/// RunNativeFine (the Rust build's): the visible ids, or None.
///
/// # Safety
/// `fine` is the engine's fine test for `query`.
pub unsafe fn run_fine(fine: Option<NativeFineCull>, query: *mut c_void, candidates: &[NativeCullCandidate], partial: i8) -> Option<Vec<u64>> {
    let mut ids = if fine.is_some() && !query.is_null() && candidates.len() <= MAX_FINE_CANDIDATES { vec![0u64; candidates.len()] } else { Vec::new() };
    // SAFETY: as documented.
    let count = unsafe { run_native_fine(fine, query, candidates, partial, &mut ids) }?;
    ids.truncate(count as usize);
    Some(ids)
}

#[cfg(test)]
mod tests {
    //! A fabricated pool: two slots chaining three nodes (one empty), its capture's buckets and their W lanes, the
    //! refusals (a cycle, bad bounds, a changed header), the evaluation through stand-in tests and the differences.
    use super::*;
    use std::collections::HashMap;

    fn node(next: u32, bounds: [f32; 6], candidates: u64, count: u32) -> [u8; NODE_BYTES] {
        let mut n = [0u8; NODE_BYTES];
        n[0..4].copy_from_slice(&next.to_le_bytes());
        for (i, v) in bounds.iter().enumerate() {
            n[0x10 + i * 4..0x14 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        n[0x28..0x30].copy_from_slice(&candidates.to_le_bytes());
        n[0x30..0x34].copy_from_slice(&count.to_le_bytes());
        n[0x34..0x38].copy_from_slice(&count.to_le_bytes());
        n
    }
    fn candidate(id: u64) -> NativeCullCandidate {
        NativeCullCandidate { id, bounds: [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], ..Default::default() }
    }
    struct Memory(HashMap<u64, Vec<u8>>);
    impl Memory {
        fn read(&self, address: u64, out: &mut [u8]) -> bool {
            self.0.get(&address).is_some_and(|b| b.len() >= out.len() && {
                out.copy_from_slice(&b[..out.len()]);
                true
            })
        }
    }
    fn pool(nodes: &[[u8; NODE_BYTES]], heads: [u32; 2]) -> Memory {
        let mut m = HashMap::new();
        let mut header = [0u8; HEADER_BYTES];
        header[0..8].copy_from_slice(&0x1000u64.to_le_bytes());
        header[8..12].copy_from_slice(&(nodes.len() as u32).to_le_bytes());
        header[12..16].copy_from_slice(&2u32.to_le_bytes());
        header[16..24].copy_from_slice(&0x2000u64.to_le_bytes());
        header[24..28].copy_from_slice(&8u32.to_le_bytes());
        header[28..32].copy_from_slice(&(NODE_BYTES as u32).to_le_bytes());
        m.insert(0x100, header.to_vec());
        m.insert(0x1000, [heads[0].to_le_bytes(), heads[1].to_le_bytes()].concat());
        for (i, n) in nodes.iter().enumerate() {
            m.insert(0x2000 + (i * NODE_BYTES) as u64, n.to_vec());
        }
        let cands = [candidate(7), candidate(8)];
        // SAFETY: plain records.
        m.insert(0x3000, unsafe { core::slice::from_raw_parts(cands.as_ptr().cast::<u8>(), 80) }.to_vec());
        Memory(m)
    }
    unsafe extern "C-unwind" fn coarse(_: *mut c_void, bounds: *const c_void, _: u32) -> i8 {
        // Inside when the low x is 0, partial at 5, else hidden.
        match unsafe { *bounds.cast::<f32>() } as i32 {
            0 => 2,
            5 => 1,
            _ => 0,
        }
    }
    unsafe extern "C-unwind" fn fine(_: *mut c_void, range: *mut i64, partial: i8, output: *mut c_void) -> i8 {
        let (begin, end) = unsafe { (*range, *range.add(1)) };
        let out = unsafe { &mut *output.cast::<crate::fine_visibility::NativeVisibleIds>() };
        let mut at = begin;
        while at < end {
            if partial == 0 {
                unsafe { *out.data.add(out.count as usize) = (*(at as *const NativeCullCandidate)).id };
                out.count += 1;
            }
            at += 40;
        }
        1
    }

    #[test]
    fn captures_evaluates_and_compares() {
        let nodes = [node(1, [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 0x3000, 2), node(u32::MAX, [5.0, 0.0, 0.0, 6.0, 1.0, 1.0], 0, 0), node(u32::MAX, [9.0, 0.0, 0.0, 9.0, 1.0, 1.0], 0, 0)];
        let m = pool(&nodes, [0, 2]);
        let mut s = SceneVisibility::default();
        assert!(s.capture(0x100, &|a, b| m.read(a, b)), "failure {}", s.failure);
        assert_eq!((s.declared, s.candidate_count, s.bytes_owned, s.buckets.len()), (3, 2, 32 * 3 + 80, 3));
        assert_eq!(s.buckets[0].source_begin, 0x3000);
        assert_eq!(&s.buckets[0].bounds.0[28..32], &0x3000u32.to_le_bytes(), "the high W lane is the candidates' low dword");
        let (mut ids, mut visible) = (Vec::new(), 0u32);
        assert!(unsafe { s.evaluate(Some(coarse), Some(fine), 0x10 as *mut c_void, &mut ids, &mut visible) });
        assert_eq!((ids, visible), (vec![7, 8], 2), "inside: every id; partial: none of this stand-in's");
        let mut other = SceneVisibility::default();
        let mut changed = nodes;
        changed[0][0x14..0x18].copy_from_slice(&0.5f32.to_le_bytes());
        let m2 = pool(&changed, [0, 2]);
        assert!(other.capture(0x100, &|a, b| m2.read(a, b)));
        assert_eq!(s.candidate_differences(&other), [0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
        assert!(!s.same_candidates(&other) && s.same_candidates(&s));
        // A cycle (node 1 chains to itself through slot 1), bad bounds and a header changed while read.
        let m3 = pool(&[nodes[0], node(1, [0.0; 6], 0, 0), nodes[2]], [0, 2]);
        assert!(!s.capture(0x100, &|a, b| m3.read(a, b)) && s.failure == 4);
        let m4 = pool(&[node(u32::MAX, [2.0, 0.0, 0.0, 1.0, 1.0, 1.0], 0, 0)], [0, u32::MAX]);
        assert!(!s.capture(0x100, &|a, b| m4.read(a, b)) && s.failure == 7);
        let calls = core::cell::Cell::new(0);
        let changing = |a: u64, b: &mut [u8]| {
            let ok = m.read(a, b);
            if a == 0x100 {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    b[8] = 9;
                }
            }
            ok
        };
        assert!(!s.capture(0x100, &changing) && s.failure == 10);
        assert!(!unsafe { s.evaluate(Some(coarse), Some(fine), 0x10 as *mut c_void, &mut Vec::new(), &mut 0) }, "a failed capture");
    }
}
