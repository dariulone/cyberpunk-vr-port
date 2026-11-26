//! The owned graph's resource replay: its declarations captured before the compiler, every native resource command
//! of its nodes checked against them (the node payloads, the asynchronous resource contexts and workers carried to
//! the node that runs them), the pool's reset sealed, and the receipts published.

use super::consumers::owned;
use super::*;
use crate::native::graph_snapshot::field_u32;
use crate::native::resource_uses::SLOT_STRIDE;
use crate::resource_replay::{self as rr, NodeBinding};

/// The queue/node scopes of the replay on this thread.
#[derive(Clone, Default)]
pub(crate) struct ReplayNode {
    pub consumer: Option<ConsumerRef>,
    pub identity: rr::Identity,
    pub node: NodeBinding,
}
pub(crate) struct ReplayPayload {
    pub node: ReplayNode,
    pub bytes: [u8; 40],
}
pub(crate) struct AsyncReplay {
    pub node: ReplayNode,
    pub context: [u8; 72],
    pub kind: u32,
}
const SCOPES: usize = 32;
thread_local! {
    static REPLAY_NODES: core::cell::RefCell<Vec<ReplayNode>> = core::cell::RefCell::new(vec![ReplayNode::default(); SCOPES]);
    static REPLAY_NODE_DEPTH: Cell<u32> = const { Cell::new(0) };
    static REPLAY_QUEUES: core::cell::RefCell<Vec<Option<ConsumerRef>>> = core::cell::RefCell::new(vec![None; SCOPES]);
    static REPLAY_QUEUE_DEPTH: Cell<u32> = const { Cell::new(0) };
}
fn enabled() -> bool {
    x::CyberpunkVR_OwnedResourceReplayEnable.load(SeqCst) != 0
}
fn node_depth() -> u32 {
    REPLAY_NODE_DEPTH.with(Cell::get)
}
fn top_node() -> Option<ReplayNode> {
    let depth = node_depth();
    (depth != 0 && depth as usize <= SCOPES).then(|| REPLAY_NODES.with(|nodes| nodes.borrow()[depth as usize - 1].clone()))
}
fn stats_array<const N: usize>(p: *mut [u64; N]) -> &'static mut [u64; N] {
    // SAFETY: an export array.
    unsafe { out(p) }
}

