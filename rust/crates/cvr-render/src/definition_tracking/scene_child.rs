//! The scene child scopes (formerly NativeDefinitionTracking.cpp's scene_child namespace): a scene section's child work
//! running inside an owned both-eyes expansion, matched to its consumer, its view and section and its unique child
//! slot, counted on the consumer while it runs, and described to the scene access readers.

use super::consumers::{join_scene_reader, owned};
use super::*;
use crate::native::graph_parent::{self as parent, Context};
use crate::scene_child::{unique_child, ChildKey};
use crate::scene_sections;
use core::cell::RefCell;

pub const MAX_DEPTH: u32 = 32;
const EXECUTION: u32 = 1;
/// scene_child::Identity (136 bytes).
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
/// A running child scope's receipt; its consumer's scope count drops with it.
pub(crate) struct Receipt {
    owner: Option<ConsumerRef>,
    identity: Identity,
    context: Context,
}
impl Drop for Receipt {
    fn drop(&mut self) {
        let Some(owner) = self.owner.as_ref() else { return };
        let _g = lock();
        // SAFETY: under the mutex.
        let c = unsafe { owner.get() };
        if c.scene_child_scopes != 0 {
            c.scene_child_scopes -= 1;
        }
    }
}
thread_local! {
    static CURRENT: RefCell<Option<Arc<Receipt>>> = const { RefCell::new(None) };
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// The scopes' saved (previous receipt, previous depth), innermost last.
    static STACK: RefCell<Vec<(Option<Arc<Receipt>>, u32)>> = const { RefCell::new(Vec::new()) };
}
fn enabled() -> bool {
    crate::declared_moved_exports::CyberpunkVR_SceneChildEnable.load(Relaxed) == 1 && x::CyberpunkVR_OwnedExecutionClosing.load(Relaxed) == 0
}
fn thread_id() -> u32 {
    // SAFETY: a plain query.
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}
/// Context: the work context's owner and wrapper, its graph parent context and the async flag, read twice.
fn context_of(address: u64) -> Option<(Context, u64, u64)> {
    let (mut before, mut after) = ([0u8; 16], [0u8; 16]);
    if address == 0 || address > u64::MAX - 72 || !read(address, &mut before) || !read_u8(address + 0x30).is_some_and(|flags| flags & 2 != 0) {
        return None;
    }
    let context = parent::read_context(Some(&mut reader()), address)?;
    if !read(address, &mut after) || !read_u8(address + 0x30).is_some_and(|flags| flags & 2 != 0) || before != after {
        return None;
    }
    let frame_owner = u64::from_le_bytes(before[..8].try_into().expect("eight"));
    let wrapper = u64::from_le_bytes(before[8..].try_into().expect("eight"));
    (frame_owner != 0 && wrapper != 0).then_some((context, frame_owner, wrapper))
}
/// Child.
fn child_of(address: u64) -> Option<ChildKey> {
    if address == 0 || address & 7 != 0 || address > u64::MAX - 24 {
        return None;
    }
    let table = read_u64(address).filter(|&table| table != 0 && table & 7 == 0 && table <= u64::MAX - 16)?;
    let work = read_u64(table + 8).filter(|&work| work != 0)?;
    (read_u64(address) == Some(table)).then_some(ChildKey { object: address, vtable: table, work, slot: 0 })
}
/// OwnerAlive.
fn owner_alive(s: &mut State, owner: &Consumer) -> bool {
    let alive = owner.scene_both_expansion.as_ref().is_some_and(|plan| plan.ready())
        && owner.ticket.is_some()
        && owner.owned_graph.is_some()
        && !owner.rejected
        && !owner.generation_rejected
        && owner.execution_verified
        && owner.declaration_snapshot_ready
        && !owner.extra_readers_completed;
    alive && owned(s, owner).is_some_and(|state| state.phase == crate::owned_graph_execution::phase::ESCAPED && state.construct_verified && state.queued(EXECUTION))
}
/// Match.
#[allow(clippy::too_many_arguments)]
fn match_owner(s: &mut State, owner: &Consumer, node: u64, context: &Context, frame_owner: u64, wrapper: u64, validate_graph: bool) -> Option<Identity> {
    if !owner_alive(s, owner) || context.view_index > 1 || frame_owner != owner.ticket().frame_owner() || wrapper != owner.resource_wrapper {
        return None;
    }
    let frozen = owned(s, owner)?.identity;
    let t = owner.ticket();
    if frozen.consumer_id != t.id() || frozen.epoch != t.epoch() || frozen.pair_generation != t.generation() || frozen.frame_owner != frame_owner || frozen.source_graph != t.source_graph_address() || frozen.execution_graph != t.graph_address() {
        return None;
    }
    let plan = owner.scene_both_expansion.as_ref()?;
    let view = context.view_index;
    let section = if view != 0 { 2u32 } else { 1u32 };
    let handle = if view != 0 { plan.tail_handle(1) } else { plan.prefix_handle(0) };
    let root = owner.execution_graph.nodes.iter().find(|value| value.handle == handle)?;
    if u32::from(root.bytes[29]) != context.slot || (validate_graph && !owner.owned_graph.as_ref().is_some_and(|graph| graph.validate_compiled())) {
        return None;
    }
    let resolved = parent::resolve(&owner.execution_graph, context)?;
    if resolved.handle != handle || resolved.object != owner.scene_both_views[view as usize].definitions()[section as usize - 1].object || resolved.work != base() + scene_sections::WORK_RVA {
        return None;
    }
    let source = &owner.scene_both_sources[view as usize];
    let mut lifetime = CallerLifetime::default();
    if !join_scene_reader(s, owner, source, &mut lifetime)
        || lifetime != owner.scene_both_lifetimes[view as usize]
        || !owner.scene_both_views[view as usize].matches(source, &lifetime)
        || source.root_identity() != plan.selected_source(view)
    {
        return None;
    }
    let child = child_of(node)?;
    if source.children().len() != scene_sections::CHILD_COUNT {
        return None;
    }
    let children: Vec<ChildKey> = source.children().iter().map(|item| ChildKey { object: item.object, vtable: item.vtable, work: item.work, slot: item.slot }).collect();
    let slot = unique_child(&children, view, section, &child)?;
    Some(Identity {
        consumer_id: frozen.consumer_id,
        epoch: frozen.epoch,
        pair_generation: frozen.pair_generation,
        frame_owner,
        source_graph: frozen.source_graph,
        execution_graph: frozen.execution_graph,
        root_object: resolved.object,
        child_object: child.object,
        child_vtable: child.vtable,
        child_work: child.work,
        work_context: context.address,
        root_handle: resolved.handle,
        view,
        section,
        child_slot: slot,
        ..Identity::default()
    })
}
/// Capture.
fn capture(node: u64, work_context: u64, phase: u32, parent_invocation: u64) -> Option<Arc<Receipt>> {
    if !enabled() || phase > 1 || node == 0 || !scene_code_compatible() {
        return None;
    }
    let g = lock();
    let s = g.state();
    if !enabled() || s.scene_child_next_invocation == u64::MAX {
        return None;
    }
    let (context, frame_owner, wrapper) = context_of(work_context)?;
    let mut selected: Option<(ConsumerRef, Identity)> = None;
    let owners: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for owner in owners {
        // SAFETY: under the mutex.
        let c = unsafe { owner.get() };
        let Some(candidate) = match_owner(s, c, node, &context, frame_owner, wrapper, true) else { continue };
        if selected.is_some() {
            return None;
        }
        selected = Some((owner, candidate));
    }
    let (owner, mut identity) = selected?;
    // SAFETY: under the mutex.
    let c = unsafe { owner.get() };
    if c.scene_child_scopes == u32::MAX || !c.scene_both_sources[identity.view as usize].matches_before(&mut reader()) {
        return None;
    }
    s.scene_child_next_invocation += 1;
    identity.invocation = s.scene_child_next_invocation;
    identity.parent_invocation = parent_invocation;
    identity.thread = thread_id();
    identity.phase = phase;
    c.scene_child_scopes += 1;
    Some(Arc::new(Receipt { owner: Some(owner), identity, context }))
}
/// Scope::Scope.
pub fn scope_enter(node: u64, work_context: u64, phase: u32) {
    let previous = CURRENT.with(|current| current.borrow_mut().take());
    let previous_depth = DEPTH.with(Cell::get);
    if previous_depth < u32::MAX {
        DEPTH.with(|depth| depth.set(previous_depth + 1));
    }
    let parent_invocation = previous.as_ref().map_or(0, |receipt| receipt.identity.invocation);
    STACK.with(|stack| stack.borrow_mut().push((previous, previous_depth)));
    if previous_depth >= MAX_DEPTH {
        return;
    }
    let receipt = capture(node, work_context, phase, parent_invocation);
    CURRENT.with(|current| *current.borrow_mut() = receipt);
}
/// Scope::~Scope.
pub fn scope_leave() {
    let (previous, previous_depth) = STACK.with(|stack| stack.borrow_mut().pop()).unwrap_or((None, 0));
    let finished = CURRENT.with(|current| core::mem::replace(&mut *current.borrow_mut(), previous));
    DEPTH.with(|depth| depth.set(previous_depth));
    drop(finished);
}
/// DescribeCurrent.
pub fn describe_current() -> Option<Identity> {
    let receipt = CURRENT.with(|current| current.borrow().clone());
    let receipt = receipt.filter(|receipt| enabled() && receipt.identity.thread == thread_id())?;
    let g = lock();
    let s = g.state();
    let owner = receipt.owner.as_ref()?;
    // SAFETY: under the mutex.
    let c = unsafe { owner.get() };
    if !enabled() || c.scene_child_scopes == 0 {
        return None;
    }
    let (context, frame_owner, wrapper) = context_of(receipt.identity.work_context)?;
    if context != receipt.context {
        return None;
    }
    let mut identity = match_owner(s, c, receipt.identity.child_object, &context, frame_owner, wrapper, false)?;
    identity.invocation = receipt.identity.invocation;
    identity.parent_invocation = receipt.identity.parent_invocation;
    identity.thread = receipt.identity.thread;
    identity.phase = receipt.identity.phase;
    (identity == receipt.identity).then_some(identity)
}
/// The .cpp's scene_child::Scope constructor.
#[no_mangle]
pub extern "C" fn cvr_ndt_child_enter(node: u64, work_context: u64, phase: u32) {
    scope_enter(node, work_context, phase);
}
/// The .cpp's scene_child::Scope destructor.
#[no_mangle]
pub extern "C" fn cvr_ndt_child_leave() {
    scope_leave();
}
/// scene_child::DescribeCurrent.
///
/// # Safety
/// `out` is writable.
#[no_mangle]
pub unsafe extern "C" fn cvr_ndt_describe_current(out: *mut Identity) -> bool {
    let found = describe_current();
    // SAFETY: the caller's output.
    unsafe { *out = found.unwrap_or_default() };
    found.is_some()
}
