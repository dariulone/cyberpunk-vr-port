//! The IA snapshot's game-code checks (src/Render/NativeIaSnapshot.cpp, `GeometryPoolCompatible`,
//! `PlacementCompatible`, `ImageBytes` and `AllocationOffset`'s guard): each code contract's bytes at its image RVA
//! hash (FNV-1a 64) to the recorded value; the game image's size from its PE headers (an AMD64 PE32+ image larger than
//! 0x306EC50 bytes, else 0); and the placed allocation's offset getter -- reached through the allocation's block
//! (+0x48), its metadata (+0x40), the metadata's table and the table's method (+0x30) -- lies in committed, executable,
//! unguarded memory of the game image.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ias_geometry_pool_code`, `cvr_ias_placement_code`,
//! `cvr_ias_image_bytes` and `cvr_ias_allocation_method` (reads through ReadProcessMemory, as the .cpp's `Read`; the
//! results stay cached by the .cpp and the getter's call stays there); the unit tests below read fabricated memory.

use crate::instance_layout::read;
use crate::native_recording::fingerprint;

/// One code contract.
#[derive(Clone, Copy, Debug)]
pub struct Code {
    pub rva: u32,
    pub bytes: u32,
    pub hash: u64,
}
const fn code(rva: u32, bytes: u32, hash: u64) -> Code {
    Code { rva, bytes, hash }
}
/// GeometryPoolCompatible's contract.
pub const GEOMETRY_POOL: [Code; 4] = [
    code(0x3DF71C, 0x6B, 0x323015FE897D094A),
    code(0x688F14, 0x312, 0x5A2C9974DB89880F),
    code(0x3445AC, 0x5A4, 0x5C372BC97D5DEB01),
    code(0x3490F4, 0x62, 0xAF5DD974E8F5ED23),
];
/// PlacementCompatible's contract.
pub const PLACEMENT: [Code; 2] = [code(0x349F5C, 0x26, 0xD8310396C182F071), code(0x34AF20, 0x23, 0xAFAD89E225DAE8ED)];

/// Every contract's code is current at `base`.
pub fn code_current(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, contract: &[Code]) -> bool {
    contract.iter().all(|c| {
        let mut bytes = vec![0u8; c.bytes as usize];
        base != 0 && base <= u64::MAX - u64::from(c.rva) - u64::from(c.bytes) && read(base + u64::from(c.rva), &mut bytes) && fingerprint(&bytes) == c.hash
    })
}

const DOS_BYTES: usize = 64;
const NT_BYTES: usize = 264;
/// ImageBytes.
pub fn image_bytes(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64) -> u64 {
    let mut dos = [0u8; DOS_BYTES];
    if !read(base, &mut dos) || u16::from_le_bytes([dos[0], dos[1]]) != 0x5A4D {
        return 0;
    }
    let lfanew = i32::from_le_bytes(dos[0x3C..0x40].try_into().unwrap());
    if lfanew <= 0 || lfanew > 0x100000 || base > u64::MAX - lfanew as u64 {
        return 0;
    }
    let mut nt = [0u8; NT_BYTES];
    if !read(base + lfanew as u64, &mut nt)
        || u32::from_le_bytes(nt[0..4].try_into().unwrap()) != 0x4550
        || u16::from_le_bytes([nt[4], nt[5]]) != 0x8664
        || u16::from_le_bytes([nt[24], nt[25]]) != 0x20B
    {
        return 0;
    }
    let size = u64::from(u32::from_le_bytes(nt[80..84].try_into().unwrap()));
    if size <= 0x306EC50 || size > u64::MAX - base {
        return 0;
    }
    size
}

