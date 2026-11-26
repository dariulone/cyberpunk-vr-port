//! The Rust forms of the C++ wrapper classes the definition tracking orchestrator used (their headers stay for the
//! C++ build and the single-pass tests): stage_definition::Snapshot (StageSnapshot), scene_sections::Snapshot
//! (SceneSections), borrowed_scene_sections::Sections (BorrowedSections), scene_expansion::Plan (ScenePlan) and
//! BothEyesPlan, owned_execution's TreeCertified / AllocatorsMatch, and owned_graph_execution::Ledger (OwnedLedger)
//! with its two typed expansion admissions. Each method keeps the C++ rules -- what it resets, which failure it leaves
//! on a refusal -- over the Rust cores the C++ reached through their C ABIs.

use crate::native::definition_tree::{NativeHandle, Snapshot as Tree, NO_INDEX, STRING_ALLOCATOR_RVA, STRING_FREE_ADAPTER_RVA};
use crate::native::graph_snapshot::{field_u32, field_u64, Node, Snapshot};
use crate::native::stage_definition;
use crate::owned_graph_execution::{self as oge, Identity, OwnerGenerationBinding};
use crate::paired_frontier::Edges;
use crate::scene_expansion::{self as expansion, Definition, Frozen, Graph, Mapping};
use crate::scene_sections as sections;
use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock};

/// A read of engine memory (the C++ ReadMemory).
pub type Read<'a> = &'a mut dyn FnMut(u64, &mut [u8]) -> bool;

/// A graph node of zeroes (graph_snapshot::Node{}).
pub const ZERO_NODE: Node = Node { handle: 0, order: 0, bytes: [0; 48], vtable: 0, work: 0, definition: [0; 32] };

/// The tree as the C ABIs lent it: its records when ready, an empty unready tree otherwise.
fn lent(tree: &Tree) -> &Tree {
    static EMPTY: LazyLock<Tree> = LazyLock::new(Tree::default);
    if tree.ready() {
        tree
    } else {
        &EMPTY
    }
}
fn graph(snapshot: &Snapshot) -> Graph<'_> {
    Graph { nodes: &snapshot.nodes, edges: &snapshot.edges, failure: snapshot.failure }
}

// ---- paired_frontier::Plan and paired_cpu_frontier::Plan with their sources (the C++ plans kept the source snapshot;
// the Rust plans leave it to their callers)
/// paired_frontier::Plan.
#[derive(Clone, Default)]
pub struct FrontierPlan {
    pub plan: crate::paired_frontier::Plan,
    pub source: Option<Arc<Snapshot>>,
}
impl FrontierPlan {
    /// Plan::BuildCandidate.
    pub fn build(source: &Snapshot, base: u64, boundary: u32) -> FrontierPlan {
        let plan = crate::paired_frontier::Plan::build_candidate(&source.nodes, &source.edges, source.failure, base, boundary);
        let source = plan.has_source.then(|| Arc::new(source.clone()));
        FrontierPlan { plan, source }
    }
    pub fn candidate(&self) -> bool {
        self.plan.result == crate::paired_frontier::CANDIDATE_NEEDS_CPU_AND_RESOURCE_COMPILE
    }
    /// Plan::Matches.
    pub fn matches(&self, current: &Snapshot, base: u64) -> bool {
        self.source.as_ref().is_some_and(|source| {
            self.candidate()
                && base == self.plan.base
                && current.failure == 0
                && self.plan.fingerprint == crate::paired_frontier::fingerprint(&current.nodes, &current.edges, current.failure, base)
                && source.nodes == current.nodes
                && source.edges == current.edges
        })
    }
}
/// paired_cpu_frontier::Plan.
#[derive(Clone, Default)]
pub struct CpuFrontierPlan {
    pub plan: crate::paired_cpu_frontier::Plan,
    source: Option<Arc<Snapshot>>,
    base: u64,
    fingerprint: u64,
}
impl CpuFrontierPlan {
    /// Plan::BuildCandidate: its source checks, then the Rust rules.
    pub fn build(frontier: &FrontierPlan) -> CpuFrontierPlan {
        use crate::paired_cpu_frontier as pcf;
        let rejected = CpuFrontierPlan { plan: pcf::Plan { failure: pcf::Failure::Source, ..Default::default() }, ..Default::default() };
        let Some(source) = frontier.source.as_ref() else { return rejected };
        if !frontier.candidate() || !frontier.matches(source, frontier.plan.base) {
            return rejected;
        }
        let (main, peer) = (&frontier.plan.roles[0], &frontier.plan.roles[1]);
        let plan = pcf::Plan::build_candidate(&pcf::Source {
            nodes: &source.nodes,
            boundary: frontier.plan.boundary,
            early_tail: frontier.plan.early_tail(),
            original: frontier.plan.original.clone(),
            proposed: frontier.plan.proposed.clone(),
            main_solid_ready: main.solid_ready,
            main_discard: main.discard,
            peer_collector: peer.collector,
            peer_solid_ready: peer.solid_ready,
            peer_discard: peer.discard,
            peer_prefix: &peer.prefix,
        });
        CpuFrontierPlan { plan, source: Some(source.clone()), base: frontier.plan.base, fingerprint: frontier.plan.fingerprint }
    }
    pub fn candidate(&self) -> bool {
        self.plan.result == crate::paired_cpu_frontier::CANDIDATE_NEEDS_OWNERSHIP_AND_RESOURCE_COMPILE
    }
    /// Plan::Matches.
    pub fn matches(&self, current: &Snapshot, base: u64) -> bool {
        self.source.as_ref().is_some_and(|source| {
            base == self.base
                && current.failure == 0
                && self.fingerprint == crate::paired_frontier::fingerprint(&current.nodes, &current.edges, current.failure, base)
                && source.nodes == current.nodes
                && source.edges == current.edges
        })
    }
}

