//! THE SCENE ACCESS TRACE (formerly src/Render/NativeSceneAccess.cpp's anonymous namespace): one epoch's journal of
//! the command lists' recordings, every resource their commands touch (with a 16-value row of its identity, placement
//! and description), the scene children the commands are attributed to, and -- when the epoch finishes -- the rows it
//! publishes through the CyberpunkVR_SceneAccess* pointers.
//!
//! The journal and its counters are owned here (the C++ half's switches and epoch are references to the counters); each
//! C++ entry point keeps its epoch capture, its attribution (scene_child, which reads the engine) and the observer lock,
//! and forwards its body. The resource's descriptor cookie is descriptor_audit.rs'.
use core::sync::atomic::{AtomicBool, Ordering};
use windows::core::{IUnknown, Interface as _};

use crate::texture_immutable::Creation;

pub const RECORDING_LIMIT: usize = 256;
pub const RESOURCE_LIMIT: usize = 4096;
pub const EVENT_LIMIT: u64 = 65536;
pub const CHILD_LIMIT: usize = 4096;
pub const NO_RESOURCE: u32 = u32::MAX;
const E_PENDING: i32 = 0x8000_000Au32 as i32;

/// scene_access_trace::Kind.
pub mod kind {
    pub const READ: u32 = 1;
    pub const WRITE: u32 = 2;
    pub const TRANSITION: u32 = 3;
    pub const ALIAS: u32 = 4;
    pub const UAV_BARRIER: u32 = 5;
    pub const SHADER: u32 = 6;
    pub const UNKNOWN: u32 = 7;
    pub const TARGETS: u32 = 8;
}

/// scene_access_trace::Event.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub sequence: u64,
    pub offset: u64,
    pub bytes: u64,
    pub auxiliary: u64,
    pub recording: u32,
    pub child: u32,
    pub kind: u32,
    pub resource: u32,
    pub other: u32,
    pub subresource: u32,
    pub before: u32,
    pub after: u32,
    pub flags: u32,
    pub tag: u32,
    pub internal: u32,
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<Event>() == 80);
impl Event {
    pub fn of(k: u32) -> Event {
        Event {
            sequence: 0,
            offset: 0,
            bytes: u64::MAX,
            auxiliary: 0,
            recording: 0,
            child: u32::MAX,
            kind: k,
            resource: NO_RESOURCE,
            other: NO_RESOURCE,
            subresource: u32::MAX,
            before: 0,
            after: 0,
            flags: 0,
            tag: 0,
            internal: 0,
            reserved: 0,
        }
    }
}
/// scene_child::Identity.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Identity {
    pub consumer_id: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub frame_owner: u64,
    pub source_graph: u64,
    pub execution_graph: u64,
    pub root_object: u64,
    pub child_object: u64,
    pub child_vtable: u64,
    pub child_work: u64,
    pub work_context: u64,
    pub invocation: u64,
    pub parent_invocation: u64,
    pub root_handle: u32,
    pub view: u32,
    pub section: u32,
    pub child_slot: u32,
    pub thread: u32,
    pub phase: u32,
    pub status: u32,
    pub reserved: u32,
}
const _: () = assert!(core::mem::size_of::<Identity>() == 136);
/// SameParent: the same scene parent (the child's own object, vtable, work, slot, invocations, thread, phase and status
/// may differ).
pub fn same_parent(a: &Identity, b: &Identity) -> bool {
    a.consumer_id == b.consumer_id
        && a.epoch == b.epoch
        && a.pair_generation == b.pair_generation
        && a.frame_owner == b.frame_owner
        && a.source_graph == b.source_graph
        && a.execution_graph == b.execution_graph
        && a.root_object == b.root_object
        && a.work_context == b.work_context
        && a.root_handle == b.root_handle
        && a.view == b.view
        && a.section == b.section
}
/// The C++ half's attribution of the command: the scene child it describes at the list (matched), the native lease it
/// was recorded on, and whether the command is the port's own.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct At {
    pub child: Identity,
    pub token: u64,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
    pub matched: bool,
    pub internal: bool,
}
const _: () = assert!(core::mem::size_of::<At>() == 184);

/// The trace's counters (the C++ half's switches and epoch are references to them; the rest under its lock).
#[repr(C)]
pub struct Counters {
    pub active: AtomicBool,
    pub unavailable: AtomicBool,
    pub finished: bool,
    pub failure: u32,
    pub epoch: u64,
    pub generation: u64,
    pub sequence: u64,
    pub event_count: u64,
    pub shader_unknown: u64,
    pub unattributed: u64,
    pub missing_metadata: u64,
    pub resets: u64,
    pub overflow: u64,
}
const _: () = assert!(core::mem::size_of::<Counters>() == 80 && core::mem::offset_of!(Counters, epoch) == 8);
impl Counters {
    pub const fn new() -> Counters {
        Counters {
            active: AtomicBool::new(false),
            unavailable: AtomicBool::new(false),
            finished: false,
            failure: 0,
            epoch: 0,
            generation: 0,
            sequence: 0,
            event_count: 0,
            shader_unknown: 0,
            unattributed: 0,
            missing_metadata: 0,
            resets: 0,
            overflow: 0,
        }
    }
}
impl Default for Counters {
    fn default() -> Self {
        Self::new()
    }
}

