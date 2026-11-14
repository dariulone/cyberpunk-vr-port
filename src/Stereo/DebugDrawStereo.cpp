// DebugDrawStereo -- the engine's debug fragments (Debug Draw's shapes, externals/cp2077_debug_draw) drawn per eye.
//
// THE NODE IS NOT STEREO-AWARE. Debug Draw adds CRenderNode_RenderDebugFragments to the main render graph (after
// ApplyFXAA, at render resolution; its "kind 0"). The node's work (sub_142EA16B4) sets one viewport over the whole
// target (sub_1403DECAC -> sub_140333A44), binds one camera (sub_140297904(ctx, camera copy, w, h, 0, 1, 3)) and
// draws its fragment lists (sub_14229D450). Under engine stereo the target holds both eyes side by side, so a shape
// came out once across the pair, cut at the seam, seen from the centre camera.
//
// WHAT A STEREO-AWARE NODE DOES INSTEAD (sub_1422A1050, DecoupledParticleLighting, eye 1 then 2 under feature 98):
// it binds the eye's camera -- sub_14206EE80(camera, eye): with camera+921 bit 0 set, the eye cameras at camera+976
// (left) and +984 (right) -- through the same sub_140297904, with the full frame size. Those eye cameras carry the
// port's own per-eye projection (Stereo/AsymProjection.cpp writes the eye views' input blocks they are built from).
// The node's camera is a stack copy (sub_1402130D0), and that copy clones both eye cameras, so they are there.
//
// SO EACH LIST IS DRAWN TWICE. While this node runs (thread-local, the node's own thread), the viewport and the camera
// it sets are recorded on their way through; at each list draw the left eye gets the left half of that viewport and
// its own camera, the right eye the right half and its camera, and the node's own viewport and camera are put back
// after. The node itself runs once, so whatever else it does (targets, the upscaled copy) happens once as before.
// Its 2D lists ("kind 1", text) bind no camera through sub_140297904 and are left alone.
//
// All four functions call further engine code, so their callers cannot keep volatile registers across them
// (not leaves; see the leaf-function rule in Hooks). Off with CyberpunkVR_DebugDrawStereo = 0.

#include "Stereo/StereoInternal.hpp"
#include "Stereo/DetourRegistry.hpp"

#include <windows.h>
#include <atomic>
#include <cstdint>
#include <cstring>

extern void Log(const char* fmt, ...);

extern "C" {
// 1 = Debug Draw's shapes drawn per eye under engine stereo. 0 = once over both eyes, as the engine does.
__declspec(dllexport) int32_t CyberpunkVR_DebugDrawStereo = 1;
// Lists drawn per eye since start (diagnostics).
__declspec(dllexport) uint32_t CyberpunkVR_DebugDrawStereoLists = 0;
}

