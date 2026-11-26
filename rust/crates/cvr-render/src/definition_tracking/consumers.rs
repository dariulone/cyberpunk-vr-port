//! The native consumers of a paired root: their registration with the root ticket, the definition generations bound
//! to them, the owned reference graph (its sources, frontier, schedule, scene sections and expansion, extra readers),
//! the construct, queue, compiler and execution hooks, the poll that retires them, and the public calls the paired
//! frame's hooks make.

use super::*;
use crate::definition_wrappers::{AdmissionFacts, ExpansionAdmissionFacts, FrontierPlan, StageSnapshot};
use crate::native::compiler_identity::CompilerBinding;
use crate::native::definition_tree::Kind as TreeKind;
use crate::native::graph_snapshot::{field_u32, field_u64, Node};
use crate::owned_graph_execution::{self as oge, Identity as OwnedIdentity, OwnerGenerationBinding};
use crate::paired_frontier as pf;
use crate::scene_expansion::Definition;

/// owned_graph_execution::Path.
const DECLARATION: u32 = 0;
const EXECUTION: u32 = 1;
const MISSING_PROOF: u32 = oge::failure::MISSING_PROOF;
const CONSTRUCT_RETURN: u64 = crate::native::consumer_identity::CONSTRUCT_RETURN;

macro_rules! rd {
    () => {
        Some(&mut reader() as &mut dyn FnMut(u64, &mut [u8]) -> bool)
    };
}
pub(crate) use rd;

fn consumers_stat(i: usize) -> &'static AtomicU64 {
    &x::CyberpunkVR_PreparationNativeConsumers[i]
}
/// The owned state of a consumer (`consumer.ownedState`).
pub(crate) fn owned<'a>(s: &'a mut State, consumer: &Consumer) -> Option<&'a mut oge::State> {
    s.owned_execution_states.find(consumer.owned_state?)
}
fn same_nodes(a: &Node, b: &Node) -> bool {
    a.handle == b.handle && a.order == b.order && a.bytes == b.bytes && a.vtable == b.vtable && a.work == b.work
}
fn source_of(snapshot: &Snapshot) -> gd::Source {
    gd::Source { nodes: snapshot.nodes.clone(), edges: snapshot.edges.clone(), failure: snapshot.failure }
}
fn owned_snapshot(graph: &OwnedGraph) -> Snapshot {
    match graph.snapshot() {
        Some((nodes, edges)) => Snapshot { nodes, edges, failure: 0 },
        None => Snapshot { failure: 1, ..Default::default() },
    }
}
/// NativeReferenceSort.
fn native_reference_sort(graph: *mut u8) -> bool {
    // SAFETY: the engine's sort over our constructed graph header; a fault is caught.
    unsafe { cvr_engine::code::guarded_call4((base() + 0x8E9B28) as usize, graph.cast(), core::ptr::null_mut(), core::ptr::null_mut(), core::ptr::null_mut()) }.is_some()
}
/// CompareGraphSlot: the slot's value exchanged under SEH.
fn compare_graph_slot(slot: u64, expected: u64, desired: u64) -> Option<u64> {
    if slot == 0 || slot & 7 != 0 || !cvr_engine::code::readable_range(slot as usize, 8) {
        return None;
    }
    // SAFETY: a committed, aligned engine word (checked just before; the .cpp's SEH covered the same access).
    Some(match unsafe { AtomicU64::from_ptr(slot as *mut u64) }.compare_exchange(expected, desired, SeqCst, SeqCst) {
        Ok(previous) | Err(previous) => previous,
    })
}
/// TopVectorMatches.
fn top_vector_matches(owner: &gd::Owner) -> bool {
    let base = base();
    let native = owner.plan.saved_header();
    if owner.plan.empty_header().data != base + 0x30947E8 || read_u64(base + 0x3094808) != Some(base + 0x120F6F0) {
        return false;
    }
    if native.capacity != 0 {
        let trailer = owner.plan.trailer_address();
        let (allocator, again) = (read_u64(trailer), read_u64(trailer));
        if allocator != Some(owner.plan.empty_header().data) || allocator != again {
            return false;
        }
    }
    owner.plan.matches_before(&mut reader())
}

// ---- the scene readers
/// JoinSceneReader.
pub(crate) fn join_scene_reader(s: &State, consumer: &Consumer, source: &SceneSections, binding: &mut CallerLifetime) -> bool {
    *binding = CallerLifetime::default();
    if !consumer.has_scene_expansion() || !source.ready() {
        return false;
    }
    let root = source.root_identity();
    let object = field_u64(&root.bytes, 16);
    let owners = &consumer.original_source.snapshot.owners;
    let Some(mapped) = consumer.original_source.snapshot.mappings.iter().find(|value| value.node_handle == root.handle && value.object == object) else { return false };
    if mapped.owner_index as usize >= owners.len() {
        return false;
    }
    let header = owners[mapped.owner_index as usize].plan.owner_address();
    let ticket = consumer.ticket();
    let mut matches = 0;
    for reader in &consumer.extra_readers {
        // SAFETY: a boxed owner under the mutex.
        let owner = unsafe { &*reader.owner };
        if owner.identity.owner_header != header {
            continue;
        }
        let pending = s.rs_ref(reader.owner).is_some_and(|state| state.has_pending_extra_reader(&reader.identity));
        if !pending
            || reader.identity.consumer_id != ticket.id()
            || reader.identity.epoch != ticket.epoch()
            || reader.identity.pair_generation != ticket.generation()
            || reader.identity.source_graph != ticket.source_graph_address()
            || reader.identity.execution_graph != ticket.graph_address()
            || !owner.definitions.contains(&object)
        {
            return false;
        }
        *binding = CallerLifetime {
            consumer_id: reader.identity.consumer_id,
            epoch: reader.identity.epoch,
            pair_generation: reader.identity.pair_generation,
            original_root: object,
            owner_header: owner.identity.owner_header,
            owner_generation: owner.identity.generation,
            owner_born_epoch: owner.identity.born_epoch,
            protection_receipt: reader.identity.lease_id,
        };
        matches += 1;
    }
    matches == 1
}
/// ValidateSceneViews.
pub(crate) fn validate_scene_views(s: &State, consumer: &Consumer) -> bool {
    if !consumer.has_scene_expansion() {
        return true;
    }
    if !scene_code_compatible() {
        return false;
    }
    let validate = |source: &SceneSections, views: &BorrowedSections, lifetime: &CallerLifetime| {
        let mut current = CallerLifetime::default();
        join_scene_reader(s, consumer, source, &mut current) && current == *lifetime && views.matches(source, &current) && source.matches_before(&mut reader())
    };
    if consumer.scene_both_expansion.is_some() {
        return (0..2).all(|side| validate(&consumer.scene_both_sources[side], &consumer.scene_both_views[side], &consumer.scene_both_lifetimes[side]));
    }
    validate(&consumer.scene_split_source, &consumer.scene_views, &consumer.scene_lifetime)
}

// ---- the extra readers
/// ReserveExtraOwners.
fn reserve_extra_owners(s: &mut State, consumer: &mut Consumer, graph_slot: u64) -> bool {
    recycle_retained_metadata(s);
    // SAFETY: the exports.
    let cache6 = unsafe { x::CyberpunkVR_OwnedExecutionCache[6] };
    if cache6 != 0 && cache6 != consumer.original_source.cache_base {
        owned_reject(owned_failure::ADMISSION, consumer.original_source.cache_base);
        return false;
    }
    let base = base();
    let referenced: std::collections::BTreeSet<u32> = consumer.original_source.snapshot.mappings.iter().map(|mapping| mapping.owner_index).collect();
    let mut pending: Vec<*mut RetainedOwner> = Vec::with_capacity(referenced.len());
    let ticket = consumer.ticket();
    for index in referenced {
        let Some(source_owner) = consumer.original_source.snapshot.owners.get(index as usize) else {
            owned_reject(owned_failure::OWNERS, 0);
            return false;
        };
        let header = source_owner.plan.owner_address();
        let found = s.mutations.get(&header).copied();
        let usable = found.is_some_and(|f| f.storage_alive && f.storage != 0 && f.generation != 0 && f.builder_closed && f.in_flight == 0 && f.builders == 0 && f.operations == 0);
        if !usable || !top_vector_matches(source_owner) {
            owned_reject(owned_failure::OWNERS, header);
            return false;
        }
        let found = found.expect("checked");
        let (saved, empty) = (source_owner.plan.saved_header(), source_owner.plan.empty_header());
        let identity = HeaderIdentity {
            owner_header: header,
            generation: found.generation,
            born_epoch: found.epoch,
            saved_native: NativeHeader { data: saved.data, capacity: saved.capacity, count: saved.count },
            empty_allocator: NativeHeader { data: empty.data, capacity: empty.capacity, count: empty.count },
        };
        let mut retained = find_retained_owner(s, &identity);
        if retained.is_null() {
            let Some(target) = s.retained_owners.iter().position(Option::is_none) else {
                owned_reject(owned_failure::RESERVATION, 0);
                return false;
            };
            let mut created = Box::new(RetainedOwner {
                identity,
                state: None,
                tree: Tree::default(),
                allocator_binding: ab::Receipt::default(),
                definitions: source_owner.plan.definitions().to_vec(),
                ready: false,
                emergency_detached: false,
                destroy_in_flight: false,
                last_consumer_id: 0,
                last_epoch: 0,
                last_pair_generation: 0,
                protected_attached: false,
                completion_flags: 0,
                declaration_group: 0,
                execution_group: 0,
                compiler_group: 0,
                completed_readers_receipt: 0,
                service_header: 0,
                service_storage: 0,
                service_epoch: 0,
                service_caller: 0,
            });
            let roots = created.definitions.clone();
            if !created.tree.capture(&roots, base, rd!(), true) || !crate::definition_wrappers::tree_certified(&created.tree) || !created.tree.matches_before(&mut reader()) {
                owned_reject(owned_failure::TREE, header);
                return false;
            }
            let mut binding = ab::Receipt::default();
            let bound = capture_allocator_locked(s, &mut binding)
                && binding.bind_saved(NativeVector16 { data: saved.data, capacity: saved.capacity, count: saved.count }, &created.tree, &[]);
            created.allocator_binding = binding;
            if !bound {
                publish_allocator_receipt(s, Some(&created.allocator_binding), created.tree.nodes().len() as u64);
                owned_reject(owned_failure::ALLOCATOR_BINDING, created.allocator_binding.rejection() as u64);
                return false;
            }
            created.state = s.retirement_states.reserve(
                &identity,
                rd::ReservationFacts { retirement_path_audited: true, layout_audited: true, destructor_tree_safe: true },
                &rd::ReservationAllocatorObservation { identity: rd::AllocatorIdentity { address: empty.data, generation: 1 }, infrastructure_matches: true },
            );
            if created.state.is_none() {
                owned_reject(owned_failure::RESERVATION, s.retirement_states.rejection() as u64);
                return false;
            }
            retained = &mut *created as *mut RetainedOwner;
            s.retained_owners[target] = Some(created);
            let _write = TelemetryWrite::new();
            totals()[0] += 1;
            retirement_event(s, RetireEvent::Reserve, retained, 0, 0, 0, 0, 0, ticket.id(), ticket.epoch(), ticket.generation());
        }
        let open = s.rs_ref(retained).is_some_and(|state| state.acquisitions_open() && state.pending_readers() < rd::MAX_EXTRA_READERS);
        // SAFETY: a boxed owner under the mutex.
        if !open || !unsafe { &*retained }.tree.matches_before(&mut reader()) {
            owned_reject(owned_failure::RESERVATION, header);
            return false;
        }
        pending.push(retained);
    }
    consumer.extra_readers.reserve(pending.len());
    for owner in pending {
        if s.next_extra_reader == u64::MAX {
            cancel_extra_readers(s, consumer, true);
            owned_reject(owned_failure::RESERVATION, 0);
            return false;
        }
        s.next_extra_reader += 1;
        let reader = ExtraReaderIdentity {
            lease_id: s.next_extra_reader,
            consumer_id: ticket.id(),
            epoch: ticket.epoch(),
            pair_generation: ticket.generation(),
            source_graph: ticket.source_graph_address(),
            execution_graph: ticket.graph_address(),
        };
        if !s.rs(owner).is_some_and(|state| state.admit_extra_reader(&reader)) {
            cancel_extra_readers(s, consumer, true);
            owned_reject(owned_failure::RESERVATION, 0);
            return false;
        }
        // SAFETY: as above.
        let o = unsafe { &mut *owner };
        o.last_consumer_id = reader.consumer_id;
        o.last_epoch = reader.epoch;
        o.last_pair_generation = reader.pair_generation;
        o.completion_flags = 0;
        o.declaration_group = 0;
        o.execution_group = 0;
        o.compiler_group = 0;
        {
            let _write = TelemetryWrite::new();
            totals()[1] += 1;
            event(s, RetireEvent::Admit, owner);
        }
        if !o.protected_attached {
            o.protected_attached = true;
            PROTECTED_ATTACHED_OWNERS.fetch_add(1, SeqCst);
        }
        let protection = [o.identity.owner_header, o.identity.generation, o.identity.born_epoch, reader.consumer_id, reader.epoch, reader.pair_generation];
        LAST_PROTECTION_SEQ.fetch_add(1, SeqCst);
        for (i, value) in protection.iter().enumerate() {
            LAST_PROTECTION_IDENTITY[i].store(*value, SeqCst);
        }
        LAST_PROTECTION_SEQ.fetch_add(1, SeqCst);
        consumer.extra_readers.push(ExtraReader { owner, identity: reader });
    }
    let bindings: Vec<OwnerGenerationBinding> = consumer
        .extra_readers
        .iter()
        .map(|reader| {
            // SAFETY: as above.
            let owner = unsafe { &*reader.owner };
            OwnerGenerationBinding { owner_header: owner.identity.owner_header, generation_id: owner.identity.generation, born_mutation_epoch: owner.identity.born_epoch, protection_receipt: reader.identity.lease_id }
        })
        .collect();
    let identity = OwnedIdentity {
        consumer_id: ticket.id(),
        epoch: ticket.epoch(),
        pair_generation: ticket.generation(),
        source_graph: ticket.source_graph_address(),
        execution_graph: ticket.graph_address(),
        frame_owner: ticket.frame_owner(),
        prerequisite_group: ticket.group(),
    };
    if consumer.has_scene_expansion() {
        let code = scene_code_compatible();
        let context_ready = crate::context_order::analyze(&consumer.execution_graph.nodes, &consumer.execution_graph.edges, consumer.execution_graph.failure, base, rd!()).failure == crate::context_order::Failure::None;
        let mut bound = code && context_ready;
        if bound && consumer.scene_both_expansion.is_some() {
            for side in 0..2 {
                if !bound {
                    break;
                }
                let mut lifetime = CallerLifetime::default();
                bound = join_scene_reader(s, consumer, &consumer.scene_both_sources[side], &mut lifetime);
                consumer.scene_both_lifetimes[side] = lifetime;
                bound = bound && {
                    let source = consumer.scene_both_sources[side].clone();
                    consumer.scene_both_views[side].bind(&source, &lifetime)
                };
            }
        } else if bound {
            let mut lifetime = CallerLifetime::default();
            bound = join_scene_reader(s, consumer, &consumer.scene_split_source, &mut lifetime);
            consumer.scene_lifetime = lifetime;
            bound = bound && {
                let source = consumer.scene_split_source.clone();
                consumer.scene_views.bind(&source, &lifetime)
            };
        }
        if !bound || !validate_scene_views(s, consumer) {
            // SAFETY: the export.
            unsafe { x::CyberpunkVR_OwnedSceneSplitStats[22] = 7 };
            cancel_extra_readers(s, consumer, true);
            owned_reject(owned_failure::SCENE_EXPANSION, 7);
            return false;
        }
        // SAFETY: the export.
        unsafe { x::CyberpunkVR_OwnedSceneSplitStats[16] = 1 };
        let protected_definitions = !consumer.extra_readers.is_empty() && consumer.extra_readers.iter().all(|reader| s.rs_ref(reader.owner).is_some_and(|state| state.has_pending_extra_reader(&reader.identity)));
        let facts = ExpansionAdmissionFacts {
            source_mapping_audited: consumer.source_verified && !consumer.generation_rejected,
            definition_protection_audited: protected_definitions,
            native_code_guarded: code,
            native_context_order_compiled: context_ready,
        };
        consumer.owned_state = if let Some(plan) = consumer.scene_both_expansion.as_deref() {
            s.owned_execution_states.admit_expanded_both(
                &identity,
                &bindings,
                plan,
                &consumer.scene_both_views,
                &consumer.scene_both_sources,
                &consumer.scene_both_lifetimes,
                &consumer.execution_graph,
                base,
                facts,
                consumer.producer_scope,
                graph_slot,
            )
        } else {
            let plan = consumer.scene_expansion.as_deref().expect("a scene expansion");
            s.owned_execution_states.admit_expanded(
                &identity,
                &bindings,
                plan,
                &consumer.scene_views,
                &consumer.scene_split_source,
                &consumer.scene_lifetime,
                &consumer.execution_graph,
                base,
                facts,
                consumer.producer_scope,
                graph_slot,
            )
        };
    } else {
        consumer.owned_state = s.owned_execution_states.admit(
            &identity,
            &bindings,
            AdmissionFacts { exact_reference_compiled: true, source_mapping_audited: true, definition_protection_audited: true },
            consumer.producer_scope,
            graph_slot,
        );
    }
    if consumer.owned_state.is_none() {
        cancel_extra_readers(s, consumer, true);
        owned_reject(owned_failure::RESERVATION, 0);
        return false;
    }
    {
        let _write = TelemetryWrite::new();
        // SAFETY: the exports.
        unsafe {
            if x::CyberpunkVR_OwnedExecutionCache[6] == 0 {
                x::CyberpunkVR_OwnedExecutionCache[6] = consumer.original_source.cache_base;
            }
            *out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionAdmission)) = [ticket.id(), ticket.epoch(), ticket.generation(), ticket.source_graph_address(), consumer.original_source.cache_base, consumer.renderer];
        }
    }
    publish_retirement_receipt(s);
    true
}

