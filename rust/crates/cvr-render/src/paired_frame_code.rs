//! The paired frame's game-code contracts (src/Stereo/NativePairedFrame.cpp, `Compatible`, `GraphCompatible`,
//! `NativeSortCompatible`, `GraphJobsCompatible`, `ResourceUsesCompatible`, `ProxyGatherCompatible` and
//! `ScheduleCompatible`): each hooked or called function's bytes at its image RVA hash (FNV-1a 64) to the recorded value
//! -- the bounded paired-frame root, the owned graph snapshot, the native sort, the graph jobs' queue and destructor
//! bodies (their last row's RVA, expected and actual hash reported), the pre-compiler resource uses and their markers,
//! the proxy preparation inputs and the CPU preparation schedule.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_npf_code` and `cvr_npf_graph_jobs` (ia_snapshot_code's
//! check, through ReadProcessMemory; the environment switch, the caches and the exports stay the .cpp's); the unit tests
//! below read fabricated memory.

use crate::ia_snapshot_code::{code_current, Code};
use crate::instance_layout::read;
use crate::native_recording::fingerprint;

const fn code(rva: u32, bytes: u32, hash: u64) -> Code {
    Code { rva, bytes, hash }
}
pub const ROOT: [Code; 1] = [code(0x409DD4, 0xA00, 0xDED3369EB748ED88)];
pub const GRAPH: [Code; 1] = [code(0x547CF4, 0x5F, 0xB21BDD71777C2FE4)];
pub const SORT: [Code; 2] = [code(0x8E9B28, 0x1A2, 0xBBD50CE198B2DC60), code(0x8E9FEC, 0xDE, 0x7EDD805AFDD84978)];
pub const GRAPH_JOBS: [Code; 7] = [
    code(0x32D60C, 0x12C, 0x9C49D739DEEFCCAC),
    code(0x2744CC, 0x4A, 0xBF52C1744ED0577C),
    code(0x272B40, 0x183, 0xFDFF35624D704435),
    code(0x2746D8, 0x156, 0xA28DBE6ABEB41274),
    code(0x272DB8, 0xCA, 0x68358540D95CCC78),
    code(0x32DE14, 0x15, 0xE9AA994824C7FD8B),
    code(0x333374, 0x98, 0x6B7C1293E9814833),
];
pub const RESOURCE_USES: [Code; 1] = [code(0x3E0330, 0x1FF, 0xEBAB3552ECE2A243)];
/// 2.5 inlines 2.31's cold chunk 0x1E43FD8 into sub_1403EB51C (the last row).
pub const RESOURCE_MARKERS: [Code; 5] = [
    code(0x332D5C, 0x234, 0x9A47DEBAE35DF58B),
    code(0xB29460, 0x2A, 0x152DF25F77AF9491),
    code(0x655FAC, 0xF2, 0x2FCB059521D7B173),
    code(0x338C04, 0xAE, 0xF3B76F14B0B8A294),
    code(0x3EB51C, 0xA0E, 0xBBDE080A046F1FEF),
];
pub const PROXY_GATHER: [Code; 1] = [code(0x33020C, 0x108, 0xDED60A539B29A3F2)];
pub const SCHEDULE: [Code; 4] = [
    code(0x8EA1C8, 0x132, 0xC35ED8EB6A3F9C5D),
    code(0x32D60C, 0x12C, 0x9C49D739DEEFCCAC),
    code(0x3E8F58, 0x199, 0x80DA43519D13BA70),
    code(0x40B10C, 0x82, 0x177EF1EE2846E667),
];
/// The contracts by the face's index.
pub const CONTRACTS: [&[Code]; 7] = [&ROOT, &GRAPH, &SORT, &RESOURCE_USES, &RESOURCE_MARKERS, &PROXY_GATHER, &SCHEDULE];

/// GraphJobsCompatible's rows: `diagnostic` takes the last row's RVA, its expected hash and (once read) its actual one.
pub fn graph_jobs(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, diagnostic: &mut [u64; 3]) -> bool {
    for row in &GRAPH_JOBS {
        diagnostic[0] = row.rva.into();
        diagnostic[1] = row.hash;
        let mut bytes = vec![0u8; row.bytes as usize];
        if base == 0 || base > u64::MAX - u64::from(row.rva) || !read(base + u64::from(row.rva), &mut bytes) {
            return false;
        }
        diagnostic[2] = fingerprint(&bytes);
        if diagnostic[2] != row.hash {
            return false;
        }
    }
    true
}

// ---- the C ABI

/// `contract` indexes CONTRACTS; an unknown one is not current.
#[no_mangle]
pub extern "C-unwind" fn cvr_npf_code(contract: u32, base: u64) -> bool {
    CONTRACTS.get(contract as usize).is_some_and(|c| code_current(&read, base, c))
}
/// # Safety
/// `diagnostic` holds three values (the export's [3], [4] and [5]).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npf_graph_jobs(base: u64, diagnostic: *mut [u64; 3]) -> bool {
    graph_jobs(&read, base, unsafe { &mut *diagnostic })
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;

    fn image(rows: &[Code], corrupt: Option<usize>) -> (u64, Vec<u8>) {
        let size = rows.iter().map(|r| (r.rva + r.bytes) as usize).max().unwrap();
        let mut bytes = vec![0u8; size];
        // Unknown code: fill each row with bytes, then record the hash the contract would need.
        for (i, b) in bytes.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        if let Some(i) = corrupt {
            bytes[rows[i].rva as usize] ^= 0xFF;
        }
        (0x1000_0000, bytes)
    }
    fn reader(base: u64, bytes: Vec<u8>) -> impl Fn(u64, &mut [u8]) -> bool {
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
    fn jobs() {
        // The real hashes cannot match fabricated bytes: the first row reports its actual hash and fails.
        let (base, bytes) = image(&GRAPH_JOBS, None);
        let expected = fingerprint(&bytes[0x32D60C..0x32D60C + 0x12C]);
        let mut d = [0u64; 3];
        assert!(!graph_jobs(&reader(base, bytes), base, &mut d));
        assert_eq!(d, [0x32D60C, 0x9C49D739DEEFCCAC, expected]);
        // An unreadable row leaves its actual hash unreported.
        let mut d = [7u64; 3];
        assert!(!graph_jobs(&|_, _| false, base, &mut d));
        assert_eq!(d, [0x32D60C, 0x9C49D739DEEFCCAC, 7]);
        assert!(!graph_jobs(&|_, _| true, 0, &mut d));
    }
    #[test]
    fn contracts() {
        // Each contract is the .cpp's table.
        assert_eq!(CONTRACTS.iter().map(|c| c.len()).collect::<Vec<_>>(), [1, 1, 2, 1, 5, 1, 4]);
        assert_eq!((CONTRACTS[6][1].rva, CONTRACTS[6][1].hash), (GRAPH_JOBS[0].rva, GRAPH_JOBS[0].hash));
        // A contract whose hashes match its bytes is current; one corrupted byte is not.
        let (base, bytes) = image(&SORT, None);
        let rows: Vec<Code> = SORT.iter().map(|r| code(r.rva, r.bytes, fingerprint(&bytes[r.rva as usize..(r.rva + r.bytes) as usize]))).collect();
        assert!(code_current(&reader(base, bytes.clone()), base, &rows));
        let (_, corrupted) = image(&SORT, Some(1));
        assert!(!code_current(&reader(base, corrupted), base, &rows));
        assert!(!cvr_npf_code(7, base));
    }
}
