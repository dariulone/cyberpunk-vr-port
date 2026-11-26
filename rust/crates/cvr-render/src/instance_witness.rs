//! THE INSTANCE RESERVATION WITNESS PATCH'S RULES (the pure part of include/Render/NativeInstanceWitness.hpp, detail):
//! the FNV-1a of the guarded native allocator (0x939ACC, 81 bytes), the 5-byte CALL rel32 from the witness site
//! (0x939B02, originally `mov rbx, [rsp+8]`) to the relay within +/-2 GiB, the allocator body that must equal the
//! original -- with exactly the patch at the site once installed -- and the site boundary no suspended thread's
//! instruction pointer may sit inside. The header forwards to these in the Rust build;
//! tools/single_pass_tests/instance_witness_test runs its cases against them.

pub const ALLOCATOR_RVA: u32 = 0x939ACC;
pub const ALLOCATOR_BYTES: usize = 81;
pub const SITE_RVA: u32 = 0x939B02;
pub const ALLOCATOR_FNV: u64 = 0x4251_259E_7ABD_2CB6;
pub const ORIGINAL_SITE: [u8; 5] = [0x48, 0x8B, 0x5C, 0x24, 0x08];

/// detail::Fnv.
pub fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(14695981039346656037u64, |h, &b| {
        (h ^ b as u64).wrapping_mul(1099511628211)
    })
}

/// detail::CallPatch: E8 rel32 from `site` to `relay`.
pub fn call_patch(site: usize, relay: usize) -> Option<[u8; 5]> {
    if site == 0 || relay == 0 || site > usize::MAX - 5 {
        return None;
    }
    let next = site + 5;
    let relative = if relay >= next {
        if relay - next > i32::MAX as usize {
            return None;
        }
        (relay - next) as u32
    } else {
        if (next - relay) as u64 > i32::MAX as u64 + 1 {
            return None;
        }
        0u32.wrapping_sub((next - relay) as u32)
    };
    let mut out = [0u8; 5];
    out[0] = 0xE8;
    out[1..].copy_from_slice(&relative.to_le_bytes());
    Some(out)
}

/// detail::Body.
pub fn body(
    original: &[u8; ALLOCATOR_BYTES],
    current: &[u8; ALLOCATOR_BYTES],
    patch: &[u8; 5],
    installed: bool,
) -> bool {
    let offset = (SITE_RVA - ALLOCATOR_RVA) as usize;
    if fnv(original) != ALLOCATOR_FNV || (installed && patch[0] != 0xE8) {
        return false;
    }
    if original[offset..offset + ORIGINAL_SITE.len()] != ORIGINAL_SITE {
        return false;
    }
    current.iter().enumerate().all(|(i, &b)| {
        b == if installed && i >= offset && i < offset + patch.len() {
            patch[i - offset]
        } else {
            original[i]
        }
    })
}

/// detail::PatchBoundary: the instruction is not inside the site's five bytes.
pub fn patch_boundary(site: usize, instruction: usize) -> bool {
    site != 0
        && site <= usize::MAX - ORIGINAL_SITE.len()
        && !(instruction > site && instruction < site + ORIGINAL_SITE.len())
}

/// detail::Fnv.
///
/// # Safety
/// `bytes` holds `count`.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iw_fnv(bytes: *const u8, count: usize) -> u64 {
    fnv(if count == 0 || bytes.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(bytes, count) }
    })
}
/// detail::CallPatch (`output` zeroed on a refusal).
///
/// # Safety
/// `output` holds five bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iw_call_patch(
    site: usize,
    relay: usize,
    output: *mut [u8; 5],
) -> bool {
    let patch = call_patch(site, relay);
    unsafe { *output = patch.unwrap_or_default() };
    patch.is_some()
}
/// detail::Body.
///
/// # Safety
/// The arrays are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_iw_body(
    original: *const [u8; ALLOCATOR_BYTES],
    current: *const [u8; ALLOCATOR_BYTES],
    patch: *const [u8; 5],
    installed: bool,
) -> bool {
    unsafe { body(&*original, &*current, &*patch, installed) }
}
/// detail::PatchBoundary.
#[no_mangle]
pub extern "C-unwind" fn cvr_iw_patch_boundary(site: usize, instruction: usize) -> bool {
    patch_boundary(site, instruction)
}
