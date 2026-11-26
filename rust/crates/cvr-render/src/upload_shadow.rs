//! The CPU-upload shadow of single pass's geometry packets (formerly src/Render/NativeUploadShadow.cpp and the
//! UploadShadow class of include/Render/NativeUploadShadow.hpp): while one buffer is watched, the bytes the engine
//! copies into it from upload heaps are kept, range by range. Only bytes observed in a CPU upload are owned: unwritten
//! holes and GPU-to-GPU overwrites can never be read as zeros or as an earlier copy's contents, and a UAV buffer (the
//! GPU writes it) is refused outright.
use core::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::Threading::GetCurrentProcess;

/// UploadShadow::Limit.
pub const LIMIT: usize = 16 * 1024 * 1024;

/// UploadShadow: the watched buffer's bytes and the ranges of them that are known.
#[derive(Default)]
pub struct Shadow {
    size: usize,
    data: Vec<u8>,
    ranges: Vec<(usize, usize)>,
}
impl Shadow {
    pub fn reset(&mut self, bytes: usize) {
        let next = if bytes <= LIMIT { bytes } else { 0 };
        if next == 0 || next != self.size {
            self.data = Vec::new();
        }
        self.ranges.clear();
        self.size = next;
    }
    /// The bytes behind the first write (false: out of memory).
    fn ensure_data(&mut self) -> bool {
        if self.data.is_empty() && self.size != 0 {
            if self.data.try_reserve_exact(self.size).is_err() {
                return false;
            }
            self.data.resize(self.size, 0);
        }
        true
    }
    /// Write: `bytes` at `at`, now known (false: out of range). Ranges that touch are merged.
    pub fn write(&mut self, at: usize, bytes: &[u8]) -> bool {
        if at > self.size || bytes.len() > self.size - at {
            return false;
        }
        if bytes.is_empty() {
            return true;
        }
        if !self.ensure_data() {
            return false;
        }
        self.data[at..at + bytes.len()].copy_from_slice(bytes);
        self.ranges.push((at, at + bytes.len()));
        self.ranges.sort();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(self.ranges.len());
        for &r in &self.ranges {
            match merged.last_mut() {
                Some(last) if r.0 <= last.1 => last.1 = last.1.max(r.1),
                _ => merged.push(r),
            }
        }
        self.ranges = merged;
        true
    }
    /// Invalidate: the range is unknown again (all of it when out of range).
    pub fn invalidate(&mut self, at: usize, bytes: usize) {
        if at > self.size || bytes > self.size - at {
            self.ranges.clear();
            return;
        }
        let end = at + bytes;
        let mut kept = Vec::with_capacity(self.ranges.len() + 1);
        for &(first, last) in &self.ranges {
            if last <= at || first >= end {
                kept.push((first, last));
            } else {
                if first < at {
                    kept.push((first, at));
                }
                if last > end {
                    kept.push((end, last));
                }
            }
        }
        self.ranges = kept;
    }
    /// Read: `out.len()` bytes at `at`, only when one known range covers them.
    pub fn read(&self, at: usize, out: &mut [u8]) -> bool {
        let n = out.len();
        if at > self.size || n > self.size - at {
            return false;
        }
        match self.ranges.iter().find(|r| at >= r.0 && at + n <= r.1) {
            Some(_) => {
                out.copy_from_slice(&self.data[at..at + n]);
                true
            }
            None => false,
        }
    }
}

// ---- the watched buffer (upload_shadow::)

struct Watched {
    resource: usize,
    size: u64,
    rejected: bool,
    shadow: Shadow,
}
static TARGET: AtomicU64 = AtomicU64::new(0);
static WATCHED: Mutex<Watched> = Mutex::new(Watched {
    resource: 0,
    size: 0,
    rejected: false,
    shadow: Shadow {
        size: 0,
        data: Vec::new(),
        ranges: Vec::new(),
    },
});
fn watched() -> std::sync::MutexGuard<'static, Watched> {
    WATCHED.lock().unwrap_or_else(|p| p.into_inner())
}

