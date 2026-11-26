#include "Utils/DebugGate.hpp"
#include "Render/SharpenPolicy.hpp"
#include "Stereo/RenderParity.hpp"
#include "Stereo/FrameGraphFeatures.hpp"
#include "Stereo/VrcamViewport.hpp"
// FrameGraph -- what the engine's frame graph decides for each view, and the RTT view it decides it for.
//
// The graph is rebuilt (fully, or incrementally) and for each view it computes a set of FEATURE BITS
// and a set of UPSCALER GROUPS. Those two sets are the whole reason the second eye can differ from the
// first while running the same code: a bit that is set for MAIN and clear for VRCAM means a pass simply
// does not run for the second view.
//
// fg_observe exists to make that difference visible. It diffs the two views' bit sets and prints what
// only one of them has -- which is how the missing passes were found, one bit at a time, rather than by
// reading a graph that has no textual form.
//
// FORCING A BIT IS NOT THE SAME AS THE PASS WORKING. A forced bit makes the producer run; whether its
// consumer has what it needs is a separate question, and the reuse code in ViewReuse.cpp is where that
// distinction is actually paid for. This file only decides what is asked for.
//
// maybe_resize_rtt and Detour_RTTViewCreate are here because the view being decided about is created
// here: the RTT component's texture is what the second eye renders into, and its dimensions have to be
// settled before any of the above means anything. Forcing ctx+0x44/0x48 to a different W/H is the note
// inside the moved block -- it records what that does and does not achieve.

#include "Camera/CameraState.hpp"   // the second eye's pose + the RTT dirty flag
#include "Overlay/ImGuiOverlay.hpp"   // OverlayArmLoadGuard
#include "Stereo/SyncStereo.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include "Stereo/EngineStereo.hpp"   // the scope camera's readiness without a second view
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
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"