/// The native recording lease a recording was attributed to: its token's address (with the reference held for it).
#[derive(Default)]
pub struct Native {
    pub token: u64,
    _held: Option<IUnknown>,
    pub generation: u64,
    pub context: u64,
    pub list: u64,
    pub registry: u64,
}
pub struct Recording {
    pub list: u64,
    _list_ref: Option<IUnknown>,
    pub native: Native,
    pub generation: u64,
    pub whole: bool,
    pub closed: bool,
    pub close: i32,
    pub child: u32,
    pub failure: u32,
    pub events: Vec<Event>,
    pub om: [u32; 9],
    pub om_known: bool,
}
/// A held resource: its reference, its creation and its row (pointer, descriptor cookie, creation kind, heap, offset,
/// bytes, dimension, width, height, depth or layers, mips, format, flags, identity known, placement known, 0).
pub struct Held {
    pub pointer: u64,
    _resource: Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>,
    pub creation: Option<Creation>,
    pub row: [u64; 16],
}
/// What the trace learns of a resource the first time it holds it.
pub struct HeldInfo {
    pub resource: Option<windows::Win32::Graphics::Direct3D12::ID3D12Resource>,
    pub creation: Option<Creation>,
    pub row: [u64; 16],
}
pub struct Trace {
    pub recordings: Vec<Recording>,
    pub resources: Vec<Held>,
    pub children: Vec<Identity>,
    pub recording_rows: Vec<[u64; 16]>,
    pub resource_rows: Vec<[u64; 16]>,
    pub event_rows: Vec<Event>,
    /// A reference on an attributed lease's token (the plugin's; none in the tests).
    pub token_ref: fn(u64) -> Option<IUnknown>,
}
fn no_token(_: u64) -> Option<IUnknown> {
    None
}
impl Default for Trace {
    fn default() -> Self {
        Trace {
            recordings: Vec::new(),
            resources: Vec::new(),
            children: Vec::new(),
            recording_rows: Vec::new(),
            resource_rows: Vec::new(),
            event_rows: Vec::new(),
            token_ref: no_token,
        }
    }
}

