//! The native prepared pair's recording and its exit comparisons (src/Render/NativePreparedPair.cpp):
//!
//! - `ImageCopies` -- the readback planes: per eye each color slot and the depth's two planes, three versions (the
//!   reference and paired arrays' eye slice, the eye's baseline target) and a fourth (the native-rate array) with native
//!   exits, every version 512-aligned after the last, the rows and row bytes equal across versions (the rows the
//!   pass's height, the row bytes a whole number of pixels: the stride; depth 4 bytes, stencil 1 at component 4 of an
//!   8-byte stride); the readback buffer (READBACK, COPY_DEST) and the baselines kept by the list; the arrays and the
//!   baselines to COPY_SOURCE, every plane's copies, the baselines back.
//! - `RecordCommands`' queries and draws -- under the draw permission: each eye's reference draws (its array slice
//!   bound, every source's state, pipeline, bank and geometry) inside query `eye`; the paired plan inside query 2 (the
//!   paired targets rebound when the mask changes; a paired step draws the family's view-instanced pipeline under the
//!   mesh comparison's equal predicate, then each eye's original under its fallback predicate; a one-eye step its
//!   original under the permission); with native exits each eye's native-rate draws with the source's own rate image;
//!   then the predication cleared, the three queries resolved, the predicate, mesh, input and membership words copied
//!   to their readbacks. The C++ objects (the target chains, the banks, the geometry) through callbacks.
//! - `CompareNativeOutput` -- the exported output copy's ten planes against the native exit (or, without one, the
//!   paired image): the differing pixels per eye, the verdict.
//! - `CompareNativeExit` -- each eye's native exit planes against the reference and paired images (statistics and
//!   samples), against the native-rate image (rate statistics), the colors' differences attributed to the refined rate
//!   tiles; the verdicts and the acceptance row; the error when the exit is incomplete or not accepted.
//!
//! In the Rust build (`CVR_RUST_VRIK`) these run through `cvr_npp_image_copies`, `cvr_npp_record_draws`,
//! `cvr_npp_compare_output` and `cvr_npp_compare_exit`; the unit tests below run the copies on WARP and the draws and
//! comparisons against a recorded trace and in-memory readbacks.

use core::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

use crate::native_exit::{acceptance, accumulate, always_finer, attribute_color, compare_plane, rate_map_shape, readback_shape, NativeExitPlane, MAX_READBACK_BYTES};
use crate::prepared_pair_exit_stats::{rate_sample, verdict, write_sample};
use crate::prepared_pair_exports::{output_verdict, Step};
use crate::prepared_pair_images::Plane;

/// QueryStride: a pipeline statistics record and an occlusion count.
pub const QUERY_STRIDE: u64 = 96;
pub const PREDICATE_READBACK_OFFSET: u64 = 3 * QUERY_STRIDE;
pub const MESH_READBACK_OFFSET: u64 = PREDICATE_READBACK_OFFSET + 8;
pub const INPUT_READBACK_OFFSET: u64 = MESH_READBACK_OFFSET + 256 * 16;

fn borrowed<T: Interface>(address: *mut c_void) -> Option<T> {
    // SAFETY: a live D3D12 object the caller holds for the call.
    unsafe { T::from_raw_borrowed(&address) }.cloned()
}
fn unowned(resource: *mut c_void) -> core::mem::ManuallyDrop<Option<ID3D12Resource>> {
    // SAFETY: the same pointer without an AddRef; ManuallyDrop never releases it.
    unsafe { core::mem::transmute::<*mut c_void, core::mem::ManuallyDrop<Option<ID3D12Resource>>>(resource) }
}
fn transition(resource: *mut c_void, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: unowned(resource),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: before,
                StateAfter: after,
            }),
        },
    }
}
fn target_state(slot: u32) -> D3D12_RESOURCE_STATES {
    if slot == 4 { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET }
}
/// Readback: a READBACK buffer in COPY_DEST, at most 512 MiB.
pub fn readback_buffer(device: &ID3D12Device, bytes: u64) -> Option<ID3D12Resource> {
    if bytes == 0 || bytes > MAX_READBACK_BYTES {
        return None;
    }
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: bytes,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_READBACK, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions are locals.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST, None, &mut out) }.ok()?;
    out
}

// ---- ImageCopies ----------------------------------------------------------------------------------------------------

/// The copies' targets: the reference, paired and native-rate arrays by slot (colors and depth in slot 4), each eye's
/// baseline targets by slot; `versions` 4 with native exits, else 3.
#[repr(C)]
pub struct ImageTargets {
    pub chains: [[*mut c_void; 5]; 3],
    pub baselines: [[*mut c_void; 5]; 2],
    pub colors: u32,
    pub width: u32,
    pub height: u32,
    pub versions: u32,
}
/// The planes (into the C++ half's room), the readback created (with its reference) and its bytes once kept.
#[repr(C)]
pub struct ImageOut {
    pub planes: *mut Plane,
    pub capacity: usize,
    pub count: usize,
    pub readback: *mut c_void,
    pub bytes: u64,
}
/// The list's keeping of one object, and the internal-commands scope from the barriers on.
#[repr(C)]
pub struct ImageHost {
    pub user: *mut c_void,
    pub keep: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, *mut c_void) -> bool,
    pub internal: unsafe extern "C-unwind" fn(*mut c_void),
}
const _: () = assert!(
    core::mem::size_of::<ImageTargets>() == 216 && core::mem::size_of::<ImageOut>() == 40 && core::mem::size_of::<ImageHost>() == 24
);
fn desc_of(resource: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
    // SAFETY: a live resource.
    borrowed::<ID3D12Resource>(resource).map(|r| unsafe { r.GetDesc() })
}
/// The planes of the copies (false on a footprint the copies refuse or a full room).
pub fn image_planes(device: &ID3D12Device, t: &ImageTargets, out: &mut ImageOut) -> Option<u64> {
    let mut size = 0u64;
    let versions = t.versions.min(4) as usize;
    out.count = 0;
    for eye in 0..2u32 {
        for slot in 0..5u32 {
            if slot >= t.colors && slot != 4 {
                continue;
            }
            let desc = desc_of(t.chains[0][slot as usize])?;
            let baseline = desc_of(t.baselines[eye as usize][slot as usize])?;
            for plane in 0..if slot == 4 { 2u32 } else { 1 } {
                let mut row = Plane { eye, slot, plane, ..Plane::default() };
                let (mut counts, mut widths) = ([0u32; 4], [0u64; 4]);
                for version in 0..versions {
                    let mut total = 0u64;
                    let (d, sub) = if version == 2 { (&baseline, plane) } else { (&desc, eye + plane * 2) };
                    // SAFETY: a live device; the outputs are locals.
                    unsafe {
                        device.GetCopyableFootprints(d, sub, 1, size, Some(&mut row.layouts[version]), Some(&mut counts[version]), Some(&mut widths[version]), Some(&mut total))
                    };
                    if total == u64::MAX || size > u64::MAX - total || size + total > u64::MAX - 511 {
                        return None;
                    }
                    size = (size + total + 511) & !511;
                }
                if counts[0] != counts[1] || counts[0] != counts[2] || counts[0] != t.height || widths[0] != widths[1] || widths[0] != widths[2] || t.width == 0 || widths[0] % t.width as u64 != 0 {
                    return None;
                }
                if versions == 4 && (counts[3] != counts[0] || widths[3] != widths[0]) {
                    return None;
                }
                row.rows = counts[0];
                row.stride = (widths[0] / t.width as u64) as u32;
                row.bytes = if slot == 4 && plane != 0 { 1 } else { 4 };
                row.component = if slot == 4 && plane != 0 && row.stride == 8 { 4 } else { 0 };
                if row.stride == 0 || row.component + row.bytes > row.stride || out.count >= out.capacity {
                    return None;
                }
                // SAFETY: the C++ half's room for `capacity` planes.
                unsafe { *out.planes.add(out.count) = row };
                out.count += 1;
            }
        }
    }
    Some(size)
}
/// ImageCopies.
///
/// # Safety
/// The targets are live resources in their render states (the depth DEPTH_WRITE), the list is recording.
pub unsafe fn image_copies(device: &ID3D12Device, list: &ID3D12GraphicsCommandList, t: &ImageTargets, out: &mut ImageOut, host: &ImageHost) -> bool {
    let Some(size) = image_planes(device, t, out) else { return false };
    let Some(readback) = readback_buffer(device, size) else { return false };
    let raw = readback.into_raw();
    out.readback = raw;
    // SAFETY: the C++ half's callbacks.
    if !unsafe { (host.keep)(host.user, list.as_raw(), raw) } {
        return false;
    }
    out.bytes = size;
    const BASELINE: [u32; 4] = [0, 1, 2, 4];
    for eye in 0..2 {
        for slot in BASELINE {
            // SAFETY: as above.
            if !unsafe { (host.keep)(host.user, list.as_raw(), t.baselines[eye][slot as usize]) } {
                return false;
            }
        }
    }
    // SAFETY: as above.
    unsafe { (host.internal)(host.user) };
    let versions = t.versions.min(4);
    // SAFETY: the list recording; the resources are the caller's.
    unsafe {
        for slot in 0..5u32 {
            if slot >= t.colors && slot != 4 {
                continue;
            }
            for chain in 0..3 {
                if chain == 2 && versions != 4 {
                    continue;
                }
                list.ResourceBarrier(&[transition(t.chains[chain][slot as usize], target_state(slot), D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            }
        }
        for eye in 0..2 {
            for slot in BASELINE {
                list.ResourceBarrier(&[transition(t.baselines[eye][slot as usize], target_state(slot), D3D12_RESOURCE_STATE_COPY_SOURCE)]);
            }
        }
        let planes = core::slice::from_raw_parts(out.planes, out.count);
        for row in planes {
            for version in 0..versions as usize {
                let from = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: unowned(match version {
                        2 => t.baselines[row.eye as usize][row.slot as usize],
                        3 => t.chains[2][row.slot as usize],
                        1 => t.chains[1][row.slot as usize],
                        _ => t.chains[0][row.slot as usize],
                    }),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: if version == 2 { row.plane } else { row.eye + row.plane * 2 } },
                };
                let to = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: unowned(raw),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { PlacedFootprint: row.layouts[version] },
                };
                list.CopyTextureRegion(&to, 0, 0, 0, &from, None);
            }
        }
        for eye in 0..2 {
            for slot in BASELINE {
                list.ResourceBarrier(&[transition(t.baselines[eye][slot as usize], D3D12_RESOURCE_STATE_COPY_SOURCE, target_state(slot))]);
            }
        }
    }
    true
}

// ---- RecordCommands' queries and draws ------------------------------------------------------------------------------

