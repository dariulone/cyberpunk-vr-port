//! The native definition tracking's code guards (src/Stereo/NativeDefinitionTracking.cpp, `CodeCompatible` and
//! `SceneCodeCompatible`): the paired root's 146 guarded game functions must each read whole from the image at their RVAs
//! and match their FNV fingerprints (one, sub_142276880, was rewritten in 2.5 and is not re-audited: its zero fingerprint
//! never matches, so the guard stays closed), with the image's initializer, pool free and delete pointers on their native
//! targets and the FLS allocation and set slots on kernel32's; and the borrowed scene sections' 18 function chunks
//! (include/Render/BorrowedSceneSections.hpp's Contract, at most 1024 bytes each) must match theirs, with the scene graph
//! vtable's six slots on their fixed targets; and the definition writers' call sites (`WriterCallsitesCompatible`): every
//! builder's reset and close call, the three header copies, the three destroys and the move are near calls to their
//! targets.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the walks forward to `cvr_ndt_code`, `cvr_ndt_scene_code` and
//! `cvr_ndt_writer_callsites` (the builder sites stay the .cpp's table; the environment
//! switch, the once-only caching, the published status and kernel32's addresses stay the .cpp's); the unit tests below
//! fingerprint fabricated code. The tables are generated from the .cpp's and the header's.

use crate::instance_layout::{read, read_u64};
use crate::native::instance_ring::Code;
use crate::native_recording::fingerprint;