// ---- stage_definition::Snapshot
/// stage_definition::Snapshot.
#[derive(Clone)]
pub struct StageSnapshot {
    ready: bool,
    failure: u32,
    target: u32,
    graph: Node,
    header: stage_definition::Header,
    stages: Vec<stage_definition::Stage>,
    tree: Option<Arc<Tree>>,
}
impl Default for StageSnapshot {
    fn default() -> Self {
        StageSnapshot { ready: false, failure: 0, target: NO_INDEX, graph: ZERO_NODE, header: [0; 64], stages: Vec::new(), tree: None }
    }
}
impl StageSnapshot {
    /// Snapshot::Reachable.
    pub fn reachable(graph: &Node, tree: &Tree, target: u32) -> bool {
        stage_definition::reachable(graph, lent(tree), target)
    }
    fn root_index(graph: &Node, tree: &Tree) -> u32 {
        stage_definition::root_index(graph, lent(tree))
    }
    /// Snapshot::Capture of the graph's root (the overload without a target).
    pub fn capture_root(&mut self, graph: &Node, tree: &Tree, read: Read<'_>) -> bool {
        self.capture(graph, tree, Self::root_index(graph, tree), read)
    }
    /// Snapshot::Capture.
    pub fn capture(&mut self, graph: &Node, tree: &Tree, target: u32, read: Read<'_>) -> bool {
        *self = StageSnapshot::default();
        match stage_definition::capture(graph, lent(tree), target, Some(read)) {
            Err(failure) => {
                self.failure = failure as u32;
                false
            }
            Ok((header, stages)) => {
                self.graph = *graph;
                self.target = target;
                self.header = header;
                self.stages = stages;
                self.tree = Some(Arc::new(tree.clone()));
                self.ready = true;
                self.failure = 0;
                true
            }
        }
    }
    /// Snapshot::MatchesBefore.
    pub fn matches_before(&self, read: Read<'_>) -> bool {
        let (true, Some(tree)) = (self.ready, self.tree.as_ref()) else { return false };
        let mut current = StageSnapshot::default();
        current.capture(&self.graph, tree, self.target, read) && current.header == self.header && current.stages == self.stages
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> u32 {
        self.failure
    }
    pub fn raw_header(&self) -> &stage_definition::Header {
        &self.header
    }
    pub fn stages(&self) -> &[stage_definition::Stage] {
        &self.stages
    }
    pub fn tree_index(&self) -> u32 {
        self.target
    }
    pub fn object(&self) -> u64 {
        match (&self.tree, self.ready) {
            (Some(tree), true) => tree.nodes()[self.target as usize].object,
            _ => 0,
        }
    }
}

// ---- scene_sections::Snapshot
/// scene_sections::Failure (the values the wrapper sets itself).
mod section_failure {
    pub const NONE: u32 = 0;
    pub const STAGES: u32 = 10;
    pub const CHANGED: u32 = 11;
    pub const PATTERN: u32 = 9;
}
#[derive(Clone, Default, PartialEq, Eq)]
struct Observed {
    header: Vec<u8>,
    handles: Vec<NativeHandle>,
    children: Vec<sections::Child>,
}
/// scene_sections::Snapshot.
#[derive(Clone)]
pub struct SceneSections {
    graph: Node,
    observed: Observed,
    render: [StageSnapshot; 2],
    tree: Option<Arc<Tree>>,
    ready: bool,
    failure: u32,
}
impl Default for SceneSections {
    fn default() -> Self {
        SceneSections { graph: ZERO_NODE, observed: Observed::default(), render: Default::default(), tree: None, ready: false, failure: 0 }
    }
}
impl SceneSections {
    /// Snapshot::Capture.
    pub fn capture(&mut self, graph: &Node, tree: &Tree, read: Read<'_>) -> bool {
        *self = SceneSections::default();
        let base = tree.image_base();
        let (root, observation) = match sections::capture(graph, lent(tree), Some(&mut *read)) {
            Err(failure) => {
                self.failure = failure as u32;
                return false;
            }
            Ok(seen) => seen,
        };
        let node = &tree.nodes()[root as usize];
        let mut render: [StageSnapshot; 2] = Default::default();
        for i in 0..2 {
            let slot = if i != 0 { 8 } else { 2 };
            let handle = &tree.handles()[(node.first_handle + slot) as usize];
            self.failure = section_failure::STAGES;
            if !render[i].capture(graph, tree, handle.child, &mut *read)
                || !sections::render_matches(&render[i].header, &render[i].stages, render[i].ready, base, i != 0, &observation.children[slot as usize].raw_prefix)
            {
                return false;
            }
        }
        // cvr_ss_recheck: the tree again, then the same observation.
        self.failure = section_failure::CHANGED;
        let same = lent(tree).matches_before(&mut *read)
            && match sections::read_once(graph, lent(tree), root, &mut *read) {
                Err(reason) => {
                    self.failure = reason as u32;
                    false
                }
                Ok(again) => {
                    self.failure = section_failure::PATTERN;
                    again == observation
                }
            };
        if !same || !render[0].matches_before(&mut *read) || !render[1].matches_before(&mut *read) {
            return false;
        }
        self.graph = *graph;
        self.observed = Observed { header: observation.header.to_vec(), handles: observation.handles.to_vec(), children: observation.children.to_vec() };
        self.render = render;
        self.tree = Some(Arc::new(tree.clone()));
        self.ready = true;
        self.failure = section_failure::NONE;
        true
    }
    /// Snapshot::MatchesBefore.
    pub fn matches_before(&self, read: Read<'_>) -> bool {
        let (true, Some(tree)) = (self.ready, self.tree.as_ref()) else { return false };
        let mut current = SceneSections::default();
        current.capture(&self.graph, tree, read)
            && current.observed == self.observed
            && current.render[0].header == self.render[0].header
            && current.render[0].stages == self.render[0].stages
            && current.render[1].header == self.render[1].header
            && current.render[1].stages == self.render[1].stages
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> u32 {
        self.failure
    }
    pub fn root_identity(&self) -> &Node {
        &self.graph
    }
    pub fn raw_header(&self) -> sections::Header {
        let mut header = [0u8; 56];
        if self.observed.header.len() == 56 {
            header.copy_from_slice(&self.observed.header);
        }
        header
    }
    pub fn handles(&self) -> &[NativeHandle] {
        &self.observed.handles
    }
    pub fn children(&self) -> &[sections::Child] {
        &self.observed.children
    }
    pub fn render_elements(&self) -> &[StageSnapshot; 2] {
        &self.render
    }
    pub fn split_index(&self) -> u32 {
        if self.ready {
            sections::PREFIX_CHILDREN
        } else {
            NO_INDEX
        }
    }
    pub fn prefix_count(&self) -> u32 {
        if self.ready {
            sections::PREFIX_CHILDREN
        } else {
            0
        }
    }
    pub fn tail_count(&self) -> u32 {
        if self.ready {
            sections::TAIL_CHILDREN
        } else {
            0
        }
    }
}

// ---- borrowed_scene_sections::Sections
/// borrowed_scene_sections::CallerLifetime.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallerLifetime {
    pub consumer_id: u64,
    pub epoch: u64,
    pub pair_generation: u64,
    pub original_root: u64,
    pub owner_header: u64,
    pub owner_generation: u64,
    pub owner_born_epoch: u64,
    pub protection_receipt: u64,
}
/// A section header handed to the engine (its address is the view's definition object).
#[repr(C, align(8))]
struct SectionHeader([u8; 56]);
struct Section {
    header: SectionHeader,
    handles: Vec<NativeHandle>,
}
struct BorrowedStorage {
    source: SceneSections,
    sections: [Section; 2],
    definitions: [Definition; 2],
}
mod borrowed_failure {
    pub const NONE: u32 = 0;
    pub const ALREADY_PREPARED: u32 = 1;
    pub const SOURCE: u32 = 2;
    pub const BINDING: u32 = 3;
    pub const SECTIONS: u32 = 4;
    pub const NOT_PREPARED: u32 = 6;
    pub const ALREADY_BOUND: u32 = 7;
    pub const CHANGED: u32 = 8;
}
/// borrowed_scene_sections::Sections: only the headers and arrays are owned; the native handles stay borrowed PODs.
#[derive(Default)]
pub struct BorrowedSections {
    storage: Option<Arc<BorrowedStorage>>,
    binding: CallerLifetime,
    failure: u32,
}
// SAFETY: the storage is immutable once prepared; its pointers name its own buffers.
unsafe impl Send for BorrowedStorage {}
unsafe impl Sync for BorrowedStorage {}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
impl BorrowedSections {
    /// Sections::Prepare.
    pub fn prepare(&mut self, source: &SceneSections) -> bool {
        if self.storage.is_some() {
            self.failure = borrowed_failure::ALREADY_PREPARED;
            return false;
        }
        self.failure = borrowed_failure::SOURCE;
        if !source.ready() {
            return false;
        }
        self.failure = borrowed_failure::SECTIONS;
        if source.handles().len() != sections::CHILD_COUNT
            || source.children().len() != sections::CHILD_COUNT
            || source.split_index() != sections::PREFIX_CHILDREN
            || source.prefix_count() != sections::PREFIX_CHILDREN
            || source.tail_count() != sections::TAIL_CHILDREN
        {
            return false;
        }
        let begins = [0, source.split_index() as usize];
        let counts = [source.prefix_count(), source.tail_count()];
        let raw = source.raw_header();
        let make = |i: usize| Section { header: SectionHeader(raw), handles: source.handles()[begins[i]..begins[i] + counts[i] as usize].to_vec() };
        let mut next = Arc::new(BorrowedStorage { source: source.clone(), sections: [make(0), make(1)], definitions: [Definition::default(); 2] });
        {
            let storage = Arc::get_mut(&mut next).expect("fresh storage");
            for i in 0..2 {
                let section = &mut storage.sections[i];
                let handles = section.handles.as_ptr() as u64;
                put64(&mut section.header.0, 40, handles);
                put32(&mut section.header.0, 48, counts[i]);
                put32(&mut section.header.0, 52, counts[i]);
                let definition = &mut storage.definitions[i];
                definition.object = section.header.0.as_ptr() as u64;
                definition.prefix.copy_from_slice(&section.header.0[..32]);
            }
        }
        self.binding = CallerLifetime::default();
        self.storage = Some(next);
        self.failure = borrowed_failure::NONE;
        true
    }
    /// Sections::Bind.
    pub fn bind(&mut self, source: &SceneSections, binding: &CallerLifetime) -> bool {
        if self.storage.is_none() {
            self.failure = borrowed_failure::NOT_PREPARED;
            return false;
        }
        if self.bound() {
            self.failure = borrowed_failure::ALREADY_BOUND;
            return false;
        }
        self.failure = borrowed_failure::BINDING;
        if !source.ready() || !Self::binding_matches(source, binding) {
            return false;
        }
        self.failure = borrowed_failure::CHANGED;
        if !self.source_matches(source) {
            return false;
        }
        self.binding = *binding;
        self.failure = borrowed_failure::NONE;
        true
    }
    /// Sections::Matches.
    pub fn matches(&self, source: &SceneSections, binding: &CallerLifetime) -> bool {
        self.bound() && self.binding == *binding && Self::binding_matches(source, binding) && self.source_matches(source)
    }
    pub fn bound(&self) -> bool {
        self.storage.is_some() && self.binding.protection_receipt != 0
    }
    pub fn definitions(&self) -> [Definition; 2] {
        self.storage.as_ref().map_or([Definition::default(); 2], |storage| storage.definitions)
    }
    fn source_matches(&self, source: &SceneSections) -> bool {
        let Some(storage) = self.storage.as_ref() else { return false };
        if !source.ready() {
            return false;
        }
        let saved = &storage.source;
        if saved.root_identity() != source.root_identity()
            || saved.raw_header() != source.raw_header()
            || saved.handles() != source.handles()
            || saved.children() != source.children()
            || saved.split_index() != source.split_index()
            || saved.prefix_count() != source.prefix_count()
            || saved.tail_count() != source.tail_count()
        {
            return false;
        }
        for i in 0..2 {
            let (a, b) = (&saved.render_elements()[i], &source.render_elements()[i]);
            if a.object() != b.object() || a.tree_index() != b.tree_index() || a.raw_header() != b.raw_header() || a.stages() != b.stages() {
                return false;
            }
            let section = &storage.sections[i];
            let begin = if i != 0 { source.split_index() } else { 0 } as usize;
            let count = if i != 0 { source.tail_count() } else { source.prefix_count() };
            let header = &section.header.0;
            if section.handles.len() != count as usize
                || section.handles[..] != source.handles()[begin..begin + count as usize]
                || header[..40] != source.raw_header()[..40]
                || field_u64(header, 40) != section.handles.as_ptr() as u64
                || field_u32(header, 48) != count
                || field_u32(header, 52) != count
                || storage.definitions[i].object != header.as_ptr() as u64
                || storage.definitions[i].prefix[..] != header[..32]
            {
                return false;
            }
        }
        true
    }
    fn binding_matches(source: &SceneSections, binding: &CallerLifetime) -> bool {
        binding.consumer_id != 0
            && binding.epoch != 0
            && binding.pair_generation != 0
            && binding.owner_generation != 0
            && binding.owner_born_epoch != 0
            && binding.protection_receipt != 0
            && binding.owner_header != 0
            && binding.owner_header & 7 == 0
            && binding.owner_header <= u64::MAX - 16
            && binding.original_root != 0
            && binding.original_root & 7 == 0
            && binding.original_root <= u64::MAX - 56
            && binding.original_root == field_u64(&source.root_identity().bytes, 16)
    }
}

// ---- scene_expansion::Plan
/// scene_expansion::Failure::Allocation and the rest by value.
#[derive(Clone, Default)]
pub struct ScenePlan {
    ready: bool,
    failure: u32,
    base: u64,
    selected: u32,
    tail: u32,
    source: Snapshot,
    expanded: Snapshot,
    definitions: [Definition; 2],
    edges: [Edges; 2],
    mappings: Vec<Mapping>,
}
impl ScenePlan {
    /// Plan::Build.
    pub fn build(&mut self, source: &Snapshot, base: u64, selected: u32, definitions: &[Definition; 2]) -> bool {
        *self = ScenePlan::default();
        match expansion::build(graph(source), base, selected, definitions) {
            Err(failure) => {
                self.failure = failure as u32;
                false
            }
            Ok(built) => {
                self.source = source.clone();
                self.expanded = Snapshot { nodes: built.nodes, edges: built.edges, failure: built.failure };
                self.mappings = built.mappings;
                self.edges = built.expanded_edges;
                self.definitions = *definitions;
                self.selected = selected;
                self.tail = built.tail;
                self.base = base;
                self.ready = true;
                self.failure = 0;
                true
            }
        }
    }
    pub fn matches_source(&self, source: &Snapshot, base: u64) -> bool {
        self.ready && base == self.base && source.failure == self.source.failure && source.nodes == self.source.nodes && source.edges == self.source.edges
    }
    pub fn matches_compiled(&self, actual: &Snapshot) -> bool {
        self.ready && actual.failure == 0 && actual.nodes == self.expanded.nodes && actual.edges == self.expanded.edges
    }
    fn frozen(&self) -> Frozen<'_> {
        Frozen { source: graph(&self.source), expanded: graph(&self.expanded), mappings: &self.mappings, definitions: &self.definitions, selected: self.selected, base: self.base }
    }
    pub fn matches_declaration(&self, source: &Snapshot, expanded: &Snapshot, base: u64) -> bool {
        self.ready && expansion::matches_declaration(&self.frozen(), graph(source), graph(expanded), base)
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> u32 {
        self.failure
    }
    pub fn source(&self) -> &Snapshot {
        &self.source
    }
    pub fn definitions(&self) -> &[Definition; 2] {
        &self.definitions
    }
    pub fn expanded(&self) -> &Snapshot {
        &self.expanded
    }
    pub fn edges(&self) -> &[Edges; 2] {
        &self.edges
    }
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }
    pub fn prefix_handle(&self) -> u32 {
        self.selected
    }
    pub fn tail_handle(&self) -> u32 {
        self.tail
    }
}

