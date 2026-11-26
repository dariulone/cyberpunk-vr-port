//! THE EYES' GEOMETRY INSTANCE MEMBERSHIP (include/Render/NativeEyeMask.hpp): a geometry instance key -- the draw's
//! 64 bytes without its per-call words (+1..+8, +0x2D.., and +0x1C..+0x20 when the index override handle is invalid:
//! 1F2B68 then uses descriptor+78 and the unused word can hold stack bytes) followed by the instance's 48 or 64
//! bytes; the address identities a GPU instance range would carry; and the membership of both eyes' instances -- each
//! eye's counted keys (an eye whose instance bytes are unknown records its geometry as unknown), the plan of one main
//! draw's per-instance eye masks (3 both eyes, 2 main only) consuming the counts, and the peer's extra instances of a
//! geometry. Prospective matching only: materials, IA bindings and camera constants must still match at the actual
//! peer draw. The header forwards to these in the Rust build; tools/single_pass_tests/eye_membership_test runs its
//! cases against them.
use std::collections::{HashMap, HashSet};

/// packets::Inputs (the fields membership reads; 256 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Inputs {
    pub valid: u32,
    pub instances: u32,
    pub command_program_field: u32,
    pub source: u32,
    pub draw: [u8; 64],
    pub shaders: [u8; 8],
    pub state: [u8; 40],
    pub object: [u8; 24],
    pub transform: [u8; 48],
    pub skin: [u8; 16],
    pub instance_offset: u32,
    pub instance_bytes: u32,
    pub instance_stride: u32,
    pub program_offset: u32,
    pub program_bytes: u32,
    pub reserved: u32,
    pub instance_gpu_address: u64,
    pub instance_resource: u64,
}
const _: () = assert!(
    core::mem::size_of::<Inputs>() == 256 && core::mem::offset_of!(Inputs, instance_offset) == 216
);

/// GeometryInstanceKey.
pub fn geometry_instance_key(draw: &[u8; 64], instance: &[u8]) -> Option<Vec<u8>> {
    if draw[0] != 0 || (instance.len() != 48 && instance.len() != 64) {
        return None;
    }
    let mut d = *draw;
    d[1..8].fill(0);
    d[0x2d..].fill(0);
    let index_override = u32::from_le_bytes(d[0x10..0x14].try_into().unwrap());
    if index_override == 0 || index_override > 0x7fff_ffff {
        d[0x1c..0x20].fill(0);
    }
    let mut key = Vec::with_capacity(64 + instance.len());
    key.extend_from_slice(&d);
    key.extend_from_slice(instance);
    Some(key)
}

/// GpuInstanceIdentities' validity (the output has `count` 48-byte rows of {resource, address + i*48}).
pub fn gpu_identities_valid(resource: u64, address: u64, count: u32, stride: u32) -> bool {
    resource != 0
        && address != 0
        && stride == 48
        && count != 0
        && count <= 32767
        && address <= u64::MAX - count as u64 * stride as u64
}

#[derive(Default)]
pub struct EyeMembership {
    counts: HashMap<Vec<u8>, [u32; 2]>,
    initial: HashMap<Vec<u8>, [u32; 2]>,
    unknown: [HashSet<Vec<u8>>; 2],
    peer_geometry: HashMap<Vec<u8>, Vec<Vec<u8>>>,
    claimed: HashSet<Vec<u8>>,
    pub totals: [u32; 2],
    pub unresolved: [u32; 2],
}

impl EyeMembership {
    /// One eye's packet and its resolved inputs (`blob` the owned instance bytes).
    pub fn add(&mut self, side: u32, packet_second: u64, input: &Inputs, blob: &[u8]) -> bool {
        if side > 1 || input.valid & 1 == 0 {
            return false;
        }
        let side = side as usize;
        if input.draw[0] != 0 || packet_second & (1u64 << 51) == 0 {
            return true;
        }
        let stride = if packet_second & (1u64 << 50) != 0 {
            64
        } else {
            48
        };
        if input.instances == 0
            || input.instances > 32767
            || input.instances as u64 != (packet_second >> 18) & 32767
            || input.instance_stride != stride
        {
            return false;
        }
        if input.valid & 16 == 0 {
            let Some(key) = geometry_instance_key(&input.draw, &[0u8; 48]) else {
                return false;
            };
            self.unknown[side].insert(key[..64].to_vec());
            self.unresolved[side] += 1;
            return true;
        }
        if input.instances as u64 * input.instance_stride as u64 != input.instance_bytes as u64
            || input.instance_offset as usize > blob.len()
            || input.instance_bytes as usize > blob.len() - input.instance_offset as usize
            || self.totals[side] > 262144 - input.instances
        {
            return false;
        }
        for i in 0..input.instances as usize {
            let at = input.instance_offset as usize + i * input.instance_stride as usize;
            let Some(key) =
                geometry_instance_key(&input.draw, &blob[at..at + input.instance_stride as usize])
            else {
                return false;
            };
            self.counts.entry(key.clone()).or_default()[side] += 1;
            let original = self.initial.entry(key.clone()).or_default();
            if side == 1 && original[1] == 0 {
                self.peer_geometry
                    .entry(key[..64].to_vec())
                    .or_default()
                    .push(key);
            }
            original[side] += 1;
        }
        self.totals[side] += input.instances;
        true
    }

