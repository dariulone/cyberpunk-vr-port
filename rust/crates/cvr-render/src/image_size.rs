//! The game image's size, read from its PE headers (src/Render/NativeBreadcrumbWrite.cpp, NativeCommandRecording.cpp
//! and NativeQueueResidencyScope.cpp `ImageBytes`, NativeIaBinding.cpp `NativeImageBytes`): an AMD64 PE32+ image (its
//! DOS header's e_lfanew in 1..=0x100000) whose SizeOfImage exceeds the caller's minimum -- the code it must contain --
//! and fits above the base; else 0.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_image_bytes_above` (reads through ReadProcessMemory, as the
//! .cpps' readers; each caller's minimum and cache stay its own); the unit tests below read fabricated images.

use crate::instance_layout::read;

const DOS_BYTES: usize = 64;
const NT_BYTES: usize = 264;

/// The image's size above `minimum`, else 0.
pub fn image_bytes_above(read: &impl Fn(u64, &mut [u8]) -> bool, base: u64, minimum: u64) -> u64 {
    let mut dos = [0u8; DOS_BYTES];
    if !read(base, &mut dos) || u16::from_le_bytes([dos[0], dos[1]]) != 0x5A4D {
        return 0;
    }
    let lfanew = i32::from_le_bytes(dos[0x3C..0x40].try_into().unwrap());
    if lfanew <= 0 || lfanew > 0x100000 || base > u64::MAX - lfanew as u64 {
        return 0;
    }
    let mut nt = [0u8; NT_BYTES];
    if !read(base + lfanew as u64, &mut nt)
        || u32::from_le_bytes(nt[0..4].try_into().unwrap()) != 0x4550
        || u16::from_le_bytes([nt[4], nt[5]]) != 0x8664
        || u16::from_le_bytes([nt[24], nt[25]]) != 0x20B
    {
        return 0;
    }
    let size = u64::from(u32::from_le_bytes(nt[80..84].try_into().unwrap()));
    if size <= minimum || size > u64::MAX - base {
        return 0;
    }
    size
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_image_bytes_above(base: u64, minimum: u64) -> u64 {
    image_bytes_above(&read, base, minimum)
}

#[cfg(test)]
mod tests {
    //! Fabricated images.
    use super::*;

    fn image(lfanew: i32, machine: u16, magic: u16, size: u32) -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        b[0..2].copy_from_slice(&0x5A4Du16.to_le_bytes());
        b[0x3C..0x40].copy_from_slice(&lfanew.to_le_bytes());
        if (0..0x300).contains(&lfanew) {
            let at = lfanew as usize;
            b[at..at + 4].copy_from_slice(&0x4550u32.to_le_bytes());
            b[at + 4..at + 6].copy_from_slice(&machine.to_le_bytes());
            b[at + 24..at + 26].copy_from_slice(&magic.to_le_bytes());
            b[at + 80..at + 84].copy_from_slice(&size.to_le_bytes());
        }
        b
    }
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
    fn sizes() {
        let base = 0x14000_0000u64;
        let at = |b: Vec<u8>, minimum| image_bytes_above(&memory(base, b), base, minimum);
        assert_eq!(at(image(0x100, 0x8664, 0x20B, 0x500000), 0x413AE0), 0x500000);
        assert_eq!(at(image(0x100, 0x8664, 0x20B, 0x413AE0), 0x413AE0), 0);
        assert_eq!(at(image(0x100, 0x8664, 0x20B, 0x413AE1), 0x413AE0), 0x413AE1);
        assert_eq!(at(image(0x80, 0x8664, 0x20B, 0x500000), 0), 0x500000);
        assert_eq!(at(image(0x100, 0x14C, 0x20B, 0x500000), 0), 0);
        assert_eq!(at(image(0x100, 0x8664, 0x10B, 0x500000), 0), 0);
        assert_eq!(at(image(0, 0x8664, 0x20B, 0x500000), 0), 0);
        assert_eq!(at(image(-4, 0x8664, 0x20B, 0x500000), 0), 0);
        assert_eq!(at(image(0x100001, 0x8664, 0x20B, 0x500000), 0), 0);
        let mut bad = image(0x100, 0x8664, 0x20B, 0x500000);
        bad[1] = 0;
        assert_eq!(at(bad, 0), 0);
        let mut bad = image(0x100, 0x8664, 0x20B, 0x500000);
        bad[0x100] = b'X';
        assert_eq!(at(bad, 0), 0);
        // An image that would not fit above its base.
        assert_eq!(image_bytes_above(&memory(u64::MAX - 0x4000, image(0x100, 0x8664, 0x20B, 0x500000)), u64::MAX - 0x4000, 0), 0);
    }
}