/// CodeCompatible's guarded functions.
pub const CODE: [Code; 146] = [
    Code { rva: 0x8E7AE0, bytes: 0x1C7, fnv: 0xC66C_F9E2_F355_BECD },
    Code { rva: 0x8E7D7C, bytes: 0xBB, fnv: 0xE5F1_4D56_0535_8CD4 },
    Code { rva: 0x8E7704, bytes: 0x31, fnv: 0xF51D_36CB_BCC6_4B9B },
    Code { rva: 0x8E90DC, bytes: 0x5F, fnv: 0x316F_5AF6_9D0D_4A4A },
    Code { rva: 0xC0F98C, bytes: 0x2D, fnv: 0x3E08_1672_73A0_2564 },
    Code { rva: 0x32DE14, bytes: 0x15, fnv: 0xE9AA_9948_24C7_FD8B },
    Code { rva: 0x2744CC, bytes: 0x4A, fnv: 0xBF52_C174_4ED0_577C },
    Code { rva: 0x2745FC, bytes: 0x1A, fnv: 0xBEF6_2AF2_C0EE_B027 },
    Code { rva: 0x273008, bytes: 0x17C, fnv: 0x7ED1_7F4B_A04A_A282 },
    Code { rva: 0xB79044, bytes: 0xC5, fnv: 0xC6D7_5E25_462E_D0F9 },
    Code { rva: 0x26E094, bytes: 0xE1, fnv: 0x2811_8627_9EB4_B7FA },
    Code { rva: 0x26D7F8, bytes: 0x19A, fnv: 0x4BC3_F855_F84B_2572 },
    Code { rva: 0x2746D8, bytes: 0x156, fnv: 0xA28D_BE6A_BEB4_1274 },
    Code { rva: 0x272DB8, bytes: 0xCA, fnv: 0x6835_8540_D95C_CC78 },
    Code { rva: 0x27449C, bytes: 0x2F, fnv: 0xD7B2_499E_3E07_7C63 },
    Code { rva: 0x3E8F58, bytes: 0x199, fnv: 0x80DA_4351_9D13_BA70 },
    Code { rva: 0xB52C98, bytes: 0xAA, fnv: 0x7058_2666_3C44_C2A3 },
    Code { rva: 0x2732D0, bytes: 0x577, fnv: 0x9177_7795_D38E_54D9 },
    Code { rva: 0x542ED4, bytes: 0x48, fnv: 0x9780_4F44_3A91_066B },
    Code { rva: 0x1413030, bytes: 0x18, fnv: 0x1C14_60B6_CC2A_E388 },
    Code { rva: 0x8E913C, bytes: 0x39, fnv: 0x4952_14B7_D32F_8F66 },
    Code { rva: 0x8E9178, bytes: 0x31, fnv: 0x5864_AB71_555A_AE4B },
    Code { rva: 0x8E9840, bytes: 0x36, fnv: 0xC96F_8470_3458_F656 },
    Code { rva: 0x8E9878, bytes: 0x41, fnv: 0xCC22_4639_CD93_45DD },
    Code { rva: 0x275084, bytes: 0x29, fnv: 0x86F3_4059_C9A7_E048 },
    Code { rva: 0x2750B0, bytes: 0x55, fnv: 0xA9D2_0973_73E5_F0CB },
    Code { rva: 0x8EA4E0, bytes: 0x39, fnv: 0xA420_FB0E_561C_9116 },
    Code { rva: 0x205C18, bytes: 0x1D, fnv: 0x569C_7BC9_FA13_A841 },
    Code { rva: 0xB60324, bytes: 0xB4, fnv: 0xF6FF_B5E0_F783_252C },
    Code { rva: 0xB603D8, bytes: 0x46, fnv: 0x449A_16CD_5B7F_6441 },
    Code { rva: 0x27316C, bytes: 0x15A, fnv: 0x998B_0212_6473_A17E },
    Code { rva: 0x547CF4, bytes: 0x5F, fnv: 0x0FDF_96C1_A140_AC78 },
    Code { rva: 0x547C04, bytes: 0xED, fnv: 0x61E2_2607_F965_F35A },
    Code { rva: 0x547B60, bytes: 0xA1, fnv: 0x4471_C53F_EB87_889F },
    Code { rva: 0xB01090, bytes: 0x8E, fnv: 0xE611_2646_279C_40C3 },
    Code { rva: 0x40AEF8, bytes: 0xD8, fnv: 0x4232_C2AC_8E04_3AAD },
    Code { rva: 0x2276760, bytes: 0x2F, fnv: 0x2406_1867_47AA_1395 },
    Code { rva: 0x2276880, bytes: 0xE5, fnv: 0x0000_0000_0000_0000 },
    Code { rva: 0xCAF31C, bytes: 0x5D, fnv: 0x33F8_7F5B_0F18_B0BF },
    Code { rva: 0x2729F4, bytes: 0x149, fnv: 0x0961_6074_1CC8_34B1 },
    Code { rva: 0x2E0400, bytes: 0xB8, fnv: 0x7047_6F89_00F1_8FCD },
    Code { rva: 0x244370E, bytes: 0x2C, fnv: 0xD5B8_A94D_943B_3AA8 },
    Code { rva: 0x2E03C8, bytes: 0x36, fnv: 0x4CFE_823B_E4F7_52E8 },
    Code { rva: 0x332F90, bytes: 0x1C8, fnv: 0x8DDE_20AE_171C_4940 },
    Code { rva: 0x4B69A0, bytes: 0x9, fnv: 0xF791_1118_1955_0B2A },
    Code { rva: 0x1FF2B0, bytes: 0x3, fnv: 0x69BC_481C_23DE_2D4D },
    Code { rva: 0x272E1C, bytes: 0x140, fnv: 0x7F8F_1FE1_2076_C7A3 },
    Code { rva: 0x274208, bytes: 0x2EA, fnv: 0xF906_7668_4D4E_AB45 },
    Code { rva: 0x8EAB98, bytes: 0x39, fnv: 0xA397_D5C4_B64F_4E46 },
    Code { rva: 0x8EA51C, bytes: 0x4B, fnv: 0xA882_CE39_EADA_1FDF },
    Code { rva: 0x274CC8, bytes: 0xAC, fnv: 0x74C6_34BA_05E9_0A46 },
    Code { rva: 0x274D74, bytes: 0xA4, fnv: 0x65BA_19E4_C49D_8107 },
    Code { rva: 0x274F88, bytes: 0xE7, fnv: 0xCB8C_134C_37BF_713D },
    Code { rva: 0x275070, bytes: 0x13, fnv: 0xB9D0_76EB_5B87_6C5F },
    Code { rva: 0x26E178, bytes: 0x6B, fnv: 0x1770_2137_77AE_C7A5 },
    Code { rva: 0x120F6F0, bytes: 0x8, fnv: 0x8D76_5EB2_DD46_2759 },
    Code { rva: 0x27681C, bytes: 0x13, fnv: 0x8FA1_16E5_4FC7_8A66 },
    Code { rva: 0x8E7E38, bytes: 0x25, fnv: 0x861B_7D1E_9FFB_23D7 },
    Code { rva: 0x8EA620, bytes: 0x25, fnv: 0x3286_83A0_9747_DEF2 },
    Code { rva: 0x190F138, bytes: 0x25, fnv: 0x3B0D_173F_B187_07B6 },
    Code { rva: 0x19159E0, bytes: 0x25, fnv: 0x479F_CFB5_F12E_F838 },
    Code { rva: 0x2276DC0, bytes: 0x2B, fnv: 0x1C07_AC7C_8988_4EA9 },
    Code { rva: 0x2276DF0, bytes: 0x2B, fnv: 0x9C03_F28C_DCCF_5FB1 },
    Code { rva: 0x275124, bytes: 0x1F2, fnv: 0x1D69_77D9_CF9A_D7B9 },
    Code { rva: 0x270174, bytes: 0x1CA, fnv: 0xF4E5_0AAD_8CCF_CE5C },
    Code { rva: 0x8E9094, bytes: 0x48, fnv: 0x69BA_D7F9_9D97_F30A },
    Code { rva: 0x8EA438, bytes: 0xA7, fnv: 0xC27F_ABA7_0A50_09FA },
    Code { rva: 0x8E9B28, bytes: 0x1A2, fnv: 0xBBD5_0CE1_98B2_DC60 },
    Code { rva: 0x8E9FEC, bytes: 0xDE, fnv: 0x7EDD_805A_FDD8_4978 },
    Code { rva: 0xB52BF0, bytes: 0xA6, fnv: 0xCD39_30C6_2B9C_9FAB },
    Code { rva: 0x8EAF50, bytes: 0x23, fnv: 0x1B0B_2543_D5BF_1DD4 },
    Code { rva: 0x8E7CA8, bytes: 0x93, fnv: 0xF4FA_CEFF_6D21_2123 },
    Code { rva: 0x8E9420, bytes: 0xDD, fnv: 0x5253_72CC_6440_7FB8 },
    Code { rva: 0x8E9500, bytes: 0xED, fnv: 0x0818_FBCD_227A_6630 },
    Code { rva: 0x8E9718, bytes: 0xC6, fnv: 0x60FC_F49C_6A0D_55C1 },
    Code { rva: 0x8E99F4, bytes: 0x54, fnv: 0x7965_1A47_FB2F_CAA8 },
    Code { rva: 0xB66B00, bytes: 0x2E, fnv: 0x8434_F6F3_6291_4F9E },
    Code { rva: 0xB66B30, bytes: 0xF8, fnv: 0xF70F_8532_69F9_5E88 },
    Code { rva: 0xBB6860, bytes: 0x92, fnv: 0x7E63_7A5A_AE0D_D739 },
    Code { rva: 0x3E0330, bytes: 0x1FF, fnv: 0xEBAB_3552_ECE2_A243 },
    Code { rva: 0x3EB51C, bytes: 0xA0E, fnv: 0xBBDE_080A_046F_1FEF },
    Code { rva: 0x334A34, bytes: 0x288, fnv: 0x9FF8_6B9A_BBC5_1087 },
    Code { rva: 0x338C04, bytes: 0xAE, fnv: 0xF3B7_6F14_B0B8_A294 },
    Code { rva: 0x333D58, bytes: 0x8B, fnv: 0xBB19_031A_6632_9502 },
    Code { rva: 0xCE7CC8, bytes: 0x11D, fnv: 0x893F_F043_1126_58D8 },
    Code { rva: 0x3EC090, bytes: 0x24B, fnv: 0x2972_4E51_2136_D3FF },
    Code { rva: 0x3DF238, bytes: 0x113, fnv: 0xFB6C_489C_9F68_B68F },
    Code { rva: 0x3DED6C, bytes: 0x4C9, fnv: 0xC733_E768_CC27_9CF2 },
    Code { rva: 0xD49950, bytes: 0x89, fnv: 0x50E1_9AEF_A093_9B05 },
    Code { rva: 0x14F3884, bytes: 0xA3, fnv: 0x8EAB_21BC_33BC_D6D4 },
    Code { rva: 0x689500, bytes: 0x90, fnv: 0x49A7_1BC0_B9C9_2D19 },
    Code { rva: 0x689590, bytes: 0x1D3, fnv: 0x3B4E_4CA6_0342_C413 },
    Code { rva: 0x3445AC, bytes: 0x5A4, fnv: 0x5C37_2BC9_7D5D_EB01 },
    Code { rva: 0x689348, bytes: 0x7D, fnv: 0xD65D_0217_E7D2_AA30 },
    Code { rva: 0xE7615C, bytes: 0xA6, fnv: 0x9068_8181_210F_A3D0 },
    Code { rva: 0x3DF434, bytes: 0xE2, fnv: 0x97E3_E68E_2846_D216 },
    Code { rva: 0x80E3E0, bytes: 0x167, fnv: 0x2D9A_DF77_C91A_7BB9 },
    Code { rva: 0x2606B44, bytes: 0x2D, fnv: 0x5491_D039_9D96_2D90 },
    Code { rva: 0x6893C8, bytes: 0x138, fnv: 0x85D3_91AC_D843_346F },
    Code { rva: 0x14F37EC, bytes: 0x9A, fnv: 0x1DBD_26F0_6DAB_FCA7 },
    Code { rva: 0x11447A8, bytes: 0x6C, fnv: 0xD416_25E6_7CEC_1B7E },
    Code { rva: 0x25EC768, bytes: 0x26, fnv: 0x0C6D_D5F1_73AF_B5C6 },
    Code { rva: 0x34B190, bytes: 0x19C, fnv: 0xAA48_2F73_BCA5_DE0C },
    Code { rva: 0x34AF64, bytes: 0x137, fnv: 0xAF2E_E82E_684B_C10A },
    Code { rva: 0x346F24, bytes: 0x303, fnv: 0x51BA_1810_B87B_452E },
    Code { rva: 0x7F1F38, bytes: 0x82, fnv: 0x732F_3226_8182_4F95 },
    Code { rva: 0x1DC4F4, bytes: 0x68, fnv: 0x80B2_06C7_C5A5_3CA0 },
    Code { rva: 0x3EC2DC, bytes: 0x88, fnv: 0x3202_F6CA_B594_AD7A },
    Code { rva: 0x246AC0A, bytes: 0x12, fnv: 0xE0FD_B846_056F_060C },
    Code { rva: 0x24580E2, bytes: 0x4D, fnv: 0x3063_AD84_F006_5AD0 },
    Code { rva: 0x40B10C, bytes: 0x82, fnv: 0x177E_F1EE_2846_E667 },
    Code { rva: 0x333374, bytes: 0x98, fnv: 0x6B7C_1293_E981_4833 },
    Code { rva: 0x2768AC, bytes: 0x146, fnv: 0x32BE_7951_9B60_1E82 },
    Code { rva: 0x276830, bytes: 0x79, fnv: 0xF0E0_CCA4_5E45_A368 },
    Code { rva: 0x96A298, bytes: 0xD1, fnv: 0x334D_9A0D_8103_271E },
    Code { rva: 0x40AFD0, bytes: 0x10E, fnv: 0x9F61_D4C8_2C29_7FEF },
    Code { rva: 0xC28A18, bytes: 0x4A, fnv: 0x6C05_F5B0_EABE_D305 },
    Code { rva: 0x2606C80, bytes: 0x8, fnv: 0x3A67_96D3_0743_903E },
    Code { rva: 0xAF0D28, bytes: 0x10C, fnv: 0x7E67_B9D4_3947_4325 },
    Code { rva: 0xB3C208, bytes: 0x85, fnv: 0x005F_3EE6_5FC0_9DF2 },
    Code { rva: 0xB62718, bytes: 0x106, fnv: 0xB952_8553_7B0A_67B1 },
    Code { rva: 0xB293DC, bytes: 0x84, fnv: 0xB433_C4F5_ECBB_D66F },
    Code { rva: 0xB538C4, bytes: 0xA8, fnv: 0xC3AB_8200_354A_E83A },
    Code { rva: 0xB3FC80, bytes: 0x7A, fnv: 0xFC48_9A11_F1BD_B31F },
    Code { rva: 0x75D30, bytes: 0x19, fnv: 0xB066_981C_B0AA_87F5 },
    Code { rva: 0x23999B0, bytes: 0x146, fnv: 0x9777_EFB3_9A46_503A },
    Code { rva: 0x260E2B4, bytes: 0xA3, fnv: 0x438D_53C7_B544_8D9E },
    Code { rva: 0x260E630, bytes: 0x170, fnv: 0xDBE4_1953_B109_AE7F },
    Code { rva: 0x260E358, bytes: 0x81, fnv: 0x0CD2_D4FC_F7DB_04C2 },
    Code { rva: 0x260E3DC, bytes: 0x32, fnv: 0x1C93_CB3C_403D_F5BE },
    Code { rva: 0x260E410, bytes: 0x34, fnv: 0xFB94_87EE_DBC9_8D19 },
    Code { rva: 0x68CBA4, bytes: 0xD0, fnv: 0x3124_93ED_B12A_1E73 },
    Code { rva: 0x68CA94, bytes: 0x37, fnv: 0x4296_27E3_1082_8B9A },
    Code { rva: 0x68CACC, bytes: 0xD7, fnv: 0x1F1D_5E67_DE62_EA8A },
    Code { rva: 0x260E9DC, bytes: 0x4C, fnv: 0x4327_F979_0B8B_14F3 },
    Code { rva: 0x260EAEC, bytes: 0x1A0, fnv: 0x55FA_4FF9_38CE_497C },
    Code { rva: 0x6A49C8, bytes: 0x54, fnv: 0x5122_8871_BDBF_4503 },
    Code { rva: 0x6A4A1C, bytes: 0x5D, fnv: 0x10B5_C92E_678B_2D4F },
    Code { rva: 0x6A4AA0, bytes: 0x74, fnv: 0xACC3_AA1B_90C7_1BEA },
    Code { rva: 0xAE356C, bytes: 0x27, fnv: 0x9E4E_EEC5_620A_9F42 },
    Code { rva: 0xAE3594, bytes: 0x5E, fnv: 0xFCDB_2CD9_6C5A_8904 },
    Code { rva: 0x6A4D00, bytes: 0x13B, fnv: 0x8C27_48DF_CA9B_BF0F },
    Code { rva: 0x24ADF34, bytes: 0x17, fnv: 0x4D8A_FCC2_9A02_4D27 },
    Code { rva: 0x6A4EEC, bytes: 0x7E, fnv: 0x23A3_CF74_16B7_3CD1 },
    Code { rva: 0x2757B0, bytes: 0x5F, fnv: 0xF995_D3B0_B9F5_3A11 },
    Code { rva: 0x6A5088, bytes: 0x51, fnv: 0x135A_A334_7C1D_E1C9 },
];
/// BorrowedSceneSections' Contract.
pub const SCENE_CONTRACT: [Code; 18] = [
    Code { rva: 0x332D5C, bytes: 564, fnv: 0x9A47_DEBA_E35D_F58B },
    Code { rva: 0x2457DD4, bytes: 13, fnv: 0x51AC_E6E3_3BC1_C950 },
    Code { rva: 0x19418E0, bytes: 11, fnv: 0x6742_FAD1_41A0_206E },
    Code { rva: 0x3E7DA0, bytes: 73, fnv: 0x3C36_DA23_B820_9163 },
    Code { rva: 0xA1FEE0, bytes: 65, fnv: 0x24F4_70AA_441F_6484 },
    Code { rva: 0x1941AC0, bytes: 6, fnv: 0x2505_EF84_B853_1C22 },
    Code { rva: 0xB538C4, bytes: 168, fnv: 0xC3AB_8200_354A_E83A },
    Code { rva: 0xB3FCFC, bytes: 175, fnv: 0x9125_9D62_4E26_FF57 },
    Code { rva: 0x252C20C, bytes: 22, fnv: 0xF5EC_9B0B_5F60_D093 },
    Code { rva: 0xB3FC80, bytes: 122, fnv: 0xB8AD_B4A3_4D84_09D9 },
    Code { rva: 0x8E913C, bytes: 57, fnv: 0x4952_14B7_D32F_8F66 },
    Code { rva: 0x24E89A6, bytes: 19, fnv: 0x4335_F9CE_9B29_5E87 },
    Code { rva: 0x8E9178, bytes: 49, fnv: 0x5864_AB71_555A_AE4B },
    Code { rva: 0x8E9840, bytes: 54, fnv: 0xC96F_8470_3458_F656 },
    Code { rva: 0x275084, bytes: 41, fnv: 0x86F3_4059_C9A7_E048 },
    Code { rva: 0x26E178, bytes: 107, fnv: 0x1770_2137_77AE_C7A5 },
    Code { rva: 0x2442FCA, bytes: 62, fnv: 0x501B_3006_237C_888A },
    Code { rva: 0x8E90DC, bytes: 95, fnv: 0x316F_5AF6_9D0D_4A4A },
];
/// BorrowedSceneSections' VtableTargets: the scene graph vtable's slots.
pub const VTABLE_TARGETS: [u64; 6] = [0x8E913C, 0x332D5C, 0x19418E0, 0x3E7DA0, 0xA1FEE0, 0x1941AC0];
/// scene_sections::VtableRva.
pub const VTABLE_RVA: u64 = 0x3094850;
/// The image pointers CodeCompatible pins: (slot RVA, target RVA).
pub const POINTERS: [(u64, u64); 3] = [(0x2FF4810, 0x75D30), (0x3572030, 0x6A5088), (0x3572038, 0x260E410)];
/// The FLS slots: FlsAlloc's and FlsSetValue's.
pub const FLS_SLOTS: [u64; 2] = [0x2FF3650, 0x2FF3658];

