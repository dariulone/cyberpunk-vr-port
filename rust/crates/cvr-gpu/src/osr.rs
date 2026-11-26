//! OSR (formerly src/Render/Osr.cpp; include/Render/Osr.hpp keeps the request and calls this): the port's own
//! reconstruction of the other eye for an AFW launch. The drawn eye's depth is splatted forward into the other eye, and
//! so is the other eye's own previous frame; each of the other eye's pixels is then traced back into whichever of the two
//! holds the surface its splatted depth says is there (the depths must agree), and what neither holds -- background only
//! this eye sees -- takes the background beside it, mirrored across the edge. Four compute passes (cs_5_0, the HLSL kept
//! as it was: docs/rust-migration-plan.md). The exported counters and tuning (CyberpunkVR_OsrStats, CyberpunkVR_OsrTune)
//! stay defined in the C++ file and are handed over with each request. One caller at a time (the frame's AFW record),
//! as the C++ global state was.
#![allow(unsafe_op_in_unsafe_fn)]
use core::ffi::{c_char, c_void, CStr};
use core::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use cvr_body::cmath;
use cvr_host::log;
use windows::core::{Interface, PCSTR};
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::*;

/// The C++ half's exported counters and tuning, handed over with each request (include/Render/Osr.hpp): [0] frames
/// reconstructed, [1] failed, [2] frames that used the made eye's history; [0] depth agreement tolerance x1000, [1] the
/// longest fill run (px), [2] the history's near limit (cm), [3] the edge distance (made-eye pixels x100).
struct Exported<'a> {
    stats: &'a [AtomicI64; 3],
    tune: [i32; 4],
}

const D3DCOMPILE_OPTIMIZATION_LEVEL3: u32 = 1 << 15;

/// The four passes. Depth texels and colour pixels share one convention: uv in 0..1, v down; a view ray is forward + tx *
/// right + ty * up with tx, ty the tangents across the eye's OpenXR window. The depth is reversed with depth = A + B / z
/// for the forward distance z; a sky texel is 0.
const SHADER: &str = r#"
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
"#;

/// cvr::osr::Camera.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Camera {
    pub position: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub forward: [f32; 3],
    /// XrFovf: left, right, up, down.
    pub fov: [f32; 4],
    pub near_plane: f32,
    pub far_plane: f32,
    pub inverted: bool,
    pub infinite: bool,
    pub jitter: [f32; 2],
}
/// cvr::osr::Request.
#[repr(C)]
pub struct Request {
    pub device: *mut c_void,
    pub list: *mut c_void,
    pub drawn: *mut c_void,
    pub depth: *mut c_void,
    pub source: Camera,
    pub dest: Camera,
    pub eye: u32,
    pub out: *mut c_void,
    pub history: bool,
    pub debug: u32,
    pub jitter_sign: f32,
}
/// cvr::osr::Stats.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stats {
    pub recorded: u64,
    pub failed: u64,
    pub last: *const c_char,
}
const _: () = assert!(core::mem::size_of::<Camera>() == 84 && core::mem::size_of::<Request>() == 232 && core::mem::size_of::<Stats>() == 24);

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Constants {
    s: [[f32; 4]; 5],
    d: [[f32; 4]; 5],
    h: [[f32; 4]; 5],
    depth_ab: [f32; 4],
    sizes: [u32; 4],
    jit: [f32; 4],
    flags: [u32; 4],
    edge: [f32; 4],
}
const _: () = assert!(core::mem::size_of::<Constants>() <= 512);

const SLOTS: u32 = 3;
const DESCRIPTORS: u32 = 12;

#[derive(Default)]
struct Tex {
    r: Option<ID3D12Resource>,
    state: D3D12_RESOURCE_STATES,
}

