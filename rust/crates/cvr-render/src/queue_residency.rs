//! THE QUEUE RESIDENCY SCOPES (formerly src/Render/NativeQueueResidencyScope.cpp and the readers of
//! include/Render/NativeQueueResidencyScope.hpp): a synchronous permission inside the exact native ExecuteSubset
//! invocation. Inside our ExecuteCommandLists observer the stack is unwound to the engine's frame at ExecuteReturn; its
//! restored registers name the residency manager, the queue and the list count, and the frame's locals the list array,
//! the per-list residency sets (each closed, its members read), the held manager lock and the submission fence. The
//! scope is live while the same frame, the same snapshot and an unreached fence value still hold. Membership questions
//! -- a pending recording in the invocation, a texture's managed record in its list's set, an immutable asset's pageable
//! in its source set -- are answered against that snapshot and the native records, read as POD only.
//! The C++ Scope holds this module's proof (a COM object); PendingPageableUse's Data stays the C++'s, filled here.
use core::cell::Cell;
use core::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering::*};
use std::sync::OnceLock;

use windows::core::{IUnknown, IUnknown_Vtbl, Interface as _, GUID, HRESULT};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::System::Diagnostics::Debug::{RtlLookupFunctionEntry, CONTEXT};

use crate::native_recording::{
    caller, cvr_native_recording_compatible, cvr_native_recording_validate_submission, fingerprint,
    Lease as Recording, EXECUTE_RETURN,
};
use crate::texture_immutable::{
    immutable_pageable, validate_immutable, ImmutableLease, ImmutablePageable,
};
use crate::texture_lease::{game, read};

pub const MAX_LISTS: u32 = 256;
pub const MAX_SET_OBJECTS: usize = 1048576;
pub const MANAGER_OFFSET: u64 = 0x1AE3780;
pub const REGISTRY_RVA: u64 = 0x3BCDCA8;
/// RTL_CRITICAL_SECTION's RecursionCount and OwningThread (x64).
const RECURSION_COUNT: u64 = 12;
const OWNING_THREAD: u64 = 16;

// ---- the readers (the header's detail part)

/// detail::Frame: the registers restored for the native frame at ExecuteReturn.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Frame {
    pub rip: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub manager: u64,
    pub queue: u64,
    pub count: u32,
    pub status: i32,
}
/// detail::SetHeader: a native residency set.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SetHeader {
    pub index: u32,
    pub pad0: u32,
    pub entries: u64,
    pub capacity: u32,
    pub pad1: u32,
    pub bytes: u64,
    pub count: i32,
    pub open: u8,
    pub out_of_memory: u8,
    pub pad2: u16,
    pub sync_manager: u64,
}
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct SourceSet {
    pub address: u64,
    pub header: SetHeader,
    pub entries: Vec<u64>,
}
/// detail::Snapshot.
#[derive(Clone, Default, Debug)]
pub struct Snapshot {
    pub base: u64,
    pub registry: u64,
    pub thread: u64,
    pub lists_address: u64,
    pub sets_address: u64,
    pub fence_object: u64,
    pub fence: u64,
    pub fence_value: u64,
    pub generation: u64,
    pub sync_manager: u64,
    pub frame: Frame,
    pub lists: Vec<u64>,
    pub sets: Vec<SourceSet>,
}
/// detail::PageableMembership.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct PageableMembership {
    pub context: u64,
    pub list: u64,
    pub registry: u64,
    pub source_set: u64,
    pub managed: u64,
    pub pageable: u64,
    pub generation: u64,
    pub submitted: u64,
    pub source_index: u32,
    pub member_index: u32,
    pub kind: u32,
    pub status: u32,
    pub header: SetHeader,
}
const _: () = assert!(
    core::mem::size_of::<Frame>() == 48
        && core::mem::size_of::<SetHeader>() == 0x30
        && core::mem::offset_of!(SetHeader, count) == 0x20
        && core::mem::offset_of!(SetHeader, sync_manager) == 0x28
        && core::mem::size_of::<PageableMembership>() == 128
);

/// The readers read through this: `mem(address, out)` fills `out` (never empty) or fails.
pub type Mem<'a> = &'a dyn Fn(u64, &mut [u8]) -> bool;
/// detail::ReadMemory.
pub type ReadMemory = unsafe extern "C" fn(*mut c_void, u64, *mut c_void, usize) -> bool;
/// detail::Read: a non-empty read at a non-null address.
pub fn reader(read: Option<ReadMemory>, user: *mut c_void) -> impl Fn(u64, &mut [u8]) -> bool {
    move |at, out| {
        // SAFETY: the caller's reader writes at most `out.len()` bytes.
        read.is_some_and(|r| {
            at != 0
                && !out.is_empty()
                && at <= u64::MAX - out.len() as u64
                && unsafe { r(user, at, out.as_mut_ptr().cast(), out.len()) }
        })
    }
}
fn read_into<T: Copy>(mem: Mem, at: u64, value: &mut T) -> bool {
    // SAFETY: T is plain data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut((value as *mut T).cast::<u8>(), core::mem::size_of::<T>())
    };
    mem(at, bytes)
}
/// detail::Field: a value at address+offset, written in place.
fn field<T: Copy>(mem: Mem, address: u64, offset: u64, value: &mut T) -> bool {
    address != 0 && address <= u64::MAX - offset && read_into(mem, address + offset, value)
}
fn read_u64s(mem: Mem, at: u64, out: &mut [u64]) -> bool {
    // SAFETY: u64s are plain data.
    let bytes =
        unsafe { core::slice::from_raw_parts_mut(out.as_mut_ptr().cast::<u8>(), out.len() * 8) };
    mem(at, bytes)
}