/// ReplayIdentity.
pub(crate) fn replay_identity(s: &mut State, consumer: &Consumer) -> rr::Identity {
    let identity = owned(s, consumer).map(|state| state.identity).unwrap_or_default();
    rr::Identity { consumer_id: identity.consumer_id, epoch: identity.epoch, generation: identity.pair_generation, pool: consumer.resource_pool, source_graph: identity.source_graph, execution_graph: identity.execution_graph }
}
/// PublishReplay.
pub(crate) fn publish_replay(s: &mut State, consumer: &mut Consumer) {
    let Some(replay) = consumer.resource_replay.as_ref() else { return };
    let identity = *replay.binding();
    let (mut entered, mut finished) = (0u64, 0u64);
    for row in &mut consumer.resource_replay_rows {
        row[5] = replay.cursor(row[2]);
        entered += (row[6] != 0) as u64;
        finished += (row[7] != 0) as u64;
    }
    let state = if consumer.resource_replay_failure != 0 || replay.is_failed() {
        4
    } else if replay.complete() {
        3
    } else {
        2
    };
    let stats = [
        state,
        identity.consumer_id,
        identity.epoch,
        identity.generation,
        identity.pool,
        identity.execution_graph,
        identity.source_graph,
        replay.declarations().len() as u64,
        replay.completed_calls(),
        replay.request_difference_count(),
        entered,
        finished,
        u64::from(if consumer.resource_replay_failure != 0 { consumer.resource_replay_failure } else { replay.rejection() }),
        u64::from(consumer.resource_replay_slot),
        u64::from(consumer.resource_replay_cursor),
        u64::from(consumer.resource_replay_misses),
    ];
    *stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayStats)) = stats;
    let proof = [
        identity.consumer_id,
        consumer.resource_replay_compiler_checked as u64,
        consumer.resource_replay_terminal_checked as u64,
        consumer.resource_replay_rows.len() as u64,
        identity.epoch,
        identity.generation,
        identity.pool,
        0,
    ];
    *stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayProof)) = proof;
    s.published_replay_rows = consumer.resource_replay_rows.clone();
    // SAFETY: the export.
    unsafe { x::CyberpunkVR_OwnedResourceReplayRows = data(&s.published_replay_rows) };
}
/// ReplayFailed.
pub(crate) fn replay_failed(s: &mut State, consumer: &mut Consumer, failure: u32, slot: u32, cursor: u32) {
    if consumer.resource_replay_failure == 0 {
        consumer.resource_replay_failure = failure;
        consumer.resource_replay_slot = slot;
        consumer.resource_replay_cursor = cursor;
    }
    let _write = TelemetryWrite::new();
    publish_replay(s, consumer);
}
fn failed(s: &mut State, consumer: &ConsumerRef, failure: u32, slot: u32, cursor: u32) {
    // SAFETY: under the mutex.
    replay_failed(s, unsafe { consumer.get() }, failure, slot, cursor);
}
/// CaptureReplay.
pub(crate) fn capture_replay(s: &mut State, consumer: &mut Consumer) -> bool {
    consumer.resource_replay = Some(Box::default());
    let async_stats = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayAsync));
    async_stats.fill(0);
    async_stats[0] = consumer.ticket().id();
    stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayMismatch)).fill(0);
    let identity = replay_identity(s, consumer);
    let nodes: Vec<NodeBinding> = consumer
        .execution_graph
        .nodes
        .iter()
        .map(|node| NodeBinding { handle: node.handle & 0xDFFF_FFFF, ordinal: node.order, slot: u32::from(node.bytes[29]), view_index: field_u32(&node.bytes, 24) })
        .collect();
    let (mut before, mut after) = ([0u32; rr::SLOTS], [0u32; rr::SLOTS]);
    let a = crate::definition_replay::capture(identity.pool, &mut before);
    let b = a.as_ref().and_then(|_| crate::definition_replay::capture(identity.pool, &mut after));
    let again = replay_identity(s, consumer);
    let captured = match (&a, &b) {
        (Some(a), Some(b)) => consumer.resource_replay.as_mut().expect("set").capture(&identity, &again, &nodes, &before, &after, a, b, rr::RECORD_REQUEST_DIFFERENCE),
        _ => false,
    };
    if !captured {
        let replay = consumer.resource_replay.as_ref().expect("set");
        let failure = if replay.is_failed() { replay.rejection() } else { 100 };
        replay_failed(s, consumer, failure, 0, 0);
        return false;
    }
    for node in &nodes {
        consumer.resource_replay_rows.push([node.handle, node.ordinal, node.slot, node.view_index, before[node.slot as usize], 0, 0, 0]);
    }
    let _write = TelemetryWrite::new();
    publish_replay(s, consumer);
    true
}
/// ReplayGrammarUnchanged.
pub(crate) fn replay_grammar_unchanged(consumer: &Consumer) -> bool {
    let Some(replay) = consumer.resource_replay.as_ref().filter(|replay| replay.is_captured()) else { return false };
    let declared: [u32; rr::SLOTS] = core::array::from_fn(|slot| replay.declared_count(slot as u32));
    crate::definition_replay::grammar_unchanged(replay.binding().pool, &declared, replay.declarations())
}