/// A source's draw state (its owned draw's original pipeline, rate image, topology, stencil, blend factor, shading rate,
/// viewport and scissor).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SourceState {
    pub pipeline: *mut c_void,
    pub rate_image: *mut c_void,
    pub topology: i32,
    pub stencil: u32,
    pub blend: [f32; 4],
    pub rate: i32,
    pub combiners: [i32; 2],
    pub viewport: D3D12_VIEWPORT,
    pub scissor: RECT,
}
/// A pair: its paired command, its two sources and the family's view-instanced pipeline.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct PairRow {
    pub command: u32,
    pub sources: [u32; 2],
    pub _pad: u32,
    pub pipeline: *mut c_void,
}
/// The draws' inputs.
#[repr(C)]
pub struct Draws {
    /// The pipeline statistics and occlusion query heaps.
    pub queries: [*mut c_void; 2],
    pub query_readback: *mut c_void,
    pub predicate: *mut c_void,
    /// DrawPermission: the input comparison's predicates, else the pass's predicate.
    pub permission: *mut c_void,
    pub mesh: *mut c_void,
    pub inputs: *mut c_void,
    /// The membership's predicates, results and readback.
    pub membership: [*mut c_void; 3],
    /// The refined rate image (w.rates.Image()).
    pub rate_image: *mut c_void,
    pub sources: *const SourceState,
    pub source_count: usize,
    pub eyes: [*const u32; 2],
    pub eye_counts: [usize; 2],
    pub steps: *const Step,
    pub step_count: usize,
    /// The pair of each source (source_count entries).
    pub pair_for_source: *const u32,
    pub pairs: *const PairRow,
    pub pair_count: usize,
    pub mesh_pairs: u32,
    pub membership_sources: u32,
    pub native_rate: bool,
    pub has_membership: bool,
}
/// The exported counters the draws add to.
#[repr(C)]
pub struct DrawCounters {
    pub depth_state: *mut [u64; 16],
    pub pair_stats: *mut [u64; 16],
    pub batch_stats: *mut [u64; 16],
    pub mesh_stats: *mut [u64; 12],
    pub native_rate: *mut [u64; 16],
}
/// The C++ objects: a target chain bound (0 reference, 1 paired, 2 native rate) for an eye mask; a source's reference
/// bank and geometry; a paired command's bank and its left source's geometry.
#[repr(C)]
pub struct DrawHost {
    pub user: *mut c_void,
    pub targets: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32, u32) -> bool,
    pub source: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32) -> bool,
    pub paired: unsafe extern "C-unwind" fn(*mut c_void, *mut c_void, u32, u32) -> bool,
}
const _: () = assert!(
    core::mem::size_of::<SourceState>() == 96
        && core::mem::offset_of!(SourceState, viewport) == 52
        && core::mem::size_of::<PairRow>() == 24
        && core::mem::size_of::<Draws>() == 192
        && core::mem::offset_of!(Draws, mesh_pairs) == 176
        && core::mem::size_of::<DrawCounters>() == 40
        && core::mem::size_of::<DrawHost>() == 32
);
/// The commands the draws emit (a D3D12 list, or a trace in the tests).
pub trait Commands {
    /// BeginQuery on both heaps (pipeline statistics, then occlusion).
    fn begin(&mut self, query: u32);
    /// EndQuery on both heaps (occlusion, then pipeline statistics).
    fn end(&mut self, query: u32);
    /// SetPredication(resource, offset, EQUAL_ZERO); a null resource clears it.
    fn predicate(&mut self, resource: *mut c_void, offset: u64);
    /// The source's state; with a rate list the rate and `image` as the rate image.
    fn state(&mut self, source: &SourceState, image: *mut c_void);
    /// With a rate list, the rate image.
    fn rate_image(&mut self, image: *mut c_void);
    fn pipeline(&mut self, pipeline: *mut c_void);
    fn mask(&mut self, mask: u32);
    /// Both queries' data for `query` into the query readback.
    fn resolve(&mut self, query: u32);
    fn copy(&mut self, destination: *mut c_void, at: u64, source: *mut c_void, from: u64, bytes: u64);
}
/// The counters as arrays.
pub struct Counters<'a> {
    pub depth_state: &'a mut [u64; 16],
    pub pair_stats: &'a mut [u64; 16],
    pub batch_stats: &'a mut [u64; 16],
    pub mesh_stats: &'a mut [u64; 12],
    pub native_rate: &'a mut [u64; 16],
}
fn bump(value: &mut u64) {
    *value = value.wrapping_add(1);
}
unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if data.is_null() || count == 0 { &[] } else { unsafe { core::slice::from_raw_parts(data, count) } }
}
fn record<C: Commands>(c: &mut C, depth: &mut [u64; 16], query: u32, draw: impl FnOnce(&mut C) -> bool) -> bool {
    c.begin(query);
    let result = draw(c);
    c.end(query);
    bump(&mut depth[15]);
    result
}
/// The queries and draws (0, else RecordCommands' failure: 7 the reference or native-rate draws, 8 the paired plan).
///
/// # Safety
/// The arrays hold their counts; the callbacks are the C++ half's.
pub unsafe fn record_draws<C: Commands>(c: &mut C, list: *mut c_void, d: &Draws, n: Counters<'_>, host: &DrawHost) -> u32 {
    // SAFETY: as documented.
    let (sources, steps, pairs) = unsafe { (slice(d.sources, d.source_count), slice(d.steps, d.step_count), slice(d.pairs, d.pair_count)) };
    // SAFETY: as documented.
    let (eyes, pair_for_source) = unsafe { ([slice(d.eyes[0], d.eye_counts[0]), slice(d.eyes[1], d.eye_counts[1])], slice(d.pair_for_source, d.source_count)) };
    // SAFETY: the C++ half's callbacks, each with the list.
    let targets = |chain: u32, mask: u32| unsafe { (host.targets)(host.user, list, chain, mask) };
    let source = |index: u32| unsafe { (host.source)(host.user, list, index) };
    let paired = |command: u32, geometry: u32| unsafe { (host.paired)(host.user, list, command, geometry) };
    let Counters { depth_state, pair_stats, batch_stats, mesh_stats, native_rate } = n;
    c.predicate(d.permission, 0);
    for eye in 0..2u32 {
        c.mask(u32::MAX);
        if !targets(0, 1 << eye) {
            return 7;
        }
        let drawn = record(c, depth_state, eye, |c| {
            for &index in eyes[eye as usize] {
                let Some(state) = sources.get(index as usize) else { return false };
                c.state(state, d.rate_image);
                c.pipeline(state.pipeline);
                if !source(index) {
                    return false;
                }
                bump(&mut pair_stats[4]);
                bump(&mut batch_stats[7]);
            }
            true
        });
        if !drawn {
            return 7;
        }
    }
    let drawn = record(c, depth_state, 2, |c| {
        let mut bound = 0u32;
        for step in steps {
            if bound != step.mask {
                if !targets(1, step.mask) {
                    return false;
                }
                bound = step.mask;
            }
            let eye = usize::from(step.mask == 2);
            let Some(&index) = eyes[eye].get(step.source[eye] as usize) else { return false };
            let Some(state) = sources.get(index as usize) else { return false };
            c.state(state, d.rate_image);
            if step.mask == 3 {
                let Some(pair) = pair_for_source.get(index as usize).and_then(|&p| pairs.get(p as usize)) else { return false };
                c.pipeline(pair.pipeline);
                c.mask(3);
                c.predicate(d.mesh, u64::from(pair.command) * 16);
                // The GPU compares every consumed mesh view and the exact instance slice: the left IA reused directly.
                if !paired(pair.command, pair.sources[0]) {
                    return false;
                }
                bump(&mut mesh_stats[10]);
                bump(&mut batch_stats[8]);
                c.predicate(d.mesh, u64::from(pair.command) * 16 + 8);
                for fallback_eye in 0..2u32 {
                    let fallback_index = pair.sources[fallback_eye as usize];
                    bound = 1 << fallback_eye;
                    if !targets(1, bound) {
                        return false;
                    }
                    let Some(fallback) = sources.get(fallback_index as usize) else { return false };
                    c.state(fallback, d.rate_image);
                    c.pipeline(fallback.pipeline);
                    c.mask(u32::MAX);
                    if !source(fallback_index) {
                        return false;
                    }
                    bump(&mut mesh_stats[11]);
                    bump(&mut batch_stats[8]);
                }
            } else {
                c.predicate(d.permission, 0);
                c.pipeline(state.pipeline);
                c.mask(u32::MAX);
                if !source(index) {
                    return false;
                }
            }
            if step.mask != 3 {
                bump(&mut batch_stats[8]);
            }
        }
        true
    });
    if !drawn {
        return 8;
    }
    if d.native_rate {
        c.predicate(d.permission, 0);
        c.mask(u32::MAX);
        for eye in 0..2u32 {
            if !targets(2, 1 << eye) {
                return 7;
            }
            for &index in eyes[eye as usize] {
                let Some(state) = sources.get(index as usize) else { return 7 };
                c.state(state, d.rate_image);
                c.rate_image(state.rate_image);
                c.pipeline(state.pipeline);
                if !source(index) {
                    return 7;
                }
                bump(&mut native_rate[2]);
            }
        }
    }
    c.predicate(core::ptr::null_mut(), 0);
    for query in 0..3 {
        c.resolve(query);
    }
    c.copy(d.query_readback, PREDICATE_READBACK_OFFSET, d.predicate, 0, 8);
    c.copy(d.query_readback, MESH_READBACK_OFFSET, d.mesh, 0, u64::from(d.mesh_pairs) * 16);
    if !d.inputs.is_null() {
        c.copy(d.query_readback, INPUT_READBACK_OFFSET, d.inputs, 0, 16);
    }
    if d.has_membership {
        let predicate_bytes = u64::from(d.membership_sources) * 16;
        c.copy(d.membership[2], 0, d.membership[0], 0, predicate_bytes);
        c.copy(d.membership[2], predicate_bytes, d.membership[1], 0, u64::from(d.membership_sources) * 32);
    }
    0
}
/// The draws on a D3D12 list.
pub struct ListCommands {
    pub list: ID3D12GraphicsCommandList,
    pub views: ID3D12GraphicsCommandList1,
    pub rate: Option<ID3D12GraphicsCommandList5>,
    pub queries: [Option<ID3D12QueryHeap>; 2],
    pub readback: Option<ID3D12Resource>,
}
impl Commands for ListCommands {
    fn begin(&mut self, query: u32) {
        // SAFETY: a recording list and its query heaps.
        unsafe {
            self.list.BeginQuery(self.queries[0].as_ref(), D3D12_QUERY_TYPE_PIPELINE_STATISTICS, query);
            self.list.BeginQuery(self.queries[1].as_ref(), D3D12_QUERY_TYPE_OCCLUSION, query);
        }
    }
    fn end(&mut self, query: u32) {
        // SAFETY: as above.
        unsafe {
            self.list.EndQuery(self.queries[1].as_ref(), D3D12_QUERY_TYPE_OCCLUSION, query);
            self.list.EndQuery(self.queries[0].as_ref(), D3D12_QUERY_TYPE_PIPELINE_STATISTICS, query);
        }
    }
    fn predicate(&mut self, resource: *mut c_void, offset: u64) {
        let resource = borrowed::<ID3D12Resource>(resource);
        // SAFETY: a recording list.
        unsafe { self.list.SetPredication(resource.as_ref(), offset, D3D12_PREDICATION_OP_EQUAL_ZERO) };
    }
    fn state(&mut self, s: &SourceState, image: *mut c_void) {
        // SAFETY: a recording list; the state's values.
        unsafe {
            self.list.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY(s.topology));
            self.list.OMSetStencilRef(s.stencil);
            self.list.OMSetBlendFactor(Some(&s.blend));
            if let Some(rate) = &self.rate {
                rate.RSSetShadingRate(D3D12_SHADING_RATE(s.rate), Some(s.combiners.as_ptr().cast()));
                rate.RSSetShadingRateImage(borrowed::<ID3D12Resource>(image).as_ref());
            }
            self.list.RSSetViewports(&[s.viewport]);
            self.list.RSSetScissorRects(&[s.scissor]);
        }
    }
    fn rate_image(&mut self, image: *mut c_void) {
        if let Some(rate) = &self.rate {
            // SAFETY: as above.
            unsafe { rate.RSSetShadingRateImage(borrowed::<ID3D12Resource>(image).as_ref()) };
        }
    }
    fn pipeline(&mut self, pipeline: *mut c_void) {
        // SAFETY: as above.
        unsafe { self.list.SetPipelineState(borrowed::<ID3D12PipelineState>(pipeline).as_ref()) };
    }
    fn mask(&mut self, mask: u32) {
        // SAFETY: as above.
        unsafe { self.views.SetViewInstanceMask(mask) };
    }
    fn resolve(&mut self, query: u32) {
        let at = u64::from(query) * QUERY_STRIDE;
        // SAFETY: as above; the readback holds the three records.
        unsafe {
            self.list.ResolveQueryData(self.queries[0].as_ref(), D3D12_QUERY_TYPE_PIPELINE_STATISTICS, query, 1, self.readback.as_ref(), at);
            self.list.ResolveQueryData(
                self.queries[1].as_ref(),
                D3D12_QUERY_TYPE_OCCLUSION,
                query,
                1,
                self.readback.as_ref(),
                at + core::mem::size_of::<D3D12_QUERY_DATA_PIPELINE_STATISTICS>() as u64,
            );
        }
    }
    fn copy(&mut self, destination: *mut c_void, at: u64, source: *mut c_void, from: u64, bytes: u64) {
        let (destination, source) = (borrowed::<ID3D12Resource>(destination), borrowed::<ID3D12Resource>(source));
        // SAFETY: as above.
        unsafe { self.list.CopyBufferRegion(destination.as_ref(), at, source.as_ref(), from, bytes) };
    }
}