struct State {
    device: usize,
    failed: bool,
    root: Option<ID3D12RootSignature>,
    clear: Option<ID3D12PipelineState>,
    splat: Option<ID3D12PipelineState>,
    resolve: Option<ID3D12PipelineState>,
    inpaint: Option<ID3D12PipelineState>,
    heap: Option<ID3D12DescriptorHeap>,
    constants: Option<ID3D12Resource>,
    mapped: *mut u8,
    slot: u32,
    step: u32,
    w: u32,
    h: u32,
    big_w: u32,
    big_h: u32,
    hist_color: [Tex; 2],
    hist_depth: [Tex; 2],
    hist_camera: [Camera; 2],
    hist_valid: [bool; 2],
    stereo_t: Tex,
    hist_t: Tex,
    res_color: Tex,
    res_depth: Tex,
    fin: Tex,
    near_d: Tex,
    recorded: u64,
    failures: u64,
    last: &'static CStr,
}
// SAFETY: free-threaded D3D12 objects and a persistently mapped upload buffer, used under the module's mutex.
unsafe impl Send for State {}
impl Default for State {
    fn default() -> State {
        State {
            device: 0,
            failed: false,
            root: None,
            clear: None,
            splat: None,
            resolve: None,
            inpaint: None,
            heap: None,
            constants: None,
            mapped: core::ptr::null_mut(),
            slot: 0,
            step: 0,
            w: 0,
            h: 0,
            big_w: 0,
            big_h: 0,
            hist_color: Default::default(),
            hist_depth: Default::default(),
            hist_camera: Default::default(),
            hist_valid: [false; 2],
            stereo_t: Tex::default(),
            hist_t: Tex::default(),
            res_color: Tex::default(),
            res_depth: Tex::default(),
            fin: Tex::default(),
            near_d: Tex::default(),
            recorded: 0,
            failures: 0,
            last: c"",
        }
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn fail_init(g: &mut State, why: &'static CStr) {
    g.failed = true;
    g.last = why;
    log!("[osr] {}\n", why.to_string_lossy());
}

unsafe fn barrier(list: &ID3D12GraphicsCommandList, r: Option<&ID3D12Resource>, a: D3D12_RESOURCE_STATES, b: D3D12_RESOURCE_STATES) {
    let Some(r) = r else { return };
    if a == b {
        return;
    }
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            Transition: core::mem::ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER {
                pResource: core::mem::transmute_copy(r),
                Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
                StateBefore: a,
                StateAfter: b,
            }),
        },
    };
    list.ResourceBarrier(&[v]);
}
unsafe fn to(list: &ID3D12GraphicsCommandList, t: &mut Tex, s: D3D12_RESOURCE_STATES) {
    barrier(list, t.r.as_ref(), t.state, s);
    t.state = s;
}
unsafe fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let v = D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            UAV: core::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                pResource: core::mem::ManuallyDrop::new(None),
            }),
        },
    };
    list.ResourceBarrier(&[v]);
}

unsafe fn make(device: &ID3D12Device, t: &mut Tex, width: u32, height: u32, format: DXGI_FORMAT, uav: bool) -> bool {
    let d = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D,
        Width: width as u64,
        Height: height,
        DepthOrArraySize: 1,
        MipLevels: 1,
        Format: format,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Flags: if uav {
            D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS
        } else {
            D3D12_RESOURCE_FLAG_NONE
        },
        ..Default::default()
    };
    let heap = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_DEFAULT,
        ..Default::default()
    };
    t.r = None;
    t.state = D3D12_RESOURCE_STATE_COMMON;
    let mut r: Option<ID3D12Resource> = None;
    let ok = device.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &d, t.state, None, &mut r).is_ok();
    t.r = r;
    ok
}