// ---- the resource command (0x334A34)
static mut RESOURCE_COMMAND: usize = 0;
extern "C" fn resource_command_hook(pool: *mut c_void, command: *const c_void, slot: u8) -> usize {
    let original = original!(RESOURCE_COMMAND: fn(*mut c_void, *const c_void, u8) -> usize);
    if !enabled() || closing() {
        return original(pool, command, slot);
    }
    let mut consumer: Option<ConsumerRef> = None;
    let mut token = rr::Token::default();
    let mut node = NodeBinding::default();
    let mut identity = rr::Identity::default();
    let mut cursor = 0u32;
    let mut observed = false;
    let slot32 = u32::from(slot);
    {
        let g = lock();
        let s = g.state();
        if let Some(scope) = top_node().filter(|scope| scope.consumer.is_some() && scope.identity.pool == pool as u64) {
            let value = scope.consumer.clone().expect("checked");
            consumer = Some(value.clone());
            node = scope.node;
            identity = scope.identity;
            // SAFETY: under the mutex.
            let c = unsafe { value.get() };
            let mut bytes = [0u8; 48];
            let mode = read_u8(identity.pool + 2);
            let mut valid = node.slot == slot32 && mode == Some(2);
            if valid {
                match read_u32(pool as u64 + u64::from(slot) * SLOT_STRIDE + 0x40) {
                    Some(value) => cursor = value,
                    None => valid = false,
                }
            }
            valid = valid && read(command as u64, &mut bytes[..1]);
            let payload = if valid { rr::payload_bytes(u32::from(bytes[0])) } else { u32::MAX };
            valid = valid && payload != u32::MAX && read(command as u64 + 4, &mut bytes[4..8]) && (payload == 0 || read(command as u64 + 8, &mut bytes[8..8 + payload as usize]));
            // 1F0ED0 writes the explicit R8 slot, not the request's scratch byte1.
            bytes[1] = slot;
            if valid {
                let length = 8 + payload as usize;
                if let Some(found) = c.resource_replay.as_mut().and_then(|replay| replay.before(&identity, &node, u32::from(mode.unwrap_or(0)), cursor, &bytes[..length])) {
                    token = found;
                    observed = true;
                }
            }
            if !observed {
                let _write = TelemetryWrite::new();
                let mismatch = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayMismatch));
                if mismatch[0] == 0 {
                    if let Some(replay) = c.resource_replay.as_ref() {
                        let offset: usize = (0..slot32).map(|i| replay.declared_count(i) as usize).sum();
                        mismatch[0] = identity.consumer_id;
                        mismatch[1] = slot32.into();
                        mismatch[2] = cursor.into();
                        mismatch[3] = replay.rejection().into();
                        if let Some(row) = replay.declarations().get(offset + cursor as usize) {
                            for (i, chunk) in row.chunks_exact(8).enumerate() {
                                mismatch[4 + i] = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
                            }
                        }
                        for (i, chunk) in bytes.chunks_exact(8).enumerate() {
                            mismatch[10 + i] = u64::from_le_bytes(chunk.try_into().expect("eight bytes"));
                        }
                    }
                }
                let failure = if valid { c.resource_replay.as_ref().map_or(101, |replay| replay.rejection()) } else { 101 };
                replay_failed(s, c, failure, slot32, cursor);
            }
        }
        if consumer.is_none() {
            let values: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
            for value in values {
                // SAFETY: under the mutex.
                let c = unsafe { value.get() };
                if c.resource_replay.as_ref().is_some_and(|replay| replay.is_captured()) && c.resource_pool == pool as u64 {
                    if read_u8(c.resource_pool + 2) == Some(2) {
                        c.resource_replay_misses += 1;
                        replay_failed(s, c, 102, slot32, 0);
                    }
                    break;
                }
            }
        }
    }
    let result = original(pool, command, slot);
    if observed {
        let g = lock();
        let s = g.state();
        let _write = TelemetryWrite::new();
        let value = consumer.expect("observed");
        // SAFETY: under the mutex.
        let c = unsafe { value.get() };
        let after = read_u32(identity.pool + u64::from(slot) * SLOT_STRIDE + 0x40);
        let ok = after.is_some_and(|after| c.resource_replay.as_mut().is_some_and(|replay| replay.after(&token, &identity, &node, after, result as u8)));
        if !ok {
            replay_failed(s, c, 104, slot32, after.unwrap_or(0));
        }
        publish_replay(s, c);
    }
    result
}