/// detail::Closed: a closed set of this sync manager, its count within its capacity and bounds.
pub fn closed(value: &SetHeader, sync_manager: u64) -> bool {
    value.index == u32::MAX
        && value.open == 0
        && value.out_of_memory == 0
        && value.count >= 0
        && value.count as u32 <= value.capacity
        && value.count as usize <= MAX_SET_OBJECTS
        && (value.count == 0 || value.entries != 0)
        && value.sync_manager == sync_manager
}
/// detail::ReadScope. Rejections: 11 arguments, 12 caller, 13 prior HRESULT, 14 stack shape, 15 frame queue/count; 20
/// registry, 21 manager, 22 native queue; 30 list array, 31 set array, 32 saved count, 33 array read, 34 list
/// identity; 40 lock guard, 41 lock owner, 42 recursion; 50 fence object, 51 fence, 52 fence value, 53 generation, 54
/// sync manager; 60 set header, 61 closed set, 62 total bound, 63 entries read, 64 null member.
pub fn read_scope(
    mem: Mem,
    base: u64,
    thread: u64,
    frame: &Frame,
    queue: u64,
    count: u32,
    lists_address: u64,
) -> Result<Snapshot, u32> {
    if base == 0
        || base > u64::MAX - REGISTRY_RVA
        || thread == 0
        || queue == 0
        || count == 0
        || count > MAX_LISTS
        || lists_address == 0
    {
        return Err(11);
    }
    if !caller(base, frame.rip, EXECUTE_RETURN) {
        return Err(12);
    }
    if frame.status < 0 {
        return Err(13);
    }
    if frame.rsp == 0
        || frame.rsp % 16 != 0
        || frame.rsp > u64::MAX - 0x91
        || frame.rbp != frame.rsp + 0x91
        || frame.rbp < 0x41
    {
        return Err(14);
    }
    if frame.queue != queue || frame.count != count {
        return Err(15);
    }
    let mut value = Snapshot {
        base,
        thread,
        frame: *frame,
        lists_address,
        ..Default::default()
    };
    if !field(mem, base, REGISTRY_RVA, &mut value.registry)
        || value.registry == 0
        || value.registry > u64::MAX - MANAGER_OFFSET
    {
        return Err(20);
    }
    if frame.manager != value.registry + MANAGER_OFFSET {
        return Err(21);
    }
    let (mut expected_queue, mut native_lists, mut guard, mut owner, mut recursion) =
        (0u64, 0u64, 0u64, 0u64, 0i32);
    if !field(mem, value.registry, 0x14104D0, &mut expected_queue) || expected_queue != queue {
        return Err(22);
    }
    if !field(mem, frame.rbp, 0x5F, &mut native_lists) || native_lists != lists_address {
        return Err(30);
    }
    if !field(mem, frame.rbp, 0x67, &mut value.sets_address) || value.sets_address == 0 {
        return Err(31);
    }
    if !read_into(mem, frame.rbp - 0x41, &mut guard)
        || frame.manager > u64::MAX - 0x138
        || guard != frame.manager + 0x138
    {
        return Err(40);
    }
    if !field(mem, guard, OWNING_THREAD, &mut owner) || owner != thread {
        return Err(41);
    }
    if !field(mem, guard, RECURSION_COUNT, &mut recursion) || recursion <= 0 {
        return Err(42);
    }
    if !read_into(mem, frame.rbp - 0x39, &mut value.fence_object) || value.fence_object == 0 {
        return Err(50);
    }
    if !field(mem, value.fence_object, 0, &mut value.fence) || value.fence == 0 {
        return Err(51);
    }
    if !field(mem, value.fence_object, 8, &mut value.fence_value)
        || value.fence_value == 0
        || value.fence_value == u64::MAX
    {
        return Err(52);
    }
    if !field(mem, frame.manager, 0xA8, &mut value.generation) || value.generation == u64::MAX {
        return Err(53);
    }
    if !field(mem, frame.manager, 0x190, &mut value.sync_manager) || value.sync_manager == 0 {
        return Err(54);
    }
    let mut native_count = 0u32;
    if !field(mem, frame.rbp, 0x6F, &mut native_count) || native_count != count {
        return Err(32);
    }
    value.lists = vec![0; count as usize];
    let mut set_addresses = vec![0u64; count as usize];
    if !read_u64s(mem, lists_address, &mut value.lists)
        || !read_u64s(mem, value.sets_address, &mut set_addresses)
    {
        return Err(33);
    }
    let mut total = 0usize;
    for i in 0..count as usize {
        if value.lists[i] == 0 || value.lists[..i].contains(&value.lists[i]) {
            return Err(34);
        }
        let mut set = SourceSet {
            address: set_addresses[i],
            ..Default::default()
        };
        if set.address != 0 {
            if !read_into(mem, set.address, &mut set.header) {
                return Err(60);
            }
            if !closed(&set.header, value.sync_manager) {
                return Err(61);
            }
            total += set.header.count as usize;
            if total > MAX_SET_OBJECTS {
                return Err(62);
            }
            set.entries = vec![0; set.header.count as usize];
            if !set.entries.is_empty() && !read_u64s(mem, set.header.entries, &mut set.entries) {
                return Err(63);
            }
            if set.entries.contains(&0) {
                return Err(64);
            }
        }
        value.sets.push(set);
    }
    Ok(value)
}
/// detail::Same.
pub fn same(a: &Snapshot, b: &Snapshot) -> bool {
    a.base == b.base
        && a.registry == b.registry
        && a.thread == b.thread
        && a.frame == b.frame
        && a.lists_address == b.lists_address
        && a.sets_address == b.sets_address
        && a.fence_object == b.fence_object
        && a.fence == b.fence
        && a.fence_value == b.fence_value
        && a.generation == b.generation
        && a.sync_manager == b.sync_manager
        && a.lists == b.lists
        && a.sets == b.sets
}
fn observed_start(observed: &mut [u64; 12], list: u64, generation: u64) {
    *observed = [0; 12];
    observed[0] = list;
    observed[10] = generation;
    observed[11] = u64::MAX;
}
/// detail::ContainsRecording. Rejections: 351 arguments/registry, 352 absent list, 353 duplicate list, 354/355
/// context-list read/mismatch, 356/357 context-kind read/unsupported.
pub fn contains_recording(
    mem: Mem,
    scope: &Snapshot,
    context: u64,
    list: u64,
    registry: u64,
    observed: &mut [u64; 12],
) -> Result<(), u32> {
    observed_start(observed, list, scope.generation);
    if context == 0 || list == 0 || registry == 0 || registry != scope.registry {
        return Err(351);
    }
    match scope.lists.iter().filter(|&&l| l == list).count() {
        0 => return Err(352),
        1 => {}
        _ => return Err(353),
    }
    let (mut actual_list, mut kind) = (0u64, 0u32);
    let list_read = field(mem, context, 0x30, &mut actual_list);
    let kind_read = field(mem, context, 0x68, &mut kind);
    observed[1] = actual_list;
    observed[5] = kind as u64;
    if !list_read {
        return Err(354);
    }
    if actual_list != list {
        return Err(355);
    }
    if !kind_read {
        return Err(356);
    }
    if kind != 1 && kind != 2 {
        return Err(357);
    }
    Ok(())
}
/// detail::Contains. Rejections: 301 arguments, 302 source list absent, 303 source index, 304 no source set; 310/311
/// context-list read/mismatch, 312/313 tracker read/mismatch, 314/315 submission-counter read/zero, 316/317 context-kind
/// read/unsupported; 320 set-header read, 321 changed header, 322 not closed; 330 record overflow; 331/332 resource
/// read/mismatch, 333/334 pageable read/mismatch, 335/336 status read/not resident, 337/338 generation read/mismatch;
/// 340 managed member absent, 341 live member read, 342 changed member.
#[allow(clippy::too_many_arguments)]
pub fn contains(
    mem: Mem,
    scope: &Snapshot,
    context: u64,
    list: u64,
    registry: u64,
    handle: u32,
    resource: u64,
    observed: &mut [u64; 12],
) -> Result<(), u32> {
    observed_start(observed, list, scope.generation);
    if context == 0
        || list == 0
        || registry != scope.registry
        || resource == 0
        || handle == 0
        || handle > 32768
    {
        return Err(301);
    }
    let Some(index) = scope.lists.iter().position(|&l| l == list) else {
        return Err(302);
    };
    let Some(set) = scope.sets.get(index) else {
        return Err(303);
    };
    observed[2] = set.address;
    if set.address == 0 {
        return Err(304);
    }
    let (mut actual_list, mut tracker, mut submitted, mut kind) = (0u64, 0u64, 0u64, 0u32);
    let list_read = field(mem, context, 0x30, &mut actual_list);
    let tracker_read = field(mem, context, 0x630, &mut tracker);
    let submitted_read = field(mem, context, 0x638, &mut submitted);
    let kind_read = field(mem, context, 0x68, &mut kind);
    observed[1] = actual_list;
    observed[3] = tracker;
    observed[4] = submitted;
    observed[5] = kind as u64;
    for (ok, code) in [
        (list_read, 310),
        (actual_list == list, 311),
        (tracker_read, 312),
        (tracker == set.address, 313),
        (submitted_read, 314),
        (submitted != 0, 315),
        (kind_read, 316),
        (kind == 1 || kind == 2, 317),
    ] {
        if !ok {
            return Err(code);
        }
    }
    let mut current = SetHeader::default();
    if !read_into(mem, set.address, &mut current) {
        return Err(320);
    }
    if current != set.header {
        return Err(321);
    }
    if !closed(&current, scope.sync_manager) {
        return Err(322);
    }
    let offset = 0x2F1D8 + (handle as u64 - 1) * 0xB0;
    if registry > u64::MAX - offset - 0xB0 {
        return Err(330);
    }
    let record = registry + offset;
    let managed = record + 0x60;
    let (mut identity, mut pageable, mut generation, mut status) = (0u64, 0u64, 0u64, 0u32);
    let identity_read = field(mem, record, 0, &mut identity);
    let pageable_read = field(mem, managed, 8, &mut pageable);
    let status_read = field(mem, managed, 0, &mut status);
    let generation_read = field(mem, managed, 0x18, &mut generation);
    let member = set.entries.iter().position(|&e| e == managed);
    observed[6] = identity;
    observed[7] = pageable;
    observed[8] = status as u64;
    observed[9] = generation;
    if let Some(m) = member {
        observed[11] = m as u64;
    }
    for (ok, code) in [
        (identity_read, 331),
        (identity == resource, 332),
        (pageable_read, 333),
        (pageable == resource, 334),
        (status_read, 335),
        (status == 0, 336),
        (generation_read, 337),
        (generation == scope.generation, 338),
    ] {
        if !ok {
            return Err(code);
        }
    }
    // Close clears the insertion bits and index. The retained ppSet entry is the membership witness; neither those
    // bits nor the freed master set can be used.
    let Some(member) = member else {
        return Err(340);
    };
    let mut actual = 0u64;
    if !field(mem, current.entries, member as u64 * 8, &mut actual) {
        return Err(341);
    }
    if actual != managed {
        return Err(342);
    }
    Ok(())
}
/// detail::SearchInvocationPageable: a diagnostic-only scan of the whole invocation, never a replacement for source-set
/// membership. Errors 501 arguments, 502 source list, 503 set shape/header, 504 member pointer, 505 managed bytes, 506
/// changed member/header (507 changing scan and 508 expired scope are the runtime's).
pub fn search_invocation_pageable(
    mem: Mem,
    scope: &Snapshot,
    source_list: u64,
    pageable: u64,
    output: &mut [u64; 20],
) -> bool {
    *output = [
        1,
        0,
        0,
        pageable,
        source_list,
        u64::MAX,
        scope.sets.len() as u64,
        0,
        0,
        0,
        0,
        0,
        u64::MAX,
        u64::MAX,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let fail = |output: &mut [u64; 20], code: u64| {
        output[2] = code;
        false
    };
    if source_list == 0
        || pageable == 0
        || scope.lists.is_empty()
        || scope.lists.len() > MAX_LISTS as usize
        || scope.sets.len() != scope.lists.len()
    {
        return fail(output, 501);
    }
    let Some(own) = scope.lists.iter().position(|&l| l == source_list) else {
        return fail(output, 502);
    };
    if scope.lists[own + 1..].contains(&source_list) {
        return fail(output, 502);
    }
    output[5] = own as u64;
    let mut total = 0usize;
    for set in &scope.sets {
        if set.entries.len() > MAX_SET_OBJECTS - total {
            return fail(output, 503);
        }
        total += set.entries.len();
    }
    let mut matches = Vec::new();
    for (index, set) in scope.sets.iter().enumerate() {
        if set.address == 0 {
            if !set.entries.is_empty() {
                return fail(output, 503);
            }
            continue;
        }
        let mut header = SetHeader::default();
        if !read_into(mem, set.address, &mut header)
            || header != set.header
            || !closed(&header, scope.sync_manager)
            || header.count as usize != set.entries.len()
        {
            return fail(output, 503);
        }
        for (member_index, &expected) in set.entries.iter().enumerate() {
            let (mut managed, mut actual, mut generation, mut again, mut status) =
                (0u64, 0u64, 0u64, 0u64, 0u32);
            if !field(mem, header.entries, member_index as u64 * 8, &mut managed)
                || managed == 0
                || managed != expected
            {
                return fail(output, 504);
            }
            if !field(mem, managed, 8, &mut actual) {
                return fail(output, 505);
            }
            output[7] += 1;
            if actual != pageable {
                continue;
            }
            if !field(mem, managed, 0, &mut status) || !field(mem, managed, 0x18, &mut generation) {
                return fail(output, 505);
            }
            let (mut status_again, mut generation_again) = (0u32, 0u64);
            if !field(mem, managed, 8, &mut again)
                || again != actual
                || !field(mem, managed, 0, &mut status_again)
                || status_again != status
                || !field(mem, managed, 0x18, &mut generation_again)
                || generation_again != generation
                || !field(mem, header.entries, member_index as u64 * 8, &mut again)
                || again != managed
            {
                return fail(output, 506);
            }
            output[if index == own { 8 } else { 9 }] += 1;
            if status == 0 && generation == scope.generation {
                output[if index == own { 10 } else { 11 }] += 1;
            }
            if output[12] == u64::MAX {
                output[12] = index as u64;
                output[13] = member_index as u64;
                output[14] = managed;
                output[15] = scope.lists[index];
                output[16] = status as u64;
                output[17] = generation;
            }
            matches.push(managed);
        }
        let mut after = SetHeader::default();
        if !read_into(mem, set.address, &mut after) || after != header {
            return fail(output, 506);
        }
    }
    matches.sort_unstable();
    matches.dedup();
    output[18] = matches.len() as u64;
    output[1] = 1;
    true
}
/// detail::ReadPageableMembership: the expected pageable (from a held, authenticated ImmutablePageable) as exactly one
/// resident member of the source list's own set. Rejections 401 arguments, 402 source set, 403 context read, 404 context
/// mismatch, 405 header read, 406 changed/open set, 407 member pointer, 408 pageable read, 409 absent, 410 ambiguous,
/// 411 managed state read, 412 not resident/generation, 413 changed membership. No neighbouring set is searched.
#[allow(clippy::too_many_arguments)]
pub fn read_pageable_membership(
    mem: Mem,
    scope: &Snapshot,
    context: u64,
    list: u64,
    registry: u64,
    pageable: u64,
    observed: &mut [u64; 12],
) -> Result<PageableMembership, u32> {
    *observed = [0; 12];
    observed[0] = list;
    observed[6] = pageable;
    observed[10] = scope.generation;
    observed[11] = u64::MAX;
    if context == 0
        || list == 0
        || registry == 0
        || registry != scope.registry
        || pageable == 0
        || scope.lists.len() > MAX_LISTS as usize
        || scope.lists.len() != scope.sets.len()
    {
        return Err(401);
    }
    let Some(index) = scope.lists.iter().position(|&l| l == list) else {
        return Err(402);
    };
    if scope.lists[index + 1..].contains(&list) {
        return Err(402);
    }
    let source = &scope.sets[index];
    observed[2] = source.address;
    if source.address == 0
        || source.entries.len() > MAX_SET_OBJECTS
        || source.header.count < 0
        || source.entries.len() != source.header.count as usize
    {
        return Err(402);
    }
    let mut value = PageableMembership {
        context,
        list,
        registry,
        source_set: source.address,
        source_index: index as u32,
        pageable,
        ..Default::default()
    };
    let (mut actual_list, mut tracker) = (0u64, 0u64);
    if !field(mem, context, 0x30, &mut actual_list)
        || !field(mem, context, 0x630, &mut tracker)
        || !field(mem, context, 0x638, &mut value.submitted)
        || !field(mem, context, 0x68, &mut value.kind)
    {
        return Err(403);
    }
    observed[1] = actual_list;
    observed[3] = tracker;
    observed[4] = value.submitted;
    observed[5] = value.kind as u64;
    if actual_list != list
        || tracker != source.address
        || value.submitted == 0
        || (value.kind != 1 && value.kind != 2)
    {
        return Err(404);
    }
    if !read_into(mem, source.address, &mut value.header) {
        return Err(405);
    }
    if value.header != source.header || !closed(&value.header, scope.sync_manager) {
        return Err(406);
    }
    let mut matched = false;
    for (i, &expected) in source.entries.iter().enumerate() {
        let (mut member, mut candidate) = (0u64, 0u64);
        if !field(mem, value.header.entries, i as u64 * 8, &mut member)
            || member == 0
            || member != expected
        {
            return Err(407);
        }
        if !field(mem, member, 8, &mut candidate) {
            return Err(408);
        }
        if candidate != pageable {
            continue;
        }
        if matched {
            return Err(410);
        }
        matched = true;
        value.managed = member;
        value.member_index = i as u32;
        observed[7] = candidate;
        observed[11] = i as u64;
        if !field(mem, member, 0, &mut value.status)
            || !field(mem, member, 0x18, &mut value.generation)
        {
            return Err(411);
        }
        observed[8] = value.status as u64;
        observed[9] = value.generation;
        if value.status != 0 || value.generation != scope.generation {
            return Err(412);
        }
    }
    if !matched {
        return Err(409);
    }
    // Re-read only the authenticated member and context after the bounded scan.
    let mut again = SetHeader::default();
    let (mut member, mut candidate, mut generation, mut submitted, mut actual) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let (mut status, mut kind) = (0u32, 0u32);
    if !read_into(mem, source.address, &mut again)
        || again != value.header
        || !field(
            mem,
            value.header.entries,
            value.member_index as u64 * 8,
            &mut member,
        )
        || member != value.managed
        || !field(mem, member, 8, &mut candidate)
        || candidate != pageable
        || !field(mem, member, 0, &mut status)
        || status != value.status
        || !field(mem, member, 0x18, &mut generation)
        || generation != value.generation
        || !field(mem, context, 0x30, &mut actual)
        || actual != list
        || !field(mem, context, 0x630, &mut actual)
        || actual != source.address
        || !field(mem, context, 0x638, &mut submitted)
        || submitted != value.submitted
        || !field(mem, context, 0x68, &mut kind)
        || kind != value.kind
    {
        return Err(413);
    }
    Ok(value)
}

// ---- the runtime scopes

thread_local! {
    static FAILURE: Cell<u32> = const { Cell::new(0) };
    /// RIP,RSP,RBP,manager,queue,count,raw RBX/HRESULT,LastFailure,detail code,expected queue,expected count,expected
    /// lists address (see the header).
    static DETAILS: Cell<[u64; 12]> = const { Cell::new([0; 12]) };
    static MEMBERSHIP: Cell<[u64; 12]> = const { Cell::new([0; 12]) };
    static SEARCH: Cell<[u64; 20]> = const { Cell::new([0; 20]) };
}
fn details(update: impl FnOnce(&mut [u64; 12])) {
    let mut d = DETAILS.get();
    update(&mut d);
    DETAILS.set(d);
}
fn stage(value: u32) {
    FAILURE.set(value);
    details(|d| d[7] = value as u64);
}
fn detail_failure(value: u64) -> bool {
    details(|d| d[8] = value);
    false
}
fn expected(queue: u64, count: u32, lists: u64) {
    details(|d| {
        d[9] = queue;
        d[10] = count as u64;
        d[11] = lists;
    });
}
fn reset_membership() {
    let mut m = [0u64; 12];
    m[11] = u64::MAX;
    MEMBERSHIP.set(m);
}
/// The runtime's reader: this process's memory (ReadProcessMemory), non-empty reads at non-null addresses.
fn memory(at: u64, out: &mut [u8]) -> bool {
    at != 0 && !out.is_empty() && at <= u64::MAX - out.len() as u64 && read(at, out)
}
fn value_at<T: Copy + Default>(at: u64) -> Option<T> {
    let mut value = T::default();
    read_into(&memory, at, &mut value).then_some(value)
}
/// ImageBytes: the game image's SizeOfImage from its headers (0 when they are not an x64 image containing ExecuteReturn).
fn image_bytes(base: u64) -> u64 {
    let Some(magic) = value_at::<u16>(base) else {
        return 0;
    };
    let Some(lfanew) = value_at::<i32>(base + 0x3C) else {
        return 0;
    };
    if magic != 0x5A4D || lfanew <= 0 || lfanew > 0x100000 {
        return 0;
    }
    let nt = base + lfanew as u64;
    let (Some(signature), Some(machine), Some(optional), Some(size)) = (
        value_at::<u32>(nt),
        value_at::<u16>(nt + 4),
        value_at::<u16>(nt + 24),
        value_at::<u32>(nt + 24 + 56),
    ) else {
        return 0;
    };
    if signature != 0x4550
        || machine != 0x8664
        || optional != 0x20B
        || size <= EXECUTE_RETURN
        || base > u64::MAX - size as u64
    {
        return 0;
    }
    size as u64
}
/// The contract: native_recording's, and the 16 audited engine functions the readers rely on.
fn compatible() -> bool {
    static COMPATIBLE: OnceLock<bool> = OnceLock::new();
    *COMPATIBLE.get_or_init(|| {
        const CONTRACT: [(u64, usize, u64); 16] = [
            (0x337264, 282, 0xAEBEF7FE0B6EC646),
            (0x3E12CC, 148, 0xA99EAC2C327A1A01),
            (0x3E68E4, 159, 0x36A662C5ADF39CDA),
            (0x3E69FC, 35, 0xFC87893BAD1DB49B),
            (0x3E7B44, 24, 0xD3ECE8804151AE74),
            (0xB78604, 308, 0xABCE987D2A4F1C35),
            (0xB676F0, 149, 0x5EC771CE4585B4E5),
            (0x25303CE, 70, 0x71CCEFA5FD74E26B),
            (0xA75834, 111, 0xB9B26CE5992548F2),
            (0xA758A4, 268, 0xD752C8B19FDAE7F9),
            (0x3E95FC, 178, 0x4F51AC4AA4337B33),
            (0x3E96B0, 34, 0xF45D2BD186398013),
            (0x3E9320, 729, 0x3184C655DC3D5392),
            (0x246A4FE, 846, 0xED7A4FE1D6329A86),
            (0x2FA5ABC, 151, 0xBAFB466EAECF93A5),
            (0x2FA5B54, 319, 0x6F012939DC1AA09A),
        ];
        if !cvr_native_recording_compatible() {
            return false;
        }
        let base = game();
        let mut bytes = [0u8; 2048];
        CONTRACT.iter().all(|&(rva, len, hash)| {
            base != 0
                && base <= u64::MAX - rva
                && memory(base + rva, &mut bytes[..len])
                && fingerprint(&bytes[..len]) == hash
        })
    })
}
#[link(name = "kernel32")]
extern "system" {
    fn RtlCaptureContext(context: *mut CONTEXT);
}
extern "C" {
    /// rust/crates/cvr-engine/native/seh.c: one RtlVirtualUnwind step under SEH (1 unwound, 2 no function entry, 0 fault).
    fn cvr_seh_unwind_step(context: *mut CONTEXT) -> i32;
}
/// The thread's stack as the TIB records it (StackLimit, StackBase).
fn stack_bounds() -> (u64, u64) {
    let (limit, base): (u64, u64);
    // SAFETY: the x64 TEB's NT_TIB StackBase (gs:[8]) and StackLimit (gs:[0x10]) of this thread.
    unsafe {
        core::arch::asm!("mov {}, gs:[0x10]", out(reg) limit, options(nostack, readonly, preserves_flags));
        core::arch::asm!("mov {}, gs:[0x08]", out(reg) base, options(nostack, readonly, preserves_flags));
    }
    (limit, base)
}
/// Unwind: the OS unwinder from this frame up to the first frame inside the game image, which must be ExecuteSubset's
/// return to ExecuteReturn; its restored registers are the native frame. Detail codes 101 image, 102 bounds, 103
/// caller, 104 native registers, 105 native metadata, 106 missing metadata, 107 progress, 108 depth, 109 fault.
#[inline(never)]
fn unwind(base: u64) -> Option<Frame> {
    let image = image_bytes(base);
    if image == 0 {
        detail_failure(101);
        return None;
    }
    let (low, high) = stack_bounds();
    #[repr(C, align(16))]
    struct Aligned(CONTEXT);
    // SAFETY: CONTEXT is plain data; RtlCaptureContext fills it with this frame's registers.
    let mut aligned: Aligned = unsafe { core::mem::zeroed() };
    let c = &mut aligned.0;
    unsafe { RtlCaptureContext(c) };
    for _ in 0..64 {
        details(|d| {
            d[0] = c.Rip;
            d[1] = c.Rsp;
            d[2] = c.Rbp;
            d[3] = c.Rsi;
            d[4] = c.R12;
            d[5] = c.R15;
            d[6] = c.Rbx;
        });
        if c.Rip == 0 || c.Rsp < low || c.Rsp >= high {
            detail_failure(102);
            return None;
        }
        if c.Rip >= base && c.Rip - base < image {
            if c.Rip != base + EXECUTE_RETURN as u64 {
                detail_failure(103);
                return None;
            }
            if c.Rbp < low
                || c.Rbp > u64::MAX - 0x73
                || c.Rbp + 0x73 > high
                || c.R15 > u32::MAX as u64
                || c.Rbx > u32::MAX as u64
            {
                detail_failure(104);
                return None;
            }
            let mut native_base = 0u64;
            // SAFETY: a code address of the game image.
            let entry = unsafe { RtlLookupFunctionEntry(c.Rip, &mut native_base, None) };
            // SAFETY: the OS's function table entry, or null.
            let ok = !entry.is_null()
                && native_base == base
                && unsafe { (*entry).BeginAddress } == 0x3E1020
                && unsafe { (*entry).EndAddress } > EXECUTE_RETURN;
            if !ok {
                detail_failure(105);
                return None;
            }
            details(|d| d[8] = 0);
            return Some(Frame {
                rip: c.Rip,
                rsp: c.Rsp,
                rbp: c.Rbp,
                manager: c.Rsi,
                queue: c.R12,
                count: c.R15 as u32,
                status: c.Rbx as u32 as i32,
            });
        }
        let previous = c.Rsp;
        // SAFETY: the context of a frame of this thread, stepped by the OS unwinder under SEH.
        match unsafe { cvr_seh_unwind_step(c) } {
            1 => {}
            2 => {
                detail_failure(106);
                return None;
            }
            _ => {
                detail_failure(109);
                return None;
            }
        }
        if c.Rsp <= previous {
            detail_failure(107);
            return None;
        }
    }
    detail_failure(108);
    None
}

/// The scope's proof: the snapshot of the invocation and the queue, fence and device it was taken for.
#[repr(C)]
struct Proof {
    vtbl: *const IUnknown_Vtbl,
    refs: AtomicU32,
    snapshot: Snapshot,
    queue: Option<ID3D12CommandQueue>,
    fence: Option<ID3D12Fence>,
    device: Option<ID3D12Device>,
}
const PROOF_TYPE: GUID = GUID::from_u128(0x6b1f0c52_3a9e_4d47_9f21_5c0e8a7d3b41);
const IID_IUNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
unsafe extern "system" fn proof_query(
    this: *mut c_void,
    iid: *const GUID,
    out: *mut *mut c_void,
) -> HRESULT {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's out-pointer and IID; `this` is a live Proof.
    unsafe {
        *out = core::ptr::null_mut();
        if iid.is_null() || (*iid != IID_IUNKNOWN && *iid != PROOF_TYPE) {
            return E_NOINTERFACE;
        }
        *out = this;
        (*(this as *const Proof)).refs.fetch_add(1, SeqCst);
    }
    S_OK
}
unsafe extern "system" fn proof_add_ref(this: *mut c_void) -> u32 {
    // SAFETY: a live Proof.
    unsafe { (*(this as *const Proof)).refs.fetch_add(1, SeqCst) + 1 }
}
unsafe extern "system" fn proof_release(this: *mut c_void) -> u32 {
    // SAFETY: a live Proof; at zero it is the last reference and is freed.
    unsafe {
        let left = (*(this as *const Proof)).refs.fetch_sub(1, SeqCst) - 1;
        if left == 0 {
            drop(Box::from_raw(this as *mut Proof));
        }
        left
    }
}
static PROOF_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: proof_query,
    AddRef: proof_add_ref,
    Release: proof_release,
};
/// This module's proof behind a C++ Scope's handle (null: no proof).
fn proof_of<'a>(handle: *mut c_void) -> Option<&'a Proof> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: a live COM object the caller holds; only this module's proofs carry PROOF_VTBL.
    let proof = unsafe { &*(handle as *const Proof) };
    (proof.vtbl == &PROOF_VTBL as *const _).then_some(proof)
}
fn thread() -> u64 {
    // SAFETY: no preconditions.
    unsafe { windows::Win32::System::Threading::GetCurrentThreadId() as u64 }
}
fn raw<T: windows::core::Interface>(p: &Option<T>) -> *mut c_void {
    p.as_ref().map_or(core::ptr::null_mut(), |v| v.as_raw())
}
/// FenceValid: the device is not removed and the invocation's fence value is not yet reached.
fn fence_valid(proof: &Proof) -> bool {
    let (Some(_), Some(fence), Some(device)) = (&proof.queue, &proof.fence, &proof.device) else {
        return detail_failure(201);
    };
    // SAFETY: live objects.
    if unsafe { device.GetDeviceRemovedReason() }.is_err() {
        return detail_failure(202);
    }
    if unsafe { fence.GetCompletedValue() } >= proof.snapshot.fence_value {
        return detail_failure(203);
    }
    details(|d| d[8] = 0);
    true
}
/// Live: still inside the same native invocation on this thread, the snapshot unchanged, the fence value unreached.
fn live(proof: &Proof) -> bool {
    let saved = &proof.snapshot;
    expected(saved.frame.queue, saved.frame.count, saved.lists_address);
    stage(2);
    if !compatible() {
        return false;
    }
    stage(5);
    if saved.thread != thread() || saved.base != game() {
        return false;
    }
    stage(3);
    let Some(frame) = unwind(saved.base) else {
        return false;
    };
    stage(4);
    let current = read_scope(
        &memory,
        saved.base,
        saved.thread,
        &frame,
        saved.frame.queue,
        saved.frame.count,
        saved.lists_address,
    );
    details(|d| d[8] = current.as_ref().err().copied().unwrap_or(0) as u64);
    let Ok(current) = current else {
        return false;
    };
    stage(5);
    if !same(saved, &current) {
        return false;
    }
    stage(6);
    fence_valid(proof)
}
/// ObserveSiblingPageables: after a strict source-set absence, a diagnostic scan of the whole invocation (the failure
/// state is restored afterwards).
fn observe_sibling_pageables(proof: &Proof, list: u64, pageable: u64) {
    let (prior_failure, prior_details, prior_membership) =
        (FAILURE.get(), DETAILS.get(), MEMBERSHIP.get());
    let short = |code: u64| {
        let mut s = [0u64; 20];
        s[0] = 1;
        s[2] = code;
        s[3] = pageable;
        s[4] = list;
        s
    };
    let (mut first, mut second) = ([0u64; 20], [0u64; 20]);
    let result = if !live(proof) {
        short(508)
    } else if !search_invocation_pageable(&memory, &proof.snapshot, list, pageable, &mut first) {
        first
    } else if !search_invocation_pageable(&memory, &proof.snapshot, list, pageable, &mut second) {
        second
    } else if first != second {
        second[1] = 0;
        second[2] = 507;
        second
    } else {
        if !live(proof) {
            second[1] = 0;
            second[2] = 508;
        }
        second
    };
    SEARCH.set(result);
    FAILURE.set(prior_failure);
    DETAILS.set(prior_details);
    MEMBERSHIP.set(prior_membership);
}
fn validate_submission(source: &Recording, queue: &Option<ID3D12CommandQueue>) -> bool {
    // SAFETY: a valid lease; a live queue.
    unsafe { cvr_native_recording_validate_submission(source, raw(queue)) }
}

