//! One diagnostic frame of single pass's shading-rate images (formerly src/Render/StereoVrsSnapshot.cpp and the
//! BuildFinerVrs of include/Render/StereoVrsSnapshot.hpp): each eye's R8_UINT rate image copied into a readback
//! buffer (and the left one frozen into an image of the port's), the shading-rate image unbound around the copies and
//! rebound after; read back once the source submission's fence is observed, as per-eye pixels or a difference count
//! and per-eye rate histograms. The owner retains the native source images until its fence.
use core::ffi::c_void;
use std::sync::Mutex;

use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_NOTIMPL, E_OUTOFMEMORY};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// E_PENDING (winerror.h).
const E_PENDING: HRESULT = HRESULT(0x8000000Au32 as i32);

/// BuildFinerVrs: the finer of two rate images per tile (both must hold only the valid rates 0,1,4,5,6,9,10).
pub fn build_finer(a: &[u8], b: &[u8]) -> Option<Vec<u8>> {
    if a.is_empty() || a.len() != b.len() {
        return None;
    }
    let valid = |v: u8| matches!(v, 0 | 1 | 4 | 5 | 6 | 9 | 10);
    a.iter()
        .zip(b)
        .map(|(&x, &y)| (valid(x) && valid(y)).then_some(((x >> 2).min(y >> 2) << 2) | (x & 3).min(y & 3)))
        .collect()
}

struct State {
    frozen: Option<ID3D12Resource>,
    readback: Option<ID3D12Resource>,
    desc: D3D12_RESOURCE_DESC,
    footprints: [D3D12_PLACED_SUBRESOURCE_FOOTPRINT; 2],
    bytes: u64,
    captured: u32,
}
// SAFETY: free-threaded D3D12 objects under the mutex.
unsafe impl Send for State {}

/// StereoVrsSnapshot.
pub struct Snapshot {
    state: Mutex<State>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            state: Mutex::new(State {
                frozen: None,
                readback: None,
                desc: Default::default(),
                footprints: Default::default(),
                bytes: 0,
                captured: 0,
            }),
        }
    }
}
fn transition(resource: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
                pResource: unsafe { core::mem::transmute_copy(resource) },
                Subresource: 0,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}

