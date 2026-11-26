//! The retained geometry's source and assembly rows (src/Render/NativeRetainedGeometry.cpp).
//!
//! AppendSource's source row: its index, eye, pure indirect kind, frame, draw tag, list and recording lease; the draw's
//! pipeline, root, input slots, arguments, index view and render target count; an indirect source's signature, pure
//! stride, operands and the caller's RVA (the first of 32 return addresses inside the game's image, measured from the
//! .cpp's frame); and the heads of its indirect evidence and snapshot rows.
//!
//! BuildSourceAssemblies' assembly rows: the head from the source row and whether the source's scope holds (its index,
//! an eye, a list, a generation and a root, no rejection, the source's root that root); the source's selector command
//! and its certification (a supported catalog family -- or the multilayer family with its original site coverage --
//! one complete, unrejected, admitted command of this occurrence, list, generation and root, the selectors evaluated
//! for this family); the tail; the pass assembly's marks (its result and failure, an assembled source and its
//! overlays) and the bank's (the banked sources and their tables).
//!
//! In the Rust build (`CVR_RUST_VRIK`) the .cpp lends its journal rows; the unit tests below check the rows.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::ID3D12CommandSignature;

use crate::geometry_commands::{DrawRecord, IndirectView};
use crate::indirect_policy::{describe, draw_kind, pure_draw_stride};

/// AppendSource's source row inputs beyond the draw.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceHeader {
    pub index: u64,
    pub side: u64,
    pub frame: u64,
    pub stage: u64,
    pub scope: u64,
    pub list: u64,
    pub generation: u64,
    pub context: u64,
    pub registry: u64,
}
const _: () = assert!(core::mem::size_of::<SourceHeader>() == 72 && core::mem::size_of::<IndirectView>() == 48);

/// The game image's window for a caller RVA.
pub const CALLER_WINDOW: u64 = 0x3000000;

/// The first of `frames` inside `[base, base + CALLER_WINDOW)`, as an RVA (0 when none).
pub fn caller_in(frames: &[u64], base: u64) -> u64 {
    frames.iter().copied().find(|&at| at >= base && at - base < CALLER_WINDOW).map_or(0, |at| at - base)
}

/// The caller RVA from 32 return addresses, `skip` frames above this function's.
#[inline(never)]
fn caller_rva(skip: u32) -> u64 {
    let mut addresses = [core::ptr::null_mut::<c_void>(); 32];
    // SAFETY: a plain stack walk into the local array.
    let count = unsafe { windows::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(skip, &mut addresses, None) } as usize;
    // SAFETY: the process image's handle.
    let base = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }.map_or(0, |module| module.0 as u64);
    let frames: Vec<u64> = addresses[..count.min(32)].iter().map(|&a| a as u64).collect();
    caller_in(&frames, base)
}

/// A signature's receipt kind and pure stride (0, 0 when null or unobserved).
fn signature_policy(signature: *mut c_void) -> (u32, u32) {
    // SAFETY: a live command signature or null, borrowed for this call.
    match unsafe { ID3D12CommandSignature::from_raw_borrowed(&signature) } {
        Some(signature) => {
            let receipt = describe(signature);
            (draw_kind(&receipt), pure_draw_stride(&receipt))
        }
        None => (0, 0),
    }
}

