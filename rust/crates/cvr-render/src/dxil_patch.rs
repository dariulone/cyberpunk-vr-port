//! In-place constant patches of signed DXIL containers (formerly src/Render/DxilPatch.cpp; docs/hidden-area-mask-20261009.md).
//!
//! A float constant in [1, 2) has bit 29 as its highest set bit, so in the LLVM bitcode of the DXIL part it is always
//! six VBR6 chunks (36 bits) long. Writing another value from that range at the same bit offset keeps every other bit
//! of the container where it was; only the container hash has to be recomputed, which is the DXBC MD5 variant D3D12
//! checks.

const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11,
    16, 23, 4, 11, 16, 23, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];
const SINES: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be,
    0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c,
    0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1, 0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1,
    0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];
const PATTERN_BITS: usize = 36;

pub type Hash = [u8; 16];

fn transform(state: &mut [u32; 4], block: &[u8]) {
    let mut x = [0u32; 16];
    for (i, w) in x.iter_mut().enumerate() {
        *w = u32::from_le_bytes([block[i * 4], block[i * 4 + 1], block[i * 4 + 2], block[i * 4 + 3]]);
    }
    let [mut a, mut b, mut c, mut d] = *state;
    for i in 0..64 {
        let (f, g) = if i < 16 {
            ((b & c) | (!b & d), i)
        } else if i < 32 {
            ((d & b) | (!d & c), (5 * i + 1) % 16)
        } else if i < 48 {
            (b ^ c ^ d, (3 * i + 5) % 16)
        } else {
            (c ^ (b | !d), (7 * i) % 16)
        };
        let f = f.wrapping_add(a).wrapping_add(SINES[i]).wrapping_add(x[g]);
        a = d;
        d = c;
        c = b;
        b = b.wrapping_add(f.rotate_left(SHIFTS[i]));
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
}

/// ContainerHash: the hash stored at offset 4 of the container (zero for 20 bytes or fewer).
pub fn container_hash(container: &[u8]) -> Hash {
    let mut out = [0u8; 16];
    if container.len() <= 20 {
        return out;
    }
    let data = &container[20..];
    let length = data.len();
    let mut state = [0x67452301u32, 0xEFCDAB89, 0x98BADCFE, 0x10325476];
    let full = length & !63usize;
    for at in (0..full).step_by(64) {
        transform(&mut state, &data[at..at + 64]);
    }
    let count = length - full;
    let bits = (length as u64 * 8) as u32;
    let last = ((length as u64 * 2) as u32) | 1;
    let mut block = [0u8; 64];
    if 63 - count < 8 {
        block[..count].copy_from_slice(&data[full..]);
        block[count] = 0x80;
        transform(&mut state, &block);
        block = [0u8; 64];
    } else {
        block[4..4 + count].copy_from_slice(&data[full..]);
        block[4 + count] = 0x80;
    }
    block[..4].copy_from_slice(&bits.to_le_bytes());
    block[60..].copy_from_slice(&last.to_le_bytes());
    transform(&mut state, &block);
    for (i, s) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&s.to_le_bytes());
    }
    out
}

/// Sign: ContainerHash stored at offset 4 of a DXBC container.
pub fn sign(container: &mut [u8]) -> bool {
    if container.len() <= 20 || &container[..4] != b"DXBC" {
        return false;
    }
    let hash = container_hash(container);
    container[4..20].copy_from_slice(&hash);
    true
}

fn word(c: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([c[at], c[at + 1], c[at + 2], c[at + 3]])
}

/// BitcodeRange: (start, length) of the LLVM bitcode inside the container's DXIL part. Err carries the range of a
/// DXIL part whose bitcode runs past it (the C++ reported it through its out-parameters while answering false).
pub fn bitcode_range(c: &[u8]) -> Result<(usize, usize), Option<(usize, usize)>> {
    let size = c.len();
    if size < 32 || &c[..4] != b"DXBC" {
        return Err(None);
    }
    let parts = word(c, 28);
    if parts > 64 || 32 + parts as usize * 4 > size {
        return Err(None);
    }
    for i in 0..parts as usize {
        let offset = word(c, 32 + i * 4) as usize;
        if offset + 8 > size {
            return Err(None);
        }
        let part_size = word(c, offset + 4) as usize;
        if &c[offset..offset + 4] != b"DXIL" {
            continue;
        }
        let program = offset + 8;
        if part_size < 24 || program + part_size > size || &c[program + 8..program + 12] != b"DXIL" {
            return Err(None);
        }
        let start = program + 8 + word(c, program + 16) as usize;
        let length = word(c, program + 20) as usize;
        return if start + length <= program + part_size { Ok((start, length)) } else { Err(Some((start, length))) };
    }
    Err(None)
}

/// PlaceholderValue: in [1, 2).
pub fn placeholder_value(value: f32) -> bool {
    let w = value.to_bits();
    (0x3F800000..0x40000000).contains(&w)
}

