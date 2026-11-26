// The exports src/Render/SinglePassTrace.cpp defines in the reference build and the Rust build defines in Rust (its rust-build
// modules): their reference definitions, for the single-pass tests built whole in the Rust build's form, whose
// Rust library leaves those modules out.
#include "Render/SinglePassTrace.hpp"
#include <atomic>
#include <cstdint>
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_SinglePassTraceCount{0},CyberpunkVR_SinglePassTraceNodeFilter{},CyberpunkVR_SinglePassTraceRequest{0},CyberpunkVR_SinglePassTraceSeq{0},CyberpunkVR_SinglePassTraceState{0};
}
