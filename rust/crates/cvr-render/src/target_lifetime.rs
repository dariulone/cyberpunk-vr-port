//! THE BORROWED TARGET LIFETIMES (formerly src/Render/NativeTargetLifetime.cpp and the readers of
//! include/Render/NativeTargetLifetime.hpp): a writable placed target the engine draws into is captured on the original
//! draw (its authenticated writable identity, texture_writable.rs) together with a POD snapshot of its native lifetime
//! -- the texture record and its owner control block, the pooled placed allocation and its allocator, the registry's
//! deferred-retirement epoch (a zero handle reference is accepted only as a member of the current unfenced bucket) and
//! the swapchain handle table --, then bound inside the same synchronous ExecuteSubset invocation (queue_residency.rs)
//! while its submission is pending; the borrowed lifetime is valid only while that scope lives and the snapshot reads
//! the same. The resize / shutdown entry points must be the original ones or the audited CET hook chain (no pointer of
//! the chain is executed). The C++ proofs (shared, holding the C++ identity and scope) stay the C++'s; the logic and the
//! failure state are here.
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering::*};
use std::sync::OnceLock;

use windows::core::Interface as _;
use windows::Win32::Graphics::Direct3D12::*;

use crate::native_recording::{
    cvr_native_recording_compatible, cvr_native_recording_validate_recording,
    cvr_native_recording_validate_submission, fingerprint, Context as RecordingContext,
};
use crate::queue_residency::{reader, Mem, ReadMemory};
use crate::source_cbv_details::{read_allocation, read_allocator, Allocation, Allocator, KNOWN};
use crate::texture_lease::{device_of, game, read};
use crate::texture_writable::{
    same_creation_metadata, same_heap_description, same_heap_properties, same_resource_description,
    writable_record, WritableIdentity,
};

pub const RETIREMENT_RVA: u64 = 0x424FCC0;
pub const REGISTRY_RVA: u64 = 0x3BCDCA8;
pub const RING_SIZE: u32 = 32769;
pub const SWAPCHAIN_TABLE_OFFSET: u64 = 0xCEBF30;
pub const REGISTRY_LOCK_OFFSET: u64 = 0x2F180;
pub const SWAPCHAIN_COUNT: usize = 32;
pub const RESIZE_RVA: u64 = 0x68813C;
pub const SHUTDOWN_RVA: u64 = 0x115CE4C;
const CET_RESET_RVA: u64 = 0xAA90;
const CET_RESET_BYTES: usize = 463;
const CET_RESET_HASH: u64 = 0xD16377FED0A771AC;

/// detail::HookContract: the native entry (whole, or its suffix behind a 5-byte jump) and CET's wrapper chain.
pub struct HookContract {
    pub native_rva: u64,
    pub native_bytes: usize,
    pub native_hash: u64,
    pub suffix_hash: u64,
    pub wrapper_rva: u64,
    pub wrapper_bytes: usize,
    pub wrapper_hash: u64,
    pub original_offset: u64,
    pub stolen: [u8; 7],
    pub stolen_bytes: usize,
}
// The hashes are the header's (2.31 values; see 65e113d9).
pub const RESIZE_HOOK: HookContract = HookContract {
    native_rva: RESIZE_RVA,
    native_bytes: 1042,
    native_hash: 0xACD5BEDEE6CECC40,
    suffix_hash: 0xFA1423D5B68EE457,
    wrapper_rva: 0x2BD10,
    wrapper_bytes: 245,
    wrapper_hash: 0x3F494FE96A691BB0,
    original_offset: 0x2410,
    stolen: [0x48, 0x8B, 0xC4, 0x44, 0x88, 0x48, 0x20],
    stolen_bytes: 7,
};
pub const SHUTDOWN_HOOK: HookContract = HookContract {
    native_rva: SHUTDOWN_RVA,
    native_bytes: 112,
    native_hash: 0x629A9859BE5E0E56,
    suffix_hash: 0xF19A4C0DC1521A40,
    wrapper_rva: 0x2BE10,
    wrapper_bytes: 107,
    wrapper_hash: 0x6EB46F90E0FD5752,
    original_offset: 0x2418,
    stolen: [0x40, 0x53, 0x48, 0x83, 0xEC, 0x20, 0],
    stolen_bytes: 6,
};

/// detail::Key.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Key {
    pub registry: u64,
    pub context: u64,
    pub list: u64,
    pub resource: u64,
    pub heap: u64,
    pub bytes: u64,
    pub alignment: u64,
    pub handle: u32,
}
/// detail::Epoch: the deferred-retirement ring's current bucket.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Epoch {
    pub address: u64,
    pub bucket: u64,
    pub fence: u64,
    pub value: u64,
    pub reserved: u64,
    pub head: u64,
    pub committed: u64,
    pub index: u32,
    pub count: u32,
    pub outstanding: u32,
}
/// detail::Swapchains: the 32 swapchain texture handle slots.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Swapchains {
    pub table: u64,
    pub lock: u64,
    pub free_count: u32,
    pub references: [i32; SWAPCHAIN_COUNT],
    pub handles: [u32; SWAPCHAIN_COUNT],
}
impl Default for Swapchains {
    fn default() -> Self {
        Swapchains {
            table: 0,
            lock: 0,
            free_count: 0,
            references: [0; 32],
            handles: [0; 32],
        }
    }
}
/// detail::Snapshot.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Snapshot {
    pub base: u64,
    pub record: u64,
    pub owner: u64,
    pub queue_slot: u64,
    pub handle_references: i32,
    pub owner_references: i32,
    pub key: Key,
    pub epoch: Epoch,
    pub swapchains: Swapchains,
    pub control: [u8; 0x38],
    pub allocation: Allocation,
    pub allocator: Allocator,
}
impl Default for Snapshot {
    fn default() -> Self {
        Snapshot {
            base: 0,
            record: 0,
            owner: 0,
            queue_slot: u64::MAX,
            handle_references: 0,
            owner_references: 0,
            key: Key::default(),
            epoch: Epoch::default(),
            swapchains: Swapchains::default(),
            control: [0; 0x38],
            allocation: Allocation::default(),
            allocator: Allocator::default(),
        }
    }
}
/// detail::ResizeChain.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ResizeChain {
    pub relay: u64,
    pub cet: u64,
    pub singleton: u64,
    pub trampoline: u64,
    pub mode: u32,
}
const _: () = assert!(
    core::mem::size_of::<Key>() == 64
        && core::mem::size_of::<Epoch>() == 72
        && core::mem::size_of::<Swapchains>() == 280
        && core::mem::size_of::<Snapshot>() == 768
        && core::mem::offset_of!(Snapshot, key) == 40
        && core::mem::offset_of!(Snapshot, control) == 456
        && core::mem::offset_of!(Snapshot, allocation) == 512
        && core::mem::size_of::<ResizeChain>() == 40
);