/// The source row (its kind: the indirect signature's pure kind, else 0); `policy` is the signature's kind and stride,
/// `caller` the caller's RVA.
pub fn source_row(row: &mut [u64; 48], header: &SourceHeader, draw: &DrawRecord, indirect: Option<(&IndirectView, (u32, u32), u64)>) -> u32 {
    let kind = indirect.map_or(0, |(_, (kind, _), _)| kind);
    row[0] = header.index;
    row[1] = header.side;
    row[2] = u64::from(kind);
    row[3] = header.frame;
    row[4] = header.stage;
    row[5] = header.scope;
    row[6] = header.list;
    row[7] = header.generation;
    row[8] = header.context;
    row[9] = header.registry;
    row[10] = draw.pipeline;
    row[11] = draw.root;
    row[19] = u64::from(draw.input_slot_mask);
    row[20] = u64::from(draw.indices);
    row[21] = u64::from(draw.instances);
    row[22] = u64::from(draw.first_index);
    row[23] = u64::from(draw.base_vertex as u32);
    row[24] = u64::from(draw.first_instance);
    row[25] = draw.index.BufferLocation;
    row[26] = u64::from(draw.index.SizeInBytes);
    row[27] = u64::from(draw.index.Format.0 as u32);
    row[46] = u64::from(draw.rt_count);
    if let Some((view, (_, stride), caller)) = indirect {
        row[29] = view.signature as u64;
        row[30] = u64::from(stride);
        row[31] = u64::from(view.max_command_count);
        row[32] = view.arguments as u64;
        row[33] = view.argument_offset;
        row[34] = view.count_buffer as u64;
        row[35] = view.count_offset;
        row[45] = caller;
    }
    kind
}
/// The indirect evidence row's head from the source row.
pub fn evidence_head(row: &[u64; 48], evidence: &mut [u64; 56]) {
    evidence[0] = row[0];
    evidence[1] = row[1];
    evidence[2] = row[45];
    evidence[14] = row[33];
    evidence[20] = row[31];
    evidence[21] = row[7];
    evidence[22] = row[8];
    evidence[23] = row[6];
    evidence[25] = row[32];
}
/// The indirect snapshot row's head from the source row.
pub fn snapshot_head(row: &[u64; 48], snapshot: &mut [u64; 24]) {
    snapshot[0] = row[0];
    snapshot[1] = row[1];
    snapshot[6] = row[6];
    snapshot[7] = row[7];
    snapshot[8] = row[0].wrapping_add(1);
    snapshot[9] = row[32];
    snapshot[10] = row[33];
    snapshot[19] = row[29];
    snapshot[22] = row[8];
}

/// The assembly row's counts from the source.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct AssemblyCounts {
    pub private_cbvs: u64,
    pub tables: u64,
    pub selector_slots: u64,
    pub assembly_tables: u64,
    pub missing_cbvs: u64,
    pub root: u64,
}
const _: () = assert!(core::mem::size_of::<AssemblyCounts>() == 48);