namespace cvr::detail {
namespace {

constexpr uintptr_t DEBUG_FRAGMENTS_WORK_RVA = 0x2EA16B4;  // CRenderNode_RenderDebugFragments work (vtable slot 2)
constexpr uintptr_t SET_VIEWPORT_RVA = 0x333A44;           // sub_140333A44(viewport): viewport and scissor
constexpr uintptr_t BIND_CAMERA_RVA = 0x297904;            // sub_140297904(ctx, camera, w, h, block, a6, slot)
constexpr uintptr_t DRAW_FRAGMENT_LIST_RVA = 0x229D450;    // sub_14229D450(drawer, draw ctx, list, a4, a5)
constexpr uintptr_t FEATURE_BIT_RVA = 0x33E358;            // sub_14033E358(ctx, bit); 98 = engine stereo this frame
constexpr int kStereoFeature = 98;
constexpr size_t kCameraStereoFlags = 921;   // bit 0: the camera has eye cameras
constexpr size_t kCameraLeft = 976, kCameraRight = 984;

// sub_1403DECAC's viewport: x, y, width, height (pixels), min and max depth.
struct Viewport { int32_t x, y, w, h; float minDepth, maxDepth; };
static_assert(sizeof(Viewport) == 24, "the engine's viewport record");

using WorkFn = __int64(__fastcall*)(void* node, uint8_t* ctx);
using SetViewportFn = __int64(__fastcall*)(Viewport* viewport);
using BindCameraFn = __int64(__fastcall*)(uint8_t* ctx, uint8_t* camera, uint64_t w, uint64_t h, void* block,
                                          uint64_t a6, uint64_t slot);
using DrawListFn = __int64(__fastcall*)(void* drawer, void* drawCtx, uint64_t list, uint64_t a4, uint64_t a5);
using FeatureBitFn = bool(__fastcall*)(void* ctx, int bit);

WorkFn g_orig_work = nullptr;
SetViewportFn g_orig_set_viewport = nullptr;
BindCameraFn g_orig_bind_camera = nullptr;
DrawListFn g_orig_draw_list = nullptr;

struct Camera { uint8_t* ctx; uint8_t* camera; uint64_t w, h; void* block; uint64_t a6, slot; };
struct NodeState {
    uint8_t* ctx = nullptr;
    bool haveViewport = false, haveCamera = false;
    Viewport viewport{};
    Camera camera{};
};
// The debug node running on this thread, under engine stereo; null otherwise.
thread_local NodeState* t_node = nullptr;

bool StereoFrame(uint8_t* ctx) {
    if (!g_exe_base || !ctx) return false;
    const auto test = reinterpret_cast<FeatureBitFn>(g_exe_base + FEATURE_BIT_RVA);
    return test(ctx, kStereoFeature);
}

uint8_t* EyeCamera(uint8_t* camera, int eye) {
    if (!camera || !(camera[kCameraStereoFlags] & 1)) return nullptr;
    return *reinterpret_cast<uint8_t**>(camera + (eye == 1 ? kCameraLeft : kCameraRight));
}

__int64 __fastcall Detour_DebugFragmentsWork(void* node, uint8_t* ctx) {
    if (CyberpunkVR_DebugDrawStereo == 0 || !StereoFrame(ctx)) return g_orig_work(node, ctx);
    NodeState state;
    state.ctx = ctx;
    NodeState* const outer = t_node;
    t_node = &state;
    const __int64 r = g_orig_work(node, ctx);
    t_node = outer;
    return r;
}

__int64 __fastcall Detour_SetViewport(Viewport* viewport) {
    if (NodeState* s = t_node; s && viewport) {
        s->viewport = *viewport;
        s->haveViewport = true;
    }
    return g_orig_set_viewport(viewport);
}

__int64 __fastcall Detour_BindCamera(uint8_t* ctx, uint8_t* camera, uint64_t w, uint64_t h, void* block, uint64_t a6,
                                     uint64_t slot) {
    if (NodeState* s = t_node; s && ctx == s->ctx) {
        s->camera = Camera{ctx, camera, w, h, block, a6, slot};
        s->haveCamera = true;
    }
    return g_orig_bind_camera(ctx, camera, w, h, block, a6, slot);
}

__int64 __fastcall Detour_DrawFragmentList(void* drawer, void* drawCtx, uint64_t list, uint64_t a4, uint64_t a5) {
    NodeState* const s = t_node;
    uint8_t* const left = s && s->haveCamera ? EyeCamera(s->camera.camera, 1) : nullptr;
    uint8_t* const right = s && s->haveCamera ? EyeCamera(s->camera.camera, 2) : nullptr;
    if (!s || !s->haveViewport || !left || !right || s->viewport.w < 2)
        return g_orig_draw_list(drawer, drawCtx, list, a4, a5);
    static std::atomic<bool> said{false};
    if (!said.exchange(true))
        Log("[debugdraw-stereo] debug fragments drawn per eye: viewport %d x %d split in two, eye cameras %p / %p\n",
            s->viewport.w, s->viewport.h, left, right);
    const Camera& c = s->camera;
    const int half = s->viewport.w / 2;
    __int64 r = 0;
    for (int eye = 1; eye <= 2; ++eye) {
        Viewport v = s->viewport;
        v.x += eye == 2 ? half : 0;
        v.w = half;
        g_orig_set_viewport(&v);
        g_orig_bind_camera(c.ctx, eye == 1 ? left : right, c.w, c.h, c.block, c.a6, c.slot);
        r = g_orig_draw_list(drawer, drawCtx, list, a4, a5);
    }
    // the node's own state again, for whatever it does next
    g_orig_set_viewport(&s->viewport);
    g_orig_bind_camera(c.ctx, c.camera, c.w, c.h, c.block, c.a6, c.slot);
    ++CyberpunkVR_DebugDrawStereoLists;
    return r;
}

}  // namespace

CVR_DETOUR("[debugdraw-stereo] debug fragments work sub_142EA16B4", DEBUG_FRAGMENTS_WORK_RVA, Detour_DebugFragmentsWork,
           g_orig_work)
CVR_DETOUR("[debugdraw-stereo] set viewport sub_140333A44", SET_VIEWPORT_RVA, Detour_SetViewport, g_orig_set_viewport)
CVR_DETOUR("[debugdraw-stereo] bind camera sub_140297904", BIND_CAMERA_RVA, Detour_BindCamera, g_orig_bind_camera)
CVR_DETOUR("[debugdraw-stereo] draw fragment list sub_14229D450", DRAW_FRAGMENT_LIST_RVA, Detour_DrawFragmentList,
           g_orig_draw_list)

}  // namespace cvr::detail