/// Capture: the scope of the current native ExecuteSubset invocation of these lists on this direct queue.
fn capture(queue_ptr: *mut c_void, count: u32, lists: u64) -> Option<*mut c_void> {
    details(|d| *d = [0; 12]);
    reset_membership();
    stage(1);
    expected(queue_ptr as u64, count, lists);
    // SAFETY: a live queue of the caller's, or null.
    let queue = unsafe { ID3D12CommandQueue::from_raw_borrowed(&queue_ptr) }?;
    // SAFETY: a live queue.
    if count == 0
        || count > MAX_LISTS
        || lists == 0
        || unsafe { queue.GetDesc() }.Type != D3D12_COMMAND_LIST_TYPE_DIRECT
    {
        return None;
    }
    stage(2);
    if !compatible() {
        return None;
    }
    let base = game();
    stage(3);
    let frame = unwind(base)?;
    stage(4);
    let snapshot = read_scope(
        &memory,
        base,
        thread(),
        &frame,
        queue_ptr as u64,
        count,
        lists,
    );
    details(|d| d[8] = snapshot.as_ref().err().copied().unwrap_or(0) as u64);
    let snapshot = snapshot.ok()?;
    let fence_ptr = snapshot.fence as *mut c_void;
    let mut proof = Box::new(Proof {
        vtbl: &PROOF_VTBL,
        refs: AtomicU32::new(1),
        snapshot,
        queue: Some(queue.clone()),
        // SAFETY: the engine's fence object named by the invocation's fence slot (as the C++ took it).
        fence: unsafe { ID3D12Fence::from_raw_borrowed(&fence_ptr) }.cloned(),
        device: None,
    });
    stage(6);
    let mut device: Option<ID3D12Device> = None;
    let mut fence_device: Option<ID3D12Device> = None;
    // SAFETY: live objects.
    let ok = unsafe { queue.GetDevice(&mut device) }.is_ok()
        && proof
            .fence
            .as_ref()
            .is_some_and(|f| unsafe { f.GetDevice(&mut fence_device) }.is_ok())
        && device.is_some()
        && raw(&device) == raw(&fence_device);
    proof.device = device;
    if !ok || !fence_valid(&proof) || !live(&proof) {
        return None;
    }
    stage(0);
    Some(Box::into_raw(proof) as *mut c_void)
}
pub(crate) fn validate(handle: *mut c_void) -> bool {
    details(|d| *d = [0; 12]);
    reset_membership();
    stage(1);
    let Some(proof) = proof_of(handle) else {
        return false;
    };
    if !live(proof) {
        return false;
    }
    stage(0);
    true
}
pub(crate) fn contains_recording_runtime(handle: *mut c_void, source: Option<&Recording>) -> bool {
    details(|d| *d = [0; 12]);
    reset_membership();
    stage(1);
    let (Some(proof), Some(source)) = (proof_of(handle), source) else {
        return false;
    };
    if source.token.is_null() || !live(proof) {
        return false;
    }
    stage(7);
    if !validate_submission(source, &proof.queue) {
        return false;
    }
    stage(8);
    let mut observed = [0u64; 12];
    let result = contains_recording(
        &memory,
        &proof.snapshot,
        source.context,
        source.list,
        source.registry,
        &mut observed,
    );
    MEMBERSHIP.set(observed);
    details(|d| d[8] = result.err().unwrap_or(0) as u64);
    if result.is_err() {
        return false;
    }
    stage(0);
    true
}
fn contains_runtime(
    handle: *mut c_void,
    source: Option<&Recording>,
    registry: u64,
    handle_id: u32,
    resource: u64,
) -> bool {
    details(|d| *d = [0; 12]);
    reset_membership();
    stage(1);
    let (Some(proof), Some(source)) = (proof_of(handle), source) else {
        return false;
    };
    if source.token.is_null() || !live(proof) {
        return false;
    }
    stage(7);
    if source.registry != registry || !validate_submission(source, &proof.queue) {
        return false;
    }
    stage(8);
    let mut observed = [0u64; 12];
    let result = contains(
        &memory,
        &proof.snapshot,
        source.context,
        source.list,
        registry,
        handle_id,
        resource,
        &mut observed,
    );
    MEMBERSHIP.set(observed);
    details(|d| d[8] = result.err().unwrap_or(0) as u64);
    if result.is_err() {
        return false;
    }
    stage(0);
    true
}
/// The Invocation the C++ copies: the thread, queue and list array of the live invocation (its lists borrowed from the
/// proof the caller holds).
#[repr(C)]
pub struct InvocationView {
    pub thread: u64,
    pub queue: u64,
    pub lists_address: u64,
    pub lists: *const u64,
    pub count: usize,
}
fn describe_invocation(handle: *mut c_void, out: &mut InvocationView) -> bool {
    stage(1);
    let Some(proof) = proof_of(handle) else {
        return false;
    };
    if !live(proof) {
        return false;
    }
    let s = &proof.snapshot;
    let value = InvocationView {
        thread: s.thread,
        queue: s.frame.queue,
        lists_address: s.lists_address,
        lists: s.lists.as_ptr(),
        count: s.lists.len(),
    };
    if !live(proof) {
        return false;
    }
    *out = value;
    stage(0);
    true
}
fn release(p: *mut c_void) {
    if !p.is_null() {
        // SAFETY: a reference the caller owns.
        unsafe { ((*(*(p as *mut *const IUnknown_Vtbl))).Release)(p) };
    }
}
/// An ImmutablePageable the C++ does not receive: its three references released.
fn release_pageable(p: &ImmutablePageable) {
    release(p.source.owner);
    release(p.source.source.resource);
    release(p.pageable);
}
fn pageable_kind(physical: &ImmutablePageable, asset: &ImmutableLease) -> Option<u32> {
    if physical.pageable == asset.source.resource {
        return Some(1);
    }
    // SAFETY: the held pageable, or null.
    let pageable = unsafe { IUnknown::from_raw_borrowed(&physical.pageable) }?;
    pageable.cast::<ID3D12Heap>().ok().map(|_| 2)
}
/// BindPageableUse: an immutable asset's pageable as a resident member of the source list's own set in the live
/// invocation, while the source lease is pending. `physical`, `membership` and `kind` are the C++ Data's (zeroed).
fn bind_pageable_use(
    handle: *mut c_void,
    source: Option<&Recording>,
    asset: Option<&ImmutableLease>,
    physical: &mut ImmutablePageable,
    membership: &mut PageableMembership,
    kind: &mut u32,
) -> bool {
    details(|d| *d = [0; 12]);
    reset_membership();
    SEARCH.set([0; 20]);
    stage(1);
    let (Some(proof), Some(source), Some(asset)) = (proof_of(handle), source, asset) else {
        return false;
    };
    if source.token.is_null()
        || asset.owner.is_null()
        || asset.source.resource.is_null()
        || !live(proof)
    {
        return false;
    }
    stage(7);
    if source.registry != asset.source.native.registry || !validate_submission(source, &proof.queue)
    {
        return false;
    }
    stage(10);
    if !immutable_pageable(raw(&proof.device), asset, physical)
        || !validate_immutable(asset, raw(&proof.queue))
    {
        return false;
    }
    let Some(k) = pageable_kind(physical, asset) else {
        return false;
    };
    *kind = k;
    stage(11);
    let mut observed = [0u64; 12];
    let result = read_pageable_membership(
        &memory,
        &proof.snapshot,
        source.context,
        source.list,
        source.registry,
        physical.pageable as u64,
        &mut observed,
    );
    MEMBERSHIP.set(observed);
    match result {
        Ok(value) => *membership = value,
        Err(code) => {
            details(|d| d[8] = code as u64);
            if code == 409 {
                observe_sibling_pageables(proof, source.list, physical.pageable as u64);
            }
            return false;
        }
    }
    stage(7);
    if !validate_submission(source, &proof.queue) || !live(proof) {
        return false;
    }
    stage(10);
    if !validate_immutable(&physical.source, raw(&proof.queue)) {
        return false;
    }
    stage(0);
    true
}
/// ValidatePageableUse: the same live invocation, the asset's pageable unchanged and the same membership.
fn validate_pageable_use(
    handle: *mut c_void,
    source: Option<&Recording>,
    physical: Option<&ImmutablePageable>,
    membership: Option<&PageableMembership>,
) -> bool {
    details(|d| *d = [0; 12]);
    reset_membership();
    SEARCH.set([0; 20]);
    stage(1);
    let (Some(proof), Some(source), Some(saved), Some(membership)) =
        (proof_of(handle), source, physical, membership)
    else {
        return false;
    };
    if !live(proof) {
        return false;
    }
    stage(10);
    let mut current = ImmutablePageable {
        source: ImmutableLease {
            owner: core::ptr::null_mut(),
            source: crate::texture_lease::Lease {
                resource: core::ptr::null_mut(),
                native: Default::default(),
                receipt: 0,
                owner: 0,
            },
        },
        pageable: core::ptr::null_mut(),
    };
    let got = immutable_pageable(raw(&proof.device), &saved.source, &mut current);
    let same_pageable = got && current.pageable == saved.pageable;
    release_pageable(&current);
    if !same_pageable || !validate_immutable(&saved.source, raw(&proof.queue)) {
        return false;
    }
    stage(11);
    let mut observed = [0u64; 12];
    let result = read_pageable_membership(
        &memory,
        &proof.snapshot,
        source.context,
        source.list,
        source.registry,
        saved.pageable as u64,
        &mut observed,
    );
    MEMBERSHIP.set(observed);
    match result {
        Ok(value) if value == *membership => {}
        Ok(_) => return detail_failure(413),
        Err(code) => {
            details(|d| d[8] = code as u64);
            if code == 409 {
                observe_sibling_pageables(proof, source.list, saved.pageable as u64);
            }
            return false;
        }
    }
    if !live(proof) {
        return false;
    }
    stage(10);
    if !validate_immutable(&saved.source, raw(&proof.queue)) {
        return false;
    }
    stage(0);
    true
}