/// The 36 bits of the value's VBR6 encoding, in stream order.
fn pattern(value: f32) -> u64 {
    let mut bits = 0u64;
    let mut w = value.to_bits();
    for chunk in 0..6 {
        let piece = (w & 31) as u64 | if chunk < 5 { 32 } else { 0 };
        w >>= 5;
        bits |= piece << (chunk * 6);
    }
    bits
}

/// FindFloat: container bit offsets of every occurrence of the placeholder's VBR6 pattern in the bitcode.
pub fn find_float(container: &[u8], placeholder: f32) -> Vec<usize> {
    let mut found = Vec::new();
    if !placeholder_value(placeholder) {
        return found;
    }
    let Ok((start, length)) = bitcode_range(container) else { return found };
    if length * 8 < PATTERN_BITS {
        return found;
    }
    let (want, mask) = (pattern(placeholder), (1u64 << PATTERN_BITS) - 1);
    let mut bit = start * 8;
    while bit + PATTERN_BITS <= (start + length) * 8 {
        let (byte, shift) = (bit / 8, bit % 8);
        let mut window = 0u64;
        for i in 0..6 {
            if byte + i < container.len() {
                window |= (container[byte + i] as u64) << (8 * i);
            }
        }
        if (window >> shift) & mask == want {
            found.push(bit);
        }
        bit += 1;
    }
    found
}

/// WriteFloat: `value` (in [1, 2)) at a bit offset found for a placeholder; the hash is not updated.
pub fn write_float(container: &mut [u8], bit: usize, value: f32) -> bool {
    if !placeholder_value(value) || (bit + PATTERN_BITS).div_ceil(8) > container.len() {
        return false;
    }
    let bits = pattern(value);
    for i in 0..PATTERN_BITS {
        let at = bit + i;
        let m = 1u8 << (at % 8);
        let byte = &mut container[at / 8];
        *byte = if (bits >> i) & 1 != 0 { *byte | m } else { *byte & !m };
    }
    true
}

// ---- the C ABI (include/Render/DxilPatch.hpp)

/// # Safety
/// `p` is null or points at `n` readable bytes.
unsafe fn bytes<'a>(p: *const u8, n: usize) -> Option<&'a [u8]> {
    // SAFETY: the caller's buffer.
    (!p.is_null()).then(|| unsafe { core::slice::from_raw_parts(p, n) })
}
/// # Safety
/// `p` is null or points at `n` writable bytes.
unsafe fn bytes_mut<'a>(p: *mut u8, n: usize) -> Option<&'a mut [u8]> {
    // SAFETY: the caller's buffer.
    (!p.is_null()).then(|| unsafe { core::slice::from_raw_parts_mut(p, n) })
}

/// # Safety
/// `container` is null or `size` readable bytes; `out` 16 writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_dxil_container_hash(container: *const u8, size: usize, out: *mut Hash) {
    // SAFETY: the caller's buffers.
    unsafe {
        let hash = bytes(container, size).map(container_hash).unwrap_or_default();
        if let Some(out) = out.as_mut() {
            *out = hash;
        }
    }
}
/// # Safety
/// `container` is null or `size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_dxil_sign(container: *mut u8, size: usize) -> bool {
    // SAFETY: the caller's buffer.
    unsafe { bytes_mut(container, size).is_some_and(sign) }
}
/// # Safety
/// `container` is null or `size` readable bytes; `start` and `length` writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_dxil_bitcode_range(container: *const u8, size: usize, start: *mut usize, length: *mut usize) -> bool {
    // SAFETY: the caller's buffers.
    unsafe {
        let range = bytes(container, size).map_or(Err(None), bitcode_range);
        let (ok, written) = match range {
            Ok(r) => (true, Some(r)),
            Err(r) => (false, r),
        };
        if let Some((s, l)) = written {
            if let Some(start) = start.as_mut() {
                *start = s;
            }
            if let Some(length) = length.as_mut() {
                *length = l;
            }
        }
        ok
    }
}
#[no_mangle]
pub extern "C" fn cvr_dxil_placeholder_value(value: f32) -> bool {
    placeholder_value(value)
}
/// Every occurrence; the first `capacity` offsets go to `out`, the answer is how many there are.
///
/// # Safety
/// `container` is null or `size` readable bytes; `out` is `capacity` writable offsets or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_dxil_find_float(container: *const u8, size: usize, placeholder: f32, out: *mut usize, capacity: usize) -> usize {
    // SAFETY: the caller's buffers.
    unsafe {
        let found = bytes(container, size).map(|c| find_float(c, placeholder)).unwrap_or_default();
        if !out.is_null() {
            for (i, &bit) in found.iter().take(capacity).enumerate() {
                *out.add(i) = bit;
            }
        }
        found.len()
    }
}
/// # Safety
/// `container` is null or `size` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_dxil_write_float(container: *mut u8, size: usize, bit: usize, value: f32) -> bool {
    // SAFETY: the caller's buffer.
    unsafe { bytes_mut(container, size).is_some_and(|c| write_float(c, bit, value)) }
}
