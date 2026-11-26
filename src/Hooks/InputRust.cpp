#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The cyberware chord in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/cyberware_chord.rs) in place of
// src/Hooks/CyberwareChord.cpp: cvr::input::*CyberwareChord, forwarded.
#include "Hooks/CyberwareChord.hpp"

#if !CVR_RUST_VRIK
extern "C" {
#endif
#if !CVR_RUST_VRIK
}
#endif

#endif
