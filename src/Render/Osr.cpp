#include "Render/Osr.hpp"
#include <d3dcompiler.h>
#include <wrl/client.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <cstring>

extern void Log(const char*, ...);

extern "C" {
// Diagnostics: [0] frames reconstructed, [1] failed, [2] frames that used the made eye's history.
__declspec(dllexport) std::atomic<int64_t> CyberpunkVR_OsrStats[3]{};
// Tuning, read every frame: [0] depth agreement tolerance x1000 (relative distance, default 30 = 3%), [1] the
// farthest a fill searches along a row or a column for the hole's bounds (pixels; the hands at 30-40 cm open holes
// of 150-250 px, and a run that reached only the hand's side filled them with the hand's colour), [2] the nearest a
// surface of the made eye's previous frame may be when this frame's drawn eye cannot confirm it (cm, 0 = no limit;
// see StillThere), [3] how far a sample may sit from a nearer surface's place in the made eye before it counts as
// that surface's edge (made-eye pixels x100, 0 = off): see Trace.
__declspec(dllexport) int32_t CyberpunkVR_OsrTune[4] = {30, 384, 100, 75};
}

namespace cvr::osr {
namespace {
using Microsoft::WRL::ComPtr;

// THE FOUR PASSES (cs_5_0). Depth texels and colour pixels share one convention: uv in 0..1, v down; a view ray is
// forward + tx * right + ty * up with tx, ty the tangents across the eye's OpenXR window (left..right, up..down).
// The depth is reversed with depth = A + B / z for the forward distance z; a sky texel is 0.
constexpr char kShader[] = R"(
cbuffer C : register(b0) {
    float4 sPos, sRight, sUp, sFwd, sTan;
    float4 dPos, dRight, dUp, dFwd, dTan;
    float4 hPos, hRight, hUp, hFwd, hTan;
    float4 depthAB;   // A, B, tolerance, history valid
    uint4 sizes;      // depth w, h; colour W, H
    float4 jit;       // source jitter (depth texels), history jitter
    uint4 flags;      // debug, pass (0 stereo, 1 history), longest fill run, history near limit (cm)
    float4 edge;      // edge distance (made-eye pixels), made-eye pixels per unit tangent
};
Texture2D<float4> SrcColor : register(t0);
Texture2D<float>  SrcDepth : register(t1);
Texture2D<float4> HistColor : register(t2);
Texture2D<float>  HistDepth : register(t3);
Texture2D<float4> ResColor : register(t4);
Texture2D<float>  ResDepth : register(t5);
RWTexture2D<uint> StereoT : register(u0);
RWTexture2D<uint> HistT : register(u1);
RWTexture2D<float4> OutColor : register(u2);
RWTexture2D<float> OutDepth : register(u3);
RWTexture2D<float4> Final : register(u4);
RWTexture2D<float> NearD : register(u5);   // the drawn eye's nearest depth over texels -1..+2 (Clear, read by Trace)
SamplerState Lin : register(s0);

static const uint kSky = 1;   // below every real depth's bits: geometry wins over sky in the max
float ZOf(float d) { return depthAB.y / max(d - depthAB.x, 1e-12); }
float DOf(float z) { return depthAB.x + depthAB.y / z; }
float2 TanOf(float2 uv, float4 t) { return float2(lerp(t.x, t.y, uv.x), lerp(t.z, t.w, uv.y)); }
float2 UvOf(float2 tn, float4 t) { return float2((tn.x - t.x) / (t.y - t.x), (tn.y - t.z) / (t.w - t.z)); }

[numthreads(8, 8, 1)]
void Clear(uint3 id : SV_DispatchThreadID) {
    if (id.x >= sizes.x || id.y >= sizes.y) return;
    StereoT[id.xy] = 0;
    HistT[id.xy] = 0;
    float nearest = 0;
    [unroll] for (int k = 0; k < 16; ++k)
        nearest = max(nearest, SrcDepth[clamp(int2(id.xy) + int2((k & 3) - 1, (k >> 2) - 1), int2(0, 0), int2(sizes.xy) - 1)]);
    NearD[id.xy] = nearest;
}

// Every depth texel of the drawn eye (pass 0) or of the made eye's previous frame (pass 1) forward into the made
// eye's depth grid; each cell keeps the nearest that lands on it (2x2 footprint, so no cracks open between texels).
[numthreads(8, 8, 1)]
void Splat(uint3 id : SV_DispatchThreadID) {
    if (id.x >= sizes.x || id.y >= sizes.y) return;
    const bool hist = flags.y != 0;
    const float d = hist ? HistDepth[id.xy] : SrcDepth[id.xy];
    const float3 P0 = hist ? hPos.xyz : sPos.xyz;
    const float3 R = hist ? hRight.xyz : sRight.xyz, U = hist ? hUp.xyz : sUp.xyz, F = hist ? hFwd.xyz : sFwd.xyz;
    const float4 T = hist ? hTan : sTan;
    const float2 j = hist ? jit.zw : jit.xy;
    const float2 uv = (float2(id.xy) + 0.5 + j) / float2(sizes.xy);
    const float2 tn = TanOf(uv, T);
    const float3 dir = F + tn.x * R + tn.y * U;
    float3 rel;
    if (d <= 0) rel = dir;                                  // sky: a direction, no parallax
    else rel = P0 + ZOf(d) * dir - dPos.xyz;
    const float zd = dot(rel, dFwd.xyz);
    if (zd <= 1e-4) return;
    const float2 uvd = UvOf(float2(dot(rel, dRight.xyz), dot(rel, dUp.xyz)) / zd, dTan);
    const uint value = d <= 0 ? kSky : asuint(max(DOf(zd), 2e-38));
    const int2 b = int2(floor(uvd * float2(sizes.xy) - 0.5));
    [unroll] for (int k = 0; k < 4; ++k) {
        const int2 c = b + int2(k & 1, k >> 1);
        if (c.x < 0 || c.y < 0 || c.x >= (int)sizes.x || c.y >= (int)sizes.y) continue;
        uint before;
        if (hist) InterlockedMax(HistT[c], value, before);
        else InterlockedMax(StereoT[c], value, before);
    }
}

// Whether a surface of the made eye's previous frame, at world point W, is still there: where this frame's drawn eye
// sees that place it must not see past it (beyond the depth tolerance, on all four texels around it: at a silhouette
// the footprint takes in the background too, and failing there pushed every edge of the scene to the fill -- the
// edges then shimmered as the eyes took turns). The hands, the weapon, the body and anyone walking by moved on their
// own and fail -- a frame-old copy of them, reprojected by the camera alone, was the second pair of hands in the holes
// the hands open; still scenery passes. Where the drawn eye cannot tell -- the place hidden behind something nearer,
// or out of its view -- a surface nearer than the near limit (flags.w cm) is taken for the player's own hands, weapon
// or body, which can hide their own old place (the piece of an old forearm by the elbow): it fails too.
bool StillThere(float3 W) {
    const float3 rel = W - sPos.xyz;
    const float zs = dot(rel, sFwd.xyz);
    const bool farEnough = zs >= flags.w * 0.01;
    if (zs <= 1e-4) return farEnough;
    const float2 uvs = UvOf(float2(dot(rel, sRight.xyz), dot(rel, sUp.xyz)) / zs, sTan);
    if (any(uvs < 0) || any(uvs > 1)) return farEnough;
    const int2 b = int2(floor(uvs * float2(sizes.xy) - 0.5 - jit.xy));
    bool seen = false, hidden = false;
    [unroll] for (int k = 0; k < 4; ++k) {
        const int2 c = clamp(b + int2(k & 1, k >> 1), int2(0, 0), int2(sizes.xy) - 1);
        const float ds = SrcDepth[c];
        if (ds <= 0) continue;
        const float z = ZOf(ds);
        if (z > zs * (1 + depthAB.z)) continue;
        if (z >= zs * (1 - depthAB.z)) seen = true;
        else hidden = true;
    }
    return seen || (hidden && farEnough);
}

// The made eye's ray at a splatted depth, traced into the drawn eye (hist false) or the made eye's previous frame:
// taken when the depth there agrees (sky with sky).
bool Trace(uint v, float3 dir, bool hist, out float4 colour) {
    colour = 0;
    const float3 P0 = hist ? hPos.xyz : sPos.xyz;
    const float3 R = hist ? hRight.xyz : sRight.xyz, U = hist ? hUp.xyz : sUp.xyz, F = hist ? hFwd.xyz : sFwd.xyz;
    const float4 T = hist ? hTan : sTan;
    const float3 rel = v == kSky ? dir : dPos.xyz + ZOf(asfloat(v)) * dir - P0;
    const float zx = dot(rel, F);
    if (zx <= 1e-4) return false;
    const float2 uvx = UvOf(float2(dot(rel, R), dot(rel, U)) / zx, T);
    if (any(uvx < 0) || any(uvx > 1)) return false;
    const float2 j = hist ? jit.zw : jit.xy;
    const int2 b = int2(floor(uvx * float2(sizes.xy) - 0.5 - j));
    bool ok = false;
    [unroll] for (int k = 0; k < 4; ++k) {
        const int2 c = clamp(b + int2(k & 1, k >> 1), int2(0, 0), int2(sizes.xy) - 1);
        const float ds = hist ? HistDepth[c] : SrcDepth[c];
        if (v == kSky) ok = ok || ds <= 0;
        else if (ds > 0) ok = ok || abs(ZOf(ds) - zx) <= depthAB.z * zx;
    }
    if (!ok) return false;
    // A sample beside a nearer surface carries that surface's colour too (its anti-aliased edge, and the colour's
    // grid is finer than the depth's): taken as the farther surface, it drew a thin copy of the near outline where
    // the parallax had moved the near surface away (the outline beside the hands). A nearer texel within reach of
    // the sample (+-1.5 depth texels: the bilinear footprint and the colour edge's offset from the depth edge)
    // makes it a hole for the fill unless that surface lands within edge.x pixels of it in the made eye. In the
    // previous frame a nearer surface that is no longer there (StillThere) moved on its own, so it always counts.
    if (edge.x > 0 && !hist) {
        // the drawn eye: only the nearest of the 16 texels can decide, made once per texel in Clear
        const float dn = NearD[clamp(b, int2(0, 0), int2(sizes.xy) - 1)];
        if (dn > 0) {
            const float invZ = v == kSky ? 0 : 1 / zx;
            const float inv = 1 / ZOf(dn);
            if (inv > invZ * (1 + depthAB.z) && (inv - invZ) * length(P0 - dPos.xyz) * edge.y > edge.x) return false;
        }
    }
    if (edge.x > 0 && hist) {
        const float invZ = v == kSky ? 0 : 1 / zx;
        const float pxPerInvZ = length(P0 - dPos.xyz) * edge.y;
        [loop] for (int e = 0; e < 16; ++e) {
            const int2 c = clamp(b + int2((e & 3) - 1, (e >> 2) - 1), int2(0, 0), int2(sizes.xy) - 1);
            const float ds = HistDepth[c];
            if (ds <= 0) continue;
            const float zt = ZOf(ds);
            const float inv = 1 / zt;
            if (inv <= invZ * (1 + depthAB.z)) continue;
            if ((inv - invZ) * pxPerInvZ > edge.x) return false;
            const float2 tt = TanOf((float2(c) + 0.5 + jit.zw) / float2(sizes.xy), hTan);
            if (!StillThere(hPos.xyz + zt * (hFwd.xyz + tt.x * hRight.xyz + tt.y * hUp.xyz))) return false;
        }
    }
    colour = hist ? HistColor.SampleLevel(Lin, uvx, 0) : SrcColor.SampleLevel(Lin, uvx, 0);
    colour.a = 1;
    return true;
}

// Per pixel of the made eye: its cell's splatted depth (else the farthest, then the nearest of the 3x3 -- at a
// silhouette the background behind the footprint's halo, in thin geometry the strand) traced into the drawn eye,
// then the same with the made eye's previous frame; what neither accounts for is a hole (alpha 0).
[numthreads(8, 8, 1)]
void Resolve(uint3 id : SV_DispatchThreadID) {
    if (id.x >= sizes.z || id.y >= sizes.w) return;
    const float2 uv = (float2(id.xy) + 0.5) / float2(sizes.zw);
    const int2 c = min(int2(uv * float2(sizes.xy)), int2(sizes.xy) - 1);
    const float2 tn = TanOf(uv, dTan);
    const float3 dir = dFwd.xyz + tn.x * dRight.xyz + tn.y * dUp.xyz;
    [loop] for (uint trip = 0; trip < 2; ++trip) {
        const bool hist = trip == 1;
        if (hist && depthAB.w <= 0) break;
        uint own = hist ? HistT[c] : StereoT[c], lo = 0xffffffff, hi = 0;
        [unroll] for (int oy = -1; oy <= 1; ++oy)
            [unroll] for (int ox = -1; ox <= 1; ++ox) {
                const int2 q = clamp(c + int2(ox, oy), int2(0, 0), int2(sizes.xy) - 1);
                const uint v = hist ? HistT[q] : StereoT[q];
                if (v != 0) { lo = min(lo, v); hi = max(hi, v); }
            }
        const uint cand[3] = {own, lo == 0xffffffff ? 0 : lo, hi};
        [loop] for (int i = 0; i < 3; ++i) {
            if (cand[i] == 0) continue;
            // The previous frame fills with what is still there only (StillThere); such a hole goes to the fill.
            if (hist && cand[i] != kSky && !StillThere(dPos.xyz + ZOf(asfloat(cand[i])) * dir)) continue;
            float4 colour;
            if (Trace(cand[i], dir, hist, colour)) {
                if (hist && (flags.x & 1)) colour.rgb = lerp(colour.rgb, float3(0, 1, 0), 0.35);
                OutColor[id.xy] = colour;
                OutDepth[id.xy] = cand[i] == kSky ? 0 : asfloat(cand[i]);
                return;
            }
        }
    }
    OutColor[id.xy] = 0;
    OutDepth[id.xy] = 0;
}

// The first resolved pixel from p along dirv within the run (0 = none): one pixel at a time near p, then in strides
// of 4 (a hole's far bound can lie a couple of hundred pixels away), stepping back to the exact bound.
int Bound(int2 p, int2 dirv, int run) {
    int k = 1, last = 0;
    [loop] while (k <= run) {
        const int2 q = p + dirv * k;
        if (q.x < 0 || q.y < 0 || q.x >= (int)sizes.z || q.y >= (int)sizes.w) return 0;
        if (ResColor[q].a > 0.5) {
            int at = k;
            [loop] for (int i = k - 1; i > last && ResColor[p + dirv * i].a > 0.5; --i) at = i;
            return at;
        }
        last = k;
        k += k < 16 ? 1 : 4;
    }
    return 0;
}

// A hole is background this eye sees and the drawn eye does not: along the row, the farther of the two resolved
// pixels bounding it is the background; the hole takes the background mirrored across that edge (the pixel as far
// beyond the edge as the hole is before it), or the edge pixel where the mirror lands on something nearer. Rows the
// run cannot close take the farther of the column's bounds the same way; with neither closed, the farthest single
// bound found (a lone bound is often the near surface itself).
[numthreads(8, 8, 1)]
void Inpaint(uint3 id : SV_DispatchThreadID) {
    if (id.x >= sizes.z || id.y >= sizes.w) return;
    const float4 here = ResColor[id.xy];
    if (here.a > 0.5) { Final[id.xy] = float4(here.rgb, 1); return; }
    const int run = (int)flags.z;
    const int2 p = int2(id.xy);
    float4 colour = 0;
    bool found = false;
    int2 single = int2(-1, -1);
    float singleDepth = 1e30;
    [loop] for (uint axis = 0; axis < 2 && !found; ++axis) {
        const int2 step = axis == 0 ? int2(1, 0) : int2(0, 1);
        const int lo = Bound(p, -step, run), hi = Bound(p, step, run);
        if (lo == 0 && hi == 0) continue;
        if (lo == 0 || hi == 0) {
            const int2 q = lo != 0 ? p - step * lo : p + step * hi;
            if (ResDepth[q] < singleDepth) { singleDepth = ResDepth[q]; single = q; }
            continue;
        }
        const bool useLo = ResDepth[p - step * lo] <= ResDepth[p + step * hi];
        const int dist = useLo ? lo : hi;
        const int2 dirv = useLo ? -step : step;
        const int2 edge = p + dirv * dist;
        const int2 mirror = edge + dirv * dist;
        const float edgeDepth = ResDepth[edge];
        colour = ResColor[edge];
        const bool inside = mirror.x >= 0 && mirror.y >= 0 && mirror.x < (int)sizes.z && mirror.y < (int)sizes.w;
        if (inside && ResColor[mirror].a > 0.5 && ResDepth[mirror] <= edgeDepth * 1.03 + 1e-9) colour = ResColor[mirror];
        found = true;
    }
    // debug tint: magenta closed run, cyan lone bound, yellow the whole-image search
    float3 tint = float3(1, 0, 1);
    if (!found && single.x >= 0) { colour = ResColor[single]; found = true; tint = float3(0, 1, 1); }
    // Nothing within the run (a hole against the image's edge): the farthest bound across the whole image. The drawn
    // eye's pixel at the same place is the last resort only: there it is mostly the near surface that opened the hole.
    if (!found) {
        const int reach = (int)max(sizes.z, sizes.w);
        [loop] for (uint d = 0; d < 4; ++d) {
            const int2 dirv = d == 0 ? int2(-1, 0) : d == 1 ? int2(1, 0) : d == 2 ? int2(0, -1) : int2(0, 1);
            const int k = Bound(p, dirv, reach);
            if (k == 0) continue;
            const int2 q = p + dirv * k;
            if (ResDepth[q] < singleDepth) { singleDepth = ResDepth[q]; single = q; }
        }
        if (single.x >= 0) { colour = ResColor[single]; found = true; tint = float3(1, 1, 0); }
    }
    if (!found) colour = SrcColor.SampleLevel(Lin, (float2(id.xy) + 0.5) / float2(sizes.zw), 0);
    if (flags.x & 1) colour.rgb = lerp(colour.rgb, tint, 0.5);
    Final[id.xy] = float4(colour.rgb, 1);
}
)";

struct Tex {
    ComPtr<ID3D12Resource> r;
    D3D12_RESOURCE_STATES state = D3D12_RESOURCE_STATE_COMMON;
};

struct Constants {
    float s[5][4], d[5][4], h[5][4];
    float depthAB[4];
    uint32_t sizes[4];
    float jit[4];
    uint32_t flags[4];
    float edge[4];
};
static_assert(sizeof(Constants) <= 512, "one constant block");

constexpr uint32_t kSlots = 3, kDescriptors = 12;

struct State {
    ID3D12Device* device = nullptr;
    bool failed = false;
    ComPtr<ID3D12RootSignature> root;
    ComPtr<ID3D12PipelineState> clear, splat, resolve, inpaint;
    ComPtr<ID3D12DescriptorHeap> heap;
    ComPtr<ID3D12Resource> constants;
    uint8_t* mapped = nullptr;
    uint32_t slot = 0, step = 0;
    uint32_t w = 0, h = 0, W = 0, H = 0;
    Tex histColor[2], histDepth[2];
    Camera histCamera[2];
    bool histValid[2]{};
    Tex stereoT, histT, resColor, resDepth, final, nearD;
    Stats stats;
} g;

void Fail(const char* why) {
    g.failed = true;
    g.stats.last = why;
    Log("[osr] %s\n", why);
}

void Barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* r, D3D12_RESOURCE_STATES a, D3D12_RESOURCE_STATES b) {
    if (a == b || !r) return;
    D3D12_RESOURCE_BARRIER v{};
    v.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition = {r, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, a, b};
    list->ResourceBarrier(1, &v);
}
void To(ID3D12GraphicsCommandList* list, Tex& t, D3D12_RESOURCE_STATES s) {
    Barrier(list, t.r.Get(), t.state, s);
    t.state = s;
}
void UavBarrier(ID3D12GraphicsCommandList* list) {
    D3D12_RESOURCE_BARRIER v{};
    v.Type = D3D12_RESOURCE_BARRIER_TYPE_UAV;
    list->ResourceBarrier(1, &v);
}