// ---- scene_expansion::BothEyesPlan
#[derive(Clone)]
pub struct BothEyesPlan {
    ready: bool,
    failure: u32,
    base: u64,
    peer: ScenePlan,
    main: ScenePlan,
    selected: [Node; 2],
    mappings: Vec<Mapping>,
}
impl Default for BothEyesPlan {
    fn default() -> Self {
        BothEyesPlan { ready: false, failure: 0, base: 0, peer: ScenePlan::default(), main: ScenePlan::default(), selected: [ZERO_NODE; 2], mappings: Vec::new() }
    }
}
impl BothEyesPlan {
    /// BothEyesPlan::Build.
    pub fn build(&mut self, source: &Snapshot, base: u64, definitions: &[[Definition; 2]; 2]) -> bool {
        *self = BothEyesPlan::default();
        match expansion::both_select(graph(source), base, definitions) {
            Err(failure) => {
                self.failure = failure as u32;
                return false;
            }
            Ok(selected) => self.selected = selected,
        }
        let mut peer = ScenePlan::default();
        if !peer.build(source, base, self.selected[1].handle, &definitions[1]) {
            self.failure = peer.failure;
            self.peer = peer;
            return false;
        }
        let mut main = ScenePlan::default();
        if !main.build(&peer.expanded, base, self.selected[0].handle, &definitions[0]) {
            self.failure = main.failure;
            self.peer = peer;
            self.main = main;
            return false;
        }
        let flat = |edges: &Edges| edges.iter().map(|&(a, b)| [a, b]).collect::<Vec<_>>();
        let (cpu, order) = (flat(&main.edges[0]), flat(&main.edges[1]));
        let mut composed = Vec::new();
        let failure = expansion::both_compose(graph(source), &self.selected, &peer.frozen(), peer.tail, &main.frozen(), main.tail, [cpu.as_slice(), order.as_slice()], &mut composed);
        self.failure = failure as u32;
        self.mappings = composed;
        self.peer = peer;
        self.main = main;
        if self.failure != 0 {
            return false;
        }
        self.base = base;
        self.ready = true;
        true
    }
    pub fn matches_source(&self, source: &Snapshot, base: u64) -> bool {
        self.ready && self.peer.matches_source(source, base)
    }
    pub fn matches_compiled(&self, actual: &Snapshot) -> bool {
        self.ready && self.main.matches_compiled(actual)
    }
    pub fn matches_declaration(&self, source: &Snapshot, expanded: &Snapshot, base: u64) -> bool {
        self.ready
            && self.peer.ready
            && self.main.ready
            && expansion::both_matches_declaration(&self.peer.frozen(), &self.main.frozen(), self.base, graph(source), graph(expanded), base)
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn rejection(&self) -> u32 {
        self.failure
    }
    pub fn source(&self) -> &Snapshot {
        self.peer.source()
    }
    pub fn expanded(&self) -> &Snapshot {
        self.main.expanded()
    }
    pub fn edges(&self) -> &[Edges; 2] {
        self.main.edges()
    }
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }
    pub fn selected_source(&self, view: u32) -> &Node {
        &self.selected[view as usize]
    }
    fn view(&self, view: u32) -> &ScenePlan {
        if view != 0 {
            &self.peer
        } else {
            &self.main
        }
    }
    pub fn definitions(&self, view: u32) -> &[Definition; 2] {
        self.view(view).definitions()
    }
    pub fn prefix_handle(&self, view: u32) -> u32 {
        self.view(view).prefix_handle()
    }
    pub fn tail_handle(&self, view: u32) -> u32 {
        self.view(view).tail_handle()
    }
}

