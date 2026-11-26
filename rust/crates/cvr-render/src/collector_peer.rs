//! The CPU collector probe's peer view (src/Stereo/CpuCollectorProbe.cpp, `CreatePeer`'s view reads, `PollOwnedCpuQuery`'s
//! changed-candidate types and early-id overlap): MAIN's view for a peer query -- another view than the source, MAIN's
//! name key empty, the same size (+0x34) as the source view, and its pose (+0x2C50, 32 bytes); MAIN's own projection
//! input (aspect +0x2C78, shift +0x2C80); a changed candidate's type (the object's vtable at id - 16) and its prepare
//! entry (+0xF8), as image RVAs; and two early-id sets' overlap and their exclusive count.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_ccp_peer_view`, `cvr_ccp_main_projection`,
//! `cvr_ccp_candidate_type` and `cvr_ccp_id_overlap` (reads through ReadProcessMemory, as the .cpp's `Read`); the unit
//! tests below read fabricated memory.

use crate::instance_layout::read;

fn rd(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64, out: &mut [u8]) -> bool {
    address != 0 && read(address, out)
}
fn u64_at(read: &impl Fn(u64, &mut [u8]) -> bool, address: u64) -> Option<u64> {
    let mut b = [0u8; 8];
    rd(read, address, &mut b).then(|| u64::from_le_bytes(b))
}
/// CreatePeer's view reads: MAIN's size and pose.
pub fn peer_view(read: &impl Fn(u64, &mut [u8]) -> bool, main: u64, context: u64, size: &mut [u32; 2], pose: &mut [u8; 32]) -> bool {
    if main == 0 {
        return false;
    }
    let Some(source) = u64_at(read, context.wrapping_add(0x18)) else { return false };
    if source == main || u64_at(read, main.wrapping_add(0x28)) != Some(0) {
        return false;
    }
    let (mut main_size, mut source_size) = ([0u8; 8], [0u8; 8]);
    if !rd(read, main.wrapping_add(0x34), &mut main_size) || !rd(read, source.wrapping_add(0x34), &mut source_size) || main_size != source_size {
        return false;
    }
    *size = [u32::from_le_bytes(main_size[..4].try_into().unwrap()), u32::from_le_bytes(main_size[4..].try_into().unwrap())];
    rd(read, main.wrapping_add(0x2C50), pose)
}
/// MAIN's own projection input: aspect, shift x, shift y.
pub fn main_projection(read: &impl Fn(u64, &mut [u8]) -> bool, main: u64) -> Option<[f32; 3]> {
    let (mut aspect, mut shift) = ([0u8; 4], [0u8; 8]);
    (rd(read, main.wrapping_add(0x2C78), &mut aspect) && rd(read, main.wrapping_add(0x2C80), &mut shift)).then(|| {
        [f32::from_le_bytes(aspect), f32::from_le_bytes(shift[..4].try_into().unwrap()), f32::from_le_bytes(shift[4..].try_into().unwrap())]
    })
}
/// A changed candidate's type and prepare entry as image RVAs (0 outside the image), when readable.
pub fn candidate_type(read: &impl Fn(u64, &mut [u8]) -> bool, id: u64, base: u64) -> Option<[u32; 2]> {
    if id < 16 {
        return None;
    }
    let kind = u64_at(read, id - 16)?;
    let prepare = u64_at(read, kind.wrapping_add(0xF8))?;
    let rva = |v: u64| if v >= base && v - base <= u64::from(u32::MAX) { (v - base) as u32 } else { 0 };
    Some([rva(kind), rva(prepare)])
}
/// Two id sets' overlap and exclusive count (both sorted; the overlap a multiset intersection).
pub fn id_overlap(a: &[u64], b: &[u64]) -> (u32, u32) {
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    a.sort_unstable();
    b.sort_unstable();
    let (mut i, mut j, mut both) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            core::cmp::Ordering::Less => i += 1,
            core::cmp::Ordering::Greater => j += 1,
            core::cmp::Ordering::Equal => {
                both += 1;
                i += 1;
                j += 1;
            }
        }
    }
    (both as u32, (a.len() + b.len() - both * 2) as u32)
}

// ---- the C ABI

