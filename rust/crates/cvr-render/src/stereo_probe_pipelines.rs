//! The native stereo probe's pipeline selection (src/Render/NativeStereoProbe.cpp, `ObserveInput`, `Hash`, `Target`
//! and `CameraBinding`): a pipeline's input slot mask (at most 32 elements, each in one of 16 slots, else no mask); the
//! probe's FNV-1a variant (its offset basis 1469598103934665603, one digit short of the standard one -- kept, since the
//! recorded shader and asset hashes use it); the captured shaders the probe replaces (the depth-only vertex shader with
//! no pixel shader when the depth asset loaded, else the probe pixel shader with the dynamic or -- when its asset
//! loaded -- the static vertex shader); and the camera's binding in the 1.1 root signature (the first vertex-visible
//! root CBV b1 in space 0, or the first descriptor table whose CBV range covers b1 in space 0, at that descriptor's
//! offset in the table).
//!
//! In the Rust build (`CVR_RUST_VRIK`) these forward to `cvr_nsp_input_mask`, `cvr_nsp_hash`, `cvr_nsp_target` and
//! `cvr_nsp_camera_binding` (the deserializer and the maps stay the .cpp's); the unit tests below cover each. Enabled's
//! shader assets load here too (`cvr_nsp_assets`: each file 32 bytes to 1 MiB and of its hash, the static and the
//! depth-only vertex shaders only with the probe's two, the asset statuses as the .cpp set them).

use core::sync::atomic::{AtomicU32, Ordering};
use windows::Win32::Graphics::Direct3D12::*;

/// The probe's hash basis.
pub const BASIS: u64 = 1469598103934665603;
/// Hash.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(BASIS, |h, &b| (h ^ u64::from(b)).wrapping_mul(1099511628211))
}

/// ObserveInput's mask: None when the layout is not admitted.
pub fn input_mask(slots: Option<&[u32]>, count: u32) -> Option<u32> {
    if count > 32 {
        return None;
    }
    if count == 0 {
        return Some(0);
    }
    let slots = slots?;
    slots.iter().take(count as usize).try_fold(0u32, |mask, &slot| (slot < 16).then(|| mask | 1 << slot))
}

/// Target: `vs` / `ps` are the bytecodes (None when null).
pub fn target(vs: Option<&[u8]>, vs_length: usize, ps: Option<&[u8]>, ps_length: usize, depth_loaded: bool, static_loaded: bool) -> bool {
    if depth_loaded && ps_length == 0 && vs_length == 3635 && vs.is_some_and(|v| hash(v) == 0x3AC849D57E763A1F) {
        return true;
    }
    let (Some(vs), Some(ps)) = (vs, ps) else { return false };
    if ps_length != 9516 || hash(ps) != 0x955C629FD0296DC3 {
        return false;
    }
    (vs_length == 7051 && hash(vs) == 0x02F5A04A6E812686) || (static_loaded && vs_length == 5238 && hash(vs) == 0x7BD6CC7EF414F11A)
}

/// One root parameter as CameraBinding reads it.
#[derive(Clone, Copy, Debug)]
pub enum Parameter<'a> {
    Cbv { visibility: i32, register: u32, space: u32 },
    Table { visibility: i32, ranges: &'a [(i32, u32, u32, u32, u32)] },
    Other { visibility: i32 },
}
const VISIBILITY_ALL: i32 = 0;
const VISIBILITY_VERTEX: i32 = 1;
const RANGE_CBV: i32 = 2;
const OFFSET_APPEND: u32 = u32::MAX;
/// CameraBinding: the root index and the table offset (u32::MAX for a root CBV). A range is
/// (type, descriptors, base register, space, offset from the table start).
pub fn camera_binding(parameters: &[Parameter]) -> Option<(u32, u32)> {
    for (i, parameter) in parameters.iter().enumerate() {
        let visibility = match parameter {
            Parameter::Cbv { visibility, .. } | Parameter::Table { visibility, .. } | Parameter::Other { visibility } => *visibility,
        };
        if visibility != VISIBILITY_ALL && visibility != VISIBILITY_VERTEX {
            continue;
        }
        match parameter {
            Parameter::Cbv { register: 1, space: 0, .. } => return Some((i as u32, u32::MAX)),
            Parameter::Table { ranges, .. } => {
                let mut offset = 0u64;
                for &(kind, descriptors, base, space, start) in ranges.iter() {
                    if start != OFFSET_APPEND {
                        offset = start.into();
                    }
                    if kind == RANGE_CBV && space == 0 && base <= 1 && u64::from(base) + u64::from(descriptors) > 1 && offset + 1 - u64::from(base) <= u64::from(u32::MAX) {
                        return Some((i as u32, (offset + 1 - u64::from(base)) as u32));
                    }
                    offset = offset.wrapping_add(descriptors.into());
                }
            }
            _ => {}
        }
    }
    None
}