// ---- the exit comparisons -------------------------------------------------------------------------------------------

/// A native exit: its readback and planes (a null readback: no exit).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExitView {
    pub readback: *mut c_void,
    pub planes: *const NativeExitPlane,
    pub count: usize,
}
/// NativeOutputCopy::Plane.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct OutputPlane {
    pub side: u32,
    pub slot: u32,
    pub plane: u32,
    pub footprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT,
    pub rows: u32,
    pub stride: u32,
    pub component: u32,
    pub bytes: u32,
}
/// CompareNativeOutput's inputs.
#[repr(C)]
pub struct OutputCompare {
    pub readback: *mut c_void,
    pub bytes: u64,
    pub copy_planes: *const OutputPlane,
    pub copy_count: usize,
    pub exits: [ExitView; 2],
    pub planes: *const Plane,
    pub plane_count: usize,
    pub paired: *const u8,
    pub paired_bytes: usize,
    pub width: u32,
    pub height: u32,
    pub recorded: bool,
}
/// CompareNativeExit's inputs.
#[repr(C)]
pub struct ExitCompare {
    pub exits: [ExitView; 2],
    pub planes: *const Plane,
    pub plane_count: usize,
    pub image: *const u8,
    pub image_bytes: usize,
    pub common: *const u8,
    pub common_bytes: usize,
    pub original: [*const u8; 2],
    pub original_bytes: [usize; 2],
    /// CyberpunkVR_PreparedBaseline[22] (the refused inputs).
    pub baseline: u64,
    /// CyberpunkVR_PreparedPairStats[6..8].
    pub pair_different: [u64; 2],
    pub width: u32,
    pub height: u32,
    pub tile: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub maps_ready: bool,
    pub allow_refinement: bool,
}
/// The exported exit arrays.
#[repr(C)]
pub struct ExitCounters {
    pub exit: *mut [u64; 24],
    pub samples: *mut [u64; 160],
    pub rate: *mut [u64; 16],
    pub attribution: *mut [u64; 12],
    pub acceptance: *mut [u64; 8],
}
const _: () = assert!(
    core::mem::size_of::<ExitView>() == 24
        && core::mem::size_of::<OutputPlane>() == 64
        && core::mem::offset_of!(OutputPlane, footprint) == 16
        && core::mem::size_of::<OutputCompare>() == 128
        && core::mem::size_of::<ExitCompare>() == 176
        && core::mem::offset_of!(ExitCompare, width) == 152
        && core::mem::size_of::<ExitCounters>() == 40
);
/// The readbacks' mapping (D3D12, or memory in the tests).
pub trait Readbacks {
    /// The readback's description (None: no readback).
    fn desc(&mut self, readback: *mut c_void) -> Option<D3D12_RESOURCE_DESC>;
    /// Its first `bytes` mapped for reading.
    fn map(&mut self, readback: *mut c_void, bytes: u64) -> Option<*const u8>;
    fn unmap(&mut self, readback: *mut c_void);
}
/// D3D12's readbacks.
pub struct Mapped;
impl Readbacks for Mapped {
    fn desc(&mut self, readback: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
        desc_of(readback)
    }
    fn map(&mut self, readback: *mut c_void, bytes: u64) -> Option<*const u8> {
        let resource = borrowed::<ID3D12Resource>(readback)?;
        let range = D3D12_RANGE { Begin: 0, End: bytes as usize };
        let mut mapped = core::ptr::null_mut();
        // SAFETY: a live readback.
        unsafe { resource.Map(0, Some(&range), Some(&mut mapped)) }.ok()?;
        (!mapped.is_null()).then_some(mapped as *const u8)
    }
    fn unmap(&mut self, readback: *mut c_void) {
        if let Some(resource) = borrowed::<ID3D12Resource>(readback) {
            // SAFETY: as above, mapped.
            unsafe { resource.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 0 })) };
        }
    }
}
fn exit_plane(row: &Plane, version: usize) -> NativeExitPlane {
    NativeExitPlane { slot: row.slot, plane: row.plane, rows: row.rows, stride: row.stride, component: row.component, bytes: row.bytes, layout: row.layouts[version] }
}
/// CompareNativeOutput (true when accepted; the exported copy not submitted passes).
///
/// # Safety
/// The arrays hold their counts.
pub unsafe fn compare_output<R: Readbacks>(r: &mut R, c: &OutputCompare, submitted: bool, baseline: u64, stats: &mut [u64; 24]) -> bool {
    if !submitted {
        return true;
    }
    stats[10] = baseline;
    // SAFETY: as documented.
    let (copy_planes, planes, paired) = unsafe { (slice(c.copy_planes, c.copy_count), slice(c.planes, c.plane_count), slice(c.paired, c.paired_bytes)) };
    if !c.recorded || c.readback.is_null() || copy_planes.len() != 10 {
        return false;
    }
    let Some(mapped) = r.map(c.readback, c.bytes) else { return false };
    // SAFETY: the mapped readback's bytes.
    let actual = unsafe { slice(mapped, c.bytes as usize) };
    let (mut valid, mut compared) = (true, 0u32);
    for side in 0..2u32 {
        if !valid {
            break;
        }
        let eye = (1 - side) as usize;
        let mut original: Option<*mut c_void> = None;
        let mut expected_data = paired;
        if stats[10] != 0 {
            let native = c.exits[eye];
            let shape = r.desc(native.readback);
            let Some(desc) = shape.filter(|desc| !native.readback.is_null() && readback_shape(desc)) else {
                valid = false;
                break;
            };
            let Some(data) = r.map(native.readback, desc.Width) else {
                valid = false;
                break;
            };
            original = Some(native.readback);
            // SAFETY: as above.
            expected_data = unsafe { slice(data, desc.Width as usize) };
        }
        for plane in copy_planes.iter().filter(|plane| plane.side == side) {
            let expected = if stats[10] != 0 {
                // SAFETY: as documented.
                let exits = unsafe { slice(c.exits[eye].planes, c.exits[eye].count) };
                match exits.iter().find(|value| value.slot == plane.slot && value.plane == plane.plane) {
                    Some(found) => *found,
                    None => {
                        valid = false;
                        break;
                    }
                }
            } else {
                match planes.iter().find(|value| value.eye as usize == eye && value.slot == plane.slot && value.plane == plane.plane) {
                    Some(found) => exit_plane(found, 1),
                    None => {
                        valid = false;
                        break;
                    }
                }
            };
            let copied = NativeExitPlane {
                slot: plane.slot,
                plane: plane.plane,
                rows: plane.rows,
                stride: plane.stride,
                component: plane.component,
                bytes: plane.bytes,
                layout: plane.footprint,
            };
            let Some(result) = compare_plane(c.width, c.height, &expected, expected_data, &expected, expected_data, &copied, actual) else {
                valid = false;
                break;
            };
            stats[6 + eye] = stats[6 + eye].wrapping_add(result.reference.count);
            compared += 1;
        }
        if let Some(readback) = original {
            r.unmap(readback);
        }
    }
    r.unmap(c.readback);
    output_verdict(valid, compared, stats)
}
/// The exit arrays as arrays.
pub struct ExitStats<'a> {
    pub exit: &'a mut [u64; 24],
    pub samples: &'a mut [u64; 160],
    pub rate: &'a mut [u64; 16],
    pub attribution: &'a mut [u64; 12],
    pub acceptance: &'a mut [u64; 8],
}
/// CompareNativeExit (true: the pass's error 43, the exit incomplete or not accepted).
///
/// # Safety
/// The arrays hold their counts.
pub unsafe fn compare_exit<R: Readbacks>(r: &mut R, c: &ExitCompare, s: &mut ExitStats<'_>) -> bool {
    s.exit[22] = c.baseline;
    s.rate[15] = c.baseline;
    // SAFETY: as documented.
    let (planes, image, common) = unsafe { (slice(c.planes, c.plane_count), slice(c.image, c.image_bytes), slice(c.common, c.common_bytes)) };
    let (mut complete, mut rate_complete, mut attributed, mut attributed_planes) = (true, true, c.maps_ready, 0u32);
    for eye in 0..2u32 {
        let exit = c.exits[eye as usize];
        let Some(desc) = r.desc(exit.readback).filter(|_| !exit.readback.is_null()) else {
            (complete, rate_complete, attributed) = (false, false, false);
            continue;
        };
        let Some(mapped) = r.map(exit.readback, desc.Width) else {
            (complete, rate_complete, attributed) = (false, false, false);
            continue;
        };
        // SAFETY: the mapped readback's bytes; the exit's planes.
        let (native_data, natives) = unsafe { (slice(mapped, desc.Width as usize), slice(exit.planes, exit.count)) };
        // SAFETY: as documented.
        let original = unsafe { slice(c.original[eye as usize], c.original_bytes[eye as usize]) };
        for native in natives {
            let Some(row) = planes.iter().find(|row| row.eye == eye && row.slot == native.slot && row.plane == native.plane) else {
                (complete, rate_complete, attributed) = (false, false, false);
                continue;
            };
            let (reference, optimized) = (exit_plane(row, 0), exit_plane(row, 1));
            let Some(comparison) = compare_plane(c.width, c.height, &reference, image, &optimized, image, native, native_data) else {
                (complete, rate_complete, attributed) = (false, false, false);
                continue;
            };
            accumulate(s.exit, eye, native, c.width, &comparison);
            write_sample(s.samples, eye, native, &comparison);
            let native_rate = exit_plane(row, 3);
            match compare_plane(c.width, c.height, &native_rate, image, &native_rate, image, native, native_data) {
                Some(rate) => rate_sample(s.rate, eye, native, c.width, rate.reference.count),
                None => rate_complete = false,
            }
            if native.slot < 3 && c.maps_ready {
                match attribute_color(c.width, c.height, &reference, image, native, native_data, c.tile, c.map_width, c.map_height, common, original) {
                    Some((inside, outside)) => {
                        let e = eye as usize;
                        s.attribution[5 + e] = s.attribution[5 + e].wrapping_add(inside);
                        s.attribution[7 + e] = s.attribution[7 + e].wrapping_add(outside);
                        attributed_planes += 1;
                    }
                    None => attributed = false,
                }
            }
        }
        r.unmap(exit.readback);
    }
    verdict(complete, rate_complete, attributed, attributed_planes, s.exit, s.rate, s.attribution);
    *s.acceptance = acceptance(c.allow_refinement, s.exit, s.rate, s.attribution, &c.pair_different);
    s.exit[20] == 0 || (s.exit[22] == 0 && s.acceptance[7] == 0)
}

// ---- the C ABI ------------------------------------------------------------------------------------------------------

