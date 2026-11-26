//! Builds the view-instanced twin of a DXIL shader in process (formerly src/Render/ViewTwinCompiler.cpp; the rewrite is
//! view_twin_ir): dxcompiler.dll disassembles, the text is rewritten, the DXIL optimizer recomputes the view-ID state
//! (-hlsl-dxilload -viewid-state -hlsl-dxilemit), the assembler packs a container and dxil.dll validates and signs it.
//! One compiler per thread (the DXC objects are not shared). The DXC modules stay loaded once loaded: DXC keeps
//! process-wide state that must not be torn down under other users.
use core::ffi::c_void;

use windows::core::{w, Interface, GUID, HRESULT, PCSTR, PCWSTR};
use windows::Win32::Graphics::Direct3D::Dxc::*;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_DEFAULT_DIRS, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR};

use crate::view_twin_ir::{rewrite_for_views, TwinInfo};

/// TwinShader: the result of a build.
#[derive(Default, Debug, Clone)]
pub struct TwinShader {
    pub ok: bool,
    pub unchanged: bool,
    /// A pixel shader twin made for this vertex shader (its inputs completed).
    pub linked: bool,
    pub loads: u32,
    pub extracts: u32,
    /// ok && !unchanged: the signed twin.
    pub container: Vec<u8>,
    pub error: Vec<u8>,
}

struct Objects {
    disassembler: Option<IDxcCompiler>,
    utils: Option<IDxcUtils>,
    optimizer: Option<IDxcOptimizer>,
    assembler: Option<IDxcAssembler>,
    validator: Option<IDxcValidator>,
}

#[derive(Default)]
pub struct Compiler {
    objects: Option<Objects>,
    error: Vec<u8>,
}

fn hr(what: &str, hr: HRESULT) -> String {
    format!("{what} failed 0x{:08X}", hr.0 as u32)
}
fn blob_bytes(blob: &IDxcBlob) -> &[u8] {
    // SAFETY: the blob's own buffer, alive with the blob.
    unsafe {
        let (p, n) = (blob.GetBufferPointer(), blob.GetBufferSize());
        if p.is_null() || n == 0 {
            &[]
        } else {
            core::slice::from_raw_parts(p as *const u8, n)
        }
    }
}
/// The blob's text without its trailing NULs.
fn text(blob: Option<&IDxcBlob>) -> Vec<u8> {
    let mut s = blob.map_or_else(Vec::new, |b| blob_bytes(b).to_vec());
    while s.last() == Some(&0) {
        s.pop();
    }
    s
}
fn prefix(mut s: Vec<u8>, n: usize) -> Vec<u8> {
    s.truncate(n);
    s
}
/// The operation's result, or the reason it failed (an empty reason when it succeeded without a blob).
fn finish(result: &IDxcOperationResult, what: &str) -> Result<IDxcBlob, Vec<u8>> {
    // SAFETY: a live operation result.
    let Ok(status) = (unsafe { result.GetStatus() }) else {
        return Err(format!("{what}: no status").into_bytes());
    };
    if status.is_err() {
        // SAFETY: as above.
        let errors = unsafe { result.GetErrorBuffer() }.ok();
        let mut why = hr(what, status).into_bytes();
        why.extend_from_slice(b": ");
        why.extend_from_slice(&prefix(text(errors.as_deref()), 600));
        return Err(why);
    }
    // SAFETY: as above.
    unsafe { result.GetResult() }.map_err(|_| Vec::new())
}

type Export = unsafe extern "system" fn() -> isize;
type CreateInstance = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;
fn create<T: Interface>(create: CreateInstance, clsid: &GUID) -> Result<T, HRESULT> {
    let mut out = core::ptr::null_mut();
    // SAFETY: DxcCreateInstance writes the new object's interface pointer.
    let result = unsafe { create(clsid, &T::IID, &mut out) };
    if result.is_err() {
        return Err(result);
    }
    if out.is_null() {
        return Err(HRESULT(0));
    }
    // SAFETY: the owned reference DxcCreateInstance returned, of the requested interface.
    Ok(unsafe { T::from_raw(out) })
}