// ---- the scene sections and stages
fn discard_node(consumer: &Consumer, frontier: &FrontierPlan, side: u32) -> Option<Node> {
    let id = frontier.plan.roles[side as usize].discard;
    consumer.original_graph.nodes.iter().find(|node| node.handle & 0xDFFF_FFFF == id && field_u32(&node.bytes, 24) == side).copied()
}
/// CaptureSceneSections.
fn capture_scene_sections(s: &mut State, consumer: &mut Consumer, admission: &crate::definition_cache::Admission, frontier: &FrontierPlan) {
    // SAFETY: the export.
    let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedSceneStats)) };
    stats[0] = 1;
    stats[1] = 2;
    stats[2] = admission.id;
    stats[3] = admission.epoch;
    stats[4] = admission.pair_generation;
    stats[7] = 1;
    let mut roots = Vec::new();
    let mut children = Vec::new();
    let mut snapshots: [SceneSections; 2] = Default::default();
    for side in 0..2u32 {
        let Some(found) = discard_node(consumer, frontier, side) else {
            stats[7] = 2;
            return;
        };
        let snapshot = &mut snapshots[side as usize];
        if !snapshot.capture(&found, &consumer.original_tree, &mut reader()) {
            stats[7] = 0x100 + u64::from(snapshot.rejection());
            return;
        }
        roots.push(SceneRootRow {
            object: field_u64(&found.bytes, 16),
            handle: u64::from(found.handle),
            view: u64::from(side),
            first: children.len() as u64,
            count: snapshot.children().len() as u64,
            split: u64::from(snapshot.split_index()),
            reserved: 0,
            header: snapshot.raw_header(),
        });
        children.extend_from_slice(snapshot.children());
    }
    consumer.scene_sections = snapshots;
    s.published_scene_roots = roots;
    s.published_scene_children = children;
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_OwnedSceneRoots = data(&s.published_scene_roots);
        x::CyberpunkVR_OwnedSceneChildren = data(&s.published_scene_children);
    }
    stats[5] = s.published_scene_roots.len() as u64;
    stats[6] = s.published_scene_children.len() as u64;
    stats[7] = 0;
    stats[1] = 1;
}
/// CaptureDiscardStages: numeric observation only; no cloned definitions or execution permissions.
fn capture_discard_stages(s: &mut State, consumer: &Consumer, admission: &crate::definition_cache::Admission, frontier: &FrontierPlan) {
    // SAFETY: the export.
    let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedStageStats)) };
    stats[0] = 1;
    stats[1] = 2;
    stats[2] = admission.id;
    stats[3] = admission.epoch;
    stats[4] = admission.pair_generation;
    stats[7] = 1;
    let mut definitions: Vec<StageDefinitionRow> = Vec::new();
    let mut stage_entries: Vec<[u8; 48]> = Vec::new();
    for side in 0..2u32 {
        let Some(found) = discard_node(consumer, frontier, side) else {
            stats[7] = 2;
            return;
        };
        let before = definitions.len();
        let tree = &consumer.original_tree;
        for index in 0..tree.nodes().len() as u32 {
            let child = &tree.nodes()[index as usize];
            if child.kind != TreeKind::RenderElements || !StageSnapshot::reachable(&found, tree, index) {
                continue;
            }
            let mut snapshot = StageSnapshot::default();
            if !snapshot.capture(&found, tree, index, &mut reader()) {
                stats[7] = 0x100 + u64::from(snapshot.rejection());
                return;
            }
            if definitions.len() >= 220 || snapshot.stages().len() > 8192 - stage_entries.len() {
                stats[7] = 3;
                return;
            }
            definitions.push(StageDefinitionRow {
                object: child.object,
                root_handle: u64::from(found.handle),
                view: u64::from(side),
                first: stage_entries.len() as u64,
                count: snapshot.stages().len() as u64,
                vtable: child.vtable,
                work: child.work,
                root_object: field_u64(&found.bytes, 16),
                header: *snapshot.raw_header(),
            });
            stage_entries.extend_from_slice(snapshot.stages());
        }
        if definitions.len() == before {
            stats[7] = 4;
            return;
        }
    }
    s.published_stage_definitions = definitions;
    s.published_stage_entries = stage_entries;
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_OwnedStageDefinitions = data(&s.published_stage_definitions);
        x::CyberpunkVR_OwnedStageEntries = data(&s.published_stage_entries);
    }
    stats[5] = s.published_stage_definitions.len() as u64;
    stats[6] = s.published_stage_entries.len() as u64;
    stats[7] = 0;
    stats[1] = 1;
}
/// PrepareSceneExpansion.
fn prepare_scene_expansion(s: &mut State, consumer: &mut Consumer, admission: &crate::definition_cache::Admission, request: u32, mode: u32) -> bool {
    // SAFETY: the export.
    let split = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedSceneSplitStats)) };
    split[0] = u64::from(mode);
    split[1] = 1;
    split[2] = admission.id;
    split[3] = admission.epoch;
    split[4] = admission.pair_generation;
    split[5] = u64::from(mode);
    let reject = |split: &mut [u64; 24], reason: u64| {
        split[22] = reason;
        owned_reject(owned_failure::SCENE_EXPANSION, reason);
        false
    };
    if request != 2 || consumer.coupled_schedule.is_none() || (mode != 1 && mode != 2) {
        return reject(split, 1);
    }
    if !scene_code_compatible() {
        return reject(split, 2);
    }
    split[17] = 1;
    let base = base();
    consumer.coupled_graph = consumer.execution_graph.clone();
    let discard_name = pf::name("GBuffer_Discard");
    let capture = |coupled: &Snapshot, tree: &Tree, side: u32, source: &mut SceneSections, views: &mut BorrowedSections| -> bool {
        let Some(found) = coupled.nodes.iter().find(|node| field_u32(&node.bytes, 24) == side && node.work == base + 0x332D5C && field_u64(&node.definition, 24) == discard_name) else { return false };
        source.capture(found, tree, &mut reader()) && views.prepare(source)
    };
    if mode == 2 {
        let mut definitions: [[Definition; 2]; 2] = Default::default();
        for side in 0..2usize {
            let (mut source, mut views) = (SceneSections::default(), BorrowedSections::default());
            let ok = capture(&consumer.coupled_graph, &consumer.original_tree, side as u32, &mut source, &mut views);
            consumer.scene_both_sources[side] = source;
            consumer.scene_both_views[side] = views;
            if !ok {
                return reject(split, 3);
            }
            definitions[side] = consumer.scene_both_views[side].definitions();
        }
        let mut plan = Box::new(BothEyesPlan::default());
        let built = plan.build(&consumer.coupled_graph, base, &definitions);
        consumer.scene_both_expansion = Some(plan);
        if !built {
            return reject(split, 4);
        }
        let plan = consumer.scene_both_expansion.as_deref().expect("built");
        // SAFETY: the export.
        let sections = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedSceneSplitSections)) };
        for row in 0..2usize {
            let side = 1 - row;
            let fields = [
                side as u64,
                u64::from(plan.prefix_handle(side as u32)),
                u64::from(plan.tail_handle(side as u32)),
                field_u64(&consumer.scene_both_sources[side].root_identity().bytes, 16),
                definitions[side][0].object,
                definitions[side][1].object,
            ];
            sections[row * 6..row * 6 + 6].copy_from_slice(&fields);
        }
    } else {
        let (mut source, mut views) = (SceneSections::default(), BorrowedSections::default());
        let ok = capture(&consumer.coupled_graph, &consumer.original_tree, 1, &mut source, &mut views);
        consumer.scene_split_source = source;
        consumer.scene_views = views;
        if !ok {
            return reject(split, 3);
        }
        let definitions = consumer.scene_views.definitions();
        let mut plan = Box::new(ScenePlan::default());
        let built = plan.build(&consumer.coupled_graph, base, consumer.scene_split_source.root_identity().handle, &definitions);
        consumer.scene_expansion = Some(plan);
        if !built {
            return reject(split, 4);
        }
        let plan = consumer.scene_expansion.as_deref().expect("built");
        split[10] = u64::from(plan.prefix_handle());
        split[11] = u64::from(plan.tail_handle());
        split[12] = field_u64(&consumer.scene_split_source.root_identity().bytes, 16);
        split[13] = definitions[0].object;
        split[14] = definitions[1].object;
    }
    let mut expanded = Box::new(OwnedGraph::default());
    let (plan_expanded, plan_edges) = match (&consumer.scene_both_expansion, &consumer.scene_expansion) {
        (Some(plan), _) if mode == 2 => (plan.expanded().clone(), plan.edges().clone()),
        (_, Some(plan)) => (plan.expanded().clone(), plan.edges().clone()),
        _ => return reject(split, 5),
    };
    let compiled_ok = expanded.create(&plan_expanded.nodes, &plan_expanded.edges, plan_expanded.failure, plan_edges)
        && native_reference_sort(expanded.sortable_data())
        && expanded.validate_compiled()
        && {
            let snapshot = owned_snapshot(&expanded);
            if mode == 2 {
                consumer.scene_both_expansion.as_ref().is_some_and(|plan| plan.matches_compiled(&snapshot))
            } else {
                consumer.scene_expansion.as_ref().is_some_and(|plan| plan.matches_compiled(&snapshot))
            }
        };
    if !compiled_ok {
        return reject(split, 5);
    }
    let compiled = owned_snapshot(&expanded);
    let context = crate::context_order::analyze(&compiled.nodes, &compiled.edges, compiled.failure, base, rd!());
    if context.failure != crate::context_order::Failure::None {
        return reject(split, 6);
    }
    if mode == 2 {
        if !consumer.scene_both_sources.iter().all(|source| source.matches_before(&mut reader())) {
            return reject(split, 6);
        }
    } else if !consumer.scene_split_source.matches_before(&mut reader()) {
        return reject(split, 6);
    }
    consumer.execution_graph = compiled.clone();
    consumer.owned_graph = Some(expanded);
    split[6] = consumer.coupled_graph.nodes.len() as u64;
    split[7] = consumer.coupled_graph.edges.len() as u64;
    split[8] = compiled.nodes.len() as u64;
    split[9] = compiled.edges.len() as u64;
    split[15] = 1;
    split[18] = u64::from(context.required_pairs);
    split[19] = u64::from(context.proved_pairs);
    s.published_split_source = consumer.coupled_graph.clone();
    s.published_split_mapping = if mode == 2 {
        consumer.scene_both_expansion.as_ref().map(|plan| plan.mappings().to_vec()).unwrap_or_default()
    } else {
        consumer.scene_expansion.as_ref().map(|plan| plan.mappings().to_vec()).unwrap_or_default()
    };
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_OwnedSceneSplitSourceNodes = data(&s.published_split_source.nodes);
        x::CyberpunkVR_OwnedSceneSplitSourceEdges = data(&s.published_split_source.edges);
        x::CyberpunkVR_OwnedSceneSplitMappingRows = data(&s.published_split_mapping);
    }
    // SAFETY: the export.
    let schedule = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionSchedule)) };
    schedule[3] = pf::fingerprint(&compiled.nodes, &compiled.edges, compiled.failure, base);
    schedule[6] = compiled.edges.len() as u64;
    schedule[7] = 0;
    schedule[8] = 0;
    for (i, node) in consumer.original_graph.nodes.iter().enumerate() {
        let Some(other) = compiled.nodes.get(i) else { break };
        for phase in 0..2 {
            schedule[7 + phase] += (node.bytes[28 + phase] != other.bytes[28 + phase]) as u64;
        }
    }
    schedule[9] = u64::from(context.required_pairs);
    schedule[10] = u64::from(context.proved_pairs);
    schedule[11] = context.missing.len() as u64;
    true
}