// ---- the probe's shader assets (Enabled)

/// The probe's shaders in the shaders folder and their hashes: the dynamic vertex and the pixel shader it needs, then the
/// static and the depth-only vertex shaders it uses when they load.
pub const ASSETS: [(&str, u64); 4] = [
    ("CyberpunkVR_SinglePassProbeVS.dxil", 0xF2BC_FF27_4A07_4AA2),
    ("CyberpunkVR_SinglePassProbePS.dxil", 0xC903_4583_D4DD_9609),
    ("CyberpunkVR_SinglePassStaticVS.dxil", 0x0D37_0481_852C_BA69),
    ("CyberpunkVR_SinglePassDepthVS.dxil", 0x600D_F2F7_0C8C_3C86),
];
/// The loaded shaders (an optional one empty when it did not load).
#[derive(Debug, Default, PartialEq)]
pub struct Assets {
    pub vs: Vec<u8>,
    pub ps: Vec<u8>,
    pub static_vs: Vec<u8>,
    pub depth_vs: Vec<u8>,
}
/// Enabled's load: the file's bytes, `status` 2 once it opens; refused under 32 bytes or over 1 MiB, unread, or hashing
/// other than `expected`.
pub fn load_asset(path: &std::path::Path, expected: u64, status: &AtomicU32) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    status.store(2, Ordering::Relaxed);
    let size = file.seek(SeekFrom::End(0)).ok()?;
    if !(32..=1024 * 1024).contains(&size) {
        return None;
    }
    let mut bytes = vec![0u8; size as usize];
    file.seek(SeekFrom::Start(0)).ok()?;
    file.read_exact(&mut bytes).ok()?;
    (hash(&bytes) == expected).then_some(bytes)
}
/// Enabled: the shaders of `table` (ASSETS) from `dir` -- `status` 1 at the start, 2 when a file opens, 3 once the
/// probe's two loaded (again after the depth shader), `static_status` 1 when the static shader loaded; None unless the
/// vertex and pixel shaders did (the pixel shader not tried without the vertex one).
pub fn load_assets(dir: &std::path::Path, table: &[(&str, u64); 4], status: &AtomicU32, static_status: &AtomicU32) -> Option<Assets> {
    status.store(1, Ordering::Relaxed);
    let load = |(name, expected): (&str, u64)| load_asset(&dir.join(name), expected, status);
    let vs = load(table[0])?;
    let ps = load(table[1])?;
    let static_vs = load(table[2]).unwrap_or_default();
    if !static_vs.is_empty() {
        static_status.store(1, Ordering::SeqCst);
    }
    status.store(3, Ordering::Release);
    let depth_vs = load(table[3]).unwrap_or_default();
    status.store(3, Ordering::Release);
    Some(Assets { vs, ps, static_vs, depth_vs })
}

// ---- the C ABI

