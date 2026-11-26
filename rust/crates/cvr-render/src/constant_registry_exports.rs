//! The constant upload registry's exported state (formerly definitions in src/Render/ConstantUploadRegistry.cpp),
//! defined on the Rust side and exported from rust/exported-data.txt; the C++ registry declares them in its header, the
//! tools read them by name.
#![allow(non_upper_case_globals)]
use core::sync::atomic::{AtomicU32, AtomicU64};

// ---- src/Render/ConstantUploadRegistry.cpp: the constant upload registry: its switch, state, counts and stats

#[no_mangle]
pub static CyberpunkVR_ConstantUploadCount: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadFailures: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadBytes: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadReads: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadMisses: AtomicU64 = AtomicU64::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadEvictions: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadTypes: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
#[no_mangle]
pub static CyberpunkVR_ConstantUploadNativeImports: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadNativeRejects: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantRegisteredTypes: [AtomicU32; 6] = [const { AtomicU32::new(0) }; 6];
#[no_mangle]
pub static CyberpunkVR_ConstantRegisteredRejects: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadApiHits: AtomicU32 = AtomicU32::new(0);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadFastPath: AtomicU32 = AtomicU32::new(1);
#[no_mangle]
pub static CyberpunkVR_ConstantUploadFastStats: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
#[no_mangle]
pub static mut CyberpunkVR_ConstantUploadFastStatsCount: u32 = 6;

#[cfg(test)]
mod tests {
    //! The initial values the .cpp's definitions gave.
    use super::*;
    use core::sync::atomic::Ordering::Relaxed;

    #[test]
    fn initial_values() {
        assert_eq!(CyberpunkVR_ConstantUploadFastPath.load(Relaxed), 1);
        // SAFETY: read by value; no test writes it.
        assert_eq!(unsafe { CyberpunkVR_ConstantUploadFastStatsCount }, 6);
        assert_eq!(CyberpunkVR_ConstantUploadFastStats.len(), 6);
    }
}