// ---- the owned reference
/// PrepareOwnedReference.
#[allow(clippy::too_many_arguments)]
fn prepare_owned_reference(s: &mut State, consumer: &mut Consumer, admission: &crate::definition_cache::Admission, destination: u64, owner_address: u64, graph_slot: u64, completion_address: u64, return_slot: u64) -> bool {
    if x::CyberpunkVR_OwnedExecutionMode.load(SeqCst) != 1
        || !crate::preparation_trace::active()
        || crate::native_state_exports::CyberpunkVR_PreparationPairState.load(SeqCst) != 2
        || !admission.registered
        || admission.epoch == 0
        || admission.pair_generation == 0
    {
        return false;
    }
    let request = x::CyberpunkVR_OwnedExecutionRequest.swap(0, SeqCst);
    if request != 1 && request != 2 {
        return false;
    }
    let _write = TelemetryWrite::new();
    let stats = owned_stats();
    stats.fill(0);
    // SAFETY: the exports.
    unsafe {
        out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionResources)).fill(0);
        out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionCompiler)).fill(0);
        let schedule = out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionSchedule));
        schedule.fill(0);
        schedule[0] = u64::from(request);
    }
    s.published_schedule_source = Snapshot::default();
    s.published_schedule_bindings.clear();
    s.published_stage_definitions.clear();
    s.published_stage_entries.clear();
    s.published_scene_roots.clear();
    s.published_scene_children.clear();
    s.published_split_source = Snapshot::default();
    s.published_split_mapping.clear();
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_OwnedExecutionScheduleNodes = 0;
        x::CyberpunkVR_OwnedExecutionScheduleEdges = 0;
        x::CyberpunkVR_OwnedExecutionScheduleBindings = 0;
        x::CyberpunkVR_OwnedStageDefinitions = 0;
        x::CyberpunkVR_OwnedStageEntries = 0;
        out(addr_of_mut!(x::CyberpunkVR_OwnedStageStats)).fill(0);
        x::CyberpunkVR_OwnedSceneRoots = 0;
        x::CyberpunkVR_OwnedSceneChildren = 0;
        out(addr_of_mut!(x::CyberpunkVR_OwnedSceneStats)).fill(0);
        let split = out(addr_of_mut!(x::CyberpunkVR_OwnedSceneSplitStats));
        split.fill(0);
        split[0] = 1;
        out(addr_of_mut!(x::CyberpunkVR_OwnedSceneSplitSections)).fill(0);
        x::CyberpunkVR_OwnedSceneSplitSourceNodes = 0;
        x::CyberpunkVR_OwnedSceneSplitSourceEdges = 0;
        x::CyberpunkVR_OwnedSceneSplitMappingRows = 0;
    }
    stats[0] = 1;
    stats[1] = admission.id;
    stats[2] = admission.epoch;
    stats[3] = admission.pair_generation;
    if closing() || !admission.registered || admission.epoch == 0 || admission.pair_generation == 0 {
        owned_reject(owned_failure::ADMISSION, 0);
        return false;
    }
    if !owned_writer_coverage_ready() {
        owned_reject(owned_failure::WRITER_COVERAGE, 0);
        return false;
    }
    // SAFETY: under the mutex.
    if s.consumers.values().any(|pending| unsafe { pending.get() }.owned_state.is_some()) {
        owned_reject(owned_failure::CAPACITY, 0);
        return false;
    }
    if return_slot > u64::MAX - 0x60 || graph_slot != return_slot + 0x60 || destination != graph_slot + 0x28 || completion_address != graph_slot + 0x180 || graph_slot & 7 != 0 {
        owned_reject(owned_failure::STACK, 0);
        return false;
    }
    let base = base();
    let source_graph = read_u64(graph_slot);
    let frame = read_u64(owner_address);
    let cache = read_u64(admission.renderer + 0x4620);
    let (Some(source_graph), Some(frame), Some(cache)) = (source_graph, frame, cache) else {
        owned_reject(owned_failure::SOURCE, 0);
        return false;
    };
    if !consumer.original_graph.capture(source_graph, &mut reader())
        || !consumer.original_source.capture(cache, source_graph, &source_of(&consumer.original_graph), base, rd!(), gd::ALLOW_CONCURRENT_HITS, gd::OWNER_MEMBERSHIP)
    {
        owned_reject(owned_failure::SOURCE, 0);
        return false;
    }
    let frontier = FrontierPlan::build(&consumer.original_graph, base, if request == 2 { pf::DISCARD } else { pf::SOLID });
    if !frontier.candidate() {
        owned_reject(owned_failure::SOURCE, 0);
        return false;
    }
    let mut edges = frontier.plan.original.clone();
    if request == 2 {
        let coupled = Box::new(CpuFrontierPlan::build(&frontier));
        let ok = coupled.candidate() && coupled.plan.invariants.shared_owner_order_preserved;
        edges = coupled.plan.proposed.clone();
        consumer.coupled_schedule = Some(coupled);
        if !ok {
            owned_reject(owned_failure::SORT, 0);
            return false;
        }
    }
    let mut graph = Box::new(OwnedGraph::default());
    let sorted = graph.create(&consumer.original_graph.nodes, &consumer.original_graph.edges, consumer.original_graph.failure, edges)
        && native_reference_sort(graph.sortable_data())
        && graph.validate_compiled()
        && (request != 1 || graph.same_source_orders());
    consumer.owned_graph = Some(graph);
    if !sorted {
        owned_reject(owned_failure::SORT, 0);
        return false;
    }
    consumer.execution_graph = owned_snapshot(consumer.owned_graph.as_ref().expect("set"));
    let context = crate::context_order::analyze(&consumer.execution_graph.nodes, &consumer.execution_graph.edges, consumer.execution_graph.failure, base, rd!());
    if context.failure != crate::context_order::Failure::None {
        owned_reject(owned_failure::SORT, 0);
        return false;
    }
    // SAFETY: the export.
    let schedule = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionSchedule)) };
    schedule[1] = admission.id;
    schedule[2] = frontier.plan.fingerprint;
    schedule[3] = pf::fingerprint(&consumer.execution_graph.nodes, &consumer.execution_graph.edges, consumer.execution_graph.failure, base);
    schedule[4] = consumer.original_graph.nodes.len() as u64;
    schedule[5] = consumer.original_graph.edges.len() as u64;
    schedule[6] = consumer.execution_graph.edges.len() as u64;
    for (i, node) in consumer.original_graph.nodes.iter().enumerate() {
        let Some(other) = consumer.execution_graph.nodes.get(i) else { break };
        for phase in 0..2 {
            schedule[7 + phase] += (node.bytes[28 + phase] != other.bytes[28 + phase]) as u64;
        }
    }
    schedule[9] = u64::from(context.required_pairs);
    schedule[10] = u64::from(context.proved_pairs);
    schedule[11] = context.missing.len() as u64;
    schedule[12] = consumer.coupled_schedule.as_ref().map_or(0, |plan| plan.plan.shared_owner_orders.len() as u64);
    schedule[13] = consumer.coupled_schedule.as_ref().map_or(0, |plan| plan.plan.reversed_owner_orders.len() as u64);
    schedule[14] = 1;
    s.published_schedule_source = consumer.original_graph.clone();
    // SAFETY: the exports.
    unsafe {
        x::CyberpunkVR_OwnedExecutionScheduleNodes = data(&s.published_schedule_source.nodes);
        x::CyberpunkVR_OwnedExecutionScheduleEdges = data(&s.published_schedule_source.edges);
    }
    let roots: Vec<u64> = consumer.original_source.snapshot.owners.iter().flat_map(|owner| owner.plan.definitions().iter().copied()).collect();
    if !consumer.original_tree.capture(&roots, base, rd!(), true) || !crate::definition_wrappers::tree_certified(&consumer.original_tree) || !consumer.original_tree.matches_before(&mut reader()) {
        owned_reject(owned_failure::TREE, 0);
        return false;
    }
    capture_discard_stages(s, consumer, admission, &frontier);
    capture_scene_sections(s, consumer, admission, &frontier);
    let split_mode = x::CyberpunkVR_OwnedSceneSplitEnable.load(SeqCst);
    if split_mode > 2 {
        // SAFETY: the export.
        unsafe { x::CyberpunkVR_OwnedSceneSplitStats[22] = 1 };
        owned_reject(owned_failure::SCENE_EXPANSION, 1);
        return false;
    }
    if split_mode != 0 && !prepare_scene_expansion(s, consumer, admission, request, split_mode) {
        return false;
    }
    let early = crate::definition_reads::early_group(graph_slot);
    if early == 0 {
        owned_reject(owned_failure::TERMINAL, 0);
        return false;
    }
    consumer.execution_terminal = Some(EarlyTerminalPin::new(early));
    consumer.producer_scope = u64::from(root_depth());
    let stats = owned_stats();
    stats[4] = source_graph;
    stats[5] = consumer.owned_graph.as_mut().map_or(0, |graph| graph.sortable_data() as u64);
    stats[6] = frame;
    stats[7] = early;
    stats[8] = consumer.original_graph.nodes.len() as u64;
    stats[9] = consumer.original_graph.edges.len() as u64;
    stats[10] = consumer.original_tree.nodes().len() as u64;
    stats[11] = consumer.original_tree.arrays().len() as u64;
    stats[12] = consumer.original_tree.strings().len() as u64;
    stats[13] = frontier.plan.fingerprint;
    true
}

