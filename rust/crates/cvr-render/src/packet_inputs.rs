//! A native geometry packet's inputs (`include/Render/NativePacketInputs.hpp`): owned snapshots of what one packet's
//! draw reads, by the 2.5 layout of the packet consumer (0x334CC0), the transform gather (0x335544) and the direct draw
//! path (0x3335D8) -- the 64-byte draw record (pool + 0x50), the six-byte shader entry (0x2541D0), the 32-byte state
//! record (0x240110: the command program pointer at +0x08, its bytes at +0x10), the 24-byte object (0x270248), the
//! first 48-byte transform (0x570280) and the skin metadata (the object's +8 pointer, +0x14); then every instance (a
//! CPU stream, the preuploaded instance buffer, or skinned matrices with the skin metadata appended) and a nonempty
//! command program (validated: at most 128 operations, inside 4 KiB) copied into the caller's blob while the native
//! input is live; and the geometry projection's equality. Snapshots, not reusable native buffer handles: a matching
//! program is not proof that what it reads also matches.
//!
//! In the Rust build (`CVR_RUST_VRIK`) `ResolveInputs`, `ValidNativeProgram`, `ResolveDetails` and `EqualGeometry`
//! forward to `cvr_pkt_*`; the blob stays the caller's vector, grown and truncated through the face's callback with its
//! own growth policy. `tools/single_pass_tests/packet_inputs_test` runs its cases against them in its Rust variant.

use core::ffi::c_void;

pub const MAX_INPUT_BLOB_BYTES: usize = 32 * 1024 * 1024;
pub const STATE_BYTES: usize = 32;
pub const STATE_PROGRAM: usize = 0x08;
pub const STATE_PROGRAM_BYTES: usize = 0x10;

/// packets::Packet.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Packet {
    pub first: u64,
    pub second: u64,
}
/// packets::Inputs.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
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
impl Default for Inputs {
    fn default() -> Self {
        Inputs {
            valid: 0,
            instances: 0,
            command_program_field: 0,
            source: 0,
            draw: [0; 64],
            shaders: [0; 8],
            state: [0; 40],
            object: [0; 24],
            transform: [0; 48],
            skin: [0; 16],
            instance_offset: 0,
            instance_bytes: 0,
            instance_stride: 0,
            program_offset: 0,
            program_bytes: 0,
            reserved: 0,
            instance_gpu_address: 0,
            instance_resource: 0,
        }
    }
}
/// packets::MappedInstances.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MappedInstances {
    pub cpu: u64,
    pub gpu: u64,
    pub bytes: u64,
    pub resource: u64,
}
const _: () =
    assert!(core::mem::size_of::<Inputs>() == 256 && core::mem::size_of::<MappedInstances>() == 32);

/// A read of the engine's memory into `out`: (address, out) -> readable.
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

