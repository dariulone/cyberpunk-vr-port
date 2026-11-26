//! The scene update's source receipt (formerly include/Render/NativeSceneUpdate.hpp, scene_update::Receipt; the header
//! keeps it for the reference build and wraps this one under `CVR_RUST_VRIK`): an observational receipt of one native
//! scene-update producer -- the queues it registers (each payload's prefix immutable from registration to entry, the
//! membership queue's scenes and their update operations captured, its job group pair held by a lease), their entry,
//! return or cancellation, the generic runners' command pages (an input inventory, not execute observations) and the
//! bounds writes attributed to a callback. The held groups prove only this captured aggregate's closure; unknown
//! descendant queues and virtual command semantics stay explicit. The first rejection is kept.

use crate::native::job_lease::{JobLeaseApi, NativeJobLease};
use core::ffi::c_void;

pub const MAX_QUEUES: usize = 128;
pub const MAX_SCENES: u32 = 4096;
pub const MAX_COMMANDS: u32 = 16384;
/// Read: (address, out, bytes) -> every byte read.
pub type Read = Option<unsafe extern "C" fn(u64, *mut c_void, usize) -> bool>;

/// scene_update::Kind.
pub mod kind {
    pub const GENERAL: u32 = 0;
    pub const PRE: u32 = 1;
    pub const MEMBERSHIP: u32 = 2;
    pub const POST: u32 = 3;
    pub const OPAQUE: u32 = 4;
}
/// scene_update::Failure.
pub mod failure {
    pub const NONE: u32 = 0;
    pub const ARGUMENTS: u32 = 1;
    pub const LIMIT: u32 = 2;
    pub const GROUP: u32 = 3;
    pub const PAYLOAD: u32 = 4;
    pub const CHANGED: u32 = 5;
    pub const BINDING: u32 = 6;
    pub const INVENTORY: u32 = 7;
    pub const OVERFLOW: u32 = 8;
    pub const SCOPE: u32 = 9;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub nonce: u64,
    pub manager: u64,
    pub caller: u64,
    pub thread: u32,
    pub reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueueBinding {
    pub kind: u32,
    pub ordinal: u32,
    pub caller: u64,
    pub callback: u64,
    pub payload: u64,
    pub prerequisite: u64,
    pub completion: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneInput {
    pub scene: u64,
    pub pool: u64,
    pub queue: u32,
    pub updates: u32,
    pub operations: [u32; 16],
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandInput {
    pub page: u64,
    pub command: u64,
    pub vtable: u64,
    pub execute: u64,
    pub destroy: u64,
    pub queue: u32,
    pub ordinal: u32,
    pub offset: u32,
    pub bytes: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MutationEntry {
    pub pool: u64,
    pub indices: u64,
    pub caller: u64,
    pub queue: u32,
    pub thread: u32,
}
/// One registered queue (the lease Rust's, owned by the receipt).
#[repr(C)]
pub struct QueueEntry {
    pub binding: QueueBinding,
    pub prefix: [u8; 56],
    pub entered: bool,
    pub returned: bool,
    pub inventory: bool,
    pub cancelled: bool,
    pub lease: *mut NativeJobLease,
}
const _: () = assert!(
    core::mem::size_of::<Identity>() == 32
        && core::mem::size_of::<QueueBinding>() == 48
        && core::mem::size_of::<SceneInput>() == 88
        && core::mem::size_of::<CommandInput>() == 56
        && core::mem::size_of::<MutationEntry>() == 32
        && core::mem::size_of::<QueueEntry>() == 120
        && core::mem::offset_of!(QueueEntry, lease) == 112
);

fn field_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}
fn field_u32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn memory(read: Read, address: u64, out: &mut [u8]) -> bool {
    // SAFETY: the reader writes at most `out.len()` bytes.
    read.is_some_and(|r| address != 0 && address <= u64::MAX - out.len() as u64 && unsafe { r(address, out.as_mut_ptr().cast(), out.len()) })
}
fn memory_at(read: Read, base: u64, offset: u64, out: &mut [u8]) -> bool {
    base != 0 && base <= u64::MAX - offset && memory(read, base + offset, out)
}
fn word<const N: usize>(read: Read, base: u64, offset: u64) -> Option<[u8; N]> {
    let mut bytes = [0u8; N];
    memory_at(read, base, offset, &mut bytes).then_some(bytes)
}
/// Group: an aligned job group with its pending counter not negative and its references positive.
fn group(address: u64, read: Read) -> bool {
    let mut values = [0u8; 8];
    address != 0
        && address & 7 == 0
        && address <= u64::MAX - 0x20
        && memory(read, address + 0x18, &mut values)
        && i32::from_le_bytes(values[0..4].try_into().unwrap()) >= 0
        && i32::from_le_bytes(values[4..8].try_into().unwrap()) > 0
}

pub struct Receipt {
    identity: Identity,
    api: JobLeaseApi,
    failure: u32,
    producer_closed: bool,
    producer_cancelled: bool,
    queues: Vec<QueueEntry>,
    scenes: Vec<SceneInput>,
    commands: Vec<CommandInput>,
    mutations: Vec<MutationEntry>,
}
impl Drop for Receipt {
    fn drop(&mut self) {
        for row in &mut self.queues {
            if !row.lease.is_null() {
                // SAFETY: the receipt's own boxed lease.
                drop(unsafe { Box::from_raw(row.lease) });
                row.lease = core::ptr::null_mut();
            }
        }
    }
}
impl Receipt {
    pub fn new(identity: Identity, api: JobLeaseApi) -> Self {
        Receipt { identity, api, failure: failure::NONE, producer_closed: false, producer_cancelled: false, queues: Vec::with_capacity(MAX_QUEUES), scenes: Vec::new(), commands: Vec::new(), mutations: Vec::new() }
    }
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn rejection(&self) -> u32 {
        self.failure
    }
    pub fn reject(&mut self, value: u32) {
        if self.failure == failure::NONE {
            self.failure = value;
        }
    }
    pub fn queues(&self) -> &[QueueEntry] {
        &self.queues
    }
    pub fn scenes(&self) -> &[SceneInput] {
        &self.scenes
    }
    pub fn commands(&self) -> &[CommandInput] {
        &self.commands
    }
    pub fn mutations(&self) -> &[MutationEntry] {
        &self.mutations
    }
    pub fn producer_closed(&self) -> bool {
        self.producer_closed
    }
    pub fn producer_cancelled(&self) -> bool {
        self.producer_cancelled
    }
    /// Register: a queue the producer (or, opaque, a callback) admits -- its payload's prefix (16 bytes, 56 for the
    /// membership queue, naming the manager) read, the membership queue's scenes captured, its groups held.
    pub fn register(&mut self, binding: QueueBinding, read: Read) -> Option<u32> {
        if ((self.producer_closed || self.producer_cancelled) && binding.kind != kind::OPAQUE)
            || binding.kind > kind::OPAQUE
            || self.identity.nonce == 0
            || self.identity.manager == 0
            || read.is_none()
            || binding.payload == 0
            || binding.callback == 0
        {
            self.reject(failure::ARGUMENTS);
            return None;
        }
        if self.queues.len() >= MAX_QUEUES {
            self.reject(failure::LIMIT);
            return None;
        }
        if self.queues.iter().any(|row| !row.entered && row.binding.payload == binding.payload) {
            self.reject(failure::BINDING);
            return None;
        }
        if (binding.prerequisite != 0 && !group(binding.prerequisite, read)) || !group(binding.completion, read) {
            self.reject(failure::GROUP);
            return None;
        }
        let ordinal = self.queues.len() as u32;
        let mut row = QueueEntry { binding: QueueBinding { ordinal, ..binding }, prefix: [0; 56], entered: false, returned: false, inventory: false, cancelled: false, lease: core::ptr::null_mut() };
        let scenes = self.scenes.len();
        if binding.kind != kind::OPAQUE {
            let bytes = if binding.kind == kind::MEMBERSHIP { 56 } else { 16 };
            if !memory(read, binding.payload, &mut row.prefix[..bytes]) || field_u64(&row.prefix, 0) != self.identity.manager {
                self.reject(failure::PAYLOAD);
                return None;
            }
            if binding.kind == kind::MEMBERSHIP && !self.capture_scenes(&row, read) {
                self.scenes.truncate(scenes);
                return None;
            }
        }
        // 14299C permits a null prerequisite: only the actual terminal is retained then; the binding keeps the null.
        let mut lease = Box::new(NativeJobLease::new(self.api));
        let begin = if binding.prerequisite != 0 { binding.prerequisite } else { binding.completion };
        // SAFETY: the engine's job groups the queue names, checked above.
        if !unsafe { lease.capture(begin as *mut c_void, binding.completion as *mut c_void) } {
            self.reject(failure::GROUP);
            self.scenes.truncate(scenes);
            return None;
        }
        row.lease = Box::into_raw(lease);
        self.queues.push(row);
        Some(ordinal)
    }
    /// Enter: the callback of a registered queue entered with the payload's prefix unchanged.
    pub fn enter(&mut self, ordinal: u32, kind: u32, payload: u64, completion: u64, read: Read) -> bool {
        let Some(row) = self.queues.get(ordinal as usize) else {
            self.reject(failure::BINDING);
            return false;
        };
        if row.entered || row.binding.kind != kind || row.binding.payload != payload || row.binding.completion != completion || kind == kind::OPAQUE {
            self.reject(failure::BINDING);
            return false;
        }
        let bytes = if kind == kind::MEMBERSHIP { 56 } else { 16 };
        let mut now = [0u8; 56];
        if !memory(read, payload, &mut now[..bytes]) || now[..bytes] != row.prefix[..bytes] {
            self.reject(failure::CHANGED);
            return false;
        }
        self.queues[ordinal as usize].entered = true;
        true
    }
    pub fn returned(&mut self, ordinal: u32) {
        match self.queues.get_mut(ordinal as usize) {
            Some(row) if row.entered && !row.returned && !row.cancelled => row.returned = true,
            _ => self.reject(failure::BINDING),
        }
    }
    pub fn close_producer(&mut self) {
        if self.producer_closed {
            self.reject(failure::SCOPE);
        }
        self.producer_closed = true;
    }
    pub fn cancel_producer(&mut self) {
        self.reject(failure::SCOPE);
        self.producer_cancelled = true;
    }
    pub fn cancel_callback(&mut self, ordinal: u32) {
        self.reject(failure::SCOPE);
        if let Some(row) = self.queues.get_mut(ordinal as usize) {
            if row.entered && !row.returned {
                row.cancelled = true;
            }
        }
    }
    /// GroupsClosed: the producer closed or cancelled, every known queue returned or cancelled (or never entered under
    /// a cancelled producer), and every held group complete. Cancellation never releases held groups while a pending
    /// counter remains.
    pub fn groups_closed(&self) -> bool {
        if !self.producer_closed && !self.producer_cancelled {
            return false;
        }
        self.queues.iter().all(|row| {
            let settled = row.binding.kind == kind::OPAQUE || row.returned || row.cancelled || (self.producer_cancelled && !row.entered);
            // SAFETY: the receipt's own lease.
            settled && !row.lease.is_null() && unsafe { (*row.lease).complete() }
        })
    }
    pub fn opaque_queues(&self) -> u32 {
        self.queues.iter().filter(|row| row.binding.kind == kind::OPAQUE).count() as u32
    }
    /// Inventory: a generic runner's detached command pages (the chain of the queue's payload) while the queue's
    /// callback runs -- each command's vtable with its execute and destroy entries; pages bounded, no cycle.
    pub fn inventory(&mut self, ordinal: u32, manager: u64, chain: u64, lane: u32, read: Read) -> bool {
        let Some(row) = self.queues.get(ordinal as usize) else {
            self.reject(failure::BINDING);
            return false;
        };
        let k = row.binding.kind;
        if !row.entered
            || row.returned
            || row.inventory
            || manager != self.identity.manager
            || chain != field_u64(&row.prefix, 8)
            || !matches!(k, kind::GENERAL | kind::PRE | kind::POST)
            || (k == kind::PRE && lane != 1)
            || (k == kind::POST && lane != 0)
        {
            self.reject(failure::BINDING);
            return false;
        }
        let capacity: u32 = if k == kind::GENERAL { 0x4000 } else { 0x800 };
        let mut seen = std::collections::BTreeSet::new();
        let mut pending: Vec<CommandInput> = Vec::new();
        let mut page = chain;
        while page != 0 {
            if seen.len() >= 4096 || !seen.insert(page) {
                self.reject(failure::INVENTORY);
                return false;
            }
            let (Some(count), Some(next)) = (word::<4>(read, page, u64::from(capacity)), word::<8>(read, page, u64::from(capacity) + 8)) else {
                self.reject(failure::INVENTORY);
                return false;
            };
            let (count, next) = (u32::from_le_bytes(count), u64::from_le_bytes(next));
            if count < 14 || count > capacity {
                self.reject(failure::INVENTORY);
                return false;
            }
            let mut offset = 14u32;
            while offset < count {
                let command = word::<2>(read, page, u64::from(offset)).map(i16::from_le_bytes).filter(|&bytes| bytes >= 10 && bytes as u32 <= count - offset).and_then(|bytes| {
                    let vtable = u64::from_le_bytes(word::<8>(read, page, u64::from(offset) + 2)?);
                    if vtable == 0 {
                        return None;
                    }
                    let mut destroy = [0u8; 8];
                    if !memory(read, vtable, &mut destroy) {
                        return None;
                    }
                    let execute = u64::from_le_bytes(word::<8>(read, vtable, 8)?);
                    let destroy = u64::from_le_bytes(destroy);
                    (execute != 0 && destroy != 0 && self.commands.len() + pending.len() < MAX_COMMANDS as usize).then_some((bytes as u32, vtable, execute, destroy))
                });
                let Some((bytes, vtable, execute, destroy)) = command else {
                    self.reject(failure::INVENTORY);
                    return false;
                };
                pending.push(CommandInput { page, command: page + u64::from(offset) + 2, vtable, execute, destroy, queue: ordinal, ordinal: pending.len() as u32, offset, bytes });
                offset += bytes;
            }
            page = next;
        }
        self.commands.extend(pending);
        self.queues[ordinal as usize].inventory = true;
        true
    }
    pub fn mutation(&mut self, ordinal: u32, pool: u64, indices: u64, caller: u64, thread: u32) {
        let live = self.queues.get(ordinal as usize).is_some_and(|row| row.entered && !row.returned && !row.cancelled);
        if !live || pool == 0 {
            self.reject(failure::BINDING);
            return;
        }
        if self.mutations.len() >= MAX_COMMANDS as usize {
            self.reject(failure::LIMIT);
            return;
        }
        self.mutations.push(MutationEntry { pool, indices, caller, queue: ordinal, thread });
    }
    /// CaptureScenes: the membership payload's scene list -- each slot's chain of nodes (no index twice, within the
    /// capacity), each node's scene and its updates counted by operation -- exactly `size` scenes.
    fn capture_scenes(&mut self, row: &QueueEntry, read: Read) -> bool {
        let p = &row.prefix;
        let (slots, nodes) = (field_u64(p, 8), field_u64(p, 24));
        let (size, slot_count, capacity, stride) = (field_u32(p, 16), field_u32(p, 20), field_u32(p, 32), field_u32(p, 36));
        if size > capacity || capacity > MAX_SCENES || slot_count > MAX_SCENES || stride > 4096 || (size != 0 && (slots == 0 || nodes == 0 || slot_count == 0 || stride < 32)) {
            self.reject(failure::PAYLOAD);
            return false;
        }
        if size == 0 {
            return true;
        }
        let mut heads = vec![0u8; slot_count as usize * 4];
        let mut seen = vec![false; capacity as usize];
        let mut pending: Vec<SceneInput> = Vec::new();
        if !memory(read, slots, &mut heads) {
            self.reject(failure::PAYLOAD);
            return false;
        }
        let mut updates = 0u32;
        for head in heads.chunks_exact(4) {
            let mut index = u32::from_le_bytes(head.try_into().unwrap());
            while index != u32::MAX {
                let at = u64::from(index) * u64::from(stride);
                if index >= capacity || seen[index as usize] || pending.len() >= size as usize || nodes > u64::MAX - at {
                    self.reject(failure::PAYLOAD);
                    return false;
                }
                seen[index as usize] = true;
                let mut node = [0u8; 32];
                if !memory(read, nodes + at, &mut node) {
                    self.reject(failure::PAYLOAD);
                    return false;
                }
                let (scene, data) = (field_u64(&node, 8), field_u64(&node, 16));
                let (count, limit) = (field_u32(&node, 28), field_u32(&node, 24));
                if scene == 0 || scene > u64::MAX - 0x7E0F0 || count > limit || count > MAX_COMMANDS - updates || (count != 0 && data == 0) {
                    self.reject(failure::PAYLOAD);
                    return false;
                }
                let mut input = SceneInput { scene, pool: scene + 0x7E0F0, queue: row.binding.ordinal, updates: count, operations: [0; 16] };
                for i in 0..u64::from(count) {
                    let mut update = [0u8; 16];
                    if data > u64::MAX - i * 16 || !memory(read, data + i * 16, &mut update) {
                        self.reject(failure::PAYLOAD);
                        return false;
                    }
                    input.operations[usize::from(update[12] & 15)] += 1;
                }
                updates += count;
                pending.push(input);
                index = field_u32(&node, 0);
            }
        }
        if pending.len() != size as usize || self.scenes.len() + pending.len() > MAX_SCENES as usize {
            self.reject(failure::PAYLOAD);
            return false;
        }
        self.scenes.extend(pending);
        true
    }
}

// ---- the C ABI (the header's Rust-build Receipt)

/// # Safety
/// `identity` and `api` are valid.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_new(identity: *const Identity, api: *const JobLeaseApi) -> *mut Receipt {
    Box::into_raw(Box::new(Receipt::new(unsafe { *identity }, unsafe { *api })))
}
/// # Safety
/// `receipt` is null or from cvr_sur_new, not used after.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_free(receipt: *mut Receipt) {
    if !receipt.is_null() {
        drop(unsafe { Box::from_raw(receipt) });
    }
}
/// # Safety
/// `receipt` is live; `binding` valid; `ordinal` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_register(receipt: *mut Receipt, binding: *const QueueBinding, read: Read, ordinal: *mut u32) -> bool {
    let index = unsafe { (*receipt).register(*binding, read) };
    unsafe { *ordinal = index.unwrap_or(u32::MAX) };
    index.is_some()
}
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_enter(receipt: *mut Receipt, ordinal: u32, kind: u32, payload: u64, completion: u64, read: Read) -> bool {
    unsafe { (*receipt).enter(ordinal, kind, payload, completion, read) }
}
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_inventory(receipt: *mut Receipt, ordinal: u32, manager: u64, chain: u64, lane: u32, read: Read) -> bool {
    unsafe { (*receipt).inventory(ordinal, manager, chain, lane, read) }
}
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_mutation(receipt: *mut Receipt, ordinal: u32, pool: u64, indices: u64, caller: u64, thread: u32) {
    unsafe { (*receipt).mutation(ordinal, pool, indices, caller, thread) }
}
/// The receipt's small steps: 0 the rejection, 1 reject `value`, 2 returned `value`, 3 close the producer, 4 producer
/// closed, 5 cancel the producer, 6 producer cancelled, 7 cancel callback `value`, 8 groups closed, 9 opaque queues.
///
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_step(receipt: *mut Receipt, step: u32, value: u32) -> u32 {
    let r = unsafe { &mut *receipt };
    match step {
        0 => r.rejection(),
        1 => {
            r.reject(value);
            0
        }
        2 => {
            r.returned(value);
            0
        }
        3 => {
            r.close_producer();
            0
        }
        4 => r.producer_closed() as u32,
        5 => {
            r.cancel_producer();
            0
        }
        6 => r.producer_cancelled() as u32,
        7 => {
            r.cancel_callback(value);
            0
        }
        8 => r.groups_closed() as u32,
        9 => r.opaque_queues(),
        _ => 0,
    }
}
/// The receipt's rows: 0 queues, 1 scenes, 2 commands, 3 mutations (`count` of them).
///
/// # Safety
/// `receipt` is live; `count` writable.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_rows(receipt: *const Receipt, which: u32, count: *mut usize) -> *const c_void {
    let r = unsafe { &*receipt };
    let (rows, n): (*const c_void, usize) = match which {
        0 => (r.queues().as_ptr().cast(), r.queues().len()),
        1 => (r.scenes().as_ptr().cast(), r.scenes().len()),
        2 => (r.commands().as_ptr().cast(), r.commands().len()),
        _ => (r.mutations().as_ptr().cast(), r.mutations().len()),
    };
    unsafe { *count = n };
    rows
}
/// # Safety
/// `receipt` is live.
#[no_mangle]
pub unsafe extern "C-unwind" fn cvr_sur_identity(receipt: *const Receipt) -> *const Identity {
    unsafe { &(*receipt).identity }
}

