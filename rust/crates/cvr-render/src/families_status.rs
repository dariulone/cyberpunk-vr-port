//! THE FAMILIES' PIPELINE STATUS WORDS (formerly src/Render/NativeStereoFamilies.cpp's NativeDepthStatus,
//! NativeDepthCompatible and NativeGeometryStatus): the words the families' pipeline observation leaves as private data
//! on an original pipeline -- its native depth pass's and its native geometry's status (1 compatible); 0 when absent.
use core::ffi::c_void;

use windows::core::{Interface, GUID};
use windows::Win32::Graphics::Direct3D12::ID3D12PipelineState;

/// nativeDepthKey and nativeGeometryKey.
const NATIVE_DEPTH_KEY: GUID = GUID::from_values(0x0db3e624, 0x37df, 0x4491, [0xa6, 0x91, 0xa1, 0x1e, 0x6d, 0x2d, 0x5e, 0x50]);
const NATIVE_GEOMETRY_KEY: GUID = GUID::from_values(0x0db3e624, 0x37df, 0x4491, [0xa6, 0x91, 0xa1, 0x1e, 0x6d, 0x2d, 0x5e, 0x51]);

fn status(pso: *mut c_void, key: &GUID) -> u32 {
    // SAFETY: a live pipeline or null.
    let Some(pso) = (unsafe { ID3D12PipelineState::from_raw_borrowed(&pso) }) else { return 0 };
    let (mut value, mut bytes) = (0u32, 4u32);
    // SAFETY: a local word and its size.
    let read = unsafe { pso.GetPrivateData(key, &mut bytes, Some((&raw mut value).cast())) };
    if read.is_ok() && bytes == 4 { value } else { 0 }
}
/// families::NativeDepthStatus.
pub fn native_depth_status(pso: *mut c_void) -> u32 {
    status(pso, &NATIVE_DEPTH_KEY)
}
/// families::NativeDepthCompatible.
pub fn native_depth_compatible(pso: *mut c_void) -> bool {
    native_depth_status(pso) == 1
}
/// families::NativeGeometryStatus.
pub fn native_geometry_status(pso: *mut c_void) -> u32 {
    status(pso, &NATIVE_GEOMETRY_KEY)
}