// ---- the generations
/// BindSource.
fn bind_source(s: &mut State, consumer: &mut Consumer, renderer: u64, admission: dg::Admission) {
    let ticket = consumer.ticket();
    consumer.renderer = renderer;
    if !s.generation_ledger.has_consumer(ticket.id()) {
        reject_generations(s, consumer, dg::Failure::Consumer);
        return;
    }
    let mut snapshot = Snapshot::default();
    let mut receipt = gd::Receipt::default();
    let cache_base = if renderer != 0 { read_u64(renderer + 0x4620) } else { None };
    let captured = cache_base.is_some_and(|cache_base| {
        snapshot.capture(ticket.source_graph_address(), &mut reader())
            && receipt.capture(cache_base, ticket.source_graph_address(), &source_of(&snapshot), base(), rd!(), gd::ALLOW_CONCURRENT_HITS, gd::OWNER_MEMBERSHIP)
    });
    if !captured {
        consumer.rejection_detail = receipt.failure as u64 | u64::from(receipt.changed_details) << 32;
        reject_generations(s, consumer, dg::Failure::Generation);
        return;
    }
    for owner in &receipt.snapshot.owners {
        let definitions = owner.plan.definitions();
        if !definitions.iter().any(|&object| object != 0) {
            continue;
        }
        let header = owner.plan.owner_address();
        let found = s.mutations.get(&header).copied();
        if !found.is_some_and(|f| f.epoch != 0 && f.in_flight == 0 && f.kind == 1) {
            consumer.rejection_header = header;
            reject_generations(s, consumer, dg::Failure::Mutation);
            return;
        }
        let mut stamp = found.expect("checked");
        if stamp.generation == 0 {
            stamp.generation = s.generation_ledger.birth_generation(&dg::HeaderObservation { owner_header: header, born_mutation_epoch: stamp.epoch, sequence_before: stamp.epoch, sequence_after: stamp.epoch, mutation_in_flight: false }, false);
            s.mutations.get_mut(&header).expect("present").generation = stamp.generation;
            if stamp.generation == 0 {
                let reason = s.generation_ledger.rejection();
                reject_generations(s, consumer, reason);
                return;
            }
            x::CyberpunkVR_PreparationGenerationStats[3].fetch_add(1, SeqCst);
        }
        if !s.generation_ledger.consumer_bound(ticket.id(), stamp.generation) && !s.generation_ledger.bind_consumer(ticket.id(), ticket.epoch(), ticket.generation(), stamp.generation, stamp.epoch, admission) {
            let reason = s.generation_ledger.rejection();
            reject_generations(s, consumer, reason);
            return;
        }
        let binding = DefinitionBinding { generation: stamp.generation, born_epoch: stamp.epoch };
        if !consumer.generations.iter().any(|value| value.generation == stamp.generation) {
            consumer.generations.push(binding);
        }
        for &object in definitions {
            if object == 0 {
                continue;
            }
            if admission == dg::Admission::Execution {
                consumer.definitions.insert(object, binding);
            } else if consumer.definitions.contains_key(&object) {
                reject_generations(s, consumer, dg::Failure::Duplicate);
                return;
            } else {
                consumer.definitions.insert(object, binding);
            }
        }
    }
    if admission == dg::Admission::Source {
        consumer.source_verified = true;
        consumer.renderer = renderer;
        x::CyberpunkVR_PreparationGenerationStats[5].fetch_add(1, SeqCst);
    } else {
        consumer.execution_verified = true;
        x::CyberpunkVR_PreparationGenerationStats[6].fetch_add(1, SeqCst);
    }
    publish_generation_stats(s);
}
/// VerifyExecution.
fn verify_execution(s: &mut State, consumer: &mut Consumer) {
    if consumer.owned_state.is_some() {
        let base = base();
        let mut valid = consumer.owned_graph.as_ref().is_some_and(|graph| graph.validate_compiled());
        let current = consumer.owned_graph.as_ref().map(|graph| owned_snapshot(graph)).unwrap_or_default();
        if valid && consumer.scene_both_expansion.is_some() {
            let plan = consumer.scene_both_expansion.as_ref().expect("checked");
            valid = plan.matches_source(&consumer.coupled_graph, base) && plan.matches_compiled(&current) && validate_scene_views(s, consumer);
        } else if valid && consumer.scene_expansion.is_some() {
            let plan = consumer.scene_expansion.as_ref().expect("checked");
            valid = plan.matches_source(&consumer.coupled_graph, base) && plan.matches_compiled(&current) && validate_scene_views(s, consumer);
        } else if valid && consumer.coupled_schedule.is_some() {
            valid = consumer.coupled_schedule.as_ref().is_some_and(|plan| plan.matches(&consumer.original_graph, base))
                && current.nodes.len() == consumer.execution_graph.nodes.len()
                && current.edges == consumer.execution_graph.edges
                && current.nodes.iter().zip(&consumer.execution_graph.nodes).all(|(a, b)| same_nodes(a, b));
        } else if valid {
            valid = consumer.owned_graph.as_ref().is_some_and(|graph| graph.same_source_orders());
        }
        for entry in &consumer.extra_readers {
            // SAFETY: a boxed owner under the mutex.
            let owner = unsafe { &*entry.owner };
            let expected = owner.identity.saved_native;
            let state = s.rs_ref(entry.owner);
            valid = valid && state.is_some_and(|state| !state.destroy_attempt_consumed()) && owner.tree.matches_before(&mut reader()) && crate::definition_wrappers::allocators_match(&owner.tree, &mut reader());
            match state {
                Some(state) if !state.exact_header_detached() && !owner.emergency_detached => {
                    valid = valid && native_header_read(owner.identity.owner_header) == Some(expected);
                }
                Some(state) => valid = valid && *state.saved_header() == expected,
                None => {}
            }
            if expected.capacity != 0 {
                valid = valid && read_u64(expected.data + u64::from(expected.capacity) * 8) == Some(owner.identity.empty_allocator.data);
            }
        }
        consumer.execution_verified = valid;
        if consumer.has_scene_expansion() {
            // SAFETY: the export.
            unsafe {
                x::CyberpunkVR_OwnedSceneSplitStats[21] = valid as u64;
                if !valid {
                    x::CyberpunkVR_OwnedSceneSplitStats[22] = 8;
                }
            }
        }
        if !valid {
            consumer.rejected = true;
            if let Some(state) = owned(s, consumer) {
                state.report_failure(MISSING_PROOF);
            }
            owned_reject(owned_failure::EXECUTION, 0);
        }
        return;
    }
    if consumer.generation_rejected || !consumer.source_verified {
        return;
    }
    // Resolve actual native headers/stamps again; object addresses alone cannot distinguish allocator reuse between
    // queueing and execution.
    let renderer = consumer.renderer;
    bind_source(s, consumer, renderer, dg::Admission::Execution);
}

// ---- the definition capture
/// Capture: the selected root's source definitions, owners and tree.
fn capture_definitions(owner_address: u64, caller: u64, ticket: Option<&Ticket>) -> Option<Ticket> {
    let base = base();
    let epoch = || u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst));
    let pair_generation = || crate::native_state_exports::CyberpunkVR_PreparationPairGeneration.load(SeqCst);
    if x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) != 1
        || !crate::preparation_trace::active()
        || crate::native_state_exports::CyberpunkVR_PreparationPairState.load(SeqCst) != 2
        || caller != base + CONSTRUCT_RETURN
        || x::CyberpunkVR_PreparationDefinitionRequest.load(SeqCst) == 0
    {
        return None;
    }
    if let Some(ticket) = ticket {
        // SAFETY: under no lock yet, but only read; the .cpp read it the same way.
        let t = unsafe { ticket.get() };
        if t.epoch() != epoch() || t.generation() != pair_generation() {
            return None;
        }
    }
    let retired;
    let g = lock();
    let s = g.state();
    if x::CyberpunkVR_PreparationDefinitionRequest.swap(0, SeqCst) == 0 {
        return None;
    }
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
    // SAFETY: the exports.
    let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionStats)) };
    stats.fill(0);
    stats[0] = 1;
    // SAFETY: the exports.
    unsafe {
        out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionAggregate)).fill(0);
        out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionDeclaration)).fill(0);
        out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionTicket)).fill(0);
        out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionTreeStats)).fill(0);
        x::CyberpunkVR_PreparationDefinitionTreeRoots = 0;
        x::CyberpunkVR_PreparationDefinitionTreeNodes = 0;
        x::CyberpunkVR_PreparationDefinitionTreeHandles = 0;
        x::CyberpunkVR_PreparationDefinitionTreeArrays = 0;
        x::CyberpunkVR_PreparationDefinitionTreeAllocators = 0;
        x::CyberpunkVR_PreparationDefinitionTreeStrings = 0;
        x::CyberpunkVR_PreparationDefinitionOwners = 0;
        x::CyberpunkVR_PreparationDefinitionMappings = 0;
        x::CyberpunkVR_PreparationDefinitionEntries = 0;
    }
    retired = s.source_ticket.take();
    s.source = gd::Receipt::default();
    s.owners.clear();
    s.mappings.clear();
    s.entries.clear();
    s.capture_owner = 0;
    s.definition_tree = Tree::default();
    s.capture_epoch = epoch();
    s.capture_generation = pair_generation();
    let (capture_epoch, capture_generation) = (s.capture_epoch, s.capture_generation);
    let current = || x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) == 1 && capture_epoch == epoch() && capture_generation == pair_generation();
    let reject = |stats: &mut [u64; 16], reason: u64| {
        stats[15] = reason;
        stats[0] = 4;
    };
    // SAFETY: under the mutex.
    let captured = ticket.filter(|ticket| unsafe { ticket.get() }.captured());
    let Some(ticket) = captured else {
        reject(stats, 500);
        x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
        drop(g);
        drop(retired);
        return None;
    };
    s.source_ticket = Some(ticket.clone());
    // SAFETY: under the mutex.
    let t = unsafe { ticket.get() };
    let graph = t.source_graph_address();
    let references = read_u64(owner_address).filter(|&owner| owner != 0 && owner <= u64::MAX - 12).and_then(|owner| {
        s.capture_owner = owner;
        read_u32(owner + 8)
    });
    if let Some(owner) = read_u64(owner_address) {
        s.capture_owner = owner;
    }
    let renderer = read_u64(base + RENDERER_GLOBAL_RVA).filter(|&renderer| renderer != 0);
    let cache_base = renderer.and_then(|renderer| read_u64(renderer + 0x4620));
    if capture_epoch == 0 || MUTATION_OVERFLOW.load(SeqCst) || s.capture_owner == 0 || s.capture_owner > u64::MAX - 12 || !references.is_some_and(|r| (r as i32) > 0) || graph == 0 || cache_base.is_none() {
        reject(stats, 100);
    } else {
        let cache_base = cache_base.expect("checked");
        let mut snapshot = Snapshot::default();
        let mut source = gd::Receipt::default();
        if !snapshot.capture(graph, &mut reader()) || !source.capture(cache_base, graph, &source_of(&snapshot), base, rd!(), gd::STABLE_COUNTER, gd::IMMUTABLE_PREFIX) {
            reject(stats, 200 + source.failure as u64);
            s.source = source;
        } else {
            s.source = source;
            for owner in &s.source.snapshot.owners {
                let header = owner.plan.owner_address();
                let found = s.mutations.get(&header).copied();
                let has_definitions = owner.plan.definitions().iter().any(|&value| value != 0);
                if has_definitions && !found.is_some_and(|f| f.in_flight == 0 && f.kind == 1) {
                    reject(stats, 300);
                    break;
                }
                let native = owner.plan.saved_header();
                let stamp = found.unwrap_or_default();
                s.owners.push(OwnerRow {
                    graph: owner.per_view_graph,
                    header,
                    data: native.data,
                    allocator: owner.plan.empty_header().data,
                    epoch: stamp.epoch,
                    entry_start: s.entries.len() as u64,
                    capacity: native.capacity,
                    count: native.count,
                    in_flight: stamp.in_flight,
                    kind: stamp.kind,
                });
                for &value in owner.plan.definitions() {
                    s.entries.push(value);
                    stats[4] += (value == 0) as u64;
                }
            }
            if stats[0] != 4 && current() {
                s.mappings = s.source.snapshot.mappings.clone();
                stats[1] = s.owners.len() as u64;
                stats[2] = s.mappings.len() as u64;
                stats[3] = s.entries.len() as u64;
                stats[5] = s.source.fingerprint;
                stats[6] = s.capture_owner;
                stats[7] = graph;
                stats[8] = cache_base;
                stats[9] = s.source.slot;
                stats[10] = s.source.snapshot.key;
                stats[11] = capture_epoch;
                stats[12] = capture_generation;
                stats[13] = caller - base;
                // SAFETY: the exports.
                unsafe {
                    x::CyberpunkVR_PreparationDefinitionOwners = data(&s.owners);
                    x::CyberpunkVR_PreparationDefinitionMappings = data(&s.mappings);
                    x::CyberpunkVR_PreparationDefinitionEntries = data(&s.entries);
                }
                stats[0] = 2;
                // SAFETY: the export.
                let tree = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionTreeStats)) };
                tree[0] = 1;
                tree[1] = capture_epoch;
                tree[2] = capture_generation;
                tree[3] = t.id();
                let entries = s.entries.clone();
                if s.definition_tree.capture(&entries, base, rd!(), true) {
                    let d = &s.definition_tree;
                    tree[0] = 2;
                    tree[4] = d.roots().len() as u64;
                    tree[5] = d.nodes().len() as u64;
                    tree[6] = d.handles().len() as u64;
                    tree[7] = d.arrays().len() as u64;
                    tree[8] = d.allocators().len() as u64;
                    tree[10] = d.strings().len() as u64;
                    // SAFETY: the exports.
                    unsafe {
                        x::CyberpunkVR_PreparationDefinitionTreeRoots = d.roots().as_ptr() as u64;
                        x::CyberpunkVR_PreparationDefinitionTreeNodes = d.nodes().as_ptr() as u64;
                        x::CyberpunkVR_PreparationDefinitionTreeHandles = d.handles().as_ptr() as u64;
                        x::CyberpunkVR_PreparationDefinitionTreeArrays = d.arrays().as_ptr() as u64;
                        x::CyberpunkVR_PreparationDefinitionTreeAllocators = d.allocators().as_ptr() as u64;
                        x::CyberpunkVR_PreparationDefinitionTreeStrings = d.strings().as_ptr() as u64;
                    }
                } else {
                    tree[0] = 3;
                    tree[9] = s.definition_tree.rejection() as u64;
                }
            } else if stats[0] != 4 {
                reject(stats, 301);
            }
        }
    }
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
    let selected = s.source_ticket.clone();
    drop(g);
    drop(retired);
    selected
}

