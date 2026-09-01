// Mirror -- the desktop window that shows what the headset sees, and the thread that drives it.
//
// It owns its OWN command queue, D3D12 swapchain and window with a message pump, so DWM keeps
// drawing it. THAT IS THE WHOLE DESIGN CONSTRAINT: presenting the mirror on the game's render thread
// costs frames in the headset, which is the one place a cost is unacceptable. So the game thread does
// exactly one thing for the mirror -- appends a small copy onto a list it was already submitting --
// and this thread GPU-waits the game fence and presents. No CPU stall crosses over.
//
// The copy is submitted from Hook_ExecuteCommandLists, when the game submits the list that wrote the
// vrcam final. Queue ordering is what guarantees the copy reads a fresh frame rather than a torn one;
// there is no CPU synchronisation doing that job, and adding one would reintroduce the stall.
//
// WHY THE TARGET IS A RESOURCE WE OWN. The engine renders the vrcam final directly into a committed
// resource created here, redirected at the ctx-keyed RenderFinal2D node -- view-aware, not
// resolution-based, so it scales to two identical VR eyes. Committed means never aliased with main's
// transients, which is what stopped the mirror alternating between the bright and dark frame.
//
// Lifted out of src/Stereo/SyncStereo.cpp as four blocks, because the mirror was never contiguous:
// its objects sat with the module's other state, the game-side copy objects sat with the command-list
// hooks that borrow them, and its two functions were nine hundred lines apart with twenty-five leaf
// probes between them.

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
#include "Stereo/EngineRvas.hpp"
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include "Stereo/EngineRvas.hpp"