    /// One main draw's per-instance eye masks (3 both, 2 main only), consuming the matched counts; `reason` 1 bad
    /// input, 2 the peer's instances are unknown, 3 an instance is not (or not enough times) in the main eye's counts.
    pub fn plan(
        &mut self,
        draw: &[u8; 64],
        instances: &[u8],
        stride: u32,
        masks: &mut [u8],
        reason: &mut u32,
    ) -> bool {
        *reason = 1;
        let stride = stride as usize;
        if (stride != 48 && stride != 64)
            || instances.is_empty()
            || instances.len() % stride != 0
            || instances.len() > 16384
        {
            return false;
        }
        let count = instances.len() / stride;
        if masks.len() != count {
            return false;
        }
        // The instances grouped by key, each group in instance order.
        let mut groups: Vec<(Vec<u8>, Vec<usize>)> = Vec::new();
        let mut group_of: HashMap<Vec<u8>, usize> = HashMap::new();
        let mut peer_unknown = false;
        for (index, instance) in instances.chunks_exact(stride).enumerate() {
            let Some(key) = geometry_instance_key(draw, instance) else {
                return false;
            };
            if index == 0 {
                peer_unknown = self.unknown[1].contains(&key[..64]);
            }
            if !self.counts.contains_key(&key) {
                *reason = 3;
                return false;
            }
            match group_of.get(&key) {
                Some(&g) => groups[g].1.push(index),
                None => {
                    group_of.insert(key.clone(), groups.len());
                    groups.push((key, vec![index]));
                }
            }
        }
        let mut result = vec![0u8; count];
        for (key, members) in &groups {
            let counted = self.counts[key];
            let n = members.len() as u32;
            let common = counted[1].min(n);
            if counted[0] < n {
                *reason = 3;
                return false;
            }
            if peer_unknown && common < n {
                *reason = 2;
                return false;
            }
            for (rank, &index) in members.iter().enumerate() {
                result[index] = if (rank as u32) < common { 3 } else { 2 };
            }
        }
        for (key, members) in &groups {
            let n = members.len() as u32;
            let counted = self.counts.get_mut(key).unwrap();
            counted[0] -= n;
            counted[1] -= counted[1].min(n);
        }
        masks.copy_from_slice(&result);
        *reason = 0;
        true
    }

    /// The peer's instances beyond the main eye's for geometries the main eye knows.
    pub fn known_extras(&self) -> u32 {
        self.initial
            .iter()
            .filter(|(key, count)| !self.unknown[0].contains(&key[..64]) && count[1] > count[0])
            .fold(0u32, |total, (_, count)| {
                total.wrapping_add(count[1] - count[0])
            })
    }

    /// The peer's unclaimed extra instance bytes of the draw's geometry (claimed once).
    pub fn extras(&mut self, draw: &[u8; 64], stride: u32) -> Option<Vec<u8>> {
        let stride = stride as usize;
        if stride != 48 && stride != 64 {
            return None;
        }
        let key = geometry_instance_key(draw, &[0u8; 64][..stride])?;
        let geometry = key[..64].to_vec();
        let Some(group) = self.peer_geometry.get(&geometry) else {
            return Some(Vec::new());
        };
        let mut bytes = Vec::new();
        let mut selected = Vec::new();
        for candidate in group {
            if candidate.len() != 64 + stride || self.claimed.contains(candidate) {
                continue;
            }
            let count = self.initial[candidate];
            if count[1] <= count[0] {
                continue;
            }
            if self.unknown[0].contains(&geometry) {
                return None;
            }
            let n = count[1] - count[0];
            if n as u64 * stride as u64 > (16384 - bytes.len()) as u64 {
                return None;
            }
            for _ in 0..n {
                bytes.extend_from_slice(&candidate[64..]);
            }
            selected.push(candidate.clone());
        }
        self.claimed.extend(selected);
        Some(bytes)
    }
}

// ---- the C ABI ------------------------------------------------------------------------------------------------------

unsafe fn slice<'a, T>(data: *const T, count: usize) -> &'a [T] {
    if count == 0 || data.is_null() {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, count) }
    }
}

