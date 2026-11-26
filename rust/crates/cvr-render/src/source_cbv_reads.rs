//! The source CBV binding's game reads (src/Render/NativeSourceCbvBinding.cpp, `Current`, `CpuDescriptor` and
//! `Native`'s record): the thread's native render context (the TLS slot's thread record, initialized at +0x14, its
//! context at +0x198, the context's state at +0x60 and command list at +0x30); a binding handle's CPU descriptor (a
//! non-dynamic record's at +0x48; a dynamic one's from the context's handle map at +0x4F8 -- a bounded chained hash of
//! 64-byte entries whose handle appears at +4 and +8, its descriptor at +0x38, at most 64 links); and a handle's
//! 0xB0-byte buffer record in the game's registry (at 0x5C0AF8 + (handle - 1) * 0xB0 from the registry the image holds at
//! 0x3BCDCA8, a constant-buffer record by its low kind nibble 6).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_scb_current`, `cvr_scb_cpu_descriptor` and
//! `cvr_scb_record` (reads through ReadProcessMemory, as the .cpp's `Read`; the TLS slot read by the .cpp's intrinsic);
//! the unit tests below read fabricated memory.

use crate::instance_layout::read;

fn u32_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u32> {
    let mut b = [0u8; 4];
    read(address, &mut b).then(|| u32::from_le_bytes(b))
}
fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    read(address, &mut b).then(|| u64::from_le_bytes(b))
}
fn field32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn field64(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

/// Current: (context, state, list) from the TLS slot array's first entry.
pub fn current(read: &impl Fn(u64, &mut [u8]) -> bool, tls: u64) -> Option<(u64, u64, u64)> {
    let thread = u64_at(read, tls).filter(|&t| t != 0 && t <= u64::MAX - 0x1A0)?;
    let mut initialized = [0u8; 1];
    if !read(thread + 0x14, &mut initialized) || initialized[0] == 0 {
        return None;
    }
    let context = u64_at(read, thread + 0x198).filter(|&c| c != 0 && c <= u64::MAX - 0x68)?;
    let state = u64_at(read, context + 0x60)?;
    let list = u64_at(read, context + 0x30)?;
    Some((context, state, list))
}

/// CpuDescriptor: the descriptor, or None.
pub fn cpu_descriptor(read: &impl Fn(u64, &mut [u8]) -> bool, context: u64, handle: u32, kind: u32, record: u64) -> Option<u64> {
    if kind != 4 {
        return u64_at(read, record.wrapping_add(0x48)).filter(|&d| d != 0);
    }
    if context > u64::MAX - 0x518 {
        return None;
    }
    let mut map = [0u8; 32];
    if !read(context + 0x4F8, &mut map) {
        return None;
    }
    let (buckets, count, bucket_count, data, capacity, stride) = (field64(&map, 0), field32(&map, 8), field32(&map, 12), field64(&map, 16), field32(&map, 24), field32(&map, 28));
    if count == 0 || count > capacity || capacity > 1048576 || bucket_count == 0 || bucket_count > 1048576 || stride != 64 || buckets == 0 || data == 0 {
        return None;
    }
    let offset = u64::from(handle % bucket_count) * 4;
    if buckets > u64::MAX - offset - 4 {
        return None;
    }
    let mut at = u32_at(read, buckets + offset)?;
    let mut n = 0u32;
    while n < count && n < 64 && at != u32::MAX {
        if at >= capacity || data > u64::MAX - u64::from(at) * 64 - 64 {
            return None;
        }
        let mut entry = [0u8; 64];
        if !read(data + u64::from(at) * 64, &mut entry) {
            return None;
        }
        if field32(&entry, 4) == handle && field32(&entry, 8) == handle {
            let descriptor = field64(&entry, 0x38);
            return (descriptor != 0).then_some(descriptor);
        }
        at = field32(&entry, 0);
        n += 1;
    }
    None
}

/// The registry's image RVA.
pub const REGISTRY_RVA: u64 = 0x3BCDCA8;
/// Native's record: (registry, record address) with the record read into `record`.
pub fn native_record(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, handle: u32, record: &mut [u8; 0xB0]) -> Option<(u64, u64)> {
    let registry = u64_at(read, base.wrapping_add(REGISTRY_RVA)).filter(|&r| r != 0)?;
    let relative = 0x5C0AF8 + u64::from(handle.wrapping_sub(1)) * 0xB0;
    if registry > u64::MAX - relative - 0xB0 {
        return None;
    }
    let address = registry + relative;
    if !read(address, record) || record[6] & 15 != 6 {
        return None;
    }
    Some((registry, address))
}

// ---- the C ABI

/// # Safety
/// The outputs are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_current(tls: u64, context: *mut u64, state: *mut u64, list: *mut u64) -> bool {
    match current(&read, tls) {
        Some((c, s, l)) => {
            unsafe { (*context, *state, *list) = (c, s, l) };
            true
        }
        None => false,
    }
}
/// # Safety
/// `descriptor` is writable (set to 0 on failure).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_cpu_descriptor(context: u64, handle: u32, kind: u32, record: u64, descriptor: *mut u64) -> bool {
    let found = cpu_descriptor(&read, context, handle, kind, record);
    unsafe { *descriptor = found.unwrap_or(0) };
    found.is_some()
}
/// # Safety
/// `record` holds 0xB0 bytes; `registry` and `address` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_scb_record(base: u64, handle: u32, record: *mut [u8; 0xB0], registry: *mut u64, address: *mut u64) -> bool {
    match native_record(&read, base, handle, unsafe { &mut *record }) {
        Some((r, a)) => {
            unsafe { (*registry, *address) = (r, a) };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Sparse memory: 8-byte-aligned words.
    struct Memory(RefCell<BTreeMap<u64, u8>>);
    impl Memory {
        fn new() -> Self {
            Memory(RefCell::new(BTreeMap::new()))
        }
        fn put(&self, at: u64, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.0.borrow_mut().insert(at + i as u64, b);
            }
        }
        fn reader(&self) -> impl Fn(u64, &mut [u8]) -> bool + '_ {
            move |address, out| {
                let m = self.0.borrow();
                for (i, o) in out.iter_mut().enumerate() {
                    match m.get(&(address + i as u64)) {
                        Some(&b) => *o = b,
                        None => return false,
                    }
                }
                true
            }
        }
    }
    #[test]
    fn contexts() {
        let m = Memory::new();
        let (tls, thread, context) = (0x1000u64, 0x2000u64, 0x3000u64);
        m.put(tls, &thread.to_le_bytes());
        m.put(thread + 0x14, &[1]);
        m.put(thread + 0x198, &context.to_le_bytes());
        m.put(context + 0x60, &0x44u64.to_le_bytes());
        m.put(context + 0x30, &0x55u64.to_le_bytes());
        assert_eq!(current(&m.reader(), tls), Some((context, 0x44, 0x55)));
        m.put(thread + 0x14, &[0]);
        assert_eq!(current(&m.reader(), tls), None);
        m.put(thread + 0x14, &[1]);
        m.put(thread + 0x198, &0u64.to_le_bytes());
        assert_eq!(current(&m.reader(), tls), None);
        m.put(tls, &0u64.to_le_bytes());
        assert_eq!(current(&m.reader(), tls), None);
    }
    fn map(m: &Memory, context: u64, buckets: u64, count: u32, bucket_count: u32, data: u64, capacity: u32, stride: u32) {
        let mut b = Vec::new();
        b.extend_from_slice(&buckets.to_le_bytes());
        b.extend_from_slice(&count.to_le_bytes());
        b.extend_from_slice(&bucket_count.to_le_bytes());
        b.extend_from_slice(&data.to_le_bytes());
        b.extend_from_slice(&capacity.to_le_bytes());
        b.extend_from_slice(&stride.to_le_bytes());
        m.put(context + 0x4F8, &b);
    }
    fn entry(m: &Memory, data: u64, index: u32, next: u32, handle: u32, descriptor: u64) {
        let mut e = [0u8; 64];
        e[0..4].copy_from_slice(&next.to_le_bytes());
        e[4..8].copy_from_slice(&handle.to_le_bytes());
        e[8..12].copy_from_slice(&handle.to_le_bytes());
        e[0x38..0x40].copy_from_slice(&descriptor.to_le_bytes());
        m.put(data + u64::from(index) * 64, &e);
    }
    #[test]
    fn descriptors() {
        let m = Memory::new();
        // A non-dynamic record's own descriptor.
        m.put(0x500 + 0x48, &0x77u64.to_le_bytes());
        assert_eq!(cpu_descriptor(&m.reader(), 0, 9, 1, 0x500), Some(0x77));
        m.put(0x600 + 0x48, &0u64.to_le_bytes());
        assert_eq!(cpu_descriptor(&m.reader(), 0, 9, 1, 0x600), None);
        // A dynamic handle: 7 hashes into bucket 7 % 4 = 3, which chains entry 2 -> entry 0 (the handle).
        let (context, buckets, data) = (0x10000u64, 0x20000u64, 0x30000u64);
        map(&m, context, buckets, 3, 4, data, 4, 64);
        m.put(buckets + 12, &2u32.to_le_bytes());
        entry(&m, data, 2, 0, 5, 0x99);
        entry(&m, data, 0, u32::MAX, 7, 0x88);
        assert_eq!(cpu_descriptor(&m.reader(), context, 7, 4, 0), Some(0x88));
        // A handle not in the chain.
        assert_eq!(cpu_descriptor(&m.reader(), context, 11, 4, 0), None);
        // The chain stops at the map's count.
        map(&m, context, buckets, 1, 4, data, 4, 64);
        assert_eq!(cpu_descriptor(&m.reader(), context, 7, 4, 0), None);
        // A malformed map is refused.
        for (count, bucket_count, capacity, stride) in [(0, 4, 4, 64), (5, 4, 4, 64), (3, 0, 4, 64), (3, 4, 4, 32), (3, 4, 1048577, 64)] {
            map(&m, context, buckets, count, bucket_count, data, capacity, stride);
            assert_eq!(cpu_descriptor(&m.reader(), context, 7, 4, 0), None);
        }
        // A link past the capacity is refused.
        map(&m, context, buckets, 3, 4, data, 2, 64);
        assert_eq!(cpu_descriptor(&m.reader(), context, 7, 4, 0), None);
    }
    #[test]
    fn records() {
        let m = Memory::new();
        let (base, registry) = (0x1000_0000u64, 0x2000_0000u64);
        m.put(base + REGISTRY_RVA, &registry.to_le_bytes());
        let address = registry + 0x5C0AF8 + 2 * 0xB0;
        let mut r = [0u8; 0xB0];
        r[6] = 0x46;
        r[0] = 0x10;
        m.put(address, &r);
        let mut out = [0u8; 0xB0];
        assert_eq!(native_record(&m.reader(), base, 3, &mut out), Some((registry, address)));
        assert_eq!((out[0], out[6]), (0x10, 0x46));
        // Another kind nibble, or an unreadable record.
        r[6] = 0x45;
        m.put(address, &r);
        assert_eq!(native_record(&m.reader(), base, 3, &mut out), None);
        assert_eq!(native_record(&m.reader(), base, 4, &mut out), None);
    }
}