unsafe fn init(g: &mut State, device: &ID3D12Device) -> bool {
    if g.device == device.as_raw() as usize && g.root.is_some() && g.clear.is_some() && g.splat.is_some() && g.resolve.is_some() && g.inpaint.is_some() {
        return true;
    }
    if g.failed {
        return false;
    }
    *g = State::default();
    g.device = device.as_raw() as usize;
    let ranges = [
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
            NumDescriptors: 6,
            ..Default::default()
        },
        // a table of its own, handed its first descriptor
        D3D12_DESCRIPTOR_RANGE {
            RangeType: D3D12_DESCRIPTOR_RANGE_TYPE_UAV,
            NumDescriptors: 6,
            ..Default::default()
        },
    ];
    let params = [
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_CBV,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                Descriptor: D3D12_ROOT_DESCRIPTOR {
                    ShaderRegister: 0,
                    RegisterSpace: 0,
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &ranges[0],
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
        D3D12_ROOT_PARAMETER {
            ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
            Anonymous: D3D12_ROOT_PARAMETER_0 {
                DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                    NumDescriptorRanges: 1,
                    pDescriptorRanges: &ranges[1],
                },
            },
            ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
        },
    ];
    let sampler = D3D12_STATIC_SAMPLER_DESC {
        Filter: D3D12_FILTER_MIN_MAG_MIP_LINEAR,
        AddressU: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressV: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        AddressW: D3D12_TEXTURE_ADDRESS_MODE_CLAMP,
        MaxLOD: D3D12_FLOAT32_MAX,
        ..Default::default()
    };
    let rs = D3D12_ROOT_SIGNATURE_DESC {
        NumParameters: 3,
        pParameters: params.as_ptr(),
        NumStaticSamplers: 1,
        pStaticSamplers: &sampler,
        Flags: D3D12_ROOT_SIGNATURE_FLAG_NONE,
    };
    let mut blob: Option<ID3DBlob> = None;
    let root = if D3D12SerializeRootSignature(&rs, D3D_ROOT_SIGNATURE_VERSION_1, &mut blob, None).is_ok() {
        blob.and_then(|b| {
            let bytes = core::slice::from_raw_parts(b.GetBufferPointer() as *const u8, b.GetBufferSize());
            device.CreateRootSignature::<ID3D12RootSignature>(0, bytes).ok()
        })
    } else {
        None
    };
    let Some(root) = root else {
        fail_init(g, c"root signature");
        return false;
    };
    g.root = Some(root.clone());
    // the passes made from Rust (rust/shaders/passes/src/osr.rs); the HLSL above with CVR_SHADER_HLSL
    let passes = [
        (c"Clear", 0, crate::dxil::OSR_CLEAR_CS),
        (c"Splat", 1, crate::dxil::OSR_SPLAT_CS),
        (c"Resolve", 2, crate::dxil::OSR_RESOLVE_CS),
        (c"Inpaint", 3, crate::dxil::OSR_INPAINT_CS),
    ];
    for (entry, which, dxil) in passes {
        let hlsl = || {
            let mut code: Option<ID3DBlob> = None;
            let mut err: Option<ID3DBlob> = None;
            let compiled = D3DCompile(
                SHADER.as_ptr() as *const c_void,
                SHADER.len(),
                PCSTR(c"osr.hlsl".as_ptr() as *const u8),
                None,
                None,
                PCSTR(entry.as_ptr() as *const u8),
                PCSTR(c"cs_5_0".as_ptr() as *const u8),
                D3DCOMPILE_OPTIMIZATION_LEVEL3,
                0,
                &mut code,
                Some(&mut err),
            );
            if compiled.is_err() || code.is_none() {
                let text = match &err {
                    Some(e) => CStr::from_ptr(e.GetBufferPointer() as *const c_char).to_string_lossy().into_owned(),
                    None => "compile failed".to_string(),
                };
                log!("[osr] {}: {}\n", entry.to_string_lossy(), text);
            }
            code.filter(|_| compiled.is_ok())
        };
        let Some(code) = crate::dxil::pick(dxil, hlsl) else {
            fail_init(g, c"shader compile");
            return false;
        };
        let mut pd = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: core::mem::transmute_copy(&root),
            CS: code.bytecode(),
            ..Default::default()
        };
        let made: windows::core::Result<ID3D12PipelineState> = device.CreateComputePipelineState(&pd);
        let _ = core::mem::ManuallyDrop::into_inner(core::mem::take(&mut pd.pRootSignature)).map(core::mem::forget);
        let Ok(pso) = made else {
            fail_init(g, c"pipeline");
            return false;
        };
        match which {
            0 => g.clear = Some(pso),
            1 => g.splat = Some(pso),
            2 => g.resolve = Some(pso),
            _ => g.inpaint = Some(pso),
        }
    }
    let hd = D3D12_DESCRIPTOR_HEAP_DESC {
        Type: D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
        NumDescriptors: SLOTS * DESCRIPTORS,
        Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
        NodeMask: 0,
    };
    let cb = D3D12_RESOURCE_DESC {
        Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
        Width: (SLOTS * 1024) as u64,
        Height: 1,
        DepthOrArraySize: 1,
        MipLevels: 1,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
        ..Default::default()
    };
    let up = D3D12_HEAP_PROPERTIES {
        Type: D3D12_HEAP_TYPE_UPLOAD,
        ..Default::default()
    };
    let heap = device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&hd).ok();
    let mut constants: Option<ID3D12Resource> = None;
    let mut mapped: *mut c_void = core::ptr::null_mut();
    let ok = heap.is_some()
        && device
            .CreateCommittedResource(&up, D3D12_HEAP_FLAG_NONE, &cb, D3D12_RESOURCE_STATE_GENERIC_READ, None, &mut constants)
            .is_ok()
        && constants.as_ref().is_some_and(|c| c.Map(0, None, Some(&mut mapped)).is_ok());
    g.heap = heap;
    g.constants = constants;
    if !ok {
        fail_init(g, c"heap or constants");
        return false;
    }
    g.mapped = mapped as *mut u8;
    g.step = device.GetDescriptorHandleIncrementSize(D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV);
    log!("[osr] ready\n");
    true
}