namespace cvr {
namespace detail {

// The TRUE vrcam final color is written by RenderFinal2D (sub_140209FF0) into
// Resource_96070 (2444x2444), which then rests in NON_PIXEL_SHADER_RESOURCE (64).
// CopyToTexture runs earlier (build order) -> its target is the pre-final (black),
// which is why capturing it gave black regardless of copy state.
// Final-color producer work-fns (main-only by cull; enable for VRCAM via node-needed).
std::atomic<bool> g_mirror_copy_armed{false};
std::atomic<uint32_t> g_mirror_src_state{ (uint32_t)D3D12_RESOURCE_STATE_COMMON };
// VRCAM own committed render target: the engine renders the vrcam final DIRECTLY into a
// stable resource WE own (redirected at the ctx-keyed RenderFinal2D node -> view-aware,
// NOT resolution-based, scales to identical VR eyes). Committed => never aliased with
// main's transients => always holds the correct vrcam final (kills the bright/dark race).
ID3D12Resource*             g_own_target = nullptr;
ID3D12DescriptorHeap*       g_own_rtv_heap = nullptr;
D3D12_CPU_DESCRIPTOR_HANDLE g_own_rtv{0};
std::mutex                  g_own_target_mtx;
ID3D12Resource*       g_d12_mtex = nullptr;
// mtex was created with ALLOW_RENDER_TARGET, so the HUD debug overlay may be drawn onto it.
bool                  g_d12_mtex_is_rt = false;
// Fullscreen premultiplied-alpha pass used only for that overlay. Lives here rather than in the
// capture path because it runs on OUR mirror list, which exists whenever the mirror window does.
static ColorBlit             g_hud_mirror_blit;
ID3D12Fence*          g_d12_fence = nullptr;
static std::atomic<uint64_t> g_d12_fence_next{1};
static std::atomic<uint64_t> g_d12_ready{0};
static std::atomic<bool>     g_d12_present_started{false};
std::mutex            g_d12_mtx;
UINT                  g_d12_w = 0, g_d12_h = 0;
DXGI_FORMAT           g_d12_fmt = DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
extern "C" __declspec(dllexport) uint32_t CyberpunkVR_DebugMirrorSrcState = 0;
extern "C" __declspec(dllexport) extern int32_t  CyberpunkVR_NoStateLies;
extern "C" __declspec(dllexport) extern uint64_t CyberpunkVR_DebugForeignStateRefusals;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorBarrierHits = 0;
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugMirrorPendingHits = 0;
std::atomic<uint64_t> g_mirror_vrcam_serial{0};
std::atomic<uint64_t> g_mirror_armed_serial{0};

// Defined with the mirror capture code below; these are used by the real-device
// command-list hooks installed before the vrcam resource is created.
static ID3D12Resource* g_mirror_stage = nullptr;
static D3D12_RESOURCE_DESC g_mirror_stage_desc{};
static ID3D12Fence* g_mirror_game_fence = nullptr;
static ID3D12CommandAllocator* g_mirror_game_copy_allocator = nullptr;
static ID3D12GraphicsCommandList* g_mirror_game_copy_list = nullptr;
static uint64_t g_mirror_game_copy_inflight = 0;
static std::mutex g_mirror_game_copy_mtx;
static std::atomic<uint64_t> g_mirror_game_fence_next{1};
static std::atomic<uint64_t> g_mirror_ready_fence{0};
std::atomic<ID3D12GraphicsCommandList*> g_mirror_pending_list{nullptr};

void d12_submit_mirror_copy(ID3D12CommandQueue* queue) {
    if (!CyberpunkVR_MirrorOutput || !queue || !g_game_device) return;
    const bool use_own = (CyberpunkVR_VrcamOwnTarget && g_own_target);
    // Prefer the stable committed snapshot (filled inline in the valid window, never
    // aliased, known COMMON state) over the transient whose heap main re-uses.
    ID3D12Resource* stable_src = nullptr;
    if (!use_own && CyberpunkVR_StableCopy &&
            g_stable_fresh.load(std::memory_order_acquire)) {
        std::lock_guard<std::mutex> lk(g_stable_mtx);
        stable_src = g_stable_tex;
    }
    const bool use_stable = (stable_src != nullptr);
    ID3D12Resource* dtex = use_own ? g_own_target
        : use_stable ? stable_src
        : g_captured_vrcam_res.load(std::memory_order_acquire);
    if (!dtex) return;
    std::unique_lock<std::mutex> lk(g_d12_copy_mtx, std::try_to_lock);
    if (!lk.owns_lock()) return;                          // single-flight; never stall
    D3D12_RESOURCE_DESC d{};
    if (!mirror_get_resource_desc(dtex, &d)) return;
    if (!d12_mirror_ensure(d)) return;                    // create mtex + fence once
    if (!g_d12_copy_list) {
        for (int i = 0; i < 4; ++i)
            if (FAILED(g_game_device->CreateCommandAllocator(
                    D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&g_d12_copy_alloc[i]))))
                return;
        if (FAILED(g_game_device->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT,
                g_d12_copy_alloc[0], nullptr, IID_PPV_ARGS(&g_d12_copy_list))))
            return;
        g_d12_copy_list->SetName(L"CVR_mirror_copy_list");
        g_d12_copy_list->Close();
    }
    const uint32_t idx = g_d12_copy_frame & 3u;
    if (g_d12_fence->GetCompletedValue() < g_d12_copy_slot_fence[idx]) return;  // in flight
    // Slot idx's previous submission is GPU-complete -> its readbacks are coherent.
    if (CyberpunkVR_LumaProbe && g_luma_valid[idx] && g_luma_map[idx]) {
        const double L = luma_probe_collect(idx);
        g_luma_valid[idx] = false;
        if (g_cb_valid[idx] && g_cb_map[idx]) cb_probe_collect(idx, L);
        if (g_ti_valid[idx] && g_ti_map[idx]) ti_probe_collect(idx, L);
    }
    g_cb_valid[idx] = false;
    g_ti_valid[idx] = false;
    if (g_visdump_slot == static_cast<int>(idx) && CyberpunkVR_VisionDump == 2) {
        vision_dump_write();
        g_visdump_slot = -1;
        CyberpunkVR_VisionDump = 0;
    }
    if (FAILED(g_d12_copy_alloc[idx]->Reset())) return;
    if (FAILED(g_d12_copy_list->Reset(g_d12_copy_alloc[idx], nullptr))) return;
    const CommandListVtableHook* e = command_list_hook_entry(g_d12_copy_list);
    if (!e || !e->barrier_call || !e->copyres) { g_d12_copy_list->Close(); return; }
    // Use the state hk_ResourceBarrier actually tracked for THIS captured resource (its
    // real resting state at copy time), NOT a fixed guess. A wrong StateBefore makes the
    // barrier a hazard and the copy reads stale/aliased heap memory (main's content) ->
    // bright/dark alternation. Fall back to the tunable only if nothing tracked yet.
    // Our own committed target always rests in RENDER_TARGET (engine renders into it; the
    // copy transitions RT->COPY_SOURCE->RT). For the fallback transient, use the actually-
    // tracked state (not a fixed guess).
    // THE FIXED GUESS IS GONE. CyberpunkVR_MirrorCopyState = 64 was documented in its own
    // definition as a "fixed GUESS fallback only", and a guessed StateBefore on an engine
    // resource is a lie to the driver's state machine -- which is the code the recurring 0x88
    // crash faults in. Our own targets have states we set ourselves and those stay; for the
    // engine's transient there is either a tracked state or no copy this frame.
    D3D12_RESOURCE_STATES copy_src_state = use_own
        ? D3D12_RESOURCE_STATE_RENDER_TARGET
        : use_stable ? D3D12_RESOURCE_STATE_COMMON   // stable rests in COMMON
        : D3D12_RESOURCE_STATE_COMMON;
    if (!use_own && !use_stable) {
        const uint32_t tracked = CyberpunkVR_DebugMirrorSrcState;
        if (tracked == 0 && CyberpunkVR_NoStateLies) {
            InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
                &CyberpunkVR_DebugForeignStateRefusals));
            return;      // a skipped mirror frame costs a frame; a wrong barrier costs the device
        }
        if (tracked != 0) copy_src_state = (D3D12_RESOURCE_STATES)tracked;
    }
    // ---- HUD on the mirror -----------------------------------------------------------------
    // The second-eye composite is otherwise only visible inside the headset, which makes it
    // untestable at a desk. Running it here shows exactly what eye 1 gets, on OUR OWN list.
    // When it runs it REPLACES the plain copy: it reads the same source and writes the composite
    // straight into the mirror texture, so there is no copy to pay for as well.
    ID3D12Resource* hudTex  = g_d12_mtex_is_rt ? CyberpunkVR_GetHudTexture() : nullptr;
    ID3D12Resource* hudBlur = hudTex ? CyberpunkVR_GetHudBlurTexture() : nullptr;
    ID3D12Resource* hudExpo = hudTex ? CyberpunkVR_GetHudExposureBuffer() : nullptr;
    ID3D12Resource* frameCb = hudTex ? CyberpunkVR_GetFrameConstantBuffer() : nullptr;
    // Optional: when it is missing the shader's own validity check rejects whatever we bind in
    // its place and falls back to the captured constants. Requiring it here is what made the HUD
    // vanish entirely -- a guard that contradicted the fallback the shader already had.
    ID3D12Resource* hudCb   = hudTex ? CyberpunkVR_GetHudConstantBuffer() : nullptr;
    CyberpunkVR_NoteHudCompositeInputs(hudTex, hudBlur, hudExpo, frameCb, hudCb);
    if (!hudCb) hudCb = frameCb;
    bool hud_composited = false;
    if (hudTex && hudBlur && hudExpo && frameCb && hudCb &&
        g_hud_mirror_blit.EnsureInitialized(g_game_device, g_d12_fmt, g_d12_w, g_d12_h)) {
        D3D12_RESOURCE_BARRIER mb[2]{};
        for (int i = 0; i < 2; ++i) {
            mb[i].Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            mb[i].Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        }
        mb[0].Transition.pResource = g_d12_mtex;
        mb[0].Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
        mb[0].Transition.StateAfter  = D3D12_RESOURCE_STATE_RENDER_TARGET;
        // The scene has to be readable by the pixel shader; it normally rests in COMMON, which
        // promotes implicitly, so only a non-COMMON source needs an explicit transition.
        UINT nb = 1;
        if (copy_src_state != D3D12_RESOURCE_STATE_COMMON) {
            mb[1].Transition.pResource = dtex;
            mb[1].Transition.StateBefore = copy_src_state;
            mb[1].Transition.StateAfter  = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
            nb = 2;
        }
        e->barrier_call(g_d12_copy_list, nb, mb);
        hud_composited = g_hud_mirror_blit.RecordHudComposite(
            g_d12_copy_list, dtex, hudTex, hudBlur, hudExpo, frameCb, hudCb, g_d12_mtex,
            hud_composite_params());
        // The scanner's outline, on top, while the mirror texture is still a render target.
        // It belongs here and not only in the headset path for a plain reason: this window IS
        // the desk-side view of eye 1, and the headset path only runs inside an XR session --
        // with no session its counters stay at zero and nothing composites anywhere, which is
        // exactly what "the outline did not appear" turned out to mean.
        if (hud_composited && CyberpunkVR_VisionToSecondEye) {
            if (ID3D12Resource* vis = CyberpunkVR_GetVisionTexture()) {
                if (g_hud_mirror_blit.RecordOverlay(g_d12_copy_list, vis, g_d12_mtex,
                                                    CyberpunkVR_VisionDebug,
                                                    CyberpunkVR_VisionFit != 0,
                                                    CyberpunkVR_VisionOffX,
                                                    CyberpunkVR_VisionOffY))
                    ++CyberpunkVR_DebugVisionOverlays;
            }
        }
        // The barrel dot, at the same NDC the overlay drew it at on the backbuffer. The mirror
        // window IS the desk-side view of eye 1, so without this the dot is invisible whenever
        // there is no headset session -- which is most of the time while working on it.
        if (hud_composited && CyberpunkVR_BarrelDotSecondEye && CyberpunkVR_BarrelDotTick &&
            GetTickCount64() - CyberpunkVR_BarrelDotTick < 250) {
            if (g_hud_mirror_blit.RecordDot(g_d12_copy_list, g_d12_mtex,
                                            CyberpunkVR_BarrelDotNdcX2,
                                            CyberpunkVR_BarrelDotNdcY,
                                            CyberpunkVR_BarrelDotRadiusPx,
                                            1.0f, 0.045f, 0.045f, 1.0f))
                ++CyberpunkVR_DebugBarrelDotDraws;
        }
        // Armed dump of the same layer, on the same list and the same fence as everything else
        // here. The snapshot rests in COMMON, which promotes implicitly for a copy source, so
        // this asserts nothing about anyone else's state.
        if (CyberpunkVR_VisionDump == 1) {
            ID3D12Resource* vis = CyberpunkVR_GetVisionTexture();
            D3D12_RESOURCE_DESC vdd{};
            if (vis && mirror_get_resource_desc(vis, &vdd)) {
                const uint32_t w = static_cast<uint32_t>(vdd.Width), h = vdd.Height;
                const uint32_t pitch = (w * 4 + 255u) & ~255u;
                const uint64_t need = static_cast<uint64_t>(pitch) * h;
                if (g_visdump_rb && (g_visdump_w != w || g_visdump_h != h)) {
                    g_visdump_rb->Unmap(0, nullptr);
                    g_visdump_rb->Release();
                    g_visdump_rb = nullptr; g_visdump_map = nullptr;
                }
                if (!g_visdump_rb) {
                    D3D12_HEAP_PROPERTIES rp{}; rp.Type = D3D12_HEAP_TYPE_READBACK;
                    rp.CreationNodeMask = rp.VisibleNodeMask = 1;
                    D3D12_RESOURCE_DESC bd{};
                    bd.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
                    bd.Width = need; bd.Height = 1; bd.DepthOrArraySize = 1;
                    bd.MipLevels = 1; bd.Format = DXGI_FORMAT_UNKNOWN;
                    bd.SampleDesc.Count = 1;
                    bd.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
                    if (SUCCEEDED(g_game_device->CreateCommittedResource(
                            &rp, D3D12_HEAP_FLAG_NONE, &bd,
                            D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                            IID_PPV_ARGS(&g_visdump_rb))) && g_visdump_rb) {
                        if (FAILED(g_visdump_rb->Map(0, nullptr, &g_visdump_map)))
                            g_visdump_map = nullptr;
                        g_visdump_w = w; g_visdump_h = h; g_visdump_pitch = pitch;
                    }
                }
                if (g_visdump_rb && g_visdump_map) {
                    D3D12_TEXTURE_COPY_LOCATION vs{}, vdst{};
                    vs.pResource = vis;
                    vs.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
                    vs.SubresourceIndex = 0;
                    vdst.pResource = g_visdump_rb;
                    vdst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
                    vdst.PlacedFootprint.Offset = 0;
                    vdst.PlacedFootprint.Footprint.Format = DXGI_FORMAT_R8G8B8A8_UNORM;
                    vdst.PlacedFootprint.Footprint.Width = w;
                    vdst.PlacedFootprint.Footprint.Height = h;
                    vdst.PlacedFootprint.Footprint.Depth = 1;
                    vdst.PlacedFootprint.Footprint.RowPitch = pitch;
                    g_d12_copy_list->CopyTextureRegion(&vdst, 0, 0, 0, &vs, nullptr);
                    g_visdump_slot = static_cast<int>(idx);
                    CyberpunkVR_VisionDump = 2;
                }
            } else {
                log("[vision] dump armed but no layer available -- scan something first");
                CyberpunkVR_VisionDump = 0;
            }
        }
        mb[0].Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
        mb[0].Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
        if (nb == 2) {
            mb[1].Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
            mb[1].Transition.StateAfter  = copy_src_state;
        }
        e->barrier_call(g_d12_copy_list, nb, mb);
    }
    if (hud_composited) g_mirror_pending_list.store(g_d12_copy_list, std::memory_order_release);
    else d12_append_mirror_copy(e, g_d12_copy_list, dtex, copy_src_state);
    // Luma probe: 8x8 center of the stable snapshot -> readback slot (same list, same
    // fence). Stable rests in COMMON between our operations; bracket accordingly.
    if (CyberpunkVR_LumaProbe && use_stable && luma_probe_ensure(idx)) {
        D3D12_RESOURCE_BARRIER lb{};
        lb.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        lb.Transition.pResource = dtex;
        lb.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        lb.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
        lb.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_SOURCE;
        e->barrier_call(g_d12_copy_list, 1, &lb);
        D3D12_TEXTURE_COPY_LOCATION src{};
        src.pResource = dtex;
        src.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        src.SubresourceIndex = 0;
        D3D12_TEXTURE_COPY_LOCATION dst{};
        dst.pResource = g_luma_rb[idx];
        dst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        dst.PlacedFootprint.Offset = 0;
        dst.PlacedFootprint.Footprint.Format = d.Format;
        dst.PlacedFootprint.Footprint.Width = 8;
        dst.PlacedFootprint.Footprint.Height = 8;
        dst.PlacedFootprint.Footprint.Depth = 1;
        dst.PlacedFootprint.Footprint.RowPitch = 256;
        const UINT cx = (UINT)(d.Width / 2), cy = (UINT)(d.Height / 2);
        D3D12_BOX box{ cx - 4, cy - 4, 0, cx + 4, cy + 4, 1 };
        g_d12_copy_list->CopyTextureRegion(&dst, 0, 0, 0, &src, &box);  // slot16 unhooked
        lb.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
        lb.Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
        e->barrier_call(g_d12_copy_list, 1, &lb);
        g_luma_parity[idx] = g_d12_copy_frame & 1u;
        g_luma_frame[idx]  = g_d12_copy_frame;
        g_luma_finidx[idx] = CyberpunkVR_DebugFinNatural;  // which fin version this frame
        g_luma_valid[idx]  = true;
    }
    // CB-flap probe: read the captured tonemap CB (848B) with the same fence. The CB
    // rests in VERTEX_AND_CONSTANT_BUFFER between frames (engine brackets its own
    // uploads the same way); queue order after this frame's submission = safe window.
    ID3D12Resource* cbres = g_cb_res.load(std::memory_order_acquire);
    if (cbres && g_cb_last_res.exchange(cbres, std::memory_order_acq_rel) != cbres) {
        g_cb_reset_pending.store(true, std::memory_order_release);
        // Graph rebuilt (CB resource changed): restart the chain-capture set so it
        // repopulates with live resources (old AddRef'd entries intentionally leak).
        g_tm_in_n.store(0, std::memory_order_release);
    }
    if (CyberpunkVR_LumaProbe && use_stable && cb_probe_ensure(idx)) {
        D3D12_RESOURCE_BARRIER cb{};
        cb.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        cb.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
        bool any = false;
        if (cbres) {
            cb.Transition.pResource = cbres;
            cb.Transition.StateBefore =
                D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER;
            cb.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            e->barrier_call(g_d12_copy_list, 1, &cb);
            g_d12_copy_list->CopyBufferRegion(g_cb_rb[idx], 0, cbres,
                g_cb_off.load(std::memory_order_acquire), 848);
            cb.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            cb.Transition.StateAfter =
                D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER;
            e->barrier_call(g_d12_copy_list, 1, &cb);
            any = true;
        }
        // Both views' adapted-exposure accumulators (28B; rest in PS|NPS after the
        // adaptation pass): vrcam -> offset 856, main -> offset 896.
        const D3D12_RESOURCE_STATES kExpoRest =
            D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE |
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE;
        ID3D12Resource* expo[2] = {
            g_expo_vrcam.load(std::memory_order_acquire),
            g_expo_main.load(std::memory_order_acquire) };
        for (int ei = 0; ei < 2; ++ei) {
            if (!expo[ei]) continue;
            cb.Transition.pResource = expo[ei];
            cb.Transition.StateBefore = kExpoRest;
            cb.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            e->barrier_call(g_d12_copy_list, 1, &cb);
            g_d12_copy_list->CopyBufferRegion(g_cb_rb[idx],
                (ei == 0) ? 856 : 896, expo[ei], 0, 28);
            cb.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
            cb.Transition.StateAfter = kExpoRest;
            e->barrier_call(g_d12_copy_list, 1, &cb);
            any = true;
        }
        g_cb_valid[idx] = any;
    }
    // Stage sampling: 8x8 center of each captured texture (whole-chain set). State =
    // last tracked StateAfter, restored after the copy. TYPELESS mapped to concrete.
    const uint32_t tin_n = g_tm_in_n.load(std::memory_order_acquire);
    if (CyberpunkVR_LumaProbe && use_stable && tin_n && ti_probe_ensure(idx)) {
        uint32_t out_i = 0;
        for (uint32_t i = 0; i < tin_n && i < 24 && out_i < 24; ++i) {
            TmInCap& cap = g_tm_in[i];
            ID3D12Resource* res = cap.res.load(std::memory_order_relaxed);
            const uint32_t st = cap.state.load(std::memory_order_relaxed);
            if (!res) continue;
            D3D12_RESOURCE_DESC rd{};
            if (!mirror_get_resource_desc(res, &rd)) continue;
            uint32_t cf = (uint32_t)rd.Format;
            if (cf == 23) cf = 24;          // R10G10B10A2_TYPELESS -> UNORM
            else if (cf == 9) cf = 10;      // R16G16B16A16_TYPELESS -> FLOAT
            else if (cf == 27) cf = 28;     // R8G8B8A8_TYPELESS -> UNORM
            else if (cf == 39) cf = 41;     // R32_TYPELESS -> R32_FLOAT
            // WHITELIST: any other format (depth R24G8, BC, R16 variants...) makes
            // the recorded CopyTextureRegion invalid -> the deferred list's Close()
            // fails EVERY frame -> mirror+probes freeze (observed). Skip them.
            if (cf != 10 && cf != 24 && cf != 26 && cf != 28 && cf != 41 &&
                cf != 87) {
                continue;
            }
            D3D12_RESOURCE_BARRIER tb{};
            tb.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            tb.Transition.pResource = res;
            tb.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            tb.Transition.StateBefore = (D3D12_RESOURCE_STATES)st;
            tb.Transition.StateAfter = D3D12_RESOURCE_STATE_COPY_SOURCE;
            const bool need =
                (tb.Transition.StateBefore != D3D12_RESOURCE_STATE_COPY_SOURCE);
            if (need) e->barrier_call(g_d12_copy_list, 1, &tb);
            D3D12_TEXTURE_COPY_LOCATION tsrc{}, tdst{};
            tsrc.pResource = res;
            tsrc.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
            tsrc.SubresourceIndex = 0;
            tdst.pResource = g_ti_rb[idx];
            tdst.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
            tdst.PlacedFootprint.Offset = out_i * 2048;
            tdst.PlacedFootprint.Footprint.Format = (DXGI_FORMAT)cf;
            tdst.PlacedFootprint.Footprint.Width = 8;
            tdst.PlacedFootprint.Footprint.Height = 8;
            tdst.PlacedFootprint.Footprint.Depth = 1;
            tdst.PlacedFootprint.Footprint.RowPitch = 256;
            const UINT tcx = (UINT)(rd.Width / 2), tcy = (UINT)(rd.Height / 2);
            D3D12_BOX tbox{ tcx - 4, tcy - 4, 0, tcx + 4, tcy + 4, 1 };
            g_d12_copy_list->CopyTextureRegion(&tdst, 0, 0, 0, &tsrc, &tbox);
            if (need) {
                tb.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_SOURCE;
                tb.Transition.StateAfter = (D3D12_RESOURCE_STATES)st;
                e->barrier_call(g_d12_copy_list, 1, &tb);
            }
            g_ti_src[idx][out_i] = res;
            g_ti_fmt[idx][out_i] = cf;
            g_ti_tag[idx][out_i] = g_tm_in_rva[i].load(std::memory_order_relaxed);
            ++out_i;
        }
        g_ti_count[idx] = out_i;
        g_ti_valid[idx] = out_i != 0;
    }
    if (FAILED(g_d12_copy_list->Close())) {
        static std::atomic<uint64_t> s_close_fails{0};
        const uint64_t n = s_close_fails.fetch_add(1) + 1;
        if (n == 1 || (n % 512) == 0)
            log("[mirror] deferred list Close FAILED (n=%llu) -- invalid recorded "
                "command; probes+mirror stall until fixed", (unsigned long long)n);
        return;
    }
    ID3D12CommandList* ls[1] = { g_d12_copy_list };
    g_orig_ExecuteCommandLists(queue, 1, ls);             // bypass our own ECL hook
    const uint64_t v = g_d12_fence_next.fetch_add(1, std::memory_order_relaxed);
    if (SUCCEEDED(queue->Signal(g_d12_fence, v))) {
        g_d12_copy_slot_fence[idx] = v;
        g_d12_ready.store(v, std::memory_order_release);
        CyberpunkVR_DebugMirrorReadyFence = v;
    }
    ++g_d12_copy_frame;
    InterlockedIncrement64(reinterpret_cast<volatile LONG64*>(
        &CyberpunkVR_DebugMirrorPendingHits));
    bool ex = false;
    if (g_d12_present_started.compare_exchange_strong(ex, true))
        std::thread(d12_present_thread).detach();
}

