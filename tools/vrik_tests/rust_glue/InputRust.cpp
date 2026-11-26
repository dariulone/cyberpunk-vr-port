// tools/vrik_tests' copy of src/Hooks/InputRust.cpp as the plugin had it when the plugin's copy started losing what only these
// programs call (the C forms of the C++ the Rust solve reads, and the C++ names over the solve's Rust exports).
// The Rust variants (vrik_*_tests_rust) compile this file in place of the plugin's.
// The cyberware chord in Rust (CVR_RUST_VRIK: rust/crates/cvr-rig/src/vrik/cyberware_chord.rs) in place of
// src/Hooks/CyberwareChord.cpp: cvr::input::*CyberwareChord, forwarded.
#include "Hooks/CyberwareChord.hpp"

extern "C" {
bool cvr_cyberware_chord_update(bool l3,float grip,bool r3,bool inputActive);
void cvr_cyberware_chord_dispatch();
bool cvr_cyberware_chord_take();
}

bool cvr::input::UpdateCyberwareChord(bool l3,float grip,bool r3,bool inputActive) {
    return cvr_cyberware_chord_update(l3,grip,r3,inputActive);
}
void cvr::input::DispatchCyberwareChord() { cvr_cyberware_chord_dispatch(); }
bool cvr::input::TakeCyberwareChord() { return cvr_cyberware_chord_take(); }