// ---- the registration of a consumer
/// RegisterConsumer.
fn register_consumer(destination: u64, owner: u64, graph: u64, completion: u64, caller: u64, return_slot: u64) -> Option<ConsumerRef> {
    let base = base();
    if closing() || caller != base + CONSTRUCT_RETURN {
        return None;
    }
    let _window = AdmissionWindow::new();
    consumers_stat(0).fetch_add(1, SeqCst);
    let value = Shared::new(Consumer::default());
    // SAFETY: not yet shared.
    let c = unsafe { value.get() };
    c.ticket = Some(Shared::new(NativeRootTicket::default()));
    let g = lock();
    let s = g.state();
    let has_admission = with_admission(|admission| admission.constructed = true).is_some();
    if s.consumers.len() >= MAX_CONSUMERS {
        match with_admission(|admission| *admission).filter(|admission| admission.registered) {
            Some(admission) => {
                s.generation_ledger.unsupported_consumer(admission.id, admission.epoch, admission.pair_generation, dg::Failure::Capacity);
            }
            None => lose_generation_coverage(s, dg::Failure::Capacity),
        }
        consumers_stat(6).fetch_add(1, SeqCst);
        return None;
    }
    let selected = (x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) == 1 || x::CyberpunkVR_OwnedExecutionMode.load(SeqCst) == 1) && crate::preparation_trace::active();
    let epoch = if selected { u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)) } else { 0 };
    let generation = if selected { crate::native_state_exports::CyberpunkVR_PreparationPairGeneration.load(SeqCst) } else { 0 };
    let id = match with_admission(|admission| admission.id) {
        Some(id) => id,
        None => {
            s.next_consumer += 1;
            s.next_consumer
        }
    };
    if with_admission(|admission| admission.registered) == Some(true) {
        if s.generation_ledger.seal_provisional_consumer(id, epoch, generation) {
            with_admission(|admission| {
                admission.epoch = epoch;
                admission.pair_generation = generation;
            });
        } else {
            let admission = with_admission(|admission| *admission).expect("registered");
            s.generation_ledger.unsupported_consumer(id, admission.epoch, admission.pair_generation, dg::Failure::Unsupported);
            with_admission(|admission| admission.registered = false);
            lose_generation_coverage(s, dg::Failure::Epoch);
        }
    }
    let admission = with_admission(|admission| *admission);
    let prepared = admission.is_some_and(|admission| prepare_owned_reference(s, c, &admission, destination, owner, graph, completion, return_slot));
    let expected = if prepared { c.owned_graph.as_mut().map_or(0, |graph| graph.sortable_data() as u64) } else { 0 };
    let registered = admission.is_some_and(|admission| admission.registered);
    // SAFETY: not yet shared.
    // SAFETY: not yet shared; the engine's owner, graph and completion are live in this construct call.
    let captured = unsafe { c.ticket.as_ref().expect("set").get().capture_before_construct(owner, graph, completion, caller, base, epoch, generation, id, rd!(), job_api(false), frame_api(), expected) };
    if !captured || s.consumers.contains_key(&c.ticket().group()) {
        if registered {
            s.generation_ledger.unsupported_consumer(id, epoch, generation, dg::Failure::Unsupported);
        } else {
            lose_generation_coverage(s, dg::Failure::Consumer);
        }
        consumers_stat(5).fetch_add(1, SeqCst);
        return None;
    }
    if prepared {
        let fallback = Shared::new(NativeRootTicket::default());
        let mut declaration = Box::new(DeclarationCompletionLease::default());
        // SAFETY: not yet shared.
        let f = unsafe { fallback.get() };
        // SAFETY: as above.
        let ok = unsafe {
            f.capture_before_construct(owner, graph, completion, caller, base, epoch, generation, id, rd!(), job_api(false), frame_api(), 0)
                && declaration.capture_before_construct(owner, f.graph_address(), caller, base, epoch, generation, id, rd!(), job_api(true))
        };
        c.fallback_ticket = Some(fallback);
        c.fallback_declaration = Some(declaration);
        if !ok {
            s.generation_ledger.unsupported_consumer(id, epoch, generation, dg::Failure::Unsupported);
            owned_reject(owned_failure::TICKET, 0);
            return None;
        }
    }
    let group = c.ticket().group();
    s.consumers.insert(group, value.clone());
    consumers_stat(1).fetch_add(1, SeqCst);
    if has_admission {
        with_admission(|admission| admission.prerequisite_group = group);
    }
    if selected && epoch != 0 && generation != 0 {
        let mut declaration = Box::new(DeclarationCompletionLease::default());
        // SAFETY: as above.
        unsafe { declaration.capture_before_construct(owner, c.ticket().graph_address(), caller, base, epoch, generation, id, rd!(), job_api(true)) };
        c.declaration = Some(declaration);
    }
    if registered {
        let renderer = admission.map_or(0, |admission| admission.renderer);
        bind_source(s, c, renderer, dg::Admission::Source);
    } else {
        reject_generations(s, c, dg::Failure::PriorCoverage);
    }
    if prepared {
        let terminal = c.execution_terminal.as_ref().map_or(0, EarlyTerminalPin::group);
        let fails = !c.source_verified
            || c.generation_rejected
            || !c.declaration.as_ref().is_some_and(|d| d.captured())
            || terminal == c.ticket().group()
            || c.declaration.as_ref().is_some_and(|d| d.group() == terminal)
            || !reserve_extra_owners(s, c, graph);
        if fails {
            cancel_extra_readers(s, c, true);
            if let Some(owned_id) = c.owned_state.take() {
                if let Some(state) = s.owned_execution_states.find(owned_id) {
                    state.cancel_selection();
                }
                s.owned_execution_states.retire(id);
            }
            c.owned_graph = None;
            c.execution_terminal = None;
            recycle_retained_metadata(s);
            publish_retirement_receipt(s);
            c.ticket = c.fallback_ticket.take();
            c.declaration = c.fallback_declaration.take();
        }
    }
    consumers_stat(7).store(s.consumers.len() as u64, SeqCst);
    Some(value)
}

// ---- the construct hook (its frame stays in the .cpp for _AddressOfReturnAddress)
/// ConstructHook.
///
/// # Safety
/// The engine's arguments; `original` is the trampoline.
#[no_mangle]
pub unsafe extern "C" fn cvr_ndt_construct(destination: *mut c_void, owner: *mut c_void, graph: *mut c_void, unused: *mut c_void, completion: *mut c_void, caller: u64, return_slot: u64, original: usize) -> *mut c_void {
    poll();
    let consumer = register_consumer(destination as u64, owner as u64, graph as u64, completion as u64, caller, return_slot);
    if let Some(consumer) = consumer.as_ref() {
        let g = lock();
        let s = g.state();
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if c.owned_state.is_some() {
            let _write = TelemetryWrite::new();
            let state = owned(s, c).expect("admitted");
            let identity = state.identity;
            let authorized = !closing() && state.preconstruct_graph_slot(c.producer_scope) == graph as u64 && state.phase == oge::phase::PREPARED;
            let matched = authorized && compare_graph_slot(graph as u64, identity.source_graph, identity.execution_graph) == Some(identity.source_graph);
            let verified = matched && read_u64(graph as u64) == Some(identity.execution_graph);
            if authorized {
                state.record_publication(&identity, c.producer_scope, verified);
            } else {
                state.cancel_selection();
            }
            if !verified {
                let restored = !authorized || compare_graph_slot(graph as u64, identity.execution_graph, identity.source_graph).is_some_and(|rollback| rollback == identity.execution_graph || rollback == identity.source_graph);
                let rollback_verified = restored && read_u64(graph as u64) == Some(identity.source_graph);
                if authorized {
                    owned(s, c).expect("admitted").record_rollback(&identity, c.producer_scope, rollback_verified);
                }
                cancel_extra_readers(s, c, rollback_verified);
                owned_reject(if rollback_verified { owned_failure::PUBLICATION } else { owned_failure::ROLLBACK }, 0);
                let state = owned(s, c).expect("admitted");
                if rollback_verified && state.can_release_owned_storage() && !state.escaped {
                    s.owned_execution_states.retire(identity.consumer_id);
                    c.owned_state = None;
                    c.owned_graph = None;
                    c.execution_terminal = None;
                    recycle_retained_metadata(s);
                    publish_retirement_receipt(s);
                    c.ticket = c.fallback_ticket.take();
                    c.declaration = c.fallback_declaration.take();
                }
            } else {
                owned(s, c).expect("admitted").mark_construct_invoked(&identity, c.producer_scope);
                for reader in c.extra_readers.clone() {
                    if let Some(state) = s.rs(reader.owner) {
                        state.mark_extra_reader_escaped(&reader.identity);
                    }
                }
                let stats = owned_stats();
                stats[0] = 2;
                stats[14] = 1;
            }
        }
    }
    let ticket = consumer.as_ref().and_then(|consumer| {
        // SAFETY: the consumer's ticket handle, cloned.
        unsafe { consumer.get() }.ticket.clone()
    });
    let selected = capture_definitions(owner as u64, caller, ticket.as_ref());
    if let (Some(_), Some(consumer)) = (selected.as_ref(), consumer.as_ref()) {
        let g = lock();
        // SAFETY: under the mutex.
        publish_selected_generations(g.state(), unsafe { consumer.get() }, false);
    }
    // SAFETY: MinHook's trampoline to the original.
    let original: extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void, *mut c_void) -> *mut c_void = unsafe { core::mem::transmute(original) };
    let result = original(destination, owner, graph, unused, completion);
    let mut retired_consumer = None;
    let mut retired_ticket = None;
    if let (Some(ticket), Some(consumer)) = (ticket.as_ref(), consumer.as_ref()) {
        let g = lock();
        let s = g.state();
        // SAFETY: under the mutex.
        let (t, c) = unsafe { (ticket.get(), consumer.get()) };
        let verified = t.verify_construct(destination as u64, rd!());
        c.fallback_ticket = None;
        c.fallback_declaration = None;
        if c.owned_state.is_some() {
            let _write = TelemetryWrite::new();
            let state = owned(s, c).expect("admitted");
            let identity = state.identity;
            state.record_construct(&identity, verified, verified);
            owned_stats()[15] = verified as u64;
            if !verified {
                owned_reject(owned_failure::CONSTRUCT, 0);
            }
        }
        if !verified {
            consumers_stat(5).fetch_add(1, SeqCst);
            reject_generations(s, c, dg::Failure::Consumer);
        }
        let is_source = s.source_ticket.as_ref().is_some_and(|source| Arc::ptr_eq(source, ticket));
        if selected.is_some() && is_source && s.capture_epoch == u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)) && x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) == 1 {
            x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
            let row = [t.id(), t.group(), t.frame_owner(), t.graph_address(), t.epoch(), t.generation(), verified as u64, 0];
            // SAFETY: the exports.
            unsafe {
                *out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionTicket)) = row;
                if !verified {
                    x::CyberpunkVR_PreparationDefinitionStats[0] = 4;
                    x::CyberpunkVR_PreparationDefinitionStats[15] = 600 + t.rejection() as u64;
                }
            }
            x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
        }
        if !verified && c.owned_state.is_none() {
            let group = t.group();
            if s.consumers.get(&group).is_some_and(|found| Arc::ptr_eq(found, consumer)) {
                retired_consumer = s.consumers.remove(&group);
                consumers_stat(8).fetch_add(1, SeqCst);
            }
            if is_source {
                retired_ticket = s.source_ticket.take();
            }
            consumers_stat(7).store(s.consumers.len() as u64, SeqCst);
        }
    }
    drop(retired_consumer);
    drop(retired_ticket);
    result
}
/// The construct detour's wish (CaptureCompatible), for the .cpp's registration.
#[no_mangle]
pub extern "C" fn cvr_ndt_capture_compatible() -> bool {
    capture_compatible()
}

