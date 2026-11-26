//! The single-pass stereo's GPU checks, formerly their HLSL: the depth commit gate (rust/crates/cvr-gpu/src/
//! depth_commit_gate.rs: the two eyes' depth and stencil compared, and two buffers' bytes), the geometry comparison
//! (geometry_compare.rs: the left and right eye's byte ranges, pair by pair) and the multilayer membership check
//! (rust/crates/cvr-render/src/multilayer_membership.rs: each source's selector records against the membership
//! bitmap). A difference clears a decision word with an atomic, as the HLSL's Interlocked* did.
use spirv_std::arch::{atomic_and, atomic_exchange, atomic_or, workgroup_memory_barrier_with_group_sync, IndexUnchecked};
use spirv_std::glam::{IVec3, UVec3, UVec4, Vec4};
use spirv_std::memory::{Scope, Semantics};
use spirv_std::{spirv, Image};

/// The atomics' scope: the queue family (wider than a workgroup, every invocation of the dispatch). Device scope under
/// rust-gpu's Vulkan memory model needs a capability naga does not read; naga writes HLSL's Interlocked* either way,
/// whose scope is the device's.
const DEVICE: u32 = Scope::QueueFamily as u32;
const RELAXED: u32 = Semantics::NONE.bits();

/// A word of a buffer, unchecked, as the HLSL's Load was: the passes bind their buffers by root descriptors, which have
/// no size -- rust-gpu's bounds check reads the length through naga's GetDimensions, gets 0 there, and skipped every
/// access (the gates never cleared, the comparisons never differed; found by the single-pass tests).
fn word(b: &[u32], i: u32) -> u32 {
    unsafe { *b.index_unchecked(i as usize) }
}
fn word_mut(b: &mut [u32], i: u32) -> &mut u32 {
    unsafe { b.index_unchecked_mut(i as usize) }
}

// ---------------------------------------------------------------- depth_commit_gate.rs
type DepthArray = Image!(2D, type = f32, sampled, arrayed);
type StencilArray = Image!(2D, type = u32, sampled, arrayed);

/// DEPTH_SHADER: slice 0 against slice 1 by the pixel (depth bits and stencil); a difference clears Decision[0].
/// Bindings: 1 Depth (t0), 2 Stencil (t1), 3 Decision (u0).
#[spirv(compute(threads(16, 16, 1)))]
pub fn depth_gate(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(descriptor_set = 0, binding = 1)] depth: &DepthArray,
    #[spirv(descriptor_set = 0, binding = 2)] stencil: &StencilArray,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] decision: &mut [u32],
) {
    let size: UVec3 = depth.query_size_lod(0);
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let a = IVec3::new(id.x as i32, id.y as i32, 0);
    let b = IVec3::new(id.x as i32, id.y as i32, 1);
    let da: Vec4 = depth.fetch(a);
    let db: Vec4 = depth.fetch(b);
    let differs = if da.x.to_bits() != db.x.to_bits() {
        true
    } else {
        let sa: UVec4 = stencil.fetch(a);
        let sb: UVec4 = stencil.fetch(b);
        sa.y != sb.y
    };
    if differs {
        unsafe { atomic_exchange::<u32, DEVICE, RELAXED>(word_mut(decision, (0) as u32), 0) };
    }
}

/// b0 of BUFFER_SHADER: uint Words; uint RowBytes; uint RowPitch; uint Rows.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Size {
    pub words: u32,
    pub row_bytes: u32,
    pub row_pitch: u32,
    pub rows: u32,
}

/// BUFFER_SHADER: two buffers' rows compared word by word (the row's last word masked); a difference clears
/// Decision[0]. Bindings: 0 Size (b0), 3 Decision (u0), 4 Main (t0), 5 Peer (t1).
#[spirv(compute(threads(64, 1, 1)))]
pub fn buffer_gate(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Size,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] decision: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 4)] main: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 5)] peer: &[u32],
) {
    if id.x >= k.words || id.y >= k.rows {
        return;
    }
    let at = id.y * k.row_pitch + id.x * 4;
    let remaining = k.row_bytes - id.x * 4;
    let mask = if remaining >= 4 { 0xffff_ffffu32 } else { (1u32 << (remaining * 8)) - 1 };
    let i = (at / 4) as usize;
    if ((word(main, (i) as u32) ^ word(peer, (i) as u32)) & mask) != 0 {
        unsafe { atomic_exchange::<u32, DEVICE, RELAXED>(word_mut(decision, (0) as u32), 0) };
    }
}