// ---- the queue scopes (called by the paired frame's queue hook)
/// BeginResourceQueue.
pub fn begin_resource_queue(context: u64, graph: u64) -> u64 {
    if !enabled() || closing() {
        return 0;
    }
    let Some(owner) = read_u64(context) else { return 0 };
    let g = lock();
    let s = g.state();
    let values: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for value in values {
        // SAFETY: under the mutex.
        let c = unsafe { value.get() };
        let Some(state) = owned(s, c) else { continue };
        if state.identity.execution_graph != graph || c.ticket().frame_owner() != owner {
            continue;
        }
        let _write = TelemetryWrite::new();
        let registration = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayRegistration));
        registration.fill(0);
        registration[0] = c.ticket().id();
        registration[1] = 1;
        if c.resource_wrapper == 0 || c.resource_pool == 0 {
            let renderer = read_u64(base() + RENDERER_GLOBAL_RVA);
            let wrapper = renderer.filter(|&renderer| renderer == c.renderer).and_then(|renderer| read_u64(renderer + 0x98)).filter(|&wrapper| wrapper != 0);
            let pool = wrapper.and_then(read_u64).filter(|&pool| pool != 0);
            let (Some(wrapper), Some(pool)) = (wrapper, pool) else {
                replay_failed(s, c, 123, 0, 0);
                return 0;
            };
            c.resource_wrapper = wrapper;
            c.resource_pool = pool;
        }
        let mode = if c.resource_pool != 0 { read_u8(c.resource_pool + 2) } else { None };
        let Some(mode) = mode else {
            replay_failed(s, c, 123, 0, 0);
            return 0;
        };
        registration[2] = u64::from(mode);
        let depth = REPLAY_QUEUE_DEPTH.with(Cell::get);
        if depth as usize == SCOPES {
            replay_failed(s, c, 110, 0, 0);
            return 0;
        }
        let mut actual = Snapshot::default();
        if !actual.capture(graph, &mut reader()) || actual.edges != c.execution_graph.edges || actual.nodes.len() != c.execution_graph.nodes.len() {
            replay_failed(s, c, 111, 0, 0);
            return 0;
        }
        if actual.nodes.iter().zip(&c.execution_graph.nodes).any(|(a, b)| a.handle != b.handle || a.bytes != b.bytes) {
            replay_failed(s, c, 116, 0, 0);
            return 0;
        }
        REPLAY_QUEUES.with(|queues| queues.borrow_mut()[depth as usize] = Some(value.clone()));
        REPLAY_QUEUE_DEPTH.with(|d| d.set(depth + 1));
        return u64::from(depth + 1);
    }
    0
}
/// EndResourceQueue.
pub fn end_resource_queue(scope: u64) {
    if scope == 0 {
        return;
    }
    let g = lock();
    let s = g.state();
    let depth = REPLAY_QUEUE_DEPTH.with(Cell::get);
    if scope != u64::from(depth) || scope as usize > SCOPES {
        let queued: Vec<ConsumerRef> = REPLAY_QUEUES.with(|queues| queues.borrow_mut().iter_mut().filter_map(Option::take).collect());
        for value in &queued {
            failed(s, value, 118, 0, 0);
        }
        REPLAY_QUEUE_DEPTH.with(|d| d.set(0));
        drop(g);
        drop(queued);
        return;
    }
    let released = REPLAY_QUEUES.with(|queues| queues.borrow_mut()[depth as usize - 1].take());
    REPLAY_QUEUE_DEPTH.with(|d| d.set(depth - 1));
    drop(g);
    drop(released);
}