/// One guard read whole into a buffer of `limit` bytes.
fn guard(base: u64, code: &Code, limit: usize) -> bool {
    let mut bytes = [0u8; 0xB00];
    let n = code.bytes as usize;
    n <= limit.min(bytes.len()) && base <= u64::MAX - code.rva as u64 && read(base + code.rva as u64, &mut bytes[..n]) && fingerprint(&bytes[..n]) == code.fnv
}
/// CodeCompatible's walk: every guard, then the pinned pointers (slot, target RVAs) and the FLS slots holding `fls`
/// (kernel32's FlsAlloc and FlsSetValue).
pub fn code(base: u64, table: &[Code], pointers: &[(u64, u64)], fls_slots: &[u64; 2], fls: [u64; 2]) -> bool {
    table.iter().all(|c| guard(base, c, 0xB00))
        && pointers.iter().all(|&(slot, target)| read_u64(base.wrapping_add(slot)) == Some(base.wrapping_add(target)))
        && fls_slots.iter().zip(fls).all(|(&slot, expected)| read_u64(base.wrapping_add(slot)) == Some(expected))
}
/// SceneCodeCompatible's walk: the vtable at `vtable_rva`.
pub fn scene_code(base: u64, table: &[Code], vtable_rva: u64, targets: &[u64]) -> bool {
    base != 0
        && base <= u64::MAX - 0x3BCDCA8
        && table.iter().all(|c| c.bytes != 0 && guard(base, c, 1024))
        && targets.iter().enumerate().all(|(i, &target)| read_u64(base + vtable_rva + i as u64 * 8) == Some(base + target))
}