/// Watch: the buffer at `gpu` (identified by its resource pointer) of `bytes`; nothing for a zero or one too large.
pub fn watch(gpu: u64, resource: usize, bytes: u64) {
    let mut w = watched();
    TARGET.store(0, Ordering::Release);
    w.resource = 0;
    w.size = 0;
    w.rejected = false;
    if gpu != 0 && resource != 0 && bytes != 0 && bytes <= LIMIT as u64 {
        w.resource = resource;
        w.size = bytes;
        w.shadow.reset(bytes as usize);
        TARGET.store(gpu, Ordering::Release);
    } else {
        w.shadow.reset(0);
    }
}
pub fn stop() {
    let mut w = watched();
    TARGET.store(0, Ordering::Release);
    w.shadow.reset(0);
}
pub fn active() -> bool {
    TARGET.load(Ordering::Relaxed) != 0
}
/// Invalidate: the watched buffer was overwritten by something not observed.
pub fn invalidate(destination: usize) {
    if !active() {
        return;
    }
    let mut w = watched();
    if destination == w.resource {
        let size = w.size as usize;
        w.shadow.reset(size);
    }
}
pub fn read(gpu: u64, resource: usize, offset: u64, out: &mut [u8]) -> bool {
    let w = watched();
    gpu != 0
        && gpu == TARGET.load(Ordering::Relaxed)
        && resource == w.resource
        && !w.rejected
        && offset <= usize::MAX as u64
        && w.shadow.read(offset as usize, out)
}
/// Observe: a buffer copy into the watched buffer -- its bytes owned when the source is an upload-heap buffer the CPU
/// mapping can be read from, else that range is unknown.
pub fn observe(destination: &ID3D12Resource, offset: u64, source: &ID3D12Resource, source_offset: u64, bytes: u64) {
    if !active() || bytes == 0 {
        return;
    }
    let mut w = watched();
    if destination.as_raw() as usize != w.resource || w.rejected {
        return;
    }
    let size = w.size;
    // SAFETY (whole block): the live resources; the mapped bytes are copied through ReadProcessMemory, which fails
    // rather than faults on memory that is not there.
    unsafe {
        let desc = destination.GetDesc();
        if desc.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || desc.Width != size
            || destination.GetGPUVirtualAddress() != TARGET.load(Ordering::Relaxed)
            || (desc.Flags & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS).0 != 0
        {
            w.rejected = true;
            w.shadow.reset(0);
            return;
        }
        if offset > size || bytes > size - offset {
            w.shadow.reset(size as usize);
            return;
        }
        let input = source.GetDesc();
        let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
        if input.Dimension != D3D12_RESOURCE_DIMENSION_BUFFER
            || source_offset > input.Width
            || bytes > input.Width - source_offset
            || source.GetHeapProperties(Some(&mut heap), Some(&mut flags)).is_err()
            || heap.Type != D3D12_HEAP_TYPE_UPLOAD
        {
            w.shadow.invalidate(offset as usize, bytes as usize);
            return;
        }
        let mut owned: Vec<u8> = Vec::new();
        if owned.try_reserve_exact(bytes as usize).is_err() {
            w.rejected = true;
            w.shadow.reset(0);
            return;
        }
        owned.resize(bytes as usize, 0);
        let range = D3D12_RANGE {
            Begin: source_offset as usize,
            End: (source_offset + bytes) as usize,
        };
        let mut data: *mut c_void = core::ptr::null_mut();
        if source.Map(0, Some(&range), Some(&mut data)).is_err() {
            w.shadow.invalidate(offset as usize, bytes as usize);
            return;
        }
        let none = D3D12_RANGE::default();
        if data.is_null() {
            source.Unmap(0, Some(&none));
            w.shadow.invalidate(offset as usize, bytes as usize);
            return;
        }
        let address = data as usize as u64;
        let mut read = 0usize;
        let copied = address <= u64::MAX - source_offset
            && ReadProcessMemory(
                GetCurrentProcess(),
                (address + source_offset) as usize as *const c_void,
                owned.as_mut_ptr() as *mut c_void,
                owned.len(),
                Some(&mut read),
            )
            .is_ok()
            && read == owned.len();
        source.Unmap(0, Some(&none));
        if !copied {
            w.shadow.invalidate(offset as usize, bytes as usize);
        } else if !w.shadow.write(offset as usize, &owned) {
            w.rejected = true;
            w.shadow.reset(0);
        }
    }
}