// ---- the pool's reset (called by the paired frame's compile hook)
/// BeforeResourceReset.
pub fn before_resource_reset(pool: u64, mode: u8, caller: u64) -> u64 {
    if mode != 3 || !enabled() {
        return 0;
    }
    let g = lock();
    let s = g.state();
    let Some(scope) = top_node() else { return 0 };
    let Some(value) = scope.consumer.clone().filter(|_| scope.identity.pool == pool) else { return 0 };
    // SAFETY: under the mutex.
    let c = unsafe { value.get() };
    let _write = TelemetryWrite::new();
    let base = base();
    let node = c.execution_graph.nodes.get(scope.node.ordinal as usize).copied();
    let mut valid = caller == base + 0xC28A38
        && node.is_some_and(|node| node.work == base + 0xC28A18 && node.vtable == base + 0x36D05B0)
        && c.resource_replay.as_ref().is_some_and(|replay| replay.complete())
        && c.resource_replay_compiler_checked
        && replay_grammar_unchanged(c);
    let a = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayAsync));
    valid = valid && a[0] == scope.identity.consumer_id && a[1] == a[2] && a[2] == a[5] && a[3] == 0 && a[4] == 0;
    for row in &c.resource_replay_rows {
        valid = valid && row[6] == 1 && row[7] == u32::from(row[1] != scope.node.ordinal);
        valid = valid && read_u32(pool + u64::from(row[2]) * SLOT_STRIDE + 0x40) == Some(row[4]);
    }
    let reset = [scope.identity.consumer_id, u64::from(scope.node.handle), u64::from(scope.node.ordinal), u64::from(scope.node.slot), caller.wrapping_sub(base), valid as u64, 0, 0];
    *stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayReset)) = reset;
    c.resource_replay_reset_sealed = valid;
    if !valid {
        replay_failed(s, c, 124, scope.node.slot, 0);
    } else {
        publish_replay(s, c);
    }
    scope.identity.consumer_id
}
/// AfterResourceReset.
pub fn after_resource_reset(consumer: u64, pool: u64, mode: u8) {
    if consumer == 0 || mode != 3 {
        return;
    }
    let g = lock();
    let s = g.state();
    let values: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for value in values {
        // SAFETY: under the mutex.
        let c = unsafe { value.get() };
        if c.ticket().id() != consumer || c.resource_pool != pool {
            continue;
        }
        let _write = TelemetryWrite::new();
        c.resource_replay_reset_returned = c.resource_replay_reset_sealed && read_u8(pool + 2) == Some(3);
        // SAFETY: the export.
        unsafe { x::CyberpunkVR_OwnedResourceReplayReset[6] = c.resource_replay_reset_returned as u64 };
        if !c.resource_replay_reset_returned {
            replay_failed(s, c, 125, 0, 0);
        }
        publish_replay(s, c);
        return;
    }
}
/// EndResourceNode.
fn end_resource_node(scope: u32) {
    if scope == 0 {
        return;
    }
    let g = lock();
    let s = g.state();
    let depth = node_depth();
    if scope != depth || scope as usize > SCOPES {
        let nodes: Vec<ReplayNode> = REPLAY_NODES.with(|nodes| {
            let mut nodes = nodes.borrow_mut();
            let taken: Vec<ReplayNode> = nodes.iter().take((depth as usize).min(SCOPES)).cloned().collect();
            nodes.iter_mut().for_each(|node| *node = ReplayNode::default());
            taken
        });
        for node in &nodes {
            if let Some(value) = node.consumer.as_ref() {
                failed(s, value, 115, 0, 0);
            }
        }
        REPLAY_NODE_DEPTH.with(|d| d.set(0));
        drop(g);
        drop(nodes);
        return;
    }
    let value = REPLAY_NODES.with(|nodes| core::mem::take(&mut nodes.borrow_mut()[depth as usize - 1]));
    REPLAY_NODE_DEPTH.with(|d| d.set(depth - 1));
    if let Some(consumer) = value.consumer.as_ref() {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if let Some(row) = c.resource_replay_rows.get_mut(value.node.ordinal as usize) {
            row[7] += 1;
            let _write = TelemetryWrite::new();
            publish_replay(s, c);
        }
    }
    drop(g);
    drop(value);
}

// ---- the asynchronous resource context (the asm thunk's call after 0x333374)
/// ObserveResourceContext.
fn observe_resource_context(destination: u64, input_context: u64, caller: u64) {
    let base = base();
    let kind = if caller == base + 0xB62798 {
        1
    } else if caller == base + 0xAF0DAE {
        2
    } else if caller == base + 0xB53924 {
        3
    } else {
        0
    };
    let mut receipt: Option<AsyncReplay> = None;
    if kind != 0 && enabled() {
        if let Some(parent) = top_node() {
            let g = lock();
            let s = g.state();
            if let Some(value) = parent.consumer.as_ref() {
                // SAFETY: under the mutex.
                let c = unsafe { value.get() };
                let mut owner = [0u8; 16];
                let matches = read(input_context, &mut owner)
                    && u64::from_le_bytes(owner[..8].try_into().expect("eight")) == c.ticket().frame_owner()
                    && u64::from_le_bytes(owner[8..].try_into().expect("eight")) == c.resource_wrapper
                    && read_u8(input_context + 0x30).is_some_and(|flags| flags & 2 != 0)
                    && read_u8(input_context + 0x38) == Some(parent.node.slot as u8)
                    && parent.node.slot <= 0xFF;
                if matches {
                    receipt = Some(AsyncReplay { node: parent.clone(), context: [0; 72], kind });
                } else {
                    replay_failed(s, c, 130, 0, 0);
                }
            }
        }
    }
    let Some(mut receipt) = receipt else { return };
    let g = lock();
    let s = g.state();
    let value = receipt.node.consumer.clone().expect("matched");
    let payload = destination.wrapping_sub(8);
    let mut second = [0u8; 72];
    if !read(destination, &mut receipt.context) || !read(destination, &mut second) || second != receipt.context || s.async_replay_payloads.len() >= 4096 || s.async_replay_payloads.contains_key(&payload) {
        failed(s, &value, 131, 0, 0);
    } else {
        s.async_replay_payloads.insert(payload, receipt);
        let _write = TelemetryWrite::new();
        let a = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayAsync));
        a[1] += 1;
        a[3] = s.async_replay_payloads.len() as u64;
    }
}
/// The asm thunk's call after the asynchronous resource context was built (CvrResourceContextAfter).
#[no_mangle]
pub extern "C" fn CvrResourceContextAfter(destination: *mut c_void, source: *const c_void, caller: u64) {
    if x::CyberpunkVR_OwnedResourceReplayEnable.load(Relaxed) == 0 {
        return;
    }
    observe_resource_context(destination as u64, source as u64, caller);
}

