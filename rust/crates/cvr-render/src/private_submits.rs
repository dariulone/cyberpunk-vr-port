//! Private command work of two probes: the depth comparison's direct allocator and list, and its submission
//! (src/Render/StereoDepthComparison.cpp, Begin: a direct queue of its own and a fence at 0, the recorded list's
//! resources prepared, executed, handed to the queue's flight and the fence signalled 1, the stage 13 until the list is
//! executed and 14 after); the paired geometry's readback comparison (src/Render/NativePairedGeometry.cpp, Poll: the
//! readback mapped whole, one side's planes compared, unmapped with nothing written).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp keeps the objects it is handed; the unit test below runs them on WARP.

use core::ffi::c_void;
use windows::core::{Interface, HRESULT};
use windows::Win32::Foundation::{E_INVALIDARG, S_OK};
use windows::Win32::Graphics::Direct3D12::*;

use crate::paired_geometry_checks::{compare, Counters, Plane};

/// A direct allocator and its open list, each into its slot as created; the first failure's HRESULT.
pub fn commands(device: &ID3D12Device, allocator: &mut Option<ID3D12CommandAllocator>, list: &mut Option<ID3D12GraphicsCommandList>) -> HRESULT {
    // SAFETY: a live device.
    let created = match unsafe { device.CreateCommandAllocator::<ID3D12CommandAllocator>(D3D12_COMMAND_LIST_TYPE_DIRECT) } {
        Ok(value) => value,
        Err(e) => return e.code(),
    };
    *allocator = Some(created.clone());
    // SAFETY: as above; the allocator just created.
    match unsafe { device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &created, None) } {
        Ok(value) => {
            *list = Some(value);
            S_OK
        }
        Err(e) => e.code(),
    }
}
/// The closed `list` submitted on a queue of its own, its fence signalled 1; the queue and fence into their slots as
/// created, the stage reached; the first failure's HRESULT.
pub fn submit(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, queue: &mut Option<ID3D12CommandQueue>, fence: &mut Option<ID3D12Fence>, stage: &mut u32) -> HRESULT {
    *stage = 13;
    // SAFETY: a live device; the default description: a direct queue.
    let created: ID3D12CommandQueue = match unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC::default()) } {
        Ok(value) => value,
        Err(e) => return e.code(),
    };
    *queue = Some(created.clone());
    // SAFETY: as above.
    let signal: ID3D12Fence = match unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) } {
        Ok(value) => value,
        Err(e) => return e.code(),
    };
    *fence = Some(signal.clone());
    let raw = list.as_raw();
    let flight = cvr_gpu::command_resources::prepare(&[raw]);
    *stage = 14;
    let lists = [Some(list.cast::<ID3D12CommandList>().expect("a command list"))];
    // SAFETY: a closed list of this device on its own queue.
    unsafe { created.ExecuteCommandLists(&lists) };
    cvr_gpu::command_resources::submit(Some(&created), flight);
    // SAFETY: a live queue and fence.
    match unsafe { created.Signal(&signal, 1) } {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}