/// ImageCopies.
///
/// # Safety
/// As image_copies; the device and list are live, `out` holds its room.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_image_copies(device: *mut c_void, list: *mut c_void, t: *const ImageTargets, out: *mut ImageOut, host: *const ImageHost) -> bool {
    let (Some(device), Some(list)) = (borrowed::<ID3D12Device>(device), borrowed::<ID3D12GraphicsCommandList>(list)) else { return false };
    // SAFETY: as documented.
    unsafe { image_copies(&device, &list, &*t, &mut *out, &*host) }
}
/// RecordCommands' queries and draws.
///
/// # Safety
/// As record_draws; the lists, heaps and resources are live, the counters the exported arrays.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_record_draws(list: *mut c_void, views: *mut c_void, rate: *mut c_void, d: *const Draws, n: *const DrawCounters, host: *const DrawHost) -> u32 {
    let (Some(command_list), Some(views)) = (borrowed::<ID3D12GraphicsCommandList>(list), borrowed::<ID3D12GraphicsCommandList1>(views)) else { return 3 };
    // SAFETY: as documented.
    let (d, n, host) = unsafe { (&*d, &*n, &*host) };
    let mut commands = ListCommands {
        list: command_list,
        views,
        rate: borrowed::<ID3D12GraphicsCommandList5>(rate),
        queries: [borrowed::<ID3D12QueryHeap>(d.queries[0]), borrowed::<ID3D12QueryHeap>(d.queries[1])],
        readback: borrowed::<ID3D12Resource>(d.query_readback),
    };
    // SAFETY: the exported arrays.
    let counters = unsafe {
        Counters { depth_state: &mut *n.depth_state, pair_stats: &mut *n.pair_stats, batch_stats: &mut *n.batch_stats, mesh_stats: &mut *n.mesh_stats, native_rate: &mut *n.native_rate }
    };
    // SAFETY: as documented.
    unsafe { record_draws(&mut commands, list, d, counters, host) }
}
/// CompareNativeOutput.
///
/// # Safety
/// As compare_output; `stats` is the exported array of 24.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_compare_output(c: *const OutputCompare, submitted: bool, baseline: u64, stats: *mut [u64; 24]) -> bool {
    // SAFETY: as documented.
    unsafe { compare_output(&mut Mapped, &*c, submitted, baseline, &mut *stats) }
}
/// CompareNativeExit (true: the pass's error 43).
///
/// # Safety
/// As compare_exit; the counters are the exported arrays.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_compare_exit(c: *const ExitCompare, n: *const ExitCounters) -> bool {
    // SAFETY: as documented.
    let n = unsafe { &*n };
    // SAFETY: the exported arrays.
    let mut stats = unsafe { ExitStats { exit: &mut *n.exit, samples: &mut *n.samples, rate: &mut *n.rate, attribution: &mut *n.attribution, acceptance: &mut *n.acceptance } };
    // SAFETY: as documented.
    unsafe { compare_exit(&mut Mapped, &*c, &mut stats) }
}

// ---- Poll's shading rate maps and the predicate word -----------------------------------------------------------------

/// Poll's rate maps: the refined image's pixels as read (`read` false when the read failed or saw invalid rates), each
/// source's side, the eyes' first sources and counts, the pass's and the rate map's extents.
#[repr(C)]
pub struct RateMaps {
    pub actual: *const u8,
    pub actual_bytes: usize,
    pub sides: *const u32,
    pub sources: usize,
    pub eye_front: [usize; 2],
    pub eye_counts: [usize; 2],
    pub width: u32,
    pub height: u32,
    pub tile: u32,
    pub map_width: u32,
    pub map_height: u32,
    pub read: bool,
    pub native: bool,
}
/// A source's eye-0 rate pixels (into the C++ half's buffer, valid until the next call) and its map's width and height.
pub type ReadRates = unsafe extern "C-unwind" fn(*mut c_void, usize, *mut *const u8, *mut usize, *mut u32, *mut u32) -> bool;
/// Stores a map: 0 and 1 an eye's original rates, 2 the common (refined) rates.
pub type StoreRates = unsafe extern "C-unwind" fn(*mut c_void, u32, *const u8, usize);
/// The rate maps' counters: CyberpunkVR_PreparedPairVrs (8), CyberpunkVR_PreparedPairVrsGpu (4) and
/// CyberpunkVR_PreparedVrsAttribution (12).
pub struct RateCounters<'a> {
    pub vrs: &'a mut [u64; 8],
    pub gpu: &'a mut [u64; 4],
    pub attribution: &'a mut [u64; 12],
}
const _: () = assert!(core::mem::size_of::<RateMaps>() == 88 && core::mem::offset_of!(RateMaps, width) == 64);
/// Poll's rate maps (true when valid; `ready` whether the native exits' attribution may use the maps): every source's
/// map read in order and folded into the finest common rates, which the refined image must equal; the first left and
/// right maps against it; with native exits each eye's maps of the map's extent and uniform, always at least as coarse
/// as the refined image.
///
/// # Safety
/// The arrays hold their counts; the callbacks are the C++ half's.
pub unsafe fn rate_maps(m: &RateMaps, user: *mut c_void, read: ReadRates, store: StoreRates, n: RateCounters<'_>, ready: &mut bool) -> bool {
    // SAFETY: as documented.
    let (actual, sides) = unsafe { (slice(m.actual, m.actual_bytes), slice(m.sides, m.sources)) };
    let mut valid = m.read;
    let (mut metadata, mut finer, mut uniform, mut counts) = (true, true, [true, true], [0usize; 2]);
    let (mut expected, mut left, mut right): (Vec<u8>, Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new(), Vec::new());
    let mut originals: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    for (i, &side) in sides.iter().enumerate() {
        if !valid {
            break;
        }
        let (mut data, mut bytes, mut map_width, mut map_height) = (core::ptr::null(), 0usize, 0u32, 0u32);
        // SAFETY: the C++ half's callback.
        if !unsafe { read(user, i, &mut data, &mut bytes, &mut map_width, &mut map_height) } {
            valid = false;
            break;
        }
        // SAFETY: the C++ half's buffer of `bytes`.
        let pixels = unsafe { slice(data, bytes) }.to_vec();
        if i == m.eye_front[0] {
            left = pixels.clone();
        }
        if i == m.eye_front[1] {
            right = pixels.clone();
        }
        if m.native {
            let eye = (1u32.wrapping_sub(side) & 1) as usize;
            metadata = metadata && map_width == m.map_width && map_height == m.map_height && pixels.len() == m.map_width as usize * m.map_height as usize;
            if counts[eye] == 0 {
                originals[eye] = pixels.clone();
            } else {
                uniform[eye] = uniform[eye] && originals[eye] == pixels;
            }
            counts[eye] += 1;
            finer = finer && always_finer(actual, &pixels);
        }
        if expected.is_empty() {
            expected = pixels;
        } else {
            match cvr_gpu::vrs_snapshot::build_finer(&expected, &pixels) {
                Some(refined) => expected = refined,
                None => {
                    valid = false;
                    expected.clear();
                }
            }
        }
    }
    for (eye, original) in originals.iter().enumerate() {
        if counts[eye] != 0 {
            // SAFETY: as above.
            unsafe { store(user, eye as u32, original.as_ptr(), original.len()) };
        }
    }
    if valid && actual.len() == expected.len() && left.len() == actual.len() && right.len() == actual.len() {
        for i in 0..actual.len() {
            n.gpu[3] = n.gpu[3].wrapping_add(u64::from(actual[i] != expected[i]));
            n.vrs[3] = n.vrs[3].wrapping_add(u64::from(left[i] != right[i]));
            n.vrs[4] = n.vrs[4].wrapping_add(u64::from(actual[i] != left[i]));
            n.vrs[5] = n.vrs[5].wrapping_add(u64::from(actual[i] != right[i]));
        }
        valid = n.gpu[3] == 0;
    } else {
        valid = false;
    }
    if m.native {
        let all = metadata && counts[0] == m.eye_counts[0] && counts[1] == m.eye_counts[1];
        for eye in 0..2 {
            n.attribution[3 + eye] = u64::from(counts[eye] == m.eye_counts[eye] && uniform[eye]);
        }
        n.attribution[9] = u64::from(valid && all);
        n.attribution[10] = u64::from(valid && all && finer);
        *ready = valid && all && finer && uniform[0] && uniform[1] && rate_map_shape(m.width, m.height, m.tile, m.map_width, m.map_height, actual.len());
        if *ready {
            // SAFETY: as above.
            unsafe { store(user, 2, actual.as_ptr(), actual.len()) };
        }
    }
    valid
}
/// The pass's predicate word written through its UPLOAD mapping (false when it does not map).
pub fn write_predicate(predicate: &ID3D12Resource, value: u64) -> bool {
    let mut mapped = core::ptr::null_mut();
    // SAFETY: a live UPLOAD buffer of at least 8 bytes.
    if unsafe { predicate.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)) }.is_err() || mapped.is_null() {
        return false;
    }
    // SAFETY: the mapped word; the write published before the GPU reads it.
    unsafe {
        core::ptr::write_volatile(mapped.cast::<u64>(), value);
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        predicate.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: 8 }));
    }
    true
}
/// Poll's rate maps.
///
/// # Safety
/// As rate_maps; the counters are the exported arrays.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_rate_maps(
    m: *const RateMaps,
    user: *mut c_void,
    read: ReadRates,
    store: StoreRates,
    vrs: *mut [u64; 8],
    gpu: *mut [u64; 4],
    attribution: *mut [u64; 12],
    ready: *mut bool,
) -> bool {
    // SAFETY: as documented.
    unsafe { rate_maps(&*m, user, read, store, RateCounters { vrs: &mut *vrs, gpu: &mut *gpu, attribution: &mut *attribution }, &mut *ready) }
}
/// The predicate word.
///
/// # Safety
/// `predicate` is a live UPLOAD buffer of at least 8 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_write_predicate(predicate: *mut c_void, value: u64) -> bool {
    borrowed::<ID3D12Resource>(predicate).is_some_and(|resource| write_predicate(&resource, value))
}

// ---- PrepareMesh's and PrepareInputs' compared ranges ---------------------------------------------------------------