// ---- the queue, compiler and execution hooks
static mut QUEUE_EXECUTION: usize = 0;
extern "C" fn queue_execution_hook(job: *mut c_void, unused: *mut c_void, payload: *mut c_void) -> usize {
    let original = original!(QUEUE_EXECUTION: fn(*mut c_void, *mut c_void, *mut c_void) -> usize);
    if closing() {
        return original(job, unused, payload);
    }
    let caller = return_address();
    {
        let g = lock();
        let s = g.state();
        let p = payload as u64;
        if caller == base() + 0x40A4B7 {
            if let (Some(owner), Some(graph), Some(prerequisite), Some(terminal)) = (read_u64(p), read_u64(p + 8), read_u64(p + 0x18), read_u64(job as u64 + 0x18)) {
                if let Some(found) = s.consumers.get(&prerequisite).cloned() {
                    // SAFETY: under the mutex.
                    let c = unsafe { found.get() };
                    if c.owned_state.is_some() {
                        let _write = TelemetryWrite::new();
                        let identity = owned(s, c).expect("admitted").identity;
                        let valid = owner == identity.frame_owner
                            && graph == identity.execution_graph
                            && c.ticket().construct_verified()
                            && c.execution_terminal.as_ref().is_some_and(|pin| terminal == pin.group());
                        let state = owned(s, c).expect("admitted");
                        if !valid || !state.record_queued(EXECUTION, &identity, terminal, true) {
                            state.report_failure(MISSING_PROOF);
                            owned_reject(owned_failure::QUEUE, 0);
                        } else {
                            owned_stats()[0] = 3;
                        }
                    }
                }
            }
        }
    }
    original(job, unused, payload)
}
static mut COMPILER_DISPATCH: usize = 0;
extern "C" fn compiler_dispatch_hook(job: *mut c_void, unused: *mut c_void, renderer_argument: *mut c_void) -> usize {
    let original = original!(COMPILER_DISPATCH: fn(*mut c_void, *mut c_void, *mut c_void) -> usize);
    if closing() {
        return original(job, unused, renderer_argument);
    }
    let caller = return_address();
    let mut consumer: Option<ConsumerRef> = None;
    {
        let g = lock();
        let s = g.state();
        if let Some(group) = with_admission(|admission| admission.prerequisite_group) {
            if let Some(found) = s.consumers.get(&group).cloned() {
                // SAFETY: under the mutex.
                let c = unsafe { found.get() };
                if c.owned_state.is_some() {
                    consumer = Some(found.clone());
                    let _write = TelemetryWrite::new();
                    let ticket = c.ticket();
                    if c.compiler.is_none() {
                        c.compiler = Some(Box::default());
                    }
                    c.compiler_producer_identity = c.declaration.as_ref().map_or(0, |d| d.context_address());
                    let binding = CompilerBinding {
                        consumer_id: ticket.id(),
                        epoch: ticket.epoch(),
                        generation: ticket.generation(),
                        renderer: c.renderer,
                        wrapper: c.resource_wrapper,
                        pool: c.resource_pool,
                        frame_owner: ticket.frame_owner(),
                        execution_graph: ticket.graph_address(),
                        producer_context: c.compiler_producer_identity,
                    };
                    let declared = c.declaration.as_ref().is_some_and(|d| d.matched()) && c.declaration_snapshot_ready && ticket.construct_verified();
                    let captured = declared && unsafe { c.compiler.as_mut().expect("set").capture_before_dispatch(&binding, job as u64, renderer_argument as u64, caller, base(), rd!(), job_api(true)) }; // SAFETY: the dispatching job is live.
                    if !captured {
                        if let Some(state) = owned(s, c) {
                            state.report_failure(MISSING_PROOF);
                        }
                        owned_reject(owned_failure::RESOURCE, 0);
                    }
                    publish_compiler_receipt(c);
                }
            }
        }
    }
    let result = original(job, unused, renderer_argument);
    if let Some(consumer) = consumer {
        let g = lock();
        let s = g.state();
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if !c.compiler.as_mut().is_some_and(|compiler| compiler.mark_original_dispatch_returned()) {
            if let Some(state) = owned(s, c) {
                state.report_failure(MISSING_PROOF);
            }
            owned_reject(owned_failure::RESOURCE, 0);
        }
        publish_compiler_receipt(c);
    }
    result
}
static mut EXECUTE: usize = 0;
fn reject_diagnostic(kind: u64, detail: u64, caller: u64, worker: u64) -> &'static mut [u64; 8] {
    // SAFETY: the export.
    let diagnostic = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationNativeConsumerReject)) };
    diagnostic[0] = kind;
    diagnostic[1] = detail;
    diagnostic[2] = caller.wrapping_sub(base());
    diagnostic[3] = worker;
    let mut bytes = [0u8; 8];
    read(worker + 0x20, &mut bytes);
    diagnostic[4] = u64::from_le_bytes(bytes);
    if diagnostic[4] != 0 {
        let mut again = [0u8; 8];
        read(diagnostic[4], &mut again);
        diagnostic[5] = u64::from_le_bytes(again);
    }
    diagnostic
}
extern "C" fn execute_hook(payload: *mut c_void, worker: *mut c_void) {
    let original = original!(EXECUTE: fn(*mut c_void, *mut c_void) -> ());
    if closing() {
        original(payload, worker);
        return;
    }
    let caller = return_address();
    let mut consumer: Option<ConsumerRef> = None;
    let p = payload as u64;
    match (read_u64(p + 8), read_u64(p + 0x18)) {
        (Some(graph), Some(group)) => {
            let g = lock();
            let s = g.state();
            // SAFETY: under the mutex.
            let found = s.consumers.get(&group).cloned().filter(|found| !unsafe { found.get() }.callback_entered);
            match found {
                Some(found) => {
                    // SAFETY: under the mutex.
                    let c = unsafe { found.get() };
                    consumer = Some(found.clone());
                    c.callback_entered = true;
                    consumers_stat(2).fetch_add(1, SeqCst);
                    if !c.ticket().match_execution_callback(p, graph, caller, rd!()) {
                        reject_diagnostic(2, group, caller, worker as u64);
                        c.rejected = true;
                        consumers_stat(5).fetch_add(1, SeqCst);
                    } else {
                        let mut completion = Box::new(TaskCompletionLease::default());
                        // SAFETY: the callback's worker is live for this call.
                        if !unsafe { completion.capture_before_callback(worker as u64, caller, base(), group, rd!(), job_api(true)) } {
                            let diagnostic = reject_diagnostic(3, completion.rejection() as u64, caller, worker as u64);
                            let (mut refs, mut pending) = (0i32, 0i32);
                            if diagnostic[5] != 0 {
                                refs = read_u32(diagnostic[5] + 0x1C).map_or(0, |v| v as i32);
                                pending = read_u32(diagnostic[5] + 0x18).map_or(0, |v| v as i32);
                            }
                            diagnostic[6] = u64::from(refs as u32);
                            diagnostic[7] = u64::from(pending as u32);
                            c.rejected = true;
                            consumers_stat(5).fetch_add(1, SeqCst);
                        }
                        let captured_group = completion.captured().then(|| completion.group());
                        c.completion = Some(completion);
                        if c.owned_state.is_some() {
                            if let Some(group) = captured_group {
                                if !c.execution_terminal.as_ref().is_some_and(|pin| group == pin.group()) {
                                    c.rejected = true;
                                    owned(s, c).expect("admitted").report_failure(oge::failure::TERMINAL_GROUP);
                                    owned_reject(owned_failure::TERMINAL, 0);
                                }
                            }
                        }
                        if !c.rejected {
                            verify_execution(s, c);
                        }
                    }
                    if c.rejected {
                        reject_generations(s, c, dg::Failure::TerminalProof);
                    }
                }
                None => {
                    consumers_stat(5).fetch_add(1, SeqCst);
                    lose_generation_coverage(s, dg::Failure::Consumer);
                    // SAFETY: the export.
                    let diagnostic = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationNativeConsumerReject)) };
                    diagnostic[0] = 1;
                    diagnostic[1] = group;
                    diagnostic[2] = caller.wrapping_sub(base());
                    diagnostic[3] = worker as u64;
                }
            }
        }
        _ => {
            let g = lock();
            lose_generation_coverage(g.state(), dg::Failure::Consumer);
            consumers_stat(5).fetch_add(1, SeqCst);
        }
    }
    original(payload, worker);
    if let Some(consumer) = consumer.as_ref() {
        let g = lock();
        let s = g.state();
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        c.callback_returned = true;
        if c.completion.as_mut().is_some_and(|completion| completion.mark_original_returned()) {
            consumers_stat(3).fetch_add(1, SeqCst);
        } else {
            c.rejected = true;
            reject_generations(s, c, dg::Failure::TerminalProof);
            consumers_stat(5).fetch_add(1, SeqCst);
        }
    }
    if let Some(consumer) = consumer.as_ref() {
        let g = lock();
        let s = g.state();
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if c.owned_state.is_some() {
            let _write = TelemetryWrite::new();
            let returned = c.completion.as_ref().is_some_and(|completion| completion.original_returned());
            let group = c.completion.as_ref().map_or(0, |completion| completion.group());
            let state = owned(s, c).expect("admitted");
            if c.rejected || !returned {
                state.report_failure(MISSING_PROOF);
                owned_stats()[0] = 6;
            } else {
                let identity = state.identity;
                state.record_original_returned(EXECUTION, &identity, group);
                owned_stats()[19] = 1;
            }
        }
    }
}
/// The pre-queue definition owners' construct (sub_140C0F98C)'s original, written by the install pass; the thunk below
/// passes it to `cvr_ndt_construct`.
#[no_mangle]
pub static cvr_ndt_construct_original: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
// The construct hook (formerly NativeDefinitionTracking.cpp's ConstructHook, kept in C++ for its frame): the engine's
// five arguments, then its return address (_ReturnAddress), that address's stack slot (_AddressOfReturnAddress, the
// caller's frame the owned reference's layout is checked against) and the original, into `cvr_ndt_construct`.
core::arch::global_asm!(
    ".globl cvr_ndt_construct_hook",
    ".def cvr_ndt_construct_hook",
    ".scl 2",
    ".type 32",
    ".endef",
    "cvr_ndt_construct_hook:",
    ".seh_proc cvr_ndt_construct_hook",
    "    sub rsp, 0x48",
    ".seh_stackalloc 0x48",
    ".seh_endprologue",
    "    mov rax, [rsp+0x70]",
    "    mov [rsp+0x20], rax",
    "    mov rax, [rsp+0x48]",
    "    mov [rsp+0x28], rax",
    "    lea rax, [rsp+0x48]",
    "    mov [rsp+0x30], rax",
    "    mov rax, [rip+cvr_ndt_construct_original]",
    "    mov [rsp+0x38], rax",
    "    call cvr_ndt_construct",
    "    add rsp, 0x48",
    "    ret",
    ".seh_endproc",
);
extern "C" {
    fn cvr_ndt_construct_hook();
}
/// ConstructWanted: the capture's contract.
extern "C" fn construct_wanted() -> bool {
    capture_compatible()
}
pub(super) fn register_detours(none: *mut bool, execute_compatible: extern "C" fn() -> bool) {
    use cvr_engine::detours::register;
    let detour = |f: usize| f as *mut c_void;
    register("[single-pass] pre-queue definition owners", 0xC0F98C, detour(cvr_ndt_construct_hook as *const () as usize), cvr_ndt_construct_original.as_ptr().cast(), Some(construct_wanted), none);
    register("[single-pass] owned reference execution publication", 0xB52BF0, detour(queue_execution_hook as *const () as usize), addr_of_mut!(QUEUE_EXECUTION).cast(), Some(code_compatible), none);
    register("[single-pass] owned compiler dispatch identity", 0xB66B00, detour(compiler_dispatch_hook as *const () as usize), addr_of_mut!(COMPILER_DISPATCH).cast(), Some(code_compatible), none);
    register("[single-pass] native root terminal completion", 0xB79044, detour(execute_hook as *const () as usize), addr_of_mut!(EXECUTE).cast(), Some(execute_compatible), none);
}