// ---- the C ABI (include/Render/NativeUploadShadow.hpp)

#[no_mangle]
pub extern "C" fn cvr_upload_shadow_new() -> *mut Shadow {
    Box::into_raw(Box::default())
}
/// # Safety
/// `s` came from cvr_upload_shadow_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_free(s: *mut Shadow) {
    if !s.is_null() {
        // SAFETY: the caller's shadow, freed once.
        drop(unsafe { Box::from_raw(s) });
    }
}
/// # Safety
/// `s` is live.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_reset(s: *mut Shadow, bytes: usize) {
    // SAFETY: the caller's shadow.
    if let Some(s) = unsafe { s.as_mut() } {
        s.reset(bytes)
    }
}
/// # Safety
/// `s` is live; `bytes` is `len` readable bytes (or null with len 0).
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_write(s: *mut Shadow, at: usize, bytes: *const u8, len: usize) -> bool {
    // SAFETY: the caller's shadow and bytes.
    unsafe {
        let data = if bytes.is_null() || len == 0 {
            &[][..]
        } else {
            core::slice::from_raw_parts(bytes, len)
        };
        s.as_mut().is_some_and(|s| s.write(at, data))
    }
}
/// # Safety
/// `s` is live.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_invalidate(s: *mut Shadow, at: usize, bytes: usize) {
    // SAFETY: the caller's shadow.
    if let Some(s) = unsafe { s.as_mut() } {
        s.invalidate(at, bytes)
    }
}
/// # Safety
/// `s` is live; `out` is `bytes` writable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_read(s: *const Shadow, at: usize, out: *mut u8, bytes: usize) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: the caller's shadow and buffer.
    unsafe { s.as_ref().is_some_and(|s| s.read(at, core::slice::from_raw_parts_mut(out, bytes))) }
}
#[no_mangle]
pub extern "C" fn cvr_upload_shadow_watch(gpu: u64, resource: u64, bytes: u64) {
    watch(gpu, resource as usize, bytes)
}
#[no_mangle]
pub extern "C" fn cvr_upload_shadow_stop() {
    stop()
}
#[no_mangle]
pub extern "C" fn cvr_upload_shadow_active() -> bool {
    active()
}
#[no_mangle]
pub extern "C" fn cvr_upload_shadow_invalidate_resource(destination: *mut c_void) {
    invalidate(destination as usize)
}
/// # Safety
/// `out` is `bytes` writable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_read_watched(gpu: u64, resource: u64, offset: u64, out: *mut u8, bytes: usize) -> bool {
    if out.is_null() {
        return false;
    }
    // SAFETY: the caller's buffer.
    read(gpu, resource as usize, offset, unsafe { core::slice::from_raw_parts_mut(out, bytes) })
}
/// # Safety
/// `destination`/`source` are ID3D12Resources or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_upload_shadow_observe(destination: *mut c_void, offset: u64, source: *mut c_void, source_offset: u64, bytes: u64) {
    // SAFETY: the caller's resources.
    unsafe {
        if let (Some(d), Some(s)) = (ID3D12Resource::from_raw_borrowed(&destination), ID3D12Resource::from_raw_borrowed(&source)) {
            observe(d, offset, s, source_offset, bytes);
        }
    }
}
