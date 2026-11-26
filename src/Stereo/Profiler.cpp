#include "Utils/DebugGate.hpp"
// Profiler -- where the two-view CPU cost actually goes.
//
// QPC accumulators split main vs vrcam, drained and published once per frame from the Present hook.
// Nothing here decides anything: it exists so that "the second view is expensive" can be answered
// with a number instead of an impression, which is what closed the CPU-optimisation question once.
//
// Lifted out of src/Stereo/SyncStereo.cpp. The cut ends at ProfPublish's closing brace rather than
// the next blank line at top level: snapping to the next blank swept in a thread_local belonging to
// the view-tagging group four lines further down. Depth zero is necessary, not sufficient.

#include "Stereo/SyncStereo.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include <d3d11.h>
#include <d3d11on12.h>
#include <dxgi1_4.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/StereoInternal.hpp"

namespace cvr {
namespace detail {

// ---- CPU profiling (phase 0): where does the 2-view CPU cost go? ------------
// QPC wall-clock accumulators, split main vs vrcam, averaged over a window and
// published to the overlay. Goal: locate the CPU bottleneck (graph build vs node
// dispatch vs submit) before optimizing the simultaneous true-stereo path.
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/stereo_start.rs
LARGE_INTEGER g_qpc_freq = { };
double        g_qpc_to_ms = 0.0;   // 1000/freq, set in init
#endif
#if CVR_RUST_VRIK
// THE PROFILER'S WORK IS RUST'S (rust/crates/cvr-render/src/stereo_profiler.rs). This file keeps what the other stereo
// files and the DLL's export table name -- the accumulators, the switches and counters, the HUD gate's state -- and
// forwards the functions.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's is rust/crates/cvr-render/src/cpp_services.rs'
extern "C" {
// The stereo module's log for the Rust half: its switch and its rate cap are one for the whole module.
void cvr_cpp_stereo_log(const char* text) { log("%s", text); }
}
#endif
#else
int64_t prof_now() { LARGE_INTEGER t; QueryPerformanceCounter(&t); return t.QuadPart; }
#endif
// per-window raw accumulators (reset each publish window)
#if CVR_RUST_VRIK
#else
std::atomic<int64_t>  g_prof_build_main_ticks{0};   // FullBuild+IncrBuild, main views
#endif
#if CVR_RUST_VRIK
#else
std::atomic<int64_t>  g_prof_build_vrcam_ticks{0};  // FullBuild+IncrBuild, vrcam view
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_build_main_calls{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_build_vrcam_calls{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<int64_t>  g_prof_disp_main_ticks{0};    // NodeDispatch orig call, main-thread nodes
#endif
#if CVR_RUST_VRIK
#else
std::atomic<int64_t>  g_prof_disp_vrcam_ticks{0};   // NodeDispatch orig call, vrcam nodes
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_disp_main_nodes{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_disp_vrcam_nodes{0};
#endif
#if !CVR_RUST_VRIK
static std::atomic<int64_t>  g_prof_frame_last{0};         // last Present QPC (frame wall time)
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_frames{0};             // frames since last audit dump/reset
#endif
// Audit denominators. Present count is NOT a safe divisor (frame generation presents more
// often than the engine builds views), so the dump also carries the number of TOP-LEVEL
// node dispatches per view -- that is exactly one per rendered view-frame -- plus the wall
// window. The DLL therefore emits raw totals and lets the parser normalise.
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_top_main{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint64_t> g_prof_top_vrcam{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<int64_t>  g_prof_window_t0{0};          // QPC at window start
#endif
// published averages (ms) read by the overlay
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfBuildMainMs = 0.0;
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfBuildVrcamMs = 0.0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfDispMainMs = 0.0;
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfDispVrcamMs = 0.0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) double   CyberpunkVR_ProfFrameMs = 0.0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ProfDispVrcamNodes = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ProfDispMainNodes = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_ProfEnable = 0;   // master toggle (OFF: profiler dormant, no per-node QPC/atomic cost)
// NODE TIMELINE (diagnostic, off by default). The per-node totals above say how much CPU each view
// costs, not WHEN it runs -- and in a CPU-bound frame the question is the critical path: whether the
// two views' graphs overlap or follow each other. Every top-level render-graph node, every frame-graph
// build and every Present lands here as one span; tools/hitch_tests/node_timeline.py reads the ring
// live. kind: 0 MAIN node, 1 VRCAM node, 2 other view's node, 3 Present, 4 MAIN build, 5 VRCAM build.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_NodeTimeline = 0;
#endif
#if !CVR_RUST_VRIK   // defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs
extern "C" __declspec(dllexport) ProfSpan CyberpunkVR_NodeTimelineRing[ProfSpanCount]{};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_NodeTimelineCursor{0};
#endif
#if CVR_RUST_VRIK
#else
void prof_timeline_add(int64_t start, int64_t end, uint32_t rva, uint32_t kind, uint32_t extra) {
    auto& span = CyberpunkVR_NodeTimelineRing[CyberpunkVR_NodeTimelineCursor.fetch_add(1, std::memory_order_relaxed) & (ProfSpanCount - 1)];
    span.start = start; span.end = end; span.rva = rva; span.kind = kind; span.thread = GetCurrentThreadId(); span.reserved = extra;
}
#endif
// Script-side trigger for CyberpunkVR_ProfDumpNodes (normally an overlay button): a nonzero write
// makes the next published frame log the per-node audit and reset its window. The live
// measurement (tools/single_pass_tests/node_cpu_audit.py) dumps once to start a clean window and
// once to read it; tools/node_audit_md.py parses the log lines.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ProfDumpRequest{0};
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_ProfDumpCount{0};
#endif
#if !CVR_RUST_VRIK
extern "C" __declspec(dllexport) void CyberpunkVR_ProfDumpNodes();
#endif
// Called once per frame from the overlay's Present hook (via the exported symbol
// below); averages the window and resets. window = frames since last publish.
#if CVR_RUST_VRIK
#else
extern "C" __declspec(dllexport) void CyberpunkVR_ProfPublish() {
    if (!CyberpunkVR_ProfEnable || !g_qpc_to_ms) return;
    if (CyberpunkVR_ProfDumpRequest.exchange(0, std::memory_order_acq_rel)) {
        CyberpunkVR_ProfDumpNodes();
        CyberpunkVR_ProfDumpCount.fetch_add(1, std::memory_order_release);
    }
    const int64_t now = prof_now();
    const int64_t prev = g_prof_frame_last.exchange(now, std::memory_order_relaxed);
    if (prev) CyberpunkVR_ProfFrameMs = (double)(now - prev) * g_qpc_to_ms;
    g_prof_frames.fetch_add(1, std::memory_order_relaxed);
    int64_t unset = 0;                  // start the audit window at the first published frame
    g_prof_window_t0.compare_exchange_strong(unset, now, std::memory_order_relaxed);
    // per-frame instantaneous build/dispatch cost = window total (these hooks fire
    // per-frame, so the window is exactly one frame when published every frame).
    const int64_t bm = g_prof_build_main_ticks.exchange(0, std::memory_order_relaxed);
    const int64_t bv = g_prof_build_vrcam_ticks.exchange(0, std::memory_order_relaxed);
    const int64_t dm = g_prof_disp_main_ticks.exchange(0, std::memory_order_relaxed);
    const int64_t dv = g_prof_disp_vrcam_ticks.exchange(0, std::memory_order_relaxed);
    CyberpunkVR_ProfBuildMainMs  = (double)bm * g_qpc_to_ms;
    CyberpunkVR_ProfBuildVrcamMs = (double)bv * g_qpc_to_ms;
    CyberpunkVR_ProfDispMainMs   = (double)dm * g_qpc_to_ms;
    CyberpunkVR_ProfDispVrcamMs  = (double)dv * g_qpc_to_ms;
    CyberpunkVR_ProfDispMainNodes  = (uint32_t)g_prof_disp_main_nodes.exchange(0, std::memory_order_relaxed);
    CyberpunkVR_ProfDispVrcamNodes = (uint32_t)g_prof_disp_vrcam_nodes.exchange(0, std::memory_order_relaxed);
    g_prof_build_main_calls.exchange(0, std::memory_order_relaxed);
    g_prof_build_vrcam_calls.exchange(0, std::memory_order_relaxed);
}
#endif

// ================================================================================================
// PER-NODE ACCOUNTING, moved here from the monolith in two blocks, with the HUD capability grant that
// sat between them going to Hud.cpp instead.
//
// The window accumulators above answer "how much does the second view cost". These answer "WHICH NODE",
// which is a different question and needs a different shape: a table keyed by the node's work-function
// RVA, split by view, with a self-time as well as a total so a node that merely CONTAINS expensive
// children is not blamed for them.
//
// THE NODE CUT SET IS NOT A PROFILER FEATURE, and it is here only because it shares the table. It lets
// a node be skipped for one view by RVA, from the overlay, live -- which is how "is this pass what
// costs us" gets answered by measurement instead of argument. It returns a chosen value in place of the
// node's own, and a wrong value there is a wrong picture, not a crash.
//
// Both tables are FIXED-SIZE (g_prof_pairs is 2048 for ~400 expected live rows). The dump prints its
// own row count, because this project has four times had a fixed-size table stop working silently.
// ================================================================================================

#if CVR_RUST_VRIK
#else
void prof_node_add(uintptr_t work, int64_t dt, int64_t self, bool vrcam) {
    if (!work || !g_exe_base) return;
    const uintptr_t rva = work - reinterpret_cast<uintptr_t>(g_exe_base);
    uint32_t h = static_cast<uint32_t>((rva >> 4) * 2654435761u);
    for (int probe = 0; probe < 16; ++probe) {
        ProfNode& n = g_prof_nodes[(h + probe) & 511];
        uintptr_t cur = n.rva.load(std::memory_order_relaxed);
        if (cur == 0) {
            uintptr_t expected = 0;
            if (n.rva.compare_exchange_strong(expected, rva)) cur = rva;
            else cur = expected;                  // lost race: someone claimed it
        }
        if (cur == rva) {
            if (vrcam) {
                n.ticks_vrcam.fetch_add(dt, std::memory_order_relaxed);
                n.self_vrcam.fetch_add(self, std::memory_order_relaxed);
                if (n.calls_vrcam.fetch_add(1, std::memory_order_relaxed) == 0)
                    n.ord_vrcam.store(g_prof_ord_vrcam.fetch_add(1, std::memory_order_relaxed) + 1,
                                      std::memory_order_relaxed);
            } else {
                n.ticks_main.fetch_add(dt, std::memory_order_relaxed);
                n.self_main.fetch_add(self, std::memory_order_relaxed);
                if (n.calls_main.fetch_add(1, std::memory_order_relaxed) == 0)
                    n.ord_main.store(g_prof_ord_main.fetch_add(1, std::memory_order_relaxed) + 1,
                                     std::memory_order_relaxed);
            }
            return;
        }
    }
}
#endif
// SceneDrv (sub_140332D5C) is the SINGLE work-fn behind ALL scene-geometry passes; the pass
// id is the rtId byte at exec-ctx+0x38. CAUTION: rtId is NOT a stable pass identity -- the
// graph builder hands out the NEXT SEQUENTIAL index per top-level pass scope
// (sub_1408E926C opens, sub_141321968 closes), so one pass added or skipped earlier
// RENUMBERS the whole tail. MAIN and VRCAM therefore disagree on numbering (a constant
// shift of 3 in the tail was observed), and comparing views by rtId NUMBER produces phantom
// "view-only passes". Passes must be matched by CONTENT instead -- hence the pair table below.
#if !CVR_RUST_VRIK
const uint32_t SCENE_DRIVER_WORK_RVA = 0x332D5C;
#endif

// ---- HUD IN THE SECOND EYE ------------------------------------------------------------------
//
// CRenderNode_DrawHUD (work sub_14033E388) opens with a per-view capability test, not with
// anything HUD-specific:
//
//     cmp  [rdx+18h], rsi        ; no view ctx at all ->
//     jz   draw                  ;   draw unconditionally
//     cmp  [rdx+20h], rsi
//     jz   draw
//     lea  rdx, word_143C1C8A0   ; the feature descriptor this node requires
//     call sub_14021BE28         ; does THIS view have those bits?
//     test al, al
//     jz   epilogue              ; <- no: return without drawing
//
// and sub_14021BE28 is a plain subset test over the view's own capability bitmask, 32 qwords at
// view+0x18A0 (6304), against a 0x110-byte descriptor from a table of them:
//
//     while (required[i] & viewBits[i]) == required[i]) if (++i >= 0x20) return 1;
//     return 0;
//
// So the second eye is not being refused by anything to do with the HUD -- it simply does not
// carry that capability bit, because a render-to-texture camera is an engine feature meant for
// mirrors and surveillance monitors, where a HUD would be wrong.
//
// This is NOT the component's `features` field (entRenderToTextureFeatures, 8 bytes: decals,
// particles, forwardNoTXAA, AA, contact shadows, local shadows, SSAO, reflections). That one is
// a handful of quality switches; the mask tested here is 2048 bits assembled by the view
// producer. Setting `features` cannot reach it.
//
// The narrowest correct intervention is therefore to answer that one question differently, and
// only for the second eye, and only while the HUD node is the one asking.
// RenderFinal2D is NOT part of this problem: it already runs for the second view -- the whole
// right-eye capture hangs off it (RENDER_FINAL2D_WORK_RVA above, the ctx-keyed RTV redirect).
// So the composite that would put a HUD surface on screen is present; only the node that DRAWS
// the HUD is being refused.
#if !CVR_RUST_VRIK
const uint32_t DRAWHUD_WORK_RVA        = 0x33E388;   // CRenderNode_DrawHUD
const uintptr_t VIEW_FEATURE_CHECK_RVA = 0x5D8D48;   // sub_1405D8D48 (2.31 sub_14021BE28): mask at ctx+0x2688 (2.31: +0x18A0)
#endif

// OFF -- forcing the gate CRASHES, and that is the useful result.
//
// With the refusal overridden the node ran on and immediately took an access violation reading
// 0xF0 off a null pointer (report 20260728-105606; our "capability refusal overridden" line is
// the last thing in the log before it). So the capability test is not an arbitrary veto: it is
// the engine declining to run a node whose prerequisites this view does not have. Skipping the
// question does not create the answer -- behind it there is no 2D/HUD state on an RTT view, and
// past the first missing pointer there would only be the next one.
//
// The way in is therefore to SATISFY the condition, not remove it: find which capability bits
// the node requires, find which of them the second view lacks, and set them where the view is
// built -- early enough that the engine itself allocates everything that follows. The mask dump
// below is the measurement that makes that possible.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int CyberpunkVR_HudInVrcam = 0;
#endif
// Dispatch census: does the node reach the second view AT ALL? If Vrcam stays 0 while Main
// climbs, the node is not in that view's graph and the capability test is not the obstacle --
// a different problem, and this hook cannot fix it.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudNodeMain    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudNodeVrcam   = 0;
// How often the engine refused the second eye, and how often we overrode that refusal.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudGateDenied  = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudGateForced  = 0;
#endif

#if !CVR_RUST_VRIK
using ViewFeatureCheckFn = uint8_t (__fastcall*)(uintptr_t work_context, uintptr_t required);
#endif
#if !CVR_RUST_VRIK
ViewFeatureCheckFn g_view_feature_check_orig = nullptr;
#endif

// ---- THE SECOND GATE: the view's draw-block list ---------------------------------------------
//
// Past the capability test, DrawHUD immediately does:
//
//     1401EE810  mov  rcx, rdi            ; work_context
//     1401EE813  call sub_14033C248       ; -> viewData
//     1401EE823  mov  r15, [r14+168h]     ; the view's draw-block list
//     1401EE83E  test r15, r15
//     1401EE841  jz   loc_1401F00EA       ; empty -> return without drawing
//
// The counters said this is where it stops: `denied` came out EXACTLY equal to the VRCAM
// dispatch count, and DrawHUD asks the capability question twice. One refusal per call means the
// node never reached the second question -- it left in between, and this is the only exit there.
//
// viewData+0x168 is not a new discovery either: Detour_DrawComposition already reads that slot,
// already calls it the block list, and already lends MAIN's to VRCAM under CullReuseMode 5. So
// the mechanism is known-good; it just was never applied to the HUD node.
//
// Borrowed and RESTORED around the call, never assigned: the slot belongs to the engine's view
// object, and leaving a foreign pointer in it after the node returns would hand MAIN's list to
// whatever runs next on that view.
#if !CVR_RUST_VRIK
using HudViewDataFn = void* (__fastcall*)(void*);
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_shared.rs.
#else
HudViewDataFn g_hud_viewdata_get = nullptr;
void* g_hud_block_main = nullptr;      // MAIN's list, captured at MAIN's own DrawHUD
#endif
// OFF for the same reason as the gate override: lending the list only carried the node further
// into state the view does not have. Kept because the measurement it produces (null vs ok) is
// still worth having, and because it becomes correct once the capability bits are set properly.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) int CyberpunkVR_HudBorrowBlocks = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockNull    = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockOk      = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugHudBlockLent    = 0;
#endif

// ---- WHAT EXACTLY THE SECOND VIEW IS MISSING -------------------------------------------------
//
// The capability test compares a 32-qword requirement descriptor against the view's own 32-qword
// bitmask, and refuses when the view does not contain every required bit:
//
//     required = word_143C1C8A0 + 8      (the descriptor's payload starts one qword in)
//     viewBits = ctx + 6304              (= view + 0x18A0)
//     for i in 0..31: if ((required[i] & viewBits[i]) != required[i]) -> refuse
//
// Note the OTHER flag block we already manipulate, at ctx + 6096, is NOT this one -- that is the
// frame-graph build word pair (f0/f1) the [fgflags] path forces. 208 bytes apart, different
// purpose, and it was never going to reach this test.
//
// Dumped ONCE for MAIN and ONCE for VRCAM, plus the per-qword delta, so the answer is a list of
// bit positions rather than a theory. Those positions are what has to be set where the view is
// created -- early enough that the engine allocates the HUD state itself, which is the whole
// difference between this and the override that crashed.
#if !CVR_RUST_VRIK
const uintptr_t HUD_REQUIRED_MASK_RVA = 0x3C1C8A0;   // word_143C1C8A0
#endif
#if CVR_RUST_VRIK
#else
bool g_hud_mask_dumped_main  = false;
#endif
#if CVR_RUST_VRIK
#else
bool g_hud_mask_dumped_vrcam = false;
#endif

// ---- GRANT THE CAPABILITY INSTEAD OF SKIPPING THE TEST ---------------------------------------
//
// Measured, the second view is short of exactly ONE bit of what DrawHUD asks for:
//
//     MAIN  w11 req=...0080 have=0000000000CFFFBF missing=0
//     VRCAM w11 req=...0080 have=0000000000CF6E3F missing=0000000000000080
//
// word 11, bit 7 -- absolute feature 711. (The full difference between the two views in that
// word is 0x9180, bits 7/8/12/15; the HUD needs only the first.) And note this has nothing to do
// with the f0/f1 pair the [fgflags] path forces: VRCAM's f0/f1 are already a superset of MAIN's
// (3C00017F vs 3C00017D), which is why FORCED never changed anything.
//
// Why setting the bit is not the same thing as the override that crashed: the override answered
// one question at the moment it was asked, deep inside the node, long after the work that
// question guards was skipped. The bit is what the rest of the engine READS -- including,
// evidently, whatever collects HUD elements, since VRCAM's draw-block list comes out empty on
// every single frame (blocks null == vrcam dispatch count, exactly). Set early and left set, the
// view genuinely declares the capability and the engine populates the state itself.
//
// Whether that is enough is a measurement, not a claim: if the block list stops being null, the
// collection ran and this was the right lever. If it stays null, collection is gated somewhere
// else and this bit is only the last of several conditions.
//
// RESULT: it was necessary but not sufficient. `denied` went to 0 with no crash -- the view now
// passes the test honestly -- but the draw-block list stays empty on every frame.
//
// And the obvious explanation is WRONG, so do not reach for it again: both views are built by the
// SAME builder. The log says so directly ("VRCAM built via FULL (sub_141D43040)"), and VRCAM's
// f0/f1 are a superset of MAIN's. Same graph, same nodes; the difference is in the DATA the
// nodes find, not in which nodes exist. So the empty list has a writer that runs for one view and
// not the other, and finding that writer is the next step -- not re-litigating the builder.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) int      CyberpunkVR_HudGrantCap  = 1;
#endif

// ProfPass moved to Stereo/StereoInternal.hpp.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // no code of the plugin names it
ProfPass g_prof_scenepass[256];   // full 0..255: 0xFF is a legitimate rtId value
#endif

// (rtId, work-fn) pair table: which NAMED nodes ran under which pass, per view. This is what
// makes the pass list comparable across views without knowing a pass name at all -- a pass is
// identified by the multiset of nodes inside it.
// ctx+0x38 is safe as the key: both ctx ctors (sub_1401ECBA0/sub_14040AFD0) init it to 0xFF,
// sub_14040B10C sets it from the node descriptor right before the runner dispatches, the
// copy-ctor sub_140333374 propagates it, and the driver never writes any ctx field -- so it is
// constant for a whole pass and inherited by children. Keying on it beats a thread-local
// "enclosing pass" stack, because job-batched children can run on a worker thread at depth 0
// while still carrying the correct inherited rtId.
// Depth is a COLUMN, not part of the key: for a top-level node the row means "this node's own
// ctx slot", not "ran inside pass N".
#if !CVR_RUST_VRIK
struct ProfPair {
    std::atomic<uint64_t> key;          // (rtId << 32) | work-fn RVA; 0 = empty slot
    std::atomic<int64_t>  self_main;
    std::atomic<int64_t>  self_vrcam;
    std::atomic<uint32_t> calls_main;
    std::atomic<uint32_t> calls_vrcam;
    std::atomic<uint32_t> nested_main;  // dispatched below depth 0 => really inside a pass
    std::atomic<uint32_t> nested_vrcam;
    std::atomic<uint32_t> owner_main;   // dispatches with the node-arg owner bit (+0x30 & 2)
    std::atomic<uint32_t> owner_vrcam;
};
static ProfPair g_prof_pairs[2048];     // ~400 live rows expected (363 main / 320 vrcam)
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugProfPairOverflow = 0;
#endif
#if CVR_RUST_VRIK
#else
// splitmix64 finaliser: the slot index must depend on the WHOLE key. A plain
// `(key >> 4) * 2654435761` does not work here -- the table index uses the LOW bits of the
// product, and those depend only on the low bits of the input, i.e. on rva alone. rtId
// (bits 32+) never reached the index, so every pass sharing a node started at the same slot;
// RenderElements alone appears under ~30 rtIds, which blew past the probe limit and dropped
// 592062 dispatches in the first capture while showing only 404 rows.
static inline uint32_t prof_pair_hash(uint64_t x) {
    x ^= x >> 33; x *= 0xff51afd7ed558ccdULL;
    x ^= x >> 33; x *= 0xc4ceb9fe1a85ec53ULL;
    x ^= x >> 33;
    return (uint32_t)x;
}
void prof_pair_add(uint8_t rtid, uint32_t rva, int64_t self, bool vrcam,
                          bool nested, bool owner) {
    if (!rva) return;
    const uint64_t key = ((uint64_t)rtid << 32) | rva;
    const uint32_t h = prof_pair_hash(key);
    for (int probe = 0; probe < 64; ++probe) {
        ProfPair& p = g_prof_pairs[(h + probe) & 2047];
        uint64_t cur = p.key.load(std::memory_order_relaxed);
        if (cur == 0) {
            uint64_t expected = 0;
            cur = p.key.compare_exchange_strong(expected, key) ? key : expected;
        }
        if (cur != key) continue;
        if (vrcam) {
            p.self_vrcam.fetch_add(self, std::memory_order_relaxed);
            p.calls_vrcam.fetch_add(1, std::memory_order_relaxed);
            if (nested) p.nested_vrcam.fetch_add(1, std::memory_order_relaxed);
            if (owner)  p.owner_vrcam.fetch_add(1, std::memory_order_relaxed);
        } else {
            p.self_main.fetch_add(self, std::memory_order_relaxed);
            p.calls_main.fetch_add(1, std::memory_order_relaxed);
            if (nested) p.nested_main.fetch_add(1, std::memory_order_relaxed);
            if (owner)  p.owner_main.fetch_add(1, std::memory_order_relaxed);
        }
        return;
    }
    CVR_DIAGNOSTIC(++CyberpunkVR_DebugProfPairOverflow);   // table full: rows would be lost, dump says so
}
#endif
// Work-fn RVA -> CRenderNode name, generated from the project's own RE census
// (engine_re/dumps/nodes/nodes_index.md, 163 nodes). This REPLACES the old hand-written
// switch, several entries of which were wrong guesses: +0x5AA54C was labelled
// "ShadowCascades?" but is RenderRainMap, +0x425AA0 "ShadowFamily?" is
// PrepareScreenSpaceWaterDepth, +0xB50EC8 "AsyncRenderJob" is FlushTextureGrabs, and
// +0x557204 "RasterTonemap" is ApplyTXAA. Table is sorted by RVA -> binary search.
#if CVR_RUST_VRIK
#else
#include "NodeNames.inc"
extern "C" __declspec(dllexport) const char* CyberpunkVR_ProfNodeName(uint32_t rva) {
    int lo = 0, hi = (int)(sizeof(k_prof_node_names) / sizeof(k_prof_node_names[0])) - 1;
    while (lo <= hi) {
        const int mid = lo + ((hi - lo) >> 1);
        const uint32_t m = k_prof_node_names[mid].rva;
        if (m == rva) return k_prof_node_names[mid].name;
        if (m < rva) lo = mid + 1; else hi = mid - 1;
    }
    return "";
}
#endif

// ---- NODE CUT (census experiment): skip selected nodes at dispatch ----------
// Rules match work-fn RVA (+ optional SceneDrv rtId) and view side. Armed live
// from the overlay; every change is logged. Master switch default OFF. Used to
// census which nodes are droppable per view (CPU win) without quality loss.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NodeCutEnable = 0;
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_NodeCutRetVal = 1;  // dispatch return when skipping
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugNodeCutSkips = 0;
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/stereo_api.rs
extern "C" __declspec(dllexport) void CyberpunkVR_NodeCutSet(uint32_t rva, uint32_t rtid, int mode) {
    cvr_stereo_node_cut_set(rva, rtid, mode);
}
#endif
#else
static std::atomic<uint32_t> g_cut_rva[64];    // 0 = empty slot
static std::atomic<uint32_t> g_cut_meta[64];   // (rtid<<8)|mode; rtid 0xFF=any; mode 0=off 1=vrcam 2=both 3=main
static std::atomic<int>      g_cut_active{0};
bool node_cut_match(uint32_t rva, uint8_t rtid, bool vrcam) {
    if (g_cut_active.load(std::memory_order_relaxed) <= 0) return false;
    for (int i = 0; i < 64; ++i) {
        if (g_cut_rva[i].load(std::memory_order_relaxed) != rva) continue;
        const uint32_t meta = g_cut_meta[i].load(std::memory_order_relaxed);
        const uint32_t mode = meta & 0xFF, mrt = meta >> 8;
        if (!mode) continue;
        if (mrt != 0xFF && mrt != rtid) continue;
        if (mode == 2 || (mode == 1 && vrcam) || (mode == 3 && !vrcam)) return true;
    }
    return false;
}
extern "C" __declspec(dllexport) int CyberpunkVR_NodeCutGet(uint32_t rva, uint32_t rtid) {
    for (int i = 0; i < 64; ++i) {
        if (g_cut_rva[i].load(std::memory_order_relaxed) != rva) continue;
        const uint32_t meta = g_cut_meta[i].load(std::memory_order_relaxed);
        if ((meta >> 8) == rtid) return (int)(meta & 0xFF);
    }
    return 0;
}
extern "C" __declspec(dllexport) void CyberpunkVR_NodeCutSet(uint32_t rva, uint32_t rtid, int mode) {
    if (!rva) return;
    int slot = -1;
    for (int i = 0; i < 64; ++i) {
        if (g_cut_rva[i].load(std::memory_order_relaxed) == rva &&
            (g_cut_meta[i].load(std::memory_order_relaxed) >> 8) == rtid) { slot = i; break; }
    }
    if (slot < 0) {
        if (!mode) return;
        for (int i = 0; i < 64; ++i) {
            uint32_t expected = 0;
            if (g_cut_rva[i].compare_exchange_strong(expected, rva)) {
                g_cut_meta[i].store((rtid << 8), std::memory_order_relaxed);
                slot = i; break;
            }
        }
        if (slot < 0) { log("[nodecut] table full"); return; }
    }
    g_cut_meta[slot].store((rtid << 8) | (uint32_t)(mode & 0xFF), std::memory_order_release);
    int active = 0;
    for (int i = 0; i < 64; ++i)
        if (g_cut_rva[i].load(std::memory_order_relaxed) &&
            (g_cut_meta[i].load(std::memory_order_relaxed) & 0xFF)) ++active;
    g_cut_active.store(active, std::memory_order_release);
    log("[nodecut] rva=+0x%X rtid=%u mode=%d (%s) active=%d",
        rva, rtid, mode,
        mode == 1 ? "cut-vrcam" : mode == 2 ? "cut-both" : mode == 3 ? "cut-main" : "off",
        active);
}
#endif

// Script-side node-cut rule (the overlay calls CyberpunkVR_NodeCutSet directly). One packed
// request per Present: bits 0..31 work RVA, 32..39 rtId (0xFF any), 40..47 mode (0 off,
// 1 cut-vrcam, 2 cut-both, 3 cut-main), bit 63 set so a zero request means "none".
// Diagnostic only: CyberpunkVR_NodeCutEnable must be set separately and defaults to 0.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/input_exports.rs.
#else
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_NodeCutRequest{0};
extern "C" __declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_NodeCutApplied{0};
#endif
#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/stereo_api.rs
extern "C" __declspec(dllexport) int CyberpunkVR_ProfSnapshotNodes(uint32_t* out_rva, double* out_msv, double* out_msm,
                                                                  uint32_t* out_cv, uint32_t* out_cm, int maxn) {
    return cvr_stereo_prof_snapshot_nodes(out_rva, out_msv, out_msm, out_cv, out_cm, maxn);
}
#endif
#if !CVR_RUST_VRIK   // the Rust build's: rust/crates/cvr-render/src/stereo_api.rs
extern "C" __declspec(dllexport) int CyberpunkVR_ProfSnapshotPasses(uint32_t* out_rtid, double* out_msv, double* out_msm,
                                                                   int maxn) {
    return cvr_stereo_prof_snapshot_passes(out_rtid, out_msv, out_msm, maxn);
}
#endif
#else
extern "C" __declspec(dllexport) void CyberpunkVR_NodeCutPoll() {
    const uint64_t request = CyberpunkVR_NodeCutRequest.exchange(0, std::memory_order_acq_rel);
    if (!(request >> 63)) return;
    CyberpunkVR_NodeCutSet(static_cast<uint32_t>(request), static_cast<uint32_t>((request >> 32) & 0xFF),
                           static_cast<int>((request >> 40) & 0xFF));
    CyberpunkVR_NodeCutApplied.fetch_add(1, std::memory_order_release);
}

// ---- live audit snapshots for the overlay (ms/frame since last dump-reset) --
extern "C" __declspec(dllexport) int CyberpunkVR_ProfSnapshotNodes(
        uint32_t* out_rva, double* out_msv, double* out_msm,
        uint32_t* out_cv, uint32_t* out_cm, int maxn) {
    // SELF ms per VIEW-FRAME. Self, because inclusive double-counts (SceneDrv contains the
    // scene passes it dispatches); per view-frame, because Present count is inflated by
    // frame generation while top-level dispatches are exactly one per rendered view.
    const uint64_t tm_n = g_prof_top_main.load(std::memory_order_relaxed);
    const uint64_t tv_n = g_prof_top_vrcam.load(std::memory_order_relaxed);
    const double fmain  = (double)(tm_n ? tm_n : 1);
    const double fvrcam = (double)(tv_n ? tv_n : 1);
    struct Row { uint32_t rva; double mv, mm; uint32_t cv, cm; };
    static Row rows[512];               // overlay/present thread only
    int n = 0;
    for (int i = 0; i < 512; ++i) {
        const uintptr_t w = g_prof_nodes[i].rva.load(std::memory_order_relaxed);
        if (!w) continue;
        Row r;
        r.rva = (uint32_t)w;
        r.mv = (double)g_prof_nodes[i].self_vrcam.load(std::memory_order_relaxed) * g_qpc_to_ms / fvrcam;
        r.mm = (double)g_prof_nodes[i].self_main.load(std::memory_order_relaxed) * g_qpc_to_ms / fmain;
        r.cv = g_prof_nodes[i].calls_vrcam.load(std::memory_order_relaxed);
        r.cm = g_prof_nodes[i].calls_main.load(std::memory_order_relaxed);
        if (r.mv + r.mm > 0.0) rows[n++] = r;
    }
    if (maxn > n) maxn = n;
    for (int i = 0; i < maxn; ++i) {
        int best = i;
        for (int j = i + 1; j < n; ++j)
            if (rows[j].mv + rows[j].mm > rows[best].mv + rows[best].mm) best = j;
        if (best != i) { Row t = rows[i]; rows[i] = rows[best]; rows[best] = t; }
        out_rva[i] = rows[i].rva; out_msv[i] = rows[i].mv; out_msm[i] = rows[i].mm;
        out_cv[i] = rows[i].cv;   out_cm[i] = rows[i].cm;
    }
    return maxn;
}
extern "C" __declspec(dllexport) int CyberpunkVR_ProfSnapshotPasses(
        uint32_t* out_rtid, double* out_msv, double* out_msm, int maxn) {
    uint64_t fr = g_prof_frames.load(std::memory_order_relaxed);
    const double frames = (double)(fr ? fr : 1);
    struct Row { uint32_t rt; double mv, mm; };
    static Row rows[128];
    int n = 0;
    for (int i = 0; i < 128; ++i) {
        const double mv = (double)g_prof_scenepass[i].ticks_vrcam.load(std::memory_order_relaxed) * g_qpc_to_ms / frames;
        const double mm = (double)g_prof_scenepass[i].ticks_main.load(std::memory_order_relaxed) * g_qpc_to_ms / frames;
        if (mv + mm <= 0.0) continue;
        rows[n].rt = (uint32_t)i; rows[n].mv = mv; rows[n].mm = mm; ++n;
    }
    if (maxn > n) maxn = n;
    for (int i = 0; i < maxn; ++i) {
        int best = i;
        for (int j = i + 1; j < n; ++j)
            if (rows[j].mv + rows[j].mm > rows[best].mv + rows[best].mm) best = j;
        if (best != i) { Row t = rows[i]; rows[i] = rows[best]; rows[best] = t; }
        out_rtid[i] = rows[i].rt; out_msv[i] = rows[i].mv; out_msm[i] = rows[i].mm;
    }
    return maxn;
}

// Dump ALL nodes + SceneDrv rtId breakdown (per-frame ms since last dump), reset.
// Emitted pipe-delimited so tools/node_audit_md.py can turn it into a named markdown
// audit; every reuse/experiment toggle is logged alongside, because a capture taken with
// an experiment armed silently invalidates the whole table (that is exactly what made the
// previous audit unusable).
static void prof_log_config();          // defined near the end: needs all the flag decls
extern "C" __declspec(dllexport) void CyberpunkVR_ProfDumpNodes() {
    struct Row { uintptr_t rva; int64_t tm, tv, sm, sv; uint32_t cm, cv, om, ov; };
    static Row rows[512];               // static: keep the hot path stack small
    int nrows = 0;
    for (int i = 0; i < 512; ++i) {
        const uintptr_t w = g_prof_nodes[i].rva.load(std::memory_order_relaxed);
        if (!w) continue;
        Row r;
        r.rva = w;
        r.tm = g_prof_nodes[i].ticks_main.exchange(0, std::memory_order_relaxed);
        r.tv = g_prof_nodes[i].ticks_vrcam.exchange(0, std::memory_order_relaxed);
        r.sm = g_prof_nodes[i].self_main.exchange(0, std::memory_order_relaxed);
        r.sv = g_prof_nodes[i].self_vrcam.exchange(0, std::memory_order_relaxed);
        r.cm = g_prof_nodes[i].calls_main.exchange(0, std::memory_order_relaxed);
        r.cv = g_prof_nodes[i].calls_vrcam.exchange(0, std::memory_order_relaxed);
        r.om = g_prof_nodes[i].ord_main.load(std::memory_order_relaxed);
        r.ov = g_prof_nodes[i].ord_vrcam.load(std::memory_order_relaxed);
        if (r.cm | r.cv) rows[nrows++] = r;
    }
    // sort by SELF time desc: inclusive double-counts parents (SceneDrv contains every
    // scene pass), so self is the only column you may rank or sum.
    for (int i = 0; i < nrows; ++i) {
        int best = i;
        for (int j = i + 1; j < nrows; ++j)
            if (rows[j].sm + rows[j].sv > rows[best].sm + rows[best].sv) best = j;
        if (best != i) { Row t = rows[i]; rows[i] = rows[best]; rows[best] = t; }
    }
    const uint64_t frames    = g_prof_frames.exchange(0, std::memory_order_relaxed);
    const uint64_t top_main  = g_prof_top_main.exchange(0, std::memory_order_relaxed);
    const uint64_t top_vrcam = g_prof_top_vrcam.exchange(0, std::memory_order_relaxed);
    const int64_t  now       = prof_now();
    const int64_t  t0        = g_prof_window_t0.exchange(now, std::memory_order_relaxed);
    const double   win_ms    = t0 ? (double)(now - t0) * g_qpc_to_ms : 0.0;
    prof_log_config();
    // ALL numbers below are WINDOW TOTALS, not per-frame. Divide main columns by
    // view_frames_main and vrcam columns by view_frames_vrcam (tools/node_audit_md.py).
    // NOT frame counts: the graph runner calls the executor directly for most nodes, so
    // almost every node is a top-level dispatch. The parser derives frames from the modal
    // per-node call count instead (tools/node_audit_md.py).
    log("[prof] AUDIT|nodes=%d|presents=%llu|node_dispatches_main=%llu|node_dispatches_vrcam=%llu"
        "|window_ms=%.1f|totals_not_per_frame",
        nrows, (unsigned long long)frames,
        (unsigned long long)top_main, (unsigned long long)top_vrcam, win_ms);
    log("[prof] HDR|ord_main|ord_vrcam|rva|name|calls_main|calls_vrcam"
        "|self_main_ms|self_vrcam_ms|incl_main_ms|incl_vrcam_ms");
    for (int i = 0; i < nrows; ++i) {
        const char* nm = CyberpunkVR_ProfNodeName((uint32_t)rows[i].rva);
        log("[prof] N|%u|%u|0x%06llX|%s|%u|%u|%.4f|%.4f|%.4f|%.4f",
            rows[i].om, rows[i].ov, (unsigned long long)rows[i].rva, nm[0] ? nm : "?",
            rows[i].cm, rows[i].cv,
            (double)rows[i].sm * g_qpc_to_ms, (double)rows[i].sv * g_qpc_to_ms,
            (double)rows[i].tm * g_qpc_to_ms, (double)rows[i].tv * g_qpc_to_ms);
    }
    // Scene passes, same convention: window totals. These are INCLUSIVE of the child nodes
    // the pass dispatches (which now have their own N rows), so pass ms and node ms overlap.
    log("[prof] HDRP|rtid|calls_main|calls_vrcam|incl_main_ms|incl_vrcam_ms");
    for (int i = 0; i < 256; ++i) {
        const int64_t tm = g_prof_scenepass[i].ticks_main.exchange(0, std::memory_order_relaxed);
        const int64_t tv = g_prof_scenepass[i].ticks_vrcam.exchange(0, std::memory_order_relaxed);
        const uint32_t cm = g_prof_scenepass[i].calls_main.exchange(0, std::memory_order_relaxed);
        const uint32_t cv = g_prof_scenepass[i].calls_vrcam.exchange(0, std::memory_order_relaxed);
        if (!(cm | cv)) continue;
        log("[prof] P|%d|%u|%u|%.4f|%.4f",
            i, cm, cv, (double)tm * g_qpc_to_ms, (double)tv * g_qpc_to_ms);
    }
    // (rtId, node) pairs: the pass CONTENT, which is what makes passes comparable between the
    // views given that rtId numbering is build-order and shifts between them.
    if (CyberpunkVR_DebugProfPairOverflow)
        log("[prof] PAIRWARN|overflow=%llu|rows_were_dropped_raise_g_prof_pairs",
            (unsigned long long)CyberpunkVR_DebugProfPairOverflow);
    CyberpunkVR_DebugProfPairOverflow = 0;
    log("[prof] HDRR|rtid|rva|name|calls_main|calls_vrcam|nested_main|nested_vrcam"
        "|owner_main|owner_vrcam|self_main_ms|self_vrcam_ms");
    for (int i = 0; i < 2048; ++i) {
        const uint64_t k = g_prof_pairs[i].key.exchange(0, std::memory_order_relaxed);
        const uint32_t cm = g_prof_pairs[i].calls_main.exchange(0, std::memory_order_relaxed);
        const uint32_t cv = g_prof_pairs[i].calls_vrcam.exchange(0, std::memory_order_relaxed);
        const uint32_t nm = g_prof_pairs[i].nested_main.exchange(0, std::memory_order_relaxed);
        const uint32_t nv = g_prof_pairs[i].nested_vrcam.exchange(0, std::memory_order_relaxed);
        const uint32_t om = g_prof_pairs[i].owner_main.exchange(0, std::memory_order_relaxed);
        const uint32_t ov = g_prof_pairs[i].owner_vrcam.exchange(0, std::memory_order_relaxed);
        const int64_t sm = g_prof_pairs[i].self_main.exchange(0, std::memory_order_relaxed);
        const int64_t sv = g_prof_pairs[i].self_vrcam.exchange(0, std::memory_order_relaxed);
        if (!k || !(cm | cv)) continue;
        const uint32_t rva = (uint32_t)(k & 0xFFFFFFFFull);
        const unsigned rtid = (unsigned)(k >> 32);
        const char* nm2 = CyberpunkVR_ProfNodeName(rva);
        log("[prof] R|%u|0x%06X|%s|%u|%u|%u|%u|%u|%u|%.4f|%.4f",
            rtid, rva, nm2[0] ? nm2 : "?", cm, cv, nm, nv, om, ov,
            (double)sm * g_qpc_to_ms, (double)sv * g_qpc_to_ms);
    }
}

// Snapshot every toggle that can silently change WHAT the audit measures: a capture
// taken with an experiment armed invalidates the whole table. Lives at the end of the
// anonymous namespace because it reads flags declared throughout the file.
static void prof_log_config() {
    log("[prof] CFG|DistantReuse=%u|LocalShadowReuse=%u|GiReuse=%u|VrcamFlagMode=%u"
        "|OcclusionGateForce=%u|CullReuseMode=%u|NodeCutEnable=%d|NodeCutSkips=%llu"
        "|LodOverride=%u|LodMask=%u|LodValue=%.3f|VrcamDlss=%u|VrcamComputeResolve=%u"
        "|MainUpscalerGroups=0x%03X|MainAaMode=%u|VrcamAaMode=%u|frameMs=%.3f",
        CyberpunkVR_DistantReuseMode, CyberpunkVR_LocalShadowReuseMode,
        CyberpunkVR_GiReuseMode, CyberpunkVR_VrcamFlagMode,
        CyberpunkVR_OcclusionGateForce, CyberpunkVR_CullReuseMode,
        CyberpunkVR_NodeCutEnable,
        (unsigned long long)CyberpunkVR_DebugNodeCutSkips,
        CyberpunkVR_LodThreshOverrideEnable, CyberpunkVR_LodThreshApplyMask,
        CyberpunkVR_LodThreshValue, CyberpunkVR_VrcamDlss,
        CyberpunkVR_VrcamComputeResolve, CyberpunkVR_DebugMainUpscalerGroups,
        CyberpunkVR_DebugMainAaMode, CyberpunkVR_DebugVrcamAaMode,
        CyberpunkVR_ProfFrameMs);
}
#endif

}  // namespace detail
}  // namespace cvr