/// The assembly row's head; whether the source's scope holds.
pub fn assembly_head(row: &mut [u64; 32], index: u64, saved: &[u64; 48], counts: &AssemblyCounts) -> bool {
    row[0] = index;
    row[1] = saved[1];
    row[8] = counts.private_cbvs;
    row[11] = saved[6];
    row[12] = saved[7];
    row[13] = saved[0].wrapping_add(1);
    row[14] = saved[11];
    row[18] = counts.tables;
    row[19] = counts.selector_slots;
    row[6] = counts.tables.wrapping_add(counts.selector_slots);
    row[27] = counts.assembly_tables;
    row[29] = counts.missing_cbvs;
    saved[0] == index && saved[1] < 2 && saved[6] != 0 && saved[7] != 0 && saved[11] != 0 && saved[44] == 0 && counts.root == saved[11]
}
/// The source's selector command (row[16], row[17]) and its certification (row[9]); `catalog` is 0 without a catalog
/// family, 1 with an unsupported one, 2 with a supported one; `multilayer` the multilayer family with its original site
/// coverage; `evaluated` the selectors evaluated for this family. False without a command.
pub fn selector_certified(row: &mut [u64; 32], saved: &[u64; 48], selectors: &[[u64; 32]], catalog: u32, multilayer: bool, evaluated: bool) -> bool {
    let Some(selector) = selectors.iter().find(|command| command[0] == row[0] && command[1] == saved[1]) else { return false };
    row[16] = selector[2];
    row[17] = selector[11];
    let multilayer = selector[25] != 0 && multilayer && selector[27] != 0;
    let supported = catalog != 0 && (catalog == 2 || multilayer);
    row[9] = u64::from(
        supported
            && selector[2] == 1
            && selector[11] != 0
            && selector[23] == 0
            && selector[24] != 0
            && selector[5] == saved[0].wrapping_add(1)
            && selector[13] == saved[6]
            && selector[14] == saved[7]
            && selector[16] == saved[11]
            && evaluated,
    );
    true
}
/// The assembly row's tail: its failure, its tables and missing CBVs since the head, no bank yet.
pub fn assembly_tail(row: &mut [u64; 32], failure: u32, assembly_tables: u64, missing_cbvs: u64) {
    row[3] = u64::from(failure);
    row[28] = assembly_tables.wrapping_sub(row[27]);
    row[30] = missing_cbvs.wrapping_sub(row[29]);
    row[22..26].fill(0);
}
/// The pass assembly's marks on a source's row: its result and failure; assembled, the source and its overlays.
pub fn assembled(row: &mut [u64; 32], index: u64, result: i32, failure: u32, overlays: &mut [[u64; 32]]) {
    row[26] = u64::from(result as u32);
    row[3] = u64::from(failure);
    if result >= 0 {
        row[21] = 1;
        for overlay in overlays.iter_mut().filter(|o| o[0] == index) {
            overlay[28] = 1;
        }
    }
}
/// The bank's marks: each banked source's row and its tables.
pub fn banked(rows: &mut [[u64; 32]], indices: &[usize], tables: &mut [[u64; 16]]) {
    for &index in indices {
        let Some(row) = rows.get_mut(index) else { continue };
        row[22] = 1;
        let (first, count) = (row[27] as usize, row[28] as usize);
        for table in tables.iter_mut().skip(first).take(count) {
            table[13] = 1;
        }
    }
}

/// The captures' row heads from the source row `sr` (CaptureCbvSnapshots', CaptureIaSnapshots', CaptureInstanceWriter's
/// and CaptureT5Snapshot's): the source's index and eye, its recording and its position (the index + 1).
pub fn cbv_command_head(command: &mut [u64; 24], sr: &[u64; 48], side: u64, list: u64, generation: u64, context: u64) {
    command[0] = sr[0];
    command[1] = side;
    command[11] = list;
    command[12] = generation;
    command[13] = context;
    command[14] = sr[0].wrapping_add(1);
}
/// CaptureIaSnapshots' command head.
pub fn ia_command_head(command: &mut [u64; 16], sr: &[u64; 48], side: u64) {
    command[0] = sr[0];
    command[1] = side;
    command[10] = sr[6];
    command[11] = sr[7];
    command[12] = sr[0].wrapping_add(1);
}
/// CaptureInstanceWriter's row head: the instance slot's lease and its view.
pub fn instance_head(row: &mut [u64; 48], sr: &[u64; 48], side: u64, resource: u64, gpu: u64, view: &crate::instance_snapshot_view::VertexBufferView) {
    row[0] = sr[0];
    row[1] = side;
    row[2] = sr[2];
    row[8] = sr[10];
    row[9] = sr[12];
    row[10] = resource;
    row[11] = gpu;
    row[12] = view.buffer_location;
    row[13] = u64::from(view.size_in_bytes);
    row[14] = u64::from(view.stride_in_bytes);
}
/// CaptureInstanceWriter's probe row once captured: its readback and predicate, the source view's offset of the first
/// instance, the readback and predicate offsets, the recording and position, the CPU owner and blob place.
pub fn instance_probe(probe: &mut [u64; 24], row: &[u64; 48], sr: &[u64; 48], readback: u64, predicate: u64, offset: u64, predicate_offset: u64) {
    probe[2] = 1;
    probe[6] = readback;
    probe[7] = predicate;
    probe[8] = row[10];
    probe[9] = row[12].wrapping_sub(row[11]).wrapping_add(sr[24].wrapping_mul(row[14]));
    probe[10] = offset;
    probe[11] = row[7];
    probe[12] = predicate_offset;
    probe[13] = sr[6];
    probe[14] = sr[7];
    probe[15] = sr[0].wrapping_add(1);
    probe[16] = row[5];
    probe[17] = row[6];
}
/// CaptureT5Snapshot's row head: the multilayer T5's resource and stamp, the recording, the position.
pub fn t5_head(row: &mut [u64; 32], sr: &[u64; 48], side: u64, resource: u64, stamp_gpu: u64, stamp_heap: u64, context: u64, generation: u64) {
    row[0] = sr[0];
    row[1] = side;
    row[4] = resource;
    row[12] = stamp_gpu;
    row[13] = stamp_heap;
    row[14] = context;
    row[15] = generation;
    row[20] = sr[0].wrapping_add(1);
}

