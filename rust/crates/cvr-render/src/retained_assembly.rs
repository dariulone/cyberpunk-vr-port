//! The retained geometry's source assembly CBV requirements (src/Render/NativeRetainedGeometry.cpp,
//! BuildSourceAssemblies): every CBV the family's shaders read, located in the source root's tables (each CBV use its
//! own domain: finite table slots, or an unbounded tail's deferred use; a use of no or more than 128 descriptors, or one
//! the root does not hold, a root failure), one requirement per descriptor slot (parameter, offset, visibility,
//! register, space, the largest reflected size; at most 128, more a capacity failure); then each requirement without an
//! owned private copy of the occurrence covering its size reported missing (whether a private copy existed; the
//! original CBV at the slot of the source's bound table on the occurrence's recording, its size, resource, address,
//! heap and the resource's heap type, an upload heap flagged) -- at most the detail limit, more truncating with a
//! capacity failure.
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends the family's uses, the decoded root, the private and original
//! CBVs and the bound tables, and appends the rows; the unit test below covers the requirements and the rows.

use crate::assembly_build::source_plan;
use crate::early_depth_checks::ResourceUse;
use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::*;

/// source_binding_assembly::Failure's Root and Capacity.
pub const ROOT: u32 = 2;
pub const CAPACITY: u32 = 12;

/// A private CBV snapshot's facts.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct PrivateCbv {
    pub parameter: u32,
    pub offset: u32,
    pub reflected: u32,
    pub copied: u64,
    pub held: bool,
}
/// An original CBV's facts.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct OriginalCbv {
    pub gpu: u64,
    pub list: u64,
    pub generation: u64,
    pub resource: *mut c_void,
    pub heap: u64,
    pub descriptor_bytes: u64,
}
/// What the .cpp lends.
#[repr(C)]
pub struct Input {
    pub root: *const D3D12_ROOT_SIGNATURE_DESC1,
    pub uses: *const ResourceUse,
    pub use_count: usize,
    pub privates: *const PrivateCbv,
    pub private_count: usize,
    pub originals: *const OriginalCbv,
    pub original_count: usize,
    pub tables: *const u64,
    pub table_count: usize,
    pub source: u64,
    pub eye: u64,
    pub list: u64,
    pub generation: u64,
    pub stride: u32,
    pub owns_copies: bool,
    pub out: *mut [u64; 16],
    pub room: usize,
}
/// What comes back: the requirement count, the missing count, the rows written, the last failure (0 none), truncation.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    pub requirements: u64,
    pub missing: u64,
    pub written: usize,
    pub failure: u32,
    pub truncated: bool,
}
const _: () = assert!(
    core::mem::size_of::<PrivateCbv>() == 32
        && core::mem::size_of::<OriginalCbv>() == 48
        && core::mem::size_of::<Input>() == 128
        && core::mem::size_of::<Output>() == 32
);