impl Compiler {
    /// Load: `folder` holds dxcompiler.dll and dxil.dll. On failure the reason is kept (Error()).
    pub fn load(&mut self, folder: &[u16]) -> bool {
        let objects = self.objects.insert(Objects {
            disassembler: None,
            utils: None,
            optimizer: None,
            assembler: None,
            validator: None,
        });
        let mut dir = folder.to_vec();
        if !(dir.is_empty() || *dir.last().unwrap_or(&0) == b'\\' as u16 || *dir.last().unwrap_or(&0) == b'/' as u16) {
            dir.push(b'\\' as u16);
        }
        let path = |name: &str| -> Vec<u16> {
            let mut p = dir.clone();
            p.extend(name.encode_utf16());
            p.push(0);
            p
        };
        let flags = LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS;
        // dxil.dll first, by full path: dxcompiler later finds the loaded module by name.
        let (validator_path, compiler_path) = (path("dxil.dll"), path("dxcompiler.dll"));
        // SAFETY: NUL-terminated paths.
        let validator_module = unsafe { LoadLibraryExW(PCWSTR(validator_path.as_ptr()), None, flags) }.ok();
        let compiler_module = unsafe { LoadLibraryExW(PCWSTR(compiler_path.as_ptr()), None, flags) }.ok();
        let (Some(compiler_module), Some(validator_module)) = (compiler_module, validator_module) else {
            self.error = b"dxcompiler.dll / dxil.dll not found".to_vec();
            return false;
        };
        let name = PCSTR(c"DxcCreateInstance".as_ptr().cast());
        // SAFETY: the exports of the loaded DXC modules, with DxcCreateInstanceProc's signature.
        let (Some(create_compiler), Some(create_validator)) = (unsafe { GetProcAddress(compiler_module, name) }, unsafe {
            GetProcAddress(validator_module, name)
        }) else {
            self.error = b"DxcCreateInstance missing".to_vec();
            return false;
        };
        let (create_compiler, create_validator): (CreateInstance, CreateInstance) =
            // SAFETY: as above.
            unsafe { (core::mem::transmute::<Export, CreateInstance>(create_compiler), core::mem::transmute::<Export, CreateInstance>(create_validator)) };
        macro_rules! make {
            ($field:ident, $create:expr, $clsid:expr, $what:expr) => {
                match create($create, &$clsid) {
                    Ok(object) => objects.$field = Some(object),
                    Err(code) => {
                        self.error = hr($what, code).into_bytes();
                        return false;
                    }
                }
            };
        }
        make!(disassembler, create_compiler, CLSID_DxcCompiler, "DxcCompiler");
        make!(utils, create_compiler, CLSID_DxcUtils, "DxcUtils");
        make!(optimizer, create_compiler, CLSID_DxcOptimizer, "DxcOptimizer");
        make!(assembler, create_compiler, CLSID_DxcAssembler, "DxcAssembler");
        make!(validator, create_validator, CLSID_DxcValidator, "DxcValidator (dxil.dll)");
        true
    }