/// The CBV snapshot command's head.
///
/// # Safety
/// The rows are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_cbv_command_head(command: *mut [u64; 24], sr: *const [u64; 48], side: u64, list: u64, generation: u64, context: u64) {
    // SAFETY: as documented.
    unsafe { cbv_command_head(&mut *command, &*sr, side, list, generation, context) };
}
/// The IA command's head.
///
/// # Safety
/// The rows are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_ia_command_head(command: *mut [u64; 16], sr: *const [u64; 48], side: u64) {
    // SAFETY: as documented.
    unsafe { ia_command_head(&mut *command, &*sr, side) };
}
/// The instance writer row's head.
///
/// # Safety
/// The rows and the view are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_head(row: *mut [u64; 48], sr: *const [u64; 48], side: u64, resource: u64, gpu: u64, view: *const crate::instance_snapshot_view::VertexBufferView) {
    // SAFETY: as documented.
    unsafe { instance_head(&mut *row, &*sr, side, resource, gpu, &*view) };
}
/// The instance probe row.
///
/// # Safety
/// The rows are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_instance_probe(probe: *mut [u64; 24], row: *const [u64; 48], sr: *const [u64; 48], readback: u64, predicate: u64, offset: u64, predicate_offset: u64) {
    // SAFETY: as documented.
    unsafe { instance_probe(&mut *probe, &*row, &*sr, readback, predicate, offset, predicate_offset) };
}
/// The T5 snapshot row's head.
///
/// # Safety
/// The rows are live.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_nrg_t5_head(row: *mut [u64; 32], sr: *const [u64; 48], side: u64, resource: u64, stamp_gpu: u64, stamp_heap: u64, context: u64, generation: u64) {
    // SAFETY: as documented.
    unsafe { t5_head(&mut *row, &*sr, side, resource, stamp_gpu, stamp_heap, context, generation) };
}

fn rows_mut<'a, T>(rows: *mut T, count: usize) -> &'a mut [T] {
    if rows.is_null() || count == 0 {
        &mut []
    } else {
        // SAFETY: the caller's rows of `count`.
        unsafe { core::slice::from_raw_parts_mut(rows, count) }
    }
}