// ---- the public calls
/// BeforeCacheLookup.
pub fn before_cache_lookup(cache: u64, use_: u32, key: u64, views: u32, missed: u64, caller: u64) {
    if root_depth() == 0 || root_depth() > 32 || caller != base() + 0x40A1D1 || closing() {
        return;
    }
    with_admission(|admission| {
        admission.cache = cache;
        admission.use_ = use_;
        admission.key = key;
        admission.views = views;
        admission.missed = missed;
        admission.cache_pending = true;
    });
}
/// BeginRoot.
pub fn begin_root(renderer: u64, caller: u64) -> u64 {
    if closing() || caller != base() + 0xB60377 || crate::native_state_exports::CyberpunkVR_PreparationRootCompatible.load(SeqCst) != 1 || !installed(0xC0F98C) || !capture_compatible() {
        return 0;
    }
    let scope = root_depth() + 1;
    ROOT_ADMISSION_DEPTH.with(|depth| depth.set(scope));
    x::CyberpunkVR_PreparationGenerationStats[0].fetch_add(1, SeqCst);
    if scope > 32 {
        let g = lock();
        lose_generation_coverage(g.state(), dg::Failure::Capacity);
        x::CyberpunkVR_PreparationGenerationStats[10].fetch_add(1, SeqCst);
        x::CyberpunkVR_PreparationGenerationStats[15].store(dg::Failure::Capacity as u64, SeqCst);
        return u64::from(scope);
    }
    with_admission(|admission| *admission = Default::default());
    let g = lock();
    let s = g.state();
    s.next_consumer += 1;
    let id = s.next_consumer;
    let registered = s.generation_ledger.register_provisional_consumer(id);
    with_admission(|admission| {
        admission.id = id;
        admission.renderer = renderer;
        admission.registered = registered;
    });
    if registered {
        x::CyberpunkVR_PreparationGenerationStats[1].fetch_add(1, SeqCst);
    } else {
        x::CyberpunkVR_PreparationGenerationStats[10].fetch_add(1, SeqCst);
        x::CyberpunkVR_PreparationGenerationStats[15].store(s.generation_ledger.rejection() as u64, SeqCst);
    }
    publish_generation_stats(s);
    u64::from(scope)
}
/// EndRoot.
pub fn end_root(scope: u64) {
    if scope == 0 {
        return;
    }
    if scope != u64::from(root_depth()) {
        x::CyberpunkVR_PreparationGenerationStats[10].fetch_add(1, SeqCst);
        return;
    }
    if scope <= 32 {
        let admission = with_admission(|admission| *admission).expect("in range");
        {
            let g = lock();
            let s = g.state();
            if admission.prerequisite_group != 0 {
                if let Some(found) = s.consumers.get(&admission.prerequisite_group).cloned() {
                    // SAFETY: under the mutex.
                    let c = unsafe { found.get() };
                    if let Some(declaration) = c.declaration.as_mut() {
                        declaration.mark_original_root_returned();
                        if c.owned_state.is_some() {
                            let _write = TelemetryWrite::new();
                            if !c.compiler.as_mut().is_some_and(|compiler| compiler.mark_original_root_returned()) {
                                owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                            }
                            let state = owned(s, c).expect("admitted");
                            let identity = state.identity;
                            state.record_producer_closed(&identity, scope);
                            c.declaration_context = 0;
                            owned_stats()[18] = 1;
                            publish_compiler_receipt(c);
                        }
                    }
                }
            }
            // The guarded root's only non-constructor return precedes its first cache lookup (zero-sized view). This
            // is not a job completion proof.
            if admission.registered && !admission.constructed && s.generation_ledger.has_consumer(admission.id) && !s.generation_ledger.cancel_unbound_consumer(admission.id, admission.epoch, admission.pair_generation, true) {
                s.generation_ledger.unsupported_consumer(admission.id, admission.epoch, admission.pair_generation, dg::Failure::Unsupported);
                x::CyberpunkVR_PreparationGenerationStats[10].fetch_add(1, SeqCst);
            }
            publish_generation_stats(s);
        }
        with_admission(|admission| *admission = Default::default());
    }
    ROOT_ADMISSION_DEPTH.with(|depth| depth.set(depth.get() - 1));
}
/// BeforeDeclare.
pub fn before_declare(graph: u64, context: u64, job: u64, caller: u64) -> u64 {
    if closing() || root_depth() == 0 || root_depth() > 32 {
        return 0;
    }
    let g = lock();
    let s = g.state();
    let group = with_admission(|admission| admission.prerequisite_group).unwrap_or(0);
    let Some(found) = s.consumers.get(&group).cloned() else { return 0 };
    // SAFETY: under the mutex.
    let c = unsafe { found.get() };
    let Some(declaration) = c.declaration.as_mut() else { return 0 };
    let matched = declaration.match_before_declare(job, graph, caller, rd!());
    if c.owned_state.is_some() {
        let _write = TelemetryWrite::new();
        let identity = owned(s, c).expect("admitted").identity;
        let base = base();
        let mut actual = Snapshot::default();
        let mut topology = actual.capture(graph, &mut reader())
            && actual.edges == c.execution_graph.edges
            && actual.nodes.len() == c.execution_graph.nodes.len()
            && actual.nodes.iter().zip(&c.execution_graph.nodes).all(|(a, b)| same_nodes(a, b));
        let mut source_now = Snapshot::default();
        if topology {
            topology = source_now.capture(c.ticket().source_graph_address(), &mut reader())
                && source_now.edges == c.original_graph.edges
                && source_now.nodes.len() == c.original_graph.nodes.len()
                && (c.has_scene_expansion() || source_now.nodes.len() == actual.nodes.len());
        }
        if topology {
            let expansion = c.has_scene_expansion();
            topology = source_now
                .nodes
                .iter()
                .enumerate()
                .all(|(i, a)| same_nodes(a, &c.original_graph.nodes[i]) && (expansion || a.definition == actual.nodes[i].definition));
        }
        if topology && c.coupled_schedule.is_some() {
            let refreshed = FrontierPlan::build(&source_now, base, pf::DISCARD);
            let cpu = CpuFrontierPlan::build(&refreshed);
            topology = refreshed.candidate()
                && cpu.candidate()
                && cpu.plan.proposed == c.coupled_schedule.as_ref().expect("checked").plan.proposed
                && crate::context_order::analyze(&actual.nodes, &actual.edges, actual.failure, base, rd!()).failure == crate::context_order::Failure::None;
            if topology && c.has_scene_expansion() {
                let mut current_coupled = OwnedGraph::default();
                topology = current_coupled.create(&source_now.nodes, &source_now.edges, source_now.failure, cpu.plan.proposed.clone()) && current_coupled.compile_reference();
                if topology {
                    let coupled = owned_snapshot(&current_coupled);
                    topology = match (&c.scene_both_expansion, &c.scene_expansion) {
                        (Some(plan), _) => plan.matches_declaration(&coupled, &actual, base),
                        (None, Some(plan)) => plan.matches_declaration(&coupled, &actual, base),
                        _ => false,
                    } && validate_scene_views(s, c);
                }
            }
        }
        let declaration_group = c.declaration.as_ref().expect("checked").group();
        let queued = matched && topology && owned(s, c).expect("admitted").record_queued(DECLARATION, &identity, declaration_group, true);
        if !queued {
            owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
            owned_reject(owned_failure::DECLARATION, 0);
            if c.has_scene_expansion() {
                // SAFETY: the export.
                unsafe { x::CyberpunkVR_OwnedSceneSplitStats[22] = 9 };
            }
        } else {
            c.declaration_context = context;
            if c.has_scene_expansion() {
                // SAFETY: the export.
                unsafe { x::CyberpunkVR_OwnedSceneSplitStats[20] = 1 };
            }
            c.execution_graph = actual;
            c.declaration_snapshot_ready = true;
            s.published_schedule_bindings = core::mem::take(&mut source_now.nodes);
            // SAFETY: the exports.
            unsafe {
                x::CyberpunkVR_OwnedExecutionScheduleBindings = data(&s.published_schedule_bindings);
                x::CyberpunkVR_OwnedExecutionSchedule[3] = pf::fingerprint(&c.execution_graph.nodes, &c.execution_graph.edges, c.execution_graph.failure, base);
                x::CyberpunkVR_OwnedExecutionSchedule[15] = 1;
            }
            let wrapper = read_u64(context + 8).filter(|&wrapper| wrapper != 0);
            let pool = wrapper.and_then(read_u64).filter(|&pool| pool != 0);
            let (Some(wrapper), Some(pool)) = (wrapper, pool) else {
                owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                owned_reject(owned_failure::RESOURCE, 0);
                return 0;
            };
            if (c.resource_wrapper != 0 && c.resource_wrapper != wrapper) || (c.resource_pool != 0 && c.resource_pool != pool) {
                owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                owned_reject(owned_failure::RESOURCE, 0);
                return 0;
            }
            c.resource_wrapper = wrapper;
            c.resource_pool = pool;
            let stats = owned_stats();
            stats[16] = declaration_group;
            stats[17] = 1;
            stats[21] = c.extra_readers.len() as u64;
        }
    }
    c.ticket().id()
}
/// CopyOwnedDeclarationSnapshot.
pub fn copy_owned_declaration_snapshot(id: u64) -> Option<Snapshot> {
    if id == 0 {
        return None;
    }
    let g = lock();
    let s = g.state();
    s.consumers.values().find_map(|consumer| {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        (c.ticket().id() == id && c.owned_state.is_some() && c.declaration_snapshot_ready).then(|| c.execution_graph.clone())
    })
}
/// AfterDeclare.
pub fn after_declare(id: u64) {
    if id == 0 {
        return;
    }
    let g = lock();
    let s = g.state();
    let consumers: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for consumer in consumers {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if c.ticket().id() != id || c.declaration.is_none() {
            continue;
        }
        let returned = c.declaration.as_mut().expect("checked").mark_original_declare_returned();
        if c.owned_state.is_some() {
            let _write = TelemetryWrite::new();
            let group = c.declaration.as_ref().expect("checked").group();
            let state = owned(s, c).expect("admitted");
            let identity = state.identity;
            if !returned || !state.record_original_returned(DECLARATION, &identity, group) {
                owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                owned_reject(owned_failure::DECLARATION, 0);
            }
        }
        return;
    }
}
/// Match: the definition capture confirmed against the declared graph.
pub fn match_snapshot(snapshot: &Snapshot, graph: u64, frame_owner: u64, epoch: u64, generation: u64) {
    let g = lock();
    let s = g.state();
    // SAFETY: the export.
    let stats = unsafe { out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionStats)) };
    let source_ready = s.source.ready && s.source.source.is_some();
    // SAFETY: under the mutex.
    let ticket_ok = s.source_ticket.as_ref().is_some_and(|ticket| unsafe { ticket.get() }.construct_verified());
    if x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) != 1
        || stats[0] != 2
        || !ticket_ok
        || !source_ready
        || epoch != u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst))
        || generation != crate::native_state_exports::CyberpunkVR_PreparationPairGeneration.load(SeqCst)
        || epoch != s.capture_epoch
        || generation != s.capture_generation
        || graph != s.source.graph
        || frame_owner != s.capture_owner
        || snapshot.failure != 0
        || s.source.source.as_ref().is_some_and(|source| snapshot.nodes != source.nodes || snapshot.edges != source.edges)
    {
        return;
    }
    for row in &s.owners {
        let found = s.mutations.get(&row.header);
        if row.epoch == 0 {
            if found.is_some() {
                return;
            }
            continue;
        }
        if !found.is_some_and(|f| f.in_flight == 0 && f.epoch == row.epoch && f.kind == row.kind) {
            return;
        }
    }
    let mut details = 0u32;
    if !s.source.matches_before(rd!(), &mut details) {
        return;
    }
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
    stats[14] = 1;
    stats[0] = 3;
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
}
/// QueueIdentity.
pub fn queue_identity(payload: u64, graph: u64, caller: u64) -> Option<(u64, u64)> {
    let g = lock();
    let s = g.state();
    let ticket = s.source_ticket.as_ref()?;
    // SAFETY: under the mutex.
    let t = unsafe { ticket.get() };
    if !s.source.ready || x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) != 1 || t.epoch() != u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)) || !t.match_queued(payload, graph, caller, rd!()) {
        return None;
    }
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
    // SAFETY: the export.
    unsafe { x::CyberpunkVR_PreparationDefinitionTicket[7] = 1 };
    x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
    Some((t.epoch(), t.generation()))
}
/// CloseOwnedExecution.
pub fn close_owned_execution() {
    let g = lock();
    let s = g.state();
    let _write = TelemetryWrite::new();
    if s.allocator_domain.services_active != 0 {
        fail_closed_intent_exception(0, 23);
        return;
    }
    x::CyberpunkVR_OwnedExecutionClosing.store(1, SeqCst);
    x::CyberpunkVR_OwnedExecutionRequest.store(0, SeqCst);
    x::CyberpunkVR_OwnedExecutionMode.store(0, SeqCst);
    x::CyberpunkVR_OwnedSceneSplitEnable.store(0, SeqCst);
    crate::declared_moved_exports::CyberpunkVR_SceneChildEnable.store(0, SeqCst);
    let consumers: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for consumer in consumers {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if let Some(state) = owned(s, c) {
            state.report_failure(MISSING_PROOF);
            owned_stats()[0] = 6;
        }
    }
    s.owned_execution_states.cancel_selection();
    s.retirement_states.shutdown();
    publish_retirement_receipt(s);
    // Registries, graphs and pins intentionally remain alive. Close grants neither producer completion nor
    // native-release/destruction authority.
}
/// Poll.
pub fn poll() {
    if closing() {
        return;
    }
    let mut retired = None;
    let mut completed: Vec<ConsumerRef> = Vec::new();
    {
        let g = lock();
        let s = g.state();
        if closing() {
            return;
        }
        if x::CyberpunkVR_PreparationDefinitionMode.load(SeqCst) == 0 || s.capture_epoch != u64::from(x::CyberpunkVR_PreparationGraphJobsEpoch.load(SeqCst)) {
            retired = s.source_ticket.take();
        }
        let keys: Vec<u64> = s.consumers.keys().copied().collect();
        for key in keys {
            let value = s.consumers.get(&key).cloned().expect("listed");
            // SAFETY: under the mutex.
            let c = unsafe { value.get() };
            if c.owned_state.is_some() {
                let _write = TelemetryWrite::new();
                let quarantined = owned(s, c).expect("admitted").phase == oge::phase::QUARANTINED;
                if c.rejected || c.declaration.as_ref().is_none_or(|d| d.failed()) || c.compiler.as_ref().is_some_and(|compiler| compiler.failed()) || quarantined {
                    owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                    owned_stats()[0] = 6;
                    continue;
                }
                let identity = owned(s, c).expect("admitted").identity;
                if let Some(compiler) = c.compiler.as_ref() {
                    // SAFETY: the compiler's group pointer, read as the .cpp's CompleteGroup did.
                    if compiler.captured() && compiler.original_dispatch_returned() && compiler.original_root_returned() && unsafe { complete_group(compiler.group() as *mut c_void) } && (!compiler.matched() || !compiler.original_compiler_returned()) {
                        owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                        owned_reject(owned_failure::RESOURCE, 0);
                        owned_stats()[0] = 6;
                        publish_compiler_receipt(c);
                        continue;
                    }
                }
                let declaration = c.declaration.as_ref().expect("checked");
                if declaration.complete() {
                    let group = declaration.group();
                    owned(s, c).expect("admitted").observe_completion(DECLARATION, &identity, group, true);
                }
                if let Some(completion) = c.completion.as_ref().filter(|completion| completion.complete()) {
                    let group = completion.group();
                    owned(s, c).expect("admitted").observe_completion(EXECUTION, &identity, group, true);
                }
                publish_compiler_receipt(c);
                if !owned(s, c).expect("admitted").can_release_owned_storage() || !c.resource_compiled || !c.compiler.as_ref().is_some_and(|compiler| compiler.complete()) {
                    continue;
                }
                if c.resource_replay.is_some() {
                    c.resource_replay_terminal_checked = c.resource_replay_reset_sealed && c.resource_replay_reset_returned;
                    for row in &c.resource_replay_rows {
                        c.resource_replay_terminal_checked = c.resource_replay_terminal_checked && row[6] == 1 && row[7] == 1;
                    }
                    if !c.resource_replay_terminal_checked {
                        replay::replay_failed(s, c, 113, 0, 0);
                    } else if !c.resource_replay.as_ref().expect("checked").complete() {
                        replay::replay_failed(s, c, 114, 0, 0);
                    }
                    // SAFETY: the export.
                    unsafe { x::CyberpunkVR_OwnedResourceReplayReset[7] = c.resource_replay_terminal_checked as u64 };
                    replay::publish_replay(s, c);
                }
                if c.scene_child_scopes != 0 {
                    continue;
                }
                if !c.extra_readers_completed {
                    let mut released = true;
                    let state = owned(s, c).expect("admitted");
                    let flags = (
                        state.producer_open,
                        state.queued(DECLARATION),
                        state.original_returned(DECLARATION),
                        state.complete(DECLARATION),
                        state.queued(EXECUTION),
                        state.original_returned(EXECUTION),
                        state.complete(EXECUTION),
                    );
                    for reader in c.extra_readers.clone() {
                        let proof = rd::OwnedGraphCompletionProof {
                            reader: reader.identity,
                            owned_identity_verified: c.ticket().construct_verified() && c.execution_verified,
                            producer_closed: !flags.0,
                            original_producer_returned: true,
                            declaration_queued: flags.1,
                            original_declaration_returned: flags.2,
                            declaration_complete: flags.3,
                            execution_queued: flags.4,
                            original_execution_returned: flags.5,
                            execution_aggregate_complete: flags.6,
                        };
                        let complete = s.rs(reader.owner).is_some_and(|state| state.complete_extra_reader(&proof));
                        released = complete && released;
                        if complete {
                            // SAFETY: a boxed owner under the mutex.
                            let owner = unsafe { &mut *reader.owner };
                            owner.completion_flags = proof.owned_identity_verified as u64
                                | ((proof.producer_closed && proof.original_producer_returned) as u64) << 1
                                | (proof.declaration_queued as u64) << 2
                                | (proof.original_declaration_returned as u64) << 3
                                | (proof.declaration_complete as u64) << 4
                                | (proof.execution_queued as u64) << 5
                                | (proof.original_execution_returned as u64) << 6
                                | (proof.execution_aggregate_complete as u64) << 7
                                | (c.compiler.as_ref().is_some_and(|compiler| compiler.complete()) as u64) << 8;
                            owner.declaration_group = c.declaration.as_ref().map_or(0, |d| d.group());
                            owner.execution_group = c.completion.as_ref().map_or(0, |completion| completion.group());
                            owner.compiler_group = c.compiler.as_ref().map_or(0, |compiler| compiler.group());
                            owner.completed_readers_receipt = s.rs_ref(reader.owner).map_or(0, |state| state.completed_readers());
                            totals()[2] += 1;
                            event(s, RetireEvent::Complete, reader.owner);
                            publish_allocator_receipt(s, Some(&owner.allocator_binding), owner.tree.nodes().len() as u64);
                        }
                    }
                    if !released {
                        owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                        owned_stats()[0] = 6;
                        continue;
                    }
                    c.extra_readers_completed = true;
                }
                // The replaced native consumer read these frozen generations, not a recaptured cache incarnation. Its
                // real owned aggregate closure also closes that earlier source-observation row.
                if s.generation_ledger.has_consumer(identity.consumer_id) {
                    let completion = c.completion.as_ref();
                    let proof = dg::AggregateTerminalProof {
                        consumer_id: identity.consumer_id,
                        request_epoch: identity.epoch,
                        pair_generation: identity.pair_generation,
                        original_returned: completion.is_some_and(|completion| completion.original_returned()),
                        aggregate_complete: completion.is_some_and(|completion| completion.complete()),
                    };
                    if !s.generation_ledger.complete_consumer(&proof) {
                        let reason = s.generation_ledger.rejection();
                        reject_generations(s, c, reason);
                        owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
                        owned_stats()[0] = 6;
                        continue;
                    }
                }
                let stats = owned_stats();
                stats[0] = 4;
                stats[20] = 1;
                s.owned_execution_states.retire(identity.consumer_id);
                c.owned_state = None;
                completed.push(s.consumers.remove(&key).expect("listed"));
                consumers_stat(4).fetch_add(1, SeqCst);
                continue;
            }
            // Observation-only pins can be dropped after a rejected callback returns. No native definitions were
            // adopted by this registry.
            if c.rejected && c.callback_returned {
                completed.push(s.consumers.remove(&key).expect("listed"));
                consumers_stat(8).fetch_add(1, SeqCst);
                continue;
            }
            if !c.completion.as_ref().is_some_and(|completion| completion.complete()) {
                continue;
            }
            if c.declaration.as_ref().is_some_and(|d| !d.failed() && !d.complete()) {
                continue;
            }
            if !c.generation_rejected {
                let t = c.ticket();
                let proof = dg::AggregateTerminalProof {
                    consumer_id: t.id(),
                    request_epoch: t.epoch(),
                    pair_generation: t.generation(),
                    original_returned: c.completion.as_ref().is_some_and(|completion| completion.original_returned()),
                    aggregate_complete: true,
                };
                if !s.generation_ledger.complete_consumer(&proof) {
                    let reason = s.generation_ledger.rejection();
                    reject_generations(s, c, reason);
                }
            }
            let is_source = s.source_ticket.as_ref().zip(c.ticket.as_ref()).is_some_and(|(a, b)| Arc::ptr_eq(a, b));
            if is_source {
                x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
                let t = c.ticket();
                let completion = c.completion.as_ref().expect("complete");
                let row = [t.id(), completion.group(), completion.original_returned() as u64, 1, t.epoch(), t.generation(), t.frame_owner(), t.graph_address()];
                // SAFETY: the exports.
                unsafe { *out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionAggregate)) = row };
                if let Some(d) = c.declaration.as_ref() {
                    let fields = [d.id(), d.group(), d.matched() as u64, d.original_declare_returned() as u64, d.original_root_returned() as u64, d.complete() as u64, d.rejection() as u64, d.frame_owner()];
                    // SAFETY: the export.
                    unsafe { *out(addr_of_mut!(x::CyberpunkVR_PreparationDefinitionDeclaration)) = fields };
                }
                x::CyberpunkVR_PreparationDefinitionSeq.fetch_add(1, SeqCst);
                publish_selected_generations(s, c, true);
            }
            completed.push(s.consumers.remove(&key).expect("listed"));
            consumers_stat(4).fetch_add(1, SeqCst);
        }
        consumers_stat(7).store(s.consumers.len() as u64, SeqCst);
        for owner in s.owner_ptrs() {
            let ready = s.rs_ref(owner).map(|state| state.native_intent_seen() && state.exact_header_detached() && state.pending_readers() == 0 && !state.quarantined() && !state.destroy_attempt_consumed());
            if let Some(ready) = ready {
                // SAFETY: a boxed owner under the mutex.
                unsafe { (*owner).ready = ready };
            }
        }
        recycle_retained_metadata(s);
        publish_retirement_receipt(s);
        let mut closed = core::mem::take(&mut s.closed_generations);
        closed.retain(|&(id, epoch)| !s.generation_ledger.forget_closed_observation(id, epoch));
        s.closed_generations = closed;
        publish_generation_stats(s);
    }
    drop(retired);
    drop(completed);
}