unsafe fn ensure_sizes(g: &mut State, device: &ID3D12Device, w: u32, h: u32, big_w: u32, big_h: u32) -> bool {
    if g.w == w && g.h == h && g.big_w == big_w && g.big_h == big_h && g.fin.r.is_some() {
        return true;
    }
    let mut ok = true;
    for e in 0..2 {
        ok = ok
            && make(device, &mut g.hist_color[e], big_w, big_h, DXGI_FORMAT_R8G8B8A8_UNORM, false)
            && make(device, &mut g.hist_depth[e], w, h, DXGI_FORMAT_R32_FLOAT, false);
        g.hist_valid[e] = false;
    }
    ok = ok
        && make(device, &mut g.stereo_t, w, h, DXGI_FORMAT_R32_UINT, true)
        && make(device, &mut g.hist_t, w, h, DXGI_FORMAT_R32_UINT, true)
        && make(device, &mut g.res_color, big_w, big_h, DXGI_FORMAT_R8G8B8A8_UNORM, true)
        && make(device, &mut g.res_depth, big_w, big_h, DXGI_FORMAT_R32_FLOAT, true)
        && make(device, &mut g.fin, big_w, big_h, DXGI_FORMAT_R8G8B8A8_UNORM, true)
        && make(device, &mut g.near_d, w, h, DXGI_FORMAT_R32_FLOAT, true);
    if !ok {
        fail_init(g, c"textures");
        return false;
    }
    g.w = w;
    g.h = h;
    g.big_w = big_w;
    g.big_h = big_h;
    log!("[osr] depth {}x{}, eye {}x{}\n", w, h, big_w, big_h);
    true
}

fn put_camera(out: &mut [[f32; 4]; 5], c: &Camera) {
    for i in 0..3 {
        out[0][i] = c.position[i];
        out[1][i] = c.right[i];
        out[2][i] = c.up[i];
        out[3][i] = c.forward[i];
    }
    out[0][3] = 0.0;
    out[1][3] = 0.0;
    out[2][3] = 0.0;
    out[3][3] = 0.0;
    out[4] = [cmath::tan(c.fov[0]), cmath::tan(c.fov[1]), cmath::tan(c.fov[2]), cmath::tan(c.fov[3])];
}

unsafe fn srv(device: &ID3D12Device, at: D3D12_CPU_DESCRIPTOR_HANDLE, r: Option<&ID3D12Resource>, f: DXGI_FORMAT) {
    let d = D3D12_SHADER_RESOURCE_VIEW_DESC {
        Format: f,
        ViewDimension: D3D12_SRV_DIMENSION_TEXTURE2D,
        Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
        Anonymous: D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
            Texture2D: D3D12_TEX2D_SRV {
                MipLevels: 1,
                ..Default::default()
            },
        },
    };
    device.CreateShaderResourceView(r, Some(&d), at);
}
unsafe fn uav(device: &ID3D12Device, at: D3D12_CPU_DESCRIPTOR_HANDLE, r: Option<&ID3D12Resource>, f: DXGI_FORMAT) {
    let d = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: f,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        ..Default::default()
    };
    device.CreateUnorderedAccessView(r, None, Some(&d), at);
}