/// GpuGeometryCompare's range row (CvrGeometryCompareRange).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareRange {
    pub pair: u32,
    pub _pad: u32,
    pub resources: [*mut c_void; 2],
    pub offsets: [u64; 2],
    pub bytes: u64,
}
impl Default for CompareRange {
    fn default() -> Self {
        CompareRange { pair: 0, _pad: 0, resources: [core::ptr::null_mut(); 2], offsets: [0; 2], bytes: 0 }
    }
}
/// A paired command and its two sources.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MeshPair {
    pub command: u32,
    pub sources: [u32; 2],
}
/// A source's owned geometry: its draw and held buffers (slot 0 the index buffer, 1..16 the vertex buffers).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct MeshSource {
    pub draw: *const crate::geometry_commands::DrawRecord,
    pub leases: [crate::geometry_commands::Lease; 17],
}
/// One check (or membership predicate) compared against expected bytes.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputPiece {
    pub resource: *mut c_void,
    pub offset: u64,
    pub data: *const u8,
    pub bytes: usize,
    /// The expected bytes padded to 4 after this piece.
    pub align: bool,
}
const _: () = assert!(
    core::mem::size_of::<CompareRange>() == 48
        && core::mem::size_of::<MeshPair>() == 12
        && core::mem::size_of::<MeshSource>() == 416
        && core::mem::size_of::<InputPiece>() == 40
);
/// PrepareMesh's ranges: per paired command its index slot and every vertex slot the first eye consumes, both eyes'
/// ranges inside their held buffers with the first eye's bytes (None when one is not).
///
/// # Safety
/// The sources' draws are live records.
pub unsafe fn mesh_ranges(pairs: &[MeshPair], sources: &[MeshSource]) -> Option<Vec<CompareRange>> {
    use crate::prepared_pair_ranges::{mesh_eye, MeshEye};
    let mut ranges = Vec::new();
    for pair in pairs {
        if pair.command == u32::MAX {
            continue;
        }
        let eyes = [sources.get(pair.sources[0] as usize)?, sources.get(pair.sources[1] as usize)?];
        // SAFETY: as documented.
        let first = unsafe { &*eyes[0].draw };
        for slot in 0..17u32 {
            if slot != 0 && first.input_slot_mask & (1 << (slot - 1)) == 0 {
                continue;
            }
            let mut range = CompareRange { pair: pair.command, ..CompareRange::default() };
            for (eye, source) in eyes.iter().enumerate() {
                // SAFETY: as documented.
                let draw = unsafe { &*source.draw };
                let lease = source.leases[slot as usize];
                let (address, bytes) = if slot != 0 {
                    let view = draw.vertices[(slot - 1) as usize];
                    (view.BufferLocation, u64::from(view.SizeInBytes))
                } else {
                    (draw.index.BufferLocation, u64::from(draw.index.SizeInBytes))
                };
                let view = MeshEye {
                    address,
                    bytes,
                    stride: draw.vertices[7].StrideInBytes,
                    first_instance: draw.first_instance,
                    instances: draw.instances,
                    present: !lease.resource.is_null(),
                    gpu: lease.gpu,
                    buffer_bytes: lease.bytes,
                };
                let (offset, extent) = mesh_eye(slot, &view, eye as u32, range.bytes)?;
                range.resources[eye] = lease.resource;
                range.offsets[eye] = offset;
                range.bytes = extent;
            }
            ranges.push(range);
        }
    }
    Some(ranges)
}
/// PrepareInputs' expected bytes: each piece's bytes after the last (padded to 4 where asked), its range against the
/// piece's source; with a fault the first byte flipped.
pub fn expected_inputs(pieces: &[InputPiece], fault: bool) -> (Vec<u8>, Vec<CompareRange>) {
    let (mut blob, mut ranges) = (Vec::new(), Vec::with_capacity(pieces.len()));
    for piece in pieces {
        ranges.push(CompareRange { resources: [piece.resource, core::ptr::null_mut()], offsets: [piece.offset, blob.len() as u64], bytes: piece.bytes as u64, ..CompareRange::default() });
        // SAFETY: the piece's expected bytes.
        blob.extend_from_slice(unsafe { slice(piece.data, piece.bytes) });
        if piece.align {
            blob.resize((blob.len() + 3) & !3, 0);
        }
    }
    if fault && !blob.is_empty() {
        blob[0] ^= 1;
    }
    (blob, ranges)
}
/// The expected bytes' UPLOAD buffer (GENERIC_READ), written and published.
pub fn expected_upload(device: &ID3D12Device, blob: &[u8]) -> Option<ID3D12Resource> {
    let desc = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: blob.len() as u64,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_UPLOAD, ..Default::default() };
    let mut out: Option<ID3D12Resource> = None;
    // SAFETY: a live device; the descriptions are locals.
    unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut out) }.ok()?;
    let upload = out?;
    let mut mapped = core::ptr::null_mut();
    // SAFETY: the new UPLOAD buffer of the blob's bytes.
    unsafe {
        upload.Map(0, Some(&D3D12_RANGE { Begin: 0, End: 0 }), Some(&mut mapped)).ok()?;
        if mapped.is_null() {
            return None;
        }
        core::ptr::copy_nonoverlapping(blob.as_ptr(), mapped.cast::<u8>(), blob.len());
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        upload.Unmap(0, Some(&D3D12_RANGE { Begin: 0, End: blob.len() }));
    }
    Some(upload)
}
/// PrepareMesh's ranges into the C++ half's room (false when a range is refused or the room is short).
///
/// # Safety
/// As mesh_ranges; the arrays hold their counts.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_mesh_ranges(
    pairs: *const MeshPair,
    pair_count: usize,
    sources: *const MeshSource,
    source_count: usize,
    out: *mut CompareRange,
    capacity: usize,
    count: *mut usize,
) -> bool {
    // SAFETY: as documented.
    let Some(ranges) = (unsafe { mesh_ranges(slice(pairs, pair_count), slice(sources, source_count)) }) else { return false };
    if ranges.len() > capacity {
        return false;
    }
    // SAFETY: the room holds `capacity` rows.
    unsafe {
        core::ptr::copy_nonoverlapping(ranges.as_ptr(), out, ranges.len());
        *count = ranges.len();
    }
    true
}
/// PrepareInputs' expected bytes, their UPLOAD buffer (handed over with its reference) and the ranges against it.
///
/// # Safety
/// The device is live; the pieces hold their count and `out` as many rows.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_expected_inputs(
    device: *mut c_void,
    pieces: *const InputPiece,
    count: usize,
    fault: bool,
    out: *mut CompareRange,
    upload: *mut *mut c_void,
    bytes: *mut u64,
) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    // SAFETY: as documented.
    let (blob, mut ranges) = expected_inputs(unsafe { slice(pieces, count) }, fault);
    let Some(buffer) = expected_upload(&device, &blob) else { return false };
    for range in &mut ranges {
        range.resources[1] = buffer.as_raw();
    }
    // SAFETY: as documented.
    unsafe {
        core::ptr::copy_nonoverlapping(ranges.as_ptr(), out, ranges.len());
        *bytes = blob.len() as u64;
        *upload = buffer.into_raw();
    }
    true
}

// ---- ValidateNativeExit ---------------------------------------------------------------------------------------------

/// ValidateNativeExit: the exit's five planes once each; its readback a READBACK buffer of the readback shape on the
/// pass's device; each plane the copy footprint of its baseline target's plane at the pass's extent, inside the readback,
/// no two planes overlapping.
///
/// # Safety
/// `baselines` holds the source's baseline targets by slot (live resources where used).
pub unsafe fn validate_exit(device: &ID3D12Device, readback: *mut c_void, planes: &[NativeExitPlane], baselines: &[*mut c_void; 5], width: u32, height: u32) -> bool {
    use crate::native_exit::{complete_planes, plane_bounds, validate_plane};
    let Some(buffer) = borrowed::<ID3D12Resource>(readback) else { return false };
    if !complete_planes(planes) {
        return false;
    }
    // SAFETY: a live readback.
    let desc = unsafe { buffer.GetDesc() };
    let mut actual: Option<ID3D12Device> = None;
    let (mut heap, mut flags) = (D3D12_HEAP_PROPERTIES::default(), D3D12_HEAP_FLAGS::default());
    // SAFETY: as above; the outputs are locals.
    let device_ok = unsafe { buffer.GetDevice(&mut actual) }.is_ok() && actual.as_ref().is_some_and(|d| d.as_raw() == device.as_raw());
    // SAFETY: as above.
    if !readback_shape(&desc) || !device_ok || unsafe { buffer.GetHeapProperties(Some(&mut heap), Some(&mut flags)) }.is_err() || heap.Type != D3D12_HEAP_TYPE_READBACK {
        return false;
    }
    let mut intervals: Vec<(u64, u64)> = Vec::with_capacity(planes.len());
    for row in planes {
        let Some(target) = baselines.get(row.slot as usize).and_then(|&r| borrowed::<ID3D12Resource>(r)) else { return false };
        // SAFETY: a live baseline target.
        let target_desc = unsafe { target.GetDesc() };
        let (mut expected, mut rows, mut row_bytes, mut total) = (D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(), 0u32, 0u64, 0u64);
        // SAFETY: a live device; the outputs are locals.
        unsafe { device.GetCopyableFootprints(&target_desc, row.plane, 1, 0, Some(&mut expected), Some(&mut rows), Some(&mut row_bytes), Some(&mut total)) };
        if total == u64::MAX || !validate_plane(row, width, height, desc.Width, &expected.Footprint, rows, row_bytes) {
            return false;
        }
        let Some(end) = plane_bounds(row, width, height, desc.Width) else { return false };
        if intervals.iter().any(|&(start, stop)| row.layout.Offset < stop && start < end) {
            return false;
        }
        intervals.push((row.layout.Offset, end));
    }
    true
}
/// ValidateNativeExit.
///
/// # Safety
/// As validate_exit; the planes hold their count.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_npp_validate_exit(
    device: *mut c_void,
    readback: *mut c_void,
    planes: *const NativeExitPlane,
    count: usize,
    baselines: *const [*mut c_void; 5],
    width: u32,
    height: u32,
) -> bool {
    let Some(device) = borrowed::<ID3D12Device>(device) else { return false };
    // SAFETY: as documented.
    unsafe { validate_exit(&device, readback, slice(planes, count), &*baselines, width, height) }
}

