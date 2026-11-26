//! The native stereo camera upload's checks (src/Render/NativeStereoCameraUpload.cpp, `NativeCompatible`,
//! `FlushCompatible`, `Current`, the upload results of `AllocateNative` and `Prepare`, and `BeginStages`' camera
//! rules): the uploader's and the flush's code contracts (ia_snapshot_code's FNV-1a check); the current native context's
//! command list (+0x30) and state (+0x60); an upload of the 2048-byte pair at a 256-aligned GPU address; a stage bound
//! to the caller's fresh camera handle trusted when its CBV resolves to at least a camera's bytes, and every such stage
//! shares that address and size; and the first camera dword the bound upload differs in (index, actual, expected).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ncu_native_code`, `cvr_ncu_flush_code`,
//! `cvr_ncu_context`, `cvr_ncu_uploaded`, `cvr_ncu_trusted` and `cvr_ncu_first_difference` (the native calls and the
//! cached results stay the .cpp's); the unit tests below cover each.

use crate::ia_snapshot_code::{code_current, Code};
use crate::instance_layout::read;

const fn code(rva: u32, bytes: u32, hash: u64) -> Code {
    Code { rva, bytes, hash }
}
/// NativeCompatible's contract.
pub const NATIVE: [Code; 3] = [code(0x333B88, 0xAD, 0x76EEBB3582ED71F9), code(0x336D3C, 0xF1, 0x6B6B9044CD5F18EA), code(0x337230, 0x33, 0x96D1A06A9C2CBA7F)];
/// FlushCompatible's contract.
pub const FLUSH: [Code; 2] = [code(0x335C3C, 0x4DC, 0x20371793713C127F), code(0x2457F84, 0xD3, 0xFDEFCB6798D5CC8B)];
/// StereoShaderCamera.hpp's ShaderCamera bytes.
pub const CAMERA_BYTES: usize = 848;
/// The uploaded pair's bytes.
pub const PAIR_BYTES: u32 = 2048;

/// Current's reads: the context's list and state.
pub fn context_fields(read: &impl Fn(u64, &mut [u8]) -> bool, context: u64) -> Option<(u64, u64)> {
    if context == 0 || context > u64::MAX - 0x68 {
        return None;
    }
    let at = |address: u64| {
        let mut b = [0u8; 8];
        read(address, &mut b).then(|| u64::from_le_bytes(b))
    };
    Some((at(context + 0x30)?, at(context + 0x60)?))
}
/// The upload result.
pub fn uploaded(bytes: u32, gpu: u64) -> bool {
    bytes == PAIR_BYTES && gpu != 0 && gpu & 255 == 0
}
/// BeginStages' trusted fresh handle.
#[allow(clippy::too_many_arguments)]
pub fn trusted(resolved: bool, bytes: u32, handle: u32, original: u32, address: u64, trusted_address: u64, trusted_bytes: u32) -> bool {
    resolved && bytes as usize >= CAMERA_BYTES && handle != 0 && original == handle && (trusted_address == 0 || (address == trusted_address && bytes == trusted_bytes))
}
/// The first differing camera dword: (index, actual, expected).
pub fn first_difference(current: &[u8; CAMERA_BYTES], before: &[u8; CAMERA_BYTES]) -> Option<(u32, u32, u32)> {
    let dword = |b: &[u8; CAMERA_BYTES], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap());
    (0..CAMERA_BYTES).step_by(4).find(|&i| current[i..i + 4] != before[i..i + 4]).map(|i| ((i / 4) as u32, dword(before, i), dword(current, i)))
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_native_code(base: u64) -> bool {
    code_current(&read, base, &NATIVE)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_flush_code(base: u64) -> bool {
    code_current(&read, base, &FLUSH)
}
/// # Safety
/// `list` and `state` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_context(context: u64, list: *mut u64, state: *mut u64) -> bool {
    match context_fields(&read, context) {
        Some((l, s)) => {
            unsafe { (*list, *state) = (l, s) };
            true
        }
        None => false,
    }
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_uploaded(bytes: u32, gpu: u64) -> bool {
    uploaded(bytes, gpu)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_ncu_trusted(resolved: bool, bytes: u32, handle: u32, original: u32, address: u64, trusted_address: u64, trusted_bytes: u32) -> bool {
    trusted(resolved, bytes, handle, original, address, trusted_address, trusted_bytes)
}
/// # Safety
/// Both cameras hold 848 bytes; `out` holds three values (written only on a difference).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ncu_first_difference(current: *const [u8; CAMERA_BYTES], before: *const [u8; CAMERA_BYTES], out: *mut [u64; 3]) -> bool {
    match unsafe { first_difference(&*current, &*before) } {
        Some((index, actual, expected)) => {
            unsafe { *out = [index.into(), actual.into(), expected.into()] };
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn contexts() {
        let origin = 0x1000u64;
        let mut bytes = vec![0u8; 0x80];
        bytes[0x30..0x38].copy_from_slice(&0xAAu64.to_le_bytes());
        bytes[0x60..0x68].copy_from_slice(&0xBBu64.to_le_bytes());
        let read = |address: u64, out: &mut [u8]| {
            let Some(at) = address.checked_sub(origin).map(|a| a as usize) else { return false };
            if at.checked_add(out.len()).is_none_or(|e| e > bytes.len()) {
                return false;
            }
            out.copy_from_slice(&bytes[at..at + out.len()]);
            true
        };
        assert_eq!(context_fields(&read, origin), Some((0xAA, 0xBB)));
        assert_eq!(context_fields(&read, 0), None);
        assert_eq!(context_fields(&read, u64::MAX - 0x67), None);
        // The state lies past readable memory.
        assert_eq!(context_fields(&read, origin + 0x20), None);
    }
    #[test]
    fn uploads() {
        assert!(uploaded(2048, 0x1_0000));
        assert!(!uploaded(2047, 0x1_0000));
        assert!(!uploaded(2048, 0));
        assert!(!uploaded(2048, 0x1_0080));
    }
    #[test]
    fn trust() {
        assert!(trusted(true, 1024, 7, 7, 0x100, 0, 0));
        assert!(trusted(true, 848, 7, 7, 0x100, 0x100, 848));
        assert!(!trusted(false, 1024, 7, 7, 0x100, 0, 0));
        assert!(!trusted(true, 847, 7, 7, 0x100, 0, 0));
        assert!(!trusted(true, 1024, 0, 0, 0x100, 0, 0));
        assert!(!trusted(true, 1024, 7, 8, 0x100, 0, 0));
        // A second stage must share the first one's address and size.
        assert!(!trusted(true, 1024, 7, 7, 0x200, 0x100, 1024));
        assert!(!trusted(true, 1024, 7, 7, 0x100, 0x100, 2048));
    }
    #[test]
    fn differences() {
        let current = [3u8; CAMERA_BYTES];
        assert_eq!(first_difference(&current, &current), None);
        let mut before = current;
        before[845] = 9;
        before[846] = 9;
        assert_eq!(first_difference(&current, &before), Some((211, u32::from_le_bytes([3, 9, 9, 3]), 0x0303_0303)));
        before[5] = 0;
        assert_eq!(first_difference(&current, &before), Some((1, u32::from_le_bytes([3, 0, 3, 3]), 0x0303_0303)));
    }
}