// ---------------------------------------------------------------- geometry_compare.rs
/// b0: uint batchFirst, batchCount, batchWidth, pairCount.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Batch {
    pub batch_first: u32,
    pub batch_count: u32,
    pub batch_width: u32,
    pub pair_count: u32,
}

/// Every pair's result word: 1 when the comparison is permitted (permission's first two words not both zero).
/// Bindings: 0 Batch (b0), 3 permission (t2), 5 results (u0).
#[spirv(compute(threads(256, 1, 1)))]
pub fn geometry_initialize(
    #[spirv(global_invocation_id)] id: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Batch,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] permission: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 5)] results: &mut [u32],
) {
    if id.x >= k.pair_count {
        return;
    }
    let allowed = word(permission, (0) as u32) != 0 || word(permission, (1) as u32) != 0;
    let at = (id.x * 4) as usize;
    *word_mut(results, (at) as u32) = if allowed { 1 } else { 0 };
    *word_mut(results, (at + 1) as u32) = 0;
    *word_mut(results, (at + 2) as u32) = 0;
    *word_mut(results, (at + 3) as u32) = 0;
}

/// `count` bytes of `bytes` from byte `address` (not aligned), the first in the low byte.
fn bytes_at(bytes: &[u32], address: u32, count: u32) -> u32 {
    let shift = (address & 3) * 8;
    let at = (address & !3u32) / 4;
    let mut value = word(bytes, at) >> shift;
    if (address & 3) + count > 4 {
        value |= word(bytes, at + 1) << (32 - shift);
    }
    value
}

/// The pairs of a batch, a group each, 4 bytes a thread: a difference clears the pair's equality (word 0) and sets
/// its difference (word 2). Bindings: 0 Batch (b0), 1 left (t0), 2 right (t1), 3 permission (t2), 4 ranges (t3),
/// 5 results (u0).
#[spirv(compute(threads(256, 1, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn geometry_compare(
    #[spirv(local_invocation_id)] thread: UVec3,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Batch,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] left: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 2)] right: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] permission: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 4)] ranges: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 5)] results: &mut [u32],
) {
    let index = group.x + k.batch_width * group.y;
    if index >= k.batch_count || (word(permission, (0) as u32) == 0 && word(permission, (1) as u32) == 0) {
        return;
    }
    let r = ((k.batch_first + index) * 4) as usize;
    let (rx, ry, rz, rw) = (word(ranges, (r) as u32), word(ranges, (r + 1) as u32), word(ranges, (r + 2) as u32), word(ranges, (r + 3) as u32));
    let at = thread.x * 4;
    if at >= rz {
        return;
    }
    let count = (rz - at).min(4);
    let mask = 0xffff_ffffu32 >> (32 - count * 8);
    if ((bytes_at(left, rx + at, count) ^ bytes_at(right, ry + at, count)) & mask) != 0 {
        let w = (rw * 4) as usize;
        unsafe {
            atomic_and::<u32, DEVICE, RELAXED>(word_mut(results, (w) as u32), 0);
            atomic_or::<u32, DEVICE, RELAXED>(word_mut(results, (w + 2) as u32), 1);
        }
    }
}

// ---------------------------------------------------------------- multilayer_membership.rs
/// Why a selector index is not a member: 8 out of the bitmap's range, 16 its bit clear, 0 a member.
fn selector_reason(membership: &[u32], membership_offset: u32, index: u32) -> u32 {
    if index >= 32768 {
        8
    } else if word(membership, (((membership_offset + (index >> 5) * 4) / 4) as usize) as u32) & (1u32 << (index & 31)) == 0 {
        16
    } else {
        0
    }
}

/// b0: uint firstMetadata.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Membership {
    pub first_metadata: u32,
}