bool Make(Tex& t, uint32_t width, uint32_t height, DXGI_FORMAT format, bool uav) {
    D3D12_RESOURCE_DESC d{};
    d.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    d.Width = width;
    d.Height = height;
    d.DepthOrArraySize = 1;
    d.MipLevels = 1;
    d.Format = format;
    d.SampleDesc.Count = 1;
    d.Flags = uav ? D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS : D3D12_RESOURCE_FLAG_NONE;
    D3D12_HEAP_PROPERTIES heap{};
    heap.Type = D3D12_HEAP_TYPE_DEFAULT;
    t.r.Reset();
    t.state = D3D12_RESOURCE_STATE_COMMON;
    return SUCCEEDED(g.device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, t.state, nullptr, IID_PPV_ARGS(&t.r)));
}

bool Init(ID3D12Device* device) {
    if (g.device == device && g.root && g.clear && g.splat && g.resolve && g.inpaint) return true;
    if (g.failed) return false;
    g = State{};
    g.device = device;
    D3D12_DESCRIPTOR_RANGE ranges[2]{};
    ranges[0] = {D3D12_DESCRIPTOR_RANGE_TYPE_SRV, 6, 0, 0, 0};
    ranges[1] = {D3D12_DESCRIPTOR_RANGE_TYPE_UAV, 6, 0, 0, 0};   // a table of its own, handed its first descriptor
    D3D12_ROOT_PARAMETER params[3]{};
    params[0].ParameterType = D3D12_ROOT_PARAMETER_TYPE_CBV;
    params[0].Descriptor.ShaderRegister = 0;
    params[1].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[1].DescriptorTable = {1, &ranges[0]};
    params[2].ParameterType = D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE;
    params[2].DescriptorTable = {1, &ranges[1]};
    D3D12_STATIC_SAMPLER_DESC sampler{};
    sampler.Filter = D3D12_FILTER_MIN_MAG_MIP_LINEAR;
    sampler.AddressU = sampler.AddressV = sampler.AddressW = D3D12_TEXTURE_ADDRESS_MODE_CLAMP;
    sampler.MaxLOD = D3D12_FLOAT32_MAX;
    D3D12_ROOT_SIGNATURE_DESC rs{3, params, 1, &sampler, D3D12_ROOT_SIGNATURE_FLAG_NONE};
    ComPtr<ID3DBlob> blob, error;
    if (FAILED(D3D12SerializeRootSignature(&rs, D3D_ROOT_SIGNATURE_VERSION_1, &blob, &error)) ||
        FAILED(device->CreateRootSignature(0, blob->GetBufferPointer(), blob->GetBufferSize(), IID_PPV_ARGS(&g.root)))) {
        Fail("root signature");
        return false;
    }
    const struct { const char* entry; ComPtr<ID3D12PipelineState>* pso; } kernels[] = {
        {"Clear", &g.clear}, {"Splat", &g.splat}, {"Resolve", &g.resolve}, {"Inpaint", &g.inpaint}};
    for (const auto& k : kernels) {
        ComPtr<ID3DBlob> code, err;
        if (FAILED(D3DCompile(kShader, sizeof(kShader) - 1, "osr.hlsl", nullptr, nullptr, k.entry, "cs_5_0",
                              D3DCOMPILE_OPTIMIZATION_LEVEL3, 0, &code, &err))) {
            Log("[osr] %s: %s\n", k.entry, err ? static_cast<const char*>(err->GetBufferPointer()) : "compile failed");
            Fail("shader compile");
            return false;
        }
        D3D12_COMPUTE_PIPELINE_STATE_DESC pd{};
        pd.pRootSignature = g.root.Get();
        pd.CS = {code->GetBufferPointer(), code->GetBufferSize()};
        if (FAILED(device->CreateComputePipelineState(&pd, IID_PPV_ARGS(k.pso->GetAddressOf())))) {
            Fail("pipeline");
            return false;
        }
    }
    D3D12_DESCRIPTOR_HEAP_DESC hd{D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, kSlots * kDescriptors,
                                  D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, 0};
    D3D12_RESOURCE_DESC cb{};
    cb.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    cb.Width = kSlots * 1024;
    cb.Height = cb.DepthOrArraySize = cb.MipLevels = 1;
    cb.SampleDesc.Count = 1;
    cb.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    D3D12_HEAP_PROPERTIES up{};
    up.Type = D3D12_HEAP_TYPE_UPLOAD;
    if (FAILED(device->CreateDescriptorHeap(&hd, IID_PPV_ARGS(&g.heap))) ||
        FAILED(device->CreateCommittedResource(&up, D3D12_HEAP_FLAG_NONE, &cb, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr,
                                               IID_PPV_ARGS(&g.constants))) ||
        FAILED(g.constants->Map(0, nullptr, reinterpret_cast<void**>(&g.mapped)))) {
        Fail("heap or constants");
        return false;
    }
    g.step = device->GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    Log("[osr] ready\n");
    return true;
}