/// ResolveInputs.
pub fn resolve_inputs(
    packet: Packet,
    pool: u64,
    read: Option<Read<'_>>,
    geometry_only: bool,
) -> Inputs {
    let mut result = Inputs {
        instances: ((packet.second >> 18) & 0x7fff) as u32,
        ..Default::default()
    };
    let Some(read) = read else { return result };
    if pool == 0 {
        return result;
    }
    let mut copy = |result: &mut Inputs,
                    offset: u64,
                    index: u64,
                    stride: u64,
                    which: usize,
                    bytes: usize,
                    flag: u32| {
        let relative = offset.wrapping_add(index.wrapping_mul(stride));
        if relative > u64::MAX - pool || pool + relative > u64::MAX - bytes as u64 {
            return;
        }
        let output: &mut [u8] = match which {
            0 => &mut result.draw[..bytes],
            1 => &mut result.shaders[..bytes],
            2 => &mut result.state[..bytes],
            3 => &mut result.object[..bytes],
            _ => &mut result.transform[..bytes],
        };
        if read(pool + relative, output) {
            result.valid |= flag;
        }
    };
    copy(
        &mut result,
        0x50,
        (packet.first >> 17) & 0x7fff,
        64,
        0,
        64,
        1,
    );
    let skin = packet.second & (1 << 50) != 0;
    if !geometry_only {
        copy(
            &mut result,
            0x2541D0,
            (packet.first >> 32) & 0x1fff,
            6,
            1,
            6,
            2,
        );
        copy(
            &mut result,
            0x240110,
            (packet.first >> 45) & 0x7ff,
            STATE_BYTES as u64,
            2,
            STATE_BYTES,
            4,
        );
    }
    if !geometry_only || skin {
        copy(&mut result, 0x270248, packet.second & 0x3ffff, 24, 3, 24, 8);
    }
    if result.valid & 4 != 0 {
        result.command_program_field = u32::from_le_bytes(
            result.state[STATE_PROGRAM_BYTES..STATE_PROGRAM_BYTES + 4]
                .try_into()
                .unwrap(),
        );
    }
    let preuploaded = packet.first & (1 << 59) != 0 && !skin;
    let ordinary = result.valid & 1 != 0 && result.draw[0] == 0 && packet.second & (1 << 51) != 0;
    result.source = if ordinary {
        if preuploaded {
            2
        } else {
            1
        }
    } else {
        0
    };
    if !geometry_only && result.instances == 1 && result.source == 1 {
        copy(
            &mut result,
            0x570280,
            (packet.second >> 33) & 0x1ffff,
            48,
            4,
            48,
            16,
        );
    }
    if packet.second & (1 << 50) == 0 {
        result.valid |= 32;
    } else if result.valid & 8 != 0 {
        let metadata = u64::from_le_bytes(result.object[8..16].try_into().unwrap());
        if metadata == 0 {
            result.skin[12..16].copy_from_slice(&1u32.to_le_bytes());
            result.valid |= 32;
        } else if metadata <= u64::MAX - 0x24 && read(metadata + 0x14, &mut result.skin) {
            result.valid |= 32;
        }
    }
    result
}

/// ValidNativeProgram.
pub fn valid_native_program(program: &[u8]) -> bool {
    if program.len() < 8 || program.len() > 4096 {
        return false;
    }
    let count = u32::from_le_bytes(program[0..4].try_into().unwrap());
    if count > 128 {
        return false;
    }
    let mut at = 8usize;
    for _ in 0..count {
        if program.len() - at < 8 {
            return false;
        }
        let bytes = u32::from_le_bytes(program[at + 4..at + 8].try_into().unwrap()) as usize;
        at += 8;
        if bytes > program.len() - at {
            return false;
        }
        at += bytes;
    }
    at == program.len()
}

/// The caller's blob: its length, its data and its resize (growing reserves by the caller's policy).
pub struct Blob<'a> {
    pub len: usize,
    pub data: *mut u8,
    pub resize: &'a mut dyn FnMut(usize, bool) -> *mut u8,
}
impl Blob<'_> {
    fn reserve(&mut self, bytes: usize) -> Option<u32> {
        if bytes > MAX_INPUT_BLOB_BYTES || self.len > MAX_INPUT_BLOB_BYTES - bytes {
            return None;
        }
        let offset = self.len;
        self.data = (self.resize)(self.len + bytes, true);
        self.len += bytes;
        Some(offset as u32)
    }
    fn truncate(&mut self, offset: usize) {
        self.data = (self.resize)(offset, false);
        self.len = offset;
    }
    fn slice(&mut self, offset: usize, bytes: usize) -> &mut [u8] {
        if bytes == 0 {
            return &mut [];
        }
        // SAFETY: the caller's resize keeps `len` bytes at `data`.
        unsafe { core::slice::from_raw_parts_mut(self.data.add(offset), bytes) }
    }
}