#[cfg(test)]
mod tests {
    //! The copies on WARP (the planes and the copied pixels of every version), the draws' command trace and the
    //! comparisons over in-memory readbacks.
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    thread_local! {
        static RATES: RefCell<Vec<(Vec<u8>, u32, u32)>> = const { RefCell::new(Vec::new()) };
        static RATE_BUFFER: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        static STORED: RefCell<Vec<(u32, Vec<u8>)>> = const { RefCell::new(Vec::new()) };
    }
    unsafe extern "C-unwind" fn read_rates(_: *mut c_void, index: usize, data: *mut *const u8, bytes: *mut usize, width: *mut u32, height: *mut u32) -> bool {
        let Some((pixels, w, h)) = RATES.with(|r| r.borrow().get(index).cloned()) else { return false };
        RATE_BUFFER.with(|b| {
            *b.borrow_mut() = pixels;
            unsafe {
                *data = b.borrow().as_ptr();
                *bytes = b.borrow().len();
                *width = w;
                *height = h;
            }
        });
        true
    }
    unsafe extern "C-unwind" fn store_rates(_: *mut c_void, which: u32, data: *const u8, bytes: usize) {
        let value = unsafe { core::slice::from_raw_parts(data, bytes) }.to_vec();
        STORED.with(|s| s.borrow_mut().push((which, value)));
    }
    fn run_rates(actual: &[u8], sources: Vec<(Vec<u8>, u32, u32)>, native: bool) -> (bool, bool, [u64; 8], [u64; 4], [u64; 12], Vec<(u32, Vec<u8>)>) {
        // Sources 0 and 2 the left eye (side 1), 1 the right (side 0).
        let sides = [1u32, 0, 1];
        RATES.with(|r| *r.borrow_mut() = sources);
        STORED.with(|s| s.borrow_mut().clear());
        let m = RateMaps {
            actual: actual.as_ptr(),
            actual_bytes: actual.len(),
            sides: sides.as_ptr(),
            sources: sides.len(),
            eye_front: [0, 1],
            eye_counts: [2, 1],
            width: 4,
            height: 2,
            tile: 2,
            map_width: 2,
            map_height: 1,
            read: true,
            native,
        };
        let (mut vrs, mut gpu, mut attribution, mut ready) = ([0u64; 8], [0u64; 4], [0u64; 12], false);
        let valid = unsafe { rate_maps(&m, core::ptr::null_mut(), read_rates, store_rates, RateCounters { vrs: &mut vrs, gpu: &mut gpu, attribution: &mut attribution }, &mut ready) };
        (valid, ready, vrs, gpu, attribution, STORED.with(|s| s.borrow().clone()))
    }
    #[test]
    fn rate_maps_fold() {
        // 1x2 (1) and 2x1 (4) fold to 1x1 (0); the refined image is that.
        let (valid, ready, vrs, gpu, attribution, stored) = run_rates(&[0, 5], vec![(vec![1, 5], 2, 1), (vec![4, 5], 2, 1), (vec![1, 5], 2, 1)], true);
        assert!(valid && ready, "the refined image equals the finest common rates; both eyes uniform");
        assert_eq!((gpu[3], vrs[3], vrs[4], vrs[5]), (0, 1, 1, 1));
        assert_eq!((attribution[3], attribution[4], attribution[9], attribution[10]), (1, 1, 1, 1));
        assert_eq!(stored, [(0, vec![1, 5]), (1, vec![4, 5]), (2, vec![0, 5])], "each eye's original map and the common one");
        let (valid, ready, _, gpu, ..) = run_rates(&[1, 5], vec![(vec![1, 5], 2, 1), (vec![4, 5], 2, 1), (vec![1, 5], 2, 1)], true);
        assert!(!valid && !ready && gpu[3] == 1, "a refined pixel coarser than the common rate");
        let (valid, ready, _, _, attribution, _) = run_rates(&[0, 5], vec![(vec![1, 5], 2, 1), (vec![4, 5], 2, 1), (vec![0, 5], 2, 1)], true);
        assert!(valid && !ready && attribution[3] == 0, "the left eye's maps not uniform");
        let (valid, ready, _, _, attribution, _) = run_rates(&[0, 5], vec![(vec![1, 5], 4, 1), (vec![4, 5], 2, 1), (vec![1, 5], 2, 1)], true);
        assert!(valid && !ready && attribution[9] == 0, "a map of another extent");
        let (valid, ready, _, _, attribution, stored) = run_rates(&[0, 5], vec![(vec![1, 5], 2, 1), (vec![4, 5], 2, 1), (vec![1, 5], 2, 1)], false);
        assert!(valid && !ready && attribution == [0; 12] && stored.is_empty(), "without native exits no attribution");
        let (valid, ..) = run_rates(&[0, 5], vec![(vec![1, 5], 2, 1), (vec![2, 5], 2, 1)], false);
        assert!(!valid, "an invalid rate value");
        let (valid, ..) = run_rates(&[0, 5], vec![(vec![1, 5], 2, 1)], false);
        assert!(!valid, "a source map that does not read");
    }
    fn warp() -> ID3D12Device {
        use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_11_0;
        use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory2, IDXGIAdapter, IDXGIFactory4, DXGI_CREATE_FACTORY_FLAGS};
        let factory: IDXGIFactory4 = unsafe { CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)) }.unwrap();
        let adapter: IDXGIAdapter = unsafe { factory.EnumWarpAdapter() }.unwrap();
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice(&adapter, D3D_FEATURE_LEVEL_11_0, &mut device) }.unwrap();
        device.unwrap()
    }
    fn texture(device: &ID3D12Device, format: DXGI_FORMAT, slices: u16, depth: bool) -> ID3D12Resource {
        let desc = D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
            Width: 8,
            Height: 4,
            DepthOrArraySize: slices,
            MipLevels: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Flags: if depth { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET },
            ..Default::default()
        };
        let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
        let state = if depth { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET };
        let mut out: Option<ID3D12Resource> = None;
        unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, state, None, &mut out) }.unwrap();
        out.unwrap()
    }
    thread_local! {
        static KEPT: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        static TRACE: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static REFUSE: RefCell<Option<String>> = const { RefCell::new(None) };
    }
    unsafe extern "C-unwind" fn keep(_: *mut c_void, _: *mut c_void, object: *mut c_void) -> bool {
        KEPT.with(|k| k.borrow_mut().push(object as usize));
        true
    }
    unsafe extern "C-unwind" fn internal(_: *mut c_void) {}

    fn record(mask: u32) -> crate::geometry_commands::DrawRecord {
        let mut d: crate::geometry_commands::DrawRecord = unsafe { core::mem::zeroed() };
        d.input_slot_mask = mask;
        d.index = D3D12_INDEX_BUFFER_VIEW { BufferLocation: 0x1100, SizeInBytes: 96, Format: DXGI_FORMAT_R16_UINT };
        d.vertices[0] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x2040, SizeInBytes: 256, StrideInBytes: 16 };
        d.vertices[7] = D3D12_VERTEX_BUFFER_VIEW { BufferLocation: 0x3000, SizeInBytes: 1024, StrideInBytes: 64 };
        d.first_instance = 2;
        d.instances = 3;
        d
    }
    fn lease(resource: usize, gpu: u64, bytes: u64) -> crate::geometry_commands::Lease {
        crate::geometry_commands::Lease { resource: resource as *mut c_void, gpu, bytes }
    }
    #[test]
    fn mesh_and_input_ranges() {
        let (left, right) = (record(1 | 128), record(1 | 128));
        let mut sources = [MeshSource { draw: &left, leases: [lease(0, 0, 0); 17] }, MeshSource { draw: &right, leases: [lease(0, 0, 0); 17] }];
        for (i, s) in sources.iter_mut().enumerate() {
            let base = (i as usize + 1) * 0x10;
            s.leases[0] = lease(base + 1, 0x1000, 0x400);
            s.leases[1] = lease(base + 2, 0x2000, 0x400);
            s.leases[8] = lease(base + 3, 0x3000, 0x400);
        }
        let pairs = [MeshPair { command: 4, sources: [0, 1] }, MeshPair { command: u32::MAX, sources: [0, 1] }];
        let ranges = unsafe { mesh_ranges(&pairs, &sources) }.unwrap();
        let rows: Vec<(u32, [usize; 2], [u64; 2], u64)> = ranges.iter().map(|r| (r.pair, [r.resources[0] as usize, r.resources[1] as usize], r.offsets, r.bytes)).collect();
        assert_eq!(rows, [(4, [0x11, 0x21], [0x100, 0x100], 96), (4, [0x12, 0x22], [0x40, 0x40], 256), (4, [0x13, 0x23], [128, 128], 192)],
            "the index, the first vertex slot and the instance window (two instances in, three of 64 bytes); the unpaired pair skipped");
        sources[1].leases[1] = lease(0x22, 0x2000, 0x80);
        assert!(unsafe { mesh_ranges(&pairs, &sources) }.is_none(), "a range beyond its held buffer");
        sources[1].leases[1] = lease(0x22, 0x2000, 0x400);
        let mut shorter = right;
        shorter.vertices[0].SizeInBytes = 128;
        sources[1].draw = &shorter;
        assert!(unsafe { mesh_ranges(&pairs, &sources) }.is_none(), "the second eye's bytes differ");
        let (a, b) = ([1u8, 2, 3], [9u8; 8]);
        let pieces = [
            InputPiece { resource: 0x5 as *mut c_void, offset: 16, data: a.as_ptr(), bytes: 3, align: true },
            InputPiece { resource: 0x6 as *mut c_void, offset: 32, data: b.as_ptr(), bytes: 8, align: true },
        ];
        let (blob, ranges) = expected_inputs(&pieces, false);
        assert_eq!(blob, [1, 2, 3, 0, 9, 9, 9, 9, 9, 9, 9, 9], "each piece 4-aligned after the last");
        assert_eq!((ranges[1].offsets, ranges[1].bytes, ranges[1].resources[0] as usize), ([32, 4], 8, 6));
        let (faulted, _) = expected_inputs(&pieces, true);
        assert_eq!(faulted[0], 0, "a fault flips the first expected byte");
        let device = warp();
        let upload = expected_upload(&device, &blob).unwrap();
        let mut mapped = core::ptr::null_mut();
        unsafe { upload.Map(0, None, Some(&mut mapped)) }.unwrap();
        assert_eq!(unsafe { core::slice::from_raw_parts(mapped as *const u8, blob.len()) }, &blob[..], "the upload holds the expected bytes");
        unsafe { upload.Unmap(0, None) };
    }
    #[test]
    fn native_exit_validation() {
        let device = warp();
        let target = |format: DXGI_FORMAT, depth: bool| {
            let desc = D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
                Width: 8,
                Height: 4,
                DepthOrArraySize: 1,
                MipLevels: 1,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Flags: if depth { D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET },
                ..Default::default()
            };
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let mut out: Option<ID3D12Resource> = None;
            unsafe { device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc, if depth { D3D12_RESOURCE_STATE_DEPTH_WRITE } else { D3D12_RESOURCE_STATE_RENDER_TARGET }, None, &mut out) }.unwrap();
            out.unwrap()
        };
        let colors: Vec<ID3D12Resource> = (0..3).map(|_| target(DXGI_FORMAT_R8G8B8A8_UNORM, false)).collect();
        let depth = target(DXGI_FORMAT_D32_FLOAT_S8X24_UINT, true);
        let baselines = [colors[0].as_raw(), colors[1].as_raw(), colors[2].as_raw(), core::ptr::null_mut(), depth.as_raw()];
        // The five planes, each at the baseline plane's footprint, one after the other.
        let mut planes = Vec::new();
        let mut at = 0u64;
        for (slot, plane) in [(0u32, 0u32), (1, 0), (2, 0), (4, 0), (4, 1)] {
            let desc = unsafe { ID3D12Resource::from_raw_borrowed(&baselines[slot as usize]).unwrap().GetDesc() };
            let (mut layout, mut rows, mut row_bytes, mut total) = (D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(), 0u32, 0u64, 0u64);
            unsafe { device.GetCopyableFootprints(&desc, plane, 1, at, Some(&mut layout), Some(&mut rows), Some(&mut row_bytes), Some(&mut total)) };
            let stride = (row_bytes / 8) as u32;
            let stencil = slot == 4 && plane == 1;
            planes.push(NativeExitPlane { slot, plane, rows, stride, component: if stencil && stride == 8 { 4 } else { 0 }, bytes: if stencil { 1 } else { 4 }, layout });
            at = (at + total + 511) & !511;
        }
        let readback = readback_buffer(&device, at).unwrap();
        let valid = |planes: &[NativeExitPlane], readback: &ID3D12Resource| unsafe { validate_exit(&device, readback.as_raw(), planes, &baselines, 8, 4) };
        assert!(valid(&planes, &readback), "the five planes at their footprints");
        let mut overlapping = planes.clone();
        overlapping[1].layout.Offset = overlapping[0].layout.Offset;
        assert!(!valid(&overlapping, &readback), "two planes overlapping");
        assert!(!valid(&planes[..4], &readback), "four planes");
        let mut pitch = planes.clone();
        pitch[2].layout.Footprint.RowPitch *= 2;
        assert!(!valid(&pitch, &readback), "a pitch off the baseline's footprint");
        let small = readback_buffer(&device, 512).unwrap();
        assert!(!valid(&planes, &small), "planes beyond the readback");
        let upload = expected_upload(&device, &[0u8; 16]).unwrap();
        assert!(!valid(&planes, &upload), "not a READBACK buffer");
    }
    #[test]
    fn image_copies_on_warp() {
        let device = warp();
        let colors = [[1.0f32, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0], [1.0, 1.0, 0.0, 1.0]];
        // The reference, paired and native-rate arrays (color slot 0 and depth), the baselines per eye.
        let chains: Vec<[ID3D12Resource; 2]> = (0..3)
            .map(|_| [texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, 2, false), texture(&device, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, 2, true)])
            .collect();
        let baselines: Vec<[ID3D12Resource; 5]> = (0..2)
            .map(|_| {
                [
                    texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false),
                    texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false),
                    texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false),
                    texture(&device, DXGI_FORMAT_R8G8B8A8_UNORM, 1, false),
                    texture(&device, DXGI_FORMAT_D32_FLOAT_S8X24_UINT, 1, true),
                ]
            })
            .collect();
        let allocator: ID3D12CommandAllocator = unsafe { device.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT) }.unwrap();
        let list: ID3D12GraphicsCommandList = unsafe { device.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None) }.unwrap();
        // Each color target cleared to its version's color.
        let heap: ID3D12DescriptorHeap = unsafe {
            device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC { Type: D3D12_DESCRIPTOR_HEAP_TYPE_RTV, NumDescriptors: 5, ..Default::default() })
        }
        .unwrap();
        let stride = unsafe { device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV) } as usize;
        let start = unsafe { heap.GetCPUDescriptorHandleForHeapStart() };
        let clears: [(&ID3D12Resource, [f32; 4]); 5] =
            [(&chains[0][0], colors[0]), (&chains[1][0], colors[1]), (&chains[2][0], colors[3]), (&baselines[0][0], colors[2]), (&baselines[1][0], colors[2])];
        for (i, (resource, color)) in clears.iter().enumerate() {
            let handle = D3D12_CPU_DESCRIPTOR_HANDLE { ptr: start.ptr + i * stride };
            unsafe {
                device.CreateRenderTargetView(*resource, None, handle);
                list.ClearRenderTargetView(handle, color, None);
            }
        }
        let raw = |r: &ID3D12Resource| r.as_raw();
        let mut t = ImageTargets { chains: [[core::ptr::null_mut(); 5]; 3], baselines: [[core::ptr::null_mut(); 5]; 2], colors: 1, width: 8, height: 4, versions: 4 };
        for (c, chain) in chains.iter().enumerate() {
            t.chains[c][0] = raw(&chain[0]);
            t.chains[c][4] = raw(&chain[1]);
        }
        for (e, eye) in baselines.iter().enumerate() {
            for slot in 0..5 {
                t.baselines[e][slot] = raw(&eye[slot]);
            }
        }
        let mut room = [Plane::default(); 10];
        let mut out = ImageOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut(), bytes: 0 };
        let host = ImageHost { user: core::ptr::null_mut(), keep, internal };
        KEPT.with(|k| k.borrow_mut().clear());
        assert!(unsafe { image_copies(&device, &list, &t, &mut out, &host) });
        let readback = unsafe { ID3D12Resource::from_raw(out.readback) };
        assert_eq!(out.count, 6, "two eyes of a color, the depth and the stencil");
        assert_eq!(KEPT.with(|k| k.borrow().len()), 9, "the readback and each eye's four baselines");
        let planes = &room[..out.count];
        for (i, row) in planes.iter().enumerate() {
            assert_eq!((row.eye, row.rows), ((i / 3) as u32, 4));
            for version in 0..4 {
                assert_eq!(row.layouts[version].Offset % 512, 0);
            }
        }
        assert_eq!((planes[0].slot, planes[0].plane, planes[0].stride, planes[0].bytes, planes[0].component), (0, 0, 4, 4, 0));
        assert_eq!((planes[1].slot, planes[1].plane, planes[1].bytes), (4, 0, 4));
        assert_eq!((planes[2].slot, planes[2].plane, planes[2].bytes), (4, 1, 1));
        let last = &planes[5].layouts[3];
        assert!(out.bytes >= last.Offset + 3 * last.Footprint.RowPitch as u64 + 8);
        unsafe { list.Close() }.unwrap();
        let queue: ID3D12CommandQueue = unsafe { device.CreateCommandQueue(&D3D12_COMMAND_QUEUE_DESC { Type: D3D12_COMMAND_LIST_TYPE_DIRECT, ..Default::default() }) }.unwrap();
        let fence: ID3D12Fence = unsafe { device.CreateFence(0, D3D12_FENCE_FLAG_NONE) }.unwrap();
        unsafe {
            queue.ExecuteCommandLists(&[Some(list.cast().unwrap())]);
            queue.Signal(&fence, 1).unwrap();
        }
        while unsafe { fence.GetCompletedValue() } < 1 {
            std::thread::yield_now();
        }
        let mut mapped = core::ptr::null_mut();
        unsafe { readback.Map(0, None, Some(&mut mapped)) }.unwrap();
        let bytes = unsafe { core::slice::from_raw_parts(mapped as *const u8, out.bytes as usize) };
        let pixel = |row: &Plane, version: usize, y: u64, x: u64| {
            let at = (row.layouts[version].Offset + y * row.layouts[version].Footprint.RowPitch as u64 + x * row.stride as u64) as usize;
            [bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]
        };
        for eye in 0..2 {
            let row = &planes[eye * 3];
            assert_eq!(pixel(row, 0, 3, 7), [255, 0, 0, 255], "the reference array's eye slice");
            assert_eq!(pixel(row, 1, 0, 0), [0, 255, 0, 255], "the paired array's eye slice");
            assert_eq!(pixel(row, 2, 1, 3), [0, 0, 255, 255], "the eye's baseline");
            assert_eq!(pixel(row, 3, 2, 5), [255, 255, 0, 255], "the native-rate array's eye slice");
        }
        unsafe { readback.Unmap(0, None) };
        let mut wrong = ImageOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut(), bytes: 0 };
        assert!(image_planes(&device, &ImageTargets { width: 3, ..t }, &mut wrong).is_none(), "row bytes not whole pixels");
        assert!(image_planes(&device, &ImageTargets { height: 8, ..t }, &mut wrong).is_none(), "rows other than the pass's height");
        let mut small = ImageOut { planes: room.as_mut_ptr(), capacity: 5, count: 0, readback: core::ptr::null_mut(), bytes: 0 };
        assert!(image_planes(&device, &t, &mut small).is_none(), "a full room");
        let mut three = ImageOut { planes: room.as_mut_ptr(), capacity: room.len(), count: 0, readback: core::ptr::null_mut(), bytes: 0 };
        let size3 = image_planes(&device, &ImageTargets { versions: 3, ..t }, &mut three).unwrap();
        assert!(size3 < out.bytes, "without native exits no native-rate copies");
    }

    struct Trace;
    fn log(value: String) {
        TRACE.with(|t| t.borrow_mut().push(value));
    }
    impl Commands for Trace {
        fn begin(&mut self, query: u32) {
            log(format!("begin {query}"));
        }
        fn end(&mut self, query: u32) {
            log(format!("end {query}"));
        }
        fn predicate(&mut self, resource: *mut c_void, offset: u64) {
            log(format!("predicate {:x} {offset}", resource as usize));
        }
        fn state(&mut self, source: &SourceState, image: *mut c_void) {
            log(format!("state {} {:x}", source.stencil, image as usize));
        }
        fn rate_image(&mut self, image: *mut c_void) {
            log(format!("rate {:x}", image as usize));
        }
        fn pipeline(&mut self, pipeline: *mut c_void) {
            log(format!("pipeline {:x}", pipeline as usize));
        }
        fn mask(&mut self, mask: u32) {
            log(format!("mask {mask:x}"));
        }
        fn resolve(&mut self, query: u32) {
            log(format!("resolve {query}"));
        }
        fn copy(&mut self, destination: *mut c_void, at: u64, source: *mut c_void, from: u64, bytes: u64) {
            log(format!("copy {:x}@{at} {:x}@{from} {bytes}", destination as usize, source as usize));
        }
    }
    fn refused(what: String) -> bool {
        let refuse = REFUSE.with(|r| r.borrow().clone());
        log(what.clone());
        refuse.is_some_and(|r| r == what)
    }
    unsafe extern "C-unwind" fn targets(_: *mut c_void, _: *mut c_void, chain: u32, mask: u32) -> bool {
        !refused(format!("targets {chain} {mask}"))
    }
    unsafe extern "C-unwind" fn source(_: *mut c_void, _: *mut c_void, index: u32) -> bool {
        !refused(format!("source {index}"))
    }
    unsafe extern "C-unwind" fn paired(_: *mut c_void, _: *mut c_void, command: u32, geometry: u32) -> bool {
        !refused(format!("paired {command} {geometry}"))
    }
    fn state(stencil: u32, pipeline: usize, rate_image: usize) -> SourceState {
        SourceState {
            pipeline: pipeline as *mut c_void,
            rate_image: rate_image as *mut c_void,
            topology: 4,
            stencil,
            blend: [1.0; 4],
            rate: 0,
            combiners: [0; 2],
            viewport: D3D12_VIEWPORT::default(),
            scissor: RECT::default(),
        }
    }
    fn run(native: bool, refuse: Option<&str>) -> (u32, Vec<String>, [u64; 16], [u64; 16], [u64; 16], [u64; 12], [u64; 16]) {
        // Sources: 0 and 2 the left eye, 1 and 3 the right; the pair (0, 1) paired as command 5, 2 and 3 alone.
        let sources = [state(10, 0xa0, 0xe0), state(11, 0xa1, 0xe1), state(12, 0xa2, 0xe2), state(13, 0xa3, 0xe3)];
        let (left, right) = ([0u32, 2], [1u32, 3]);
        let steps = [Step { mask: 3, source: [0, 0] }, Step { mask: 1, source: [1, 0] }, Step { mask: 2, source: [0, 1] }];
        let pairs = [PairRow { command: 5, sources: [0, 1], _pad: 0, pipeline: 0xb0 as *mut c_void }];
        let pair_for_source = [0u32, 0, 9, 9];
        let d = Draws {
            queries: [0x1 as *mut c_void, 0x2 as *mut c_void],
            query_readback: 0x3 as *mut c_void,
            predicate: 0x4 as *mut c_void,
            permission: 0x5 as *mut c_void,
            mesh: 0x6 as *mut c_void,
            inputs: 0x7 as *mut c_void,
            membership: [0x8 as *mut c_void, 0x9 as *mut c_void, 0xc as *mut c_void],
            rate_image: 0xd as *mut c_void,
            sources: sources.as_ptr(),
            source_count: sources.len(),
            eyes: [left.as_ptr(), right.as_ptr()],
            eye_counts: [2, 2],
            steps: steps.as_ptr(),
            step_count: steps.len(),
            pair_for_source: pair_for_source.as_ptr(),
            pairs: pairs.as_ptr(),
            pair_count: 1,
            mesh_pairs: 1,
            membership_sources: 2,
            native_rate: native,
            has_membership: true,
        };
        let (mut depth, mut pair, mut batch, mut mesh, mut rate) = ([0u64; 16], [0u64; 16], [0u64; 16], [0u64; 12], [0u64; 16]);
        let counters = Counters { depth_state: &mut depth, pair_stats: &mut pair, batch_stats: &mut batch, mesh_stats: &mut mesh, native_rate: &mut rate };
        let host = DrawHost { user: core::ptr::null_mut(), targets, source, paired };
        TRACE.with(|t| t.borrow_mut().clear());
        REFUSE.with(|r| *r.borrow_mut() = refuse.map(str::to_owned));
        let reason = unsafe { record_draws(&mut Trace, 0x10 as *mut c_void, &d, counters, &host) };
        let trace = TRACE.with(|t| t.borrow().clone());
        (reason, trace, depth, pair, batch, mesh, rate)
    }
    #[test]
    fn draws_trace() {
        let (reason, trace, depth, pair, batch, mesh, rate) = run(true, None);
        assert_eq!(reason, 0);
        let expected = [
            "predicate 5 0",
            // The left eye's reference draws in query 0, the right's in query 1.
            "mask ffffffff", "targets 0 1", "begin 0", "state 10 d", "pipeline a0", "source 0", "state 12 d", "pipeline a2", "source 2", "end 0",
            "mask ffffffff", "targets 0 2", "begin 1", "state 11 d", "pipeline a1", "source 1", "state 13 d", "pipeline a3", "source 3", "end 1",
            // The paired plan in query 2: the paired step under the mesh's equal predicate, its fallbacks, then each eye alone.
            "begin 2", "targets 1 3", "state 10 d", "pipeline b0", "mask 3", "predicate 6 80", "paired 5 0", "predicate 6 88",
            "targets 1 1", "state 10 d", "pipeline a0", "mask ffffffff", "source 0",
            "targets 1 2", "state 11 d", "pipeline a1", "mask ffffffff", "source 1",
            "targets 1 1", "state 12 d", "predicate 5 0", "pipeline a2", "mask ffffffff", "source 2",
            "targets 1 2", "state 13 d", "predicate 5 0", "pipeline a3", "mask ffffffff", "source 3", "end 2",
            // The native-rate draws with each source's own rate image.
            "predicate 5 0", "mask ffffffff",
            "targets 2 1", "state 10 d", "rate e0", "pipeline a0", "source 0", "state 12 d", "rate e2", "pipeline a2", "source 2",
            "targets 2 2", "state 11 d", "rate e1", "pipeline a1", "source 1", "state 13 d", "rate e3", "pipeline a3", "source 3",
            "predicate 0 0", "resolve 0", "resolve 1", "resolve 2",
            "copy 3@288 4@0 8", "copy 3@296 6@0 16", "copy 3@4392 7@0 16", "copy c@0 8@0 32", "copy c@32 9@0 64",
        ];
        assert_eq!(trace, expected);
        assert_eq!((depth[15], pair[4], batch[7], batch[8], mesh[10], mesh[11], rate[2]), (3, 4, 4, 5, 1, 2, 4));
        let (reason, trace, ..) = run(false, None);
        assert_eq!(reason, 0);
        assert!(!trace.iter().any(|row| row.starts_with("targets 2") || row.starts_with("rate ")), "no native-rate draws without native exits");
        let (reason, trace, depth, ..) = run(true, Some("source 2"));
        assert_eq!(reason, 7, "a reference draw refused");
        assert_eq!((trace.last().unwrap().as_str(), depth[15]), ("end 0", 1), "the query still ended");
        assert_eq!(run(true, Some("targets 0 2")).0, 7, "the right eye's reference targets");
        assert_eq!(run(true, Some("paired 5 0")).0, 8, "the paired command");
        assert_eq!(run(true, Some("targets 1 2")).0, 8, "a paired step's fallback targets");
        assert_eq!(run(true, Some("targets 2 2")).0, 7, "the native-rate targets");
    }

    #[derive(Default)]
    struct Memory {
        buffers: HashMap<usize, Vec<u8>>,
        mapped: Vec<usize>,
        refuse: Option<usize>,
    }
    impl Readbacks for Memory {
        fn desc(&mut self, readback: *mut c_void) -> Option<D3D12_RESOURCE_DESC> {
            let bytes = self.buffers.get(&(readback as usize))?.len() as u64;
            Some(D3D12_RESOURCE_DESC {
                Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
                Width: bytes,
                Height: 1,
                DepthOrArraySize: 1,
                MipLevels: 1,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
                ..Default::default()
            })
        }
        fn map(&mut self, readback: *mut c_void, bytes: u64) -> Option<*const u8> {
            if self.refuse == Some(readback as usize) {
                return None;
            }
            let buffer = self.buffers.get(&(readback as usize))?;
            assert!(bytes as usize <= buffer.len());
            self.mapped.push(readback as usize);
            Some(buffer.as_ptr())
        }
        fn unmap(&mut self, readback: *mut c_void) {
            let at = self.mapped.iter().position(|&m| m == readback as usize).expect("unmapped once mapped");
            self.mapped.remove(at);
        }
    }
    const W: u32 = 4;
    const H: u32 = 2;
    fn footprint(offset: u64, format: DXGI_FORMAT) -> D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
        D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
            Offset: offset,
            Footprint: D3D12_SUBRESOURCE_FOOTPRINT { Format: format, Width: W, Height: H, Depth: 1, RowPitch: 256 },
        }
    }
    /// The five planes' kinds: slot, plane, stride, bytes.
    const KINDS: [(u32, u32, u32, u32); 5] = [(0, 0, 4, 4), (1, 0, 4, 4), (2, 0, 4, 4), (4, 0, 4, 4), (4, 1, 1, 1)];
    /// The pass's planes: per eye the five kinds, each version at its own 512-byte place.
    fn pass_planes() -> Vec<Plane> {
        let mut planes = Vec::new();
        for eye in 0..2u32 {
            for (k, &(slot, plane, stride, bytes)) in KINDS.iter().enumerate() {
                let mut row = Plane { eye, slot, plane, rows: H, stride, component: 0, bytes, ..Plane::default() };
                for version in 0..4 {
                    row.layouts[version] = footprint(((eye as u64 * 5 + k as u64) * 4 + version as u64) * 512, DXGI_FORMAT_R32_TYPELESS);
                }
                planes.push(row);
            }
        }
        planes
    }
    fn fill(image: &mut [u8], layout: &D3D12_PLACED_SUBRESOURCE_FOOTPRINT, stride: u32, bytes: u32, value: u8) {
        for y in 0..H as u64 {
            for x in 0..W as u64 {
                let at = (layout.Offset + y * layout.Footprint.RowPitch as u64 + x * stride as u64) as usize;
                image[at..at + bytes as usize].fill(value);
            }
        }
    }
    fn native_planes() -> Vec<NativeExitPlane> {
        KINDS
            .iter()
            .enumerate()
            .map(|(k, &(slot, plane, stride, bytes))| NativeExitPlane { slot, plane, rows: H, stride, component: 0, bytes, layout: footprint(k as u64 * 512, DXGI_FORMAT_R32_TYPELESS) })
            .collect()
    }

    #[test]
    fn output_comparison() {
        let planes = pass_planes();
        let mut image = vec![0u8; 40 * 512];
        for row in &planes {
            fill(&mut image, &row.layouts[1], row.stride, row.bytes, 0x40 + row.eye as u8);
        }
        // The output copy: per side (the other eye) the five planes, holding the paired image's pixels.
        let mut copy_planes = Vec::new();
        let mut copied = vec![0u8; 10 * 512];
        for side in 0..2u32 {
            for (k, &(slot, plane, stride, bytes)) in KINDS.iter().enumerate() {
                let row = OutputPlane { side, slot, plane, footprint: footprint((side as u64 * 5 + k as u64) * 512, DXGI_FORMAT_R32_TYPELESS), rows: H, stride, component: 0, bytes };
                fill(&mut copied, &row.footprint, stride, bytes, 0x40 + (1 - side) as u8);
                copy_planes.push(row);
            }
        }
        let mut memory = Memory::default();
        memory.buffers.insert(0x100, copied.clone());
        let mut c = OutputCompare {
            readback: 0x100 as *mut c_void,
            bytes: copied.len() as u64,
            copy_planes: copy_planes.as_ptr(),
            copy_count: copy_planes.len(),
            exits: [ExitView { readback: core::ptr::null_mut(), planes: core::ptr::null(), count: 0 }; 2],
            planes: planes.as_ptr(),
            plane_count: planes.len(),
            paired: image.as_ptr(),
            paired_bytes: image.len(),
            width: W,
            height: H,
            recorded: true,
        };
        let mut stats = [0u64; 24];
        assert!(unsafe { compare_output(&mut memory, &c, false, 0, &mut stats) }, "nothing submitted");
        assert_eq!(stats, [0; 24]);
        assert!(unsafe { compare_output(&mut memory, &c, true, 0, &mut stats) }, "the output equal to the paired image");
        assert_eq!((stats[6], stats[7], stats[8], stats[9], stats[10]), (0, 0, 1, 0, 0));
        assert!(memory.mapped.is_empty());
        // One differing stencil pixel on side 0 (the right eye).
        let mut differing = copied.clone();
        differing[4 * 512 + 256 + 2] ^= 1;
        memory.buffers.insert(0x100, differing);
        let mut stats = [0u64; 24];
        assert!(!unsafe { compare_output(&mut memory, &c, true, 0, &mut stats) });
        assert_eq!((stats[6], stats[7], stats[8]), (0, 1, 1), "the right eye's difference");
        // Against the native exits (the inputs refused): each eye's exit holds the copy's pixels.
        memory.buffers.insert(0x100, copied.clone());
        let natives = native_planes();
        for eye in 0..2usize {
            let mut exit = vec![0u8; 5 * 512];
            for row in &natives {
                fill(&mut exit, &row.layout, row.stride, row.bytes, 0x40 + eye as u8);
            }
            memory.buffers.insert(0x200 + eye, exit);
            c.exits[eye] = ExitView { readback: (0x200 + eye) as *mut c_void, planes: natives.as_ptr(), count: natives.len() };
        }
        let mut stats = [0u64; 24];
        assert!(unsafe { compare_output(&mut memory, &c, true, 1, &mut stats) });
        assert_eq!((stats[8], stats[9], stats[10]), (1, 1, 1), "exact against the native exits");
        assert!(memory.mapped.is_empty(), "every readback unmapped");
        memory.refuse = Some(0x200);
        let mut stats = [0u64; 24];
        assert!(!unsafe { compare_output(&mut memory, &c, true, 1, &mut stats) }, "an exit that does not map");
        assert_eq!(stats[8], 0);
        memory.refuse = None;
        c.copy_count = 9;
        assert!(!unsafe { compare_output(&mut memory, &c, true, 0, &mut [0; 24]) }, "nine planes");
        c.copy_count = 10;
        c.recorded = false;
        assert!(!unsafe { compare_output(&mut memory, &c, true, 0, &mut [0; 24]) }, "not recorded");
    }

    #[test]
    fn exit_comparison() {
        let planes = pass_planes();
        let natives = native_planes();
        let mut image = vec![0u8; 40 * 512];
        let mut memory = Memory::default();
        for eye in 0..2usize {
            let mut exit = vec![0u8; 5 * 512];
            for row in &natives {
                fill(&mut exit, &row.layout, row.stride, row.bytes, 0x20 + eye as u8);
            }
            memory.buffers.insert(0x300 + eye, exit);
        }
        // The reference, paired and native-rate images equal to each eye's exit.
        for row in &planes {
            for version in [0usize, 1, 3] {
                fill(&mut image, &row.layouts[version], row.stride, row.bytes, 0x20 + row.eye as u8);
            }
        }
        let rates = vec![0u8; 4];
        let mut c = ExitCompare {
            exits: [
                ExitView { readback: 0x300 as *mut c_void, planes: natives.as_ptr(), count: 5 },
                ExitView { readback: 0x301 as *mut c_void, planes: natives.as_ptr(), count: 5 },
            ],
            planes: planes.as_ptr(),
            plane_count: planes.len(),
            image: image.as_ptr(),
            image_bytes: image.len(),
            common: rates.as_ptr(),
            common_bytes: rates.len(),
            original: [rates.as_ptr(), rates.as_ptr()],
            original_bytes: [4, 4],
            baseline: 0,
            pair_different: [0, 0],
            width: W,
            height: H,
            tile: 2,
            map_width: 2,
            map_height: 1,
            maps_ready: false,
            allow_refinement: false,
        };
        let compare = |memory: &mut Memory, c: &ExitCompare| {
            let (mut exit, mut samples, mut rate, mut attribution, mut accepted) = ([0u64; 24], [0u64; 160], [0u64; 16], [0u64; 12], [0u64; 8]);
            exit[0] = 1;
            exit[1] = 2;
            let mut s = ExitStats { exit: &mut exit, samples: &mut samples, rate: &mut rate, attribution: &mut attribution, acceptance: &mut accepted };
            let error = unsafe { compare_exit(memory, c, &mut s) };
            (error, exit, samples, rate, attribution, accepted)
        };
        let (error, exit, samples, rate, _, accepted) = compare(&mut memory, &c);
        assert!(!error, "an exact exit accepted");
        assert_eq!((exit[2], exit[20], exit[21], exit[22]), (10, 1, 1, 0), "ten planes compared, complete and strict");
        assert_eq!((rate[3], rate[13], rate[15]), (10, 1, 0), "the native-rate planes compared");
        assert_eq!(accepted[7], 1);
        assert_eq!((samples[0], samples[16 * 5]), (0, 1), "each eye's samples");
        assert!(memory.mapped.is_empty());
        // A differing reference depth pixel for the left eye.
        let depth = &planes[3];
        image[(depth.layouts[0].Offset + 256 + 4) as usize] ^= 1;
        c.image = image.as_ptr();
        let (error, exit, samples, ..) = compare(&mut memory, &c);
        assert!(error, "a difference without approved refinement");
        assert_eq!((exit[8], exit[9], exit[20], exit[21]), (1, 0, 1, 0), "the left eye's depth difference, not strict");
        assert_eq!((samples[3 * 16 + 3], samples[3 * 16 + 5], samples[3 * 16 + 6]), (1, 1, 1), "the first difference at (1, 1)");
        image[(depth.layouts[0].Offset + 256 + 4) as usize] ^= 1;
        // The colors' differences attributed to the refined tiles.
        let color = &planes[5];
        image[(color.layouts[0].Offset + 256) as usize] ^= 1;
        c.image = image.as_ptr();
        let refined = vec![1u8, 0, 0, 0];
        let original = vec![5u8, 0, 0, 0];
        c.maps_ready = true;
        c.common = refined.as_ptr();
        c.common_bytes = 2;
        c.original = [original.as_ptr(), original.as_ptr()];
        c.original_bytes = [2, 2];
        let (_, _, _, _, attribution, _) = compare(&mut memory, &c);
        assert_eq!((attribution[5], attribution[6], attribution[7], attribution[8]), (0, 1, 0, 0), "the right eye's color difference inside a refined tile");
        image[(color.layouts[0].Offset + 256) as usize] ^= 1;
        c.image = image.as_ptr();
        c.maps_ready = false;
        memory.refuse = Some(0x301);
        let (error, exit, ..) = compare(&mut memory, &c);
        assert!(error && exit[20] == 0, "an exit that does not map leaves the comparison incomplete");
        memory.refuse = None;
        c.exits[1].count = 4;
        let (error, exit, ..) = compare(&mut memory, &c);
        assert!(error && exit[2] == 9, "a missing plane");
        c.exits[1].count = 5;
        c.baseline = 1;
        let (error, exit, _, rate, ..) = compare(&mut memory, &c);
        assert!(!error && exit[22] == 1 && rate[15] == 1, "refused inputs: complete exits without acceptance");
    }
}
