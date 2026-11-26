//! The CPU collector probe's game-code contracts (src/Stereo/CpuCollectorProbe.cpp, `Compatible` and
//! `QueryCompatible`): each function's bytes at its image RVA hash (FNV-1a 64) to the recorded value -- the owned
//! collector's lifecycle (2.5's grown collector block, its initializer's extra-projection pass and the destroyer's
//! member teardown), the query's initializer, job builder and destructor, the fine culling bodies beyond their
//! trampoline-covered prologues, the scene culling bodies, and the peer camera rebuild (2.5's off-axis branch).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ccp_code` (ia_snapshot_code's check, through
//! ReadProcessMemory; the exports, the fine-before-scene-before-peer chain and the detours stay the .cpp's); the unit
//! tests below check the tables.

use crate::ia_snapshot_code::{code_current, Code};
use crate::instance_layout::read;

const fn code(rva: u32, bytes: u32, hash: u64) -> Code {
    Code { rva, bytes, hash }
}
pub const COLLECTOR: [Code; 7] = [
    code(0x928D28, 0xB5, 0xDA81906969062833),
    code(0x11F3160, 0x2B, 0xBD3CE905B2F2F939),
    code(0x11F318C, 0x79, 0x03BC714D958D6A3E),
    code(0xC0F46C, 0x160, 0x08519173B2F5B1A8),
    code(0x11F3520, 0x64, 0x203E9A724B4FDE8B),
    code(0x11F3584, 0x2E, 0x584580F4CE6EA6C8),
    code(0x25B47B4, 0x24, 0x333311B0E83E07CC),
];
pub const QUERY: [Code; 8] = [
    code(0x5ABBD0, 0x4E7, 0xD837640E2319072C),
    code(0x2746D8, 0x12E, 0x852D109C81F4C709),
    code(0x272DB8, 0x62, 0x13B6B2B563D6DEF8),
    code(0x272F30, 0xD8, 0x2DC24D5A2D13D9E6),
    code(0x2744CC, 0x4A, 0xBF52C1744ED0577C),
    code(0x333374, 0x98, 0x6B7C1293E9814833),
    code(0x32DE14, 0x15, 0xE9AA994824C7FD8B),
    code(0x4ED568, 0x41, 0xCFCFA044ECC25870),
];
pub const FINE: [Code; 2] = [code(0x29D140, 0x338, 0x87E87660EA59BE32), code(0x19B78C, 0x67, 0xAE9FD73DCB825F0D)];
pub const SCENE: [Code; 3] = [code(0x29CEE4, 0x22C, 0x37E53D58ACD7E571), code(0x29CCEC, 0x1F7, 0x6EBE388EB49A585F), code(0xC651E4, 0x31, 0xF1919D5E26ED5C43)];
pub const CAMERA: [Code; 1] = [code(0x296EA0, 0x62D, 0xF11C4A2E7D6CBFED)];
/// The contracts by the face's index.
pub const CONTRACTS: [&[Code]; 5] = [&COLLECTOR, &QUERY, &FINE, &SCENE, &CAMERA];

// ---- the C ABI

/// `contract` indexes CONTRACTS; an unknown one is not current.
#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_code(contract: u32, base: u64) -> bool {
    CONTRACTS.get(contract as usize).is_some_and(|c| code_current(&read, base, c))
}

#[cfg(test)]
mod tests {
    //! The tables.
    use super::*;
    use crate::native_recording::fingerprint;

    #[test]
    fn tables() {
        assert_eq!(CONTRACTS.map(|c| c.len()), [7, 8, 2, 3, 1]);
        // The .cpp's buffers bound each row: 512 bytes for the collector, 2048 for the query's.
        assert!(COLLECTOR.iter().all(|c| c.bytes <= 512));
        assert!(QUERY.iter().chain(&FINE).chain(&SCENE).chain(&CAMERA).all(|c| c.bytes <= 2048));
        assert!(!cvr_ccp_code(5, 0x1000_0000));
        assert!(!cvr_ccp_code(0, 0));
    }
    #[test]
    fn rows() {
        // A row is current exactly when its bytes hash to it.
        let base = 0x10_0000u64;
        let body: Vec<u8> = (0..0x62Du32).map(|i| (i * 7) as u8).collect();
        let row = [code(0x10, 0x62D, fingerprint(&body))];
        let read = |address: u64, out: &mut [u8]| {
            if address != base + 0x10 || out.len() != body.len() {
                return false;
            }
            out.copy_from_slice(&body);
            true
        };
        assert!(code_current(&read, base, &row));
        assert!(!code_current(&read, base, &[code(0x10, 0x62D, CAMERA[0].hash)]));
    }
}
