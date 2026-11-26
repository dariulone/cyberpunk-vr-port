//! FNV-1a, as RED4ext's `Hashing/FNV1a.hpp`: CName values (64-bit) and memory pool handles (32-bit).

pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    hash
}

pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u32;
        hash = hash.wrapping_mul(0x0100_0193);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_values() {
        // FNV-1a reference values.
        assert_eq!(super::fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(super::fnv1a64(b"a"), 0xAF63_DC4C_8601_EC8C);
        assert_eq!(super::fnv1a32(b""), 0x811C_9DC5);
        assert_eq!(super::fnv1a32(b"a"), 0xE40C_292C);
    }
}