pub fn fail(c: &mut Counters, reason: u32) {
    if c.failure == 0 {
        c.failure = reason;
    }
}
fn creation_fields(creation: &Option<Creation>) -> (u64, u64, u64, u32) {
    creation.as_ref().map_or((0, 0, 0, 0), |c| (c.heap.as_ref().map_or(0, |h| h.as_raw() as u64), c.offset, c.bytes, c.kind))
}
impl Trace {
    pub fn find(&self, list: u64) -> Option<usize> {
        let mut result: Option<usize> = None;
        for (i, r) in self.recordings.iter().enumerate() {
            if r.list == list && result.is_none_or(|k| r.generation > self.recordings[k].generation) {
                result = Some(i);
            }
        }
        result
    }
    /// New: a fresh recording (whole after a reset, its bound targets then known); it replaces the list's latest one
    /// when that is not a source; none past the limit.
    pub fn new_recording(&mut self, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, whole: bool) -> Option<usize> {
        if list == 0 || c.generation == u64::MAX {
            fail(c, 1);
            return None;
        }
        c.generation += 1;
        let next = Recording {
            list,
            _list_ref: list_ref(),
            native: Native::default(),
            generation: c.generation,
            whole,
            closed: false,
            close: E_PENDING,
            child: u32::MAX,
            failure: 0,
            events: Vec::new(),
            om: [NO_RESOURCE; 9],
            om_known: whole,
        };
        if let Some(old) = self.find(list).filter(|&k| self.recordings[k].native.token == 0) {
            c.event_count -= self.recordings[old].events.len() as u64;
            self.recordings[old] = next;
            return Some(old);
        }
        if self.recordings.len() >= RECORDING_LIMIT {
            c.overflow += 1;
            fail(c, 2);
            return None;
        }
        self.recordings.push(next);
        Some(self.recordings.len() - 1)
    }
    pub fn get(&mut self, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>) -> Option<usize> {
        self.find(list).or_else(|| self.new_recording(c, list, list_ref, false))
    }
    /// Hold: the resource's index, held with its row the first time (an identity or placement not known counted).
    pub fn hold(&mut self, c: &mut Counters, resource: u64, info: &dyn Fn(u64) -> HeldInfo) -> u32 {
        if resource == 0 {
            return NO_RESOURCE;
        }
        if let Some(i) = self.resources.iter().position(|h| h.pointer == resource) {
            return i as u32;
        }
        if self.resources.len() >= RESOURCE_LIMIT {
            c.overflow += 1;
            fail(c, 2);
            return NO_RESOURCE;
        }
        let HeldInfo { resource: held, creation, row } = info(resource);
        if row[13] == 0 || row[14] == 0 {
            c.missing_metadata += 1;
        }
        self.resources.push(Held { pointer: resource, _resource: held, creation, row });
        (self.resources.len() - 1) as u32
    }
    /// HoldBreadcrumb: the destination held; its authenticated creation (a known allocation) upgrades the cached one or
    /// fills a missing placement, a conflicting one refuses.
    pub fn hold_breadcrumb(&mut self, c: &mut Counters, observed: crate::target_access::Resource, creation: &dyn Fn() -> Option<Creation>, info: &dyn Fn(u64) -> HeldInfo) -> u32 {
        let index = self.hold(c, observed.pointer, info);
        if index == NO_RESOURCE {
            return index;
        }
        if !crate::texture_read::known_allocation(&observed.allocation) {
            return index;
        }
        let cached = &mut self.resources[index as usize];
        let (heap, offset, bytes, kind_of) = creation_fields(&cached.creation);
        let prior = crate::target_access::Resource {
            pointer: cached.pointer,
            allocation: crate::texture_read::Allocation { heap, offset, bytes, kind: kind_of },
            flags: cached.row[12] as u32,
        };
        let matched = crate::target_access::compare_authenticated_creation(&prior, &observed);
        if matched == crate::target_access::creation::CONFLICT {
            return NO_RESOURCE;
        }
        if matched == crate::target_access::creation::UPGRADE || cached.row[14] == 0 {
            let missing = cached.row[13] == 0 || cached.row[14] == 0;
            cached.creation = creation();
            let a = &observed.allocation;
            cached.row[2] = u64::from(a.kind);
            cached.row[3] = a.heap;
            cached.row[4] = a.offset;
            cached.row[5] = a.bytes;
            cached.row[14] = 1;
            if missing && cached.row[13] != 0 && c.missing_metadata != 0 {
                c.missing_metadata -= 1;
            }
        }
        index
    }
    /// Child: the attributed scene child's index (its native lease must be the recording's own and its parent the
    /// recording's child's -- else the recording and the trace fail with 3); none unattributed or past the limit.
    pub fn child(&mut self, c: &mut Counters, index: usize, at: &At) -> u32 {
        if !at.matched {
            return u32::MAX;
        }
        let r = &self.recordings[index];
        let native = &r.native;
        if at.token == 0
            || at.generation == 0
            || at.context == 0
            || at.registry == 0
            || at.list != r.list
            || (native.token != 0
                && (native.token != at.token
                    || native.generation != at.generation
                    || native.context != at.context
                    || native.registry != at.registry))
            || (r.child != u32::MAX
                && (r.child as usize >= self.children.len() || !same_parent(&self.children[r.child as usize], &at.child)))
        {
            self.recordings[index].failure = 3;
            fail(c, 3);
            return u32::MAX;
        }
        let held = (self.token_ref)(at.token);
        let r = &mut self.recordings[index];
        r.native = Native { token: at.token, _held: held, generation: at.generation, context: at.context, list: at.list, registry: at.registry };
        if let Some(i) = self.children.iter().position(|ch| *ch == at.child) {
            if r.child == u32::MAX {
                r.child = i as u32;
            }
            return i as u32;
        }
        if self.children.len() >= CHILD_LIMIT {
            c.overflow += 1;
            fail(c, 2);
            return u32::MAX;
        }
        let i = self.children.len() as u32;
        self.children.push(at.child);
        if r.child == u32::MAX {
            r.child = i;
        }
        i
    }
    /// Append: the event numbered, its recording and attributed child set (a closed recording fails with 4; past the
    /// event limit an overflow).
    pub fn append(&mut self, c: &mut Counters, index: usize, at: &At, mut event: Event) {
        if self.recordings[index].closed {
            self.recordings[index].failure = 4;
            fail(c, 4);
            return;
        }
        if c.event_count >= EVENT_LIMIT || c.sequence == u64::MAX {
            c.overflow += 1;
            fail(c, 2);
            return;
        }
        c.sequence += 1;
        event.sequence = c.sequence;
        event.recording = index as u32;
        event.child = self.child(c, index, at);
        event.internal = u32::from(at.internal);
        if event.child == u32::MAX && !at.internal {
            c.unattributed += 1;
        }
        self.recordings[index].events.push(event);
        c.event_count += 1;
    }
    /// The schema 1 stats row.
    pub fn stats_row(&self, c: &Counters) -> [u64; 16] {
        let selected = self.recordings.iter().filter(|r| r.native.token != 0).count() as u64;
        [
            1,
            u64::from(c.active.load(Ordering::Relaxed)),
            u64::from(c.finished),
            c.epoch,
            self.recordings.len() as u64,
            self.resources.len() as u64,
            c.event_count,
            self.children.len() as u64,
            selected,
            c.shader_unknown,
            c.unattributed,
            c.missing_metadata,
            c.resets,
            c.overflow,
            u64::from(c.failure),
            0,
        ]
    }
    /// Arm: a new epoch (the trace emptied, its counts zeroed; an unavailable observer fails with 6 and stays off).
    pub fn arm(&mut self, c: &mut Counters) {
        c.active.store(false, Ordering::SeqCst);
        if c.epoch == u64::MAX {
            c.overflow += 1;
            fail(c, 2);
            return;
        }
        *self = Trace { token_ref: self.token_ref, ..Trace::default() };
        c.epoch += 1;
        c.generation = 0;
        c.sequence = 0;
        c.event_count = 0;
        c.shader_unknown = 0;
        c.unattributed = 0;
        c.missing_metadata = 0;
        c.resets = 0;
        c.overflow = 0;
        c.failure = 0;
        c.finished = false;
        if c.unavailable.load(Ordering::SeqCst) {
            fail(c, 6);
            return;
        }
        c.active.store(true, Ordering::SeqCst);
    }
    /// Finish: the epoch closed (aborted fails with 8; anything but two closed, successful source recordings with 9)
    /// and its rows built: the resources', each recording's with its events' first row and count, the events.
    pub fn finish(&mut self, c: &mut Counters, aborted: bool) {
        c.active.store(false, Ordering::SeqCst);
        c.finished = true;
        if aborted {
            fail(c, 8);
        }
        let selected = self.recordings.iter().filter(|r| r.native.token != 0).count();
        if selected != 2 {
            fail(c, 9);
        }
        for r in &self.recordings {
            if r.native.token != 0 && (!r.closed || r.close < 0) {
                fail(c, 9);
            }
        }
        self.recording_rows.clear();
        self.resource_rows.clear();
        self.event_rows.clear();
        self.resource_rows.extend(self.resources.iter().map(|h| h.row));
        for r in &self.recordings {
            let first = self.event_rows.len() as u64;
            self.event_rows.extend_from_slice(&r.events);
            let child = self.children.get(r.child as usize).copied().unwrap_or_default();
            self.recording_rows.push([
                r.list,
                r.generation,
                u64::from(r.whole),
                u64::from(r.closed),
                u64::from(r.close as u32),
                r.native.generation,
                r.native.token,
                r.native.context,
                r.native.registry,
                u64::from(child.root_handle),
                u64::from(child.view),
                child.consumer_id,
                child.epoch,
                child.pair_generation,
                first,
                r.events.len() as u64,
            ]);
        }
    }
}