namespace cvr {
namespace detail {

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static void maybe_resize_rtt(uintptr_t comp);   // defined after g_main_ctx
#endif

#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin's scope is rust/crates/cvr-render/src/frame_graph.rs'
thread_local bool t_rtt_prepare_ours = false;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
// The original, called with the scope flag raised when this preparer call is for OUR view. Every
// return path of the detour below goes through here, so the flag cannot leak past the call.
// Identified by the camera-name hash at a1+0x260 and by nothing else, which is this project's
// only accepted way to say which view an object belongs to.
static __int64 rtt_call_orig(__int64 a1, __int64 a2) {
    bool ours = false;
    __try {
        const auto key=a1?*reinterpret_cast<uint64_t*>(a1+0x260):0;
        ours = a1 && (key==g_vrcam_ctx_key.load(std::memory_order_relaxed) ||
                      key==cvr::cname_hash("vr_optics_feed"));
    } __except (EXCEPTION_EXECUTE_HANDLER) { ours = false; }
    const bool prev = t_rtt_prepare_ours;
    t_rtt_prepare_ours = ours;
    const __int64 r = g_orig_rtt_viewcreate(a1, a2);
    t_rtt_prepare_ours = prev;
    return r;
}

static __int64 __fastcall Detour_RTTViewCreate(__int64 a1, __int64 a2) {
    if (a1) {
        __try {
            uint64_t vt = *reinterpret_cast<uint64_t*>(a1);
            if (vt == reinterpret_cast<uint64_t>(g_exe_base) + RTT_HOST_VTABLE_RVA) {
                uint32_t w = *reinterpret_cast<uint32_t*>(a1 + 0x258);
                uint32_t h = *reinterpret_cast<uint32_t*>(a1 + 0x25C);
                // Bind ONE component and keep it. This used to latch whichever RTT component
                // came through last, which was harmless when the player carried a single
                // vrcam component -- but there is now one per resolution, so the per-frame
                // fov writes could land on a disabled (or destroyed) one.
                // The selected resolution is the discriminator, and it is consulted ONLY on
                // the first bind: the authored set has exactly one component per resolution,
                // and the resolution comes from the launcher's pick, so nothing is hardcoded.
                const uintptr_t cached = g_vrcam_comp.load(std::memory_order_relaxed);
                const uint32_t sel_w = g_vrcam_sel_w.load(std::memory_order_relaxed);
                const uint32_t sel_h = g_vrcam_sel_h.load(std::memory_order_relaxed);
                const bool dims_match = !sel_w || !sel_h || (w == sel_w && h == sel_h);
                if (cached && static_cast<uintptr_t>(a1) != cached) {
                    if (!dims_match) {
                        CVR_DIAGNOSTIC(++CyberpunkVR_DebugRttCompRejects);
                        return rtt_call_orig(a1, a2);
                    }
                    // Same selected resolution, different object: the component was destroyed
                    // and re-created (resolution switch / entity respawn). Re-bind, or every
                    // later write would target freed memory.
                    g_vrcam_comp.store(static_cast<uintptr_t>(a1), std::memory_order_release);
                    g_vrcam_base_fov = *reinterpret_cast<float*>(a1 + 0x128);
                    CyberpunkVR_DebugVrcamBaseFov = g_vrcam_base_fov;
                    log("[rtt] re-bound vrcam component %p -> %p (%ux%u)",
                        reinterpret_cast<void*>(cached), reinterpret_cast<void*>(a1), w, h);
                } else if (!cached) {
                    if (!dims_match) {
                        if (cvr::RuntimeDiagnosticsEnabled() && (CyberpunkVR_DebugRttCompRejects++ % 600) == 0)
                            log("[rtt] ignoring component %p %ux%u (selected %ux%u)",
                                reinterpret_cast<void*>(a1), w, h, sel_w, sel_h);
                        return rtt_call_orig(a1, a2);
                    }
                    g_vrcam_comp.store(static_cast<uintptr_t>(a1), std::memory_order_release);
                    // Capture the AUTHORED fov before anything of ours writes to it -- the
                    // zoom needs a reference that cannot drift with our own output.
                    g_vrcam_base_fov = *reinterpret_cast<float*>(a1 + 0x128);
                    CyberpunkVR_DebugVrcamBaseFov = g_vrcam_base_fov;
                    log("[rtt] bound vrcam component %p %ux%u fov=%.3f",
                        reinterpret_cast<void*>(a1), w, h, g_vrcam_base_fov);
                }
                CyberpunkVR_DebugRttComp = static_cast<uint64_t>(a1);
                // MARK THE CAMERA DIRTY SO THE DRIVER ACTUALLY REBUILDS THE VIEW, and give it a fresh
                // pose to rebuild from. See the note beside CyberpunkVR_BdCamDirty in VrCore.cpp for
                // the disassembly this comes from and the measurements that named it.
                //
                // Placed here because this detour runs once per view-create -- i.e. once per driver
                // tick, on the driver's own thread -- and the driver tests the flag at the TOP of its
                // next tick, so a store made here is consumed on the following frame rather than
                // racing the build that has already happened.
                if (CyberpunkVR_BdCamDirty != 0 &&
                    g_bdActive.load(std::memory_order_relaxed) &&
                    g_bdScenePoseValid.load(std::memory_order_acquire)) {
                    __try {
                        uint8_t* comp = reinterpret_cast<uint8_t*>(a1);
                        if ((CyberpunkVR_BdCamDirty & 2) && g_headQuatValid) {
                            // The rotation is the one LocateCamera composed for THIS frame -- the
                            // scene's own base with the head on top -- so both eyes are built from one
                            // composition. The viewpoint is the scene camera plus half an IPD along the
                            // right vector of that same rotation, with the sign copied from
                            // PatchCamera rather than re-derived: getting it wrong is pseudo-stereo.
                            const float hq[4] = { g_headQuatComposed[0], g_headQuatComposed[1],
                                                  g_headQuatComposed[2], g_headQuatComposed[3] };
                            float* q = reinterpret_cast<float*>(comp + 0xF0);
                            q[0] = hq[0]; q[1] = hq[1]; q[2] = hq[2]; q[3] = hq[3];
                            if (CyberpunkVR_IpdInWorldPos) {
                                float r[3] = {};
                                ComputeRightVectorFromQuaternion(hq, r);
                                if (IsPlausibleUnitVector3(r)) {
                                    const float half = GetDesiredHalfIpd();
                                    const float sign = CyberpunkVR_MainIsRightEye ? -1.0f : +1.0f;
                                    int32_t* p = reinterpret_cast<int32_t*>(comp + 0xE0);
                                    for (int i = 0; i < 3; ++i)
                                        p[i] = g_bdScenePosFP[i].load(std::memory_order_relaxed) +
                                               static_cast<int32_t>(r[i] * half * sign * 131072.0f);
                                }
                            }
                            CVR_DIAGNOSTIC(++CyberpunkVR_DebugBdCamPose);
                        }
                        if (CyberpunkVR_BdCamDirty & 1) {
                            *reinterpret_cast<volatile int32_t*>(comp + 0xA00) = 1;
                            CVR_DIAGNOSTIC(++CyberpunkVR_DebugBdCamDirty);
                        }
                    } __except (EXCEPTION_EXECUTE_HANDLER) {}
                }
                CyberpunkVR_DebugRttW = w;
                CyberpunkVR_DebugRttH = h;
                g_mirror_vrcam_serial.fetch_add(1, std::memory_order_release);
                // Resize the OUTPUT DynamicTexture to the target (main res) so the
                // render follows. Converges over 1-2 view-creates (async render cmd).
                maybe_resize_rtt(static_cast<uintptr_t>(a1));
                if (cvr::RuntimeDiagnosticsEnabled() && (CyberpunkVR_DebugRttHits++ % 300) == 0) {
                    log("[rtt] view-create comp=%p dims=%ux%u hits=%llu",
                        reinterpret_cast<void*>(a1), w, h,
                        (unsigned long long)CyberpunkVR_DebugRttHits);
                }
                if (g_rtt_res_override) {
                    *reinterpret_cast<uint32_t*>(a1 + 0x258) = g_rtt_w;
                    *reinterpret_cast<uint32_t*>(a1 + 0x25C) = g_rtt_h;
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return rtt_call_orig(a1, a2);
}
#endif

// --- DIAGNOSTIC (phase 1): where does the eye go in sub_140409DD4's per-view loop? ---
// FULL build  = sub_141D43040(v3, v50, v50+112, ctx+6096, v5)  -> a4 = ctx+6096
// incremental = sub_14227BD10(v3, v50, v50+112, ctx+6096, v5)  -> a4 = ctx+6096
// FinalOnly (LABEL_80) = sub_1428E6700(...)  (ctx not in args -> can't tag the eye)
// If FullEye stays 0 while FullTotal climbs => the eye is diverted BEFORE the full
// build (FinalOnly/skip) => it needs the natural view path, not forcing.
using BuildFn = __int64 (__fastcall*)(__int64, __int64, __int64, __int64, __int64);
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // no code of the plugin names it
static BuildFn g_orig_full_build = nullptr;
#endif
#if !CVR_RUST_VRIK
static BuildFn g_orig_incr_build = nullptr;
#endif
#if CVR_RUST_VRIK
#else
bool g_enable_build_probe = true;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFullEye = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFullTotal = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugIncrEye = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugIncrTotal = 0;
#endif

// ---- FrameGraph feature-flag diff + force for the VRCAM (RTT) view ----------
// a4 = ctx+6096; a4[0]/a4[1] are the two 64-bit feature-flag words read by the
// builder (sub_1407305B0(a4,N) => word[N/64] bit N%64). ctx+0x28 = view key
// (== the virtualCameraName CName hash for the RTT), ctx+0x44/0x48 = w/h.
// We log every distinct view's flags, capture the richest 16:9 view as the
// "main template", and (when g_rtt_force_flags is toggled on via IPC) OR that
// template into the VRCAM view's flags so the builder emits the SAME passes
// (tonemap / final) that main gets. Guarded so a bad pass can't kill the game.
// VRCAM view key lives in g_vrcam_ctx_key (top of file): it is derived from the selected
// component's virtualCameraName, because there is one component per render resolution.
// Force ON by default: the VRCAM full build is ONE-SHOT + cached, and it runs
// early (before any UI toggle), so the OR must already be armed when that single
// build executes. Use a bootstrap template until the real MAIN view is known;
// then consume its freshly computed flags, including settings being turned OFF.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
std::atomic<bool> g_rtt_force_flags{true};
#endif
// Force VRCAM's feature flags to main's for full quality, but additionally SET
// feature-bit 50 = "reuse shadow cascades" (fg builder sub_141D43040: if
// sub_1407305B0(flags,50) -> SKIP ClearShadowCascades + RenderCascade%u and
// reference the existing atlas). main has bit50 CLEAR (it renders cascades);
// giving VRCAM bit50 makes it SAMPLE main's cascades instead of regenerating
// them into the shared atlas -> no main shadow flicker, VRCAM keeps quality.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
std::atomic<bool> g_force_view_flags{true};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsSceneLinear=1;
#endif
#if !CVR_RUST_VRIK
static const uint64_t SHADOW_CASCADE_REUSE_BIT = (1ULL << 51);  // SET = skip cascade regen (2.31: bit 50)
#endif
// Distant shadows: work-fn sub_1405A93E8 renders them only if flags bit 11
// (0x800) is set. VRCAM naturally lacks it; forcing flags=main gave it bit11 ->
// VRCAM regenerated distant shadows into the shared distant-shadow buffer ->
// out-the-window flicker on main. CLEAR it so VRCAM reuses main's distant shadows.
#if !CVR_RUST_VRIK
static const uint64_t DISTANT_SHADOW_BIT = (1ULL << 11);         // CLEAR = skip distant regen
#endif
// f1 bit 24 (overall bit 88) is the master gate for the async lighting-compute
// block (GI / clustered light grid / reflections). Builder: v19 = f1 & 0x1082850
// gates v21..v26 -> v273 -> AsyncComputeDuringShadowmaps. VRCAM rebuilding those
// view-dependent global structures collides with main -> light/shadow flicker on
// distant (out-the-window) geometry. CLEAR it so VRCAM reuses main's.
#if !CVR_RUST_VRIK
static const uint64_t LIGHTING_COMPUTE_BIT_F1 = (1ULL << 29);    // CLEAR in f1 = reuse main's GI/clusters (overall 93; 2.31: 88)
#endif
// BISECTION: main-has / vrcam-naturally-lacks f0 bits are the suspects that make
// vrcam rebuild view-dependent global lighting structures. Clearing this group
// from the forced flags tests whether the out-the-window light/shadow collision
// lives in the HIGH half {26,31,32,33,34,58}. bits: 26=0x47C3E60 31=0x80000000
// 32=0x1_00000000 33=0x2_00000000 34=0x4_00000000 58=0x400_00000000_0000.
// bit 31 = Global Illumination. Confirmed: CRenderNode_GlobalIllumination::work
// (sub_1407F4130) does `if (sub_14033E358(a2, 31)) { update GI (sub_14077F758) }`.
// main has it (builds GI); VRCAM forcing it -> rebuilds the shared GI buffer from
// its frustum -> main GI (ambient light/shadows) flicker out-the-window. CLEARED
// for VRCAM -> its GI node skips the update and reuses main's GI.
#if !CVR_RUST_VRIK
static const uint64_t GI_FEATURE_BIT = (1ULL << 31);
#endif
// THE SCOPE VIEW'S SCREEN-SPACE PASSES, off whatever MAIN has. FlagCompute sub_14227E5D0 sets bit 15 from the
// ambient occlusion setting (Developer/FeatureToggles/SSAO and the graphics menu's AO) and bit 21 from SSR
// (Developer/FeatureToggles/ScreenSpaceReflection); bit 22, planar SSR, requires 21. The scope view takes MAIN's
// flags, so both ran on a narrow image that the lens then magnifies, noise and all.
#if !CVR_RUST_VRIK
static const uint64_t SCOPE_SCREEN_SPACE_BITS = (1ULL << 15) | (1ULL << 21) | (1ULL << 22);
#endif
// 1 = the scope view renders without AO and SSR (default), 0 = with MAIN's, for A/B.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OpticsNoScreenSpace = 1;
#endif
// THE SCOPE VIEW IS ONE EYE. Under engine stereo MAIN carries render feature 98 (both eyes in one frame) and
// 99, which FlagCompute derives from it; f1 bits 34/35. Taken over with MAIN's f1 they made the engine draw the
// scope view's passes per eye (sub_14033DDA4: `if (feature 98)` eye 1, eye 2), with the eye functor of a view
// that never built eyes -- a call through null at +0x2093FC2, the moment a camera scope came up.
#if !CVR_RUST_VRIK
static const uint64_t SCOPE_STEREO_BITS_F1 = (1ULL << (98 - 64)) | (1ULL << (99 - 64));
#endif
// 2.5 numbering: 2.31's main flags {0x3C00017FAD75FF51, 0x517F008} with every group renumbered
// (FlagCompute 2.31 vs 2.5: groups 0-43 kept, 44-74 +1, 76-86 +3, 87-90 +5; 2.5 adds 44, 76, 78, 90, 91, 96+).
#if !CVR_RUST_VRIK
static constexpr cvr::stereo::FrameGraphFlags kBootstrapMainFlags{0x7800017FAD75FF51ULL,0x00000000A0BF8010ULL};
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/typed_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFgMainF0 = kBootstrapMainFlags.f0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFgMainF1 = kBootstrapMainFlags.f1;
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static cvr::stereo::MainViewFeatureCache g_main_features;

static cvr::stereo::FrameGraphFlags MainFeaturesForVrcam() {
    auto flags=kBootstrapMainFlags;
    g_main_features.Read(g_main_view_ctx.load(std::memory_order_acquire),flags);
    return flags;
}

static void ObserveMainFeatures(uintptr_t context,uintptr_t computed) {
    __try {
        if(context && computed && g_main_features.Observe(context,g_main_view_ctx.load(std::memory_order_acquire),
                *reinterpret_cast<const uint64_t*>(context+0x28),*reinterpret_cast<const cvr::stereo::FrameGraphFlags*>(computed))) {
            const auto flags=MainFeaturesForVrcam();
            CyberpunkVR_DebugFgMainF0=flags.f0;
            CyberpunkVR_DebugFgMainF1=flags.f1;
        }
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
}
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFgRttF0 = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugFgRttF1 = 0;
#endif
//  FRAME-GRAPH UPSCALER SELECTOR (ROOT crop fix) 
// SCENE_FULL (sub_141D43040) picks the temporal upscaler at BUILD time via
// sub_1407305B0(a4,N) = bit(N&0x3F) of a4[N>>6]. Groups 69/71/72/73 = DLSS/FSR2/FSR3/
// XeSS. If NONE are set it emits the NATIVE TAAU/resolve node (sub_1418629D4 ->
// PipelineState_563) that re-does a temporal upscale at render-res -> CROPS the DLSS
// output (1418 viewport on the 2444 target). main has group 69 (DLSS) SET at build ->
// no 563. vrcam does NOT (its native upscaler mode is TAA) -> 563. Our runtime 0x45
// force (in ApplyDLSS) is TOO LATE -- the graph already baked the TAAU branch.
// FIX: set group 69 (bit5, == flag 0x45) and clear 71/72/73 in the BUILD flag bitset
// for vrcam, at flag-compute AND right before the full builder. Gated on VrcamDlss
// ONLY (not VrcamDlssScale): group69 must be set even for DLAA else the builder thinks
// there's no temporal upscaler. Only forced when MAIN itself selected DLSS.
// 2.5: the builder (sub_142276E60) tests 70/72/73/74 and the two new upscaler groups 76/78 (2.31: 69/71/72/73).
#if !CVR_RUST_VRIK
constexpr uint64_t FG_DLSS_BIT_F1      = 1ull << (70 - 64);   // 0x040 (group 70 / DLSS)
#endif
constexpr uint64_t FG_UPSCALER_MASK_F1 =                      // 0x5740
    (1ull << (70 - 64)) | (1ull << (72 - 64)) | (1ull << (73 - 64)) | (1ull << (74 - 64)) |
    (1ull << (76 - 64)) | (1ull << (78 - 64));
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static std::atomic<uint64_t> g_main_upscaler_groups{0};       // MAIN's chosen upscaler groups (f1 & mask)
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMainUpscalerGroups = 0; // diag: main's upscaler bits
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugUpscalerForceHits  = 0; // diag: vrcam forced -> DLSS count
#endif
// fwd (real definitions live further below near the DLSS-for-vrcam block)
#if !CVR_RUST_VRIK
extern "C" __declspec(dllexport) int32_t  CyberpunkVR_VrcamDlss;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamDlssScale;
#endif
// LIVE A/B of the two reuse bits that are the ONLY difference between vrcam and
// main flags (camera + env handles proven identical). Diagnoses which reuse bit
// breaks vrcam lighting (wrong light / triangle shadows / light bleed / no refl).
//   0 = current: cascade-reuse (bit50 SET) + GI-reuse (bit31 CLEAR)
//   1 = EXACT main flags (bit50 CLEAR, bit31 SET) -> vrcam builds own cascades+GI
//       (expect main shadow flicker, but tells us if reuse is what breaks visuals)
//   2 = cascade-reuse only (bit50 SET, bit31 SET=GI native/own)
//   3 = GI-reuse only (bit50 CLEAR=cascade native/own, bit31 CLEAR)
// DEFAULT WAS 1: with the temporal-history fix (StreamlineHistoryFix) giving vrcam a
// real per-view temporal view-state, vrcam building its OWN cascades+GI no longer
// collides with main (main flicker GONE, proven live) and fixes interior shadows/
// sun light that cascade/GI-reuse (mode 0) broke. mode 0 kept for A/B fallback.
//
// NOW 2, AS THE SINGLE-FACTOR TEST FOR THE FOLIAGE-SHADOW MISMATCH (2026-08-17). Mode 1 means each eye
// rasterises the sun cascades itself, into what the shared-state notes say is ONE atlas -- so the second
// view rebuilds a global that the other view also builds. Everything that FEEDS those two builds is now
// measured identical (the cascade record: 0 differing dwords in-frame; casters: 94002 draws against 94004;
// per-index dispatches: casc0/casc1 2410 each with no casc2/3 for either; wind; the whole 480-byte frame
// block, mirrored). What was never verified is the OUTPUT -- and the only way an identical-input rebuild
// can still differ is a race between one view's rebuild and the other's sampling, which lands hardest on
// alpha-tested foliage. Bit 50 removes the second rebuild entirely, so this tests exactly that and costs
// GPU time rather than spending it.
//
// Mode 2 rather than mode 0 deliberately: mode 0 is cascade reuse AND GI reuse, and the note above records
// that the pair broke interior shadows and sun light without saying which half did it. Mode 2 is cascade
// reuse alone, with GI staying native at the flag level (its reuse is a separate node-level hook,
// CyberpunkVR_GiReuseMode).
//
// TWO PREDICTED FAILURE MODES, written down so the result can be read rather than guessed at:
//   * interior shadows / sun light break in the second eye again -> then it was the cascade half of mode 0,
//     and reuse is not available at all;
//   * the second view's shadows SLIDE WITH MOVEMENT -> its graph runs first, so it would sample an atlas
//     rasterised with the PREVIOUS frame's cascade placement while using this frame's matrix. At walking
//     pace that is ~4 cm of world offset per frame.
// Either of those is a reason to go back to 1, and neither is a reason to call the test wasted.
//
// AND IT WAS THE SECOND ONE, measured: MAIN fine, shadows swimming in the left eye with many artefacts. The left
// eye is the second view, so reuse in THIS DIRECTION is structurally wrong -- not because sharing an atlas
// is wrong, but because the view that reuses runs FIRST and therefore samples content from the previous
// frame with the current frame's matrix. Back to 1.
//
// THE DIRECTION IS THE WHOLE POINT, and it is fixable: the second view runs first, so IT should be the
// builder and MAIN the reuser. Then the atlas is built and sampled inside one frame, against a record the
// in-frame probe already showed to be identical at both passes -- no staleness at all. That cannot be done
// from here (this path forces the second view's flags, not MAIN's); it belongs in the cascade node detour,
// where the node can simply be skipped for MAIN. See CyberpunkVR_CascadeSkipMain in ViewReuse.cpp.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamFlagMode = 1;
// VRCAM final-color EXTRACTION fix: the full scene builder gates ExtractionSceneColor
// (sub_1428E5748) and ExtractionFinalColor (sub_1408EAFCC) on build-bit 64
// (sub_1407305B0(a4,64) == f1 bit 0). CopyToTexture (unconditional) READS the final-
// color 0x3D7E6258 -> for main it's written by a separate final builder, but VRCAM
// only runs the scene builder where bit 64 is 0 -> extraction skipped -> VRCAM copies
// an unwritten (black) final-color. Set bit 64 in VRCAM's computed flags so the scene
// builder emits the extraction passes for VRCAM too. Default OFF -> toggle live.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamExtractionFix = 1;   // proven live-safe: adds ExtractionFinalColor
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamExtractionHits = 0;
// VRCAM final-color GROUP: the scene builder only adds ExtractionFinalColor (bit 64),
// NOT ClearFinalColorTarget / DeclareCommonResourceAllocs_FinalOnly (those live only in
// the separate blank/final builder that VRCAM never runs). Hook the Extraction ADDER
// (sub_1408EAFCC) and, when it is called from the SCENE builder (== VRCAM, since only
// VRCAM has bit 64 set there), inject the Declare + Clear adders FIRST so the graph gets
// Declare -> Clear -> Extraction, matching the engine's blank-builder order. Return-addr
// range-gated to the full/incremental scene builders so MAIN's blank builder (which
// already adds all three) is never double-fed. Default OFF (Declare_FinalOnly may
// re-declare final-color) -> enable + verify live.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamFinalGroup = 0;         // OFF: ClearFinalColorTarget literally clears 0x3D7E6258 (the RenderFinal2D output we capture) to the bg color -> wipes the vrcam frame to black. RenderFinal2D already draws the full frame; no clear needed.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamFinalGroupHits = 0;
#endif
// VRCAM composition GROUP: DrawComposition / CompositionPostProcess / FullscreenVideo
// live in SCENE_FULL under build-bit 82 and in the final builder sub_140982C7C; VRCAM
// runs SCENE_INCR (sub_14227BD10) which has NEITHER -> missing. RenderFinal2D is added
// by name. Inject all of them via the same Extraction-adder hook (fires for VRCAM with
// the builder ctx). NOTE: CopyToTexture copies final-color 0x3D7E6258 (pre-composition);
// composition writes 0x31CF52F9 -> adding it does NOT change what the mirror copies. It
// makes VRCAM's graph node-complete. Default OFF -> enable + verify live.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamCompositionGroup = 0;   // default OFF: composition inputs absent in VRCAM RTT graph -> downstream fault
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugVrcamCompositionHits = 0;
#endif
// VRCAM actually builds via the RTT builder sub_141D47FF0 (live: FullEye/IncrEye=0 yet
// Extraction present). MAIN never enters it (main uses SCENE_FULL + final sub_140982C7C),
// so injecting when the Extraction adder is called from here is VRCAM-only.
#if !CVR_RUST_VRIK
using PassAdderFn = __int64 (__fastcall*)(__int64, __int64, __int64, int);
using NamedPassFn = __int64 (__fastcall*)(unsigned int, __int64, __int64, __int64, const char*, int);
#endif
#if !CVR_RUST_VRIK
static PassAdderFn g_orig_extraction_adder = nullptr;
static PassAdderFn g_declare_adder = nullptr;
static PassAdderFn g_clear_adder = nullptr;
static PassAdderFn g_drawcomp_adder = nullptr;
static PassAdderFn g_composition_adder = nullptr;
static PassAdderFn g_fsvideo_adder = nullptr;
static NamedPassFn g_add_named_pass = nullptr;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static uint64_t g_fg_logged[32];
static int      g_fg_logged_n = 0;
static bool g_fg_vrcam_full_logged = false;
static bool g_fg_vrcam_incr_logged = false;
static uint64_t SupportedHardwareFlags(uint64_t flags) {
    // 2.31 PrepareShadingRateImage (+76FCAC) divides its texture dimensions by
    // this native tile size. The seeded NVIDIA flags must not enable it on a
    // device whose tile size is zero (issue #93, RX 5700 XT dump).
    __try {
        const auto* pool=*reinterpret_cast<const uint8_t* const*>(g_exe_base+0x3BCDCA8);
        if(pool) {
            const auto filtered=cvr::stereo::SupportedShadingRateFlags(flags,pool[0xB]);
            if(filtered!=flags) {
                static std::atomic<bool> logged{};
                if(!logged.exchange(true))log("[fgflags] VRCAM shading-rate image disabled: native tile size is zero");
            }
            return filtered;
        }
    } __except(EXCEPTION_EXECUTE_HANDLER) {}
    return flags;
}
// Defined with the HUD identification state it resets, far below. See the call site.
// hud_rearm_for_new_graph moved with the HUD; declared in Stereo/StereoInternal.hpp.
static void fg_observe(__int64 a4, const char* which) {
    if (!a4) return;
    __try {
        uint8_t* ctx = reinterpret_cast<uint8_t*>(a4) - 6096;
        uint64_t key = *reinterpret_cast<uint64_t*>(ctx + 0x28);
        uint32_t w   = *reinterpret_cast<uint32_t*>(ctx + 0x44);
        uint32_t h   = *reinterpret_cast<uint32_t*>(ctx + 0x48);
        uint64_t f0  = *reinterpret_cast<uint64_t*>(a4);
        uint64_t f1  = *reinterpret_cast<uint64_t*>(a4 + 8);
        bool seen = false;
        for (int i = 0; i < g_fg_logged_n; ++i) if (g_fg_logged[i] == key) { seen = true; break; }
        if (!seen && g_fg_logged_n < 32) {
            g_fg_logged[g_fg_logged_n++] = key;
            log("[fgflags] %s key=%016llX %ux%u f0=%016llX f1=%016llX%s",
                which, (unsigned long long)key, w, h,
                (unsigned long long)f0, (unsigned long long)f1,
                key == g_vrcam_ctx_key ? " <-VRCAM" : "");
            // A FULL BUILD UNDER A KEY WE HAVE NEVER SEEN IS A NEW GRAPH, AND OUR HUD
            // IDENTIFICATION DOES NOT SURVIVE ONE.
            //
            // Opening the map or the inventory rebuilds the frame graph, and it does not come back
            // the way it left. A tester's session: key 0000000000000000 at 2560x2560 for twenty
            // minutes, then D512F33B8A4F15C9 at 1280x1280 when the map opened, then
            // 9947B0B7A7CD0843 at 2848x2848 on the way back -- a third key at a third size. One
            // line before that first rebuild the composite lost all five of its inputs at once
            // ("waiting on: surface blur-pyramid exposure frame-constants composite-constants")
            // and never regained them, because identification had latched onto a node from the old
            // graph and switched descriptor matching off behind itself. Nothing left to find it
            // with. Changing a graphics setting brought it back, which is the same thing from the
            // other direction: yet another rebuild, and that one happened to re-name the node.
            //
        }
        // RE-ARM ON EVERY TRANSITION, NOT ON FIRST SIGHTING.
        //
        // The log above only prints keys it has never seen, and the first version of this hung the
        // re-arm off that -- which would have fixed the first map opening of a session and no
        // other, while the report is explicitly that it keeps happening. What matters is the
        // CHANGE: the full builder ran under a different key than last time, so whatever the HUD
        // path is holding belongs to the previous one.
        //
        // Debounced, because two graphs alternating frame to frame would otherwise re-arm forever
        // and never let an identification settle.
        if (which[0] == 'f' && key!=cvr::cname_hash("vr_optics_feed")) {
            static uint64_t s_lastFullKey = 0;
            static bool     s_haveLastFullKey = false;
            static uint64_t s_lastRearmMs = 0;
            if (!s_haveLastFullKey) {
                s_haveLastFullKey = true;
                s_lastFullKey = key;
            } else if (key != s_lastFullKey) {
                s_lastFullKey = key;
                const uint64_t nowMs = GetTickCount64();
                if (nowMs - s_lastRearmMs >= 250) {
                    s_lastRearmMs = nowMs;
                    hud_rearm_for_new_graph(key);
                }
            }
        }
        // Which builder processes the VRCAM view? (full has tonemap/EndRender;
        // if VRCAM only ever shows up via incr/FinalOnly it never tonemaps.)
        if (key == g_vrcam_ctx_key) {
            if (which[0] == 'f' && !g_fg_vrcam_full_logged) {
                g_fg_vrcam_full_logged = true;
                log("[fgflags] VRCAM built via FULL (sub_141D43040) f0=%016llX f1=%016llX",
                    (unsigned long long)f0, (unsigned long long)f1);
            } else if (which[0] == 'i' && !g_fg_vrcam_incr_logged) {
                g_fg_vrcam_incr_logged = true;
                log("[fgflags] VRCAM built via INCR (sub_14227BD10) f0=%016llX f1=%016llX",
                    (unsigned long long)f0, (unsigned long long)f1);
            }
        }
        // Earliest point we hold the second view's ctx: grant the HUD capability here, before
        // the graph is built and before anything downstream reads it.
        if (key == g_vrcam_ctx_key) hud_grant_capability(reinterpret_cast<uintptr_t>(ctx));
        const bool optics=key==cvr::cname_hash("vr_optics_feed");
        if (key == g_vrcam_ctx_key || optics) {
            const auto main=MainFeaturesForVrcam();
            if (g_rtt_force_flags.load(std::memory_order_relaxed) &&
                (main.f0 | main.f1)) {
                uint64_t n0 = f0 | main.f0;
                uint64_t n1 = SupportedHardwareFlags(f1 | main.f1);
                if(optics) {
                    n0=cvr::stereo::MergeViewFlags(n0|SHADOW_CASCADE_REUSE_BIT,f0,true);
                    n1=(n1&~FG_UPSCALER_MASK_F1)|(f1&FG_UPSCALER_MASK_F1);
                    n1&=~SCOPE_STEREO_BITS_F1;
                }
                *reinterpret_cast<uint64_t*>(a4)     = n0;   // actually applied
                *reinterpret_cast<uint64_t*>(a4 + 8) = n1;
                if(!optics) {
                    CyberpunkVR_DebugFgRttF0 = n0;
                    CyberpunkVR_DebugFgRttF1 = n1;
                }
                static uint64_t s_forced0 = 0;
                if (!optics && s_forced0 != n0) {             // log once per change
                    s_forced0 = n0;
                    log("[fgflags] FORCED vrcam -> f0=%016llX f1=%016llX (was %016llX/%016llX)",
                        (unsigned long long)n0, (unsigned long long)n1,
                        (unsigned long long)f0, (unsigned long long)f1);
                }
            } else if(!optics) {
                CyberpunkVR_DebugFgRttF0 = f0;
                CyberpunkVR_DebugFgRttF1 = f1;
            }
            // UPSCALER SELECTOR (guarantee, runs right before the FULL builder reads a4
            // for `if(!69 && !71 && !72 && !73)`): force group 69 (DLSS), clear 71/72/73.
            // See FG_UPSCALER block. Gated on VrcamDlss; only when MAIN chose DLSS.
            if (!optics && CyberpunkVR_VrcamDlss &&
                (g_main_upscaler_groups.load(std::memory_order_acquire) & FG_DLSS_BIT_F1)) {
                uint64_t* fr = reinterpret_cast<uint64_t*>(a4);
                uint64_t nf1 = (fr[1] & ~FG_UPSCALER_MASK_F1) | FG_DLSS_BIT_F1;
                if (nf1 != fr[1]) {
                    fr[1] = nf1;
                    CyberpunkVR_DebugFgRttF1 = nf1;
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugUpscalerForceHits);
                }
            }
        } else if (key==0 && reinterpret_cast<uintptr_t>(ctx)==g_main_view_ctx.load(std::memory_order_acquire)) {
            // Capture MAIN's chosen upscaler groups (reliable primary is FlagCompute
            // key==0; this main-ish path is a backup). Only latch when DLSS is present
            // so a transient pre-DLSS frame can't clear it.
            if ((f1 & FG_DLSS_BIT_F1) != 0) {
                g_main_upscaler_groups.store(f1 & FG_UPSCALER_MASK_F1, std::memory_order_release);
                cvr::sharpen::gameUpscaler.SelectDlss((f1 & FG_DLSS_BIT_F1)!=0);
                CyberpunkVR_DebugMainUpscalerGroups = f1 & FG_UPSCALER_MASK_F1;
            }
        }
        if(optics && CyberpunkVR_OpticsSceneLinear)
            *reinterpret_cast<uint64_t*>(a4)&=~3ull;
        if(optics && CyberpunkVR_OpticsNoScreenSpace)
            *reinterpret_cast<uint64_t*>(a4)&=~SCOPE_SCREEN_SPACE_BITS;
        if(optics)
            *reinterpret_cast<uint64_t*>(a4+8)&=~SCOPE_STEREO_BITS_F1;
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}

// Attribute a graph-build duration (ticks) to main vs vrcam via the view ctx.
static void prof_add_build(__int64 a4, int64_t t0, int64_t t1) {
    const int64_t dt = t1 - t0;
    bool vrcam = false;
    if (a4) {
        __try {
            uint8_t* ctx = reinterpret_cast<uint8_t*>(a4) - 6096;
            vrcam = (*reinterpret_cast<uint64_t*>(ctx + 0x28) == g_vrcam_ctx_key);
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (CyberpunkVR_NodeTimeline) prof_timeline_add(t0, t1, 0, vrcam ? 5u : 4u);
    if (!CyberpunkVR_ProfEnable) return;
    if (vrcam) {
        g_prof_build_vrcam_ticks.fetch_add(dt, std::memory_order_relaxed);
        g_prof_build_vrcam_calls.fetch_add(1, std::memory_order_relaxed);
    } else {
        g_prof_build_main_ticks.fetch_add(dt, std::memory_order_relaxed);
        g_prof_build_main_calls.fetch_add(1, std::memory_order_relaxed);
    }
}

static __int64 __fastcall Detour_FullBuild(__int64 a1, __int64 a2, __int64 a3,
                                           __int64 a4, __int64 a5) {
    CVR_DIAGNOSTIC(CyberpunkVR_DebugFullTotal++);
    fg_observe(a4, "full");   // DLSS upscaler-group capture/force backup for the crop fix
    if (CyberpunkVR_ProfEnable || CyberpunkVR_NodeTimeline) {
        const int64_t t0 = prof_now();
        const __int64 r = g_orig_full_build(a1, a2, a3, a4, a5);
        prof_add_build(a4, t0, prof_now());
        return r;
    }
    return g_orig_full_build(a1, a2, a3, a4, a5);
}

static __int64 __fastcall Detour_IncrBuild(__int64 a1, __int64 a2, __int64 a3,
                                           __int64 a4, __int64 a5) {
    CVR_DIAGNOSTIC(CyberpunkVR_DebugIncrTotal++);
    fg_observe(a4, "incr");   // DLSS upscaler-group capture/force backup for the crop fix
    if (CyberpunkVR_ProfEnable || CyberpunkVR_NodeTimeline) {
        const int64_t t0 = prof_now();
        const __int64 r = g_orig_incr_build(a1, a2, a3, a4, a5);
        prof_add_build(a4, t0, prof_now());
        return r;
    }
    return g_orig_incr_build(a1, a2, a3, a4, a5);
}
#endif

// --- per-frame feature-flag WRITER hook (sub_141D49540) ----------------------
// The view's render-feature flags (viewobj+0x17D0 == ctx+6096) are RE-DERIVED
// EVERY frame per-view by sub_141D49540, which returns a pointer to the 16-byte
// flag block that its caller then copies into viewobj+0x17D0:
//     call sub_141D49540 ; movups xmm0,[rax] ; movdqu [rbx+17D0h],xmm0
// This per-frame rewrite is exactly what wiped the flags we forced at the (cached)
// one-shot build. Hook it and, for the VRCAM view (key @ viewobj+0x28), OR the
// captured main-template flags into the freshly computed result on EVERY call ->
// the forced flags now persist through both the build and per-frame node execution
// (the tonemap/bloom/exposure work-fns read viewobj+0x17D0 at record time). If
// these flags gate the reduced passes, they now light up; if not, this proves it.
// a1=renderer a2=outFlags a3=view context a4=view-state; return=outFlags.
#if !CVR_RUST_VRIK
using FlagComputeFn = __int64(__fastcall*)(void*, __int64, __int64, __int64);
#endif
#if !CVR_RUST_VRIK
FlagComputeFn g_orig_flag_compute = nullptr;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttFlagForceHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttEnvBindHits = 0;
#endif
// LIVE A/B control for distant-shadow reuse (write via x64dbg to isolate distant
// in VRCAM):  0 = distant OFF for vrcam (bit 11 cleared -> node no-ops, no distant
// shadows).  1 = distant REUSE (bit 11 kept SET + vrcam's distant work skipped ->
// vrcam samples main's distant map).  Flip 1->0->1 and watch vrcam's far-field
// (out-the-window) sun shadows disappear/reappear = proof distant reuse is live.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DistantReuseMode = 1;
// Force VRCAM camera to match MAIN instead of relying on the RTT-camera asset.
// Camera params live at ctx+0x70 (a3-view-params). We copy PROJECTION from main
// (lens/fov/aspect/near/far at a3+0x10..0x48 = ctx+0x80..0xB8, and the inv/fwd
// proj matrices at a3+0x150..0x1D0 = ctx+0x1C0..0x240) but KEEP vrcam's own view
// matrix/position (a3+0x50..0x140 = ctx+0xC0..0x1B0) so steer/IPD still apply.
// Resolution: force vrcam render dims (ctx+0x44/0x48) + rect to main's W/H.
// (LOD/culling in this engine is screen-space-error driven -> forcing resolution
// + fov to main makes VRCAM's LOD selection match main automatically.)
// NOTE: ForceVrcamRes via ctx dims is INVALID  the RTT view is created with dims
// == its RTT texture (2444^2); forcing ctx+0x44/0x48 to a different W/H makes the
// view fail validation at init -> VRCAM never renders (absent from Nsight). To
// change VRCAM resolution, resize the RTT TEXTURE (dynamicTextureRes asset) so the
// dims derive correctly. ForceVrcamCam copies MAIN's camera SCALARS (fov/zoom/near/
// far) into the vrcam view-ctx so both eyes match under gameplay fov + weapon ZOOM.
// Live-confirmed ctx layout: +0x90 fovV, +0x9C zoom, +0xB0 nearZ, +0xB4 farZ. It does
// NOT touch orientation (+0x80..0x8C) or aspect (+0x98, vrcam
// keeps its own for its square RTT dims) and copies NO proj matrix. Default ON.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ForceVrcamCam = 1;   // vrcam fov/zoom/near/far = main

// ---- WHICH CAMERA FIELDS THE SECOND VIEW TAKES FROM MAIN -------------------------------------
//
// ForceVrcamCam above copies MAIN's fov/zoom/near/far onto the second view's camera context, and
// the note beside it records what that context is for: writing it steers CULLING only. What the
// second view actually RENDERS with is its component's own fov at comp+0x128, set further down.
// Two sources, kept in step by the fact that they normally agree.
//
// They stop agreeing the moment a scripted beat drives MAIN's camera, and then the copy is
// actively harmful, because one of the things it steers is the CLUSTERED LIGHT GRID:
//
//     ClusteredLightsCull   bins every light into a froxel grid over the frustum -- and the
//                           frustum it uses is the one in the view context, i.e. MAIN's
//     RenderLightsIntegrate maps each pixel to a froxel -- using the frustum the second view is
//                           actually rendered with, i.e. its own
//
// The grid is then filled for one frustum and read with another, every pixel lands in a bin that
// was never filled, and the analytic light contribution disappears while emissive, sky and
// ambient carry on -- which is the reported defect exactly.
//
// How this was pinned, in order. Skipping ClusteredLightsCull for the second view reproduces the
// picture one for one, while skipping RenderLightBuffers gives something else entirely. Every
// other account was measured away: only two views run that cull (by camera-name hash), the
// grid-write gate holds for both in the same 2:1 ratio over thousands of calls, the capability
// gate passes for both, the light list is ONE shared object with the same count for both, and the
// capture's submission order has MAIN's own cull running after the second view has already
// sampled the grid, so nothing overwrites it. A mid-frame race was the next guess and is dead
// too: sampling the four scalars at the cull and again at the integrate gave ZERO drift over
// 1929 pairs while the defect was on screen. What did fix it, live, was switching the copy off.
//
// So the copy is the cause and the VALUE is the problem, not its timing. Bisected on the picture
// field by field: NEAR is the one that does it, which is what the shape of the symptom already
// suggested -- the grid's depth axis is sliced logarithmically between near and far, so a near that
// belongs to another camera moves every slice boundary at once and every pixel reads a bin nobody
// filled. A wrong fov would have put the light in the wrong PLACE; a wrong near removes it.
//
// Shipped default is fov + zoom, which is what this mechanism exists for -- matching MAIN's field
// of view and its ADS zoom so culling and LOD agree with the flat view. The depth range is not
// MAIN's to lend: the second view renders with its own, so it must cull with its own too. Dropping
// far alongside near for the same reason, rather than keeping a half-borrowed frustum that happens
// not to break today.
//
// One bit per field, live, so the bisection stays repeatable instead of becoming a number to trust:
//
//     bit 0 = fov      bit 1 = zoom      bit 2 = near      bit 3 = far
//
// F is the old behaviour, 3 is what ships.
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_VrcamCamFields = 3;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_ForceVrcamRes = 0;   // DISABLED (breaks RTT view)
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugForceCamHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugForceResHits = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainW = 0;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMainH = 0;
#endif

// Engine handle copy-assign: sub_1407CDAE4(dst, src) -> AddRef(src), Release(old
// dst), dst = src. This is how the per-view setup (sub_14036F7D4) binds the world
// environment handle into MAIN's view ctx. RTT views never get it (fields stay 0)
// -> no exposure/tonemap/bloom. Using the engine's own refcounted assign (not a
// raw pointer copy) keeps the env alive -> no use-after-free / -1 deref crash.
#if !CVR_RUST_VRIK
using HandleAssignFn = void*(__fastcall*)(void* dst, void* src);
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/stereo_shared.rs.
#else
HandleAssignFn g_handle_assign = nullptr;
#endif

// Live MAIN view ctx (key==0): source of the environment handles to mirror.
#if CVR_RUST_VRIK
#else
static uintptr_t g_main_ctx = 0;
static uintptr_t main_ctx_now() { return g_main_ctx; }
#endif
// Which named render-mask categories does each view actually hold? One line, both views,
// every category -- so the next "the second eye is missing X" question is answered by reading
// the log instead of by another session of bisecting nodes.
#if CVR_RUST_VRIK
// The report and its table are Rust's: rust/crates/cvr-render/src/render_mask_report.rs.
#else
 void render_mask_report() {
    if(!cvr::RuntimeDiagnosticsEnabled())return;
    static uint64_t s_last = 0;
    const uint64_t now = GetTickCount64();
    if (s_last && now - s_last < 20000) return;
    const uintptr_t mc = main_ctx_now();
    const uintptr_t vc = g_vrcam_ctx_seen.load(std::memory_order_acquire);
    if (!mc || !vc || !g_exe_base) return;
    s_last = now;
    const uintptr_t base = reinterpret_cast<uintptr_t>(g_exe_base);
    char line[1400];
    int used = 0;
    line[0] = 0;
    for (uint32_t k = 0; k < kRenderMaskCount; ++k) {
        bool m = true, v = true;
        __try {
            const uint64_t* need =
                reinterpret_cast<const uint64_t*>(base + kRenderMasks[k].desc_rva) + 1;
            const uint64_t* hm = reinterpret_cast<const uint64_t*>(mc + 0x2688);
            const uint64_t* hv = reinterpret_cast<const uint64_t*>(vc + 0x2688);
            for (int i = 0; i < 32; ++i) {
                if ((hm[i] & need[i]) != need[i]) m = false;
                if ((hv[i] & need[i]) != need[i]) v = false;
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { return; }
        if (used < static_cast<int>(sizeof(line)) - 48)
            used += snprintf(line + used, sizeof(line) - used, "%s=%c%c ",
                             kRenderMasks[k].name, m ? 'M' : '-', v ? 'V' : '-');
    }
    log("[rmask] per-view render-mask categories (M = main has it, V = vrcam has it): %s", line);
}
#endif

// The 3 environment handle slots (16 bytes each) that MAIN fills and VRCAM leaves
// zero (found by live diff). Assigned via the engine's refcounted handle-assign.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static const uint32_t kEnvHandleOffs[] = { 0x2490, 0x2B18, 0x2FD8 };
#endif

// Additional environment slots, for the scanner's colour grade.
//
// `base\gameplay\focus_mode.envparam` is what tints the screen green: its
// renderAreaSettings/areaParameters[0]/Data carries
//     ldrLut = base\weather\24h_basic\luts\cp2077_scanning_v0001.xbm
//     hdrLut = base\weather\24h_basic\luts\hdri\cp2077_scanning_hdr_acess_v0001.xbm
// (a mod that blanks exactly those two paths removes the tint, which is how this was pinned).
// So the tint is an ENVIRONMENT AREA OVERRIDE pushed onto the player's view, not a render flag
// and not a shader parameter -- which is why every gate, mask, feature bit and constant checked
// so far came back identical between the views.
//
// The three slots above were found as MAIN-set/VRCAM-zero. The diff only ever printed that case,
// so slots where the second view holds its OWN different handle were invisible until
// CyberpunkVR_ViewDataDiff=2. With all differing ranges printed, the graph context shows nine
// more 8-byte fields laid out as three elements of stride 0x3A0 with three pointers each --
// exactly the shape of a blended area-params list holding hdrLut/ldrLut and friends.
//
// One bit per offset so they can be bisected live. Default 0: these are refcounted handles and a
// wrong guess here can take the process down, so they get turned on deliberately, not by default.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static const uint32_t kEnvExtraOffs[] = {
    0x1F0, 0x220, 0x380,      // element 0
    0x590, 0x5C0, 0x720,      // element 1  (+0x3A0)
    0x930, 0x960, 0xAC0,      // element 2  (+0x740)
};
static const uint32_t kEnvExtraCount =
    static_cast<uint32_t>(sizeof(kEnvExtraOffs) / sizeof(kEnvExtraOffs[0]));
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_EnvExtraMask = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugEnvExtraBinds = 0;
#endif

// ---- the water description --------------------------------------------------------------------
//
// MEASURED 2026-09-05, four dumps of THIS object (MAIN and VRCAM, above water and in it), each one
// identified by the camera-name key at +0x28 at the moment of the read. Of 1152 qwords exactly
// seven carry the signature "MAIN moved when the player entered the water, VRCAM did not move at
// all", and all seven sit inside one 96-byte block the second view never receives:
//
//                        MAIN dry    MAIN in water   VRCAM (never changes)
//     +0x1800             -2.9131          1.0400            -1.0000
//     +0x1804             29.0584         29.0584        -10000.0000
//     +0x1808..0x181C     0.75, -10, -10, 5, 1           uninitialised
//     +0x1820..0x184C     rotation about Z, 135.7 deg         identity
//     +0x1850..0x185C     world sample point                   (0,0,0)
//
// +0x1800 IS THE DEPTH, and that is derived, not guessed: level(+0x1804) minus the camera's Z
// (+0x178) reproduces it to a thousandth in both frames -- 29.0584 - 31.9716 = -2.9132 with the eye
// above the surface, 29.0584 - 28.0202 = +1.0382 with it under. Positive means submerged, so the
// underwater treatment keys off a number the second view holds pinned at -1: permanently dry.
//
// +0x1850 follows MAIN's camera a couple of metres behind it -- it is the point at which the view
// asks the world for its environment. The second view's is the world ORIGIN, which is why it never
// finds the water volume at all, and why its level reads the "no water here" sentinel -10000.
//
// PURE NUMBERS. Not one refcounted handle in the range, which is exactly why this is copied with a
// store rather than through the engine's handle-assign like the three slots above: the handle route
// is the one that froze the mirror when it was guessed at (see kEnvExtraOffs, still defaulted off).
//
// One bit per group so a wrong one can be dropped live without losing the others.
//
// MEASURED NEGATIVE, 2026-09-05, and the default is 0 because of it. With all four groups on, the
// counters showed the copy running -- 45913 qwords written, climbing every frame -- and the second
// view stayed sharp underwater. Bit 0 in isolation keeps writing with the guard silent, so the
// DEPTH and the water level did reach the second view and the blur still did not follow: whatever
// consumes this block is not what draws the underwater treatment, or it is not the only input.
//
// Bit 1 is the one that trips the address guard (skips froze under mask 1 and resumed under mask 2).
// That is the guard working, not a bug: the second view's +0x1808..+0x181C holds uninitialised
// bytes, and some of them read as user-space addresses, so the copy refuses to overwrite them.
// Kept in the tree, defaulted off, so the next attempt starts from the measurement instead of
// rediscovering it.
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static const struct { uint32_t off, len; } kWaterMirror[] = {
    { 0x25E0, 0x08 },   // bit 0: depth under the surface, and the surface's height
    { 0x25E8, 0x18 },   // bit 1: the volume's own constants (0.75 / -10 / -10 / 5 / 1)
    { 0x2600, 0x30 },   // bit 2: the water plane's rotation
    { 0x2630, 0x10 },   // bit 3: the world point the environment is sampled at
};
static const uint32_t kWaterCount =
    static_cast<uint32_t>(sizeof(kWaterMirror) / sizeof(kWaterMirror[0]));
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_WaterMirror = 0;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWaterMirrors = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWaterPtrSkips = 0;
#endif

// The second view's own view object, captured where it is unambiguous -- inside FlagCompute, off the
// camera-name key. `g_vrcam_ctx_seen` says the same thing but is written from the node dispatcher,
// and the assignment hook below has to be certain which object it is refusing to feed.
#if CVR_RUST_VRIK
#else
static std::atomic<uintptr_t> g_vrcam_view{0};
static std::atomic<uintptr_t> g_optics_view{0};
static uintptr_t vrcam_view_now() { return g_vrcam_view.load(std::memory_order_acquire); }
#endif

#if !CVR_RUST_VRIK   // unused by the Rust build
bool PhysicalOpticsStereoReady() {
    // The auxiliary camera may only start after the right eye has produced a
    // fresh frame. An XR session alone is already active on the loading screen.
    // No resource is acquired or modified by this game-thread readiness query.
    // Engine stereo has no second view to wait for: both eyes are MAIN's own frame. The camera
    // starts once the engine renders the eyes -- not before the main menu, not on a loading screen.
    if(cvr::engine_stereo::Enabled())
        return cvr::engine_stereo::Rendering() && !cvr::engine_stereo::OnePicture();
    const auto tick=g_stable_tick.load(std::memory_order_acquire);
    const auto now=GetTickCount64();
    if(!tick || now<tick || now-tick>500)return false;
    const auto main=g_main_view_ctx.load(std::memory_order_acquire);
    const auto right=vrcam_view_now();
    const auto key=g_vrcam_ctx_key.load(std::memory_order_acquire);
    if(!main || !right || main==right || !key)return false;
    __try {
        return *reinterpret_cast<const uint64_t*>(main+0x28)==0 &&
            *reinterpret_cast<const uint64_t*>(right+0x28)==key;
    } __except(EXCEPTION_EXECUTE_HANDLER) {return false;}
}
#endif
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static const uint32_t kWaterOff = 0x25E0;

// Eight bytes at a time, refusing any pair that reads as a user-space address on EITHER side:
// MAIN's pointer stored here would be a dereference waiting to happen, and MAIN's float written
// over a pointer this view still owns is the same crash from the other direction. The block
// measured as floats only, so the guard should never fire -- a DebugWaterPtrSkips that climbs
// means the layout moved and the range above is wrong.
static void water_mirror_apply(uintptr_t view) {
    const uint32_t wm = CyberpunkVR_WaterMirror;
    if (!wm || !view || !g_main_ctx || view == g_main_ctx) return;
    __try {
        for (uint32_t k = 0; k < kWaterCount; ++k) {
            if (!(wm & (1u << k))) continue;
            const uint32_t off = kWaterMirror[k].off;
            const uint32_t len = kWaterMirror[k].len;
            for (uint32_t i = 0; i + 8 <= len; i += 8) {
                const uint64_t sv = *reinterpret_cast<uint64_t*>(g_main_ctx + off + i);
                const uint64_t dv = *reinterpret_cast<uint64_t*>(view + off + i);
                if ((sv >= 0x10000000000ull && sv < 0x7FFFFFFFFFFFull) ||
                    (dv >= 0x10000000000ull && dv < 0x7FFFFFFFFFFFull)) {
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugWaterPtrSkips);
                    continue;
                }
                if (sv != dv) {
                    *reinterpret_cast<uint64_t*>(view + off + i) = sv;
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugWaterMirrors);
                }
            }
        }
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
// Resolve the render texture-manager: texMgr = *(*(exe+0x3BBCAE8)+0x70).
static void* resolve_texmgr() {
    __try {
        uintptr_t renderer = *reinterpret_cast<uintptr_t*>(g_exe_base + RVA_RENDERER_GLOBAL);
        if (!renderer) return nullptr;
        return *reinterpret_cast<void**>(renderer + 0x70);
    } __except (EXCEPTION_EXECUTE_HANDLER) { return nullptr; }
}

// Resize the RTT output DynamicTexture (*(comp+0x1E8)) to the target so the vrcam
// render matches. Target = explicit RttResizeW/H, else MAIN dims (g_main_ctx+0x44/
// +0x4C). No-op when already correct. Marshaled/thread-safe. SEH-guarded.
static void maybe_resize_rtt(uintptr_t comp) {
    if (!CyberpunkVR_RttResizeMatchMain || !comp) return;
    __try {
        void* dtex = *reinterpret_cast<void**>(comp + 0x1E8);
        if (!dtex) return;
        uint8_t* d = reinterpret_cast<uint8_t*>(dtex);
        uint32_t cw = *reinterpret_cast<uint32_t*>(d + 0x40);
        uint32_t ch = *reinterpret_cast<uint32_t*>(d + 0x44);
        CyberpunkVR_DebugRttDtexW = cw;
        CyberpunkVR_DebugRttDtexH = ch;
        uint32_t tw = CyberpunkVR_RttResizeW, th = CyberpunkVR_RttResizeH;
        if (!tw || !th) {                 // no explicit target -> match MAIN
            const uintptr_t main = main_ctx_now();
            if (!main) return;
            tw = *reinterpret_cast<uint32_t*>(main + 0x44);
            th = *reinterpret_cast<uint32_t*>(main + 0x4C);
        }
        if (!tw || !th || tw > 8192 || th > 8192) return;
        if (cw == tw && ch == th) return; // already correct
        if (!g_resize_dyntex)
            g_resize_dyntex = reinterpret_cast<ResizeDynTexFn>(g_exe_base + RESIZE_DYNTEX_RVA);
        void* texMgr = resolve_texmgr();
        if (!texMgr) return;
        void* holder = dtex;              // proven-safe pattern: &localHolder
        g_resize_dyntex(texMgr, &holder, tw, th, 1);
        // Align the OUTPUT texture's own CPU dims + disable scaleToViewport so the
        // FINAL output resource follows the target too (not just the render RTs).
        *reinterpret_cast<uint32_t*>(d + 0x40) = tw;   // width
        *reinterpret_cast<uint32_t*>(d + 0x44) = th;   // height
        *reinterpret_cast<uint32_t*>(d + 0x48) = 0;    // scaleToViewport off
        CVR_DIAGNOSTIC(++CyberpunkVR_DebugRttResizeHits);
    } __except (EXCEPTION_EXECUTE_HANDLER) {}
}
#endif

// Bridges the FlagCompute hook to the RectCompute hook within a single per-view
// setup pass (both run sequentially on the same thread inside sub_1404E4xxx).
#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static thread_local PendingVrcamViewport t_vrcam_viewport{};
#endif
// Same numbers, readable from the RECORDING threads. The pending viewport is
// thread-local to setup, so a command-list hook cannot use it.
#if CVR_RUST_VRIK
#else
std::atomic<uint32_t> g_vrcam_view_w{0};
#endif
#if CVR_RUST_VRIK
#else
std::atomic<uint32_t> g_vrcam_view_h{0};
#endif

// STEP 1 SCOPE: this file forces the VRCAM view's PROJECTION only -- fov, zoom, near
// and far, copied from MAIN so the second view frames the world identically. It does
// NOT touch camera position or orientation. Every eye-offset / tripod / mirror / HMD
// path that used to live here is gone: writing the camera at this stage happens after
// culling and after the weapon viewmodel is placed, so it slid the world and dragged
// the weapon with the head. Camera work belongs in the engine's own camera hooks and
// is a later step.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) int32_t CyberpunkVR_OverlayVisible = 1;
#endif

// Captured from MAIN's camera ctx (slot 0) and applied to vrcam (slot 1) in the same
// per-frame camera writer -> smooth fov + weapon ZOOM match (fov@0x90, zoom@0x9C).
#if CVR_RUST_VRIK
#else
float g_main_cam_fov  = 0.f;
#endif
#if CVR_RUST_VRIK
#else
float g_main_cam_zoom = 0.f;
#endif
#if CVR_RUST_VRIK
#else
float g_main_cam_near = 0.f;
#endif
#if CVR_RUST_VRIK
#else
float g_main_cam_far  = 0.f;
#endif
// MAIN's forward-projection vertical scale (ctx+0x214 = cot(fovV/2)) -- the field that
// actually carries weapon ADS, and the source the vrcam fov is derived from.
#if CVR_RUST_VRIK
#else
float g_main_proj_yy = 0.f;
#endif
#if CVR_RUST_VRIK
#else
float g_ads_factor   = 1.0f;   // ADS as a scale factor, 1.0 = no zoom
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/declared_moved_exports.rs.
#else
extern "C" __declspec(dllexport) float CyberpunkVR_DebugMainCamFov      = 0.f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugMainProjYY      = 0.f;
extern "C" __declspec(dllexport) float CyberpunkVR_MainAdsZoomFactor = 0.f;
extern "C" __declspec(dllexport) float CyberpunkVR_DebugVrcamWantFov    = 0.f;
#endif
// Explicit vertical FOV in degrees for the vrcam eye. 0 = follow MAIN, which is what the
// flat-screen testbed wants. This is the hook for the headset: once the HMD drives the eye,
// put its FOV here and weapon ADS keeps applying on top of it.
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) float CyberpunkVR_VrcamFovDeg = 0.f;
#endif
// Last fov/zoom we forced a vrcam view-rebuild for -> only re-dirty on an actual change
// (avoids a per-frame RTT view rebuild when fov/zoom are stable; the game already
// rebuilds on camera movement).
#if !CVR_RUST_VRIK
static float g_last_forced_fov  = -1.f;
static float g_last_forced_zoom = -1.f;
#endif

// A scope is an independent RTT view. It needs a drawable native viewport and
// an extracted final colour, but must never enter the second-eye pose/history
// path. Match the authored camera name, not its size or its place in a frame.
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // no code of the plugin names it
static const uint64_t kPhysicalOpticsViewKey = cvr::cname_hash("vr_optics_feed");
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_OpticsViewSetupHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_OpticsViewExtractHits = 0;
#endif

#if CVR_RUST_VRIK
// FLAGCOMPUTE IS RUST'S (rust/crates/cvr-render/src/frame_graph.rs), registered from engine_detours.rs.
#else
__int64 __fastcall Detour_FlagCompute(void* a1, __int64 a2, __int64 a3, __int64 a4) {
    t_vrcam_viewport = {};
    bool vrcam = false;
    if (a3 && g_rtt_force_flags.load(std::memory_order_relaxed)) {
        __try {
            uint64_t key = *reinterpret_cast<uint64_t*>(a3 + 0x28);
            if (key == 0) {
                g_main_ctx = static_cast<uintptr_t>(a3);   // live MAIN env source
                // IPD stereo is applied in Detour_SlConstants (the camera writer,
                // the struct the render actually reads)  NOT this FlagCompute ctx.
            } else if (key == g_vrcam_ctx_key) {
                vrcam = true;
                uint32_t w = *reinterpret_cast<uint32_t*>(a3 + 0x44);
                uint32_t h = *reinterpret_cast<uint32_t*>(a3 + 0x4C);  // H@+0x4C ([W,W,H,H]); +0x48 is a W-dup (rect square bug)
                // FORCE resolution = MAIN: resize VRCAM render dims to main's W/H
                // (main dims @ g_main_ctx+0x44/0x48). Rect below then uses main size.
                if (g_main_ctx && CyberpunkVR_ForceVrcamRes) {
                    uint32_t mw = *reinterpret_cast<uint32_t*>(g_main_ctx + 0x44);
                    uint32_t mh = *reinterpret_cast<uint32_t*>(g_main_ctx + 0x48);
                    CyberpunkVR_DebugMainW = mw; CyberpunkVR_DebugMainH = mh;
                    if (mw && mh) {
                        w = mw; h = mh;
                        *reinterpret_cast<uint32_t*>(a3 + 0x44) = mw;
                        *reinterpret_cast<uint32_t*>(a3 + 0x48) = mh;
                        CVR_DIAGNOSTIC(++CyberpunkVR_DebugForceResHits);
                    }
                }
                // Arm even on the first setup, before native code has written
                // render dimensions. RectCompute already has destination bounds.
                t_vrcam_viewport={static_cast<uintptr_t>(a3)+PendingVrcamViewport::InputOffset,w,h};
                if (w && h) {
                    // Give FlagCompute a valid render rect BEFORE it runs: it reads
                    // ctx+0x14 (sub_14033AA6C) to decide the lighting feature set.
                    // Empty rect -> reduced flags -> no lighting-composite resources.
                    *reinterpret_cast<uint32_t*>(a3 + 0x14) = 0;
                    *reinterpret_cast<uint32_t*>(a3 + 0x18) = 0;
                    *reinterpret_cast<uint32_t*>(a3 + 0x1C) = w;
                    *reinterpret_cast<uint32_t*>(a3 + 0x20) = h;
                    g_vrcam_view_w.store(w, std::memory_order_release);
                    g_vrcam_view_h.store(h, std::memory_order_release);
                }
                // Bind MAIN's live environment handle onto VRCAM using the engine's
                // OWN refcounted handle-assign (AddRef) -> exposure/tonemap/bloom
                // exactly like main, dynamic, and crash-safe (no raw-ptr aliasing).
                if (g_main_ctx && g_handle_assign) {
                    bool bound = false;
                    for (uint32_t off : kEnvHandleOffs) {
                        void** src = reinterpret_cast<void**>(g_main_ctx + off);
                        if (src[0]) {   // only when MAIN currently holds an env handle
                            g_handle_assign(reinterpret_cast<void*>(a3 + off),
                                            reinterpret_cast<void*>(g_main_ctx + off));
                            bound = true;
                        }
                    }
                    if (bound) CVR_DIAGNOSTIC(++CyberpunkVR_DebugRttEnvBindHits);
                    // The extra candidates, same mechanism, opt-in per slot.
                    const uint32_t xm = CyberpunkVR_EnvExtraMask;
                    for (uint32_t k = 0; k < kEnvExtraCount && xm; ++k) {
                        if (!(xm & (1u << k))) continue;
                        const uint32_t off = kEnvExtraOffs[k];
                        void** src = reinterpret_cast<void**>(g_main_ctx + off);
                        if (!src[0]) continue;
                        g_handle_assign(reinterpret_cast<void*>(a3 + off),
                                        reinterpret_cast<void*>(g_main_ctx + off));
                        CVR_DIAGNOSTIC(++CyberpunkVR_DebugEnvExtraBinds);
                    }
                }
                g_vrcam_view.store(static_cast<uintptr_t>(a3), std::memory_order_release);
                // The water description is numbers, not handles, so it is copied straight
                // across rather than assigned. See kWaterMirror for what each group is.
                water_mirror_apply(static_cast<uintptr_t>(a3));
                // (VRCAM camera fov/zoom force = MAIN is done in Detour_SlConstants, the
                // per-frame camera writer -> smooth, same ctx.)
                // IPD stereo for vrcam (RIGHT eye) is applied in Detour_SlConstants too.
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { vrcam = false; }
    }
    bool parityReady=false;
    bool optics=false;
    if (a3) {
        __try {
            optics = *reinterpret_cast<const uint64_t*>(a3 + 0x28) == kPhysicalOpticsViewKey;
            if (optics) {
                const uint32_t w = *reinterpret_cast<const uint32_t*>(a3 + 0x44);
                const uint32_t h = *reinterpret_cast<const uint32_t*>(a3 + 0x4C);
                // Same verified RectCompute commit as the native RTT eye. An
                // empty rect selects the reduced graph, which cannot produce
                // a colour image for the lens. Keep this view's own dimensions.
                t_vrcam_viewport={static_cast<uintptr_t>(a3)+PendingVrcamViewport::InputOffset,w,h};
                if (w && h && w <= 8192 && h <= 8192) {
                    *reinterpret_cast<VrcamRenderRect*>(a3 + 0x14) =
                        {0,0,static_cast<int32_t>(w),static_cast<int32_t>(h)};
                }
                // Use engine handle assignment so the scope owns its references
                // to the player's current exposure/environment, including dusk.
                if (g_main_ctx && g_handle_assign) {
                    for (uint32_t off : kEnvHandleOffs) {
                        if (*reinterpret_cast<void**>(g_main_ctx + off))
                            g_handle_assign(reinterpret_cast<void*>(a3 + off),
                                            reinterpret_cast<void*>(g_main_ctx + off));
                    }
                    const uint32_t extras=CyberpunkVR_EnvExtraMask;
                    for(uint32_t k=0;k<kEnvExtraCount;++k) {
                        const auto off=kEnvExtraOffs[k];
                        if((extras&(1u<<k)) && *reinterpret_cast<void**>(g_main_ctx+off))
                            g_handle_assign(reinterpret_cast<void*>(a3+off),
                                            reinterpret_cast<void*>(g_main_ctx+off));
                    }
                }
                g_optics_view.store(static_cast<uintptr_t>(a3),std::memory_order_release);
                water_mirror_apply(static_cast<uintptr_t>(a3));
                CVR_DIAGNOSTIC(++CyberpunkVR_OpticsViewSetupHits);
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) { optics=false; }
    }
    __int64 res = cvr::stereo::ComputeFlagsWithMainAa(g_orig_flag_compute,a1,a2,a3,a4,&parityReady);
    // MAIN's result is transient: reading its context later can see cleared
    // flags. Capture at the producer, not the highest-popcount graph ever seen.
    if(res)ObserveMainFeatures(static_cast<uintptr_t>(a3),static_cast<uintptr_t>(res));
    // AND AGAIN ONCE THE ORIGINAL HAS RUN. Whether this block is filled before FlagCompute or
    // inside it has not been measured, and it is not worth a build to find out: writing it twice
    // costs 96 bytes on one view per frame and covers both orders.
    if (vrcam || optics) water_mirror_apply(static_cast<uintptr_t>(a3));
    if (res && (vrcam || optics) && g_force_view_flags.load(std::memory_order_relaxed)) {
        __try {
            // (Optional / default-OFF) Force VRCAM's feature flags to the current
            // main view's. This makes VRCAM run the FULL main pass set, but that
            // includes VIEW-DEPENDENT global passes (shadow cascade regen, GI)
            // that write SHARED resources main also reads -> main shadow flicker.
            // Left off: VRCAM uses its own natural flags; rect + env-bind already
            // give it lighting + exposure/tonemap without touching main's shadows.
            const auto main=MainFeaturesForVrcam();
            const uint64_t m0=main.f0,m1=main.f1;
            if (m0 | m1) {
                uint64_t* f = reinterpret_cast<uint64_t*>(res);
                // FIX: flags = main, but reuse main's view-dependent global shadow/
                // lighting structures instead of letting VRCAM rebuild the shared
                // buffers (which shifts/flickers them for main):
                // RIGOROUS reuse mechanism (verified via RE of each work-fn):
                //   bit 50 SET   -> cascade reuse (builder sub_141D43040 references the
                //                   EXISTING cascade atlas instead of building; TRUE reuse).
                //   bit 31 CLEAR -> GI reuse (work sub_1407F4130 gates ONLY the update
                //                   sub_14077F758; the apply sub_1407F4230 + global GI data
                //                   at renderer+184 run/persist regardless -> reuse main's GI).
                //   bit 11 KEPT SET -> distant shadows ENABLED for vrcam (its lighting
                //                   samples the distant map). bit 11 gates the WHOLE distant
                //                   node sub_1405A93E8 (render + shared distant-manager state
                //                   advance) with no reuse-only sub-gate, so we instead keep
                //                   the bit set and SKIP vrcam's distant node via a dedicated
                //                   hook (Detour_DistantWork) -> vrcam neither advances the
                //                   shared manager (no ~1Hz shift) nor rebuilds -> it reuses
                //                   main's distant result. (GI-style reuse done through a hook
                //                   because distant lacks GI's update-only sub-gate.)
                uint64_t vf0;
                switch (CyberpunkVR_VrcamFlagMode) {
                    case 1:  vf0 = m0; break;                                        // exact main (own cascades+GI)
                    case 2:  vf0 = (m0 | SHADOW_CASCADE_REUSE_BIT); break;           // cascade reuse only, GI native
                    case 3:  vf0 = m0 & ~GI_FEATURE_BIT; break;                      // GI reuse only, cascade native
                    default: vf0 = (m0 | SHADOW_CASCADE_REUSE_BIT) & ~GI_FEATURE_BIT; // both reuse (current)
                }
                if (CyberpunkVR_DistantReuseMode == 0)
                    vf0 &= ~DISTANT_SHADOW_BIT;   // A/B: distant OFF for vrcam
                // The scope uses the same scene features/reuse policy as the
                // second eye, but its own AA and resolution-dependent resolve.
                f[0] = cvr::stereo::MergeViewFlags(vf0,f[0],optics || parityReady);
                const auto ownResolve=f[1]&FG_UPSCALER_MASK_F1;
                f[1] = SupportedHardwareFlags(m1);
                if(optics)f[1]=(f[1]&~FG_UPSCALER_MASK_F1)|ownResolve;
                else {
                    CyberpunkVR_DebugFgRttF0 = f[0];
                    CyberpunkVR_DebugFgRttF1 = f[1];
                }
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugRttFlagForceHits);
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // VRCAM final-color extraction fix: force build-bit 64 (f1 bit 0) so the scene
    // builder emits ExtractionSceneColor + ExtractionFinalColor for VRCAM, writing the
    // final-color that VRCAM's (unconditional) CopyToTexture then copies. Applied LAST
    // so it survives the flag-force block above. Gated + default OFF.
    if (res && vrcam && CyberpunkVR_VrcamExtractionFix) {
        __try {
            uint64_t* f = reinterpret_cast<uint64_t*>(res);
            f[1] |= 1ULL;                       // bit 64 -> sub_1407305B0(a4,64) == true
            CyberpunkVR_DebugFgRttF1 = f[1];
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugVrcamExtractionHits);
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    if (res && optics) {
        __try {
            // The RTT CopyToTexture consumer requires ExtractionFinalColor.
            auto* f=reinterpret_cast<uint64_t*>(res);
            f[1] |= 1ULL;
            // Native RTT omits these postprocessing groups. Direct texture
            // readback confirmed missing exposure/colour processing even when
            // the image was correctly extracted. Retain this view's own AA,
            // hardware features and scene flags; inherit only groups 79/80.
            const auto main=MainFeaturesForVrcam();
            constexpr uint64_t postGroups=(1ULL<<15)|(1ULL<<16);
            f[1]=(f[1]&~postGroups)|(main.f1&postGroups);
            // The optical image is shaded into the eye's HDR scene, whose
            // tonemap runs afterwards. Keep this intermediate scene linear.
            if(CyberpunkVR_OpticsSceneLinear)f[0]&=~3ull;
            if(CyberpunkVR_OpticsNoScreenSpace)f[0]&=~SCOPE_SCREEN_SPACE_BITS;
            f[1]&=~SCOPE_STEREO_BITS_F1;
            CVR_DIAGNOSTIC(++CyberpunkVR_OpticsViewExtractHits);
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    // UPSCALER SELECTOR (primary): capture MAIN's chosen upscaler (key==0), and for
    // vrcam force group 69 (DLSS) + clear 71/72/73 so SCENE_FULL's builder does NOT emit
    // the native TAAU/resolve (PipelineState_563) that crops. Runs LAST (survives the
    // main-flag-force above), independent of g_rtt_force_flags (re-reads the key), gated
    // on VrcamDlss ONLY, and only when MAIN itself selected DLSS. See FG_UPSCALER block.
    if (res) {
        __try {
            uint64_t key2 = a3 ? *reinterpret_cast<uint64_t*>(a3 + 0x28) : ~0ULL;
            uint64_t* f = reinterpret_cast<uint64_t*>(res);
            if (key2 == 0) {
                g_main_upscaler_groups.store(f[1] & FG_UPSCALER_MASK_F1, std::memory_order_release);
                cvr::sharpen::gameUpscaler.SelectDlss((f[1] & FG_DLSS_BIT_F1)!=0);
                CyberpunkVR_DebugMainUpscalerGroups = f[1] & FG_UPSCALER_MASK_F1;
                // AND THIS IS WHERE VRCAM'S DLSS DECIDES ITSELF. The whole feature is "mirror
                // whatever MAIN's upscaler is", and every gate below already refused to act unless
                // MAIN had group 69 -- so a separate switch could only ever be the wrong half of
                // an AND. Reading it off MAIN's own build flags removes the second thing to get
                // right, and it tracks the graphics menu live: turn DLSS off in the game and the
                // next graph build clears this, which unsticks vrcam's eval flag in Detour_ApplyDlss.
                const int32_t want = (f[1] & FG_DLSS_BIT_F1) ? 1 : 0;
                if (want != CyberpunkVR_VrcamDlss) {
                    CyberpunkVR_VrcamDlss = want;
                    log("[dlss] MAIN upscaler groups=%llX -> VRCAM DLSS %s (automatic)",
                        (unsigned long long)(f[1] & FG_UPSCALER_MASK_F1), want ? "ON" : "off");
                }
            } else if (key2 == g_vrcam_ctx_key && CyberpunkVR_VrcamDlss &&
                       (g_main_upscaler_groups.load(std::memory_order_acquire) & FG_DLSS_BIT_F1)) {
                uint64_t nf1 = (f[1] & ~FG_UPSCALER_MASK_F1) | FG_DLSS_BIT_F1;
                if (nf1 != f[1]) {
                    f[1] = nf1;
                    CyberpunkVR_DebugFgRttF1 = nf1;
                    CVR_DIAGNOSTIC(++CyberpunkVR_DebugUpscalerForceHits);
                }
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {}
    }
    return res;
}
#endif

// The viewport-rect computer. Its caller does: call sub_1404E3EB4 ; movups
// xmm0,[rax] ; movdqu [ctx+0x14],xmm0. For the VRCAM view (flagged by the
// FlagCompute hook that ran immediately before) the input viewport is empty so
// the result is (0,0,0,0), which makes the engine skip the entire lighting
// composite. Overwrite the result with the full RTT rect (0,0,W,H) so the rect
// written into ctx+0x14 is valid -> full lighting flags + resources + integrate.
#if !CVR_RUST_VRIK
using RectComputeFn = __int64(__fastcall*)(void*, void*, void*);
#endif
#if !CVR_RUST_VRIK
static RectComputeFn g_orig_rect_compute = nullptr;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugRttRectHits = 0;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static __int64 __fastcall Detour_RectCompute(void* a1, void* a2, void* a3) {
    const auto callerRva=reinterpret_cast<uintptr_t>(_ReturnAddress())-reinterpret_cast<uintptr_t>(g_exe_base);
    __int64 res = g_orig_rect_compute(a1, a2, a3);
    if (res && t_vrcam_viewport.Matches(reinterpret_cast<uintptr_t>(a1),callerRva)) {
        __try {
            VrcamRenderRect bounds{},rectangle{};
            if((!t_vrcam_viewport.width || !t_vrcam_viewport.height) && a3)
                bounds=*reinterpret_cast<const VrcamRenderRect*>(a3);
            if(t_vrcam_viewport.Apply(reinterpret_cast<uintptr_t>(a1),callerRva,bounds,rectangle)) {
                *reinterpret_cast<VrcamRenderRect*>(res)=rectangle;
                CVR_DIAGNOSTIC(++CyberpunkVR_DebugRttRectHits);
            }
        } __except (EXCEPTION_EXECUTE_HANDLER) {t_vrcam_viewport={};}
    }
    return res;
}
#endif

// ---- the water state, assigned by the engine's own operator ----------------------------------
//
// WHY A HOOK AND NOT THE COPY ABOVE. The copy works -- the counters proved it writes every frame and
// the underwater blur DID appear in the second eye -- but it flickered, and a hardware write
// breakpoint on the second view's +0x1800 caught the other writer in the act: sub_14067B380, reached
// from a large field-by-field copy of the whole view-parameter block, restoring the second view's OWN
// description a moment later. Which of the two lands last varies from frame to frame, and that is
// exactly what flicker is. Racing it with a second store would be the same bug with better odds.
//
// So the engine's own assignment is hooked and simply handed a different source. That also fixes
// something the memcpy could never do: the struct is 96 bytes of numbers followed by TWO REFCOUNTED
// HANDLES at +0x60 and +0x68, and this function AddRefs the incoming pair and Releases the outgoing
// one. Copying it by hand would either skip them -- which the port's own pointer guard did, refusing
// the qwords at +0x1810/+0x1818 because they read as heap addresses -- or corrupt their counts.
// Through the engine's operator both handles travel correctly and for free.
//
// SCOPED ON A POINTER THE PORT ITSELF SET, not on what the data looks like: the substitution happens
// only when the destination is exactly the second view's own water struct, and that view was
// identified by its camera-name key inside FlagCompute. Every other caller in the game -- MAIN's own
// assignment included -- passes through untouched.
#if !CVR_RUST_VRIK
using WaterAssignFn = void*(__fastcall*)(void*, void*);
#endif
#if !CVR_RUST_VRIK
WaterAssignFn g_orig_water_assign = nullptr;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/used_moved_exports_rest.rs.
#else
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_WaterAssign = 1;
#endif
#if CVR_RUST_VRIK
// Defined in Rust: rust/crates/cvr-render/src/render_moved_exports.rs.
#else
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWaterAssigns = 0;
#endif

#if !CVR_RUST_VRIK  // the Rust build's is rust/crates/cvr-render/src/frame_graph.rs
static void* __fastcall Detour_WaterAssign(void* dst, void* src) {
    if (CyberpunkVR_WaterAssign) {
        const uintptr_t vv = g_vrcam_view.load(std::memory_order_acquire);
        const uintptr_t ov = g_optics_view.load(std::memory_order_acquire);
        const uintptr_t mc = g_main_ctx;
        const auto address=reinterpret_cast<uintptr_t>(dst);
        if (mc && ((vv && vv!=mc && address==vv+kWaterOff) ||
                   (ov && ov!=mc && address==ov+kWaterOff))) {
            src = reinterpret_cast<void*>(mc + kWaterOff);
            CVR_DIAGNOSTIC(InterlockedIncrement64(
                reinterpret_cast<volatile LONG64*>(&CyberpunkVR_DebugWaterAssigns)));
        }
    }
    return g_orig_water_assign(dst, src);
}
#endif

// ---- registered where they are defined -------------------------------------------------------
#if CVR_RUST_VRIK
// THE BUILD OBSERVERS, FLAGCOMPUTE, THE RECT COMMIT, THE WATER ASSIGN AND THE RTT VIEW-CREATE ARE RUST'S
// (rust/crates/cvr-render/src/frame_graph.rs), registered here with this file's trampolines; Rust is handed them,
// FlagCompute's trampoline (render_parity.rs runs it), the upscaler gate, g_main_view_ctx, the engine's handle assign
// and the stereo log (the HUD's re-arm and grant and the half IPD are Rust's own).
extern "C" {
// Its table is built in Rust (frame_graph.rs, from stereo_shared.rs's objects); the upscaler gate stays here.
// The upscaler gate: rust/crates/cvr-render/src/xr_manager_services.rs.
}
// The four detours and their originals register from Rust (rust/crates/cvr-render/src/engine_detours.rs).
#else
CVR_DETOUR("[water] state-assign sub_14067B380", WATER_ASSIGN_RVA, Detour_WaterAssign, g_orig_water_assign)
CVR_DETOUR("[build] full-build sub_141D43040",      FULL_BUILD_RVA,   Detour_FullBuild,   g_orig_full_build)
CVR_DETOUR("[build] incr-build sub_14227BD10",      INCR_BUILD_RVA,   Detour_IncrBuild,   g_orig_incr_build)
CVR_DETOUR("[lighting] rect-compute sub_14206EF20",  RECT_COMPUTE_RVA, Detour_RectCompute, g_orig_rect_compute)
#endif
#if !CVR_RUST_VRIK   // the Rust build's registers from rust/crates/cvr-render/src/engine_detours.rs
CVR_DETOUR("[rtt] view-create sub_14048FAF4", RTT_VIEWCREATE_RVA, Detour_RTTViewCreate, g_orig_rtt_viewcreate)
#endif

}  // namespace detail
}  // namespace cvr