// ---- the asynchronous workers
static mut PASS_END: usize = 0;
static mut DEFINITION_WORKER: usize = 0;
static mut RANGE_WORKER: usize = 0;
/// ResourceWorkerRun.
fn resource_worker_run(payload: *mut c_void, worker: *mut c_void, kind: u32, original: usize) {
    let mut scope = 0u32;
    if enabled() {
        let g = lock();
        let s = g.state();
        if let Some(receipt) = s.async_replay_payloads.remove(&(payload as u64)) {
            let value = receipt.node.consumer.clone().expect("a receipt's consumer");
            // SAFETY: under the mutex.
            let c = unsafe { value.get() };
            let _write = TelemetryWrite::new();
            let a = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayAsync));
            a[3] = s.async_replay_payloads.len() as u64;
            let (mut context, mut again) = ([0u8; 72], [0u8; 72]);
            let ok = receipt.kind == kind
                && read(payload as u64 + 8, &mut context)
                && read(payload as u64 + 8, &mut again)
                && context == receipt.context
                && again == context
                && c.resource_replay.as_ref().is_some_and(|replay| *replay.binding() == receipt.node.identity)
                && read_u8(c.resource_pool + 2) == Some(2)
                && node_depth() as usize != SCOPES;
            if !ok {
                a[4] += 1;
                replay_failed(s, c, 132, 0, 0);
            } else {
                scope = node_depth() + 1;
                REPLAY_NODE_DEPTH.with(|d| d.set(scope));
                REPLAY_NODES.with(|nodes| nodes.borrow_mut()[scope as usize - 1] = receipt.node.clone());
                a[2] += 1;
            }
        }
    }
    {
        let address = payload as u64;
        let (mut child, mut context) = (0u64, 0u64);
        if crate::declared_moved_exports::CyberpunkVR_SceneChildEnable.load(Relaxed) == 1 && kind == 2 && address != 0 && address <= u64::MAX - 80 {
            context = address + 8;
            let mut bytes = [0u8; 8];
            read(address, &mut bytes);
            child = u64::from_le_bytes(bytes);
        }
        // Only A96BFC carries a definition at payload[0]. Other worker kinds clear inherited child attribution until
        // their real dispatcher runs.
        scene_child::scope_enter(child, context, 1);
        // SAFETY: MinHook's trampoline to the original.
        let original: extern "C" fn(*mut c_void, *mut c_void) = unsafe { core::mem::transmute(original) };
        original(payload, worker);
        scene_child::scope_leave();
    }
    if scope != 0 {
        let g = lock();
        let s = g.state();
        if scope != node_depth() {
            if let Some(value) = REPLAY_NODES.with(|nodes| nodes.borrow()[scope as usize - 1].consumer.clone()) {
                failed(s, &value, 133, 0, 0);
            }
        } else {
            let parent = REPLAY_NODES.with(|nodes| core::mem::take(&mut nodes.borrow_mut()[scope as usize - 1]));
            REPLAY_NODE_DEPTH.with(|d| d.set(scope - 1));
            let _write = TelemetryWrite::new();
            stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayAsync))[5] += 1;
            if let Some(value) = parent.consumer.as_ref() {
                // SAFETY: under the mutex.
                publish_replay(s, unsafe { value.get() });
            }
            drop(g);
            drop(parent);
        }
    }
}
extern "C" fn resource_pass_end(payload: *mut c_void, worker: *mut c_void) {
    // SAFETY: the trampoline's value.
    resource_worker_run(payload, worker, 1, unsafe { PASS_END });
}
extern "C" fn resource_definition_worker(payload: *mut c_void, worker: *mut c_void) {
    // SAFETY: as above.
    resource_worker_run(payload, worker, 2, unsafe { DEFINITION_WORKER });
}
extern "C" fn resource_range_worker(payload: *mut c_void, worker: *mut c_void) {
    // SAFETY: as above.
    resource_worker_run(payload, worker, 3, unsafe { RANGE_WORKER });
}

