//! The render files' game-code contracts (src/Render/NativeIndirectCopy.cpp and NativeIaBinding.cpp `Hash`,
//! NativeGeometryPackets.cpp `DrawGeometryCompatible`, NativeSourceCbvBinding.cpp `PrepareCodeGuards`): a function's
//! bytes at its image RVA -- no more than the caller's buffer holds -- hash (FNV-1a 64) to the recorded value; the draw
//! geometry's ordinary flush (its 15-byte consumer detour excluded) and skinned flush and gather; and the source CBV
//! binding's fifteen native upload, bind and flush functions.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_code_hash`, `cvr_code_rows` and `cvr_code_table` (through
//! ReadProcessMemory, as the .cpp's `Read`; the call sites' own rows, the caches and the base checks stay the .cpp's);
//! the unit tests below read fabricated memory.

use crate::instance_layout::read;
use crate::native_recording::fingerprint;

/// One code row, as the .cpps lay it out (`struct Code {uint32_t rva,bytes;uint64_t hash;}`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub rva: u32,
    pub bytes: u32,
    pub hash: u64,
}
const _: () = assert!(core::mem::size_of::<Row>() == 16);
const fn code(rva: u32, bytes: u32, hash: u64) -> Row {
    Row { rva, bytes, hash }
}

/// One function is current at `base`: within `capacity`, its range addressable and readable, its hash recorded.
pub fn hash_current(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, rva: u64, bytes: u64, expected: u64, capacity: u64) -> bool {
    if base == 0 || bytes > capacity || base > u64::MAX - rva {
        return false;
    }
    let mut code = vec![0u8; bytes as usize];
    read(base + rva, &mut code) && fingerprint(&code) == expected
}
/// Every row is current at `base`.
pub fn rows_current(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, rows: &[Row], capacity: u32) -> bool {
    base != 0 && rows.iter().all(|r| hash_current(read, base, r.rva.into(), r.bytes.into(), r.hash, capacity.into()))
}

/// DrawGeometryCompatible's ordinary flush (2.5 moved only its constant scratch).
pub const GEOMETRY_ORDINARY: [Row; 2] = [code(0x3353D8, 0x168, 0xB1E1FF97629AB0DA), code(0x334CCF, 0x707, 0x5A76D7E54F97EEC4)];
/// DrawGeometryCompatible's skinned flush and gather (2.5 reordered only the gather's local constant block).
pub const GEOMETRY_SKINNED: [Row; 2] = [code(0x333C80, 0xD8, 0x087244DA3165FD2C), code(0x335544, 0x3B9, 0x9C5CE84210616C2A)];
/// The source CBV binding's PrepareCodeGuards.
pub const SOURCE_CBV: [Row; 15] = [
    code(0x336B54, 487, 0x664356FF35FEE308),
    code(0x337968, 314, 0xB6933C2DB6992BFB),
    code(0x336D3C, 241, 0x6B6B9044CD5F18EA),
    code(0x337230, 51, 0x96D1A06A9C2CBA7F),
    code(0x333B88, 173, 0x76EEBB3582ED71F9),
    code(0x333E0C, 108, 0xDE2A0793240ED287),
    code(0x3345BC, 611, 0x4C5BA16F0EB0E34C),
    code(0x336100, 934, 0xD8D2033BD559B499),
    code(0x334328, 75, 0xF9FA501E795819C9),
    code(0x334374, 336, 0xBA3063A4D82D7932),
    code(0x3344CC, 243, 0x4C6F33AC0618E59E),
    code(0x335C3C, 1244, 0x20371793713C127F),
    code(0x3E3A0C, 423, 0xA658FD27E52F148F),
    code(0x5209FC, 296, 0x79F8D9910A55837D),
    code(0xD41788, 137, 0xFA610B5219D7D92E),
];
/// NativeIndirectCopy's Compatible: the indirect copy's eighteen functions.
pub const INDIRECT_COPY: [Row; 18] = [
    code(0x337DA0, 384, 0xADFE5FFA4DFDB190),
    code(0x3E3BB4, 54, 0xF999B875F95AD550),
    code(0x3377D0, 406, 0xCFA7FAC68D39C483),
    code(0x3DF6C8, 84, 0xE6F5013C9EE381A8),
    code(0x3DF71C, 107, 0x323015FE897D094A),
    code(0x3445AC, 1444, 0x5C372BC97D5DEB01),
    code(0x345BD0, 103, 0x175F3FAEDBA47E55),
    code(0x689590, 467, 0x3B4E4CA60342C413),
    code(0x6897C0, 836, 0x384B406967967322),
    code(0x345734, 46, 0x9B07C7789C5010D6),
    code(0x349CFC, 224, 0x44B98676710DAFF2),
    code(0x2FAAF80, 16, 0x5025DF88EF167533),
    code(0x348220, 36, 0x515FE90101A21305),
    code(0x348244, 195, 0x8DFB4CD8294CBA43),
    code(0x34AF20, 36, 0x5EC96C4652F87013),
    code(0x349F5C, 40, 0x1B58D5C1FAEB7251),
    code(0x18F3F48, 52, 0x95650F6E1318A268),
    code(0x18F3FB4, 105, 0x8550A7D22883C599),
];
/// NativeDescriptorArena's Compatible: native table materialization and its cold path, which own cursor+0x14 and use
/// precisely [base+cursor, base+capacity) before allocating a new block (2.5: the allocator sub_140419DF4 inlines its
/// 2.31 cold path sub_141EF5D8A, one whole-function row).
pub const DESCRIPTOR_ARENA: [Row; 12] = [
    code(0x335C3C, 0x4DC, 0x20371793713C127F),
    code(0x2457F84, 0xD3, 0xFDEFCB6798D5CC8B),
    code(0x419DF4, 0x138, 0x6B65A1650C2F23E4),
    code(0x3E9860, 185, 0x0990890D546C54BD),
    code(0x3E97E4, 85, 0x894B596726ACB33A),
    code(0x3E96EC, 82, 0x792C0A460B932B2E),
    code(0x3E9740, 163, 0x7DF6C8153488B6BC),
    code(0x246A84E, 111, 0xA2E05EB8DB0627E2),
    code(0x3E7D34, 106, 0x0A0DB7DBCE20E1CD),
    code(0x3E6CD4, 38, 0xB56EF3B145CCE822),
    code(0x3E7ED4, 86, 0x15A749BD01861343),
    code(0x5B3154, 72, 0x5928B74865426BF6),
];
/// The tables by the faces' index, with their buffers' capacities.
pub const TABLES: [(&[Row], u32); 5] =
    [(&GEOMETRY_ORDINARY, 2048), (&GEOMETRY_SKINNED, 2048), (&SOURCE_CBV, 2048), (&INDIRECT_COPY, 2048), (&DESCRIPTOR_ARENA, 0x500)];