bool EnsureSizes(uint32_t w, uint32_t h, uint32_t W, uint32_t H) {
    if (g.w == w && g.h == h && g.W == W && g.H == H && g.final.r) return true;
    bool ok = true;
    for (int e = 0; e < 2; ++e) {
        ok = ok && Make(g.histColor[e], W, H, DXGI_FORMAT_R8G8B8A8_UNORM, false) && Make(g.histDepth[e], w, h, DXGI_FORMAT_R32_FLOAT, false);
        g.histValid[e] = false;
    }
    ok = ok && Make(g.stereoT, w, h, DXGI_FORMAT_R32_UINT, true) && Make(g.histT, w, h, DXGI_FORMAT_R32_UINT, true) &&
         Make(g.resColor, W, H, DXGI_FORMAT_R8G8B8A8_UNORM, true) && Make(g.resDepth, W, H, DXGI_FORMAT_R32_FLOAT, true) &&
         Make(g.final, W, H, DXGI_FORMAT_R8G8B8A8_UNORM, true) && Make(g.nearD, w, h, DXGI_FORMAT_R32_FLOAT, true);
    if (!ok) { Fail("textures"); return false; }
    g.w = w; g.h = h; g.W = W; g.H = H;
    Log("[osr] depth %ux%u, eye %ux%u\n", w, h, W, H);
    return true;
}

