//! The binding and preparation files' game-code contracts (src/Render/NativeSourceSrvBinding.cpp `PrepareCodeGuards`,
//! NativePreparation.cpp `PairPrepareCompatible`, NativeMaterialRead.cpp and NativeMaterialBinding.cpp `Compatible`):
//! each function's bytes at its image RVA -- within the caller's buffer -- hash (FNV-1a 64) to the recorded value; the
//! source SRV binding's nine functions (its header's table, passed in); the pair preparation's graph context prepare
//! (2.5 rewrote it and its row is not re-audited, so it never matches), its neighbour and its tail; the material read's
//! three; and the material binding's three. The material read and the source SRV binding record 0x337230's hash as
//! 0x0396F3E29AEB7926 where the camera upload and the material binding record 0x96D1A06A9C2CBA7F -- kept as written.
//! The pair preparation's first row (0x74D bytes) exceeds the .cpp's 1024-byte buffer, which the .cpp overruns; here the
//! capacity check fails it instead (the row never matches either way).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_code_rows` and `cvr_binding_code` (code_tables'
//! check, through ReadProcessMemory; the caches, the base checks and the exports stay the .cpp's); the unit tests below
//! check the tables.

use crate::code_tables::{rows_current, Row};
use crate::instance_layout::read;

const fn code(rva: u32, bytes: u32, hash: u64) -> Row {
    Row { rva, bytes, hash }
}
/// PairPrepareCompatible's regions.
pub const PREPARATION_PAIR: [Row; 3] = [code(0x4231A0, 0x74D, 0x0000000000000000), code(0x4230A0, 0xDF, 0x8660E0F8609D6EE7), code(0x516264, 0x20, 0x0AC550E5218DAFA1)];
/// The material read's Compatible.
pub const MATERIAL_READ: [Row; 3] = [code(0x337DA0, 0x180, 0xB36B2D868965D718), code(0x3E3BB4, 0x34, 0x1A0F0B6F177A422C), code(0x337230, 0x33, 0x0396F3E29AEB7926)];
/// The material binding's Compatible.
pub const MATERIAL_BINDING: [Row; 3] = [code(0x336B54, 0x1E7, 0x664356FF35FEE308), code(0x336D3C, 0xF1, 0x6B6B9044CD5F18EA), code(0x337230, 0x33, 0x96D1A06A9C2CBA7F)];
/// The tables by the faces' index, with their buffers' capacities.
pub const TABLES: [(&[Row], u32); 3] = [(&PREPARATION_PAIR, 1024), (&MATERIAL_READ, 512), (&MATERIAL_BINDING, 512)];

// ---- the C ABI

/// `table` indexes TABLES; an unknown one is not current.
#[no_mangle]
pub extern "C-unwind" fn cvr_binding_code(table: u32, base: u64) -> bool {
    TABLES.get(table as usize).is_some_and(|&(rows, capacity)| rows_current(&read, base, rows, capacity))
}

#[cfg(test)]
mod tests {
    //! The tables.
    use super::*;
    use crate::native_recording::fingerprint;

    #[test]
    fn tables() {
        assert_eq!(TABLES.map(|(rows, capacity)| (rows.len(), capacity)), [(3, 1024), (3, 512), (3, 512)]);
        // Every row fits its caller's buffer but the pair preparation's first (0x74D bytes into the .cpp's 1024-byte
        // buffer: the .cpp overruns it before its never-matching hash fails; the capacity check fails it first here).
        assert!(PREPARATION_PAIR[0].bytes > 1024);
        assert!(TABLES.iter().flat_map(|(rows, capacity)| rows.iter().map(move |r| (r, *capacity))).filter(|(r, _)| **r != PREPARATION_PAIR[0]).all(|(r, capacity)| r.bytes <= capacity));
        assert!(!cvr_binding_code(3, 0x1000_0000));
        assert!(!cvr_binding_code(1, 0));
    }
    #[test]
    fn unaudited() {
        // The pair preparation's first row records no hash: whatever its bytes, the contract is not current.
        let read = |_: u64, out: &mut [u8]| {
            out.fill(0);
            true
        };
        assert_ne!(fingerprint(&[0; 0x74D]), 0);
        assert!(!rows_current(&read, 0x1000, &PREPARATION_PAIR[..1], 1024));
    }
}