/// GeometryInstanceKey into `out` (128 bytes), its length into `length`.
///
/// # Safety
/// `draw` is valid; `instance` holds `bytes`; `out` holds 128 bytes; `length` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_key(
    draw: *const [u8; 64],
    instance: *const u8,
    bytes: usize,
    out: *mut u8,
    length: *mut usize,
) -> bool {
    unsafe {
        let Some(key) = geometry_instance_key(&*draw, slice(instance, bytes)) else {
            return false;
        };
        core::ptr::copy_nonoverlapping(key.as_ptr(), out, key.len());
        *length = key.len();
        true
    }
}

/// GpuInstanceIdentities: valid, and with `out` (count*48 bytes) the identities written.
///
/// # Safety
/// `out` is null or holds count*48 bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_identities(
    resource: u64,
    address: u64,
    count: u32,
    stride: u32,
    out: *mut u8,
) -> bool {
    if !gpu_identities_valid(resource, address, count, stride) {
        return false;
    }
    if !out.is_null() {
        for i in 0..count as usize {
            let row = unsafe { core::slice::from_raw_parts_mut(out.add(i * 48), 48) };
            row.fill(0);
            row[0..8].copy_from_slice(&resource.to_le_bytes());
            row[8..16].copy_from_slice(&(address + i as u64 * 48).to_le_bytes());
        }
    }
    true
}

/// A membership; free it with cvr_em_free.
#[no_mangle]
pub extern "C-unwind" fn cvr_em_new() -> *mut EyeMembership {
    Box::into_raw(Box::default())
}

/// # Safety
/// `membership` comes from cvr_em_new (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_free(membership: *mut EyeMembership) {
    if !membership.is_null() {
        drop(unsafe { Box::from_raw(membership) });
    }
}

/// EyeMembership::Add; the totals and unresolved counts into `state` ([totals 0, totals 1, unresolved 0, unresolved 1]).
///
/// # Safety
/// `membership` is live; `input` is valid; `blob` holds `bytes`; `state` holds four values.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_add(
    membership: *mut EyeMembership,
    side: u32,
    packet_second: u64,
    input: *const Inputs,
    blob: *const u8,
    bytes: usize,
    state: *mut u32,
) -> bool {
    unsafe {
        let m = &mut *membership;
        let added = m.add(side, packet_second, &*input, slice(blob, bytes));
        let state = core::slice::from_raw_parts_mut(state, 4);
        state[..2].copy_from_slice(&m.totals);
        state[2..].copy_from_slice(&m.unresolved);
        added
    }
}

/// EyeMembership::Plan into `masks` (`mask_count` = instances / stride).
///
/// # Safety
/// `membership` is live; `draw` is valid; `instances` holds `bytes`; `masks` holds `mask_count`; `reason` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_plan(
    membership: *mut EyeMembership,
    draw: *const [u8; 64],
    instances: *const u8,
    bytes: usize,
    stride: u32,
    masks: *mut u8,
    mask_count: usize,
    reason: *mut u32,
) -> bool {
    unsafe {
        let masks = if mask_count == 0 || masks.is_null() {
            &mut [][..]
        } else {
            core::slice::from_raw_parts_mut(masks, mask_count)
        };
        (*membership).plan(&*draw, slice(instances, bytes), stride, masks, &mut *reason)
    }
}

/// EyeMembership::KnownExtras.
///
/// # Safety
/// `membership` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_known_extras(membership: *const EyeMembership) -> u32 {
    unsafe { (*membership).known_extras() }
}

/// EyeMembership::Extras: the bytes (read them with cvr_em_bytes, free with cvr_em_bytes_free) or null.
///
/// # Safety
/// `membership` is live; `draw` is valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_extras(
    membership: *mut EyeMembership,
    draw: *const [u8; 64],
    stride: u32,
) -> *mut Vec<u8> {
    unsafe {
        match (*membership).extras(&*draw, stride) {
            Some(bytes) => Box::into_raw(Box::new(bytes)),
            None => core::ptr::null_mut(),
        }
    }
}

/// The bytes of cvr_em_extras.
///
/// # Safety
/// `bytes` comes from cvr_em_extras; `length` is writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_bytes(
    bytes: *const Vec<u8>,
    length: *mut usize,
) -> *const u8 {
    unsafe {
        *length = (*bytes).len();
        (*bytes).as_ptr()
    }
}

/// # Safety
/// `bytes` comes from cvr_em_extras (or is null) and is not used again.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_em_bytes_free(bytes: *mut Vec<u8>) {
    if !bytes.is_null() {
        drop(unsafe { Box::from_raw(bytes) });
    }
}