impl Snapshot {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }
    pub fn initialize(&self, device: &ID3D12Device, image: &ID3D12Resource) -> HRESULT {
        let mut s = self.lock();
        if s.frozen.is_some() {
            return E_INVALIDARG;
        }
        // SAFETY (whole block): the live device and image; descriptions on the stack.
        unsafe {
            s.desc = image.GetDesc();
            let d0 = s.desc;
            if d0.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
                || d0.Format != DXGI_FORMAT_R8_UINT
                || d0.MipLevels != 1
                || d0.DepthOrArraySize != 1
                || d0.SampleDesc.Count != 1
                || d0.Width == 0
                || d0.Height == 0
                || d0.Width > 512
                || d0.Height > 512
            {
                return E_NOTIMPL;
            }
            let d = D3D12_RESOURCE_DESC {
                Alignment: 0,
                Flags: D3D12_RESOURCE_FLAG_NONE,
                ..d0
            };
            let mut heap = D3D12_HEAP_PROPERTIES {
                Type: D3D12_HEAP_TYPE_DEFAULT,
                ..Default::default()
            };
            let mut frozen: Option<ID3D12Resource> = None;
            if let Err(e) = device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut frozen) {
                return e.code();
            }
            s.frozen = frozen;
            let mut size = 0u64;
            let mut first = D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default();
            device.GetCopyableFootprints(&d0, 0, 1, 0, Some(&mut first), None, None, Some(&mut size));
            let mut second = first;
            second.Offset = (size + 511) & !511;
            s.footprints = [first, second];
            s.bytes = second.Offset + size;
            let buffer = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: s.bytes,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            };
            heap.Type = D3D12_HEAP_TYPE_READBACK;
            let mut readback: Option<ID3D12Resource> = None;
            let hr = device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &buffer, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut readback);
            s.readback = readback;
            match hr {
                Ok(()) => HRESULT(0),
                Err(e) => e.code(),
            }
        }
    }
    fn retain_locked(s: &State, list: &ID3D12GraphicsCommandList) -> bool {
        crate::command_resources::keep(
            list,
            &[
                s.frozen.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw),
                s.readback.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw),
            ],
        )
    }
    pub fn retain(&self, list: &ID3D12GraphicsCommandList) -> bool {
        Self::retain_locked(&self.lock(), list)
    }
    pub fn capture(&self, list: &ID3D12GraphicsCommandList, image: &ID3D12Resource, eye: u32) -> HRESULT {
        let mut s = self.lock();
        let (Some(frozen), Some(readback)) = (s.frozen.clone(), s.readback.clone()) else {
            return E_INVALIDARG;
        };
        if eye > 1 || s.captured & (1 << eye) != 0 {
            return E_INVALIDARG;
        }
        // SAFETY (whole block): the recording list and live resources.
        unsafe {
            let d = image.GetDesc();
            if d.Dimension != s.desc.Dimension
                || d.Format != s.desc.Format
                || d.Width != s.desc.Width
                || d.Height != s.desc.Height
                || d.MipLevels != 1
                || d.DepthOrArraySize != 1
                || d.SampleDesc.Count != 1
            {
                return E_INVALIDARG;
            }
            let list5: ID3D12GraphicsCommandList5 = match list.cast() {
                Ok(l) => l,
                Err(e) => return e.code(),
            };
            if !Self::retain_locked(&s, list) || !crate::command_resources::keep(list, &[image.as_raw()]) {
                return E_OUTOFMEMORY;
            }
            list5.RSSetShadingRateImage(None);
            list.ResourceBarrier(&[transition(image, D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            let from = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(image),
                Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
            };
            let to = D3D12_TEXTURE_COPY_LOCATION {
                pResource: core::mem::transmute_copy(&readback),
                Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                    PlacedFootprint: s.footprints[eye as usize],
                },
            };
            list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            if eye == 0 {
                let to = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: core::mem::transmute_copy(&frozen),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
                };
                list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            }
            list.ResourceBarrier(&[transition(image, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE)]);
            if eye == 0 {
                list.ResourceBarrier(&[transition(&frozen, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_SHADING_RATE_SOURCE)]);
            }
            list5.RSSetShadingRateImage(image);
        }
        s.captured |= 1 << eye;
        HRESULT(0)
    }
    /// ReadEye: one eye's pixels (rows packed), after the source submission's fence.
    pub fn read_eye(&self, eye: u32, out: &mut [u8]) -> HRESULT {
        let s = self.lock();
        let Some(readback) = &s.readback else { return E_INVALIDARG };
        let (w, h) = (s.desc.Width as usize, s.desc.Height as usize);
        if eye > 1 || s.captured & (1 << eye) == 0 || out.len() != w * h {
            return E_INVALIDARG;
        }
        let mut mapped: *mut c_void = core::ptr::null_mut();
        let range = D3D12_RANGE {
            Begin: 0,
            End: s.bytes as usize,
        };
        // SAFETY (whole block): the completed readback; the mapped range holds what the reads cover.
        unsafe {
            if let Err(e) = readback.Map(0, Some(&range), Some(&mut mapped)) {
                return e.code();
            }
            if mapped.is_null() {
                return E_FAIL;
            }
            let f = &s.footprints[eye as usize];
            for y in 0..h {
                core::ptr::copy_nonoverlapping(
                    (mapped as *const u8).add(f.Offset as usize + y * f.Footprint.RowPitch as usize),
                    out.as_mut_ptr().add(y * w),
                    w,
                );
            }
            readback.Unmap(0, Some(&D3D12_RANGE::default()));
        }
        HRESULT(0)
    }
    /// Read: how many tiles differ between the eyes, and each eye's histogram of rates 0..15.
    pub fn read(&self, differences: &mut u64, histogram: &mut [[u64; 16]; 2]) -> HRESULT {
        let s = self.lock();
        let Some(readback) = &s.readback else { return E_PENDING };
        if s.captured != 3 {
            return E_PENDING;
        }
        let mut data: *mut c_void = core::ptr::null_mut();
        let range = D3D12_RANGE {
            Begin: 0,
            End: s.bytes as usize,
        };
        // SAFETY (whole block): the completed readback; the mapped range holds what the reads cover.
        unsafe {
            if let Err(e) = readback.Map(0, Some(&range), Some(&mut data)) {
                return e.code();
            }
            if data.is_null() {
                return E_FAIL;
            }
            *differences = 0;
            *histogram = [[0; 16]; 2];
            let bytes = data as *const u8;
            let [f0, f1] = &s.footprints;
            for y in 0..s.desc.Height as usize {
                for x in 0..s.desc.Width as usize {
                    let a = *bytes.add(f0.Offset as usize + y * f0.Footprint.RowPitch as usize + x);
                    let b = *bytes.add(f1.Offset as usize + y * f1.Footprint.RowPitch as usize + x);
                    *differences += (a != b) as u64;
                    if a < 16 {
                        histogram[0][a as usize] += 1;
                    }
                    if b < 16 {
                        histogram[1][b as usize] += 1;
                    }
                }
            }
            readback.Unmap(0, Some(&D3D12_RANGE::default()));
        }
        HRESULT(0)
    }
}

