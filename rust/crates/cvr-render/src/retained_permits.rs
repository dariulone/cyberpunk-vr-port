//! The retained geometry's instance permits (src/Render/NativeRetainedGeometry.cpp, BeforeSubmit): at an eye's
//! submission, each of its GPU instance captures (a GPU row of the side with a capture) is permitted when nothing failed
//! and its instance writer's row validated its source; the permits written into the eye's mapped predicate words (at the
//! row's predicate offset), the whole predicate published.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends the journal's rows; the unit test below runs the permits on a WARP
//! upload buffer.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// The predicate's words.
pub const WORDS: usize = 2048;

/// The permits of `side` into `predicate` (false when it cannot be mapped).
pub fn permits(predicate: &ID3D12Resource, gpu_rows: &mut [[u64; 24]], instance_rows: &[[u64; 48]], side: u64, failed: bool) -> bool {
    let mut mapped = core::ptr::null_mut();
    // SAFETY: a live UPLOAD buffer of WORDS words.
    if unsafe { predicate.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.is_err() || mapped.is_null() {
        return false;
    }
    // SAFETY: the mapping holds WORDS words.
    let words = unsafe { core::slice::from_raw_parts_mut(mapped as *mut u64, WORDS) };
    for row in gpu_rows.iter_mut().filter(|r| r[1] == side && r[2] != 0) {
        row[3] = u64::from(!failed && instance_rows.get(row[0] as usize).is_some_and(|i| i[15] != 0));
        if let Some(word) = words.get_mut((row[12] / 8) as usize) {
            *word = row[3];
        }
    }
    // SAFETY: as above.
    unsafe { predicate.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: WORDS * 8 })) };
    true
}

/// The permits (false when the predicate cannot be mapped).
///
/// # Safety
/// A live predicate; `gpu_rows` and `instance_rows` hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_permits(predicate: *mut c_void, gpu_rows: *mut [u64; 24], gpu_count: usize, instance_rows: *const [u64; 48], instance_count: usize, side: u64, failed: bool) -> bool {
    // SAFETY: as documented.
    let Some(predicate) = (unsafe { ID3D12Resource::from_raw_borrowed(&predicate) }) else { return false };
    let gpu = if gpu_rows.is_null() || gpu_count == 0 { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(gpu_rows, gpu_count) } };
    let instances = if instance_rows.is_null() || instance_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(instance_rows, instance_count) } };
    permits(predicate, gpu, instances, side, failed)
}

#[cfg(test)]
mod tests {
    //! A validated and an unvalidated capture of one eye, the other eye's untouched; a failure withdraws the permit.
    use super::*;
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};

    #[test]
    fn instance_permits() {
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        let predicate = crate::readback_buffers::buffer(device.as_ref().unwrap(), None, (WORDS * 8) as u64, D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ).unwrap();
        assert!(crate::readback_buffers::zero(&predicate, WORDS * 8));
        let mut instances = [[0u64; 48]; 3];
        instances[0][15] = 1;
        let mut gpu = [[0u64; 24]; 3];
        gpu[0][..3].copy_from_slice(&[0, 1, 1]);
        gpu[0][12] = 0;
        gpu[1][..3].copy_from_slice(&[1, 1, 1]);
        gpu[1][12] = 8;
        gpu[2][..3].copy_from_slice(&[0, 0, 1]);
        gpu[2][12] = 16;
        gpu[2][3] = 7;
        assert!(permits(&predicate, &mut gpu, &instances, 1, false));
        assert_eq!((gpu[0][3], gpu[1][3], gpu[2][3]), (1, 0, 7), "the validated capture permitted; the other eye's untouched");
        let mut mapped = core::ptr::null_mut();
        unsafe { predicate.Map(0, None, Some(&mut mapped)) }.unwrap();
        let words = unsafe { core::slice::from_raw_parts(mapped as *const u64, 3) };
        assert_eq!(words, &[1, 0, 0]);
        unsafe { predicate.Unmap(0, None) };
        assert!(permits(&predicate, &mut gpu, &instances, 1, true));
        assert_eq!(gpu[0][3], 0, "a failure withdraws the permit");
    }
}
