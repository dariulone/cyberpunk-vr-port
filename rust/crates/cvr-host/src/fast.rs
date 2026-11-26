//! The plugin's hash maps' hasher: FxHash's word mixing (a rotate, xor and multiply per word) with a final rotation,
//! so aligned pointers and small integers -- nearly every key here (lists, resources, pipelines, cookies) -- spread
//! over the table's low bucket bits. std's default SipHash is keyed against hash flooding, which nothing in-process
//! needs, and costs several times more on the per-draw and per-submission lookups the C++ did with an identity hash.

use core::hash::{BuildHasherDefault, Hasher};
use std::collections::{HashMap, HashSet};

const MULTIPLIER: u64 = 0xf135_7aea_2e62_a9c5;

/// FxHash's mixing over 64-bit words.
#[derive(Clone, Copy, Default)]
pub struct FastHasher(u64);
impl FastHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0 ^ word).wrapping_mul(MULTIPLIER);
    }
}
impl Hasher for FastHasher {
    #[inline]
    fn finish(&self) -> u64 {
        // The multiply leaves the input's zero low bits zero: rotate the well-mixed high bits down.
        self.0.rotate_left(26)
    }
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word) ^ ((rest.len() as u64) << 59));
        }
    }
    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.add(value.into());
    }
    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.add(value.into());
    }
    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.add(value.into());
    }
    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.add(value);
    }
    #[inline]
    fn write_usize(&mut self, value: usize) {
        self.add(value as u64);
    }
    #[inline]
    fn write_i32(&mut self, value: i32) {
        self.add(value as u32 as u64);
    }
    #[inline]
    fn write_i64(&mut self, value: i64) {
        self.add(value as u64);
    }
}
pub type FastBuild = BuildHasherDefault<FastHasher>;
pub type FastMap<K, V> = HashMap<K, V, FastBuild>;
pub type FastSet<K> = HashSet<K, FastBuild>;
/// An empty map, usable in a static.
pub const fn fast_map<K, V>() -> FastMap<K, V> {
    HashMap::with_hasher(BuildHasherDefault::new())
}
/// An empty set, usable in a static.
pub const fn fast_set<K>() -> FastSet<K> {
    HashSet::with_hasher(BuildHasherDefault::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::hash::BuildHasher;

    #[test]
    fn spreads_aligned_keys() {
        // 4096 pointers 64 bytes apart: the low 8 bits of their hashes cover well over half of the 256 values.
        let build = FastBuild::default();
        let mut low = std::collections::BTreeSet::new();
        for i in 0..4096u64 {
            low.insert(build.hash_one(0x7ff6_0000_0000u64 + i * 64) & 255);
        }
        assert!(low.len() > 200, "{}", low.len());
        let mut map: FastMap<usize, u32> = fast_map();
        for i in 0..1000usize {
            map.insert(i * 16, i as u32);
        }
        assert!((0..1000usize).all(|i| map[&(i * 16)] == i as u32));
        let mut set: FastSet<(u64, u32)> = fast_set();
        assert!(set.insert((1, 2)) && !set.insert((1, 2)) && set.insert((2, 1)));
        // Byte keys hash their length too.
        assert_ne!(build.hash_one(&b"ab"[..]), build.hash_one(&b"ab\0"[..]));
    }
}
