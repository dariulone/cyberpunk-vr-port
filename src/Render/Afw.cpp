#include "Render/Afw.hpp"
#include "Render/AfwPlugin.hpp"
#include "Render/Osr.hpp"
#include "Core/Paths.hpp"
#include "Framegen/Inputs.hpp"
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <mutex>

extern void Log(const char*, ...);

extern "C" {
// Diagnostics: [0] frames warped, [1] fallbacks, [2] plugin state (0 not tried, 1 loaded, -1 failed); of the warped
// frames whose renderer frame directly follows the last warped one, [3] drawn for the other eye than that one, [4] for
// the same eye; [5] warped frames after a gap in the renderer's frames.
__declspec(dllexport) std::atomic<int64_t> CyberpunkVR_AfwStats[6]{};
// Tuning, read every frame (tools write it into the running game): [0] the warp mode (pd::FrameWarpMode), [1] the
// motion vectors' type (pd::MVType; 3 = from the frame before: Normal after a frame drawn for the same eye, else
// FromOtherEye), [2] IgnoreMotionThreshold x100, [3] the plugin's debug view, [4] clear before
// warping. [5] motion vectors off (InMotionScale 0). [6] the reconstruction: 0 PureDark's plugin, 1 the port's own
// OSR (Render/Osr.hpp; also whenever the plugin is not there), the default: on still frames 35.7 against the plugin's
// 33.8 dB at the worst and 37.8 against 36.3 on average, at 0.44 against 1.59 ms of GPU (docs/osr-20261104.md).
// [7] OSR's depth jitter: 0 subtracted (the measured best), 1 added, 2 none.
__declspec(dllexport) int32_t CyberpunkVR_AfwTune[8] = {3, 3, 250, 0, 0, 0, 1, 0};
// Extra factor on the motion vectors' scale, x and y (-1 flips one).
__declspec(dllexport) float CyberpunkVR_AfwMotionScale[2] = {1.0f, 1.0f};
}