/// # Safety
/// `size` holds two values and `pose` 32 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_peer_view(main: u64, context: u64, size: *mut [u32; 2], pose: *mut [u8; 32]) -> bool {
    unsafe { peer_view(&read, main, context, &mut *size, &mut *pose) }
}
/// # Safety
/// `out` holds three floats (written only when read).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_main_projection(main: u64, out: *mut [f32; 3]) -> bool {
    match main_projection(&read, main) {
        Some(p) => {
            unsafe { *out = p };
            true
        }
        None => false,
    }
}
/// # Safety
/// `out` holds two values (written only when read).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_candidate_type(id: u64, base: u64, out: *mut [u32; 2]) -> bool {
    match candidate_type(&read, id, base) {
        Some(t) => {
            unsafe { *out = t };
            true
        }
        None => false,
    }
}
/// # Safety
/// `a` holds `na` ids, `b` `nb`; `out` two values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_ccp_id_overlap(a: *const u64, na: usize, b: *const u64, nb: usize, out: *mut [u32; 2]) {
    let slice = |p: *const u64, n: usize| if p.is_null() || n == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(p, n) } };
    let (both, exclusive) = id_overlap(slice(a, na), slice(b, nb));
    unsafe { *out = [both, exclusive] };
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
    fn world() -> (Memory, u64, u64) {
        let mut m = Memory(BTreeMap::new());
        let (main, context, source) = (0x10_0000u64, 0x20_0000u64, 0x30_0000u64);
        m.put(context + 0x18, &source.to_le_bytes());
        m.put(main + 0x28, &0u64.to_le_bytes());
        let size = [1920u32.to_le_bytes(), 1080u32.to_le_bytes()].concat();
        m.put(main + 0x34, &size);
        m.put(source + 0x34, &size);
        m.put(main + 0x2C50, &[4u8; 32]);
        (m, main, context)
    }
    #[test]
    fn views() {
        let (m, main, context) = world();
        let (mut size, mut pose) = ([0u32; 2], [0u8; 32]);
        assert!(peer_view(&m.reader(), main, context, &mut size, &mut pose));
        assert_eq!((size, pose[31]), ([1920, 1080], 4));
        // A named MAIN, another size, the same view or no MAIN fails.
        let (mut m, main, context) = world();
        m.put(main + 0x28, &1u64.to_le_bytes());
        assert!(!peer_view(&m.reader(), main, context, &mut size, &mut pose));
        let (mut m, main, context) = world();
        m.put(0x30_0000 + 0x34, &[0u8; 8]);
        assert!(!peer_view(&m.reader(), main, context, &mut size, &mut pose));
        let (mut m, main, context) = world();
        m.put(context + 0x18, &main.to_le_bytes());
        assert!(!peer_view(&m.reader(), main, context, &mut size, &mut pose));
        assert!(!peer_view(&world().0.reader(), 0, context, &mut size, &mut pose));
    }
    #[test]
    fn projections_and_types() {
        let mut m = Memory(BTreeMap::new());
        m.put(0x1000 + 0x2C78, &1.5f32.to_le_bytes());
        m.put(0x1000 + 0x2C80, &[0.25f32.to_le_bytes(), (-0.5f32).to_le_bytes()].concat());
        assert_eq!(main_projection(&m.reader(), 0x1000), Some([1.5, 0.25, -0.5]));
        assert_eq!(main_projection(&m.reader(), 0x2000), None);
        let base = 0x1_4000_0000u64;
        m.put(0x5000 - 16, &(base + 0x1234).to_le_bytes());
        m.put(base + 0x1234 + 0xF8, &0x77u64.to_le_bytes());
        assert_eq!(candidate_type(&m.reader(), 0x5000, base), Some([0x1234, 0]));
        assert_eq!(candidate_type(&m.reader(), 8, base), None);
        assert_eq!(candidate_type(&m.reader(), 0x6000, base), None);
    }
    #[test]
    fn overlaps() {
        assert_eq!(id_overlap(&[3, 1, 2, 2], &[2, 4, 3]), (2, 3));
        assert_eq!(id_overlap(&[], &[1]), (0, 1));
        assert_eq!(id_overlap(&[5, 5], &[5, 5, 5]), (2, 1));
    }
}