// ---- owned_execution's certificate
/// owned_execution::CertificateNodes: (vtable, work, destructor) RVAs of the certified definition node kinds.
const CERTIFICATE_NODES: [(u64, u64, u64); 133] = [
    (0x30946f8, 0xcb1e88, 0x8eab98), (0x31da860, 0xddb6b8, 0x8ea620), (0x31da890, 0xdac960, 0x8ea620),
    (0x36d1ac8, 0x7ed908, 0x8e7e38), (0x36d19a8, 0x3401fc, 0x8e7e38), (0x3094728, 0x776b04, 0x8e7e38),
    (0x3094758, 0xbf436c, 0x8e7e38), (0x3094788, 0xb2a87c, 0x8e7e38), (0x30947b8, 0x33dda4, 0x8ea4e0),
    (0x3094820, 0xb7cf7c, 0x8e7e38), (0x3094850, 0x332d5c, 0x8e913c), (0x36ced58, 0x655fac, 0x8ea51c),
    (0x31c8140, 0x777514, 0x190f138), (0x3219980, 0x77e4c8, 0x19159e0), (0x32328e8, 0xd6c0c0, 0x8e7e38),
    (0x3241050, 0x7f6b98, 0x8e7e38), (0x33db940, 0x7f1ffc, 0x19159e0), (0x36cea88, 0x55831c, 0x8e7e38),
    (0x36ced28, 0x41fe28, 0x8ea620), (0x36d1978, 0x778c04, 0x8ea620), (0x36d0df0, 0x411b94, 0x8ea620),
    (0x36d1c18, 0xceea0c, 0x8ea620), (0x36d1bb8, 0x22a6820, 0x2276dc0), (0x36d0e50, 0xed769c, 0x8ea620),
    (0x36d0e80, 0x7f12a0, 0x8ea620), (0x36d0e20, 0xb0d388, 0x8ea620), (0x36d0c10, 0x77a100, 0x8ea620),
    (0x36d0a60, 0xe9ca98, 0x8ea620), (0x36d0ac0, 0xe9bbc4, 0x8ea620), (0x36d0be0, 0xdd3d24, 0x8ea620),
    (0x36d0a30, 0xe9ce6c, 0x8ea620), (0x36d1948, 0xe9a9b0, 0x8ea620), (0x36d1be8, 0xd632dc, 0x8ea620),
    (0x36d19d8, 0xe9b338, 0x8ea620), (0x36d1a08, 0xe3f9e0, 0x8ea620), (0x36d0d60, 0x77e104, 0x8ea620),
    (0x36d0d90, 0xd6bc78, 0x8ea620), (0x36d0d30, 0x557204, 0x8ea620), (0x36d0250, 0xc7a944, 0x8ea620),
    (0x36d0280, 0x4262cc, 0x8ea620), (0x36d0dc0, 0x559144, 0x8e7e38), (0x36d0220, 0xc92e90, 0x8ea620),
    (0x36d1a38, 0x7f3c20, 0x8ea620), (0x36d0f10, 0x77d450, 0x8ea620), (0x36d0eb0, 0xcc3d44, 0x8ea620),
    (0x36d0ee0, 0xdcc578, 0x8ea620), (0x36d0cd0, 0x41ce60, 0x8ea620), (0x36d0d00, 0x22873b0, 0x2276dc0),
    (0x36d0ca0, 0x7f23b4, 0x8ea620), (0x36d0310, 0x3fda34, 0x8ea620), (0x36d02b0, 0xdd38e8, 0x8ea620),
    (0x36d0370, 0x5b09b0, 0x8ea620), (0x36d11b0, 0x7f86cc, 0x8ea620), (0x36d11e0, 0x7f4130, 0x8ea620),
    (0x36d0fd0, 0xf60474, 0x8ea620), (0x36d1000, 0x33e388, 0x8ea620), (0x36d0fa0, 0x33d0ac, 0x8ea620),
    (0x36d10f0, 0xbfdd24, 0x8ea620), (0x36d1120, 0x5d798c, 0x8ea620), (0x36d0640, 0xc46574, 0x8ea620),
    (0x36d1090, 0x8eb158, 0x8ea620), (0x36d10c0, 0xbf60f0, 0x8ea620), (0x36d1030, 0x4f3164, 0x8ea620),
    (0x36d05b0, 0xc28a18, 0x8ea620), (0x36d05e0, 0x1ff2b0, 0x8ea620), (0x36d0580, 0x4f4708, 0x8ea620),
    (0x36d0970, 0xc7a5c8, 0x8ea620), (0x36d0940, 0x5aeec8, 0x8ea620), (0x36d0730, 0xe42360, 0x8ea620),
    (0x36d0760, 0x557c28, 0x8ea620), (0x36d06d0, 0x5aa54c, 0x8ea620), (0x36d0700, 0x423a1c, 0x8ea620),
    (0x36d04f0, 0x5a764c, 0x8ea620), (0x36d0520, 0x410c64, 0x8ea620), (0x36d0490, 0xd60084, 0x8ea620),
    (0x36d08b0, 0x3fe078, 0x8ea620), (0x36d0850, 0x7f1db0, 0x8ea620), (0x36d0880, 0x7f1d50, 0x8ea620),
    (0x36d0c70, 0x22a3f80, 0x2276dc0), (0x36d09a0, 0x7f36d8, 0x8ea620), (0x36d0c40, 0x5a93e8, 0x8ea620),
    (0x36d1b58, 0x777d0c, 0x8ea620), (0x36d0a00, 0x41f188, 0x8ea620), (0x36d1a68, 0x40fc6c, 0x8ea620),
    (0x36d09d0, 0x410f1c, 0x8ea620), (0x36d1af8, 0x55a040, 0x8ea620), (0x36d07f0, 0x7f0c98, 0x8ea620),
    (0x36d0790, 0x425aa0, 0x8ea620), (0x36d07c0, 0x426778, 0x8ea620), (0x36d0820, 0x777d04, 0x8ea620),
    (0x36d0b20, 0xb66728, 0x8ea620), (0x36d13d8, 0x416600, 0x8ea620), (0x36d14c8, 0xdd3fdc, 0x8ea620),
    (0x36d1408, 0x4172c8, 0x8e7e38), (0x36d1498, 0x414dfc, 0x8e7e38), (0x36d14f8, 0x7f1078, 0x8ea620),
    (0x36d1468, 0x7ef4b0, 0x8e7e38), (0x36d1258, 0xb3c07c, 0x8e7e38), (0x36d1288, 0x3e1b20, 0x8e7e38),
    (0x36d1708, 0x7f6a60, 0x8ea620), (0x36d1798, 0x7740d8, 0x8ea620), (0x36d1678, 0xb7a5cc, 0x8ea620),
    (0x36d16d8, 0x7f2bf0, 0x8ea620), (0x36d16a8, 0xb79f3c, 0x8ea620), (0x36d12b8, 0x414140, 0x8ea620),
    (0x36d1318, 0x413ae8, 0x8ea620), (0x36d12e8, 0xdd4220, 0x8ea620), (0x36d1738, 0xd5bac4, 0x8e7e38),
    (0x36d17c8, 0x8fa3d0, 0x8ea620), (0x36d1768, 0x557b54, 0x8ea620), (0x36d13a8, 0xc0f204, 0x8e7e38),
    (0x36d17f8, 0x4edfe0, 0x8e7e38), (0x36d1858, 0xa00830, 0x8e7e38), (0x36d1918, 0x41aa5c, 0x8e7e38),
    (0x36d1828, 0x41b94c, 0x8e7e38), (0x36d1888, 0x41ade8, 0x8e7e38), (0x36d15b8, 0xd2a678, 0x8e7e38),
    (0x36d15e8, 0x7f12f8, 0x8e7e38), (0x36d18b8, 0x41c4e4, 0x8e7e38), (0x36d18e8, 0x7f1454, 0x8e7e38),
    (0x33fc0f0, 0x5aa944, 0x8e7e38), (0x36d1588, 0x41af74, 0x8ea620), (0x36d1648, 0xca9e60, 0x8e7e38),
    (0x36d1558, 0x41eb2c, 0x8ea620), (0x36d1b28, 0x22707b0, 0x2276df0), (0x36d1b88, 0x77f47c, 0x8e7e38),
    (0x36d1d38, 0xc48278, 0x8e7e38), (0x36d1d68, 0x7f476c, 0x8e7e38), (0x36d1a98, 0x7ee0bc, 0x8e7e38),
    (0x36d1d98, 0x77596c, 0x8e7e38), (0x36d1cd8, 0x33bd84, 0x8e7e38), (0x36d1d08, 0xc45c70, 0x8e7e38),
    (0x36d1c78, 0x33d510, 0x8e7e38),
];
/// owned_execution::TreeCertified.
pub fn tree_certified(tree: &Tree) -> bool {
    if !tree.ready() || tree.image_base() == 0 || tree.nodes().is_empty() {
        return false;
    }
    let base = tree.image_base();
    let common = base + 0x3BCDD80;
    if tree.allocators().iter().any(|a| a.allocator != common || a.raw & !7 != common) {
        return false;
    }
    for node in tree.nodes() {
        if node.vtable < base || node.work < base || node.destructor < base {
            return false;
        }
        if !CERTIFICATE_NODES.iter().any(|&(vtable, work, destructor)| node.vtable == base + vtable && node.work == base + work && node.destructor == base + destructor) {
            return false;
        }
    }
    if tree.arrays().iter().any(|a| a.allocator != base + 0x30947E8 || a.free_callback != base + 0x120F6F0) {
        return false;
    }
    !tree.strings().iter().any(|s| {
        s.mode != 1
            || s.capacity == 0
            || s.allocator_value != 0
            || s.fallback_global_address != base + STRING_ALLOCATOR_RVA
            || s.fallback_allocator != common
            || s.fallback_raw & !7 != common
            || s.fallback_free_adapter != base + STRING_FREE_ADAPTER_RVA
    })
}
/// owned_execution::AllocatorsMatch.
pub fn allocators_match(tree: &Tree, read: Read<'_>) -> bool {
    if !tree_certified(tree) {
        return false;
    }
    let mut twice = |address: u64, expected: u64| {
        let (mut a, mut b) = ([0u8; 8], [0u8; 8]);
        read(address, &mut a) && read(address, &mut b) && u64::from_le_bytes(a) == expected && b == a
    };
    tree.allocators().iter().all(|a| twice(a.address, a.raw))
        && tree.strings().iter().all(|s| twice(s.fallback_global_address, s.fallback_raw))
        && tree.arrays().iter().all(|a| twice(a.allocator + 0x20, a.free_callback))
}