unsafe fn copy_whole(list: &ID3D12GraphicsCommandList, dst: &ID3D12Resource, src: &ID3D12Resource) {
    let (d, s) = (dst.GetDesc(), src.GetDesc());
    let to = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(dst),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let from = D3D12_TEXTURE_COPY_LOCATION {
        pResource: core::mem::transmute_copy(src),
        Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
        Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
    };
    let bx = D3D12_BOX {
        left: 0,
        top: 0,
        front: 0,
        right: d.Width.min(s.Width) as u32,
        bottom: d.Height.min(s.Height),
        back: 1,
    };
    list.CopyTextureRegion(&to, 0, 0, 0, &from, Some(&bx));
    core::mem::forget(to);
    core::mem::forget(from);
}

fn limits(c: &Camera) -> (f64, f64) {
    let n = if c.near_plane > 0.0 { c.near_plane as f64 } else { 0.02 };
    let f = if c.far_plane as f64 > n { c.far_plane as f64 } else { 16000.0 };
    (n, f)
}
fn depth_a(c: &Camera) -> f32 {
    let (n, f) = limits(c);
    if c.infinite {
        return if c.inverted { 0.0 } else { 1.0 };
    }
    if c.inverted {
        (-n / (f - n)) as f32
    } else {
        (f / (f - n)) as f32
    }
}
fn depth_b(c: &Camera) -> f32 {
    let (n, f) = limits(c);
    if c.infinite {
        return if c.inverted { n as f32 } else { (-n) as f32 };
    }
    if c.inverted {
        (n * f / (f - n)) as f32
    } else {
        (-n * f / (f - n)) as f32
    }
}

fn clamp_i(v: i32, lo: i32, hi: i32) -> i32 {
    if hi < v {
        hi
    } else if v < lo {
        lo
    } else {
        v
    }
}