/// The allocation and allocator vtables NativeIndirectCopy relies on (their RVAs), and their first five methods.
pub const ALLOCATION_VTABLE: u64 = 0x30466A8;
pub const ALLOCATOR_VTABLE: u64 = 0x306ED78;
pub const ALLOCATION_METHODS: [u64; 5] = [0x2FAC534, 0x2FAAF80, 0x348220, 0x3483A4, 0x348244];
pub const ALLOCATOR_METHODS: [u64; 5] = [0x2FAC534, 0x2FAAF80, 0x348220, 0x18F3FB4, 0x18F3F48];
/// Both vtables' first five methods are the game's.
pub fn allocator_vtables_current(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64) -> bool {
    if base > u64::MAX - ALLOCATOR_VTABLE {
        return false;
    }
    let methods = |at: u64| -> Option<[u64; 5]> {
        let mut b = [0u8; 40];
        (at != 0 && read(at, &mut b)).then(|| core::array::from_fn(|i| u64::from_le_bytes(b[i * 8..i * 8 + 8].try_into().unwrap())))
    };
    let (Some(allocation), Some(allocator)) = (methods(base + ALLOCATION_VTABLE), methods(base + ALLOCATOR_VTABLE)) else { return false };
    (0..5).all(|i| allocation[i] == base + ALLOCATION_METHODS[i] && allocator[i] == base + ALLOCATOR_METHODS[i])
}

// ---- the C ABI

