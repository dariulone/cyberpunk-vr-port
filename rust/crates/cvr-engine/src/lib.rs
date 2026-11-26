//! The REDengine boundary (stage 2 of docs/rust-migration-plan.md). One module owns what touches the game's code:
//!
//!   game_build  which Cyberpunk2077.exe this is, and whether the port's addresses belong to it
//!   addresses   the verified engine RVAs, generated from the same header the C++ half reads (`cvr-tool
//!               engine-addresses`), so both languages take every address from one checked source
//!   code        guarded reads of engine memory and expected-instruction checks before anything is patched
//!   detours     the stereo module's engine detours by RVA: registered where they are written, installed in one pass
//!   hooks       the detour manager: chains installed all or nothing, refused on an unknown build or an address
//!               remap.py could not carry over
//!   stages      the hook stages: every CVR_HOOK registered, a stage installed in order with one line per hook
//!   trampoline  hand-written patches: the one stub arena of the plugin, the byte emitter, the jump into a stub
//!   vtable      the COM vtable slot registry: a slot patched once, the method it held found by the detour
pub mod addresses;
pub mod code;
pub mod detours;
pub mod far_hook;
pub mod game_build;
pub mod hooks;
mod seh;
pub use seh::{install_first_chance, GUARDED_FAULTS};
pub mod stack;
pub mod stages;
pub mod trampoline;
pub mod vtable;
