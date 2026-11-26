//! The CPU collector probe's query inputs (src/Stereo/CpuCollectorProbe.cpp, `QueryHook`'s view check, input reads and
//! post-start check): the query context's view (+0x18) being the VRCAM view by its name key (+0x28); the source
//! storage's four input pointers (scene, view data, view flags, camera), its mode (+0xDF2C0), the camera's 904 bytes
//! and the 0x330 bytes of view flags; and, once the query started, both unchanged.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ccp_vrcam_view`, `cvr_ccp_collector_input` and
//! `cvr_ccp_inputs_unchanged` (reads through ReadProcessMemory, as the .cpp's `Read`); the unit tests below read
//! fabricated memory.

use crate::instance_layout::read;

pub const CAMERA_BYTES: usize = 904;
pub const VIEW_FLAGS_BYTES: usize = 0x330;

fn rd(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64, out: &mut [u8]) -> bool {
    address != 0 && read(address, out)
}
fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    rd(read, address, &mut b).then(|| u64::from_le_bytes(b))
}
/// QueryHook's view check.
pub fn vrcam_view(read: &impl Fn(u64, &mut [u8]) -> bool, context: u64, key: u64) -> bool {
    u64_at(read, context.wrapping_add(0x18)).filter(|&view| view != 0).and_then(|view| u64_at(read, view.wrapping_add(0x28))) == Some(key)
}
/// QueryHook's input reads.
pub fn collector_input(
    read: &impl Fn(u64, &mut [u8]) -> bool,
    storage: u64,
    source: &mut [u64; 4],
    mode: &mut u8,
    camera: &mut [u8; CAMERA_BYTES],
    flags: &mut [u8; VIEW_FLAGS_BYTES],
) -> bool {
    let mut raw = [0u8; 32];
    if !rd(read, storage, &mut raw) {
        return false;
    }
    for (s, chunk) in source.iter_mut().zip(raw.chunks_exact(8)) {
        *s = u64::from_le_bytes(chunk.try_into().unwrap());
    }
    let mut m = [0u8; 1];
    if !rd(read, storage.wrapping_add(0xDF2C0), &mut m) {
        return false;
    }
    *mode = m[0];
    rd(read, source[3], camera) && rd(read, source[2], flags)
}
/// The post-start check: the camera and view flags read again, unchanged.
pub fn inputs_unchanged(read: &impl Fn(u64, &mut [u8]) -> bool, camera_at: u64, flags_at: u64, camera: &[u8; CAMERA_BYTES], flags: &[u8; VIEW_FLAGS_BYTES]) -> bool {
    let (mut c, mut f) = ([0u8; CAMERA_BYTES], [0u8; VIEW_FLAGS_BYTES]);
    rd(read, camera_at, &mut c) && rd(read, flags_at, &mut f) && c == *camera && f == *flags
}

// ---- the C ABI

#[no_mangle]
pub extern "C-unwind" fn cvr_ccp_vrcam_view(context: u64, key: u64) -> bool {
    vrcam_view(&read, context, key)
}
/// # Safety
/// The outputs hold 4 pointers, a byte, 904 and 0x330 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_collector_input(storage: u64, source: *mut [u64; 4], mode: *mut u8, camera: *mut [u8; CAMERA_BYTES], flags: *mut [u8; VIEW_FLAGS_BYTES]) -> bool {
    unsafe { collector_input(&read, storage, &mut *source, &mut *mode, &mut *camera, &mut *flags) }
}
/// # Safety
/// `camera` holds 904 bytes and `flags` 0x330.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_inputs_unchanged(camera_at: u64, flags_at: u64, camera: *const [u8; CAMERA_BYTES], flags: *const [u8; VIEW_FLAGS_BYTES]) -> bool {
    unsafe { inputs_unchanged(&read, camera_at, flags_at, &*camera, &*flags) }
}

#[cfg(test)]
mod tests {
    //! Fabricated memory.
    use super::*;
    use std::collections::BTreeMap;

    struct Memory(BTreeMap<u64, u8>);
    impl Memory {
        fn put(&mut self, at: u64, bytes: &[u8]) {
            for (i, &b) in bytes.iter().enumerate() {
                self.0.insert(at + i as u64, b);
            }
        }
        fn reader(&self) -> impl Fn(u64, &mut [u8]) -> bool + '_ {
            |address, out| {
                for (i, o) in out.iter_mut().enumerate() {
                    match self.0.get(&(address + i as u64)) {
                        Some(&b) => *o = b,
                        None => return false,
                    }
                }
                true
            }
        }
    }
    #[test]
    fn views() {
        let mut m = Memory(BTreeMap::new());
        m.put(0x1000 + 0x18, &0x2000u64.to_le_bytes());
        m.put(0x2000 + 0x28, &0xABCDu64.to_le_bytes());
        assert!(vrcam_view(&m.reader(), 0x1000, 0xABCD));
        assert!(!vrcam_view(&m.reader(), 0x1000, 0xABCE));
        m.put(0x1000 + 0x18, &0u64.to_le_bytes());
        assert!(!vrcam_view(&m.reader(), 0x1000, 0));
    }
    #[test]
    fn inputs() {
        let mut m = Memory(BTreeMap::new());
        let (storage, flags_at, camera_at) = (0x10_0000u64, 0x20_0000u64, 0x30_0000u64);
        let source = [0x111u64, 0x222, flags_at, camera_at];
        for (i, s) in source.iter().enumerate() {
            m.put(storage + i as u64 * 8, &s.to_le_bytes());
        }
        m.put(storage + 0xDF2C0, &[3]);
        m.put(camera_at, &[5u8; CAMERA_BYTES]);
        m.put(flags_at, &[6u8; VIEW_FLAGS_BYTES]);
        let (mut s, mut mode, mut camera, mut flags) = ([0u64; 4], 0u8, [0u8; CAMERA_BYTES], [0u8; VIEW_FLAGS_BYTES]);
        assert!(collector_input(&m.reader(), storage, &mut s, &mut mode, &mut camera, &mut flags));
        assert_eq!((s, mode, camera[903], flags[0x32F]), (source, 3, 5, 6));
        assert!(inputs_unchanged(&m.reader(), camera_at, flags_at, &camera, &flags));
        m.put(camera_at + 100, &[9]);
        assert!(!inputs_unchanged(&m.reader(), camera_at, flags_at, &camera, &flags));
        // A view-flags block that cannot be read whole fails the input.
        let mut short = Memory(BTreeMap::new());
        for (i, x) in source.iter().enumerate() {
            short.put(storage + i as u64 * 8, &x.to_le_bytes());
        }
        short.put(storage + 0xDF2C0, &[3]);
        short.put(camera_at, &[5u8; CAMERA_BYTES]);
        short.put(flags_at, &[6u8; VIEW_FLAGS_BYTES - 1]);
        assert!(!collector_input(&short.reader(), storage, &mut s, &mut mode, &mut camera, &mut flags));
    }
}
