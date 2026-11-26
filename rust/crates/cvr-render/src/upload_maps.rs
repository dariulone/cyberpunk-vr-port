//! The mapped upload heaps (formerly src/Stereo/SyncStereo.cpp's `upload_map_read` and its table, ViewConstants.cpp's
//! `upload_cpu_for_va`, and the HUD's snapshot of the table): an upload-heap resource the HUD, the view constants or the
//! censuses ask about is mapped once (read range empty), kept referenced and remembered with its CPU pointer, width and
//! GPU address, up to 64 of them (the table full said once in the stereo log); a resource that is not an upload heap,
//! or does not map, is refused and remembered in a 64-entry ring so it is not asked again soon. A GPU address with the
//! bytes it needs inside one of the mapped heaps resolves to its CPU bytes.
//!
//! In the Rust build the C++ half forwards to `cvr_upload_map_read`, `cvr_upload_cpu_for_va` and
//! `cvr_upload_maps_snapshot`; the unit tests below run the table over fake resources.
use std::sync::{Mutex, MutexGuard};

/// MappedUpload (include/Stereo/StereoInternal.hpp).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MappedUpload {
    pub res: usize,
    pub ptr: usize,
    pub size: u64,
    pub va: u64,
}
const _: () = assert!(core::mem::size_of::<MappedUpload>() == 32);
const EMPTY: MappedUpload = MappedUpload { res: 0, ptr: 0, size: 0, va: 0 };
pub const CAPACITY: usize = 64;

/// What the table asks of a resource.
pub trait Resource {
    /// Whether it lives in an upload heap (its heap properties read).
    fn upload_heap(&self) -> bool;
    /// Map(0, empty read range): the CPU pointer.
    fn map(&self) -> Option<usize>;
    /// The reference the table keeps.
    fn add_ref(&self);
    /// Its width and GPU address.
    fn width(&self) -> u64;
    fn gpu_va(&self) -> u64;
}

pub struct Table {
    maps: [MappedUpload; CAPACITY],
    n: usize,
    refused: [usize; CAPACITY],
    refused_w: usize,
    full_said: bool,
}
impl Table {
    pub const fn new() -> Table {
        Table { maps: [EMPTY; CAPACITY], n: 0, refused: [0; CAPACITY], refused_w: 0, full_said: false }
    }
    /// upload_map_read: the resource's CPU bytes, mapping it on first ask; `say` gets the table-full line.
    pub fn read(&mut self, key: usize, res: &impl Resource, say: &mut impl FnMut(String)) -> usize {
        if key == 0 {
            return 0;
        }
        if let Some(m) = self.maps[..self.n].iter().find(|m| m.res == key) {
            return m.ptr;
        }
        if self.refused.contains(&key) {
            return 0;
        }
        if self.n >= CAPACITY {
            if !self.full_said {
                self.full_said = true;
                say(format!("[hud] upload-map table full at {} rings -- no further heaps will be mapped", self.n));
            }
            return 0;
        }
        let mapped = if res.upload_heap() { res.map().filter(|&p| p != 0) } else { None };
        let Some(ptr) = mapped else {
            self.refused[self.refused_w] = key;
            self.refused_w = (self.refused_w + 1) % CAPACITY;
            return 0;
        };
        res.add_ref();
        self.maps[self.n] = MappedUpload { res: key, ptr, size: res.width(), va: res.gpu_va() };
        self.n += 1;
        ptr
    }
    /// upload_cpu_for_va: the CPU bytes for `need` bytes at a GPU address inside a mapped heap.
    pub fn cpu_for_va(&self, va: u64, need: u64) -> usize {
        self.maps[..self.n]
            .iter()
            .filter(|m| m.ptr != 0 && m.va != 0)
            .find(|m| va >= m.va && va.wrapping_add(need) <= m.va.wrapping_add(m.size))
            .map_or(0, |m| m.ptr + (va - m.va) as usize)
    }
    /// The mapped heaps, at most `out.len()`.
    pub fn snapshot(&self, out: &mut [MappedUpload]) -> usize {
        let n = self.n.min(out.len());
        out[..n].copy_from_slice(&self.maps[..n]);
        n
    }
}
impl Default for Table {
    fn default() -> Self {
        Table::new()
    }
}

static TABLE: Mutex<Table> = Mutex::new(Table::new());
fn table() -> MutexGuard<'static, Table> {
    TABLE.lock().unwrap_or_else(|e| e.into_inner())
}
/// upload_cpu_for_va over the process's table (0 when not mapped).
pub fn mapped_cpu_for_va(va: u64, need: u64) -> usize {
    table().cpu_for_va(va, need)
}

// ---- the C ABI (the Rust build: D3D12 resources and the stereo log are the DLL's)

#[cfg(feature = "rust-build")]
mod abi {
    use super::*;
    use core::ffi::{c_char, c_void};
    use windows::core::Interface;
    use windows::Win32::Graphics::Direct3D12::*;