// ---- the C ABI (include/Render/StereoVrsSnapshot.hpp)

/// # Safety
/// `a`/`b` are `len` readable bytes (or null with len 0); `out` is `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_build_finer(a: *const u8, a_len: usize, b: *const u8, b_len: usize, out: *mut u8) -> bool {
    // SAFETY: the caller's buffers.
    unsafe {
        let s = |p: *const u8, n: usize| if p.is_null() || n == 0 { &[][..] } else { core::slice::from_raw_parts(p, n) };
        match build_finer(s(a, a_len), s(b, b_len)) {
            Some(v) if !out.is_null() => {
                core::ptr::copy_nonoverlapping(v.as_ptr(), out, v.len());
                true
            }
            _ => false,
        }
    }
}
#[no_mangle]
pub extern "C" fn cvr_vrs_snapshot_new() -> *mut Snapshot {
    Box::into_raw(Box::default())
}
/// # Safety
/// `s` came from cvr_vrs_snapshot_new (freed once) or is null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_free(s: *mut Snapshot) {
    if !s.is_null() {
        // SAFETY: the caller's object, freed once.
        drop(unsafe { Box::from_raw(s) });
    }
}
/// # Safety
/// `s` is live; the others are D3D12 objects or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_initialize(s: *const Snapshot, device: *mut c_void, image: *mut c_void) -> HRESULT {
    // SAFETY: the caller's objects.
    unsafe {
        match (s.as_ref(), ID3D12Device::from_raw_borrowed(&device), ID3D12Resource::from_raw_borrowed(&image)) {
            (Some(s), Some(d), Some(i)) => s.initialize(d, i),
            _ => E_INVALIDARG,
        }
    }
}
/// # Safety
/// As cvr_vrs_snapshot_initialize.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_capture(s: *const Snapshot, list: *mut c_void, image: *mut c_void, eye: u32) -> HRESULT {
    // SAFETY: the caller's objects.
    unsafe {
        match (
            s.as_ref(),
            ID3D12GraphicsCommandList::from_raw_borrowed(&list),
            ID3D12Resource::from_raw_borrowed(&image),
        ) {
            (Some(s), Some(l), Some(i)) => s.capture(l, i, eye),
            _ => E_INVALIDARG,
        }
    }
}
/// # Safety
/// As cvr_vrs_snapshot_initialize.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_retain(s: *const Snapshot, list: *mut c_void) -> bool {
    // SAFETY: the caller's objects.
    unsafe {
        match (s.as_ref(), ID3D12GraphicsCommandList::from_raw_borrowed(&list)) {
            (Some(s), Some(l)) => s.retain(l),
            _ => false,
        }
    }
}
/// # Safety
/// `s` is live; `out` is `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_read_eye(s: *const Snapshot, eye: u32, out: *mut u8, len: usize) -> HRESULT {
    // SAFETY: the caller's object and buffer.
    unsafe {
        match s.as_ref() {
            Some(s) if !out.is_null() => s.read_eye(eye, core::slice::from_raw_parts_mut(out, len)),
            _ => E_INVALIDARG,
        }
    }
}
/// # Safety
/// `s` is live; the out-pointers writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_read(s: *const Snapshot, differences: *mut u64, histogram: *mut [[u64; 16]; 2]) -> HRESULT {
    // SAFETY: the caller's object and out-pointers.
    unsafe {
        match (s.as_ref(), differences.as_mut(), histogram.as_mut()) {
            (Some(s), Some(d), Some(h)) => s.read(d, h),
            _ => E_INVALIDARG,
        }
    }
}
/// The frozen image (borrowed) and the size.
///
/// # Safety
/// `s` is live; the out-pointers writable or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_vrs_snapshot_info(s: *const Snapshot, frozen: *mut *mut c_void, width: *mut u32, height: *mut u32) {
    // SAFETY: the caller's object and out-pointers.
    unsafe {
        let Some(s) = s.as_ref() else { return };
        let st = s.lock();
        if let Some(o) = frozen.as_mut() {
            *o = st.frozen.as_ref().map_or(core::ptr::null_mut(), Interface::as_raw);
        }
        if let Some(o) = width.as_mut() {
            *o = st.desc.Width as u32;
        }
        if let Some(o) = height.as_mut() {
            *o = st.desc.Height;
        }
    }
}