static LRESULT CALLBACK MirrorWndProc(HWND hh, UINT m, WPARAM wp, LPARAM lp) {
    if (m == WM_CLOSE) return 0;               // don't destroy on close
    return DefWindowProcW(hh, m, wp, lp);
}

// ---- D3D12 present thread (real second swapchain for VRCAM) ----------------
// Owns its OWN command queue + D3D12 swapchain + window (message pump, so DWM
// keeps drawing it). Copies g_d12_mtex (filled by the game's appended copy) into
// the backbuffer on its own queue (GPU-waits the game fence -> reads the fresh
// frame, no CPU stall on the game thread) and Presents. Present + window are
// fully off the game render thread -> no FPS drop, smooth input.
void d12_present_thread() {
    UINT w = 0, h = 0;
    for (int i = 0; i < 6000 && CyberpunkVR_MirrorOutput; ++i) {
        if (g_d12_mtex && g_game_device && g_d12_fence) { w = g_d12_w; h = g_d12_h; break; }
        Sleep(10);
    }
    if (!g_d12_mtex || !g_game_device || !g_d12_fence) return;
    ID3D12Device* dev = g_game_device;

    D3D12_COMMAND_QUEUE_DESC qd = {}; qd.Type = D3D12_COMMAND_LIST_TYPE_DIRECT;
    ID3D12CommandQueue* q = nullptr;
    if (FAILED(dev->CreateCommandQueue(&qd, IID_PPV_ARGS(&q))) || !q) return;
    ID3D12CommandAllocator* alloc = nullptr;
    ID3D12GraphicsCommandList* list = nullptr;
    if (FAILED(dev->CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT, IID_PPV_ARGS(&alloc)))) return;
    if (FAILED(dev->CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, alloc, nullptr, IID_PPV_ARGS(&list)))) return;
    list->Close();
    ID3D12Fence* pf = nullptr;
    if (FAILED(dev->CreateFence(0, D3D12_FENCE_FLAG_NONE, IID_PPV_ARGS(&pf)))) return;
    HANDLE ev = CreateEventW(nullptr, FALSE, FALSE, nullptr); UINT64 pv = 0;

    WNDCLASSEXW wc = { sizeof(wc) };
    wc.lpfnWndProc = MirrorWndProc; wc.hInstance = GetModuleHandleW(nullptr);
    wc.lpszClassName = L"CyberpunkVR_Mirror"; RegisterClassExW(&wc);
    // client area == vrcam resolution (w x h) so OBS captures native res, no scaling.
    RECT wr = { 0, 0, (LONG)w, (LONG)h };
    AdjustWindowRect(&wr, WS_OVERLAPPEDWINDOW, FALSE);
    HWND hwnd = CreateWindowExW(0, wc.lpszClassName, L"VRCAM Mirror (OBS)",
        WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
        wr.right - wr.left, wr.bottom - wr.top,
        nullptr, nullptr, wc.hInstance, nullptr);
    if (!hwnd) return;
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    HMODULE dxgi = GetModuleHandleW(L"dxgi.dll"); if (!dxgi) dxgi = LoadLibraryW(L"dxgi.dll");
    using PFN_CDXGI2 = HRESULT (WINAPI*)(UINT, REFIID, void**);
    auto pFac = dxgi ? reinterpret_cast<PFN_CDXGI2>(GetProcAddress(dxgi, "CreateDXGIFactory2")) : nullptr;
    IDXGIFactory2* fac = nullptr;
    if (!pFac || FAILED(pFac(0, IID_PPV_ARGS(&fac))) || !fac) return;
    DXGI_SWAP_CHAIN_DESC1 sd = {};
    sd.Width = w; sd.Height = h; sd.Format = DXGI_FORMAT_R8G8B8A8_UNORM; sd.SampleDesc.Count = 1;
    sd.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT; sd.BufferCount = 2;
    sd.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD; sd.Scaling = DXGI_SCALING_STRETCH;
    IDXGISwapChain1* sc1 = nullptr;
    HRESULT hr = fac->CreateSwapChainForHwnd(q, hwnd, &sd, nullptr, nullptr, &sc1);
    fac->Release();
    CyberpunkVR_DebugMirrorLastHr = (uint32_t)hr;
    if (FAILED(hr) || !sc1) return;
    IDXGISwapChain3* sc = nullptr;
    if (FAILED(sc1->QueryInterface(IID_PPV_ARGS(&sc)))) { sc1->Release(); return; }
    sc1->Release();
    CyberpunkVR_DebugMirrorState = 3;
    log("[mirror] d12 present-thread ready %ux%u", w, h);

    // RTV descriptor heap for the 2 backbuffers (used only by the red test pattern).
    ID3D12DescriptorHeap* rtvHeap = nullptr;
    D3D12_CPU_DESCRIPTOR_HANDLE rtvH[2] = {};
    {
        D3D12_DESCRIPTOR_HEAP_DESC hd = {};
        hd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_RTV; hd.NumDescriptors = 2;
        if (SUCCEEDED(dev->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&rtvHeap))) && rtvHeap) {
            const UINT inc = dev->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_RTV);
            const D3D12_CPU_DESCRIPTOR_HANDLE base = rtvHeap->GetCPUDescriptorHandleForHeapStart();
            for (UINT i = 0; i < 2; ++i) {
                rtvH[i] = base; rtvH[i].ptr += (SIZE_T)inc * i;
                ID3D12Resource* bbi = nullptr;
                if (SUCCEEDED(sc->GetBuffer(i, IID_PPV_ARGS(&bbi))) && bbi) {
                    if (g_orig_CreateRTV) g_orig_CreateRTV(dev, bbi, nullptr, rtvH[i]);
                    else dev->CreateRenderTargetView(bbi, nullptr, rtvH[i]);
                    bbi->Release();
                }
            }
        }
    }

    // ---- Format-converting present pipeline -------------------------------
    // The vrcam final (g_d12_mtex) is an HDR packed-float target (R11G11B10),
    // not copy-compatible with the 8-bit backbuffer. Sample it in a fullscreen
    // pass and write the backbuffer -> works for ANY source format (this is the
    // HDR->8bit step the engine's missing swapchain-composition passes would do).
    ID3D12RootSignature* convRoot = nullptr;
    ID3D12PipelineState* convPso  = nullptr;
    ID3D12DescriptorHeap* srvHeap = nullptr;
    bool convOk = false;
    {
        HMODULE d3dc = GetModuleHandleW(L"d3dcompiler_47.dll");
        if (!d3dc) d3dc = LoadLibraryW(L"d3dcompiler_47.dll");
        using PFN_D3DCompile = HRESULT (WINAPI*)(LPCVOID, SIZE_T, LPCSTR,
            const D3D_SHADER_MACRO*, ID3DInclude*, LPCSTR, LPCSTR, UINT, UINT,
            ID3DBlob**, ID3DBlob**);
        auto pCompile = d3dc ? reinterpret_cast<PFN_D3DCompile>(
            GetProcAddress(d3dc, "D3DCompile")) : nullptr;
        HMODULE d3d12 = GetModuleHandleW(L"d3d12.dll");
        using PFN_SerRS = HRESULT (WINAPI*)(const D3D12_ROOT_SIGNATURE_DESC*,
            D3D_ROOT_SIGNATURE_VERSION, ID3DBlob**, ID3DBlob**);
        auto pSerRS = d3d12 ? reinterpret_cast<PFN_SerRS>(
            GetProcAddress(d3d12, "D3D12SerializeRootSignature")) : nullptr;
        // EXACT replica of the engine's PipelineState_1335 MODE 0 (SDR 8-bit path):
        // Load the HDR texel (no filter) -> max(0) -> sRGB OETF -> animated triangular
        // dither (anti-banding). No tonemap (done earlier by ApplyBloomAndTonemapping),
        // no exposure scale (MODE 0 has none). Matches main's final swapchain write.
        static const char* kHLSL =
            "Texture2D gTex:register(t0);"
            "cbuffer T:register(b0){float gT;};"                          // per-frame time (dither anim)
            "void VSMain(uint vid:SV_VertexID,out float4 pos:SV_Position,out float2 uv:TEXCOORD0){"
            "uv=float2((vid<<1)&2,vid&2);pos=float4(uv.x*2-1,1-uv.y*2,0,1);}"
            "float4 PSMain(float4 pos:SV_Position,float2 uv:TEXCOORD0):SV_Target{"
            "float3 c=max(0.0,gTex.Load(int3(int2(pos.xy),0)).rgb);"
            "float3 st=step(0.0031308,c);"
            "float3 e=lerp(c*12.92,1.055*pow(abs(c),1.0/2.4)-0.055,st);"  // engine sRGB OETF
            "float t=frac(gT*6.2272);float2 p=pos.xy;"                    // triangular dither (animated)
            "float ax=frac(p.x*211.1488),ay=frac(p.y*210.944);"
            "float d1=dot(float3(ax,ay,t),float3(ay+33.33,ax+33.33,t+33.33));"
            "float u1=d1+ax,v1=d1+ay,w1=u1+v1;"
            "float3 n1=float3(frac(w1*(d1+t)),frac((u1*2)*v1),frac(w1*u1));"
            "float2 q=p+64.0;"
            "float bx=frac(q.x*211.1488),by=frac(q.y*210.944);"
            "float d2=dot(float3(bx,by,t),float3(by+33.33,bx+33.33,t+33.33));"
            "float u2=d2+bx,v2=d2+by,w2=u2+v2;"
            "float3 n2=float3(frac(w2*(d2+t)),frac((u2*2)*v2),frac(w2*u2));"
            "float3 vv=e*510.0;float3 edge=min(min(float3(1,1,1),vv),510.0-vv);"
            "float3 o=((n1-0.5)+edge*(n2-0.5))*(1.0/255.0)+e;"
            "return float4(o,1);}";
        ID3DBlob* vs = nullptr; ID3DBlob* ps = nullptr; ID3DBlob* err = nullptr;
        if (pCompile && pSerRS &&
            SUCCEEDED(pCompile(kHLSL, strlen(kHLSL), "conv", nullptr, nullptr,
                "VSMain", "vs_5_0", 0, 0, &vs, &err)) && vs &&
            SUCCEEDED(pCompile(kHLSL, strlen(kHLSL), "conv", nullptr, nullptr,
                "PSMain", "ps_5_0", 0, 0, &ps, &err)) && ps) {
            D3D12_DESCRIPTOR_RANGE range = {};
            range.RangeType = D3D12_DESCRIPTOR_RANGE_TYPE_SRV;
            range.NumDescriptors = 1;
            D3D12_ROOT_PARAMETER rp[2] = {};
            rp[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
            rp[0].DescriptorTable.NumDescriptorRanges = 1;
            rp[0].DescriptorTable.pDescriptorRanges = &range;
            rp[0].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            rp[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_32BIT_CONSTANTS;
            rp[1].Constants.ShaderRegister = 0;   // b0
            rp[1].Constants.RegisterSpace = 0;
            rp[1].Constants.Num32BitValues = 1;   // gT
            rp[1].ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            D3D12_STATIC_SAMPLER_DESC ss = {};
            ss.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
            ss.AddressU = ss.AddressV = ss.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
            ss.MaxLOD = D3D12_FLOAT32_MAX;
            ss.ShaderVisibility = D3D12_SHADER_VISIBILITY_PIXEL;
            D3D12_ROOT_SIGNATURE_DESC rsd = {};
            rsd.NumParameters = 2; rsd.pParameters = rp;
            rsd.NumStaticSamplers = 1; rsd.pStaticSamplers = &ss;
            ID3DBlob* rsBlob = nullptr;
            if (SUCCEEDED(pSerRS(&rsd, D3D_ROOT_SIGNATURE_VERSION_1, &rsBlob, nullptr)) && rsBlob &&
                SUCCEEDED(dev->CreateRootSignature(0, rsBlob->GetBufferPointer(),
                    rsBlob->GetBufferSize(), IID_PPV_ARGS(&convRoot))) && convRoot) {
                D3D12_GRAPHICS_PIPELINE_STATE_DESC pd = {};
                pd.pRootSignature = convRoot;
                pd.VS = { vs->GetBufferPointer(), vs->GetBufferSize() };
                pd.PS = { ps->GetBufferPointer(), ps->GetBufferSize() };
                pd.BlendState.RenderTarget[0].RenderTargetWriteMask = 0xF;
                pd.SampleMask = 0xFFFFFFFF;
                pd.RasterizerState.FillMode = D3D12_FILL_MODE_SOLID;
                pd.RasterizerState.CullMode = D3D12_CULL_MODE_NONE;
                pd.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE_TRIANGLE;
                pd.NumRenderTargets = 1;
                pd.RTVFormats[0] = DXGI_FORMAT_R8G8B8A8_UNORM;
                pd.SampleDesc.Count = 1;
                if (SUCCEEDED(dev->CreateGraphicsPipelineState(&pd, IID_PPV_ARGS(&convPso))) && convPso) {
                    D3D12_DESCRIPTOR_HEAP_DESC shd = {};
                    shd.Type = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
                    shd.NumDescriptors = 1;
                    shd.Flags = D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE;
                    if (SUCCEEDED(dev->CreateDescriptorHeap(&shd, IID_PPV_ARGS(&srvHeap))) && srvHeap) {
                        D3D12_SHADER_RESOURCE_VIEW_DESC sv = {};
                        sv.Format = g_d12_fmt;
                        sv.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
                        sv.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
                        sv.Texture2D.MipLevels = 1;
                        dev->CreateShaderResourceView(g_d12_mtex, &sv,
                            srvHeap->GetCPUDescriptorHandleForHeapStart());
                        convOk = true;
                    }
                }
            }
            if (rsBlob) rsBlob->Release();
        }
        if (vs) vs->Release(); if (ps) ps->Release(); if (err) err->Release();
        log("[mirror] convert-pipeline %s (mtex fmt=%u)",
            convOk ? "ready" : "FAILED", (unsigned)g_d12_fmt);
    }

    uint64_t last = 0;
    bool shown = true;                       // window was created visible
    for (;;) {
        MSG m; while (PeekMessageW(&m, nullptr, 0, 0, PM_REMOVE)) { TranslateMessage(&m); DispatchMessageW(&m); }
        // Follow the toggle with the window itself: pausing Present alone would leave a frozen
        // mirror window on screen, which reads as "the toggle did nothing".
        const bool want = CyberpunkVR_MirrorOutput != 0;
        if (want != shown) {
            shown = want;
            ShowWindow(hwnd, want ? SW_SHOWNOACTIVATE : SW_HIDE);
        }
        if (!want) { Sleep(50); continue; }
        const bool testpat = (CyberpunkVR_MirrorTestPattern != 0) && rtvHeap;
        const uint64_t rdy = g_d12_ready.load(std::memory_order_acquire);
        if (!testpat && rdy <= last) { Sleep(2); continue; }
        if (!testpat) q->Wait(g_d12_fence, rdy);   // GPU: our copy after the game's write
        alloc->Reset(); list->Reset(alloc, nullptr);
        const UINT idx = sc->GetCurrentBackBufferIndex();
        ID3D12Resource* bb = nullptr;
        if (SUCCEEDED(sc->GetBuffer(idx, IID_PPV_ARGS(&bb))) && bb) {
            D3D12_RESOURCE_BARRIER b = {};
            b.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
            b.Transition.pResource = bb;
            b.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
            if (testpat) {
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_RENDER_TARGET;
                list->ResourceBarrier(1, &b);
                const float red[4] = { 1.0f, 0.0f, 0.0f, 1.0f };
                list->ClearRenderTargetView(rtvH[idx], red, 0, nullptr);
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_PRESENT;
                list->ResourceBarrier(1, &b);
            } else if (convOk && rtvHeap) {
                // HDR (e.g. R11G11B10) mtex -> 8-bit backbuffer via a fullscreen
                // sample+write -- the same HDR->8bit composite MAIN's RenderFinal2D
                // does into its swapchain backbuffer, but for the vrcam final.
                D3D12_RESOURCE_BARRIER mb = {};
                mb.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
                mb.Transition.pResource = g_d12_mtex;
                mb.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
                mb.Transition.StateBefore = D3D12_RESOURCE_STATE_COMMON;
                mb.Transition.StateAfter  = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                list->ResourceBarrier(1, &mb);
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_RENDER_TARGET;
                list->ResourceBarrier(1, &b);
                ID3D12DescriptorHeap* heaps[] = { srvHeap };
                list->SetDescriptorHeaps(1, heaps);
                list->SetGraphicsRootSignature(convRoot);
                list->SetPipelineState(convPso);
                list->SetGraphicsRootDescriptorTable(0,
                    srvHeap->GetGPUDescriptorHandleForHeapStart());
                float ditherT = (float)(pv & 0x3FF);   // animate dither per present frame
                list->SetGraphicsRoot32BitConstants(1, 1, &ditherT, 0);
                D3D12_VIEWPORT vp = { 0.f, 0.f, (float)w, (float)h, 0.f, 1.f };
                D3D12_RECT rc = { 0, 0, (LONG)w, (LONG)h };
                list->RSSetViewports(1, &vp);
                list->RSSetScissorRects(1, &rc);
                list->OMSetRenderTargets(1, &rtvH[idx], FALSE, nullptr);
                list->IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                list->DrawInstanced(3, 1, 0, 0);
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_RENDER_TARGET;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_PRESENT;
                list->ResourceBarrier(1, &b);
                mb.Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
                mb.Transition.StateAfter  = D3D12_RESOURCE_STATE_COMMON;
                list->ResourceBarrier(1, &mb);
            } else {
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_PRESENT;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_COPY_DEST;
                list->ResourceBarrier(1, &b);
                list->CopyResource(bb, g_d12_mtex);    // mtex COMMON -> implicit COPY_SOURCE
                b.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
                b.Transition.StateAfter  = D3D12_RESOURCE_STATE_PRESENT;
                list->ResourceBarrier(1, &b);
            }
            list->Close();
            ID3D12CommandList* ls[] = { list };
            q->ExecuteCommandLists(1, ls);
            sc->Present(testpat ? 0 : 1, 0);       // vsync on OUR thread only
            bb->Release();
            q->Signal(pf, ++pv);                   // throttle THIS thread (allocator reuse)
            if (pf->GetCompletedValue() < pv) { pf->SetEventOnCompletion(pv, ev); WaitForSingleObject(ev, 100); }
            ++CyberpunkVR_DebugMirrorFrames;
            last = rdy;
            if (testpat) Sleep(16);
        } else {
            list->Close();
        }
    }
}

}  // namespace detail
}  // namespace cvr
