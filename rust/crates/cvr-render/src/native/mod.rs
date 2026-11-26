//! Single pass's native graph and resource records (the logic of include/Render/Native*.hpp), ported bottom-up: each
//! module is the header's data and checks, tested with the cases of its tools/single_pass_tests program, ahead of the
//! C++ consumers that move onto it.
//!
//!   allocator_binding  the common allocator's identity receipt and the allocations a saved definition tree implies
//!   compiler_identity  the render graph compiler's identity pinned through one dispatch
//!   consumer_identity  the native root's identity ticket: owner and group pins, constructor transfer, late queue match
//!   declaration_completion  the graph's declaration jobs' D0 group pinned through the declaration's completion
//!   definition_generations  the definition owners' generations and the consumers bound to them, to retirement
//!   definition_owner  the graph's definition owner vector's move proposal and the definitions' single retirement
//!   definition_tree  a bounded, twice-read metadata snapshot of a definition's native object tree and its storage
//!   graph_jobs  the engine job groups a graph's nodes run in, and the leases held on them
//!   instance_ring  the instance upload ring's numeric observations and its completed writers' owned CPU copies
//!   job_receipt  a deferred receipt of a graph's job groups and frame owner, matched to its declaration later
//!   job_lease  a hold on an engine job group pair, released on drop
//!   graph_parent  a node's parent receipt from the native context's exact view and order slot; the code certificate
//!   graph_snapshot  an owned, re-verified copy of a compiled render graph's nodes and edges
//!   resource_uses  a bounded snapshot of the native resource-use pool the graph compiler reads
//!   retirement_deferral  a definition owner's added readers, exact native intent, header transfer and one destruction
//!   stage_definition  a render-elements node's native stage definition bytes, read twice against the tree and graph
//!   task_completion  a scheduler callback's terminal job group, held to the callback's completion
//!   upload_arena  the native upload arena's fields and the guarded ready/reset/destroy invocation stack
//!   upload_reservation  the upload ring allocator's successful-CAS receipts and the external witness patch plan
pub mod allocator_binding;
pub mod compiler_identity;
pub mod consumer_identity;
pub mod declaration_completion;
pub mod definition_generations;
pub mod definition_owner;
pub mod definition_tree;
pub mod graph_jobs;
pub mod graph_parent;
pub mod graph_snapshot;
pub mod instance_ring;
pub mod job_lease;
pub mod job_receipt;
pub mod resource_uses;
pub mod retirement_deferral;
pub mod stage_definition;
pub mod task_completion;
pub mod upload_arena;
pub mod upload_reservation;