// ---- the C ABI: the header's readers (a Snapshot crosses as a view of its vectors) and the module's runtime

/// detail::SetHeader with its entries as the C++ vector holds them.
#[repr(C)]
pub struct SetView {
    pub address: u64,
    pub header: SetHeader,
    pub entries: *const u64,
    pub count: usize,
}
/// detail::Snapshot as views of its vectors.
#[repr(C)]
pub struct SnapshotView {
    pub base: u64,
    pub registry: u64,
    pub thread: u64,
    pub lists_address: u64,
    pub sets_address: u64,
    pub fence_object: u64,
    pub fence: u64,
    pub fence_value: u64,
    pub generation: u64,
    pub sync_manager: u64,
    pub frame: Frame,
    pub lists: *const u64,
    pub list_count: usize,
    pub sets: *const SetView,
    pub set_count: usize,
}
const _: () = assert!(
    core::mem::size_of::<SetView>() == 72
        && core::mem::size_of::<SnapshotView>() == 160
        && core::mem::size_of::<InvocationView>() == 40
);
/// # Safety
/// The view's pointers are valid for their counts.
unsafe fn from_view(v: &SnapshotView) -> Snapshot {
    let slice = |p: *const u64, n: usize| {
        if n == 0 {
            Vec::new()
        } else {
            unsafe { core::slice::from_raw_parts(p, n) }.to_vec()
        }
    };
    let sets = if v.set_count == 0 {
        &[][..]
    } else {
        unsafe { core::slice::from_raw_parts(v.sets, v.set_count) }
    };
    Snapshot {
        base: v.base,
        registry: v.registry,
        thread: v.thread,
        lists_address: v.lists_address,
        sets_address: v.sets_address,
        fence_object: v.fence_object,
        fence: v.fence,
        fence_value: v.fence_value,
        generation: v.generation,
        sync_manager: v.sync_manager,
        frame: v.frame,
        lists: slice(v.lists, v.list_count),
        sets: sets
            .iter()
            .map(|s| SourceSet {
                address: s.address,
                header: s.header,
                entries: slice(s.entries, s.count),
            })
            .collect(),
    }
}
/// A snapshot read for the C++ (its view points into it until cvr_qr_snapshot_free).
struct Held {
    snapshot: Snapshot,
    sets: Vec<SetView>,
}
fn set_rejection(out: *mut u32, code: u32) {
    if !out.is_null() {
        // SAFETY: the caller's slot.
        unsafe { *out = code };
    }
}
fn with_observed<R>(out: *mut [u64; 12], f: impl FnOnce(&mut [u64; 12]) -> R) -> R {
    let mut local = [0u64; 12];
    let r = f(&mut local);
    if !out.is_null() {
        // SAFETY: the caller's array.
        unsafe { *out = local };
    }
    r
}