fn read_into<T: Copy>(mem: Mem, at: u64, value: &mut T) -> bool {
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), core::mem::size_of::<T>())
    };
    mem(at, bytes)
}
/// detail::Field (queue_residency's): a value at address+offset, written in place.
fn field<T: Copy>(mem: Mem, address: u64, offset: u64, value: &mut T) -> bool {
    address != 0 && address <= u64::MAX - offset && read_into(mem, address + offset, value)
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

/// detail::RelativeTarget: next + displacement, refusing wrap-around and a null result.
pub fn relative_target(next: u64, displacement: i32) -> Option<u64> {
    let output = if displacement < 0 {
        let distance = (-(displacement as i64)) as u64;
        if next < distance {
            return None;
        }
        next - distance
    } else {
        if next > u64::MAX - displacement as u32 as u64 {
            return None;
        }
        next + displacement as u32 as u64
    };
    (output != 0).then_some(output)
}
/// detail::ReadHookChain: the native entry untouched (mode 1), or CET's installed chain -- a jump to a relay, the relay
/// to CET's audited wrapper in the loaded module, ResetState, the singleton's original pointer to a trampoline of the
/// stolen bytes rejoining the native entry -- everything read twice (mode 2). Rejections: 1 native read, 2 entry/body,
/// 3 relative target, 4 relay, 5 module, 6 wrapper, 7 ResetState, 8 singleton, 9 original pointer, 10 trampoline, 11
/// changing chain. The chain read so far is returned with a rejection.
pub fn read_hook_chain(mem: Mem, base: u64, cet: u64, spec: &HookContract) -> (ResizeChain, u32) {
    let mut value = ResizeChain::default();
    let mut native = vec![0u8; spec.native_bytes];
    if base == 0
        || base > u64::MAX - spec.native_rva - spec.native_bytes as u64
        || !mem(base + spec.native_rva, &mut native)
    {
        return (value, 1);
    }
    if fingerprint(&native) == spec.native_hash {
        let mut again = vec![0u8; spec.native_bytes];
        if !mem(base + spec.native_rva, &mut again) || again != native {
            return (value, 11);
        }
        value.mode = 1;
        return (value, 0);
    }
    if native[0] != 0xE9 || fingerprint(&native[5..]) != spec.suffix_hash {
        return (value, 2);
    }
    let displacement = i32::from_le_bytes(native[1..5].try_into().unwrap());
    match relative_target(base + spec.native_rva + 5, displacement) {
        Some(relay) => value.relay = relay,
        None => return (value, 3),
    }
    const JUMP: [u8; 6] = [0xFF, 0x25, 0, 0, 0, 0];
    let mut relay = [0u8; 14];
    if !mem(value.relay, &mut relay) || relay[..6] != JUMP {
        return (value, 4);
    }
    let destination = u64_at(&relay, 6);
    value.cet = cet;
    if cet == 0 || cet > u64::MAX - 0x730000 || destination != cet + spec.wrapper_rva {
        return (value, 5);
    }
    let mut dos = [0u8; 64];
    let mut nt = [0u8; 264];
    let lfanew = |dos: &[u8; 64]| i32::from_le_bytes(dos[0x3C..0x40].try_into().unwrap());
    if !mem(cet, &mut dos)
        || u16::from_le_bytes([dos[0], dos[1]]) != 0x5A4D
        || lfanew(&dos) <= 0
        || lfanew(&dos) > 0x1000
        || !field(mem, cet, lfanew(&dos) as u32 as u64, &mut nt)
        || u32_at(&nt, 0) != 0x4550
        || u16::from_le_bytes([nt[4], nt[5]]) != 0x8664
        || u32_at(&nt, 8) != 0x68D9142A
        || u16::from_le_bytes([nt[24], nt[25]]) != 0x20B
        || u32_at(&nt, 24 + 56) != 0x730000
    {
        return (value, 5);
    }
    let mut wrapper = vec![0u8; spec.wrapper_bytes];
    let mut reset = vec![0u8; CET_RESET_BYTES];
    if !mem(cet + spec.wrapper_rva, &mut wrapper) || fingerprint(&wrapper) != spec.wrapper_hash {
        return (value, 6);
    }
    if !mem(cet + CET_RESET_RVA, &mut reset) || fingerprint(&reset) != CET_RESET_HASH {
        return (value, 7);
    }
    if !field(mem, cet, 0x6DD8F0, &mut value.singleton) || value.singleton == 0 {
        return (value, 8);
    }
    if !field(
        mem,
        value.singleton,
        spec.original_offset,
        &mut value.trampoline,
    ) || value.trampoline == 0
    {
        return (value, 9);
    }
    let mut trampoline = vec![0u8; spec.stolen_bytes + 14];
    if !mem(value.trampoline, &mut trampoline)
        || trampoline[..spec.stolen_bytes] != spec.stolen[..spec.stolen_bytes]
        || trampoline[spec.stolen_bytes..spec.stolen_bytes + 6] != JUMP
    {
        return (value, 10);
    }
    if u64_at(&trampoline, spec.stolen_bytes + 6)
        != base + spec.native_rva + spec.stolen_bytes as u64
    {
        return (value, 10);
    }
    let mut native_again = vec![0u8; spec.native_bytes];
    let mut relay_again = [0u8; 14];
    let mut trampoline_again = vec![0u8; spec.stolen_bytes + 14];
    let mut wrapper_again = vec![0u8; spec.wrapper_bytes];
    let mut reset_again = vec![0u8; CET_RESET_BYTES];
    let mut dos_again = [0u8; 64];
    let mut nt_again = [0u8; 264];
    let (mut singleton_again, mut original_again) = (0u64, 0u64);
    if !mem(base + spec.native_rva, &mut native_again)
        || native_again != native
        || !mem(value.relay, &mut relay_again)
        || relay_again != relay
        || !mem(cet, &mut dos_again)
        || dos_again != dos
        || !field(mem, cet, lfanew(&dos) as u32 as u64, &mut nt_again)
        || nt_again != nt
        || !mem(cet + spec.wrapper_rva, &mut wrapper_again)
        || wrapper_again != wrapper
        || !mem(cet + CET_RESET_RVA, &mut reset_again)
        || reset_again != reset
        || !field(mem, cet, 0x6DD8F0, &mut singleton_again)
        || singleton_again != value.singleton
        || !field(
            mem,
            value.singleton,
            spec.original_offset,
            &mut original_again,
        )
        || original_again != value.trampoline
        || !mem(value.trampoline, &mut trampoline_again)
        || trampoline_again != trampoline
    {
        return (value, 11);
    }
    value.mode = 2;
    (value, 0)
}
/// detail::ReadSwapchains: the swapchain table under no lock, its free count consistent, none of its 32 handles the
/// key's, read twice where it can change.
pub fn read_swapchains(mem: Mem, key: &Key) -> Option<Swapchains> {
    if key.registry == 0 || key.registry > u64::MAX - SWAPCHAIN_TABLE_OFFSET - 0x1650 {
        return None;
    }
    let mut value = Swapchains {
        table: key.registry + SWAPCHAIN_TABLE_OFFSET,
        ..Default::default()
    };
    let mut locked = 0u8;
    if !field(mem, value.table, 0, &mut value.lock)
        || value.lock != key.registry + REGISTRY_LOCK_OFFSET
        || !field(mem, value.lock, 0, &mut locked)
        || locked != 0
        || !field(mem, value.table, 8, &mut value.free_count)
        || value.free_count > SWAPCHAIN_COUNT as u32
    {
        return None;
    }
    let mut free = 0u32;
    for i in 0..SWAPCHAIN_COUNT {
        if !field(
            mem,
            value.table,
            0x10 + i as u64 * 0xB0,
            &mut value.references[i],
        ) || value.references[i] < -1
            || value.references[i] == i32::MAX
            || !field(
                mem,
                value.table,
                0x40 + i as u64 * 0xB0,
                &mut value.handles[i],
            )
            || value.handles[i] > 32768
            || value.handles[i] == key.handle
        {
            return None;
        }
        free += (value.references[i] == -1) as u32;
    }
    let (mut lock_again, mut count_again) = (0u64, 0u32);
    if free != value.free_count
        || !field(mem, value.table, 0, &mut lock_again)
        || lock_again != value.lock
        || !field(mem, value.table, 8, &mut count_again)
        || count_again != value.free_count
        || !field(mem, value.lock, 0, &mut locked)
        || locked != 0
    {
        return None;
    }
    Some(value)
}
/// detail::ReadEpoch: the deferred-retirement ring's current unfenced bucket, with no writer and the registry's
/// retirement lock free (read before and after).
pub fn read_epoch(mem: Mem, base: u64, registry: u64) -> Option<Epoch> {
    if base == 0 || base > u64::MAX - RETIREMENT_RVA - 0x20CD1 {
        return None;
    }
    let mut next = Epoch {
        address: base + RETIREMENT_RVA,
        ..Default::default()
    };
    let (mut actual, mut stopped, mut lock, mut writers) = (0u64, 0u8, 0u8, 0u8);
    if !field(mem, base, REGISTRY_RVA, &mut actual)
        || actual != registry
        || !field(mem, registry, 1, &mut stopped)
        || stopped != 0
        || !field(mem, registry, 0x1410734, &mut lock)
        || lock != 0
        || !field(mem, registry, 0x1410730, &mut next.outstanding)
        || next.outstanding == 0
        || next.outstanding >= i32::MAX as u32
        || !field(mem, next.address, 0x20CD0, &mut writers)
        || writers != 0
        // +0x20CC0: the texture ring's bucket index.
        || !field(mem, next.address, 0x20CC0, &mut next.index)
        || next.index >= 128
    {
        return None;
    }
    next.bucket = next.address + 0x200C0 + 24 * next.index as u64;
    if !field(mem, next.bucket, 0, &mut next.fence)
        || next.fence != 0
        || !field(mem, next.bucket, 8, &mut next.value)
        || next.value != 0
        || !field(mem, next.bucket, 16, &mut next.count)
        || next.count >= RING_SIZE
        || !field(mem, next.address, 0, &mut next.reserved)
        || !field(mem, next.address, 0x20040, &mut next.head)
        || !field(mem, next.address, 0x20080, &mut next.committed)
        || next.reserved != next.committed
        || next.head > next.committed
        || next.committed - next.head >= RING_SIZE as u64
        || next.count as u64 > next.committed - next.head
    {
        return None;
    }
    if !field(mem, registry, 0x1410734, &mut lock)
        || lock != 0
        || !field(mem, next.address, 0x20CD0, &mut writers)
        || writers != 0
    {
        return None;
    }
    Some(next)
}
/// detail::CurrentMember: the handle exactly once in the current bucket (its sequence in `slot`).
pub fn current_member(mem: Mem, epoch: &Epoch, handle: u32, slot: &mut u64) -> bool {
    *slot = u64::MAX;
    for i in 0..epoch.count as u64 {
        let sequence = epoch
            .committed
            .wrapping_sub(epoch.count as u64)
            .wrapping_add(i);
        let mut actual = 0u32;
        if !field(
            mem,
            epoch.address,
            8 + 4 * (sequence % RING_SIZE as u64),
            &mut actual,
        ) {
            return false;
        }
        if actual == handle {
            if *slot != u64::MAX {
                return false;
            }
            *slot = sequence;
        }
    }
    *slot != u64::MAX
}
/// detail::ReadSnapshot. Failures: 1 key, 2 epoch, 3 context, 4 native record, 5 refs, 6 control, 7 allocation, 8
/// allocator, 9 queue membership, 10 changed epoch, 11 native swapchain provenance. The snapshot read so far is returned
/// with a failure.
pub fn read_snapshot(mem: Mem, base: u64, key: &Key, source: bool) -> (Snapshot, u32) {
    let mut value = Snapshot {
        base,
        key: *key,
        ..Default::default()
    };
    if key.registry == 0
        || key.context == 0
        || key.list == 0
        || key.resource == 0
        || key.heap == 0
        || key.bytes == 0
        || key.bytes == u64::MAX
        || key.alignment == 0
        || key.handle == 0
        || key.handle > 32768
    {
        return (value, 1);
    }
    match read_epoch(mem, base, key.registry) {
        Some(epoch) => value.epoch = epoch,
        None => return (value, 2),
    }
    match read_swapchains(mem, key) {
        Some(swapchains) => value.swapchains = swapchains,
        None => return (value, 11),
    }
    let (mut list, mut kind) = (0u64, 0u32);
    if !field(mem, key.context, 0x30, &mut list)
        || list != key.list
        || !field(mem, key.context, 0x68, &mut kind)
        || (kind != 1 && kind != 2)
    {
        return (value, 3);
    }
    let offset = 0x2F1D8 + (key.handle as u64 - 1) * 0xB0;
    if key.registry > u64::MAX - offset - 0xB0 {
        return (value, 4);
    }
    value.record = key.registry + offset;
    let (mut resource, mut tag) = (0u64, 0u8);
    if !field(mem, value.record, 0, &mut resource)
        || resource != key.resource
        || !field(mem, value.record, 8, &mut tag)
        || tag != 2
        || !field(mem, value.record, 0x10, &mut value.owner)
        || value.owner == 0
    {
        return (value, 4);
    }
    if !read_into(mem, value.record - 8, &mut value.handle_references)
        || value.handle_references < 0
        || value.handle_references == i32::MAX
        || (source && value.handle_references == 0)
    {
        return (value, 5);
    }
    if !mem(value.owner, &mut value.control) {
        return (value, 6);
    }
    value.owner_references = u32_at(&value.control, 0) as i32;
    let allocation = u64_at(&value.control, 8);
    if value.owner_references <= 0
        || value.owner_references == i32::MAX
        || allocation == 0
        || u64_at(&value.control, 0x18) != key.alignment
    {
        return (value, 6);
    }
    value.allocation = read_allocation(mem, base, allocation);
    if value.allocation.observation != KNOWN
        || value.allocation.kind != 2
        || value.allocation.resource != 0
        || value.allocation.heap != key.heap
        || value.allocation.bytes < key.bytes
        || u64_at(&value.control, 0x20) != value.allocation.bytes
    {
        return (value, 7);
    }
    value.allocator = read_allocator(mem, base, key.registry, &value.allocation);
    if value.allocator.observation != KNOWN
        || !value.allocator.association_known
        || !value.allocator.pimpl_matches
    {
        return (value, 8);
    }
    if value.handle_references == 0
        && !current_member(mem, &value.epoch, key.handle, &mut value.queue_slot)
    {
        return (value, 9);
    }
    if read_epoch(mem, base, key.registry) != Some(value.epoch) {
        return (value, 10);
    }
    if read_swapchains(mem, key) != Some(value.swapchains) {
        return (value, 11);
    }
    (value, 0)
}
fn same_except_ref(a: &[u8], b: &[u8], at: usize) -> bool {
    a.iter()
        .zip(b)
        .enumerate()
        .all(|(i, (x, y))| (i >= at && i < at + 4) || x == y)
}
/// detail::SameIdentity: the same record, owner, swapchain table, unfenced bucket and allocation (reference counts
/// aside).
pub fn same_identity(a: &Snapshot, b: &Snapshot) -> bool {
    a.base == b.base
        && a.key == b.key
        && a.record == b.record
        && a.owner == b.owner
        && a.swapchains == b.swapchains
        && a.epoch.address == b.epoch.address
        && a.epoch.bucket == b.epoch.bucket
        && a.epoch.index == b.epoch.index
        && a.epoch.fence == 0
        && a.epoch.value == 0
        && b.epoch.fence == 0
        && b.epoch.value == 0
        && same_except_ref(&a.control, &b.control, 0)
        && a.allocation.address == b.allocation.address
        && same_except_ref(&a.allocation.raw, &b.allocation.raw, 8)
        && a.allocator.address == b.allocator.address
        && same_except_ref(&a.allocator.raw, &b.allocator.raw, 8)
}
fn raw<T: windows::core::Interface>(p: &Option<T>) -> *mut c_void {
    p.as_ref().map_or(core::ptr::null_mut(), |v| v.as_raw())
}
/// detail::SameBorrowedIdentity: both separately authenticated; cross-capture matching excludes unrelated mutable record
/// bytes.
pub fn same_borrowed_identity(a: &WritableIdentity, b: &WritableIdentity) -> bool {
    !raw(&a.creation.resource).is_null()
        && !raw(&a.creation.heap).is_null()
        && !a.recording.token.is_null()
        && same_creation_metadata(&a.creation, &b.creation)
        && a.native.registry == b.native.registry
        && a.native.resource == b.native.resource
        && a.native.handle == b.native.handle
        && a.native.reserved == b.native.reserved
        && a.recording.token == b.recording.token
        && a.recording.generation == b.recording.generation
        && a.recording.context == b.recording.context
        && a.recording.list == b.recording.list
        && a.recording.registry == b.recording.registry
        && a.original_context == b.original_context
        && a.expected_writable_state == b.expected_writable_state
        && same_resource_description(&a.resource_desc, &b.resource_desc)
        && same_heap_description(&a.heap_desc, &b.heap_desc)
        && same_heap_properties(&a.resource_heap_properties, &b.resource_heap_properties)
        && a.resource_heap_flags == b.resource_heap_flags
        && a.allocator_owner_observation == b.allocator_owner_observation
        && writable_record(&a.native_record) == writable_record(&b.native_record)
}

// ---- the runtime

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
    static DETAILS: Cell<[u64; 12]> = const { Cell::new([0; 12]) };
    static HOOK_FAILURE: Cell<[u64; 3]> = const { Cell::new([0; 3]) };
}
fn details(update: impl FnOnce(&mut [u64; 12])) {
    let mut d = DETAILS.get();
    update(&mut d);
    DETAILS.set(d);
}
fn stage(value: u32) {
    FAILURE.set(value);
    details(|d| d[0] = value as u64);
}
fn reset() {
    let mut d = [0u64; 12];
    d[11] = u64::MAX;
    DETAILS.set(d);
}
/// The runtime's reader: this process's memory, non-empty reads at non-null addresses.
fn memory(at: u64, out: &mut [u8]) -> bool {
    at != 0 && !out.is_empty() && at <= u64::MAX - out.len() as u64 && read(at, out)
}
type ValidateIdentity = unsafe extern "C" fn(*const c_void, *mut c_void) -> bool;
static VALIDATE_IDENTITY: AtomicPtr<c_void> = AtomicPtr::new(core::ptr::null_mut());
/// # Safety
/// `validate` is texture_read::ValidateWritableIdentity over a C++ WritableTargetIdentity (its private copy included).
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_attach(validate: ValidateIdentity) {
    VALIDATE_IDENTITY.store(validate as *mut c_void, Release);
}
/// texture_read::ValidateWritableIdentity of the C++ identity behind `identity`.
fn validate_identity(identity: *const WritableIdentity, queue: *mut c_void) -> bool {
    let f = VALIDATE_IDENTITY.load(Acquire);
    if !f.is_null() {
        // SAFETY: the attached function, over the caller's C++ record.
        return unsafe { core::mem::transmute::<*mut c_void, ValidateIdentity>(f)(identity.cast(), queue) };
    }
    #[cfg(all(feature = "rust-build", not(test)))]
    {
        // The plugin's (formerly NativeTargetLifetime.cpp's attached texture_read::ValidateWritableIdentity): the record
        // in the C++ layout -- the 368-byte identity, then its data_ whose first word is the authenticated copy (null
        // when none) -- checked against that copy.
        if identity.is_null() {
            return false;
        }
        // SAFETY: a C++-layout identity: its data_ follows the identity.
        let captured = unsafe { *identity.cast::<u8>().add(core::mem::size_of::<WritableIdentity>()).cast::<*const WritableIdentity>() };
        // SAFETY: the identity and its copy (or null), borrowed for the call.
        return unsafe { crate::texture_writable::cvr_tr_validate_writable_identity(identity, captured, queue) };
    }
    #[allow(unreachable_code)]
    false
}
/// The contract: native_recording's and the texture reads', and 46 audited engine functions.
fn contract() -> (bool, [u64; 3]) {
    static RESULT: OnceLock<(bool, [u64; 3])> = OnceLock::new();
    *RESULT.get_or_init(|| {
        const CONTRACT: [(u64, usize, u64); 46] = [
            (0x3E3248, 145, 0x61B5A10A99C316E9),
            (0xBF4624, 175, 0xFAA8CC8CC5084187),
            (0x3E9860, 185, 0x0990890D546C54BD),
            (0x3E98FC, 242, 0x073F687FA3D6BF90),
            (0xABDA80, 624, 0x74155FEF74957E23),
            (0x5B1938, 195, 0x773420FC3024FE47),
            (0x3E6CD4, 38, 0xB56EF3B145CCE822),
            (0x34BE60, 200, 0xFCD41E9D27E316E6),
            (0x2458EEC, 13, 0x8F0FFB5666BF3724),
            (0x34C16C, 438, 0xF20D07F3C00D184F),
            (0x34C3C0, 806, 0x9667FCCC34D1C033),
            (0x34C39C, 36, 0x41112018CADE440D),
            (0x34C7A0, 97, 0xB4D21C2B77A7C288),
            (0x10A226C, 80, 0xD094737CE3FCCDD7),
            (0x34B588, 346, 0x6E1062B700B72556),
            (0x34AC7C, 682, 0x920B1746CE34F233),
            (0x689FFC, 24, 0x20DF5B92407C73E4),
            (0x34AF20, 35, 0xAFAD89E225DAE8ED),
            (0x349F5C, 38, 0xD8310396C182F071),
            (0x275084, 41, 0x86F34059C9A7E048),
            (0x344408, 55, 0x4CDBE58204F3CD9F),
            (0x347EEC, 133, 0xDFEDCE894319584E),
            (0x2FA5EE0, 135, 0x38FCAFE70967B57A),
            (0x688141, 1037, 0xFA1423D5B68EE457),
            (0x346F24, 771, 0x51BA1810B87B452E),
            (0x5B4028, 229, 0x4BC3EFD38EC58363),
            (0x2FAAF80, 13, 0x4B71F66010D54C4D),
            (0x348220, 33, 0xEFE63006A4D426CB),
            (0x348244, 195, 0x8DFB4CD8294CBA43),
            (0x3483A4, 33, 0x16D17AF39F50AAA4),
            (0x18F3FB4, 105, 0x8550A7D22883C599),
            (0x18F3F48, 52, 0x95650F6E1318A268),
            (0x2FAC534, 57, 0x2BA6EF10E4DE61C9),
            (0x3E79C8, 333, 0x589B9E0C5E74800F),
            (0x22EE470, 0xF9, 0x0000000000000000),
            (0x18F6DD0, 114, 0xCEA9048EE2A9A579),
            (0x688788, 1302, 0x55625DB1357B5061),
            (0x115CDA8, 160, 0x6E2F847D51A0038E),
            (0x115CC6C, 282, 0x4F2C2E002AAAEF4F),
            (0x5B1CC8, 160, 0xE1422569D46719B7),
            (0x115CE51, 107, 0x9AC8230DC6E39765),
            (0x5D9850, 187, 0x62AC08357DBAEDC4),
            (0x22ECB90, 392, 0x6026D355662DB891),
            (0x22ECD20, 177, 0x9521C42D0BB82F9B),
            (0x115CD88, 30, 0xC61C709CD0D405E6),
            (0x115CE40, 9, 0x83A0AB8395F967AF),
        ];
        if !cvr_native_recording_compatible() {
            return (
                false,
                [
                    0,
                    1,
                    crate::native_recording::cvr_native_recording_last_failure() as u64,
                ],
            );
        }
        if !crate::texture_lease::compatible() {
            return (false, [1, 1, crate::texture_lease::cvr_tr_failure() as u64]);
        }
        let base = game();
        let mut bytes = [0u8; 2048];
        for (rva, len, hash) in CONTRACT {
            if base == 0
                || base > u64::MAX - rva
                || len > bytes.len()
                || !memory(base + rva, &mut bytes[..len])
            {
                return (false, [rva, hash, 0]);
            }
            let actual = fingerprint(&bytes[..len]);
            if actual != hash {
                return (false, [rva, hash, actual]);
            }
        }
        (true, [0; 3])
    })
}
/// Compatible: the contract, and the resize / shutdown entries original or the audited CET chain.
fn compatible() -> bool {
    let (accepted, failed) = contract();
    if !accepted {
        details(|d| d[1..4].copy_from_slice(&failed));
        return false;
    }
    let base = game();
    // SAFETY: a module lookup by name.
    let cet = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleHandleW(windows::core::w!(
            "cyber_engine_tweaks.asi"
        ))
    }
    .map_or(0, |m| m.0 as u64);
    let reject = |rva: u64, chain: &ResizeChain, rejected: u32| {
        let hook = [rva, 0, rejected as u64];
        HOOK_FAILURE.set(hook);
        details(|d| {
            d[1..4].copy_from_slice(&hook);
            d[4] = chain.relay;
            d[5] = chain.cet;
            d[6] = chain.singleton;
            d[7] = chain.trampoline;
        });
        false
    };
    let (chain, rejected) = read_hook_chain(&memory, base, cet, &RESIZE_HOOK);
    if rejected != 0 {
        return reject(RESIZE_RVA, &chain, rejected);
    }
    let (chain, rejected) = read_hook_chain(&memory, base, cet, &SHUTDOWN_HOOK);
    if rejected != 0 {
        return reject(SHUTDOWN_RVA, &chain, rejected);
    }
    HOOK_FAILURE.set([0; 3]);
    true
}
fn observed(value: &Snapshot) {
    details(|d| {
        d[2] = value.record;
        d[3] = value.owner;
        d[4] = value.allocation.address;
        d[5] = value.allocation.heap;
        d[6] = value.handle_references as u32 as u64;
        d[7] = value.owner_references as u32 as u64;
        d[8] = value.epoch.bucket;
        d[9] = value.epoch.index as u64;
        d[10] = value.epoch.count as u64;
        d[11] = value.queue_slot;
    });
}
/// Read: the snapshot twice, the same identity both times.
fn read_twice(key: &Key, source: bool) -> Option<Snapshot> {
    stage(5);
    let (first, failure) = read_snapshot(&memory, game(), key, source);
    if failure != 0 {
        details(|d| d[1] = failure as u64);
        observed(&first);
        return None;
    }
    observed(&first);
    let (second, failure) = read_snapshot(&memory, game(), key, source);
    if failure != 0 {
        details(|d| d[1] = failure as u64);
        observed(&second);
        return None;
    }
    observed(&second);
    stage(6);
    same_identity(&first, &second).then_some(second)
}
/// The C++ proofs as Rust sees them (all borrowed from the C++ records).
struct Borrow<'a> {
    identity: *const WritableIdentity,
    identity_view: &'a WritableIdentity,
    source: &'a Snapshot,
    scope: *mut c_void,
    bound: &'a Snapshot,
}
/// Live: the scope still valid, the captured identity still authenticated, the lifetime snapshot unchanged.
fn live(b: &Borrow) -> bool {
    stage(2);
    if !compatible() {
        return false;
    }
    stage(8);
    if !crate::queue_residency::validate(b.scope) {
        return false;
    }
    stage(3);
    if !validate_identity(b.identity, core::ptr::null_mut()) {
        return false;
    }
    let Some(now) = read_twice(&b.source.key, false) else {
        return false;
    };
    stage(6);
    if !same_identity(b.source, &now) || !same_identity(b.bound, &now) {
        return false;
    }
    stage(8);
    crate::queue_residency::validate(b.scope)
}
fn recording_context(c: &crate::texture_lease::UploadContext) -> RecordingContext {
    RecordingContext {
        context: c.context,
        state: c.state,
        list: c.list,
    }
}
/// Capture: the original draw's authenticated placed target and the snapshot of its native lifetime.
fn capture(identity_ptr: *const WritableIdentity, out: &mut Snapshot) -> bool {
    reset();
    stage(1);
    // SAFETY: a valid C++ identity (borrowed).
    let identity = unsafe { &*identity_ptr };
    let c = &identity.creation;
    let context = &identity.original_context;
    if raw(&c.resource).is_null()
        || raw(&c.heap).is_null()
        || c.kind != 2
        || c.offset != 0
        || c.bytes == 0
        || c.bytes == u64::MAX
        || identity.recording.token.is_null()
        || identity.native.resource != raw(&c.resource) as u64
        || identity.native.registry != identity.recording.registry
        || context.context == 0
        || context.state == 0
        || context.list == 0
    {
        return false;
    }
    stage(2);
    if !compatible() {
        return false;
    }
    stage(3);
    if !validate_identity(identity_ptr, core::ptr::null_mut()) {
        return false;
    }
    let list = identity.recording.list as *mut c_void;
    let native_context = recording_context(context);
    // SAFETY: valid records.
    let recorded = || unsafe {
        cvr_native_recording_validate_recording(&identity.recording, list, &native_context)
    };
    stage(4);
    if !recorded() {
        return false;
    }
    let key = Key {
        registry: identity.native.registry,
        context: identity.recording.context,
        list: identity.recording.list,
        resource: identity.native.resource,
        heap: raw(&c.heap) as u64,
        bytes: c.bytes,
        alignment: if identity.resource_desc.SampleDesc.Count > 1 {
            0x400000
        } else {
            0x10000
        },
        handle: identity.native.handle,
    };
    let Some(source) = read_twice(&key, true) else {
        return false;
    };
    if source.owner != identity.allocator_owner_observation {
        stage(6);
        return false;
    }
    stage(4);
    if !recorded() {
        return false;
    }
    stage(3);
    if !validate_identity(identity_ptr, core::ptr::null_mut()) {
        return false;
    }
    *out = source;
    stage(0);
    true
}
/// Bind: the captured lifetime borrowed through the scope of the native invocation submitting its pending recording.
fn bind(
    identity_ptr: *const WritableIdentity,
    source: Option<&Snapshot>,
    queue: *mut c_void,
    scope: *mut c_void,
    out: &mut Snapshot,
) -> bool {
    reset();
    stage(1);
    let Some(source) = source.filter(|_| !identity_ptr.is_null()) else {
        return false;
    };
    if queue.is_null() || scope.is_null() {
        return false;
    }
    // SAFETY: a valid C++ identity (borrowed).
    let identity = unsafe { &*identity_ptr };
    stage(2);
    if !compatible() {
        return false;
    }
    stage(8);
    if !crate::queue_residency::validate(scope) {
        return false;
    }
    // SAFETY: valid records.
    let submitted =
        || unsafe { cvr_native_recording_validate_submission(&identity.recording, queue) };
    stage(7);
    if !submitted() {
        return false;
    }
    stage(8);
    if !crate::queue_residency::contains_recording_runtime(scope, Some(&identity.recording)) {
        return false;
    }
    stage(3);
    if !validate_identity(identity_ptr, queue) {
        return false;
    }
    let Some(bound) = read_twice(&source.key, false) else {
        return false;
    };
    stage(6);
    if !same_identity(source, &bound) {
        return false;
    }
    stage(7);
    if !submitted() {
        return false;
    }
    let b = Borrow {
        identity: identity_ptr,
        identity_view: identity,
        source,
        scope,
        bound: &bound,
    };
    if !live(&b) {
        return false;
    }
    *out = bound;
    stage(0);
    true
}
fn borrow<'a>(
    identity: *const WritableIdentity,
    source: *const Snapshot,
    scope: *mut c_void,
    bound: *const Snapshot,
) -> Option<Borrow<'a>> {
    if identity.is_null() || source.is_null() || bound.is_null() {
        return None;
    }
    // SAFETY: the C++ proof's records, valid while the caller holds it.
    Some(Borrow {
        identity,
        identity_view: unsafe { &*identity },
        source: unsafe { &*source },
        scope,
        bound: unsafe { &*bound },
    })
}
fn validate(b: Option<Borrow>) -> bool {
    reset();
    stage(1);
    let Some(b) = b else {
        return false;
    };
    if !live(&b) {
        return false;
    }
    stage(0);
    true
}
/// GetBorrowedPageable: the exact authenticated placed heap of the borrowed target, its identity the expected one.
fn borrowed_pageable(
    device_ptr: *mut c_void,
    expected_ptr: *const WritableIdentity,
    b: Option<Borrow>,
    out: &mut *mut c_void,
) -> bool {
    reset();
    stage(1);
    // SAFETY: a live device of the caller's, or null; a valid expected identity.
    let device = unsafe { ID3D12Device::from_raw_borrowed(&device_ptr) };
    let expected = unsafe { &*expected_ptr };
    let (Some(device), Some(b)) = (device, b) else {
        return false;
    };
    if raw(&expected.creation.resource).is_null()
        || raw(&expected.creation.heap).is_null()
        || expected.recording.token.is_null()
    {
        return false;
    }
    stage(3);
    if !validate_identity(expected_ptr, core::ptr::null_mut()) || !live(&b) {
        return false;
    }
    let captured = b.identity_view;
    stage(10);
    if !same_borrowed_identity(expected, captured) {
        return false;
    }
    stage(11);
    let creation = &captured.creation;
    let (Some(resource), Some(heap)) = (creation.resource.as_ref(), creation.heap.as_ref()) else {
        return false;
    };
    // SAFETY (rest of the function): live objects.
    if unsafe { device.GetNodeCount() } != 1
        || creation.kind != 2
        || creation.offset != 0
        || creation.bytes == 0
        || creation.bytes == u64::MAX
    {
        return false;
    }
    if device_of(resource).map(|d| d.as_raw()) != Some(device_ptr)
        || device_of(heap).map(|d| d.as_raw()) != Some(device_ptr)
        || !same_resource_description(&captured.resource_desc, &unsafe { resource.GetDesc() })
        || !same_heap_description(&captured.heap_desc, &unsafe { heap.GetDesc() })
    {
        return false;
    }
    let needed = unsafe { device.GetResourceAllocationInfo(0, &[captured.resource_desc]) };
    if needed.SizeInBytes == 0
        || needed.SizeInBytes == u64::MAX
        || needed.Alignment == 0
        || needed.SizeInBytes > creation.bytes
        || creation.bytes > captured.heap_desc.SizeInBytes
    {
        return false;
    }
    let Ok(pageable) = heap.cast::<ID3D12Pageable>() else {
        return false;
    };
    stage(3);
    if !validate_identity(expected_ptr, core::ptr::null_mut()) || !live(&b) {
        return false;
    }
    *out = pageable.into_raw();
    stage(0);
    true
}