/// ResolveDetails: the packet's inputs with its program and every instance copied into `blob`.
pub fn resolve_details(
    packet: Packet,
    pool: u64,
    uploaded: MappedInstances,
    read: Option<Read<'_>>,
    blob: &mut Blob,
    geometry_only: bool,
) -> Inputs {
    let Some(read) = read else {
        return finish_without_reader(resolve_inputs(packet, pool, None, geometry_only));
    };
    let mut result = resolve_inputs(packet, pool, Some(&mut *read), geometry_only);
    // copy: reserve, read into the blob, truncate back on a refused read.
    let mut copy = |blob: &mut Blob, address: u64, bytes: usize, offset: &mut u32| -> bool {
        if address == 0 || address > u64::MAX - bytes as u64 {
            return false;
        }
        let Some(at) = blob.reserve(bytes) else {
            return false;
        };
        *offset = at;
        if read(address, blob.slice(at as usize, bytes)) {
            return true;
        }
        blob.truncate(at as usize);
        false
    };
    if result.valid & 4 != 0 {
        if result.command_program_field == 0 {
            result.valid |= 64;
        } else if result.command_program_field <= 4096 {
            let address = u64::from_le_bytes(
                result.state[STATE_PROGRAM..STATE_PROGRAM + 8]
                    .try_into()
                    .unwrap(),
            );
            let bytes = result.command_program_field as usize;
            let mut offset = result.program_offset;
            if copy(blob, address, bytes, &mut offset) {
                result.program_offset = offset;
                let program = blob.slice(offset as usize, bytes).to_vec();
                if valid_native_program(&program) {
                    result.program_bytes = result.command_program_field;
                    result.valid |= 64;
                } else {
                    blob.truncate(offset as usize);
                }
            } else {
                result.program_offset = offset;
            }
        }
    }
    result.valid &= !16u32; // the complete stream below is the authority, not its first matrix
    if result.instances == 0 || result.source == 0 {
        return result;
    }
    let skin = packet.second & (1 << 50) != 0;
    let first = (packet.second >> 33) & 0x1ffff;
    result.instance_stride = if skin { 64 } else { 48 };
    let bytes = result.instances as usize * result.instance_stride as usize;
    if result.source == 2 {
        let offset = first * 48;
        if uploaded.gpu != 0
            && offset <= uploaded.bytes
            && bytes as u64 <= uploaded.bytes - offset
            && uploaded.gpu <= u64::MAX - offset
        {
            result.instance_gpu_address = uploaded.gpu + offset;
            result.instance_resource = uploaded.resource;
            let mut at = result.instance_offset;
            if uploaded.cpu != 0
                && uploaded.cpu <= u64::MAX - offset
                && copy(blob, uploaded.cpu + offset, bytes, &mut at)
            {
                result.instance_offset = at;
                result.instance_bytes = bytes as u32;
                result.valid |= 16;
            } else {
                result.instance_offset = at;
            }
        }
    } else if pool != 0 && pool <= u64::MAX - 0x570280 - first * 48 {
        let source = pool + 0x570280 + first * 48;
        if !skin {
            let mut at = result.instance_offset;
            if copy(blob, source, bytes, &mut at) {
                result.instance_bytes = bytes as u32;
                result.valid |= 16;
            }
            result.instance_offset = at;
        } else if result.valid & 32 != 0 {
            let mut matrices = vec![0u8; result.instances as usize * 48];
            if source <= u64::MAX - matrices.len() as u64 && read(source, &mut matrices) {
                if let Some(at) = blob.reserve(bytes) {
                    result.instance_offset = at;
                    let skin_bytes = result.skin;
                    let destination = blob.slice(at as usize, bytes);
                    for i in 0..result.instances as usize {
                        destination[i * 64..i * 64 + 48]
                            .copy_from_slice(&matrices[i * 48..i * 48 + 48]);
                        destination[i * 64 + 48..i * 64 + 64].copy_from_slice(&skin_bytes);
                    }
                    result.instance_bytes = bytes as u32;
                    result.valid |= 16;
                }
            }
        }
    }
    if result.valid & 16 != 0 {
        let first_matrix = blob.slice(result.instance_offset as usize, 48).to_vec();
        result.transform.copy_from_slice(&first_matrix);
    }
    result
}
/// ResolveDetails without a reader: ResolveInputs refused everything, and the details find nothing to copy.
fn finish_without_reader(mut result: Inputs) -> Inputs {
    result.valid &= !16u32;
    if result.instances != 0 && result.source != 0 {
        // Unreachable: without a reader ResolveInputs leaves source 0.
    }
    result
}