/// A queried region: its state, type, allocation base and protection.
#[derive(Clone, Copy, Debug, Default)]
pub struct Region {
    pub state: u32,
    pub kind: u32,
    pub allocation_base: u64,
    pub protect: u32,
}
const MEM_COMMIT: u32 = 0x1000;
const MEM_IMAGE: u32 = 0x100_0000;
const PAGE_EXECUTE_ANY: u32 = 0x10 | 0x20 | 0x40 | 0x80;
const PAGE_GUARD: u32 = 0x100;
/// AllocationOffset's guard (after PlacementCompatible).
pub fn allocation_method(read: &impl Fn(u64, &mut [u8]) -> bool, query: &impl Fn(u64) -> Option<Region>, base: u64, allocation: u64) -> bool {
    let u64_at = |address: u64| {
        let mut b = [0u8; 8];
        read(address, &mut b).then(|| u64::from_le_bytes(b))
    };
    if allocation > u64::MAX - 0x50 {
        return false;
    }
    let Some(block) = u64_at(allocation + 0x48).filter(|&v| v != 0 && v <= u64::MAX - 0x48) else { return false };
    let Some(metadata) = u64_at(block + 0x40).filter(|&v| v != 0) else { return false };
    let Some(table) = u64_at(metadata).filter(|&v| v != 0 && v <= u64::MAX - 0x38) else { return false };
    let Some(method) = u64_at(table + 0x30).filter(|&v| v != 0) else { return false };
    let Some(region) = query(method) else { return false };
    region.state == MEM_COMMIT && region.kind == MEM_IMAGE && region.allocation_base == base && region.protect & PAGE_EXECUTE_ANY != 0 && region.protect & PAGE_GUARD == 0
}