void PutCamera(float (&out)[5][4], const Camera& c) {
    for (int i = 0; i < 3; ++i) {
        out[0][i] = c.position[i];
        out[1][i] = c.right[i];
        out[2][i] = c.up[i];
        out[3][i] = c.forward[i];
    }
    out[0][3] = out[1][3] = out[2][3] = out[3][3] = 0;
    out[4][0] = std::tan(c.fov.angleLeft);
    out[4][1] = std::tan(c.fov.angleRight);
    out[4][2] = std::tan(c.fov.angleUp);
    out[4][3] = std::tan(c.fov.angleDown);
}

void Srv(D3D12_CPU_DESCRIPTOR_HANDLE at, ID3D12Resource* r, DXGI_FORMAT f) {
    D3D12_SHADER_RESOURCE_VIEW_DESC d{};
    d.Format = f;
    d.ViewDimension = D3D12_SRV_DIMENSION_TEXTURE2D;
    d.Shader4ComponentMapping = D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING;
    d.Texture2D.MipLevels = 1;
    g.device->CreateShaderResourceView(r, &d, at);
}
void Uav(D3D12_CPU_DESCRIPTOR_HANDLE at, ID3D12Resource* r, DXGI_FORMAT f) {
    D3D12_UNORDERED_ACCESS_VIEW_DESC d{};
    d.Format = f;
    d.ViewDimension = D3D12_UAV_DIMENSION_TEXTURE2D;
    g.device->CreateUnorderedAccessView(r, nullptr, &d, at);
}

