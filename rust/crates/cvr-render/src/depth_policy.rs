//! Which shaders the single-pass path may run an extra time (formerly src/Render/NativeDepthPolicy.cpp): an additional
//! depth-reference draw is authorised only for a shader that writes nothing -- empty stages are fine, unsupported
//! bytecode and any UAV binding fail closed -- and the native routing into one array layer also rejects ViewID and a
//! vertex shader output that selects an array layer or viewport (only the pixel shader may be empty).
//!
//! The container is checked first (its parts, its feature flags: no descriptor-heap indexing, no ViewID for one
//! layer); then the shader is reflected, by d3dcompiler's D3DReflect or, for DXIL it cannot read, by the dxcompiler
//! named in CVR_STEREO_REFLECTION_DXC.
use core::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use windows::core::{Interface, GUID, HRESULT};
use windows::Win32::Graphics::Direct3D::Dxc::{CLSID_DxcUtils, DxcBuffer, IDxcUtils};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D::*;
use windows::Win32::Graphics::Direct3D12::*;

type CreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

/// The dxcompiler named by CVR_STEREO_REFLECTION_DXC (an absolute path), loaded once and never freed: COM releases
/// happen during reflection, never from a destructor at DLL detach.
fn modern_reflection_factory() -> Option<CreateInstance> {
    static FACTORY: OnceLock<usize> = OnceLock::new();
    let p = *FACTORY.get_or_init(|| {
        let Some(path) = std::env::var_os("CVR_STEREO_REFLECTION_DXC").map(PathBuf::from) else {
            return 0;
        };
        if path.as_os_str().is_empty() || !path.is_absolute() {
            return 0;
        }
        let wide: Vec<u16> = path.as_os_str().encode_wide_z();
        // SAFETY: a NUL-terminated path; the module stays loaded for the process.
        unsafe {
            use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};
            let Ok(module) = LoadLibraryExW(
                windows::core::PCWSTR(wide.as_ptr()),
                None,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS,
            ) else {
                return 0;
            };
            GetProcAddress(module, windows::core::s!("DxcCreateInstance")).map_or(0, |f| f as usize)
        }
    });
    // SAFETY: dxcompiler's DxcCreateInstance.
    (p != 0).then(|| unsafe { core::mem::transmute::<usize, CreateInstance>(p) })
}
trait WideZ {
    fn encode_wide_z(&self) -> Vec<u16>;
}
impl WideZ for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(core::iter::once(0)).collect()
    }
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
/// The container's parts and feature flags (None for missing bytes behind a non-zero length).
fn explicit_resources(code: Option<&[u8]>, len: usize, single_layer: bool) -> bool {
    if len == 0 {
        return true;
    }
    let Some(b) = code else { return false };
    if !(32..=16 * 1024 * 1024).contains(&len) || &b[..4] != b"DXBC" || word(b, 24) as usize != len {
        return false;
    }
    let count = word(b, 28) as usize;
    if count > 64 || count > (len - 32) / 4 {
        return false;
    }
    let (mut feature, mut dxil) = (false, false);
    for i in 0..count {
        let at = word(b, 32 + 4 * i) as usize;
        if at < 32 + 4 * count || at > len - 8 {
            return false;
        }
        let size = word(b, at + 4) as usize;
        if size > len - at - 8 {
            return false;
        }
        dxil |= &b[at..at + 4] == b"DXIL";
        if &b[at..at + 4] == b"SFI0" {
            if feature || size != 8 {
                return false;
            }
            feature = true;
            let flags = u64::from_le_bytes(b[at + 8..at + 16].try_into().unwrap_or_default());
            if flags & (D3D_SHADER_REQUIRES_RESOURCE_DESCRIPTOR_HEAP_INDEXING | D3D_SHADER_REQUIRES_ATOMIC_INT64_ON_DESCRIPTOR_HEAP_RESOURCE) as u64 != 0 {
                return false;
            }
            if single_layer && flags & D3D_SHADER_REQUIRES_VIEW_ID as u64 != 0 {
                return false;
            }
        }
    }
    !dxil || feature
}
/// _stricmp(name, text) == 0.
fn named(name: windows::core::PCSTR, text: &str) -> bool {
    // SAFETY: the reflection's NUL-terminated semantic name.
    !name.is_null() && unsafe { name.as_bytes() }.eq_ignore_ascii_case(text.as_bytes())
}
fn reflect(code: &[u8]) -> Option<ID3D12ShaderReflection> {
    let mut raw: *mut c_void = core::ptr::null_mut();
    // SAFETY: the caller's bytecode and its length; an out-pointer for the interface.
    if unsafe { D3DReflect(code.as_ptr() as *const c_void, code.len(), &ID3D12ShaderReflection::IID, &mut raw) }.is_ok() && !raw.is_null() {
        // SAFETY: the reflection D3DReflect made (one reference).
        return Some(unsafe { ID3D12ShaderReflection::from_raw(raw) });
    }
    let create = modern_reflection_factory()?;
    // SAFETY (whole block): dxcompiler's factory, its utilities, the caller's bytecode.
    unsafe {
        let mut utils: *mut c_void = core::ptr::null_mut();
        if create(&CLSID_DxcUtils, &IDxcUtils::IID, &mut utils).is_err() || utils.is_null() {
            return None;
        }
        let utils = IDxcUtils::from_raw(utils);
        let buffer = DxcBuffer {
            Ptr: code.as_ptr() as *const c_void,
            Size: code.len(),
            Encoding: 0,
        };
        let mut raw: *mut c_void = core::ptr::null_mut();
        utils.CreateReflection(&buffer, &ID3D12ShaderReflection::IID, &mut raw).ok()?;
        (!raw.is_null()).then(|| ID3D12ShaderReflection::from_raw(raw))
    }
}
fn read_only(code: Option<&[u8]>, len: usize, single_layer: bool, vertex: bool) -> bool {
    if len == 0 {
        return !single_layer || !vertex;
    }
    if !explicit_resources(code, len, single_layer) {
        return false;
    }
    let Some(code) = code else { return false };
    static REFLECTION: Mutex<()> = Mutex::new(());
    let _guard = REFLECTION.lock().unwrap_or_else(|p| p.into_inner());
    let Some(reflection) = reflect(code) else { return false };
    // SAFETY (whole block): the live reflection and out-structures on the stack.
    unsafe {
        let mut desc = D3D12_SHADER_DESC::default();
        if reflection.GetDesc(&mut desc).is_err() || desc.BoundResources > 4096 {
            return false;
        }
        if single_layer {
            let kind = (desc.Version >> 16) & 0xffff;
            let wanted = if vertex {
                D3D12_SHVER_VERTEX_SHADER.0 as u32
            } else {
                D3D12_SHVER_PIXEL_SHADER.0 as u32
            };
            if kind != wanted
                || reflection.GetRequiresFlags() & D3D_SHADER_REQUIRES_VIEW_ID as u64 != 0
                || desc.InputParameters > 128
                || desc.OutputParameters > 128
            {
                return false;
            }
            for side in 0..2 {
                let count = if side == 1 { desc.OutputParameters } else { desc.InputParameters };
                for i in 0..count {
                    let mut p = D3D12_SIGNATURE_PARAMETER_DESC::default();
                    let hr = if side == 1 {
                        reflection.GetOutputParameterDesc(i, &mut p)
                    } else {
                        reflection.GetInputParameterDesc(i, &mut p)
                    };
                    if hr.is_err() || p.SemanticName.is_null() || named(p.SemanticName, "SV_ViewID") {
                        return false;
                    }
                    if vertex
                        && side == 1
                        && (p.SystemValueType == D3D_NAME_RENDER_TARGET_ARRAY_INDEX
                            || p.SystemValueType == D3D_NAME_VIEWPORT_ARRAY_INDEX
                            || named(p.SemanticName, "SV_RenderTargetArrayIndex")
                            || named(p.SemanticName, "SV_ViewportArrayIndex"))
                    {
                        return false;
                    }
                }
            }
        }
        for i in 0..desc.BoundResources {
            let mut binding = D3D12_SHADER_INPUT_BIND_DESC::default();
            if reflection.GetResourceBindingDesc(i, &mut binding).is_err() {
                return false;
            }
            if !matches!(
                binding.Type,
                D3D_SIT_CBUFFER
                    | D3D_SIT_TBUFFER
                    | D3D_SIT_TEXTURE
                    | D3D_SIT_SAMPLER
                    | D3D_SIT_STRUCTURED
                    | D3D_SIT_BYTEADDRESS
                    | D3D_SIT_RTACCELERATIONSTRUCTURE
            ) {
                return false;
            }
        }
    }
    true
}

// ---- the C ABI (include/Render/NativeDepthPolicy.hpp)

/// # Safety
/// `code` is `len` readable bytes or null.
unsafe fn bytes<'a>(code: *const c_void, len: usize) -> Option<&'a [u8]> {
    // SAFETY: the caller's bytecode.
    (!code.is_null()).then(|| unsafe { core::slice::from_raw_parts(code as *const u8, len) })
}
/// # Safety
/// `code` is `len` readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_depth_policy_explicit_shader_resources(code: *const c_void, len: usize) -> bool {
    // SAFETY: the caller's bytecode.
    explicit_resources(unsafe { bytes(code, len) }, len, false)
}
/// # Safety
/// `code` is `len` readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_depth_policy_shader_read_only(code: *const c_void, len: usize) -> bool {
    // SAFETY: the caller's bytecode.
    read_only(unsafe { bytes(code, len) }, len, false, false)
}
/// # Safety
/// `code` is `len` readable bytes or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_depth_policy_shader_single_layer_read_only(code: *const c_void, len: usize, vertex: bool) -> bool {
    // SAFETY: the caller's bytecode.
    read_only(unsafe { bytes(code, len) }, len, true, vertex)
}