// ---- the owned resource compile
/// BeforeOwnedResourceCompile.
pub fn before_owned_resource_compile(pool: u64, mode: u8, job: u64, caller: u64) -> u64 {
    if closing() || mode != 2 || caller != base() + 0xBB68B5 {
        return 0;
    }
    let g = lock();
    let s = g.state();
    let consumers: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for consumer in consumers {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if c.owned_state.is_none() || c.resource_compiled || !c.compiler.as_ref().is_some_and(|compiler| compiler.matches_compiler_parent(job, rd!())) {
            continue;
        }
        let _write = TelemetryWrite::new();
        // SAFETY: the export.
        let fields = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionResources)) };
        let (id, source_graph) = (c.ticket().id(), c.ticket().source_graph_address());
        let expected = {
            let t = c.ticket();
            CompilerBinding {
                consumer_id: t.id(),
                epoch: t.epoch(),
                generation: t.generation(),
                renderer: c.renderer,
                wrapper: c.resource_wrapper,
                pool: c.resource_pool,
                frame_owner: t.frame_owner(),
                execution_graph: t.graph_address(),
                producer_context: c.compiler_producer_identity,
            }
        };
        let ok = c.compiler.as_ref().expect("checked").matches_binding(&expected)
            && c.declaration_snapshot_ready
            && c.declaration.as_ref().is_some_and(|d| d.matched())
            && c.compiler.as_mut().expect("checked").match_before_compiler(pool, mode, job, caller, rd!());
        if !ok {
            fields[0] = 4;
            fields[11] = u64::from(owned_failure::RESOURCE);
            owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
            publish_compiler_receipt(c);
            return 0;
        }
        fields[0] = 1;
        fields[1] = pool;
        fields[2] = c.resource_wrapper;
        fields[3] = source_graph;
        fields[4] = expected.execution_graph;
        fields[5] = c.original_source.fingerprint;
        fields[6] = pf::fingerprint(&c.execution_graph.nodes, &c.execution_graph.edges, c.execution_graph.failure, base());
        fields[7] = 1;
        fields[8] = 0xBB68B5;
        fields[9] = u64::from(mode);
        let graph = c.execution_graph.clone();
        if !c.resource_uses.capture_pool(pool, &graph, rd!(), 0) {
            fields[0] = 4;
            fields[11] = c.resource_uses.failure as u64;
            owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
            return 0;
        }
        if x::CyberpunkVR_OwnedResourceReplayEnable.load(SeqCst) != 0 {
            replay::capture_replay(s, c);
        }
        publish_compiler_receipt(c);
        return id;
    }
    0
}
/// AfterOwnedResourceCompile.
pub fn after_owned_resource_compile(id: u64, pool: u64) {
    if id == 0 {
        return;
    }
    let g = lock();
    let s = g.state();
    let consumers: Vec<ConsumerRef> = s.consumers.values().cloned().collect();
    for consumer in consumers {
        // SAFETY: under the mutex.
        let c = unsafe { consumer.get() };
        if c.ticket().id() != id || c.owned_state.is_none() {
            continue;
        }
        let _write = TelemetryWrite::new();
        // SAFETY: the export.
        let fields = unsafe { out(addr_of_mut!(x::CyberpunkVR_OwnedExecutionResources)) };
        fields[0] = 2;
        let returned = !closing() && c.compiler.as_mut().is_some_and(|compiler| compiler.mark_original_compiler_returned());
        let state = if returned && c.resource_pool == pool && fields[1] == pool { read_u8(pool + 2) } else { None };
        if state == Some(2) {
            fields[10] = 2;
            fields[0] = 3;
            c.resource_compiled = true;
            if c.resource_replay.is_some() {
                c.resource_replay_compiler_checked = replay::replay_grammar_unchanged(c);
                if !c.resource_replay_compiler_checked {
                    replay::replay_failed(s, c, 112, 0, 0);
                } else {
                    replay::publish_replay(s, c);
                }
            }
        } else {
            fields[0] = 4;
            fields[11] = u64::from(owned_failure::RESOURCE);
            owned(s, c).expect("admitted").report_failure(MISSING_PROOF);
        }
        publish_compiler_receipt(c);
        return;
    }
}
#[allow(dead_code)]
const _: u32 = MAX_TRACKED_OWNERS as u32;