/// One row (the IndirectCopy and IaBinding `Hash`).
#[no_mangle]
pub extern "C-unwind" fn cvr_code_hash(base: u64, rva: u64, bytes: u64, expected: u64, capacity: u64) -> bool {
    hash_current(&read, base, rva, bytes, expected, capacity)
}
/// # Safety
/// `rows` holds `count` rows (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_code_rows(base: u64, rows: *const Row, count: usize, capacity: u32) -> bool {
    let rows = if rows.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(rows, count) } };
    rows_current(&read, base, rows, capacity)
}
/// Grading.cpp's WantNativePacketsProbe: the native geometry packets function's prologue, before its detour.
pub const PACKETS_PROLOGUE_RVA: u64 = 0x334CC0;
pub const PACKETS_PROLOGUE: [u8; 15] = [0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x10, 0x4C, 0x89, 0x40, 0x18, 0x48, 0x89, 0x48, 0x08];
/// The prologue is the game's.
pub fn packets_prologue_current(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64) -> bool {
    let mut b = [0u8; 15];
    read(base.wrapping_add(PACKETS_PROLOGUE_RVA), &mut b) && b == PACKETS_PROLOGUE
}
#[no_mangle]
pub extern "C-unwind" fn cvr_packets_prologue(base: u64) -> bool {
    packets_prologue_current(&read, base)
}
/// NativeIndirectCopy's allocator vtable check.
#[no_mangle]
pub extern "C-unwind" fn cvr_nic_allocator_vtables(base: u64) -> bool {
    allocator_vtables_current(&read, base)
}
/// `table` indexes TABLES; an unknown one is not current.
#[no_mangle]
pub extern "C-unwind" fn cvr_code_table(table: u32, base: u64) -> bool {
    TABLES.get(table as usize).is_some_and(|&(rows, capacity)| rows_current(&read, base, rows, capacity))
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    fn memory(base: u64, bytes: Vec<u8>) -> impl Fn(u64, &mut [u8]) -> bool {
        move |address, out| {
            let Some(at) = address.checked_sub(base).map(|a| a as usize) else { return false };
            if at.checked_add(out.len()).is_none_or(|e| e > bytes.len()) {
                return false;
            }
            out.copy_from_slice(&bytes[at..at + out.len()]);
            true
        }
    }
    #[test]
    fn rows() {
        let base = 0x4000u64;
        let bytes: Vec<u8> = (0..0x400u32).map(|i| (i * 13 + 5) as u8).collect();
        let row = |rva: u32, n: u32| code(rva, n, fingerprint(&bytes[rva as usize..(rva + n) as usize]));
        let read = memory(base, bytes.clone());
        assert!(rows_current(&read, base, &[row(0x10, 0x20), row(0x100, 0x200)], 0x200));
        // Over the capacity, a wrong hash, an unreadable range or a null base.
        assert!(!rows_current(&read, base, &[row(0x100, 0x200)], 0x1FF));
        assert!(!rows_current(&read, base, &[code(0x10, 0x20, 0)], 0x200));
        assert!(!rows_current(&read, base, &[row(0x10, 0x20), code(0x3F0, 0x20, 0)], 0x200));
        assert!(!rows_current(&read, 0, &[], 0x200));
        assert!(rows_current(&read, base, &[], 0));
        assert!(!rows_current(&read, u64::MAX - 0xF, &[code(0x10, 0, fingerprint(&[]))], 0x200));
        assert!(hash_current(&read, base, 0x10, 0x20, row(0x10, 0x20).hash, 0x20));
        assert!(!hash_current(&read, base, 0x10, 0x20, row(0x10, 0x20).hash, 0x1F));
        assert!(!hash_current(&read, base, u64::MAX, 0, fingerprint(&[]), 0x20));
    }
    #[test]
    fn tables() {
        assert_eq!(TABLES.map(|(rows, capacity)| (rows.len(), capacity)), [(2, 2048), (2, 2048), (15, 2048), (18, 2048), (12, 0x500)]);
        assert!(TABLES.iter().all(|(rows, capacity)| rows.iter().all(|r| r.bytes <= *capacity)));
        assert!(!cvr_code_table(5, 0x1000_0000));
        assert!(!cvr_code_table(0, 0));
    }
    #[test]
    fn vtables() {
        let base = 0x10_0000u64;
        let mut bytes = vec![0u8; (ALLOCATOR_VTABLE + 40) as usize];
        // The game's methods, as the .cpp lists them (not the constants above).
        let game: [(u64, [u64; 5]); 2] = [
            (0x30466A8, [0x2FAC534, 0x2FAAF80, 0x348220, 0x3483A4, 0x348244]),
            (0x306ED78, [0x2FAC534, 0x2FAAF80, 0x348220, 0x18F3FB4, 0x18F3F48]),
        ];
        for (at, methods) in game {
            for (i, m) in methods.iter().enumerate() {
                let p = at as usize + i * 8;
                bytes[p..p + 8].copy_from_slice(&(base + m).to_le_bytes());
            }
        }
        assert!(allocator_vtables_current(&memory(base, bytes.clone()), base));
        // One method elsewhere, a vtable out of reach, or a base too high.
        let p = ALLOCATOR_VTABLE as usize + 3 * 8;
        let mut moved = bytes.clone();
        moved[p..p + 8].copy_from_slice(&(base + 0x18F3F48).to_le_bytes());
        assert!(!allocator_vtables_current(&memory(base, moved), base));
        assert!(!allocator_vtables_current(&memory(base, bytes[..ALLOCATOR_VTABLE as usize].to_vec()), base));
        assert!(!allocator_vtables_current(&memory(base, bytes), u64::MAX - 0x10));
    }
    #[test]
    fn prologues() {
        let base = 0x1000u64;
        let mut bytes = vec![0u8; 0x334CC0 + 15];
        bytes[0x334CC0..][..15].copy_from_slice(&[0x48, 0x8B, 0xC4, 0x48, 0x89, 0x58, 0x10, 0x4C, 0x89, 0x40, 0x18, 0x48, 0x89, 0x48, 0x08]);
        assert!(packets_prologue_current(&memory(base, bytes.clone()), base));
        let mut patched = bytes;
        patched[0x334CC0] = 0xE9;
        assert!(!packets_prologue_current(&memory(base, patched), base));
    }
}