/// Per source (a group of 32 lanes, one selector record a lane): the producer enabled, the record base read from b4,
/// each record's five selector indices checked against the membership bitmap; lane 0 gathers the lanes and writes the
/// predicates and the results. Bindings: 0 (b0), 1 b4 (t0), 2 t5 (t1), 3 membership (t2), 4 producer (t3), 5 metadata
/// (t4), 6 predicates (u0), 7 results (u1).
#[spirv(compute(threads(32, 1, 1)))]
#[allow(clippy::too_many_arguments)]
pub fn membership_check(
    #[spirv(local_invocation_index)] lane: u32,
    #[spirv(workgroup_id)] group: UVec3,
    #[spirv(uniform, descriptor_set = 0, binding = 0)] k: &Membership,
    #[spirv(storage_buffer, descriptor_set = 0, binding = 1)] b4: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 2)] t5: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 3)] membership: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 4)] producer: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 5)] metadata: &[u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 6)] predicates: &mut [u32],
    #[spirv(storage_buffer, descriptor_set = 0, binding = 7)] results: &mut [u32],
    #[spirv(workgroup)] shared: &mut [u32; 4],
    #[spirv(workgroup)] failures: &mut [u32; 32],
    #[spirv(workgroup)] missing: &mut [u32; 32],
    #[spirv(workgroup)] record_counts: &mut [u32; 32],
    #[spirv(workgroup)] selector_counts: &mut [u32; 32],
) {
    // shared: enabled, baseRecord, mask, invalidBase
    let m = ((k.first_metadata + group.x) * 8) as usize;
    let (b4_offset, t5_offset, first_element, num_elements) = (word(metadata, (m) as u32), word(metadata, (m + 1) as u32), word(metadata, (m + 2) as u32), word(metadata, (m + 3) as u32));
    let (membership_offset, source_index) = (word(metadata, (m + 4) as u32), word(metadata, (m + 5) as u32));
    let l = lane as usize;
    failures[l] = 0;
    missing[l] = 0xffff_ffff;
    record_counts[l] = 0;
    selector_counts[l] = 0;
    if lane == 0 {
        let enabled = word(producer, (0) as u32) != 0 || word(producer, (1) as u32) != 0;
        shared[0] = if enabled { 1 } else { 0 };
        shared[1] = 0;
        shared[2] = 0;
        shared[3] = 0;
        if enabled {
            let raw = f32::from_bits(word(b4, (((b4_offset + 128) / 4) as usize) as u32));
            shared[2] = word(b4, (((b4_offset + 224) / 4) as usize) as u32);
            // isfinite(raw), by the bits (a literal infinity is refused by naga)
            if raw.to_bits() & 0x7f80_0000 != 0x7f80_0000 && raw >= 0.0 && raw < 4294967296.0 {
                shared[1] = raw as u32;
            } else {
                shared[3] = 1;
            }
        }
    }
    workgroup_memory_barrier_with_group_sync();
    let (enabled, base_record, mask, invalid_base) = (shared[0], shared[1], shared[2], shared[3]);
    if enabled != 0 && invalid_base == 0 && (mask & (1u32 << lane)) != 0 {
        if base_record > 0xffff_ffff - lane {
            failures[l] = 2;
        } else {
            let record = base_record + lane;
            if record >= num_elements {
                failures[l] = 4;
            } else {
                let address = (t5_offset + (first_element + record) * 128) / 4;
                let a = word(t5, ((address + 25) as usize) as u32);
                let b = word(t5, ((address + 26) as usize) as u32);
                let c = word(t5, ((address + 27) as usize) as u32);
                record_counts[l] = 1;
                selector_counts[l] = 5;
                // {a, c >> 16, c & 65535, b & 65535, b >> 16}, in order
                let indices = [a, c >> 16, c & 65535, b & 65535, b >> 16];
                let mut j = 0;
                while j < 5 {
                    let index = indices[j];
                    let reason = selector_reason(membership, membership_offset, index);
                    failures[l] |= reason;
                    if reason != 0 && missing[l] == 0xffff_ffff {
                        missing[l] = index;
                    }
                    j += 1;
                }
            }
        }
    }
    workgroup_memory_barrier_with_group_sync();
    if lane == 0 {
        let (mut error, mut records, mut selectors, mut first_bit, mut first_index) = (invalid_base, 0u32, 0u32, 0xffff_ffffu32, 0xffff_ffffu32);
        let mut i = 0u32;
        while i < 32 {
            let f = failures[i as usize];
            error |= f;
            records += record_counts[i as usize];
            selectors += selector_counts[i as usize];
            if f != 0 && first_bit == 0xffff_ffff {
                first_bit = i;
                first_index = missing[i as usize];
            }
            i += 1;
        }
        let p = (source_index * 4) as usize;
        *word_mut(predicates, (p) as u32) = if enabled != 0 && error == 0 { 1 } else { 0 };
        *word_mut(predicates, (p + 1) as u32) = 0;
        *word_mut(predicates, (p + 2) as u32) = if enabled != 0 && error != 0 { 1 } else { 0 };
        *word_mut(predicates, (p + 3) as u32) = 0;
        let r = (source_index * 8) as usize;
        *word_mut(results, (r) as u32) = enabled;
        *word_mut(results, (r + 1) as u32) = error;
        *word_mut(results, (r + 2) as u32) = base_record;
        *word_mut(results, (r + 3) as u32) = mask;
        *word_mut(results, (r + 4) as u32) = records;
        *word_mut(results, (r + 5) as u32) = selectors;
        *word_mut(results, (r + 6) as u32) = first_bit;
        *word_mut(results, (r + 7) as u32) = first_index;
    }
}