// ---- the C ABI: the header's readers and the module's runtime (src/Render/NativeTargetLifetime.cpp in the Rust
// build)

fn put<T>(out: *mut T, value: T) {
    if !out.is_null() {
        // SAFETY: the caller's slot.
        unsafe { *out = value };
    }
}
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_relative_target(
    next: u64,
    displacement: i32,
    out: *mut u64,
) -> bool {
    let value = relative_target(next, displacement);
    put(out, value.unwrap_or(0));
    value.is_some()
}
/// # Safety
/// `out` valid; `failure` valid or null; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_read_hook_chain(
    read_fn: Option<ReadMemory>,
    user: *mut c_void,
    base: u64,
    cet: u64,
    shutdown: bool,
    out: *mut ResizeChain,
    failure: *mut u32,
) -> bool {
    let spec = if shutdown {
        &SHUTDOWN_HOOK
    } else {
        &RESIZE_HOOK
    };
    let (chain, rejected) = read_hook_chain(&reader(read_fn, user), base, cet, spec);
    put(out, chain);
    put(failure, rejected);
    rejected == 0
}
/// # Safety
/// `key` and `out` valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_read_swapchains(
    read_fn: Option<ReadMemory>,
    user: *mut c_void,
    key: *const Key,
    out: *mut Swapchains,
) -> bool {
    // SAFETY: as the caller promises.
    let value = read_swapchains(&reader(read_fn, user), unsafe { &*key });
    put(out, value.unwrap_or_default());
    value.is_some()
}
/// # Safety
/// `out` valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_read_epoch(
    read_fn: Option<ReadMemory>,
    user: *mut c_void,
    base: u64,
    registry: u64,
    out: *mut Epoch,
) -> bool {
    let value = read_epoch(&reader(read_fn, user), base, registry);
    put(out, value.unwrap_or_default());
    value.is_some()
}
/// # Safety
/// `epoch` and `slot` valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_current_member(
    read_fn: Option<ReadMemory>,
    user: *mut c_void,
    epoch: *const Epoch,
    handle: u32,
    slot: *mut u64,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe { current_member(&reader(read_fn, user), &*epoch, handle, &mut *slot) }
}
/// # Safety
/// `key` and `out` valid; `failure` valid or null; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_read_snapshot(
    read_fn: Option<ReadMemory>,
    user: *mut c_void,
    base: u64,
    key: *const Key,
    source: bool,
    out: *mut Snapshot,
    failure: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    let (value, code) = read_snapshot(&reader(read_fn, user), base, unsafe { &*key }, source);
    put(out, value);
    put(failure, code);
    code == 0
}
/// # Safety
/// Valid snapshots.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_same_identity(a: *const Snapshot, b: *const Snapshot) -> bool {
    // SAFETY: as the caller promises.
    same_identity(unsafe { &*a }, unsafe { &*b })
}
/// # Safety
/// Valid C++ identities (borrowed).
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_same_borrowed_identity(
    a: *const WritableIdentity,
    b: *const WritableIdentity,
) -> bool {
    // SAFETY: as the caller promises.
    same_borrowed_identity(unsafe { &*a }, unsafe { &*b })
}
#[no_mangle]
pub extern "C" fn cvr_tl_compatible() -> bool {
    compatible()
}
/// # Safety
/// `identity` a valid C++ identity; `source` the caller's snapshot record.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_capture(
    identity: *const WritableIdentity,
    source: *mut Snapshot,
) -> bool {
    // SAFETY: as the caller promises.
    capture(identity, unsafe { &mut *source })
}
/// # Safety
/// `identity` and `source` the captured proof's (or null); `queue` live or null; `scope` a queue residency handle or
/// null; `bound` the caller's snapshot record.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_bind(
    identity: *const WritableIdentity,
    source: *const Snapshot,
    queue: *mut c_void,
    scope: *mut c_void,
    bound: *mut Snapshot,
) -> bool {
    // SAFETY: as the caller promises.
    bind(identity, unsafe { source.as_ref() }, queue, scope, unsafe {
        &mut *bound
    })
}
/// # Safety
/// The borrowed proof's records (or nulls).
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_validate(
    identity: *const WritableIdentity,
    source: *const Snapshot,
    scope: *mut c_void,
    bound: *const Snapshot,
) -> bool {
    validate(borrow(identity, source, scope, bound))
}
/// # Safety
/// `device` live or null; `expected` a valid C++ identity; the borrowed proof's records (or nulls); `out` a null ComPtr.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_borrowed_pageable(
    device: *mut c_void,
    expected: *const WritableIdentity,
    identity: *const WritableIdentity,
    source: *const Snapshot,
    scope: *mut c_void,
    bound: *const Snapshot,
    out: *mut *mut c_void,
) -> bool {
    // SAFETY: as the caller promises.
    borrowed_pageable(
        device,
        expected,
        borrow(identity, source, scope, bound),
        unsafe { &mut *out },
    )
}
#[no_mangle]
pub extern "C" fn cvr_tl_last_failure() -> u32 {
    FAILURE.get()
}
/// # Safety
/// `out` receives 12 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_last_details(out: *mut [u64; 12]) {
    // SAFETY: as the caller promises.
    unsafe { *out = DETAILS.get() };
}
/// LastContractFailure: the contract's failure, else the hook chains'.
///
/// # Safety
/// `out` receives 3 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_tl_last_contract_failure(out: *mut [u64; 3]) {
    let (accepted, failed) = contract();
    // SAFETY: as the caller promises.
    unsafe { *out = if accepted { HOOK_FAILURE.get() } else { failed } };
}