/// # Safety
/// `layout` is valid (its elements readable when non-null); `mask` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsp_input_mask(layout: *const D3D12_INPUT_LAYOUT_DESC, mask: *mut u32) -> bool {
    let layout = unsafe { &*layout };
    let slots: Option<Vec<u32>> = (!layout.pInputElementDescs.is_null() && layout.NumElements <= 32)
        .then(|| unsafe { core::slice::from_raw_parts(layout.pInputElementDescs, layout.NumElements as usize) }.iter().map(|e| e.InputSlot).collect());
    match input_mask(slots.as_deref(), layout.NumElements) {
        Some(value) => {
            unsafe { *mask = value };
            true
        }
        None => false,
    }
}
/// # Safety
/// `data` holds `length` bytes (or is null with 0).
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsp_hash(data: *const u8, length: usize) -> u64 {
    hash(if data.is_null() || length == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, length) } })
}
/// # Safety
/// Both bytecodes are valid descriptions.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsp_target(vs: *const D3D12_SHADER_BYTECODE, ps: *const D3D12_SHADER_BYTECODE, depth_loaded: bool, static_loaded: bool) -> bool {
    let view = |b: &D3D12_SHADER_BYTECODE| (!b.pShaderBytecode.is_null()).then(|| unsafe { core::slice::from_raw_parts(b.pShaderBytecode.cast::<u8>(), b.BytecodeLength) });
    let (vs, ps) = unsafe { (&*vs, &*ps) };
    target(view(vs), vs.BytecodeLength, view(ps), ps.BytecodeLength, depth_loaded, static_loaded)
}
/// # Safety
/// `desc` is a valid 1.1 root signature description; `root` and `offset` are writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsp_camera_binding(desc: *const D3D12_ROOT_SIGNATURE_DESC1, root: *mut u32, offset: *mut u32) -> bool {
    let desc = unsafe { &*desc };
    let raw = if desc.NumParameters == 0 || desc.pParameters.is_null() { &[][..] } else { unsafe { core::slice::from_raw_parts(desc.pParameters, desc.NumParameters as usize) } };
    let ranges: Vec<Vec<(i32, u32, u32, u32, u32)>> = raw
        .iter()
        .map(|p| {
            if p.ParameterType != D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                return Vec::new();
            }
            // SAFETY: the type selects the table member.
            let table = unsafe { p.Anonymous.DescriptorTable };
            if table.NumDescriptorRanges == 0 || table.pDescriptorRanges.is_null() {
                return Vec::new();
            }
            unsafe { core::slice::from_raw_parts(table.pDescriptorRanges, table.NumDescriptorRanges as usize) }
                .iter()
                .map(|r| (r.RangeType.0, r.NumDescriptors, r.BaseShaderRegister, r.RegisterSpace, r.OffsetInDescriptorsFromTableStart))
                .collect()
        })
        .collect();
    let parameters: Vec<Parameter> = raw
        .iter()
        .zip(&ranges)
        .map(|(p, ranges)| {
            let visibility = p.ShaderVisibility.0;
            if p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_CBV {
                // SAFETY: the type selects the descriptor member.
                let d = unsafe { p.Anonymous.Descriptor };
                Parameter::Cbv { visibility, register: d.ShaderRegister, space: d.RegisterSpace }
            } else if p.ParameterType == D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE {
                Parameter::Table { visibility, ranges }
            } else {
                Parameter::Other { visibility }
            }
        })
        .collect();
    match camera_binding(&parameters) {
        Some((r, o)) => {
            unsafe { (*root, *offset) = (r, o) };
            true
        }
        None => false,
    }
}
/// The loaded shaders' bytes (null and 0 for one not loaded).
#[repr(C)]
pub struct AssetView {
    pub vs: *const u8,
    pub vs_bytes: usize,
    pub ps: *const u8,
    pub ps_bytes: usize,
    pub static_vs: *const u8,
    pub static_bytes: usize,
    pub depth_vs: *const u8,
    pub depth_bytes: usize,
}
static LOADED: std::sync::OnceLock<Option<Assets>> = std::sync::OnceLock::new();
/// Enabled's assets, loaded once from the shaders folder into `status` and `static_status` (the .cpp's exports); false
/// when the probe's shaders did not load, else their view in `out`.
///
/// # Safety
/// `status` and `static_status` are live atomics; `out` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nsp_assets(status: *const AtomicU32, static_status: *const AtomicU32, out: *mut AssetView) -> bool {
    // SAFETY: as documented.
    let (status, static_status) = unsafe { (&*status, &*static_status) };
    let loaded = LOADED.get_or_init(|| load_assets(cvr_host::paths::dir(cvr_host::paths::Folder::Shaders), &ASSETS, status, static_status));
    let Some(a) = loaded else { return false };
    let view = |v: &Vec<u8>| (if v.is_empty() { core::ptr::null() } else { v.as_ptr() }, v.len());
    let ((vs, vs_bytes), (ps, ps_bytes), (static_vs, static_bytes), (depth_vs, depth_bytes)) = (view(&a.vs), view(&a.ps), view(&a.static_vs), view(&a.depth_vs));
    unsafe { *out = AssetView { vs, vs_bytes, ps, ps_bytes, static_vs, static_bytes, depth_vs, depth_bytes } };
    true
}