fn query(address: u64) -> Option<Region> {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION};
    // SAFETY: a query about an address, into a zeroed record.
    let mut info: MEMORY_BASIC_INFORMATION = unsafe { core::mem::zeroed() };
    let written = unsafe { VirtualQuery(address as *const core::ffi::c_void, &mut info, core::mem::size_of::<MEMORY_BASIC_INFORMATION>()) };
    (written != 0).then(|| Region { state: info.State, kind: info.Type, allocation_base: info.AllocationBase as u64, protect: info.Protect })
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_ias_geometry_pool_code(base: u64) -> bool {
    code_current(&read, base, &GEOMETRY_POOL)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ias_placement_code(base: u64) -> bool {
    code_current(&read, base, &PLACEMENT)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ias_image_bytes(base: u64) -> u64 {
    image_bytes(&read, base)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ias_allocation_method(base: u64, allocation: u64) -> bool {
    allocation_method(&read, &query, base, allocation)
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    /// Memory at `origin`.
    fn memory(origin: u64, bytes: Vec<u8>) -> impl Fn(u64, &mut [u8]) -> bool {
        move |address: u64, out: &mut [u8]| {
            let Some(at) = address.checked_sub(origin) else { return false };
            let at = at as usize;
            if at.checked_add(out.len()).is_none_or(|end| end > bytes.len()) {
                return false;
            }
            out.copy_from_slice(&bytes[at..at + out.len()]);
            true
        }
    }
    #[test]
    fn codes() {
        let base = 0x1000u64;
        let mut bytes = vec![0u8; 0x100];
        bytes[0x20..0x28].copy_from_slice(b"abcdefgh");
        let contract = [code(0x20, 8, fingerprint(b"abcdefgh")), code(0x40, 4, fingerprint(&[0; 4]))];
        let read = memory(base, bytes);
        assert!(code_current(&read, base, &contract));
        assert!(!code_current(&read, base, &[code(0x20, 8, fingerprint(b"abcdefgi"))]));
        // Unreadable code, a null base or an overflowing range fails.
        assert!(!code_current(&read, base, &[code(0xFC, 8, 0)]));
        assert!(!code_current(&read, 0, &contract));
        assert!(!code_current(&read, u64::MAX - 0x24, &contract));
    }
    fn image(lfanew: i32, machine: u16, magic: u16, size: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(&0x5A4Du16.to_le_bytes());
        b[0x3C..0x40].copy_from_slice(&lfanew.to_le_bytes());
        let at = 0x100usize;
        b[at..at + 4].copy_from_slice(&0x4550u32.to_le_bytes());
        b[at + 4..at + 6].copy_from_slice(&machine.to_le_bytes());
        b[at + 24..at + 26].copy_from_slice(&magic.to_le_bytes());
        b[at + 80..at + 84].copy_from_slice(&size.to_le_bytes());
        b
    }
    #[test]
    fn images() {
        let base = 0x10000u64;
        assert_eq!(image_bytes(&memory(base, image(0x100, 0x8664, 0x20B, 0x4000000)), base), 0x4000000);
        assert_eq!(image_bytes(&memory(base, image(0x100, 0x8664, 0x20B, 0x306EC50)), base), 0);
        assert_eq!(image_bytes(&memory(base, image(0x100, 0x8664, 0x20B, 0x306EC51)), base), 0x306EC51);
        assert_eq!(image_bytes(&memory(base, image(0x100, 0x14C, 0x20B, 0x4000000)), base), 0);
        assert_eq!(image_bytes(&memory(base, image(0x100, 0x8664, 0x10B, 0x4000000)), base), 0);
        assert_eq!(image_bytes(&memory(base, image(0, 0x8664, 0x20B, 0x4000000)), base), 0);
        assert_eq!(image_bytes(&memory(base, image(0x100001, 0x8664, 0x20B, 0x4000000)), base), 0);
        let mut b = image(0x100, 0x8664, 0x20B, 0x4000000);
        b[0] = b'N';
        assert_eq!(image_bytes(&memory(base, b), base), 0);
        let mut b = image(0x100, 0x8664, 0x20B, 0x4000000);
        b[0x100] = 0;
        assert_eq!(image_bytes(&memory(base, b), base), 0);
        // The NT headers must be readable whole.
        let mut b = image(0x100, 0x8664, 0x20B, 0x4000000);
        b.truncate(0x100 + NT_BYTES - 1);
        assert_eq!(image_bytes(&memory(base, b), base), 0);
    }
    #[test]
    fn methods() {
        let origin = 0x1000u64;
        let mut b = vec![0u8; 0x200];
        let put = |b: &mut Vec<u8>, at: u64, v: u64| b[(at - origin) as usize..(at - origin) as usize + 8].copy_from_slice(&v.to_le_bytes());
        let (allocation, block, metadata, table, method, base) = (0x1000u64, 0x1080u64, 0x1100u64, 0x1140u64, 0x7000_1234u64, 0x7000_0000u64);
        put(&mut b, allocation + 0x48, block);
        put(&mut b, block + 0x40, metadata);
        put(&mut b, metadata, table);
        put(&mut b, table + 0x30, method);
        let read = memory(origin, b.clone());
        let good = Region { state: MEM_COMMIT, kind: MEM_IMAGE, allocation_base: base, protect: 0x20 };
        let at = |region: Region| move |address: u64| (address == method).then_some(region);
        assert!(allocation_method(&read, &at(good), base, allocation));
        for bad in [
            Region { state: 0x2000, ..good },
            Region { kind: 0x2_0000, ..good },
            Region { allocation_base: base + 1, ..good },
            Region { protect: 0x04, ..good },
            Region { protect: 0x20 | PAGE_GUARD, ..good },
        ] {
            assert!(!allocation_method(&read, &at(bad), base, allocation), "{bad:?}");
        }
        assert!(!allocation_method(&read, &|_| None, base, allocation));
        assert!(!allocation_method(&read, &at(good), base, u64::MAX - 0x4F));
        // Each null link fails.
        for link in [allocation + 0x48, block + 0x40, metadata, table + 0x30] {
            let mut c = b.clone();
            put(&mut c, link, 0);
            assert!(!allocation_method(&memory(origin, c), &at(good), base, allocation), "{link:#x}");
        }
    }
}
