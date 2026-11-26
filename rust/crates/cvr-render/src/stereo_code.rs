//! The stereo hooks' game-code contracts (src/Stereo/NativeDrawOrigin.cpp `OriginCompatible`, NativeBoundsTrace.cpp
//! `Compatible` and `ParticleCompatible`, NativeSceneUpdate.cpp `PrepareCodeGuards`): each function's bytes at its
//! image RVA -- within the caller's buffer -- hash (FNV-1a 64) to the recorded value; the scoped native draw and its
//! caller, the bounds trace's culling body, the particle trace's two bodies and the scene update's twenty guards.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_stereo_code` (code_tables' check, through
//! ReadProcessMemory; the environment switches, the caches and the exports stay the .cpp's); the unit tests below check
//! the tables.

use crate::code_tables::{rows_current, Row};
use crate::instance_layout::read;

const fn code(rva: u32, bytes: u32, hash: u64) -> Row {
    Row { rva, bytes, hash }
}
/// OriginCompatible's draw and its caller.
pub const DRAW_ORIGIN: [Row; 2] = [code(0x335900, 0xF0, 0xCF41106E47A35853), code(0x3353D8, 0x168, 0xB1E1FF97629AB0DA)];
/// The bounds trace's Compatible.
pub const BOUNDS: [Row; 1] = [code(0x29ABD8, 0x1A9, 0x92FA499B9D21DB2C)];
/// ParticleCompatible's.
pub const PARTICLE: [Row; 2] = [
    code(0x29173C, 0xD7, 0x2D8AD96221D4BDBC),
    code(0x291814, 0x79F, 0x9700D29BBBBDD215),
];
/// The scene update's PrepareCodeGuards.
pub const SCENE_UPDATE: [Row; 20] = [
    code(0x3D74F8, 0x1F2, 0xB682B7BE03C4519F),
    code(0xCA64A0, 0xF9, 0xE6E5D30FBC771754),
    code(0x3D77DC, 0xF9, 0xDEB337E44385D5CC),
    code(0x3D78D8, 0xD6, 0x89E34113B71A08D2),
    code(0x3D79B0, 0xA3, 0x12DA6FCEE6CBDE7E),
    code(0x3D67C8, 0xF9, 0x13D4A20D011DC218),
    code(0x3D7AF0, 0xD4, 0x8A6E4118DB7E56AD),
    code(0x3D7BE4, 0xF3, 0xADE3DCDFB62EAD77),
    code(0xD33AB4, 0x6D, 0x81FB05338C0A8BE5),
    code(0xB67920, 0x73, 0x69E9F0C0FFF4E3FE),
    code(0x3D7A54, 0x9E, 0x2038C473ED1117D7),
    code(0x49E61C, 0x2F7, 0xCCC666F4BF24A797),
    code(0xC296F4, 0x70, 0x37D86E5EF04C79F3),
    code(0x67A92C, 0xD2, 0xDBE66A465B147828),
    code(0x4EFF98, 0x108, 0xFAFDFADDB39FE16A),
    code(0x272B40, 0x183, 0xFDFF35624D704435),
    code(0x27316C, 0x15A, 0x998B02126473A17E),
    code(0x2744CC, 0x4A, 0xBF52C1744ED0577C),
    code(0x2746D8, 0x156, 0xA28DBE6ABEB41274),
    code(0x272DB8, 0xCA, 0x68358540D95CCC78),
];
/// The tables by the faces' index, with their buffers' capacities.
pub const TABLES: [(&[Row], u32); 4] = [(&DRAW_ORIGIN, 0x168), (&BOUNDS, 0x1A9), (&PARTICLE, 2048), (&SCENE_UPDATE, 0x400)];

// ---- the C ABI

/// `table` indexes TABLES; an unknown one is not current.
#[no_mangle]
pub extern "C-unwind" fn cvr_stereo_code(table: u32, base: u64) -> bool {
    TABLES.get(table as usize).is_some_and(|&(rows, capacity)| rows_current(&read, base, rows, capacity))
}

#[cfg(test)]
mod tests {
    //! The tables.
    use super::*;

    #[test]
    fn tables() {
        assert_eq!(TABLES.map(|(rows, capacity)| (rows.len(), capacity)), [(2, 0x168), (1, 0x1A9), (2, 2048), (20, 0x400)]);
        assert!(TABLES.iter().all(|(rows, capacity)| rows.iter().all(|r| r.bytes <= *capacity)));
        assert!(!cvr_stereo_code(4, 0x1000_0000));
        assert!(!cvr_stereo_code(1, 0));
    }
}