void CopyWhole(ID3D12GraphicsCommandList* list, ID3D12Resource* dst, ID3D12Resource* src) {
    const auto d = dst->GetDesc(), s = src->GetDesc();
    D3D12_TEXTURE_COPY_LOCATION to{dst, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    D3D12_TEXTURE_COPY_LOCATION from{src, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    D3D12_BOX box{0, 0, 0, UINT((std::min)(d.Width, s.Width)), (std::min)(d.Height, s.Height), 1};
    list->CopyTextureRegion(&to, 0, 0, 0, &from, &box);
}

float DepthA(const Camera& c) {
    const double n = c.nearPlane > 0 ? c.nearPlane : 0.02, f = c.farPlane > n ? c.farPlane : 16000.0;
    if (c.infinite) return c.inverted ? 0.0f : 1.0f;
    return c.inverted ? float(-n / (f - n)) : float(f / (f - n));
}
float DepthB(const Camera& c) {
    const double n = c.nearPlane > 0 ? c.nearPlane : 0.02, f = c.farPlane > n ? c.farPlane : 16000.0;
    if (c.infinite) return c.inverted ? float(n) : float(-n);
    return c.inverted ? float(n * f / (f - n)) : float(-n * f / (f - n));
}

}  // namespace

void Reset() { g.histValid[0] = g.histValid[1] = false; }

Stats Statistics() { return g.stats; }

bool Record(const Request& q) {
    const auto fail = [](const char* why) {
        ++g.stats.failed;
        g.stats.last = why;
        CyberpunkVR_OsrStats[1].fetch_add(1, std::memory_order_relaxed);
        return false;
    };
    if (!q.device || !q.list || !q.drawn || !q.depth || !q.out || q.eye > 1) return fail("no request");
    if (!Init(q.device)) return fail("init");
    const auto dd = q.depth->GetDesc(), od = q.out->GetDesc(), cd = q.drawn->GetDesc();
    if (dd.Format != DXGI_FORMAT_R32_FLOAT) return fail("depth not R32_FLOAT");
    const uint32_t w = uint32_t(dd.Width), h = dd.Height, W = uint32_t(od.Width), H = od.Height;
    if (uint32_t(cd.Width) < W || cd.Height < H) return fail("drawn smaller than the eye");
    if (!EnsureSizes(w, h, W, H)) return fail("textures");
    auto* list = q.list;
    const uint32_t n = q.eye, o = n ^ 1u;
    // the drawn eye and its depth into this eye's history: the source now, the made eye's history in the next frame
    To(list, g.histColor[n], D3D12_RESOURCE_STATE_COPY_DEST);
    To(list, g.histDepth[n], D3D12_RESOURCE_STATE_COPY_DEST);
    Barrier(list, q.drawn, D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
    CopyWhole(list, g.histColor[n].r.Get(), q.drawn);
    Barrier(list, q.drawn, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
    Barrier(list, q.depth, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE, D3D12_RESOURCE_STATE_COPY_SOURCE);
    list->CopyResource(g.histDepth[n].r.Get(), q.depth);
    Barrier(list, q.depth, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    for (Tex* t : {&g.histColor[n], &g.histDepth[n], &g.histColor[o], &g.histDepth[o]})
        To(list, *t, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    for (Tex* t : {&g.stereoT, &g.histT, &g.resColor, &g.resDepth, &g.final, &g.nearD})
        To(list, *t, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);

    const bool history = q.history && g.histValid[o];
    Constants c{};
    PutCamera(c.s, q.source);
    PutCamera(c.d, q.dest);
    PutCamera(c.h, g.histCamera[o]);
    c.depthAB[0] = DepthA(q.source);
    c.depthAB[1] = DepthB(q.source);
    c.depthAB[2] = float((std::max)(1, CyberpunkVR_OsrTune[0])) / 1000.0f;
    c.depthAB[3] = history ? 1.0f : 0.0f;
    c.sizes[0] = w; c.sizes[1] = h; c.sizes[2] = W; c.sizes[3] = H;
    c.jit[0] = q.jitterSign * q.source.jitter[0];
    c.jit[1] = q.jitterSign * q.source.jitter[1];
    c.jit[2] = q.jitterSign * g.histCamera[o].jitter[0];
    c.jit[3] = q.jitterSign * g.histCamera[o].jitter[1];
    c.flags[0] = q.debug;
    c.flags[2] = uint32_t((std::clamp)(CyberpunkVR_OsrTune[1], 8, 512));
    c.flags[3] = uint32_t((std::clamp)(CyberpunkVR_OsrTune[2], 0, 10000));
    c.edge[0] = float((std::clamp)(CyberpunkVR_OsrTune[3], 0, 10000)) / 100.0f;
    c.edge[1] = float(W) / (std::max)(c.d[4][1] - c.d[4][0], 1e-6f);
    const uint32_t slot = g.slot;
    g.slot = (g.slot + 1) % kSlots;
    // two constant blocks a slot: the stereo splat and every other pass read the first, the history splat the second
    Constants stereo = c, past = c;
    stereo.flags[1] = 0;
    past.flags[1] = 1;
    std::memcpy(g.mapped + slot * 1024, &stereo, sizeof(stereo));
    std::memcpy(g.mapped + slot * 1024 + 512, &past, sizeof(past));

    // descriptors: t0 source colour, t1 source depth, t2 history colour, t3 history depth, t4/t5 resolved colour and
    // depth (read by the fill); u0 stereo splat, u1 history splat, u2/u3 resolved, u4 final, u5 nearest depth
    auto cpu = g.heap->GetCPUDescriptorHandleForHeapStart();
    auto gpu = g.heap->GetGPUDescriptorHandleForHeapStart();
    cpu.ptr += SIZE_T(slot) * kDescriptors * g.step;
    gpu.ptr += UINT64(slot) * kDescriptors * g.step;
    const auto at = [&](uint32_t i) { auto h2 = cpu; h2.ptr += SIZE_T(i) * g.step; return h2; };
    Srv(at(0), g.histColor[n].r.Get(), DXGI_FORMAT_R8G8B8A8_UNORM);
    Srv(at(1), g.histDepth[n].r.Get(), DXGI_FORMAT_R32_FLOAT);
    Srv(at(2), g.histColor[o].r.Get(), DXGI_FORMAT_R8G8B8A8_UNORM);
    Srv(at(3), g.histDepth[o].r.Get(), DXGI_FORMAT_R32_FLOAT);
    Srv(at(4), g.resColor.r.Get(), DXGI_FORMAT_R8G8B8A8_UNORM);
    Srv(at(5), g.resDepth.r.Get(), DXGI_FORMAT_R32_FLOAT);
    Uav(at(6), g.stereoT.r.Get(), DXGI_FORMAT_R32_UINT);
    Uav(at(7), g.histT.r.Get(), DXGI_FORMAT_R32_UINT);
    Uav(at(8), g.resColor.r.Get(), DXGI_FORMAT_R8G8B8A8_UNORM);
    Uav(at(9), g.resDepth.r.Get(), DXGI_FORMAT_R32_FLOAT);
    Uav(at(10), g.final.r.Get(), DXGI_FORMAT_R8G8B8A8_UNORM);
    Uav(at(11), g.nearD.r.Get(), DXGI_FORMAT_R32_FLOAT);
    auto srvTable = gpu, uavTable = gpu;
    uavTable.ptr += 6ull * g.step;

    ID3D12DescriptorHeap* heaps[] = {g.heap.Get()};
    list->SetDescriptorHeaps(1, heaps);
    list->SetComputeRootSignature(g.root.Get());
    const auto cbv = g.constants->GetGPUVirtualAddress() + slot * 1024;
    list->SetComputeRootConstantBufferView(0, cbv);
    list->SetComputeRootDescriptorTable(1, srvTable);
    list->SetComputeRootDescriptorTable(2, uavTable);
    const UINT dx = (w + 7) / 8, dy = (h + 7) / 8, cx = (W + 7) / 8, cy = (H + 7) / 8;
    list->SetPipelineState(g.clear.Get());
    list->Dispatch(dx, dy, 1);
    UavBarrier(list);
    list->SetPipelineState(g.splat.Get());
    list->Dispatch(dx, dy, 1);
    if (history) {
        list->SetComputeRootConstantBufferView(0, cbv + 512);
        list->Dispatch(dx, dy, 1);
        list->SetComputeRootConstantBufferView(0, cbv);
    }
    UavBarrier(list);
    list->SetPipelineState(g.resolve.Get());
    list->Dispatch(cx, cy, 1);
    To(list, g.resColor, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    To(list, g.resDepth, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    list->SetPipelineState(g.inpaint.Get());
    list->Dispatch(cx, cy, 1);
    // the made eye into the second eye's slot
    To(list, g.final, D3D12_RESOURCE_STATE_COPY_SOURCE);
    Barrier(list, q.out, D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
    CopyWhole(list, q.out, g.final.r.Get());
    Barrier(list, q.out, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);

    g.histCamera[n] = q.source;
    g.histValid[n] = true;
    ++g.stats.recorded;
    CyberpunkVR_OsrStats[0].fetch_add(1, std::memory_order_relaxed);
    if (history) CyberpunkVR_OsrStats[2].fetch_add(1, std::memory_order_relaxed);
    return true;
}

}  // namespace cvr::osr