/// EqualGeometry.
pub fn equal_geometry(a: &Inputs, a_blob: &[u8], b: &Inputs, b_blob: &[u8]) -> bool {
    if (a.valid & 17) != (b.valid & 17)
        || a.instances != b.instances
        || a.draw != b.draw
        || a.source != b.source
        || a.instance_stride != b.instance_stride
        || a.instance_bytes != b.instance_bytes
        || a.instance_gpu_address != b.instance_gpu_address
        || a.instance_resource != b.instance_resource
    {
        return false;
    }
    if a.valid & 16 == 0 {
        return true;
    }
    let (ao, ab, bo) = (
        a.instance_offset as usize,
        a.instance_bytes as usize,
        b.instance_offset as usize,
    );
    if ao > a_blob.len()
        || ab > a_blob.len() - ao
        || bo > b_blob.len()
        || b.instance_bytes as usize > b_blob.len() - bo
    {
        return false;
    }
    a_blob[ao..ao + ab] == b_blob[bo..bo + ab]
}

// ---- C ABI ----------------------------------------------------------------------------------------------------------

/// packets::ReadInputs: (context, address, out, bytes) -> readable.
pub type ReadInputs = unsafe extern "C-unwind" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
/// The face's blob resize: (blob, size, growing) -> data.
pub type ResizeBlob = unsafe extern "C-unwind" fn(*mut c_void, usize, bool) -> *mut u8;

unsafe fn slice<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(data, len) }
    }
}

/// ResolveInputs.
///
/// # Safety
/// `output` is writable; `read` follows packets::ReadInputs.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pkt_resolve_inputs(
    packet: *const Packet,
    pool: u64,
    read: Option<ReadInputs>,
    context: *mut c_void,
    geometry_only: bool,
    output: *mut Inputs,
) {
    unsafe {
        let mut reader = read.map(|read| {
            move |address: u64, out: &mut [u8]| {
                read(context, address, out.as_mut_ptr().cast(), out.len())
            }
        });
        *output = resolve_inputs(
            *packet,
            pool,
            reader
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            geometry_only,
        );
    }
}
/// ValidNativeProgram.
///
/// # Safety
/// `program` holds `bytes` bytes.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pkt_valid_program(program: *const u8, bytes: usize) -> bool {
    unsafe { valid_native_program(slice(program, bytes)) }
}
/// ResolveDetails over the caller's blob (`size` its length, `data` its bytes, `resize` its resize).
///
/// # Safety
/// `output` is writable; `read` follows packets::ReadInputs; `resize` keeps the blob's bytes valid.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C-unwind" fn cvr_pkt_resolve_details(
    packet: *const Packet,
    pool: u64,
    uploaded: *const MappedInstances,
    read: Option<ReadInputs>,
    context: *mut c_void,
    blob: *mut c_void,
    size: usize,
    data: *mut u8,
    resize: ResizeBlob,
    geometry_only: bool,
    output: *mut Inputs,
) {
    unsafe {
        let mut reader = read.map(|read| {
            move |address: u64, out: &mut [u8]| {
                read(context, address, out.as_mut_ptr().cast(), out.len())
            }
        });
        let mut grow = |size: usize, growing: bool| resize(blob, size, growing);
        let mut view = Blob {
            len: size,
            data,
            resize: &mut grow,
        };
        *output = resolve_details(
            *packet,
            pool,
            *uploaded,
            reader
                .as_mut()
                .map(|f| f as &mut dyn FnMut(u64, &mut [u8]) -> bool),
            &mut view,
            geometry_only,
        );
    }
}
/// EqualGeometry.
///
/// # Safety
/// The inputs are valid; the blobs hold their lengths.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_pkt_equal_geometry(
    a: *const Inputs,
    a_blob: *const u8,
    a_len: usize,
    b: *const Inputs,
    b_blob: *const u8,
    b_len: usize,
) -> bool {
    unsafe { equal_geometry(&*a, slice(a_blob, a_len), &*b, slice(b_blob, b_len)) }
}