#[no_mangle]
pub unsafe extern "C" fn cvr_qr_closed(header: *const SetHeader, sync_manager: u64) -> bool {
    // SAFETY: a valid header.
    closed(unsafe { &*header }, sync_manager)
}
/// # Safety
/// `frame` and `view` valid; `rejection` valid or null; `read` reads for `user`. The view stays valid until the
/// returned handle (null on refusal) is freed.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_qr_read_scope(
    read: Option<ReadMemory>,
    user: *mut c_void,
    base: u64,
    thread_id: u64,
    frame: *const Frame,
    queue: u64,
    count: u32,
    lists_address: u64,
    view: *mut SnapshotView,
    rejection: *mut u32,
) -> *mut c_void {
    // SAFETY: as the caller promises.
    let result = read_scope(
        &reader(read, user),
        base,
        thread_id,
        unsafe { &*frame },
        queue,
        count,
        lists_address,
    );
    set_rejection(rejection, result.as_ref().err().copied().unwrap_or(0));
    let Ok(snapshot) = result else {
        return core::ptr::null_mut();
    };
    let sets = snapshot
        .sets
        .iter()
        .map(|s| SetView {
            address: s.address,
            header: s.header,
            entries: s.entries.as_ptr(),
            count: s.entries.len(),
        })
        .collect::<Vec<_>>();
    let held = Box::new(Held { snapshot, sets });
    let s = &held.snapshot;
    // SAFETY: the caller's view.
    unsafe {
        *view = SnapshotView {
            base: s.base,
            registry: s.registry,
            thread: s.thread,
            lists_address: s.lists_address,
            sets_address: s.sets_address,
            fence_object: s.fence_object,
            fence: s.fence,
            fence_value: s.fence_value,
            generation: s.generation,
            sync_manager: s.sync_manager,
            frame: s.frame,
            lists: s.lists.as_ptr(),
            list_count: s.lists.len(),
            sets: held.sets.as_ptr(),
            set_count: held.sets.len(),
        }
    };
    Box::into_raw(held) as *mut c_void
}
/// # Safety
/// A handle cvr_qr_read_scope returned (or null).
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_snapshot_free(handle: *mut c_void) {
    if !handle.is_null() {
        // SAFETY: the box cvr_qr_read_scope made.
        drop(unsafe { Box::from_raw(handle as *mut Held) });
    }
}
/// # Safety
/// Valid views.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_same(a: *const SnapshotView, b: *const SnapshotView) -> bool {
    // SAFETY: as the caller promises.
    unsafe { same(&from_view(&*a), &from_view(&*b)) }
}
/// # Safety
/// `scope` valid; `rejection` and `observed` valid or null; `read` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_qr_contains_recording(
    read: Option<ReadMemory>,
    user: *mut c_void,
    scope: *const SnapshotView,
    context: u64,
    list: u64,
    registry: u64,
    rejection: *mut u32,
    observed: *mut [u64; 12],
) -> bool {
    // SAFETY: as the caller promises.
    let scope = unsafe { from_view(&*scope) };
    let result = with_observed(observed, |o| {
        contains_recording(&reader(read, user), &scope, context, list, registry, o)
    });
    set_rejection(rejection, result.err().unwrap_or(0));
    result.is_ok()
}
/// # Safety
/// As cvr_qr_contains_recording.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_qr_contains(
    read: Option<ReadMemory>,
    user: *mut c_void,
    scope: *const SnapshotView,
    context: u64,
    list: u64,
    registry: u64,
    handle: u32,
    resource: u64,
    rejection: *mut u32,
    observed: *mut [u64; 12],
) -> bool {
    // SAFETY: as the caller promises.
    let scope = unsafe { from_view(&*scope) };
    let result = with_observed(observed, |o| {
        contains(
            &reader(read, user),
            &scope,
            context,
            list,
            registry,
            handle,
            resource,
            o,
        )
    });
    set_rejection(rejection, result.err().unwrap_or(0));
    result.is_ok()
}
/// # Safety
/// `scope` and `output` valid; `read` reads for `user`.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_search_invocation_pageable(
    read: Option<ReadMemory>,
    user: *mut c_void,
    scope: *const SnapshotView,
    source_list: u64,
    pageable: u64,
    output: *mut [u64; 20],
) -> bool {
    // SAFETY: as the caller promises.
    let scope = unsafe { from_view(&*scope) };
    search_invocation_pageable(&reader(read, user), &scope, source_list, pageable, unsafe {
        &mut *output
    })
}
/// # Safety
/// `scope` and `output` valid; `rejection` and `observed` valid or null; `read` reads for `user`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn cvr_qr_read_pageable_membership(
    read: Option<ReadMemory>,
    user: *mut c_void,
    scope: *const SnapshotView,
    context: u64,
    list: u64,
    registry: u64,
    pageable: u64,
    output: *mut PageableMembership,
    rejection: *mut u32,
    observed: *mut [u64; 12],
) -> bool {
    // SAFETY: as the caller promises.
    let scope = unsafe { from_view(&*scope) };
    unsafe { *output = PageableMembership::default() };
    let result = with_observed(observed, |o| {
        read_pageable_membership(
            &reader(read, user),
            &scope,
            context,
            list,
            registry,
            pageable,
            o,
        )
    });
    set_rejection(rejection, result.err().unwrap_or(0));
    match result {
        Ok(value) => {
            // SAFETY: as the caller promises.
            unsafe { *output = value };
            true
        }
        Err(_) => false,
    }
}