#[cfg(test)]
mod tests {
    //! A few of the reference test's cases (tools/single_pass_tests/scene_update_test runs all of them against this in
    //! its Rust variant): a queue's life to closure, an immutable payload, the membership scenes, an inventory.
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static MEMORY: RefCell<BTreeMap<u64, u8>> = const { RefCell::new(BTreeMap::new()) };
        static PENDING: RefCell<BTreeMap<u64, i32>> = const { RefCell::new(BTreeMap::new()) };
    }
    fn put(address: u64, bytes: &[u8]) {
        MEMORY.with(|m| {
            for (i, b) in bytes.iter().enumerate() {
                m.borrow_mut().insert(address + i as u64, *b);
            }
        });
    }
    unsafe extern "C" fn read(address: u64, out: *mut c_void, bytes: usize) -> bool {
        MEMORY.with(|m| {
            let m = m.borrow();
            (0..bytes as u64).all(|i| {
                m.get(&(address + i)).is_some_and(|b| {
                    unsafe { *out.cast::<u8>().add(i as usize) = *b };
                    true
                })
            })
        })
    }
    unsafe extern "C" fn retain(_: *mut c_void) {}
    unsafe extern "C" fn release(group: *mut *mut c_void) {
        unsafe { *group = core::ptr::null_mut() };
    }
    unsafe extern "C" fn done(group: *mut c_void) -> bool {
        PENDING.with(|p| p.borrow().get(&(group as u64)).copied().unwrap_or(0) == 0)
    }
    const MANAGER: u64 = 0x10000;
    const PAYLOAD: u64 = 0x20000;
    const BEGIN: u64 = 0x30000;
    const END: u64 = 0x30100;
    fn group_at(address: u64, work: i32) {
        put(address + 0x18, &work.to_le_bytes());
        put(address + 0x1C, &1i32.to_le_bytes());
        PENDING.with(|p| p.borrow_mut().insert(address, work));
    }
    fn fresh() -> Receipt {
        MEMORY.with(|m| m.borrow_mut().clear());
        group_at(BEGIN, 0);
        group_at(END, 0);
        put(PAYLOAD, &MANAGER.to_le_bytes());
        put(PAYLOAD + 8, &0x40000u64.to_le_bytes());
        Receipt::new(Identity { nonce: 1, manager: MANAGER, caller: 0x293B33, thread: 77, reserved: 0 }, JobLeaseApi { retain: Some(retain), release: Some(release), complete: Some(done) })
    }
    fn binding(k: u32) -> QueueBinding {
        QueueBinding { kind: k, ordinal: 0, caller: 0xC1A168, callback: 0xCA0E50, payload: PAYLOAD, prerequisite: BEGIN, completion: END }
    }

    #[test]
    fn queues_payloads_scenes_and_inventory() {
        let mut r = fresh();
        let first = r.register(binding(kind::GENERAL), Some(read)).unwrap();
        assert!(!r.groups_closed(), "the producer still open");
        group_at(END, 1);
        assert!(r.enter(first, kind::GENERAL, PAYLOAD, END, Some(read)));
        r.returned(first);
        r.close_producer();
        assert!(!r.groups_closed(), "a pending group");
        group_at(END, 0);
        assert!(r.groups_closed() && r.rejection() == failure::NONE);

        let mut r = fresh();
        let index = r.register(binding(kind::GENERAL), Some(read)).unwrap();
        put(PAYLOAD, &(MANAGER + 8).to_le_bytes());
        assert!(!r.enter(index, kind::GENERAL, PAYLOAD, END, Some(read)) && r.rejection() == failure::CHANGED && !r.queues()[0].entered, "a payload changed");

        let mut r = fresh();
        let mut prefix = [0u8; 56];
        prefix[0..8].copy_from_slice(&MANAGER.to_le_bytes());
        prefix[8..16].copy_from_slice(&0x70000u64.to_le_bytes());
        prefix[16..20].copy_from_slice(&2u32.to_le_bytes());
        prefix[20..24].copy_from_slice(&1u32.to_le_bytes());
        prefix[24..32].copy_from_slice(&0x71000u64.to_le_bytes());
        prefix[32..36].copy_from_slice(&2u32.to_le_bytes());
        prefix[36..40].copy_from_slice(&32u32.to_le_bytes());
        put(PAYLOAD, &prefix);
        put(0x70000, &0u32.to_le_bytes());
        for i in 0..2u64 {
            let mut node = [0u8; 32];
            node[0..4].copy_from_slice(&(if i == 1 { u32::MAX } else { 1 }).to_le_bytes());
            node[8..16].copy_from_slice(&(0x90000 + i * 0x100000).to_le_bytes());
            node[16..24].copy_from_slice(&(0x72000 + i * 0x100).to_le_bytes());
            node[24..28].copy_from_slice(&2u32.to_le_bytes());
            node[28..32].copy_from_slice(&2u32.to_le_bytes());
            put(0x71000 + i * 32, &node);
            for j in 0..2u64 {
                let mut update = [0u8; 16];
                update[12] = (j + 1 + i * 2) as u8;
                put(0x72000 + i * 0x100 + j * 16, &update);
            }
        }
        r.register(binding(kind::MEMBERSHIP), Some(read)).unwrap();
        let s = r.scenes();
        assert!(s.len() == 2 && s[0].scene == 0x90000 && s[1].scene == 0x190000 && s[0].pool == 0x90000 + 0x7E0F0 && s[0].operations[1] == 1 && s[1].operations[4] == 1);

        let mut r = fresh();
        let chain = 0x40000u64;
        let page_bytes = vec![0u8; 0x800 + 16];
        put(chain, &page_bytes);
        put(chain + 14, &16i16.to_le_bytes());
        put(chain + 16, &0x50000u64.to_le_bytes());
        put(chain + 30, &16i16.to_le_bytes());
        put(chain + 32, &0x50010u64.to_le_bytes());
        put(chain + 0x800, &46u32.to_le_bytes());
        put(chain + 0x808, &0u64.to_le_bytes());
        for (k, v) in [(0x50000u64, 0x61000u64), (0x50008, 0x62000), (0x50010, 0x63000), (0x50018, 0x64000)] {
            put(k, &v.to_le_bytes());
        }
        let index = r.register(binding(kind::PRE), Some(read)).unwrap();
        assert!(r.enter(index, kind::PRE, PAYLOAD, END, Some(read)) && r.inventory(index, MANAGER, chain, 1, Some(read)));
        let c = r.commands();
        assert!(c.len() == 2 && c[0].command == chain + 16 && c[0].execute == 0x62000 && c[1].destroy == 0x63000 && c[0].offset == 14 && c[1].offset == 30);
        assert!(!r.inventory(index, MANAGER, chain, 1, Some(read)) && r.rejection() == failure::BINDING, "one inventory per queue");
    }
}