/// RelativeCall: the five bytes before `returned` are a near call (E8) to `target`.
pub fn relative_call(base: u64, returned: u32, target: u32) -> bool {
    let mut bytes = [0u8; 5];
    returned >= 5
        && base <= u64::MAX - u64::from(returned)
        && read(base + u64::from(returned) - 5, &mut bytes)
        && bytes[0] == 0xE8
        && i64::from(returned) + i64::from(i32::from_le_bytes(bytes[1..5].try_into().unwrap())) == i64::from(target)
}
/// WriterCallsitesCompatible's walk: every builder's reset (8E7D7C) and close (8EAF50) call (`sites` the .cpp's
/// builderSites, begin and end returns), the three header copies (8EA438), the three destroys (8E7704) and the move
/// (8E7CA8).
pub fn writer_callsites(base: u64, sites: &[[u32; 2]]) -> bool {
    sites.iter().all(|&[begin, end]| relative_call(base, begin, 0x8E7D7C) && relative_call(base, end, 0x8EAF50))
        && [0x8E9484, 0x8E957A, 0x8E9779].iter().all(|&returned| relative_call(base, returned, 0x8EA438))
        && [0x8E7C6D, 0x8E767B, 0x24E88AD].iter().all(|&returned| relative_call(base, returned, 0x8E7704))
        && relative_call(base, 0x8E7C1C, 0x8E7CA8)
}