/// The source row; its kind.
///
/// # Safety
/// `row`, `header` and `draw` are live; `indirect` null or a live view. Called from AppendSource itself, so the caller
/// RVA's return addresses start at its frame.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C-unwind" fn cvr_nrg_source_row(row: *mut [u64; 48], header: *const SourceHeader, draw: *const DrawRecord, indirect: *const IndirectView) -> u32 {
    // SAFETY: as documented.
    let view = unsafe { indirect.as_ref() };
    // Skip caller_rva's and this function's frames: the first address is AppendSource's, as its own walk had it.
    let lent = view.map(|view| (view, signature_policy(view.signature), caller_rva(2)));
    // SAFETY: as documented.
    unsafe { source_row(&mut *row, &*header, &*draw, lent) }
}
/// The evidence and snapshot heads (either null).
///
/// # Safety
/// `row` is live; `evidence` and `snapshot` null or live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_source_heads(row: *const [u64; 48], evidence: *mut [u64; 56], snapshot: *mut [u64; 24]) {
    // SAFETY: as documented.
    let row = unsafe { &*row };
    if let Some(evidence) = unsafe { evidence.as_mut() } {
        evidence_head(row, evidence);
    }
    if let Some(snapshot) = unsafe { snapshot.as_mut() } {
        snapshot_head(row, snapshot);
    }
}
/// The assembly row's head; whether the scope holds.
///
/// # Safety
/// The rows and counts are live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_assembly_head(row: *mut [u64; 32], index: u64, saved: *const [u64; 48], counts: *const AssemblyCounts) -> bool {
    // SAFETY: as documented.
    unsafe { assembly_head(&mut *row, index, &*saved, &*counts) }
}
/// The selector certification; whether the source has a command.
///
/// # Safety
/// The rows are live; `selectors` holds `count` commands.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_assembly_selector(row: *mut [u64; 32], saved: *const [u64; 48], selectors: *mut [u64; 32], count: usize, catalog: u32, multilayer: bool, evaluated: bool) -> bool {
    // SAFETY: as documented.
    unsafe { selector_certified(&mut *row, &*saved, rows_mut(selectors, count), catalog, multilayer, evaluated) }
}
/// The assembly row's tail.
///
/// # Safety
/// `row` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_assembly_tail(row: *mut [u64; 32], failure: u32, assembly_tables: u64, missing_cbvs: u64) {
    // SAFETY: as documented.
    assembly_tail(unsafe { &mut *row }, failure, assembly_tables, missing_cbvs);
}
/// The pass assembly's marks.
///
/// # Safety
/// `row` is live; `overlays` holds `count` rows.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_assembly_assembled(row: *mut [u64; 32], index: u64, result: i32, failure: u32, overlays: *mut [u64; 32], count: usize) {
    // SAFETY: as documented.
    assembled(unsafe { &mut *row }, index, result, failure, rows_mut(overlays, count));
}
/// The bank's marks.
///
/// # Safety
/// The rows hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_nrg_assembly_banked(rows: *mut [u64; 32], count: usize, indices: *const usize, index_count: usize, tables: *mut [u64; 16], table_count: usize) {
    let indices = if indices.is_null() || index_count == 0 { &[][..] } else { unsafe { core::slice::from_raw_parts(indices, index_count) } };
    banked(rows_mut(rows, count), indices, rows_mut(tables, table_count));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw() -> DrawRecord {
        // SAFETY: plain data.
        let mut d: DrawRecord = unsafe { core::mem::zeroed() };
        d.pipeline = 0x100;
        d.root = 0x200;
        d.input_slot_mask = 0x81;
        d.indices = 36;
        d.instances = 4;
        d.first_index = 6;
        d.base_vertex = -2;
        d.first_instance = 3;
        d.index.BufferLocation = 0x9000;
        d.index.SizeInBytes = 72;
        d.index.Format = windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R16_UINT;
        d.rt_count = 3;
        d
    }

    #[test]
    fn source_rows() {
        let header = SourceHeader { index: 7, side: 1, frame: 99, stage: 4, scope: 5, list: 0xA0, generation: 11, context: 12, registry: 13 };
        let mut row = [0u64; 48];
        row[28] = 77;
        assert_eq!(source_row(&mut row, &header, &draw(), None), 0);
        assert_eq!(row[..12], [7, 1, 0, 99, 4, 5, 0xA0, 11, 12, 13, 0x100, 0x200]);
        assert_eq!(row[19..28], [0x81, 36, 4, 6, 0xFFFF_FFFE, 3, 0x9000, 72, 57]);
        assert_eq!((row[28], row[29], row[45], row[46]), (77, 0, 0, 3), "a direct draw has no indirect fields");
        let view = IndirectView { signature: 0x300 as *mut c_void, max_command_count: 8, arguments: 0x400 as *mut c_void, argument_offset: 16, count_buffer: 0x500 as *mut c_void, count_offset: 4 };
        assert_eq!(source_row(&mut row, &header, &draw(), Some((&view, (1, 20), 0x1234))), 1);
        assert_eq!((row[2], row[29..36].to_vec(), row[45]), (1, vec![0x300, 20, 8, 0x400, 16, 0x500, 4], 0x1234));
        let (mut evidence, mut snapshot) = ([0u64; 56], [0u64; 24]);
        evidence_head(&row, &mut evidence);
        snapshot_head(&row, &mut snapshot);
        assert_eq!([evidence[0], evidence[1], evidence[2], evidence[14], evidence[20], evidence[21], evidence[22], evidence[23], evidence[25]], [7, 1, 0x1234, 16, 8, 11, 12, 0xA0, 0x400]);
        assert_eq!([snapshot[0], snapshot[1], snapshot[6], snapshot[7], snapshot[8], snapshot[9], snapshot[10], snapshot[19], snapshot[22]], [7, 1, 0xA0, 11, 8, 0x400, 16, 0x300, 12]);
        assert_eq!(caller_in(&[5, 0x1000_0010, 0x1000_0020], 0x1000_0000), 0x10, "the first frame inside the image");
        assert_eq!(caller_in(&[0x1000_0000 + CALLER_WINDOW], 0x1000_0000), 0, "beyond the image's window");
        // The real walk from this test finds no frame of the test binary's image only when that is absurd; just run it.
        let _ = caller_rva(0);
        // An unobserved signature: kind and stride 0.
        assert_eq!(signature_policy(core::ptr::null_mut()), (0, 0));
    }

    #[test]
    fn capture_heads() {
        let mut sr = [0u64; 48];
        sr[0] = 6;
        sr[2] = 1;
        sr[6] = 0xA0;
        sr[7] = 9;
        sr[10] = 0x100;
        sr[12] = 44;
        sr[24] = 3;
        let mut cbv = [0u64; 24];
        cbv_command_head(&mut cbv, &sr, 1, 0xA0, 9, 12);
        assert_eq!([cbv[0], cbv[1], cbv[11], cbv[12], cbv[13], cbv[14]], [6, 1, 0xA0, 9, 12, 7]);
        let mut ia = [0u64; 16];
        ia_command_head(&mut ia, &sr, 1);
        assert_eq!([ia[0], ia[1], ia[10], ia[11], ia[12]], [6, 1, 0xA0, 9, 7]);
        let view = crate::instance_snapshot_view::VertexBufferView { buffer_location: 0x5040, size_in_bytes: 6 << 20, stride_in_bytes: 48 };
        let mut row = [0u64; 48];
        instance_head(&mut row, &sr, 1, 0x700, 0x5000, &view);
        assert_eq!(row[..3], [6, 1, 1]);
        assert_eq!(row[8..15], [0x100, 44, 0x700, 0x5000, 0x5040, 6 << 20, 48]);
        row[5] = 0x900;
        row[6] = 33;
        row[7] = 96;
        let mut probe = [0u64; 24];
        instance_probe(&mut probe, &row, &sr, 0xB0, 0xC0, 128, 48);
        assert_eq!(probe[2], 1);
        assert_eq!(probe[6..18], [0xB0, 0xC0, 0x700, 0x40 + 3 * 48, 128, 96, 48, 0xA0, 9, 7, 0x900, 33], "the first instance's offset in the view");
        let mut t5 = [0u64; 32];
        t5_head(&mut t5, &sr, 0, 0xD0, 0xE000, 0xF0, 12, 9);
        assert_eq!([t5[0], t5[1], t5[4], t5[12], t5[13], t5[14], t5[15], t5[20]], [6, 0, 0xD0, 0xE000, 0xF0, 12, 9, 7]);
    }

    #[test]
    fn assembly_rows() {
        let mut saved = [0u64; 48];
        saved[0] = 3;
        saved[1] = 1;
        saved[6] = 0xA0;
        saved[7] = 9;
        saved[11] = 0x200;
        let counts = AssemblyCounts { private_cbvs: 2, tables: 5, selector_slots: 1, assembly_tables: 40, missing_cbvs: 6, root: 0x200 };
        let mut row = [0u64; 32];
        assert!(assembly_head(&mut row, 3, &saved, &counts));
        assert_eq!([row[0], row[1], row[6], row[8], row[11], row[12], row[13], row[14], row[18], row[19], row[27], row[29]], [3, 1, 6, 2, 0xA0, 9, 4, 0x200, 5, 1, 40, 6]);
        for (at, value) in [(0usize, 4u64), (1, 2), (6, 0), (7, 0), (11, 0), (44, 1)] {
            let mut bad = saved;
            bad[at] = value;
            assert!(!assembly_head(&mut [0; 32], 3, &bad, &counts), "scope field {at}");
        }
        assert!(!assembly_head(&mut [0; 32], 3, &saved, &AssemblyCounts { root: 0x201, ..counts }), "another root");

        let mut selector = [0u64; 32];
        selector[0] = 3;
        selector[1] = 1;
        selector[2] = 1;
        selector[11] = 1;
        selector[24] = 1;
        selector[5] = 4;
        selector[13] = 0xA0;
        selector[14] = 9;
        selector[16] = 0x200;
        let mut other = selector;
        other[1] = 0;
        let mut selectors = [other, selector];
        assert!(selector_certified(&mut row, &saved, &selectors, 2, false, true));
        assert_eq!((row[9], row[16], row[17]), (1, 1, 1));
        assert!(selector_certified(&mut row, &saved, &selectors, 2, false, false));
        assert_eq!(row[9], 0, "unevaluated selectors");
        assert!(selector_certified(&mut row, &saved, &selectors, 1, false, true));
        assert_eq!(row[9], 0, "an unsupported catalog family");
        selectors[1][25] = 1;
        selectors[1][27] = 1;
        assert!(selector_certified(&mut row, &saved, &selectors, 1, true, true));
        assert_eq!(row[9], 1, "the multilayer family");
        assert!(selector_certified(&mut row, &saved, &selectors, 0, true, true));
        assert_eq!(row[9], 0, "no catalog family");
        for (at, value) in [(2usize, 2u64), (11, 0), (23, 1), (24, 0), (5, 5), (13, 1), (14, 1), (16, 1)] {
            let mut bad = selectors;
            bad[1][at] = value;
            row[9] = 7;
            assert!(selector_certified(&mut row, &saved, &bad, 2, false, true));
            assert_eq!(row[9], 0, "selector field {at}");
        }
        assert!(!selector_certified(&mut row, &saved, &selectors[..1], 2, false, true), "no command of this eye");

        row[22..26].fill(9);
        assembly_tail(&mut row, 13, 43, 6);
        assert_eq!((row[3], row[28], row[30], &row[22..26]), (13, 3, 0, &[0u64; 4][..]));
        let mut overlays = [[0u64; 32]; 2];
        overlays[0][0] = 3;
        overlays[1][0] = 4;
        assembled(&mut row, 3, -2147024809, 14, &mut overlays);
        assert_eq!((row[26], row[3], row[21], overlays[0][28]), (0x80070057, 14, 0, 0), "a failed assembly marks nothing");
        assembled(&mut row, 3, 0, 0, &mut overlays);
        assert_eq!((row[26], row[3], row[21], overlays[0][28], overlays[1][28]), (0, 0, 1, 1, 0));
        let mut rows = [[0u64; 32]; 2];
        rows[1] = row;
        let mut tables = [[0u64; 16]; 44];
        banked(&mut rows, &[1], &mut tables);
        assert_eq!(rows[1][22], 1);
        assert_eq!(tables.iter().map(|t| t[13]).sum::<u64>(), 3);
        assert_eq!((tables[39][13], tables[40][13], tables[42][13], tables[43][13]), (0, 1, 1, 0));
    }
}
