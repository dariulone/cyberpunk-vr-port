//! TweakDBID (RED4ext.SDK NativeTypes.hpp): a name's CRC32-B and its length, with the flat buffer offset in the top
//! three bytes (big endian). A derived id (`base + ".name"`) continues the base's CRC with the suffix.

const fn table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[i] = c;
        i += 1;
    }
    t
}
static CRC32_TABLE: [u32; 256] = table();

/// RED4ext::CRC32(data, len, seed).
pub fn crc32(data: &[u8], seed: u32) -> u32 {
    let mut crc = !seed;
    for &b in data {
        crc = (crc >> 8) ^ CRC32_TABLE[((crc & 0xFF) ^ b as u32) as usize];
    }
    !crc
}
fn make(hash: u32, length: u8) -> u64 {
    hash as u64 | (length as u64) << 32
}
/// TweakDBID(name), and the name handed to the observer (observe_names) when one is set.
pub fn from_name(name: &str) -> u64 {
    let value = name_value(name);
    let observer = OBSERVER.load(core::sync::atomic::Ordering::Acquire);
    if !observer.is_null() {
        // SAFETY: a `fn(u64, &str)` set by observe_names.
        unsafe { core::mem::transmute::<*mut (), fn(u64, &str)>(observer)(value, name) };
    }
    value
}
/// TweakDBID(name) alone.
pub fn name_value(name: &str) -> u64 {
    make(crc32(name.as_bytes(), 0), name.len() as u8)
}
static OBSERVER: core::sync::atomic::AtomicPtr<()> = core::sync::atomic::AtomicPtr::new(core::ptr::null_mut());
/// Every name from_name builds an id from, handed to `f` (the plugin's TweakDB names without the Lua host).
pub fn observe_names(f: fn(u64, &str)) {
    OBSERVER.store(f as *mut (), core::sync::atomic::Ordering::Release);
}
/// TweakDBID(base, name): `base + name` with no offset.
pub fn derive(base: u64, name: &str) -> u64 {
    let hash = crc32(name.as_bytes(), base as u32);
    make(hash, ((base >> 32) as u8).wrapping_add(name.len() as u8))
}
pub fn is_valid(id: u64) -> bool {
    id as u32 != 0 && (id >> 32) as u8 != 0
}
/// ToTDBOffset: the three big-endian bytes at 5..8.
pub fn offset(id: u64) -> i32 {
    let b = id.to_le_bytes();
    i32::from_le_bytes([b[7], b[6], b[5], 0])
}
/// SetTDBOffset.
pub fn with_offset(id: u64, offset: i32) -> u64 {
    let mut b = id.to_le_bytes();
    let o = offset.to_le_bytes();
    b[5] = o[2];
    b[6] = o[1];
    b[7] = o[0];
    u64::from_le_bytes(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_is_crc32b() {
        assert_eq!(crc32(b"123456789", 0), 0xCBF4_3926);
    }

    #[test]
    fn a_derived_id_continues_the_base() {
        let base = from_name("Items.Foo");
        assert_eq!(derive(base, ".bar"), from_name("Items.Foo.bar"));
        assert!(is_valid(base) && !is_valid(0));
    }

    #[test]
    fn the_offset_round_trips() {
        let id = with_offset(from_name("A.b"), 0x12_3456);
        assert_eq!(offset(id), 0x12_3456);
        assert_eq!(id & 0xFF_FFFF_FFFF, from_name("A.b"));
    }
}