/// The CBV requirements: [parameter, offset, visibility, register, space, bytes]; the last failure (ROOT, CAPACITY).
pub fn requirements(root: &D3D12_ROOT_SIGNATURE_DESC1, uses: &[ResourceUse]) -> (Vec<[u32; 6]>, u32) {
    let mut found: Vec<[u32; 6]> = Vec::new();
    let mut failure = 0;
    for use_ in uses.iter().filter(|u| u.kind == D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 as u32) {
        let domain = if use_.count == 0 || use_.count > 128 { None } else { source_plan(root, core::slice::from_ref(use_)) };
        let Some(domain) = domain else {
            failure = ROOT;
            continue;
        };
        let mut require = |parameter: u32, offset: u32, count: u32| {
            for n in 0..count {
                if let Some(existing) = found.iter_mut().find(|v| v[0] == parameter && v[1] == offset + n) {
                    existing[5] = existing[5].max(use_.bytes);
                    continue;
                }
                if found.len() >= 128 {
                    failure = CAPACITY;
                    break;
                }
                found.push([parameter, offset + n, use_.visibility, use_.shader_register + n, use_.space, use_.bytes]);
            }
        };
        for table in &domain.finite {
            for &(offset, count, _) in &table.uses {
                require(table.parameter, offset, count);
            }
        }
        for &(parameter, offset, ref deferred) in &domain.deferred {
            require(parameter, offset, deferred.count);
        }
    }
    (found, failure)
}
/// The missing rows into `out` (at most its length); the counts and the last failure.
pub fn missing(required: &[[u32; 6]], i: &Input, privates: &[PrivateCbv], originals: &[OriginalCbv], tables: &[u64], out: &mut [[u64; 16]]) -> Output {
    let mut o = Output { requirements: required.len() as u64, ..Default::default() };
    for r in required {
        let private = privates.iter().find(|v| v.parameter == r[0] && v.offset == r[1]);
        let owned = i.owns_copies && private.is_some_and(|v| v.held);
        if owned && private.is_some_and(|v| v.reflected >= r[5] && v.copied >= u64::from(r[5])) {
            continue;
        }
        o.missing += 1;
        if o.written >= out.len() {
            o.truncated = true;
            o.failure = CAPACITY;
            continue;
        }
        let mut row = [0u64; 16];
        row[..8].copy_from_slice(&[i.source, i.eye, u64::from(r[2]), u64::from(r[3]), u64::from(r[4]), u64::from(r[0]), u64::from(r[1]), u64::from(r[5])]);
        row[10] = if owned { 2 } else { 1 };
        let stride = u64::from(i.stride);
        let original = originals.iter().find(|v| {
            let Some(&base) = tables.get(r[0] as usize).filter(|_| stride != 0) else { return false };
            base != 0 && u64::from(r[1]) <= (u64::MAX - base) / stride && v.gpu == base + u64::from(r[1]) * stride && v.list == i.list && v.generation == i.generation
        });
        if let Some(v) = original {
            row[8] = v.descriptor_bytes;
            row[11] = v.resource as u64;
            row[12] = v.gpu;
            row[13] = v.heap;
            // SAFETY: the original's resource lives (the source holds it) or is null.
            if let Some(resource) = unsafe { ID3D12Resource::from_raw_borrowed(&v.resource) } {
                let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
                if unsafe { resource.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_ok() {
                    row[9] = heap.Type.0 as u64;
                    row[15] = u64::from(heap.Type == D3D12_HEAP_TYPE_UPLOAD || heap.Type == D3D12_HEAP_TYPE_GPU_UPLOAD);
                }
            }
        }
        out[o.written] = row;
        o.written += 1;
    }
    o
}

fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    // SAFETY: the .cpp lends `count` entries at `data` for the call.
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
/// The requirements and the missing rows (the requirements' last failure first, the rows' after).
///
/// # Safety
/// The input's arrays hold their counts; `out` holds `room` rows; `output` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_missing_cbvs(input: *const Input, output: *mut Output) {
    // SAFETY: as documented.
    let i = unsafe { &*input };
    let (required, failure) = match unsafe { i.root.as_ref() } {
        Some(root) => requirements(root, slice(i.uses, i.use_count)),
        None => (Vec::new(), ROOT),
    };
    let out = if i.out.is_null() || i.room == 0 { &mut [][..] } else { unsafe { core::slice::from_raw_parts_mut(i.out, i.room) } };
    let mut o = missing(&required, i, slice(i.privates, i.private_count), slice(i.originals, i.original_count), slice(i.tables, i.table_count), out);
    if o.failure == 0 {
        o.failure = failure;
    }
    unsafe { *output = o };
}

#[cfg(test)]
mod tests {
    //! The requirements of a finite table and a tail, merged sizes and failures; the missing rows.
    use super::*;