    extern "C" {
        /// src/Stereo/Profiler.cpp: cvr::log("%s", text).
        fn cvr_cpp_stereo_log(text: *const c_char);
    }
    struct D3d<'a>(&'a ID3D12Resource);
    impl Resource for D3d<'_> {
        fn upload_heap(&self) -> bool {
            let (mut props, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
            // SAFETY: a live resource; out-parameters.
            unsafe { self.0.GetHeapProperties(Some(&mut props), Some(&mut flags)) }.is_ok() && props.Type == D3D12_HEAP_TYPE_UPLOAD
        }
        fn map(&self) -> Option<usize> {
            let mut ptr: *mut c_void = core::ptr::null_mut();
            let none = D3D12_RANGE { Begin: 0, End: 0 };
            // SAFETY: a live upload resource; an empty read range.
            unsafe { self.0.Map(0, Some(&none), Some(&mut ptr)) }.ok().map(|_| ptr as usize)
        }
        fn add_ref(&self) {
            // The table's reference, never released (the heaps live for the session).
            core::mem::forget(self.0.clone());
        }
        fn width(&self) -> u64 {
            // SAFETY: a live resource.
            unsafe { self.0.GetDesc() }.Width
        }
        fn gpu_va(&self) -> u64 {
            // SAFETY: a live buffer.
            unsafe { self.0.GetGPUVirtualAddress() }
        }
    }
    fn say(text: String) {
        let line = std::ffi::CString::new(text).unwrap_or_default();
        // SAFETY: a C string for the C++ half's log.
        unsafe { cvr_cpp_stereo_log(line.as_ptr()) };
    }

    /// # Safety
    /// `res` is null or a live ID3D12Resource.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_upload_map_read(res: *mut c_void) -> *const u8 {
        // SAFETY: as the caller vouches.
        let Some(resource) = (unsafe { ID3D12Resource::from_raw_borrowed(&res) }) else { return core::ptr::null() };
        table().read(res as usize, &D3d(resource), &mut say) as *const u8
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_upload_cpu_for_va(va: u64, need: u64) -> *const u8 {
        table().cpu_for_va(va, need) as *const u8
    }
    /// # Safety
    /// `out` holds `capacity` entries.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_upload_maps_snapshot(out: *mut MappedUpload, capacity: u32) -> u32 {
        if out.is_null() {
            return 0;
        }
        // SAFETY: as the caller vouches.
        let out = unsafe { core::slice::from_raw_parts_mut(out, capacity as usize) };
        table().snapshot(out) as u32
    }
}

#[cfg(test)]
mod tests {
    //! Fake resources.
    use super::*;
    use std::cell::Cell;

    struct Fake {
        upload: bool,
        ptr: Option<usize>,
        width: u64,
        va: u64,
        refs: Cell<u32>,
        maps: Cell<u32>,
    }
    impl Resource for Fake {
        fn upload_heap(&self) -> bool {
            self.upload
        }
        fn map(&self) -> Option<usize> {
            self.maps.set(self.maps.get() + 1);
            self.ptr
        }
        fn add_ref(&self) {
            self.refs.set(self.refs.get() + 1);
        }
        fn width(&self) -> u64 {
            self.width
        }
        fn gpu_va(&self) -> u64 {
            self.va
        }
    }
    fn fake(upload: bool, ptr: Option<usize>) -> Fake {
        Fake { upload, ptr, width: 0x1000, va: 0x10_0000, refs: Cell::new(0), maps: Cell::new(0) }
    }
    #[test]
    fn maps_once() {
        let mut t = Table::new();
        let mut said = Vec::new();
        let r = fake(true, Some(0x5000));
        assert_eq!(t.read(1, &r, &mut |s| said.push(s)), 0x5000);
        assert_eq!(t.read(1, &r, &mut |s| said.push(s)), 0x5000);
        assert_eq!((r.maps.get(), r.refs.get()), (1, 1));
        assert_eq!(t.read(0, &r, &mut |s| said.push(s)), 0);
        // A GPU address inside the heap, with the bytes it needs.
        assert_eq!(t.cpu_for_va(0x10_0010, 16), 0x5010);
        assert_eq!(t.cpu_for_va(0x10_0FF0, 16), 0x5FF0);
        assert_eq!(t.cpu_for_va(0x10_0FF8, 16), 0);
        assert_eq!(t.cpu_for_va(0xF_FFF0, 4), 0);
        let mut out = [EMPTY; 4];
        assert_eq!(t.snapshot(&mut out), 1);
        assert_eq!(out[0], MappedUpload { res: 1, ptr: 0x5000, size: 0x1000, va: 0x10_0000 });
        assert!(said.is_empty());
    }
    #[test]
    fn refuses() {
        let mut t = Table::new();
        let mut said = Vec::new();
        // Not an upload heap, or not mapped: refused, and not asked again.
        let default = fake(false, Some(1));
        assert_eq!(t.read(2, &default, &mut |s| said.push(s)), 0);
        assert_eq!(t.read(2, &fake(true, Some(9)), &mut |s| said.push(s)), 0);
        assert_eq!(default.maps.get(), 0);
        let unmapped = fake(true, None);
        assert_eq!(t.read(3, &unmapped, &mut |s| said.push(s)), 0);
        assert_eq!(t.read(3, &unmapped, &mut |s| said.push(s)), 0);
        assert_eq!((unmapped.maps.get(), unmapped.refs.get()), (1, 0));
        // The ring holds both refusals.
        assert_eq!(t.read(2, &fake(true, Some(9)), &mut |s| said.push(s)), 0);
        // The ring forgets the oldest after 64 refusals.
        for k in 100..164 {
            t.read(k, &fake(false, None), &mut |s| said.push(s));
        }
        assert_eq!(t.read(2, &fake(true, Some(0x7000)), &mut |s| said.push(s)), 0x7000);
        // A full table says so once.
        let mut full = Table::new();
        for k in 1..=64 {
            assert_ne!(full.read(k, &fake(true, Some(k * 16)), &mut |s| said.push(s)), 0);
        }
        assert_eq!(full.read(65, &fake(true, Some(1)), &mut |s| said.push(s)), 0);
        assert_eq!(full.read(66, &fake(true, Some(1)), &mut |s| said.push(s)), 0);
        assert_eq!(said, ["[hud] upload-map table full at 64 rings -- no further heaps will be mapped".to_string()]);
    }
}