#[no_mangle]
pub extern "C" fn cvr_qr_last_failure() -> u32 {
    FAILURE.get()
}
/// # Safety
/// `out` receives 12 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_last_details(out: *mut [u64; 12]) {
    // SAFETY: as the caller promises.
    unsafe { *out = DETAILS.get() };
}
/// # Safety
/// `out` receives 12 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_membership_details(out: *mut [u64; 12]) {
    // SAFETY: as the caller promises.
    unsafe { *out = MEMBERSHIP.get() };
}
/// # Safety
/// `out` receives 20 values.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_pageable_search_details(out: *mut [u64; 20]) {
    // SAFETY: as the caller promises.
    unsafe { *out = SEARCH.get() };
}
/// Capture: `out` receives the proof (a reference for the caller's Scope) or null.
///
/// # Safety
/// `queue` live or null; `out` valid.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_capture(
    queue: *mut c_void,
    count: u32,
    lists: u64,
    out: *mut *mut c_void,
) -> bool {
    let proof = capture(queue, count, lists);
    // SAFETY: as the caller promises.
    unsafe { *out = proof.unwrap_or(core::ptr::null_mut()) };
    proof.is_some()
}
/// # Safety
/// `proof` a Scope's handle or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_validate(proof: *mut c_void) -> bool {
    validate(proof)
}
/// # Safety
/// `proof` a Scope's handle or null; `source` valid or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_contains_recording_runtime(
    proof: *mut c_void,
    source: *const Recording,
) -> bool {
    // SAFETY: as the caller promises.
    contains_recording_runtime(proof, unsafe { source.as_ref() })
}
/// # Safety
/// As cvr_qr_contains_recording_runtime.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_contains_runtime(
    proof: *mut c_void,
    source: *const Recording,
    registry: u64,
    handle: u32,
    resource: u64,
) -> bool {
    // SAFETY: as the caller promises.
    contains_runtime(
        proof,
        unsafe { source.as_ref() },
        registry,
        handle,
        resource,
    )
}
/// # Safety
/// `proof` a Scope's handle or null; `out` valid (its lists borrowed from the proof the caller holds).
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_describe_invocation(
    proof: *mut c_void,
    out: *mut InvocationView,
) -> bool {
    // SAFETY: as the caller promises.
    describe_invocation(proof, unsafe { &mut *out })
}
/// # Safety
/// `proof` a Scope's handle or null; the records valid or null; `physical`, `membership` and `kind` the caller's zeroed
/// Data fields (`physical` receives references).
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_bind_pageable_use(
    proof: *mut c_void,
    source: *const Recording,
    asset: *const ImmutableLease,
    physical: *mut ImmutablePageable,
    membership: *mut PageableMembership,
    kind: *mut u32,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        bind_pageable_use(
            proof,
            source.as_ref(),
            asset.as_ref(),
            &mut *physical,
            &mut *membership,
            &mut *kind,
        )
    }
}
/// # Safety
/// `proof` a Scope's handle or null; the records valid or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_qr_validate_pageable_use(
    proof: *mut c_void,
    source: *const Recording,
    physical: *const ImmutablePageable,
    membership: *const PageableMembership,
) -> bool {
    // SAFETY: as the caller promises.
    unsafe {
        validate_pageable_use(
            proof,
            source.as_ref(),
            physical.as_ref(),
            membership.as_ref(),
        )
    }
}