// ---- the entry points' bodies (the C++ half captured the epoch and the attribution and holds the lock)

pub fn close(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, result: i32) {
    if let Some(i) = t.get(c, list, list_ref) {
        let r = &mut t.recordings[i];
        r.closed = true;
        r.close = result;
        if result < 0 {
            r.failure = 7;
            fail(c, 7);
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub fn operand(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, resource: u64, write: bool, tag: u32, subresource: u32, offset: u64, bytes: u64, info: &dyn Fn(u64) -> HeldInfo) {
    if let Some(i) = t.get(c, list, list_ref) {
        let mut event = Event::of(if write { kind::WRITE } else { kind::READ });
        event.resource = t.hold(c, resource, info);
        event.offset = offset;
        event.bytes = bytes;
        event.subresource = subresource;
        event.tag = tag;
        if event.resource == NO_RESOURCE {
            event.kind = kind::UNKNOWN;
        }
        t.append(c, i, at, event);
    }
}
pub fn unknown(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, tag: u32) {
    if let Some(i) = t.get(c, list, list_ref) {
        let mut event = Event::of(kind::UNKNOWN);
        event.tag = tag;
        t.append(c, i, at, event);
    }
}
pub fn shader(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, graphics: bool) {
    if let Some(i) = t.get(c, list, list_ref) {
        let mut marker = Event::of(kind::SHADER);
        marker.flags = u32::from(graphics);
        t.append(c, i, at, marker);
        c.shader_unknown += 1;
        if graphics {
            if !t.recordings[i].om_known {
                let mut event = Event::of(kind::UNKNOWN);
                event.tag = 8;
                t.append(c, i, at, event);
            }
            let om = t.recordings[i].om;
            for resource in om.into_iter().filter(|&r| r != NO_RESOURCE) {
                let mut event = Event::of(kind::WRITE);
                event.resource = resource;
                event.tag = 1;
                t.append(c, i, at, event);
            }
        }
    }
}
/// Targets: the bound targets held (unknown when they could not be read), one targets event each.
#[allow(clippy::too_many_arguments)]
pub fn targets(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, known: bool, held: &[u64; 9], info: &dyn Fn(u64) -> HeldInfo) {
    if let Some(i) = t.get(c, list, list_ref) {
        t.recordings[i].om = [NO_RESOURCE; 9];
        t.recordings[i].om_known = known;
        if !known {
            let mut event = Event::of(kind::UNKNOWN);
            event.tag = 8;
            t.append(c, i, at, event);
            return;
        }
        for (n, &resource) in held.iter().enumerate() {
            let index = t.hold(c, resource, info);
            t.recordings[i].om[n] = index;
            if index == NO_RESOURCE {
                continue;
            }
            let mut event = Event::of(kind::TARGETS);
            event.resource = index;
            event.tag = n as u32;
            t.append(c, i, at, event);
        }
    }
}
/// ClearView: each cleared resource written (unknown when the views could not be read, or a resource not held).
#[allow(clippy::too_many_arguments)]
pub fn clear_view(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, known: bool, held: &[u64; 9], info: &dyn Fn(u64) -> HeldInfo) {
    if let Some(i) = t.get(c, list, list_ref) {
        if !known {
            let mut event = Event::of(kind::UNKNOWN);
            event.tag = 6;
            t.append(c, i, at, event);
            return;
        }
        for &resource in held.iter().filter(|&&r| r != 0) {
            let mut event = Event::of(kind::WRITE);
            event.resource = t.hold(c, resource, info);
            event.tag = 6;
            if event.resource == NO_RESOURCE {
                event.kind = kind::UNKNOWN;
            }
            t.append(c, i, at, event);
        }
    }
}
/// One resource barrier as the trace records it.
#[derive(Clone, Copy, Debug)]
pub enum Barrier {
    Transition { resource: u64, subresource: u32, before: u32, after: u32 },
    Aliasing { before: u64, after: u64 },
    Uav { resource: u64 },
    Other,
}
/// Barriers: an unreadable array is one unknown event (tag 5); each barrier one event with its flags.
#[allow(clippy::too_many_arguments)]
pub fn barriers(t: &mut Trace, c: &mut Counters, list: u64, list_ref: &dyn Fn() -> Option<IUnknown>, at: &At, items: Option<&[(Barrier, u32)]>, info: &dyn Fn(u64) -> HeldInfo) {
    if let Some(i) = t.get(c, list, list_ref) {
        let Some(items) = items else {
            let mut event = Event::of(kind::UNKNOWN);
            event.tag = 5;
            t.append(c, i, at, event);
            return;
        };
        for &(barrier, flags) in items {
            let mut event = Event::of(kind::UNKNOWN);
            event.flags = flags;
            match barrier {
                Barrier::Transition { resource, subresource, before, after } => {
                    event.kind = kind::TRANSITION;
                    event.resource = t.hold(c, resource, info);
                    event.subresource = subresource;
                    event.before = before;
                    event.after = after;
                }
                Barrier::Aliasing { before, after } => {
                    event.kind = kind::ALIAS;
                    event.resource = t.hold(c, before, info);
                    event.other = t.hold(c, after, info);
                }
                Barrier::Uav { resource } => {
                    event.kind = kind::UAV_BARRIER;
                    event.resource = t.hold(c, resource, info);
                }
                Barrier::Other => {}
            }
            t.append(c, i, at, event);
        }
    }
}
/// ImmediateWrite's body: an unknown write (tag 13) unless the breadcrumb resolved to a held destination, then a
/// four-byte write at its offset.
#[allow(clippy::too_many_arguments)]
pub fn immediate(
    t: &mut Trace,
    c: &mut Counters,
    list: u64,
    list_ref: &dyn Fn() -> Option<IUnknown>,
    at: &At,
    resolved: Option<(u64, crate::target_access::Resource)>,
    creation: &dyn Fn() -> Option<Creation>,
    info: &dyn Fn(u64) -> HeldInfo,
) {
    if let Some(i) = t.get(c, list, list_ref) {
        let mut event = Event::of(kind::UNKNOWN);
        event.tag = 13;
        if let Some((offset, observed)) = resolved {
            event.resource = t.hold_breadcrumb(c, observed, creation, info);
            if event.resource != NO_RESOURCE {
                event.kind = kind::WRITE;
                event.offset = offset;
                event.bytes = 4;
                event.subresource = 0;
            }
        }
        t.append(c, i, at, event);
    }
}

// ---- the trace's C ABI (src/Render/NativeSceneAccess.cpp's entry points, under its observer lock)

#[cfg_attr(test, allow(dead_code))]
struct State(core::cell::UnsafeCell<Counters>, core::cell::UnsafeCell<Trace>);
// SAFETY: the counters' switches are atomics; everything else is read and written under the C++ half's observer lock.
unsafe impl Sync for State {}
static STATE: State = State(
    core::cell::UnsafeCell::new(Counters::new()),
    core::cell::UnsafeCell::new(Trace {
        recordings: Vec::new(),
        resources: Vec::new(),
        children: Vec::new(),
        recording_rows: Vec::new(),
        resource_rows: Vec::new(),
        event_rows: Vec::new(),
        token_ref: plugin_token_ref,
    }),
);
#[cfg(not(test))]
fn plugin_token_ref(token: u64) -> Option<IUnknown> {
    let raw = token as *mut core::ffi::c_void;
    // SAFETY: the attribution's live lease token; a reference taken for the recording.
    unsafe { IUnknown::from_raw_borrowed(&raw).cloned() }
}
#[cfg(test)]
fn plugin_token_ref(_: u64) -> Option<IUnknown> {
    None
}
/// The counters, for the C++ half's references.
#[no_mangle]
pub extern "C" fn cvr_sa_counters() -> *mut Counters {
    STATE.0.get()
}

#[cfg(not(test))]
pub(crate) mod abi {
    use super::*;
    use core::ffi::c_void;
    use windows::Win32::Graphics::Direct3D12::{
        ID3D12Heap, ID3D12Resource, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_TYPE_ALIASING,
        D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_BARRIER_TYPE_UAV,
    };

    #[allow(non_upper_case_globals)]
    extern "C" {
        static mut CyberpunkVR_SceneAccessStats: [u64; 16];
        static mut CyberpunkVR_SceneAccessRecordings: u64;
        static mut CyberpunkVR_SceneAccessResources: u64;
        static mut CyberpunkVR_SceneAccessEvents: u64;
        static mut CyberpunkVR_SceneAccessChildren: u64;
    }
    fn state() -> (&'static mut Trace, &'static mut Counters) {
        // SAFETY: only under the C++ half's observer lock.
        unsafe { (&mut *STATE.1.get(), &mut *STATE.0.get()) }
    }
    fn list_ref(list: u64) -> impl Fn() -> Option<IUnknown> {
        move || {
            let raw = list as *mut c_void;
            // SAFETY: the caller's live command list; a reference taken for the recording.
            unsafe { IUnknown::from_raw_borrowed(&raw).cloned() }
        }
    }
    /// A resource's row: its pointer, descriptor cookie, creation (registered, else as the receipts observed its
    /// allocation), description and whether the identity and the placement are known.
    fn plugin_info(resource: u64) -> HeldInfo {
        let raw = resource as *mut c_void;
        // SAFETY: the caller's live resource.
        let held = unsafe { ID3D12Resource::from_raw_borrowed(&raw).cloned() };
        // descriptors::GetHeldResourceCookie on a live resource.
        let cookie = if raw.is_null() { 0 } else { crate::descriptor_audit::cvr_sda_audit_held_cookie(raw) };
        let identity = cookie != 0;
        let mut creation = crate::texture_immutable::query_creation(raw);
        if creation.is_none() {
            let (mut kind_of, mut heap, mut offset, mut bytes) = (0u32, core::ptr::null_mut(), 0u64, 0u64);
            // SAFETY: the resource and the outputs; the heap comes with its reference.
            if unsafe { crate::texture_receipts::cvr_tr_query_observed_allocation(raw, &mut kind_of, &mut heap, &mut offset, &mut bytes) } {
                creation = Some(Creation {
                    kind: kind_of,
                    resource: held.clone(),
                    // SAFETY: the reference handed over.
                    heap: (!heap.is_null()).then(|| unsafe { ID3D12Heap::from_raw(heap) }),
                    offset,
                    bytes,
                    ..Default::default()
                });
            }
        }
        let placed = creation.is_some();
        let (heap, offset, bytes, kind_of) = creation_fields(&creation);
        // SAFETY: the live resource's description.
        let desc = held.as_ref().map(|r| unsafe { r.GetDesc() }).unwrap_or_default();
        let row = [
            resource,
            cookie,
            u64::from(kind_of),
            heap,
            offset,
            bytes,
            desc.Dimension.0 as u64,
            desc.Width,
            u64::from(desc.Height),
            u64::from(desc.DepthOrArraySize),
            u64::from(desc.MipLevels),
            desc.Format.0 as u64,
            desc.Flags.0 as u64,
            u64::from(identity),
            u64::from(placed),
            0,
        ];
        HeldInfo { resource: held, creation, row }
    }
    unsafe fn at_of<'a>(at: *const At) -> &'a At {
        // SAFETY: the C++ half's attribution for this call.
        unsafe { &*at }
    }

    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_publish_stats() {
        let (t, c) = state();
        // SAFETY: the row, under the lock.
        unsafe { *(&raw mut CyberpunkVR_SceneAccessStats) = t.stats_row(c) };
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_capture_epoch() -> u64 {
        let (_, c) = state();
        if c.active.load(Ordering::SeqCst) {
            c.epoch
        } else {
            0
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_fail(reason: u32) {
        let (_, c) = state();
        fail(c, reason);
    }
    /// Arm: the published pointers withdrawn, a new epoch.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_arm() {
        let (t, c) = state();
        c.active.store(false, Ordering::SeqCst);
        if c.epoch == u64::MAX {
            c.overflow += 1;
            fail(c, 2);
            return;
        }
        // SAFETY: the pointers, under the lock.
        unsafe {
            *(&raw mut CyberpunkVR_SceneAccessRecordings) = 0;
            *(&raw mut CyberpunkVR_SceneAccessResources) = 0;
            *(&raw mut CyberpunkVR_SceneAccessEvents) = 0;
            *(&raw mut CyberpunkVR_SceneAccessChildren) = 0;
        }
        t.arm(c);
    }
    fn data<T>(v: &[T], capacity: usize) -> u64 {
        if capacity == 0 {
            0
        } else {
            v.as_ptr() as u64
        }
    }
    /// Finish: the rows built and their pointers published.
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_finish(aborted: bool) {
        let (t, c) = state();
        t.finish(c, aborted);
        // SAFETY: the pointers, under the lock; the rows live until the next Arm or Finish.
        unsafe {
            *(&raw mut CyberpunkVR_SceneAccessRecordings) = data(&t.recording_rows, t.recording_rows.capacity());
            *(&raw mut CyberpunkVR_SceneAccessResources) = data(&t.resource_rows, t.resource_rows.capacity());
            *(&raw mut CyberpunkVR_SceneAccessEvents) = data(&t.event_rows, t.event_rows.capacity());
            *(&raw mut CyberpunkVR_SceneAccessChildren) = data(&t.children, t.children.capacity());
        }
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_reset(list: u64) {
        let (t, c) = state();
        c.resets += 1;
        t.new_recording(c, list, &list_ref(list), true);
    }
    #[no_mangle]
    pub extern "C-unwind" fn cvr_sa_close(list: u64, result: i32) {
        let (t, c) = state();
        close(t, c, list, &list_ref(list), result);
    }
    /// # Safety
    /// `at` is the C++ half's attribution.
    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_operand(list: u64, at: *const At, resource: u64, write: bool, tag: u32, subresource: u32, offset: u64, bytes: u64) {
        let (t, c) = state();
        operand(t, c, list, &list_ref(list), unsafe { at_of(at) }, resource, write, tag, subresource, offset, bytes, &plugin_info);
    }
    /// # Safety
    /// As cvr_sa_operand.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_unknown(list: u64, at: *const At, tag: u32) {
        let (t, c) = state();
        unknown(t, c, list, &list_ref(list), unsafe { at_of(at) }, tag);
    }
    /// # Safety
    /// As cvr_sa_operand.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_shader(list: u64, at: *const At, graphics: bool) {
        let (t, c) = state();
        shader(t, c, list, &list_ref(list), unsafe { at_of(at) }, graphics);
    }
    /// # Safety
    /// As cvr_sa_operand; `held` holds nine resources or nulls.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_targets(list: u64, at: *const At, known: bool, held: *const [u64; 9]) {
        let (t, c) = state();
        targets(t, c, list, &list_ref(list), unsafe { at_of(at) }, known, unsafe { &*held }, &plugin_info);
    }
    /// # Safety
    /// As cvr_sa_targets.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_clear_view(list: u64, at: *const At, known: bool, held: *const [u64; 9]) {
        let (t, c) = state();
        clear_view(t, c, list, &list_ref(list), unsafe { at_of(at) }, known, unsafe { &*held }, &plugin_info);
    }
    /// # Safety
    /// As cvr_sa_operand; `items` holds `count` barriers or is null.
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_barriers(list: u64, at: *const At, count: u32, items: *const D3D12_RESOURCE_BARRIER) {
        let (t, c) = state();
        let raw = |r: &core::mem::ManuallyDrop<Option<ID3D12Resource>>| r.as_ref().map_or(0, |r| r.as_raw() as u64);
        let read: Option<Vec<(Barrier, u32)>> = (count == 0 || !items.is_null()).then(|| {
            (0..count as usize)
                .map(|n| {
                    // SAFETY: the caller's array; the type selects the union member.
                    unsafe {
                        let b = &*items.add(n);
                        let barrier = match b.Type {
                            D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => {
                                let x = &b.Anonymous.Transition;
                                Barrier::Transition {
                                    resource: raw(&x.pResource),
                                    subresource: x.Subresource,
                                    before: x.StateBefore.0 as u32,
                                    after: x.StateAfter.0 as u32,
                                }
                            }
                            D3D12_RESOURCE_BARRIER_TYPE_ALIASING => {
                                let x = &b.Anonymous.Aliasing;
                                Barrier::Aliasing { before: raw(&x.pResourceBefore), after: raw(&x.pResourceAfter) }
                            }
                            D3D12_RESOURCE_BARRIER_TYPE_UAV => Barrier::Uav { resource: raw(&b.Anonymous.UAV.pResource) },
                            _ => Barrier::Other,
                        };
                        (barrier, b.Flags.0 as u32)
                    }
                })
                .collect()
        });
        barriers(t, c, list, &list_ref(list), unsafe { at_of(at) }, read.as_deref(), &plugin_info);
    }
    /// ImmediateWrite's body; when `resolved`, the breadcrumb's destination offset and its creation's raw values.
    ///
    /// # Safety
    /// As cvr_sa_operand; the resource and heap live or null.
    #[allow(clippy::too_many_arguments)]
    #[no_mangle]
    pub unsafe extern "C-unwind" fn cvr_sa_immediate(
        list: u64,
        at: *const At,
        resolved: bool,
        offset: u64,
        resource: u64,
        heap: u64,
        heap_offset: u64,
        bytes: u64,
        kind_of: u32,
        flags: u32,
    ) {
        let (t, c) = state();
        let observed = crate::target_access::Resource {
            pointer: resource,
            allocation: crate::texture_read::Allocation { heap, offset: heap_offset, bytes, kind: kind_of },
            flags,
        };
        let creation = || {
            let (r, h) = (resource as *mut c_void, heap as *mut c_void);
            // SAFETY: the snapshot's live resource and heap; references taken for the trace.
            let (r, h) = unsafe { (ID3D12Resource::from_raw_borrowed(&r).cloned(), ID3D12Heap::from_raw_borrowed(&h).cloned()) };
            Some(Creation { kind: kind_of, resource: r, heap: h, offset: heap_offset, bytes, ..Default::default() })
        };
        immediate(t, c, list, &list_ref(list), unsafe { at_of(at) }, resolved.then_some((offset, observed)), &creation, &plugin_info);
    }
}

#[cfg(test)]
mod tests {
    //! The trace: recordings started, replaced and refused; resources held with their rows; children attributed and
    //! refused; events numbered; the targets, shader and barrier bodies; the rows Finish builds; the stats row.
    use super::*;

    const LIST: u64 = 0x1000;
    fn none() -> Option<IUnknown> {
        None
    }
    fn info(resource: u64) -> HeldInfo {
        let mut row = [0u64; 16];
        row[0] = resource;
        row[13] = 1;
        row[14] = u64::from(resource != 0xBAD);
        HeldInfo { resource: None, creation: None, row }
    }
    fn at(view: u32) -> At {
        At {
            child: Identity { consumer_id: 1, view, root_handle: 7, ..Default::default() },
            token: 0,
            generation: 0,
            context: 0,
            list: LIST,
            registry: 0,
            matched: false,
            internal: false,
        }
    }

    #[test]
    fn recordings_started_replaced_and_limited() {
        let (mut t, mut c) = (Trace::default(), Counters::default());
        let i = t.get(&mut c, LIST, &none).unwrap();
        assert!(!t.recordings[i].whole && !t.recordings[i].om_known && t.recordings[i].close == E_PENDING);
        unknown(&mut t, &mut c, LIST, &none, &at(0), 3);
        assert_eq!(c.event_count, 1);
        // A reset replaces a non-source recording in place, its events no longer counted.
        let j = t.new_recording(&mut c, LIST, &none, true).unwrap();
        assert_eq!((i, j, c.event_count, c.generation), (0, 0, 0, 2));
        assert!(t.recordings[j].whole && t.recordings[j].om_known);
        assert!(t.new_recording(&mut c, 0, &none, false).is_none());
        assert_eq!(c.failure, 1);
        for n in 1..RECORDING_LIMIT as u64 {
            t.get(&mut c, 0x10_0000 + n, &none).unwrap();
        }
        assert!(t.get(&mut c, 0x20_0000, &none).is_none());
        assert_eq!(c.overflow, 1);
    }

    #[test]
    fn events_numbered_attributed_and_closed() {
        let (mut t, mut c) = (Trace::default(), Counters::default());
        let mut a = at(1);
        a.matched = true;
        a.token = 0x7000;
        a.generation = 3;
        a.context = 4;
        a.registry = 5;
        operand(&mut t, &mut c, LIST, &none, &a, 0xA0, true, 9, 0, 16, 32, &info);
        operand(&mut t, &mut c, LIST, &none, &at(0), 0xBAD, false, 1, 0, 0, u64::MAX, &info);
        let r = &t.recordings[0];
        assert_eq!((r.events[0].sequence, r.events[0].child, r.events[0].kind, r.events[0].bytes), (1, 0, kind::WRITE, 32));
        assert_eq!((r.events[1].sequence, r.events[1].child), (2, u32::MAX));
        assert_eq!((c.unattributed, c.missing_metadata, t.children.len(), r.child), (1, 1, 1, 0));
        assert_eq!(t.recordings[0].native.token, 0x7000);
        // Another parent on the same recording fails it.
        let mut other = a;
        other.child.view = 0;
        unknown(&mut t, &mut c, LIST, &none, &other, 2);
        assert_eq!((t.recordings[0].failure, c.failure), (3, 3));
        close(&mut t, &mut c, LIST, &none, -1);
        unknown(&mut t, &mut c, LIST, &none, &a, 2);
        assert_eq!(t.recordings[0].failure, 4, "a closed recording fails with 4");
        assert_eq!(t.recordings[0].events.len(), 3, "a closed recording takes no more");
    }

    #[test]
    fn targets_shader_and_barriers() {
        let (mut t, mut c) = (Trace::default(), Counters::default());
        let mut held = [0u64; 9];
        held[0] = 0xC0;
        held[8] = 0xC8;
        targets(&mut t, &mut c, LIST, &none, &at(0), true, &held, &info);
        shader(&mut t, &mut c, LIST, &none, &at(0), true);
        let kinds: Vec<(u32, u32, u32)> = t.recordings[0].events.iter().map(|e| (e.kind, e.resource, e.tag)).collect();
        assert_eq!(
            kinds,
            [(kind::TARGETS, 0, 0), (kind::TARGETS, 1, 8), (kind::SHADER, NO_RESOURCE, 0), (kind::WRITE, 0, 1), (kind::WRITE, 1, 1)]
        );
        assert_eq!(c.shader_unknown, 1);
        barriers(&mut t, &mut c, LIST, &none, &at(0), None, &info);
        barriers(
            &mut t,
            &mut c,
            LIST,
            &none,
            &at(0),
            Some(&[(Barrier::Aliasing { before: 0xC0, after: 0xD0 }, 0), (Barrier::Other, 2)]),
            &info,
        );
        let last: Vec<(u32, u32, u32, u32)> = t.recordings[0].events[5..].iter().map(|e| (e.kind, e.resource, e.other, e.tag)).collect();
        assert_eq!(last, [(kind::UNKNOWN, NO_RESOURCE, NO_RESOURCE, 5), (kind::ALIAS, 0, 2, 0), (kind::UNKNOWN, NO_RESOURCE, NO_RESOURCE, 0)]);
    }

    #[test]
    fn finish_rows_and_stats() {
        let (mut t, mut c) = (Trace::default(), Counters::default());
        t.arm(&mut c);
        assert!(c.active.load(Ordering::Relaxed) && c.epoch == 1);
        operand(&mut t, &mut c, LIST, &none, &at(0), 0xA0, false, 0, 0, 0, 4, &info);
        let stats = t.stats_row(&c);
        assert_eq!((stats[0], stats[1], stats[3], stats[4], stats[5], stats[6], stats[8]), (1, 1, 1, 1, 1, 1, 0));
        t.finish(&mut c, false);
        assert_eq!(c.failure, 9, "not the two sources");
        assert!(!c.active.load(Ordering::Relaxed) && c.finished);
        assert_eq!(t.recording_rows[0][..2], [LIST, 1]);
        assert_eq!((t.recording_rows[0][14], t.recording_rows[0][15], t.event_rows.len(), t.resource_rows[0][0]), (0, 1, 1, 0xA0));
        c.unavailable.store(true, Ordering::Relaxed);
        t.arm(&mut c);
        assert!(!c.active.load(Ordering::Relaxed) && c.failure == 6 && c.epoch == 2);
    }
}