namespace cvr::afw {
namespace {

using Microsoft::WRL::ComPtr;

bool g_launch = false;

struct Plugin {
    HMODULE module = nullptr;
    pd::InitDeviceFn initDevice = nullptr;
    pd::InitFrameWarpFn initWarp = nullptr;
    pd::EvaluateFrameWarpFn evaluate = nullptr;
    pd::D3D12RendererAPI* api = nullptr;
    ID3D12Device* device = nullptr;
    bool failed = false;
    // The plugin's eye buffers (InitFrameWarp), sized to the eyes.
    pd::EyeFrameBuffers eyes{};
    uint32_t width = 0, height = 0;
    // The drawn eye's depth and motion, one pair per eye kept across frames (the plugin reads the other eye's from
    // the frame before), sized to the inputs.
    pd::TextureDesc depth[2], motion[2];
    uint32_t depthW = 0, depthH = 0, motionW = 0, motionH = 0;
    DXGI_FORMAT depthFormat = DXGI_FORMAT_UNKNOWN, motionFormat = DXGI_FORMAT_UNKNOWN;
    Stats stats;
} g;

void Fail(const char* why) {
    g.failed = true;
    g.stats.last = why;
    CyberpunkVR_AfwStats[2].store(-1, std::memory_order_relaxed);
    Log("[afw] %s: the launch stays one picture for both eyes\n", why);
}

bool Load(ID3D12Device* device, ID3D12CommandQueue* queue) {
    if (g.api) return g.device == device;
    if (g.failed) return false;
    const std::wstring path = cvr::paths::File(cvr::paths::Folder::Root, L"PDAFWPlugin.dll");
    g.module = LoadLibraryW(path.c_str());
    if (!g.module) {
        Log("[afw] PDAFWPlugin.dll not found at %ls (error %lu) -- it comes with PureDark's UEVR AFW release\n",
            path.c_str(), GetLastError());
        Fail("no PDAFWPlugin.dll");
        return false;
    }
    g.initDevice = reinterpret_cast<pd::InitDeviceFn>(GetProcAddress(g.module, "InitDevice"));
    g.initWarp = reinterpret_cast<pd::InitFrameWarpFn>(GetProcAddress(g.module, "InitFrameWarp"));
    g.evaluate = reinterpret_cast<pd::EvaluateFrameWarpFn>(GetProcAddress(g.module, "EvaluateFrameWarp"));
    if (!g.initDevice || !g.initWarp || !g.evaluate) {
        Fail("PDAFWPlugin.dll lacks InitDevice / InitFrameWarp / EvaluateFrameWarp");
        return false;
    }
    // The adapter the game's device runs on.
    ComPtr<IDXGIFactory4> factory;
    ComPtr<IDXGIAdapter1> adapter;
    if (SUCCEEDED(CreateDXGIFactory2(0, IID_PPV_ARGS(&factory))))
        factory->EnumAdapterByLuid(device->GetAdapterLuid(), IID_PPV_ARGS(&adapter));
    pd::DeviceParams params{};
    params.d3d12Device = device;
    params.d3d12Queue = queue;
    params.dxgiAdapter = adapter.Get();
    g.api = g.initDevice(params);
    if (!g.api) {
        Fail("InitDevice failed");
        return false;
    }
    adapter.Detach();   // the plugin keeps it
    g.device = device;
    CyberpunkVR_AfwStats[2].store(1, std::memory_order_relaxed);
    Log("[afw] PDAFWPlugin.dll loaded from %ls\n", path.c_str());
    return true;
}

bool EnsureEyes(uint32_t width, uint32_t height) {
    if (g.width == width && g.height == height && g.eyes.eyeFrameBuffers[0].color.pTexture) return true;
    pd::FrameWarpInitParams params{};
    params.hmdWidth = int(width);
    params.hmdHeight = int(height);
    params.eyeFormat = DXGI_FORMAT_R8G8B8A8_UNORM;
    params.backbufferFormat = DXGI_FORMAT_R8G8B8A8_UNORM;
    g.eyes = g.initWarp(params);
    if (!g.eyes.eyeFrameBuffers[0].color.pTexture || !g.eyes.eyeFrameBuffers[1].color.pTexture) {
        Fail("InitFrameWarp gave no eye buffers");
        return false;
    }
    g.width = width;
    g.height = height;
    Log("[afw] eye buffers %ux%u\n", width, height);
    return true;
}

DXGI_FORMAT Typed(DXGI_FORMAT f) {
    switch (f) {
    case DXGI_FORMAT_R32_TYPELESS: return DXGI_FORMAT_R32_FLOAT;
    case DXGI_FORMAT_R16G16_TYPELESS: return DXGI_FORMAT_R16G16_FLOAT;
    default: return f;
    }
}

bool EnsureInput(pd::TextureDesc (&textures)[2], uint32_t& w, uint32_t& h, DXGI_FORMAT& format, ID3D12Resource* source) {
    const auto desc = source->GetDesc();
    const DXGI_FORMAT want = Typed(desc.Format);
    if (textures[0].pTexture && w == uint32_t(desc.Width) && h == desc.Height && format == want) return true;
    for (auto& t : textures) {
        if (t.pTexture) t.pTexture->Release();
        t = pd::TextureDesc{};
        if (!g.api->CreateTexture(int(desc.Width), int(desc.Height), want, D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE, t, true))
            return false;
    }
    w = uint32_t(desc.Width);
    h = desc.Height;
    format = want;
    return true;
}

void Barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* r, D3D12_RESOURCE_STATES a, D3D12_RESOURCE_STATES b) {
    if (a == b || !r) return;
    D3D12_RESOURCE_BARRIER v{};
    v.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition = {r, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, a, b};
    list->ResourceBarrier(1, &v);
}

void CopyRegion(ID3D12GraphicsCommandList* list, ID3D12Resource* dst, ID3D12Resource* src) {
    const auto d = dst->GetDesc(), s = src->GetDesc();
    D3D12_TEXTURE_COPY_LOCATION to{dst, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    to.SubresourceIndex = 0;
    D3D12_TEXTURE_COPY_LOCATION from{src, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    from.SubresourceIndex = 0;
    D3D12_BOX box{0, 0, 0, UINT((std::min)(d.Width, s.Width)), (std::min)(d.Height, s.Height), 1};
    list->CopyTextureRegion(&to, 0, 0, 0, &from, &box);
}

// THE CAMERAS. Column-major 4x4 (m[col * 4 + row]); view space right-handed (x right, y up, -z forward) -- the
// handedness does not matter to the plugin as long as the matrices agree (tools/afw_tests/afw_replay.cpp); the
// projection's depth is the engine's own convention (reversed: near 1, far 0), with the depth texture as it is.
struct M4 { double m[16]{}; double& at(int r, int c) { return m[c * 4 + r]; } double at(int r, int c) const { return m[c * 4 + r]; } };

M4 Inverse(const M4& a) {
    const double* m = a.m;
    double inv[16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    double det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    M4 o;
    if (!(std::fabs(det) > 1e-300)) return o;
    det = 1.0 / det;
    for (int i = 0; i < 16; ++i) o.m[i] = inv[i] * det;
    return o;
}

pd::Mat4 ToMat(const M4& a) {
    pd::Mat4 o{};
    for (int i = 0; i < 16; ++i) o.m[i] = float(a.m[i]);
    return o;
}

M4 WorldToView(const float position[3], const cvr::framegen::CameraData& c) {
    const double ax[3][3] = {{c.right[0], c.right[1], c.right[2]},
                             {c.up[0], c.up[1], c.up[2]},
                             {-c.forward[0], -c.forward[1], -c.forward[2]}};
    M4 v;
    for (int r = 0; r < 3; ++r) {
        double t = 0;
        for (int k = 0; k < 3; ++k) {
            v.at(r, k) = ax[r][k];
            t += ax[r][k] * position[k];
        }
        v.at(r, 3) = -t;
    }
    v.at(3, 3) = 1;
    return v;
}

M4 ViewToClip(const XrFovf& fov, const cvr::framegen::CameraData& c) {
    const double l = std::tan(fov.angleLeft), r = std::tan(fov.angleRight), u = std::tan(fov.angleUp), d = std::tan(fov.angleDown);
    const double n = c.nearPlane > 0 ? c.nearPlane : 0.02, f = c.farPlane > n ? c.farPlane : 16000.0;
    // depth = A + B / z for the forward distance z
    double A, B;
    if (c.infinite) { if (c.inverted) { A = 0; B = n; } else { A = 1; B = -n; } }
    else if (c.inverted) { B = n * f / (f - n); A = -n / (f - n); }
    else { B = -n * f / (f - n); A = f / (f - n); }
    M4 p;   // view z is backward: forward distance = -z
    p.at(0, 0) = 2.0 / (r - l);
    p.at(0, 2) = (r + l) / (r - l);
    p.at(1, 1) = 2.0 / (u - d);
    p.at(1, 2) = (u + d) / (u - d);
    p.at(2, 2) = -A;
    p.at(2, 3) = B;
    p.at(3, 2) = -1;
    return p;
}

}  // namespace

void SetLaunch(bool on) { g_launch = on; }
bool Launch() { return g_launch; }
// Without the plugin the port's own reconstruction makes the other eye: the launch takes turns either way.
bool Active() { return g_launch; }

bool UsesOsr() { return CyberpunkVR_AfwTune[6] != 0 || g.failed; }

int FrameEye() {
    // The eye turns over whenever the renderer's clock has moved since the last ask: every new frame the camera is
    // placed for, whatever the clock's step. Asked on the game thread only (Hooks/PatchCamera.cpp), as often as
    // MAIN's camera is written in one frame -- all of those writes get the same eye.
    static uint32_t s_clock = 0;
    static int s_eye = 1;
    uint32_t clock = 0;
    if (cvr::framegen::ReadRenderFrameIndex(&clock) && clock != s_clock) {
        s_clock = clock;
        s_eye ^= 1;
    }
    return s_eye;
}

Stats Statistics() { return g.stats; }

void SetMode(int mode) {
    if (mode != 0) CyberpunkVR_AfwTune[6] = mode == kModeOsr ? 1 : 0;
}

void Fallback(const char* why) {
    ++g.stats.fallbacks;
    // Logged when the reason changes and then at powers of two of its run, so a steady fallback does not fill the log.
    static uint64_t s_run = 0;
    s_run = (why == g.stats.last) ? s_run + 1 : 1;
    if ((s_run & (s_run - 1)) == 0)
        Log("[afw] fallback x%llu: %s (warped %llu so far)\n", static_cast<unsigned long long>(s_run), why,
            static_cast<unsigned long long>(g.stats.warped));
    g.stats.last = why;
    CyberpunkVR_AfwStats[1].fetch_add(1, std::memory_order_relaxed);
}

bool Warp(const WarpRequest& q) {
    const auto fallback = [](const char* why) {
        Fallback(why);
        return false;
    };
    if (!Active() || !q.device || !q.queue || !q.list || !q.drawn || !q.inputs || !q.out || q.eye > 1) return fallback("no request");
    const auto& in = *q.inputs;
    if (!in.hasDepth || !in.hasMotion || !in.depth || !in.motion) return fallback("no depth or motion");
    // The inputs were copied on the game's queue ahead of this list: same-queue order is their readiness, as for the
    // depth layer (Render/StereoDepth.cpp). No wait on another queue is introduced here.
    if (in.queue != q.queue || !in.submitted) return fallback("inputs on another queue or not submitted");
    const auto& cam = in.camera;
    const uint32_t n = q.eye, other = n ^ 1u;
    const float side = n == 0 ? 1.0f : -1.0f;   // from the left eye the right one is to the right
    const float otherPos[3] = {cam.position[0] + side * q.ipd * cam.right[0], cam.position[1] + side * q.ipd * cam.right[1],
                               cam.position[2] + side * q.ipd * cam.right[2]};
    if (UsesOsr()) {
        cvr::osr::Request r{};
        r.device = q.device;
        r.list = q.list;
        r.drawn = q.drawn;
        r.depth = in.depth.Get();
        r.eye = n;
        r.out = q.out;
        r.history = CyberpunkVR_AfwTune[0] != 1;   // "alternate eye" mode: the stereo warp alone
        r.debug = CyberpunkVR_AfwTune[3] ? 1u : 0u;
        r.jitterSign = CyberpunkVR_AfwTune[7] == 1 ? 1.0f : CyberpunkVR_AfwTune[7] == 2 ? 0.0f : -1.0f;
        for (cvr::osr::Camera* c : {&r.source, &r.dest}) {
            for (int i = 0; i < 3; ++i) {
                c->position[i] = cam.position[i];
                c->right[i] = cam.right[i];
                c->up[i] = cam.up[i];
                c->forward[i] = cam.forward[i];
            }
            c->nearPlane = cam.nearPlane;
            c->farPlane = cam.farPlane;
            c->inverted = cam.inverted;
            c->infinite = cam.infinite;
            c->jitter[0] = cam.jitter[0];
            c->jitter[1] = cam.jitter[1];
        }
        for (int i = 0; i < 3; ++i) r.dest.position[i] = otherPos[i];
        r.source.fov = q.fov[n];
        r.dest.fov = q.fov[other];
        if (!cvr::osr::Record(r)) return fallback("OSR could not record");
        ++g.stats.warped;
        CyberpunkVR_AfwStats[0].fetch_add(1, std::memory_order_relaxed);
        return true;
    }
    if (!Load(q.device, q.queue)) return fallback("no plugin");
    const auto outDesc = q.out->GetDesc();
    if (!EnsureEyes(uint32_t(outDesc.Width), outDesc.Height)) return fallback("no eye buffers");
    if (!EnsureInput(g.depth, g.depthW, g.depthH, g.depthFormat, in.depth.Get()) ||
        !EnsureInput(g.motion, g.motionW, g.motionH, g.motionFormat, in.motion.Get()))
        return fallback("no input copies");
    pd::TextureDesc& color = g.eyes.eyeFrameBuffers[n].color;
    pd::TextureDesc& made = g.eyes.eyeFrameBuffers[other].color;
    auto* list = q.list;

    // the drawn eye, its depth and motion, into the plugin's buffers
    Barrier(list, q.drawn, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list, color.pTexture, color.initialState, D3D12_RESOURCE_STATE_COPY_DEST);
    CopyRegion(list, color.pTexture, q.drawn);
    Barrier(list, color.pTexture, D3D12_RESOURCE_STATE_COPY_DEST, color.initialState);
    Barrier(list, q.drawn, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
    const struct { ID3D12Resource* source; pd::TextureDesc* target; } inputs[2] = {
        {in.depth.Get(), &g.depth[n]}, {in.motion.Get(), &g.motion[n]}};
    for (const auto& i : inputs) {
        Barrier(list, i.source, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
        Barrier(list, i.target->pTexture, i.target->initialState, D3D12_RESOURCE_STATE_COPY_DEST);
        list->CopyResource(i.target->pTexture, i.source);
        Barrier(list, i.target->pTexture, D3D12_RESOURCE_STATE_COPY_DEST, i.target->initialState);
        Barrier(list, i.source, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }

    // the two cameras: the drawn eye where it was drawn, the other one IPD along the camera's right
    const M4 sv = WorldToView(cam.position, cam), dv = WorldToView(otherPos, cam);
    const M4 sp = ViewToClip(q.fov[n], cam), dp = ViewToClip(q.fov[other], cam);
    // one per eye and kept: the plugin may read the other eye's from the frame before, as UEVR keeps them
    static pd::CameraData cameras[2]{};
    pd::CameraData& camera = cameras[n];
    camera.srcWorldToViewMatrix = ToMat(sv);
    camera.srcViewToWorldMatrix = ToMat(Inverse(sv));
    camera.srcViewToClipMatrix = ToMat(sp);
    camera.srcClipToViewMatrix = ToMat(Inverse(sp));
    camera.destWorldToViewMatrix = ToMat(dv);
    camera.destViewToWorldMatrix = ToMat(Inverse(dv));
    camera.destViewToClipMatrix = ToMat(dp);
    camera.destClipToViewMatrix = ToMat(Inverse(dp));
    M4 id;
    for (int i = 0; i < 4; ++i) id.at(i, i) = 1;
    camera.camWorldToViewMatrix = camera.camViewToWorldMatrix = camera.camViewToClipMatrix = camera.camClipToViewMatrix = ToMat(id);

    static pd::FrameBufferDesc frame;
    frame.color = color;
    frame.depth = g.depth[n];
    frame.motionVectors = g.motion[n];
    pd::FrameWarpEvaluateParams p{};
    p.InCmdList = list;
    p.InEyeFrameBuffer = &frame;
    p.IsHudlessColor = true;
    p.IgnoreMotionThreshold = float(CyberpunkVR_AfwTune[2]) / 100.0f;
    p.Debug = CyberpunkVR_AfwTune[3] != 0;
    p.ClearBeforeWarping = CyberpunkVR_AfwTune[4] != 0;
    // the engine's vectors in a frame the eyes take turns in point at the OTHER eye's previous frame
    // THE ENGINE'S MOTION VECTORS POINT AT ITS PREVIOUS FRAME, whichever eye that was drawn for: the other eye when
    // the eyes took turns (FromOtherEye), this eye when the turn did not come round -- two camera placements under
    // one move of the renderer's clock (FrameEye). The frame before is known when it was warped too.
    static uint32_t s_lastFrame = 0;
    static int s_lastEye = -1;
    const bool follows = s_lastEye >= 0 && in.frameId == s_lastFrame + 1;
    const bool sameEye = follows && uint32_t(s_lastEye) == n;
    CyberpunkVR_AfwStats[follows ? (sameEye ? 4 : 3) : 5].fetch_add(1, std::memory_order_relaxed);
    s_lastFrame = in.frameId;
    s_lastEye = int(n);
    const int mvType = CyberpunkVR_AfwTune[1];
    p.MotionVectorsType = mvType == 3 ? (sameEye ? pd::Normal : pd::FromOtherEye)
                                      : pd::MVType(mvType >= 0 && mvType <= 2 ? mvType : 1);
    const float mvOn = CyberpunkVR_AfwTune[5] ? 0.0f : 1.0f;
    p.InMotionScale[0] = mvOn * CyberpunkVR_AfwMotionScale[0] * cam.motionScale[0] * float(outDesc.Width);
    p.InMotionScale[1] = mvOn * CyberpunkVR_AfwMotionScale[1] * cam.motionScale[1] * float(outDesc.Height);
    const int mode = CyberpunkVR_AfwTune[0];
    p.Mode = pd::FrameWarpMode(mode >= 1 && mode <= 3 ? mode : 3);
    p.EyeIndex = pd::EyeIndex(n);
    p.CameraData = &camera;
    g.evaluate(p);

    // the made eye into the second eye's slot
    Barrier(list, made.pTexture, made.initialState, D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list, q.out, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
    CopyRegion(list, q.out, made.pTexture);
    Barrier(list, q.out, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list, made.pTexture, D3D12_RESOURCE_STATE_COPY_SOURCE, made.initialState);
    ++g.stats.warped;
    CyberpunkVR_AfwStats[0].fetch_add(1, std::memory_order_relaxed);
    if (g.stats.warped == 1 || g.stats.warped % 5000 == 0)
        Log("[afw] warped %llu (eye %u made from %u; colour %ux%u, depth %ux%u fmt %d, motion %ux%u fmt %d, scale %.4f %.4f)\n",
            static_cast<unsigned long long>(g.stats.warped), other, n, unsigned(outDesc.Width), unsigned(outDesc.Height),
            g.depthW, g.depthH, int(g.depthFormat), g.motionW, g.motionH, int(g.motionFormat), cam.motionScale[0],
            cam.motionScale[1]);
    return true;
}

}  // namespace cvr::afw