    /// Build: `vertex`, for a pixel shader, is the vertex shader of the pipeline (its outputs may complete the pixel
    /// shader's inputs).
    pub fn build(&self, dxil: &[u8], vertex: &[u8]) -> TwinShader {
        let mut out = TwinShader::default();
        let objects = self.objects.as_ref();
        let (Some(disassembler), Some(utils), Some(optimizer), Some(assembler), Some(validator)) = (
            objects.and_then(|o| o.disassembler.as_ref()),
            objects.and_then(|o| o.utils.as_ref()),
            objects.and_then(|o| o.optimizer.as_ref()),
            objects.and_then(|o| o.assembler.as_ref()),
            objects.and_then(|o| o.validator.as_ref()),
        ) else {
            out.error = b"compiler not loaded".to_vec();
            return out;
        };
        let fail = |out: &mut TwinShader, why: String| out.error = why.into_bytes();
        // SAFETY: DXC copies or references the caller's bytes for the blob's lifetime (this call).
        let source = match unsafe { utils.CreateBlob(dxil.as_ptr().cast(), dxil.len() as u32, DXC_CP_ACP) } {
            Ok(s) => s,
            Err(e) => {
                fail(&mut out, hr("CreateBlob", e.code()));
                return out;
            }
        };
        // SAFETY: a live blob.
        let disassembly = match unsafe { disassembler.Disassemble(&source) } {
            Ok(d) => d,
            Err(e) => {
                fail(&mut out, hr("Disassemble", e.code()));
                return out;
            }
        };
        let mut vertex_text = Vec::new();
        if !vertex.is_empty() {
            // SAFETY: as above.
            if let Ok(vs) = unsafe { utils.CreateBlob(vertex.as_ptr().cast(), vertex.len() as u32, DXC_CP_ACP) } {
                if let Ok(vs_text) = unsafe { disassembler.Disassemble(&vs) } {
                    vertex_text = text(Some(&vs_text));
                }
            }
        }
        let ir = rewrite_for_views(&text(Some(&disassembly)), (!vertex_text.is_empty()).then_some(&vertex_text[..]));
        out.linked = ir.signature;
        out.loads = ir.loads;
        out.extracts = ir.extracts;
        if !ir.ok {
            out.error = [&b"rewrite: "[..], &ir.error].concat();
            return out;
        }
        if ir.unchanged {
            out.ok = true;
            out.unchanged = true;
            return out;
        }
        // SAFETY: the rewritten text, alive for the blob's use below.
        let module_text = match unsafe { utils.CreateBlob(ir.text.as_ptr().cast(), ir.text.len() as u32, DXC_CP_UTF8) } {
            Ok(t) => t,
            Err(e) => {
                fail(&mut out, hr("CreateBlob", e.code()));
                return out;
            }
        };
        let passes = [w!("-hlsl-dxilload"), w!("-viewid-state"), w!("-hlsl-dxilemit")];
        let (mut module, mut messages): (Option<IDxcBlob>, Option<IDxcBlobEncoding>) = (None, None);
        // SAFETY: the optimizer writes the module and its messages.
        let result = unsafe { optimizer.RunOptimizer(&module_text, &passes, &mut module, Some(&mut messages)) };
        let Some(module) = module.filter(|_| result.is_ok()) else {
            let code = result.err().map_or(HRESULT(0), |e| e.code());
            let mut why = hr("RunOptimizer", code).into_bytes();
            why.extend_from_slice(b": ");
            why.extend_from_slice(&prefix(text(messages.as_deref()), 600));
            out.error = why;
            return out;
        };
        // SAFETY: a live module blob.
        let assembled = match unsafe { assembler.AssembleToContainer(&module) } {
            Ok(r) => r,
            Err(e) => {
                fail(&mut out, hr("AssembleToContainer", e.code()));
                return out;
            }
        };
        let container = match finish(&assembled, "assemble") {
            Ok(c) => c,
            Err(why) => {
                out.error = why;
                return out;
            }
        };
        drop(assembled);
        // SAFETY: a live container blob, signed in place.
        let validated = match unsafe { validator.Validate(&container, DxcValidatorFlags_InPlaceEdit) } {
            Ok(r) => r,
            Err(e) => {
                fail(&mut out, hr("Validate", e.code()));
                return out;
            }
        };
        // In-place validation signs the container it was given; some versions hand back that same blob.
        match finish(&validated, "validate") {
            Ok(signed) => {
                out.container = blob_bytes(&signed).to_vec();
                out.ok = true;
            }
            Err(why) => out.error = why,
        }
        out
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn cvr_view_twin_compiler_new() -> *mut Compiler {
    Box::into_raw(Box::default())
}
/// # Safety
/// `c` came from cvr_view_twin_compiler_new (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_compiler_free(c: *mut Compiler) {
    if !c.is_null() {
        // SAFETY: the wrapper's compiler, dropped once (its COM objects released; the modules stay loaded).
        drop(unsafe { Box::from_raw(c) });
    }
}
/// # Safety
/// `c` is a live compiler; `folder` points to `len` UTF-16 units.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_compiler_load(c: *mut Compiler, folder: *const u16, len: usize) -> bool {
    // SAFETY: the wrapper's compiler and the caller's folder.
    let Some(c) = (unsafe { c.as_mut() }) else { return false };
    let folder = if len == 0 || folder.is_null() {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(folder, len) }
    };
    c.load(folder)
}
/// # Safety
/// `c` is a live compiler; `data`/`len` receive its last load error (valid until the next load).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_compiler_error(c: *const Compiler, data: *mut *const u8, len: *mut usize) {
    // SAFETY: the wrapper's compiler and outputs.
    if let Some(c) = unsafe { c.as_ref() } {
        unsafe {
            *data = c.error.as_ptr();
            *len = c.error.len();
        }
    }
}
/// # Safety
/// `c` is a live compiler; `dxil` points to `bytes` bytes and `vertex` (if any) to `vertex_bytes`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_build(c: *const Compiler, dxil: *const u8, bytes: usize, vertex: *const u8, vertex_bytes: usize) -> *mut TwinShader {
    // SAFETY: the caller's byte ranges.
    let slice = |p: *const u8, n: usize| {
        if n == 0 || p.is_null() {
            &[][..]
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }
        }
    };
    let shader = match unsafe { c.as_ref() } {
        Some(c) => c.build(slice(dxil, bytes), slice(vertex, vertex_bytes)),
        None => TwinShader {
            error: b"compiler not loaded".to_vec(),
            ..Default::default()
        },
    };
    Box::into_raw(Box::new(shader))
}
/// # Safety
/// `shader` came from cvr_view_twin_build; `info` points to a TwinInfo.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_shader_info(shader: *const TwinShader, info: *mut TwinInfo) {
    // SAFETY: the caller's build and output.
    let (Some(s), Some(info)) = (unsafe { shader.as_ref() }, unsafe { info.as_mut() }) else {
        return;
    };
    *info = TwinInfo {
        ok: s.ok,
        unchanged: s.unchanged,
        linked: s.linked,
        loads: s.loads,
        handles: 0,
        extracts: s.extracts,
        data: s.container.as_ptr(),
        data_len: s.container.len(),
        error: s.error.as_ptr(),
        error_len: s.error.len(),
    };
}
/// # Safety
/// `shader` came from cvr_view_twin_build (or is null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cvr_view_twin_shader_free(shader: *mut TwinShader) {
    if !shader.is_null() {
        // SAFETY: the build handed out once.
        drop(unsafe { Box::from_raw(shader) });
    }
}