/// One side's comparison of the mapped readback: 0 compared, 1 not mapped, 2 a plane outside it.
pub fn compare_readback(readback: &ID3D12Resource, planes: &[Plane], side: u32, counters: Counters) -> u32 {
    // SAFETY: a live buffer.
    let width = unsafe { readback.GetDesc() }.Width as usize;
    let mut data = core::ptr::null_mut();
    // SAFETY: a live READBACK buffer of `width`.
    if unsafe { readback.Map(0, Some(&D3D12_RANGE { Begin: 0, End: width }), Some(&mut data)) }.is_err() || data.is_null() {
        return 1;
    }
    // SAFETY: the mapping holds `width` bytes.
    let compared = compare(unsafe { core::slice::from_raw_parts(data as *const u8, width) }, planes, side, counters);
    // SAFETY: as above; nothing written.
    unsafe { readback.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
    if compared {
        0
    } else {
        2
    }
}

fn borrowed<T: Interface>(raw: *mut c_void) -> Option<T> {
    // SAFETY: a live interface of that kind or null, borrowed for the call.
    unsafe { T::from_raw_borrowed(&raw) }.cloned()
}
fn put<T: Interface>(slot: *mut *mut c_void, value: Option<T>) {
    if let Some(value) = value {
        // SAFETY: the caller's slot.
        unsafe { *slot = value.into_raw() };
    }
}

/// The allocator and list into their slots (references, as created).
///
/// # Safety
/// A live device; the slots writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_commands(device: *mut c_void, allocator: *mut *mut c_void, list: *mut *mut c_void) -> HRESULT {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return E_INVALIDARG };
    let (mut a, mut l) = (None, None);
    let hr = commands(&device, &mut a, &mut l);
    put(allocator, a);
    put(list, l);
    hr
}
/// The submission; the queue and fence into their slots (references, as created), the stage into `stage`.
///
/// # Safety
/// A live device and closed list; the slots writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sdc_submit(device: *mut c_void, list: *mut c_void, queue: *mut *mut c_void, fence: *mut *mut c_void, stage: *mut u32) -> HRESULT {
    let (Some(device), Some(list)) = (borrowed::<ID3D12Device>(device), borrowed::<ID3D12GraphicsCommandList>(list)) else { return E_INVALIDARG };
    let (mut q, mut f, mut s) = (None, None, 13);
    let hr = submit(&device, &list, &mut q, &mut f, &mut s);
    put(queue, q);
    put(fence, f);
    // SAFETY: the caller's slot.
    unsafe { *stage = s };
    hr
}
/// The readback comparison.
///
/// # Safety
/// A live readback; `planes` holds `count`; the counters hold 12, 2 and 12 values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npg_compare_readback(readback: *mut c_void, planes: *const Plane, count: usize, side: u32, different: *mut [u64; 12], bytes: *mut [u64; 2], changed: *mut [u64; 12]) -> u32 {
    let Some(readback) = borrowed::<ID3D12Resource>(readback) else { return 1 };
    let planes = if planes.is_null() || count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(planes, count) } };
    // SAFETY: as documented.
    compare_readback(&readback, planes, side, unsafe { Counters { different: &mut *different, bytes: &mut *bytes, changed: &mut *changed } })
}

#[cfg(test)]
mod tests {
    //! WARP: an allocator and list, a closed list submitted and its fence reached, a readback compared.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn private_work() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let device = device.unwrap();
        let (mut a, mut l) = (None, None);
        assert_eq!(commands(&device, &mut a, &mut l), S_OK);
        let list = l.unwrap();
        unsafe { list.Close() }.unwrap();
        let (mut q, mut f, mut stage) = (None, None, 0);
        assert_eq!(submit(&device, &list, &mut q, &mut f, &mut stage), S_OK);
        assert_eq!(stage, 14);
        let fence = f.unwrap();
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }

        let readback = crate::readback_buffers::buffer(&device, None, 256, D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST).unwrap();
        let (mut different, mut bytes, mut changed) = ([0u64; 12], [0u64; 2], [0u64; 12]);
        // SAFETY: plain data.
        let mut plane: Plane = unsafe { core::mem::zeroed() };
        plane.rows = 1;
        plane.width = 1;
        plane.stride = 4;
        plane.bytes = 4;
        for (n, layout) in plane.layout.iter_mut().enumerate() {
            layout.Offset = n as u64 * 16;
            layout.Footprint.RowPitch = 256;
        }
        assert_eq!(compare_readback(&readback, &[plane], 0, Counters { different: &mut different, bytes: &mut bytes, changed: &mut changed }), 0);
        assert_eq!(bytes[0], 4, "one texel compared");
        plane.layout[2].Offset = 1024;
        assert_eq!(compare_readback(&readback, &[plane], 0, Counters { different: &mut different, bytes: &mut bytes, changed: &mut changed }), 2, "outside the readback");
    }
}