unsafe fn record(g: &mut State, q: &Request, x: &Exported) -> Result<(), &'static CStr> {
    let (Some(device), Some(list), Some(drawn), Some(depth), Some(out)) = (
        ID3D12Device::from_raw_borrowed(&q.device),
        ID3D12GraphicsCommandList::from_raw_borrowed(&q.list),
        ID3D12Resource::from_raw_borrowed(&q.drawn),
        ID3D12Resource::from_raw_borrowed(&q.depth),
        ID3D12Resource::from_raw_borrowed(&q.out),
    ) else {
        return Err(c"no request");
    };
    if q.eye > 1 {
        return Err(c"no request");
    }
    if !init(g, device) {
        return Err(c"init");
    }
    let (dd, od, cd) = (depth.GetDesc(), out.GetDesc(), drawn.GetDesc());
    if dd.Format != DXGI_FORMAT_R32_FLOAT {
        return Err(c"depth not R32_FLOAT");
    }
    let (w, h, big_w, big_h) = (dd.Width as u32, dd.Height, od.Width as u32, od.Height);
    if (cd.Width as u32) < big_w || cd.Height < big_h {
        return Err(c"drawn smaller than the eye");
    }
    if !ensure_sizes(g, device, w, h, big_w, big_h) {
        return Err(c"textures");
    }
    let n = q.eye as usize;
    let o = n ^ 1;
    // the drawn eye and its depth into this eye's history: the source now, the made eye's history in the next frame
    to(list, &mut g.hist_color[n], D3D12_RESOURCE_STATE_COPY_DEST);
    to(list, &mut g.hist_depth[n], D3D12_RESOURCE_STATE_COPY_DEST);
    barrier(list, Some(drawn), D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_COPY_SOURCE);
    copy_whole(list, g.hist_color[n].r.as_ref().unwrap(), drawn);
    barrier(list, Some(drawn), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_PRESENT);
    barrier(
        list,
        Some(depth),
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
        D3D12_RESOURCE_STATE_COPY_SOURCE,
    );
    list.CopyResource(g.hist_depth[n].r.as_ref().unwrap(), depth);
    barrier(
        list,
        Some(depth),
        D3D12_RESOURCE_STATE_COPY_SOURCE,
        D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE,
    );
    for (e, color) in [(n, true), (n, false), (o, true), (o, false)] {
        let t = if color { &mut g.hist_color[e] } else { &mut g.hist_depth[e] };
        to(list, t, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    }
    for t in [&mut g.stereo_t, &mut g.hist_t, &mut g.res_color, &mut g.res_depth, &mut g.fin, &mut g.near_d] {
        to(list, t, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
    }

    let history = q.history && g.hist_valid[o];
    let tune = &x.tune;
    let mut c = Constants::default();
    put_camera(&mut c.s, &q.source);
    put_camera(&mut c.d, &q.dest);
    put_camera(&mut c.h, &g.hist_camera[o]);
    c.depth_ab[0] = depth_a(&q.source);
    c.depth_ab[1] = depth_b(&q.source);
    c.depth_ab[2] = 1i32.max(tune[0]) as f32 / 1000.0;
    c.depth_ab[3] = if history { 1.0 } else { 0.0 };
    c.sizes = [w, h, big_w, big_h];
    c.jit[0] = q.jitter_sign * q.source.jitter[0];
    c.jit[1] = q.jitter_sign * q.source.jitter[1];
    c.jit[2] = q.jitter_sign * g.hist_camera[o].jitter[0];
    c.jit[3] = q.jitter_sign * g.hist_camera[o].jitter[1];
    c.flags[0] = q.debug;
    c.flags[2] = clamp_i(tune[1], 8, 512) as u32;
    c.flags[3] = clamp_i(tune[2], 0, 10000) as u32;
    c.edge[0] = clamp_i(tune[3], 0, 10000) as f32 / 100.0;
    c.edge[1] = big_w as f32 / cmath::max(c.d[4][1] - c.d[4][0], 1e-6);
    let slot = g.slot;
    g.slot = (g.slot + 1) % SLOTS;
    // two constant blocks a slot: the stereo splat and every other pass read the first, the history splat the second
    let (mut stereo, mut past) = (c, c);
    stereo.flags[1] = 0;
    past.flags[1] = 1;
    core::ptr::copy_nonoverlapping(
        &stereo as *const Constants as *const u8,
        g.mapped.add(slot as usize * 1024),
        core::mem::size_of::<Constants>(),
    );
    core::ptr::copy_nonoverlapping(
        &past as *const Constants as *const u8,
        g.mapped.add(slot as usize * 1024 + 512),
        core::mem::size_of::<Constants>(),
    );

    // descriptors: t0 source colour, t1 source depth, t2 history colour, t3 history depth, t4/t5 resolved colour and
    // depth (read by the fill); u0 stereo splat, u1 history splat, u2/u3 resolved, u4 final, u5 nearest depth
    let heap = g.heap.as_ref().unwrap();
    let mut cpu = heap.GetCPUDescriptorHandleForHeapStart();
    let mut gpu = heap.GetGPUDescriptorHandleForHeapStart();
    cpu.ptr += (slot * DESCRIPTORS * g.step) as usize;
    gpu.ptr += (slot * DESCRIPTORS * g.step) as u64;
    let at = |i: u32| D3D12_CPU_DESCRIPTOR_HANDLE {
        ptr: cpu.ptr + (i * g.step) as usize,
    };
    srv(device, at(0), g.hist_color[n].r.as_ref(), DXGI_FORMAT_R8G8B8A8_UNORM);
    srv(device, at(1), g.hist_depth[n].r.as_ref(), DXGI_FORMAT_R32_FLOAT);
    srv(device, at(2), g.hist_color[o].r.as_ref(), DXGI_FORMAT_R8G8B8A8_UNORM);
    srv(device, at(3), g.hist_depth[o].r.as_ref(), DXGI_FORMAT_R32_FLOAT);
    srv(device, at(4), g.res_color.r.as_ref(), DXGI_FORMAT_R8G8B8A8_UNORM);
    srv(device, at(5), g.res_depth.r.as_ref(), DXGI_FORMAT_R32_FLOAT);
    uav(device, at(6), g.stereo_t.r.as_ref(), DXGI_FORMAT_R32_UINT);
    uav(device, at(7), g.hist_t.r.as_ref(), DXGI_FORMAT_R32_UINT);
    uav(device, at(8), g.res_color.r.as_ref(), DXGI_FORMAT_R8G8B8A8_UNORM);
    uav(device, at(9), g.res_depth.r.as_ref(), DXGI_FORMAT_R32_FLOAT);
    uav(device, at(10), g.fin.r.as_ref(), DXGI_FORMAT_R8G8B8A8_UNORM);
    uav(device, at(11), g.near_d.r.as_ref(), DXGI_FORMAT_R32_FLOAT);
    let srv_table = gpu;
    let uav_table = D3D12_GPU_DESCRIPTOR_HANDLE {
        ptr: gpu.ptr + 6 * g.step as u64,
    };

    list.SetDescriptorHeaps(&[Some(heap.clone())]);
    list.SetComputeRootSignature(g.root.as_ref().unwrap());
    let cbv = g.constants.as_ref().unwrap().GetGPUVirtualAddress() + slot as u64 * 1024;
    list.SetComputeRootConstantBufferView(0, cbv);
    list.SetComputeRootDescriptorTable(1, srv_table);
    list.SetComputeRootDescriptorTable(2, uav_table);
    let (dx, dy, cx, cy) = (w.div_ceil(8), h.div_ceil(8), big_w.div_ceil(8), big_h.div_ceil(8));
    list.SetPipelineState(g.clear.as_ref().unwrap());
    list.Dispatch(dx, dy, 1);
    uav_barrier(list);
    list.SetPipelineState(g.splat.as_ref().unwrap());
    list.Dispatch(dx, dy, 1);
    if history {
        list.SetComputeRootConstantBufferView(0, cbv + 512);
        list.Dispatch(dx, dy, 1);
        list.SetComputeRootConstantBufferView(0, cbv);
    }
    uav_barrier(list);
    list.SetPipelineState(g.resolve.as_ref().unwrap());
    list.Dispatch(cx, cy, 1);
    to(list, &mut g.res_color, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    to(list, &mut g.res_depth, D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE);
    list.SetPipelineState(g.inpaint.as_ref().unwrap());
    list.Dispatch(cx, cy, 1);
    // the made eye into the second eye's slot
    to(list, &mut g.fin, D3D12_RESOURCE_STATE_COPY_SOURCE);
    barrier(list, Some(out), D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_COPY_DEST);
    copy_whole(list, out, g.fin.r.as_ref().unwrap());
    barrier(list, Some(out), D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE);

    g.hist_camera[n] = q.source;
    g.hist_valid[n] = true;
    g.recorded += 1;
    x.stats[0].fetch_add(1, Ordering::Relaxed);
    if history {
        x.stats[2].fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

// ---- the C ABI (include/Render/Osr.hpp)

/// cvr::osr::Record: the made eye recorded into request.out; false when nothing was recorded.
///
/// # Safety
/// `request` is a cvr::osr::Request whose objects are the caller's live ones (or null).
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_osr_record(request: *const Request, stats: *const [AtomicI64; 3], tune: *const [i32; 4]) -> bool {
    let (Some(stats), Some(tune)) = (stats.as_ref(), tune.as_ref()) else { return false };
    let x = Exported { stats, tune: core::ptr::read_volatile(tune) };
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let g = s.get_or_insert_with(State::default);
    let result = match request.as_ref() {
        Some(q) => record(g, q, &x),
        None => Err(c"no request"),
    };
    match result {
        Ok(()) => true,
        Err(why) => {
            g.failures += 1;
            g.last = why;
            x.stats[1].fetch_add(1, Ordering::Relaxed);
            false
        }
    }
}
/// cvr::osr::Reset: both eyes' history forgotten.
#[no_mangle]
pub extern "C" fn cvr_gpu_osr_reset() {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(g) = s.as_mut() {
        g.hist_valid = [false; 2];
    }
}
/// cvr::osr::Statistics.
///
/// # Safety
/// `out` is a cvr::osr::Stats or null.
#[no_mangle]
pub unsafe extern "C" fn cvr_gpu_osr_statistics(out: *mut Stats) {
    let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(out) = out.as_mut() {
        *out = match s.as_ref() {
            Some(g) => Stats {
                recorded: g.recorded,
                failed: g.failures,
                last: g.last.as_ptr(),
            },
            None => Stats {
                recorded: 0,
                failed: 0,
                last: c"".as_ptr(),
            },
        };
    }
}