// ---- the node payloads and nodes
static mut RESOURCE_PAYLOAD: usize = 0;
extern "C" fn resource_payload_hook(holder: *mut c_void, unused: *mut c_void, input: *mut c_void) -> usize {
    let caller = return_address();
    let base = base();
    let mut consumer: Option<ConsumerRef> = None;
    let mut receipt = ReplayPayload { node: ReplayNode::default(), bytes: [0; 40] };
    let queue_depth = REPLAY_QUEUE_DEPTH.with(Cell::get);
    if enabled() && caller == base + 0x32D6C5 && queue_depth != 0 && queue_depth as usize <= SCOPES {
        let g = lock();
        let s = g.state();
        consumer = REPLAY_QUEUES.with(|queues| queues.borrow()[queue_depth as usize - 1].clone());
        if let Some(value) = consumer.as_ref() {
            let read_bytes = read(input as u64, &mut receipt.bytes[..32]) && read(input as u64 + 0x21, &mut receipt.bytes[0x21..0x22]);
            if read_bytes {
                let identity = {
                    // SAFETY: under the mutex.
                    let c = unsafe { value.get() };
                    replay_identity(s, c)
                };
                // SAFETY: under the mutex.
                let c = unsafe { value.get() };
                let mut matches = 0;
                for node in &c.execution_graph.nodes {
                    if node.bytes[24..32] == receipt.bytes[8..16] && node.bytes[8..24] == receipt.bytes[16..32] {
                        receipt.node = ReplayNode { consumer: Some(value.clone()), identity, node: NodeBinding { handle: node.handle & 0xDFFF_FFFF, ordinal: node.order, slot: u32::from(node.bytes[29]), view_index: field_u32(&node.bytes, 24) } };
                        matches += 1;
                    }
                }
                let owner = u64::from_le_bytes(receipt.bytes[..8].try_into().expect("eight"));
                if matches != 1 || owner != c.ticket().frame_owner() {
                    replay_failed(s, c, 119, 0, 0);
                    receipt.node = ReplayNode::default();
                }
            } else {
                failed(s, value, 120, 0, 0);
            }
        }
    }
    let result = original!(RESOURCE_PAYLOAD: fn(*mut c_void, *mut c_void, *mut c_void) -> usize)(holder, unused, input);
    if receipt.node.consumer.is_some() {
        let g = lock();
        let s = g.state();
        let value = consumer.expect("matched");
        let (mut a, mut b) = ([0u8; 16], [0u8; 16]);
        let mut bytes = [0u8; 40];
        let ok = read(holder as u64, &mut a) && read(holder as u64, &mut b) && a == b && {
            let first = u64::from_le_bytes(a[..8].try_into().expect("eight"));
            let second = u64::from_le_bytes(a[8..].try_into().expect("eight"));
            first == base + 0x276830
                && read(second, &mut bytes[..32])
                && read(second + 0x21, &mut bytes[0x21..0x22])
                && bytes == receipt.bytes
                && s.replay_payloads.len() < 512
                && !s.replay_payloads.contains_key(&second)
        };
        if !ok {
            failed(s, &value, 121, 0, 0);
        } else {
            let second = u64::from_le_bytes(a[8..].try_into().expect("eight"));
            s.replay_payloads.insert(second, receipt);
            let _write = TelemetryWrite::new();
            let registration = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayRegistration));
            registration[3] += 1;
            registration[5] = s.replay_payloads.len() as u64;
        }
    }
    result
}
static mut RESOURCE_NODE: usize = 0;
extern "C" fn resource_node_hook(payload: *mut c_void, worker: *mut c_void) {
    let caller = return_address();
    let mut scope = 0u32;
    if enabled() {
        let g = lock();
        let s = g.state();
        if let Some(receipt) = s.replay_payloads.remove(&(payload as u64)) {
            let value = receipt.node.consumer.clone().expect("a payload's consumer");
            // SAFETY: under the mutex.
            let c = unsafe { value.get() };
            let base = base();
            let (mut a, mut b) = ([0u8; 40], [0u8; 40]);
            let p = payload as u64;
            let renderer = read_u64(base + RENDERER_GLOBAL_RVA);
            let wrapper = renderer.filter(|&r| r == c.renderer).and_then(|r| read_u64(r + 0x98));
            let pool = wrapper.filter(|&w| w == c.resource_wrapper).and_then(read_u64);
            let ok = c.resource_replay.as_ref().is_some_and(|replay| replay.is_captured() && *replay.binding() == receipt.node.identity)
                && caller == base + 0x276867
                && read(p, &mut a[..32])
                && read(p + 0x21, &mut a[0x21..0x22])
                && read(p, &mut b[..32])
                && read(p + 0x21, &mut b[0x21..0x22])
                && a == receipt.bytes
                && b == a
                && pool.is_some_and(|pool| pool == c.resource_pool)
                && read_u8(c.resource_pool + 2) == Some(2)
                && node_depth() as usize != SCOPES;
            if !ok {
                replay_failed(s, c, 122, 0, 0);
            } else {
                scope = node_depth() + 1;
                REPLAY_NODE_DEPTH.with(|d| d.set(scope));
                let ordinal = receipt.node.node.ordinal as usize;
                REPLAY_NODES.with(|nodes| nodes.borrow_mut()[scope as usize - 1] = receipt.node.clone());
                if let Some(row) = c.resource_replay_rows.get_mut(ordinal) {
                    row[6] += 1;
                }
                let _write = TelemetryWrite::new();
                let registration = stats_array(addr_of_mut!(x::CyberpunkVR_OwnedResourceReplayRegistration));
                registration[4] += 1;
                registration[5] = s.replay_payloads.len() as u64;
            }
        }
    }
    original!(RESOURCE_NODE: fn(*mut c_void, *mut c_void) -> ())(payload, worker);
    end_resource_node(scope);
}
pub(super) fn register_detours(none: *mut bool) {
    use cvr_engine::detours::register;
    let detour = |f: usize| f as *mut c_void;
    register("[single-pass] native declaration/execution resource parity", 0x334A34, detour(resource_command_hook as *const () as usize), addr_of_mut!(RESOURCE_COMMAND).cast(), Some(code_compatible), none);
    register("[single-pass] asynchronous pass-end resource scope", 0xB293DC, detour(resource_pass_end as *const () as usize), addr_of_mut!(PASS_END).cast(), Some(code_compatible), none);
    register("[single-pass] asynchronous definition resource scope", 0xB3C208, detour(resource_definition_worker as *const () as usize), addr_of_mut!(DEFINITION_WORKER).cast(), Some(code_compatible), none);
    register("[single-pass] asynchronous range resource scope", 0xB3FC80, detour(resource_range_worker as *const () as usize), addr_of_mut!(RANGE_WORKER).cast(), Some(code_compatible), none);
    register("[single-pass] early resource-node payload identity", 0x96A298, detour(resource_payload_hook as *const () as usize), addr_of_mut!(RESOURCE_PAYLOAD).cast(), Some(code_compatible), none);
    register("[single-pass] source-bound execution resource node", 0x2768AC, detour(resource_node_hook as *const () as usize), addr_of_mut!(RESOURCE_NODE).cast(), Some(code_compatible), none);
}
const _: () = assert!(core::mem::size_of::<rr::Identity>() == 48 && SCOPES == 32);