// ---- the C ABI

/// `fls_alloc` and `fls_set` are kernel32's FlsAlloc and FlsSetValue (the .cpp requires kernel32 first).
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_code(base: u64, fls_alloc: u64, fls_set: u64) -> bool {
    code(base, &CODE, &POINTERS, &FLS_SLOTS, [fls_alloc, fls_set])
}
/// # Safety
/// `sites` holds `count` builder sites (begin and end returns).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ndt_writer_callsites(base: u64, sites: *const [u32; 2], count: usize) -> bool {
    let sites = if sites.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(sites, count) } };
    writer_callsites(base, sites)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ndt_scene_code(base: u64) -> bool {
    scene_code(base, &SCENE_CONTRACT, VTABLE_RVA, &VTABLE_TARGETS)
}

#[cfg(test)]
mod tests {
    //! Fabricated code and pointers matched by their fingerprints and targets, then changed.
    use super::*;

    #[test]
    fn tables() {
        assert_eq!(CODE.len(), 146);
        assert_eq!(CODE.iter().filter(|c| c.fnv == 0).count(), 1);
        assert!(CODE.iter().all(|c| c.bytes as usize <= 0xB00));
        assert!(SCENE_CONTRACT.iter().all(|c| c.bytes as usize <= 1024));
    }
    #[test]
    fn guards() {
        // A fabricated image: code at 0x100, pointer slots at 0x180.., FLS slots at 0x1A0...
        let mut image = vec![0u8; 0x200];
        image[0x100..0x140].fill(0x90);
        let base = image.as_ptr() as u64;
        let put = |image: &mut Vec<u8>, at: usize, value: u64| image[at..at + 8].copy_from_slice(&value.to_le_bytes());
        put(&mut image, 0x180, base + 0x100);
        put(&mut image, 0x188, base + 0x110);
        put(&mut image, 0x1A0, 0xAAAA);
        put(&mut image, 0x1A8, 0xBBBB);
        let good = Code { rva: 0x100, bytes: 0x40, fnv: fingerprint(&image[0x100..0x140]) };
        assert!(guard(base, &good, 0xB00));
        assert!(!guard(base, &Code { fnv: good.fnv ^ 1, ..good }, 0xB00));
        assert!(!guard(base, &good, 0x3F));
        assert!(scene_code(base, &[good], 0x180, &[0x100, 0x110]));
        assert!(!scene_code(base, &[good], 0x180, &[0x100, 0x111]));
        assert!(!scene_code(base, &[Code { bytes: 0, ..good }], 0x180, &[]));
        assert!(!scene_code(0, &[good], 0x180, &[]));
        assert!(!scene_code(base, &[Code { fnv: 0, ..good }], 0x180, &[]));
        let pointers = [(0x180, 0x100), (0x188, 0x110)];
        assert!(code(base, &[good], &pointers, &[0x1A0, 0x1A8], [0xAAAA, 0xBBBB]));
        assert!(!code(base, &[good], &pointers, &[0x1A0, 0x1A8], [0xAAAA, 0xBBBC]));
        assert!(!code(base, &[good], &[(0x180, 0x108)], &[0x1A0, 0x1A8], [0xAAAA, 0xBBBB]));
        assert!(!code(base, &[Code { fnv: 0, ..good }], &pointers, &[0x1A0, 0x1A8], [0xAAAA, 0xBBBB]));
    }
    #[test]
    fn calls() {
        // A call at 0x10 returning to 0x15 and reaching 0x5 (displacement -16), one at 0x40 reaching 0x1000.
        let mut image = vec![0u8; 0x100];
        image[0x10] = 0xE8;
        image[0x11..0x15].copy_from_slice(&(-16i32).to_le_bytes());
        image[0x40] = 0xE8;
        image[0x41..0x45].copy_from_slice(&(0x1000i32 - 0x45).to_le_bytes());
        let base = image.as_ptr() as u64;
        assert!(relative_call(base, 0x15, 0x5));
        assert!(relative_call(base, 0x45, 0x1000));
        assert!(!relative_call(base, 0x15, 0x6));
        assert!(!relative_call(base, 0x16, 0x6));
        assert!(!relative_call(base, 4, 0));
        image[0x10] = 0xE9;
        assert!(!relative_call(base, 0x15, 0x5));
        assert!(!writer_callsites(base, &[[0x15, 0x45]]));
    }
}