    fn root(ranges: &[D3D12_DESCRIPTOR_RANGE1]) -> (Vec<D3D12_ROOT_PARAMETER1>, D3D12_ROOT_SIGNATURE_DESC1) {
        let parameters: Vec<D3D12_ROOT_PARAMETER1> = ranges
            .iter()
            .map(|range| D3D12_ROOT_PARAMETER1 {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER1_0 { DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE1 { NumDescriptorRanges: 1, pDescriptorRanges: range } },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            })
            .collect();
        let desc = D3D12_ROOT_SIGNATURE_DESC1 { NumParameters: parameters.len() as u32, pParameters: parameters.as_ptr(), ..Default::default() };
        (parameters, desc)
    }
    fn cbvs(base: u32, count: u32, offset: u32) -> D3D12_DESCRIPTOR_RANGE1 {
        D3D12_DESCRIPTOR_RANGE1 { RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_CBV, NumDescriptors: count, BaseShaderRegister: base, OffsetInDescriptorsFromTableStart: offset, ..Default::default() }
    }
    fn cbv(register: u32, count: u32, bytes: u32) -> ResourceUse {
        ResourceUse { kind: D3D12_DESCRIPTOR_RANGE_TYPE_CBV.0 as u32, visibility: 5, space: 0, shader_register: register, count, bytes }
    }
    #[test]
    fn requirements_and_rows() {
        let ranges = [cbvs(0, 8, 2), cbvs(8, u32::MAX, 0)];
        let (_held, desc) = root(&ranges);
        let srv = ResourceUse { kind: D3D12_DESCRIPTOR_RANGE_TYPE_SRV.0 as u32, ..cbv(0, 1, 0) };
        let (found, failure) = requirements(&desc, &[cbv(1, 2, 64), srv, cbv(2, 1, 256), cbv(9, 1, 16)]);
        assert_eq!(failure, 0);
        assert_eq!(found, vec![[0, 3, 5, 1, 0, 64], [0, 4, 5, 2, 0, 256], [1, 1, 5, 9, 0, 16]], "slots of the finite table (the larger size kept) and the tail");
        assert_eq!(requirements(&desc, &[cbv(20, 0, 4), cbv(1, 129, 4)]).1, ROOT, "no or too many descriptors");
        assert_eq!(requirements(&desc, &[cbv(1, 1, 4), ResourceUse { space: 3, ..cbv(1, 1, 4) }]).1, ROOT, "a use the root does not hold");
        let i = Input {
            root: &desc,
            uses: core::ptr::null(),
            use_count: 0,
            privates: core::ptr::null(),
            private_count: 0,
            originals: core::ptr::null(),
            original_count: 0,
            tables: core::ptr::null(),
            table_count: 0,
            source: 7,
            eye: 1,
            list: 10,
            generation: 11,
            stride: 32,
            owns_copies: true,
            out: core::ptr::null_mut(),
            room: 0,
        };
        let privates = [PrivateCbv { parameter: 0, offset: 3, reflected: 64, copied: 64, held: true }, PrivateCbv { parameter: 0, offset: 4, reflected: 64, copied: 64, held: true }];
        let originals = [OriginalCbv { gpu: 1000 + 4 * 32, list: 10, generation: 11, resource: core::ptr::null_mut(), heap: 5, descriptor_bytes: 256 }];
        let tables = [1000u64, 0];
        let mut out = [[0u64; 16]; 4];
        let o = missing(&found, &i, &privates, &originals, &tables, &mut out);
        assert_eq!((o.requirements, o.missing, o.written, o.failure, o.truncated), (3, 2, 2, 0, false), "the owned 64-byte copy covers its slot");
        assert_eq!(&out[0][..11], &[7, 1, 5, 2, 0, 0, 4, 256, 256, 0, 2], "too small an owned copy: missing, with the original at the bound slot");
        assert_eq!((out[0][12], out[0][13]), (1128, 5));
        assert_eq!((out[1][5], out[1][6], out[1][10], out[1][8]), (1, 1, 1, 0), "no private copy and no bound table: missing alone");
        let o = missing(&found, &Input { owns_copies: false, ..i }, &privates, &originals, &tables, &mut out[..1]);
        assert_eq!((o.missing, o.written, o.failure, o.truncated), (3, 1, CAPACITY, true), "copies of another occurrence; the detail limit");
    }
}