/// NativeInstanceResource (src/Render/NativeStereoProbe.cpp): up the stack to the fresh native IA binding
/// (0x339DB0..0x339E8F), whose RBX is the buffer record's offset (a multiple of 0xB0 below 0x20000000) and RBP the
/// renderer: the record's resource ([RBP+RBX+0x5C0B08], the engine retained it before the call); 0 otherwise.
pub fn instance_resource(game: u64) -> u64 {
    // From four frames lower than the .cpp's 24.
    cvr_engine::stack::walk(24 + 4, |c, _, _| {
        (c.Rip >= game.wrapping_add(0x339DB0) && c.Rip < game.wrapping_add(0x339E8F)).then(|| {
            if c.Rbx % 0xB0 != 0 || c.Rbx > 0x2000_0000 {
                return 0;
            }
            let mut word = [0u8; 8];
            if cvr_engine::stack::read(c.Rbp.wrapping_add(c.Rbx).wrapping_add(0x5C0B08), &mut word) { u64::from_le_bytes(word) } else { 0 }
        })
    })
    .unwrap_or(0)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_nsp_instance_resource() -> u64 {
    // SAFETY: the executable's own module.
    instance_resource(unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(core::ptr::null()) as u64 })
}

#[cfg(test)]
mod tests {
    //! Each.
    use super::*;

