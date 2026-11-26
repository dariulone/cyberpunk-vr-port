//! The native instance observer's code guards (src/Render/NativeInstanceObserver.cpp,
//! `instance_observer::PrepareCodeGuards`): the ring's native code (instance_ring's NATIVE_CODE) and the observer's five
//! guarded callers must each read whole from the game image at their RVAs (at most 1024 bytes) and match their FNV
//! fingerprints.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the guard walk forwards to `cvr_nio_code_guards` (the once-only attempt, its lock
//! and the image check stay the .cpp's); the unit test below fingerprints fabricated code.

use crate::instance_layout::read;
use crate::native::instance_ring::{Code, NATIVE_CODE};
use crate::native_recording::fingerprint;

/// The observer's guarded callers.
pub const CALLERS: [Code; 5] = [
    Code { rva: 0x333C80, bytes: 0xD6, fnv: 0x625C_D726_0627_37B4 },
    Code { rva: 0x335900, bytes: 0xED, fnv: 0xF528_EEF7_7337_A62D },
    Code { rva: 0x939970, bytes: 0x10E, fnv: 0x41C7_7E4F_7FA5_3C26 },
    Code { rva: 0x2E9528C, bytes: 0xCD, fnv: 0x73EB_63DD_3808_9156 },
    Code { rva: 0x2457F1C, bytes: 0x68, fnv: 0xFAC4_991B_2FFF_FD49 },
];

/// One guard: the code read whole at `base + rva` and fingerprinted.
pub fn guard(base: u64, code: &Code) -> bool {
    let mut bytes = [0u8; 1024];
    let n = code.bytes as usize;
    n <= bytes.len() && base <= u64::MAX - code.rva as u64 - code.bytes as u64 && read(base + code.rva as u64, &mut bytes[..n]) && fingerprint(&bytes[..n]) == code.fnv
}
/// Every guard.
pub fn guards(base: u64) -> bool {
    NATIVE_CODE.iter().chain(CALLERS.iter()).all(|code| guard(base, code))
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_nio_code_guards(base: u64) -> bool {
    guards(base)
}

#[cfg(test)]
mod tests {
    //! A fabricated code block matched by its fingerprint, then changed, oversized and out of range.
    use super::*;

    #[test]
    fn fingerprints() {
        let code = vec![0x90u8; 0x40];
        let base = code.as_ptr() as u64 - 0x100;
        let good = Code { rva: 0x100, bytes: 0x40, fnv: fingerprint(&code) };
        assert!(guard(base, &good));
        assert!(!guard(base, &Code { fnv: good.fnv ^ 1, ..good }));
        assert!(!guard(base, &Code { bytes: 2048, ..good }));
        assert!(!guard(u64::MAX, &good));
        drop(code);
    }
}