// ---- owned_graph_execution::Ledger
/// owned_graph_execution::AdmissionFacts.
#[derive(Clone, Copy, Default)]
pub struct AdmissionFacts {
    pub exact_reference_compiled: bool,
    pub source_mapping_audited: bool,
    pub definition_protection_audited: bool,
}
/// owned_graph_execution::ExpansionAdmissionFacts.
#[derive(Clone, Copy, Default)]
pub struct ExpansionAdmissionFacts {
    pub source_mapping_audited: bool,
    pub definition_protection_audited: bool,
    pub native_code_guarded: bool,
    pub native_context_order_compiled: bool,
}
const MAX_CONSUMERS: usize = 256;
mod ledger_failure {
    pub const NONE: u32 = 0;
    pub const CAPACITY: u32 = 3;
    pub const ADMISSION: u32 = 5;
    pub const BINDINGS: u32 = 6;
    pub const IDENTITY: u32 = 7;
    pub const INCOMPLETE: u32 = 15;
}
/// owned_graph_execution::Ledger: the bounded registry of state records (it owns no native lease).
#[derive(Default)]
pub struct OwnedLedger {
    states: BTreeMap<u64, Box<oge::State>>,
    last_consumer: u64,
    selected: u64,
    failure: u32,
}
fn pointer(address: u64, bytes: u64) -> bool {
    address != 0 && address & 7 == 0 && address <= u64::MAX - bytes
}
impl OwnedLedger {
    fn begin_admission(&mut self, identity: &Identity, producer_scope: u64, graph_slot: u64) -> bool {
        let (failure, last, admitted) = oge::begin_admission(identity, self.last_consumer, producer_scope, graph_slot);
        self.failure = failure;
        self.last_consumer = last;
        admitted
    }
    fn admit_verified(&mut self, identity: &Identity, bindings: &[OwnerGenerationBinding], producer_scope: u64, graph_slot: u64) -> Option<u64> {
        self.failure = ledger_failure::BINDINGS;
        if !oge::verified_bindings(bindings) {
            return None;
        }
        self.failure = ledger_failure::CAPACITY;
        if self.states.len() >= MAX_CONSUMERS {
            return None;
        }
        let mut stored = [OwnerGenerationBinding::default(); oge::MAX_OWNER_BINDINGS];
        stored[..bindings.len()].copy_from_slice(bindings);
        let state = Box::new(oge::State {
            identity: *identity,
            bindings: stored,
            binding_count: bindings.len(),
            producer_scope,
            graph_slot,
            paths: Default::default(),
            phase: oge::phase::PREPARED,
            failure: 0,
            selected: true,
            escaped: false,
            producer_open: true,
            construct_verified: false,
        });
        self.states.insert(identity.consumer_id, state);
        if let Some(previous) = self.states.get_mut(&self.selected) {
            previous.cancel_selection();
        }
        self.selected = identity.consumer_id;
        self.failure = ledger_failure::NONE;
        Some(identity.consumer_id)
    }
    /// Ledger::Admit: the state's consumer id (find it with `find`).
    pub fn admit(&mut self, identity: &Identity, bindings: &[OwnerGenerationBinding], facts: AdmissionFacts, producer_scope: u64, graph_slot: u64) -> Option<u64> {
        if !self.begin_admission(identity, producer_scope, graph_slot) {
            return None;
        }
        self.failure = ledger_failure::ADMISSION;
        if !facts.exact_reference_compiled || !facts.source_mapping_audited || !facts.definition_protection_audited {
            return None;
        }
        self.admit_verified(identity, bindings, producer_scope, graph_slot)
    }
    /// Ledger::AdmitExpanded (one eye).
    #[allow(clippy::too_many_arguments)]
    pub fn admit_expanded(
        &mut self,
        identity: &Identity,
        bindings: &[OwnerGenerationBinding],
        plan: &ScenePlan,
        views: &BorrowedSections,
        sections: &SceneSections,
        lifetime: &CallerLifetime,
        actual_compiled: &Snapshot,
        base: u64,
        facts: ExpansionAdmissionFacts,
        producer_scope: u64,
        graph_slot: u64,
    ) -> Option<u64> {
        if !self.begin_admission(identity, producer_scope, graph_slot) {
            return None;
        }
        self.failure = ledger_failure::ADMISSION;
        if !facts.source_mapping_audited
            || !facts.definition_protection_audited
            || !facts.native_code_guarded
            || !facts.native_context_order_compiled
            || !plan.ready()
            || !views.bound()
            || !views.matches(sections, lifetime)
            || !plan.matches_source(plan.source(), base)
            || !plan.matches_compiled(actual_compiled)
            || *plan.definitions() != views.definitions()
        {
            return None;
        }
        let source = plan.source();
        if source.nodes.is_empty() || source.nodes.len() >= 220 || actual_compiled.nodes.len() != source.nodes.len() + 1 {
            return None;
        }
        let selected = source.nodes.iter().find(|node| node.handle == plan.prefix_handle())?;
        if selected != sections.root_identity() || field_u64(&selected.bytes, 16) != lifetime.original_root {
            return None;
        }
        self.failure = ledger_failure::IDENTITY;
        if identity.consumer_id != lifetime.consumer_id || identity.epoch != lifetime.epoch || identity.pair_generation != lifetime.pair_generation {
            return None;
        }
        self.failure = ledger_failure::BINDINGS;
        if bindings.is_empty() || bindings.len() > oge::MAX_OWNER_BINDINGS {
            return None;
        }
        let required = OwnerGenerationBinding {
            owner_header: lifetime.owner_header,
            generation_id: lifetime.owner_generation,
            born_mutation_epoch: lifetime.owner_born_epoch,
            protection_receipt: lifetime.protection_receipt,
        };
        let mut matches = 0;
        for i in 0..bindings.len() {
            matches += (bindings[i] == required) as usize;
            if (0..i).any(|j| bindings[j].protection_receipt == bindings[i].protection_receipt) {
                return None;
            }
        }
        if matches != 1 {
            return None;
        }
        self.admit_verified(identity, bindings, producer_scope, graph_slot)
    }
    /// Ledger::AdmitExpanded (both eyes).
    #[allow(clippy::too_many_arguments)]
    pub fn admit_expanded_both(
        &mut self,
        identity: &Identity,
        bindings: &[OwnerGenerationBinding],
        plan: &BothEyesPlan,
        views: &[BorrowedSections; 2],
        sections: &[SceneSections; 2],
        lifetimes: &[CallerLifetime; 2],
        actual_compiled: &Snapshot,
        base: u64,
        facts: ExpansionAdmissionFacts,
        producer_scope: u64,
        graph_slot: u64,
    ) -> Option<u64> {
        if !self.begin_admission(identity, producer_scope, graph_slot) {
            return None;
        }
        self.failure = ledger_failure::ADMISSION;
        if !facts.source_mapping_audited
            || !facts.definition_protection_audited
            || !facts.native_code_guarded
            || !facts.native_context_order_compiled
            || !plan.ready()
            || !plan.matches_source(plan.source(), base)
            || !plan.matches_compiled(actual_compiled)
        {
            return None;
        }
        let source = plan.source();
        if source.nodes.len() < 2 || source.nodes.len() > 218 || actual_compiled.nodes.len() != source.nodes.len() + 2 {
            return None;
        }
        let mut clones = [0u64; 4];
        for view in 0..2usize {
            if !views[view].bound() || !views[view].matches(&sections[view], &lifetimes[view]) || *plan.definitions(view as u32) != views[view].definitions() {
                return None;
            }
            // The second expansion changes adjacency in its intermediate source. Join both captured roots to S0,
            // never to that intermediate graph.
            let selected = plan.selected_source(view as u32);
            if selected != sections[view].root_identity()
                || field_u32(&selected.bytes, 24) != view as u32
                || field_u64(&selected.bytes, 16) != lifetimes[view].original_root
            {
                return None;
            }
            for part in 0..2 {
                let object = plan.definitions(view as u32)[part].object;
                if !pointer(object, 56) {
                    return None;
                }
                let index = 2 * view + part;
                if (0..index).any(|previous| object < clones[previous] + 56 && clones[previous] < object + 56) {
                    return None;
                }
                clones[index] = object;
            }
        }
        self.failure = ledger_failure::IDENTITY;
        if lifetimes.iter().any(|l| identity.consumer_id != l.consumer_id || identity.epoch != l.epoch || identity.pair_generation != l.pair_generation) {
            return None;
        }
        self.failure = ledger_failure::BINDINGS;
        if bindings.is_empty() || bindings.len() > oge::MAX_OWNER_BINDINGS {
            return None;
        }
        for i in 0..bindings.len() {
            if (0..i).any(|j| bindings[j].protection_receipt == bindings[i].protection_receipt) {
                return None;
            }
        }
        for lifetime in lifetimes {
            let required = OwnerGenerationBinding {
                owner_header: lifetime.owner_header,
                generation_id: lifetime.owner_generation,
                born_mutation_epoch: lifetime.owner_born_epoch,
                protection_receipt: lifetime.protection_receipt,
            };
            if bindings.iter().filter(|&&b| b == required).count() != 1 {
                return None;
            }
        }
        self.admit_verified(identity, bindings, producer_scope, graph_slot)
    }
    pub fn find(&mut self, consumer_id: u64) -> Option<&mut oge::State> {
        self.states.get_mut(&consumer_id).map(|state| &mut **state)
    }
    pub fn cancel_selection(&mut self) {
        let selected = self.selected;
        if let Some(state) = self.find(selected) {
            state.cancel_selection();
        }
        self.selected = 0;
    }
    pub fn retire(&mut self, consumer_id: u64) -> bool {
        match self.states.get(&consumer_id) {
            Some(state) if state.can_release_owned_storage() => {}
            _ => {
                self.failure = ledger_failure::INCOMPLETE;
                return false;
            }
        }
        self.states.remove(&consumer_id);
        if self.selected == consumer_id {
            self.selected = 0;
        }
        self.failure = ledger_failure::NONE;
        true
    }
    pub fn size(&self) -> usize {
        self.states.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(consumer_id: u64) -> Identity {
        Identity { consumer_id, epoch: 1, pair_generation: 1, source_graph: 0x1000, execution_graph: 0x2000, frame_owner: 0x3000, prerequisite_group: 0x4000 }
    }
    const FACTS: AdmissionFacts = AdmissionFacts { exact_reference_compiled: true, source_mapping_audited: true, definition_protection_audited: true };
    const BINDING: [OwnerGenerationBinding; 1] = [OwnerGenerationBinding { owner_header: 0x5000, generation_id: 1, born_mutation_epoch: 1, protection_receipt: 1 }];

    // Ledger::Admit: ids only increase, every audited fact is required, a new admission takes the selection, and an
    // unreleased state stays.
    #[test]
    fn ledger_admits_in_sequence() {
        let mut ledger = OwnerLedgerProbe::default();
        assert_eq!(ledger.0.admit(&identity(1), &BINDING, FACTS, 1, 0x6000), Some(1));
        assert_eq!(ledger.0.admit(&identity(1), &BINDING, FACTS, 1, 0x6000), None, "a repeated consumer id is out of sequence");
        let unaudited = AdmissionFacts { definition_protection_audited: false, ..FACTS };
        assert_eq!(ledger.0.admit(&identity(2), &BINDING, unaudited, 1, 0x6000), None);
        assert_eq!(ledger.0.failure, ledger_failure::ADMISSION);
        assert_eq!(ledger.0.admit(&identity(3), &BINDING, FACTS, 1, 0x6000), Some(3));
        assert!(!ledger.0.find(1).expect("admitted").selected, "the newer admission cancels the older selection");
        assert!(!ledger.0.retire(3), "an unreleased state is kept");
        assert_eq!(ledger.0.failure, ledger_failure::INCOMPLETE);
        assert_eq!(ledger.0.size(), 2);
    }
    #[derive(Default)]
    struct OwnerLedgerProbe(OwnedLedger);

    #[test]
    fn unready_trees_and_sections_are_refused() {
        assert!(!tree_certified(&Tree::default()));
        let mut views = BorrowedSections::default();
        assert!(!views.prepare(&SceneSections::default()));
        assert_eq!(views.failure, borrowed_failure::SOURCE);
        assert!(!views.bind(&SceneSections::default(), &CallerLifetime::default()));
        assert_eq!(views.failure, borrowed_failure::NOT_PREPARED);
        assert!(!ScenePlan::default().matches_compiled(&Snapshot::default()));
    }
}