    #[test]
    fn assets_load() {
        let dir = std::env::temp_dir().join(format!("cvr_nsp_assets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bytes = |seed: u8, n: usize| (0..n).map(|i| seed.wrapping_add(i as u8)).collect::<Vec<u8>>();
        let (vs, ps, depth) = (bytes(1, 40), bytes(2, 64), bytes(4, 32));
        std::fs::write(dir.join("vs.dxil"), &vs).unwrap();
        std::fs::write(dir.join("ps.dxil"), &ps).unwrap();
        std::fs::write(dir.join("depth.dxil"), &depth).unwrap();
        std::fs::write(dir.join("short.dxil"), bytes(3, 31)).unwrap();
        let (status, static_status) = (AtomicU32::new(0), AtomicU32::new(0));
        let table = [("vs.dxil", hash(&vs)), ("ps.dxil", hash(&ps)), ("short.dxil", hash(&bytes(3, 31))), ("depth.dxil", hash(&depth))];
        let loaded = load_assets(&dir, &table, &status, &static_status).unwrap();
        assert_eq!(loaded, Assets { vs: vs.clone(), ps: ps.clone(), static_vs: Vec::new(), depth_vs: depth.clone() }, "a static shader under 32 bytes");
        assert_eq!((status.load(Ordering::Relaxed), static_status.load(Ordering::Relaxed)), (3, 0));
        let table = [table[0], table[1], ("depth.dxil", hash(&depth)), ("missing.dxil", 0)];
        let loaded = load_assets(&dir, &table, &status, &static_status).unwrap();
        assert_eq!((loaded.static_vs.len(), loaded.depth_vs.len(), static_status.load(Ordering::Relaxed)), (32, 0, 1), "a static shader, no depth one");
        let wrong = [table[0], ("ps.dxil", hash(&ps) ^ 1), table[2], table[3]];
        assert!(load_assets(&dir, &wrong, &status, &static_status).is_none(), "a pixel shader of another hash");
        assert_eq!(status.load(Ordering::Relaxed), 2, "its file opened");
        let missing = [("missing.dxil", 0), table[1], table[2], table[3]];
        assert!(load_assets(&dir, &missing, &status, &static_status).is_none());
        assert_eq!(status.load(Ordering::Relaxed), 1, "nothing opened");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hashes() {
        assert_eq!(hash(&[]), BASIS);
        assert_eq!(hash(b"a"), (BASIS ^ 0x61).wrapping_mul(1099511628211));
        assert_ne!(hash(b"a"), crate::native_recording::fingerprint(b"a"));
    }
    #[test]
    fn masks() {
        assert_eq!(input_mask(Some(&[0, 3, 3, 15]), 4), Some(0x8009));
        assert_eq!(input_mask(None, 0), Some(0));
        assert_eq!(input_mask(None, 1), None);
        assert_eq!(input_mask(Some(&[0, 16]), 2), None);
        assert_eq!(input_mask(Some(&[0; 33]), 33), None);
        assert_eq!(input_mask(Some(&[1; 32]), 32), Some(2));
    }
    #[test]
    fn targets() {
        // Hash-matching bytecodes are not available here; the length and presence gates are.
        let vs = vec![0u8; 7051];
        let ps = vec![0u8; 9516];
        assert!(!target(Some(&vs), 7051, Some(&ps), 9516, true, true));
        assert!(!target(None, 7051, Some(&ps), 9516, true, true));
        assert!(!target(Some(&vs), 7051, None, 9516, true, true));
        let depth = vec![0u8; 3635];
        assert!(!target(Some(&depth), 3635, None, 0, true, true));
        assert!(!target(None, 3635, None, 0, true, true));
    }
    #[test]
    fn cameras() {
        let p = |visibility| Parameter::Other { visibility };
        // A root CBV b1 space 0, vertex or all visibility.
        assert_eq!(camera_binding(&[p(0), Parameter::Cbv { visibility: 1, register: 1, space: 0 }]), Some((1, u32::MAX)));
        assert_eq!(camera_binding(&[Parameter::Cbv { visibility: 2, register: 1, space: 0 }]), None);
        assert_eq!(camera_binding(&[Parameter::Cbv { visibility: 0, register: 1, space: 1 }]), None);
        assert_eq!(camera_binding(&[Parameter::Cbv { visibility: 0, register: 0, space: 0 }]), None);
        // A table: an SRV range of 3 appended, then a CBV b0..b1 appended at 3 -> b1 at 4.
        let ranges = [(0, 3, 0, 0, OFFSET_APPEND), (RANGE_CBV, 2, 0, 0, OFFSET_APPEND)];
        assert_eq!(camera_binding(&[Parameter::Table { visibility: 0, ranges: &ranges }]), Some((0, 4)));
        // An explicit offset resets the running offset; a CBV b1 range starts there.
        let ranges = [(0, 3, 0, 0, OFFSET_APPEND), (RANGE_CBV, 1, 1, 0, 10)];
        assert_eq!(camera_binding(&[Parameter::Table { visibility: 1, ranges: &ranges }]), Some((0, 10)));
        // A CBV b0 range of one does not cover b1; a range in space 1 or based at b2 does not either.
        for range in [(RANGE_CBV, 1, 0, 0, 0), (RANGE_CBV, 4, 0, 1, 0), (RANGE_CBV, 4, 2, 0, 0)] {
            assert_eq!(camera_binding(&[Parameter::Table { visibility: 0, ranges: &[range] }]), None);
        }
        // A pixel-only table is skipped for the next.
        let ranges = [(RANGE_CBV, 2, 0, 0, 0)];
        assert_eq!(camera_binding(&[Parameter::Table { visibility: 5, ranges: &ranges }, Parameter::Table { visibility: 0, ranges: &ranges }]), Some((1, 1)));
    }
    #[test]
    fn abi() {
        let range = D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, NumDescriptors: 2, OffsetInDescriptorsFromTableStart: 5, ..Default::default() };
        let parameters = [
            D3D12_ROOT_PARAMETER1 { ParameterType: D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS, ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL, ..Default::default() },
            D3D12_ROOT_PARAMETER1 {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 1, pDescriptorRanges: &range } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_VERTEX,
            },
        ];
        let desc = D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: 2, pParameters: parameters.as_ptr(), ..Default::default() };
        let (mut root, mut offset) = (0, 0);
        assert!(unsafe { cvr_nsp_camera_binding(&desc, &mut root, &mut offset) });
        assert_eq!((root, offset), (1, 6));
        let elements = [D3D12_INPUT_ELEMENT_DESC { InputSlot: 2, ..Default::default() }, D3D12_INPUT_ELEMENT_DESC { InputSlot: 5, ..Default::default() }];
        let layout = D3D12_INPUT_LAYOUT_DESC { pInputElementDescs: elements.as_ptr(), NumElements: 2 };
        let mut mask = 0;
        assert!(unsafe { cvr_nsp_input_mask(&layout, &mut mask) });
        assert_eq!(mask, 0x24);
        let null = D3D12_INPUT_LAYOUT_DESC { pInputElementDescs: core::ptr::null(), NumElements: 2 };
        assert!(!unsafe { cvr_nsp_input_mask(&null, &mut mask) });
    }
}
