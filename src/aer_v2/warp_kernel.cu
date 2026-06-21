// CUDA warp kernel for AER V2.
//
// CUDA variant of the synthetic-eye warp. It combines NvOF flow, engine DLSS
// motion vectors, prev/current depth, NvOF cost confidence, and pose reprojection
// behind alternate-eye per-scale launch wrappers.
//
// Algorithm (current implementation):
//   flow = sample NvOF at this pixel's grid cell (S10.5 → pixels)
//   prevUv = uv - flow * blend / outSize
//   currUv = uv + flow * (1-blend) / outSize
//   color  = confidence/depth-aware lerp(tex(prevUv), tex(currUv), blend)
//   write to outSurf
#include <cuda_runtime.h>
#include <cuda_fp16.h>

struct WarpPoseParams {
    float prevToPred[16];
    float currToPred[16];
    float tanLeft;
    float tanRight;
    float tanDown;
    float tanUp;
    float nearZ;
    float refineStrength;   // 0..1 scale on MV + pose flow refinement
    float occlusionSharp;   // 0..1 scale on occlusion edge-sharpening
    float foveation;        // 0..1 fixed-foveation periphery fraction (0 = off)
    float flowSmooth;       // 0..0.9 NvOF flow 3x3 spatial low-pass
};

static __device__ __forceinline__ float clamp01(float v) {
    return v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v);
}

// Manual bilinear read from a uchar4 surface (replaces tex2D for imported
// D3D12 external memory where texture objects return zeros).
static __device__ __forceinline__ float4 surf2Dbilinear_rgba8(
    cudaSurfaceObject_t surf, float fx, float fy, int outW, int outH) {
    const int x0 = max(0, min(static_cast<int>(fx), outW - 1));
    const int y0 = max(0, min(static_cast<int>(fy), outH - 1));
    const int x1 = min(x0 + 1, outW - 1);
    const int y1 = min(y0 + 1, outH - 1);
    const float wx = fx - static_cast<float>(x0);
    const float wy = fy - static_cast<float>(y0);
    uchar4 c00, c10, c01, c11;
    surf2Dread(&c00, surf, x0 * 4, y0);
    surf2Dread(&c10, surf, x1 * 4, y0);
    surf2Dread(&c01, surf, x0 * 4, y1);
    surf2Dread(&c11, surf, x1 * 4, y1);
    auto lerp = [](float a, float b, float t) { return a + (b - a) * t; };
    const float inv = 1.0f / 255.0f;
    return make_float4(
        lerp(lerp(c00.x * inv, c10.x * inv, wx), lerp(c01.x * inv, c11.x * inv, wx), wy),
        lerp(lerp(c00.y * inv, c10.y * inv, wx), lerp(c01.y * inv, c11.y * inv, wx), wy),
        lerp(lerp(c00.z * inv, c10.z * inv, wx), lerp(c01.z * inv, c11.z * inv, wx), wy),
        1.0f);
}

// Manual bilinear read from a R32_FLOAT depth surface.
static __device__ __forceinline__ float surf2Dbilinear_r32f(
    cudaSurfaceObject_t surf, float fx, float fy, int outW, int outH) {
    const int x0 = max(0, min(static_cast<int>(fx), outW - 1));
    const int y0 = max(0, min(static_cast<int>(fy), outH - 1));
    const int x1 = min(x0 + 1, outW - 1);
    const int y1 = min(y0 + 1, outH - 1);
    const float wx = fx - static_cast<float>(x0);
    const float wy = fy - static_cast<float>(y0);
    float d00, d10, d01, d11;
    surf2Dread(&d00, surf, x0 * 4, y0);
    surf2Dread(&d10, surf, x1 * 4, y0);
    surf2Dread(&d01, surf, x0 * 4, y1);
    surf2Dread(&d11, surf, x1 * 4, y1);
    const float d0 = d00 + (d10 - d00) * wx;
    const float d1 = d01 + (d11 - d01) * wx;
    return d0 + (d1 - d0) * wy;
}

static __device__ __forceinline__ float3 mulPoint3x4(const float* m, float3 p) {
    return make_float3(
        m[0] * p.x + m[1] * p.y + m[2] * p.z + m[12],
        m[4] * p.x + m[5] * p.y + m[6] * p.z + m[13],
        m[8] * p.x + m[9] * p.y + m[10] * p.z + m[14]);
}

static __device__ __forceinline__ float2 projectPoseUv(float2 uv, float depth, const float* m, const WarpPoseParams& pose) {
    if (depth <= 0.0f || pose.nearZ <= 0.0f) {
        return uv;
    }
    // CP2077 depth is reversed-Z with an effectively infinite far plane. The
    // pose reprojection needs an approximate view-space distance, not the raw
    // depth buffer sample itself. Mirror the existing stereo reprojection path:
    // z ~= near / depth, and skip reprojection for sky/far-plane samples.
    const float depthClamped = fmaxf(depth, 1.0e-4f);
    const float zView = pose.nearZ / depthClamped;
    const float rayX = pose.tanLeft + (pose.tanRight - pose.tanLeft) * uv.x;
    const float rayY = pose.tanDown + (pose.tanUp - pose.tanDown) * uv.y;
    // HANDEDNESS BRIDGE. This screen ray is in a DX-style convention (+X right,
    // +Y DOWN — uv.y=0 is the top texel and maps to tanDown<0 — and +Z forward),
    // but `m` is an OpenXR-frame rigid transform built from head quaternions
    // (+X right, +Y up, -Z forward). Applying the rotation directly rotated a ray
    // with two flipped axes -> skewed "fisheye" reprojection that the agreement
    // gate then rejected (so head-turn fell back to raw NvOF = tearing). Convert
    // DX->OpenXR (negate Y,Z) before the transform and back (negate Y,Z) after.
    const float3 pRenderXr = make_float3(rayX * zView, -rayY * zView, -zView);
    const float3 pPredXr = mulPoint3x4(m, pRenderXr);
    const float3 pPred = make_float3(pPredXr.x, -pPredXr.y, -pPredXr.z);
    const float z = fabsf(pPred.z) > 1.0e-5f ? pPred.z : zView;
    const float xProj = pPred.x / z;
    const float yProj = pPred.y / z;
    const float u = (xProj - pose.tanLeft) / (pose.tanRight - pose.tanLeft);
    const float v = (yProj - pose.tanDown) / (pose.tanUp - pose.tanDown);
    return make_float2(clamp01(u), clamp01(v));
}

static __device__ __forceinline__ uchar4 pack_color(float4 c, uint32_t outputFormat) {
    (void)outputFormat;
    auto ch = [](float v) -> unsigned char {
        v = v < 0.0f ? 0.0f : (v > 1.0f ? 1.0f : v);
        return static_cast<unsigned char>(v * 255.0f + 0.5f);
    };
    uchar4 out;
    // CUDA external arrays expose the D3D12 resource's byte/channel order. Keep
    // channels in that order for both RGBA and BGRA resources; swapping here
    // inverts red/blue on R8G8B8A8 backbuffers.
    out.x = ch(c.x);
    out.y = ch(c.y);
    out.z = ch(c.z);
    out.w = ch(c.w);
    return out;
}

// BILINEAR flow sample in PIXELS. NvOF stores one short2 per GRID_SIZE×GRID_SIZE
// cell in S10.5 fixed point (/32 -> px). Nearest-cell sampling made the motion
// field blocky: adjacent pixels in different cells jumped to a different vector,
// so object edges shimmered frame-to-frame ("running dots"). Bilinear across the
// 4 surrounding cells gives a smooth sub-pixel field -> stable edges.
template <uint32_t GRID_SIZE>
static __device__ __forceinline__ float2 sampleFlowBilinearPx(
    const short2* flowDevPtr, uint32_t flowStride, uint32_t x, uint32_t y, uint32_t outW, uint32_t outH) {
    const int flowW = static_cast<int>((outW + GRID_SIZE - 1) / GRID_SIZE);
    const int flowH = static_cast<int>((outH + GRID_SIZE - 1) / GRID_SIZE);
    // Pixel center mapped into flow-grid space (cell centers at integer coords).
    const float gx = (static_cast<float>(x) + 0.5f) / static_cast<float>(GRID_SIZE) - 0.5f;
    const float gy = (static_cast<float>(y) + 0.5f) / static_cast<float>(GRID_SIZE) - 0.5f;
    const int gx0 = static_cast<int>(floorf(gx));
    const int gy0 = static_cast<int>(floorf(gy));
    const float wx = gx - static_cast<float>(gx0);
    const float wy = gy - static_cast<float>(gy0);
    const int x0 = max(0, min(gx0, flowW - 1));
    const int x1 = max(0, min(gx0 + 1, flowW - 1));
    const int y0 = max(0, min(gy0, flowH - 1));
    const int y1 = max(0, min(gy0 + 1, flowH - 1));
    auto cell = [&](int cx, int cy) -> float2 {
        const char* row = reinterpret_cast<const char*>(flowDevPtr) + static_cast<size_t>(cy) * flowStride;
        const short2 s = reinterpret_cast<const short2*>(row)[cx];
        return make_float2(static_cast<float>(s.x) * (1.0f / 32.0f), static_cast<float>(s.y) * (1.0f / 32.0f));
    };
    const float2 f00 = cell(x0, y0), f10 = cell(x1, y0), f01 = cell(x0, y1), f11 = cell(x1, y1);
    const float fx0x = f00.x + (f10.x - f00.x) * wx;
    const float fx0y = f00.y + (f10.y - f00.y) * wx;
    const float fx1x = f01.x + (f11.x - f01.x) * wx;
    const float fx1y = f01.y + (f11.y - f01.y) * wx;
    return make_float2(fx0x + (fx1x - fx0x) * wy, fx0y + (fx1y - fx0y) * wy);
}

static __device__ __forceinline__ bool uvInBounds(float2 uv) {
    return uv.x >= 0.0f && uv.x <= 1.0f && uv.y >= 0.0f && uv.y <= 1.0f;
}

template <uint32_t GRID_SIZE>
static __device__ __forceinline__ void aer_v2_warp_depth_impl(
    cudaTextureObject_t prevColorTex,
    cudaTextureObject_t currColorTex,
    const short2* flowDevPtr,
    uint32_t flowStride,
    uint32_t /*flowGridSize*/,
    cudaTextureObject_t engineMvTex,
    float engineMvScaleX,
    float engineMvScaleY,
    uint32_t engineMvW,
    uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex,
    cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr,
    uint32_t costStride,
    cudaSurfaceObject_t outSurf,
    uint32_t outW,
    uint32_t outH,
    uint32_t outputFormat,
    WarpPoseParams pose,
    float blend)
{
    const uint32_t x = blockIdx.x * blockDim.x + threadIdx.x;
    const uint32_t y = blockIdx.y * blockDim.y + threadIdx.y;
    if (x >= outW || y >= outH) {
        return;
    }

    // All input textures are read via surf2Dread (texture objects return zeros
    // for D3D12-imported external memory on this driver/runtime combination).
    cudaSurfaceObject_t prevSurf = static_cast<cudaSurfaceObject_t>(prevColorTex);
    cudaSurfaceObject_t currSurf = static_cast<cudaSurfaceObject_t>(currColorTex);
    cudaSurfaceObject_t prevDepthSurf = static_cast<cudaSurfaceObject_t>(prevDepthTex);
    cudaSurfaceObject_t currDepthSurf = static_cast<cudaSurfaceObject_t>(currDepthTex);
    cudaSurfaceObject_t mvSurf = static_cast<cudaSurfaceObject_t>(engineMvTex);
    const bool haveDepth = (prevDepthSurf != 0 && currDepthSurf != 0);
    const bool haveMv = (mvSurf != 0 && engineMvW > 0 && engineMvH > 0);

    const float2 invSize = make_float2(1.0f / static_cast<float>(outW),
                                       1.0f / static_cast<float>(outH));
    const float2 uv = make_float2((static_cast<float>(x) + 0.5f) * invSize.x,
                                  (static_cast<float>(y) + 0.5f) * invSize.y);

    // ===== FIXED FOVEATION (no eye-tracking) — two zones =====
    // Inner circle = SWEET SPOT: the full NvOF bidirectional warp. Outer ring =
    // PERIPHERY: skip the optical-flow warp ENTIRELY and just copy the raw
    // nearest-in-time captured frame at HALF resolution (snap to even texels =
    // 2x2 blocky). That's the cheapest possible path (1 read + 1 write, no flow/
    // depth/refine) and hides warp artifacts in the low-acuity periphery, exactly
    // the "круг внутри круга, снаружи пол-разрешения без NvOF" request.
    // `foveation` = fraction of the lens radius that is periphery (0 = off). A
    // feather band softens the ring so it isn't a hard aliased circle.
    float foveBlend = 0.0f;  // 0 = full warp (sweet spot), 1 = raw half-res periphery
    const cudaSurfaceObject_t nearestSurf = (blend >= 0.5f) ? currSurf : prevSurf;
    if (pose.foveation > 0.01f) {
        const float cx = uv.x - 0.5f, cy = uv.y - 0.5f;
        const float r = sqrtf(cx * cx + cy * cy) * 2.0f;  // 0 center .. 1 edge-midpoint
        const float inner = 1.0f - pose.foveation;        // sweet-spot radius
        const float feather = 0.08f;
        foveBlend = clamp01((r - inner) / feather);
        if (foveBlend >= 1.0f) {
            // Periphery: raw frame, NO warp, at REDUCED resolution. The block size
            // grows with how deep into the periphery the pixel is (smooth gradient,
            // clearly visible): ~4px just past the sweet spot up to ~16px at the
            // lens edge. (A 2x2 snap was imperceptible after the compositor's
            // downscale — this is the fix for "border not visible".)
            const float depth = clamp01((r - inner) / fmaxf(0.30f, 1.0f - inner));
            const int blk = 4 + static_cast<int>(depth * 12.0f);  // 4..16 px blocks
            const int sx = static_cast<int>(x) - (static_cast<int>(x) % blk);
            const int sy = static_cast<int>(y) - (static_cast<int>(y) % blk);
            uchar4 rawC;
            surf2Dread(&rawC, nearestSurf, sx * 4, sy);
            surf2Dwrite(rawC, outSurf, static_cast<int>(x * sizeof(uchar4)), static_cast<int>(y));
            return;
        }
    }
    // Inside the feather ring the refinement also tapers toward the periphery.
    const float foveRefine = 1.0f - foveBlend;

    // Sub-pixel BILINEAR same-eye temporal flow (prev->curr motion in px).
    float2 flowPx = sampleFlowBilinearPx<GRID_SIZE>(flowDevPtr, flowStride, x, y, outW, outH);
    // Idea #3 — NvOF flow 3x3 spatial low-pass. Per-frame optical-flow is noisy
    // on textured/animated surfaces, which shimmers the stale-eye warp. Average
    // the center cell with its 8 grid-cell neighbors and blend toward the
    // average by pose.flowSmooth (0 = off, raw flow; up to ~0.6 for stable but
    // slightly trailing motion). Weighted so a lone noisy cell can't drag the
    // average (median-ish: drop the most extreme neighbor). Cheap: 8 extra
    // short2 reads per pixel, no global memory traffic beyond the flow grid.
    if (pose.flowSmooth > 0.0f) {
        const int g = static_cast<int>(GRID_SIZE);
        float2 sum = flowPx; int cnt = 1;
        float maxDev2 = 0.0f; float2 worst = flowPx;
        #pragma unroll
        for (int dy = -g; dy <= g; dy += g) {
            #pragma unroll
            for (int dx = -g; dx <= g; dx += g) {
                if (dx == 0 && dy == 0) continue;
                const int nx = static_cast<int>(x) + dx;
                const int ny = static_cast<int>(y) + dy;
                if (nx < 0 || ny < 0 || nx >= static_cast<int>(outW) || ny >= static_cast<int>(outH)) continue;
                const float2 nf = sampleFlowBilinearPx<GRID_SIZE>(flowDevPtr, flowStride, nx, ny, outW, outH);
                sum.x += nf.x; sum.y += nf.y; ++cnt;
                const float dev2 = (nf.x - flowPx.x) * (nf.x - flowPx.x) + (nf.y - flowPx.y) * (nf.y - flowPx.y);
                if (dev2 > maxDev2) { maxDev2 = dev2; worst = nf; }
            }
        }
        if (cnt > 2) { sum.x -= worst.x; sum.y -= worst.y; cnt -= 1; }
        const float invc = 1.0f / static_cast<float>(cnt);
        const float2 avg = make_float2(sum.x * invc, sum.y * invc);
        const float w = pose.flowSmooth;
        flowPx = make_float2(flowPx.x + (avg.x - flowPx.x) * w, flowPx.y + (avg.y - flowPx.y) * w);
    }

    // ===== DLSS engine motion-vector refinement (the reference fuses MV + NvOF) =====
    // Engine MV is PER-PIXEL exact (no NvOF grid blockiness), but its sign/units
    // convention is engine-specific. AGREEMENT-GATE makes it self-correcting and
    // safe: convert MV to output-res pixels, try BOTH signs, and only adopt the
    // candidate that AGREES with the NvOF flow (same direction). If neither
    // agrees (wrong data / disocclusion) keep NvOF flow untouched -> MV can sharpen
    // but never corrupt. This is why the old "MV caused jitter" failure can't recur.
    if (haveMv) {
        const uint32_t mx = min(static_cast<uint32_t>(uv.x * engineMvW), engineMvW - 1);
        const uint32_t my = min(static_cast<uint32_t>(uv.y * engineMvH), engineMvH - 1);
        ushort2 rawMv;
        surf2Dread(&rawMv, mvSurf, static_cast<int>(mx * 4), static_cast<int>(my));
        float2 mvVal = make_float2(__half2float(__ushort_as_half(rawMv.x)),
                                   __half2float(__ushort_as_half(rawMv.y)));
        // MV -> output-resolution pixels (scale by NGX mvScale, then MV-res→out-res).
        const float sx = (static_cast<float>(outW) / static_cast<float>(engineMvW));
        const float sy = (static_cast<float>(outH) / static_cast<float>(engineMvH));
        float2 mvPx = make_float2(mvVal.x * engineMvScaleX * sx, mvVal.y * engineMvScaleY * sy);
        const float flowMag2 = flowPx.x * flowPx.x + flowPx.y * flowPx.y;
        const float mvMag2   = mvPx.x * mvPx.x + mvPx.y * mvPx.y;
        if (mvMag2 > 0.25f && mvMag2 < 4.0e6f && flowMag2 > 0.25f) {
            const float dotPos = flowPx.x * mvPx.x + flowPx.y * mvPx.y;     // +sign
            const float dotNeg = -dotPos;                                  // -sign
            float2 mvCand = (dotNeg > dotPos) ? make_float2(-mvPx.x, -mvPx.y) : mvPx;
            const float agree = (flowPx.x * mvCand.x + flowPx.y * mvCand.y) * rsqrtf(flowMag2 * (mvCand.x*mvCand.x + mvCand.y*mvCand.y) + 1e-6f);
            if (agree > 0.5f) {
                // Blend flow toward the sharper MV by how strongly they agree.
                const float w = clamp01((agree - 0.5f) * 2.0f) * 0.75f * clamp01(pose.refineStrength) * foveRefine;
                flowPx = make_float2(flowPx.x + (mvCand.x - flowPx.x) * w,
                                     flowPx.y + (mvCand.y - flowPx.y) * w);
            }
        }
    }

    // ===== Quaternion POSE-REPROJECTION refinement (the reference sub_193FF0/194130) =====
    // Camera motion (head turn) is the dominant displacement in VR. Using depth +
    // the render->predicted camera delta we can predict each pixel's screen-space
    // shift analytically. Like the MV path, AGREEMENT-GATE it against the NvOF
    // flow so a wrong matrix/sign (the old "fisheye") self-disables instead of
    // corrupting. Depth-gated. currToPred maps the curr render frame -> predicted.
    if (haveDepth) {
        const float dC = surf2Dbilinear_r32f(currDepthSurf, uv.x * outW - 0.5f, uv.y * outH - 0.5f, outW, outH);
        if (dC > 1.0e-5f) {
            const float2 poseUv = projectPoseUv(uv, dC, pose.currToPred, pose);
            float2 posePx = make_float2((poseUv.x - uv.x) * outW, (poseUv.y - uv.y) * outH);
            const float poseMag2 = posePx.x * posePx.x + posePx.y * posePx.y;
            const float flowMag2b = flowPx.x * flowPx.x + flowPx.y * flowPx.y;
            if (poseMag2 > 0.25f && poseMag2 < 4.0e6f && flowMag2b > 0.25f) {
                const float dp = flowPx.x * posePx.x + flowPx.y * posePx.y;
                float2 poseCand = (dp < 0.0f) ? make_float2(-posePx.x, -posePx.y) : posePx;
                const float agreeP = (flowPx.x * poseCand.x + flowPx.y * poseCand.y) *
                    rsqrtf(flowMag2b * (poseCand.x*poseCand.x + poseCand.y*poseCand.y) + 1e-6f);
                if (agreeP > 0.6f) {
                    const float w = clamp01((agreeP - 0.6f) * 2.5f) * 0.5f * clamp01(pose.refineStrength) * foveRefine;
                    flowPx = make_float2(flowPx.x + (poseCand.x - flowPx.x) * w,
                                         flowPx.y + (poseCand.y - flowPx.y) * w);
                }
            }
        }
    }

    const float2 flowUv = make_float2(flowPx.x * invSize.x, flowPx.y * invSize.y);

    // NvOF cost-map confidence (per cell). High cost = unreliable vector.
    float confidence = 1.0f;
    if (costMapDevPtr && costStride > 0) {
        const uint32_t cY = y / GRID_SIZE;
        const uint32_t cX = x / GRID_SIZE;
        const unsigned char* costRow = reinterpret_cast<const unsigned char*>(costMapDevPtr) + static_cast<size_t>(cY) * costStride;
        confidence = 1.0f - static_cast<float>(costRow[cX]) * (1.0f / 255.0f);
    }
    confidence = clamp01(confidence);

    auto sampleAt = [&](cudaSurfaceObject_t s, float2 suv) -> float4 {
        return surf2Dbilinear_rgba8(s, suv.x * outW - 0.5f, suv.y * outH - 0.5f, outW, outH);
    };

    float4 outColor;
    if (blend <= 1.0f) {
        // ===== BIDIRECTIONAL motion-compensated interpolation =====
        // The pixel exists in BOTH frames: trace it back into prev and forward
        // into curr, then blend. Using both sources (not just prev) fills the
        // areas one side occludes -> removes the torn "chunks".
        const float2 prevUv = make_float2(uv.x - flowUv.x * blend,        uv.y - flowUv.y * blend);
        const float2 currUv = make_float2(uv.x + flowUv.x * (1.0f - blend), uv.y + flowUv.y * (1.0f - blend));
        const bool prevOk = uvInBounds(prevUv);
        const bool currOk = uvInBounds(currUv);
        const float4 cPrev = sampleAt(prevSurf, make_float2(clamp01(prevUv.x), clamp01(prevUv.y)));
        const float4 cCurr = sampleAt(currSurf, make_float2(clamp01(currUv.x), clamp01(currUv.y)));
        // Weight toward curr = blend, but if one side is disoccluded (out of
        // frame) take the other entirely. Low confidence -> bias to the nearest
        // real frame instead of trusting a bad vector.
        float w = blend;
        if (prevOk && !currOk) w = 0.0f;
        else if (!prevOk && currOk) w = 1.0f;
        const float hardPick = (blend < 0.5f) ? 0.0f : 1.0f;  // nearest real frame in time
        w = w + (1.0f - confidence) * (hardPick - w);

        // DEPTH-AWARE OCCLUSION: sample scene depth (reversed-Z, larger = NEARER)
        // at each side's source location. At a depth discontinuity the two sides
        // see different surfaces; pick the NEARER one (it occludes the farther),
        // which removes the foreground/background halo that a luma-only test
        // misses. Guarded — when depth wasn't imported, falls back to photometric.
        if (haveDepth) {
            const float dP = surf2Dbilinear_r32f(prevDepthSurf, clamp01(prevUv.x) * outW - 0.5f, clamp01(prevUv.y) * outH - 0.5f, outW, outH);
            const float dC = surf2Dbilinear_r32f(currDepthSurf, clamp01(currUv.x) * outW - 0.5f, clamp01(currUv.y) * outH - 0.5f, outW, outH);
            if (dP > 1.0e-5f && dC > 1.0e-5f) {              // both valid (not sky/far)
                const float ddiff = fabsf(dP - dC);
                if (ddiff > 0.02f) {                         // real depth edge
                    const float depthPick = (dP > dC) ? 0.0f : 1.0f;  // nearer side wins
                    const float occ = clamp01((ddiff - 0.02f) / 0.05f);
                    w = w + (depthPick - w) * occ;
                }
            }
        }

        // OCCLUSION-AWARE: where prev and curr disagree strongly (a moving edge /
        // occlusion boundary), a 50/50 blend produces a semi-transparent GHOST.
        // Detect the disagreement (luma delta) and sharpen the weight toward the
        // nearest-in-time frame there, so edges stay crisp; consistent regions
        // keep the smooth interpolation that hides judder.
        const float lumaPrev = 0.299f * cPrev.x + 0.587f * cPrev.y + 0.114f * cPrev.z;
        const float lumaCurr = 0.299f * cCurr.x + 0.587f * cCurr.y + 0.114f * cCurr.z;
        const float disagree = fabsf(lumaPrev - lumaCurr);
        const float sharpen = clamp01((disagree - 0.12f) / 0.28f) * clamp01(pose.occlusionSharp);  // overlay-tunable
        w = w + (hardPick - w) * sharpen;

        outColor = make_float4(
            cPrev.x + (cCurr.x - cPrev.x) * w,
            cPrev.y + (cCurr.y - cPrev.y) * w,
            cPrev.z + (cCurr.z - cPrev.z) * w,
            1.0f);
    } else {
        // ===== EXTRAPOLATION (stale eye, blend>1) =====
        // No data past curr; continue motion FROM the most recent real frame
        // (curr) — its content is freshest. Fade to raw curr as the displacement
        // grows or the vector is unreliable, so fast turns degrade to a calm
        // frozen pixel rather than a smear.
        const float ext = blend - 1.0f;  // how far past curr
        // OBJECT motion (NvOF): unreliable at large magnitude -> fade to no-op so a
        // bad vector degrades to a calm frozen pixel instead of a smear.
        const float2 flowOff = make_float2(flowUv.x * ext, flowUv.y * ext);
        const float offMag = sqrtf(flowOff.x * flowOff.x + flowOff.y * flowOff.y);
        const float kMaxShift = 0.04f;
        float warpW = confidence;
        if (offMag > kMaxShift) warpW *= clamp01(1.0f - (offMag - kMaxShift) / kMaxShift);
        float2 srcUv = make_float2(uv.x - flowOff.x * warpW, uv.y - flowOff.y * warpW);

        // CAMERA motion (pose + depth): the DOMINANT displacement on head turns and
        // the part NvOF cannot extrapolate (it only sees prev->curr object motion).
        // This was the stale eye's missing piece — with no camera comp the whole
        // world tore/judder on turns. Reliable when depth is valid, so apply it
        // FULLY (bounded), NOT faded like the object flow. Gather = undo the
        // curr->target camera shift. Scaled by refineStrength so the SAME overlay
        // slider that gates the interpolation pose-refine also gates this (default
        // 0 = unchanged behavior until the corrected math is validated in-headset).
        const float refine = clamp01(pose.refineStrength);
        if (haveDepth && refine > 0.0f) {
            const float dCx = surf2Dbilinear_r32f(currDepthSurf, uv.x * outW - 0.5f, uv.y * outH - 0.5f, outW, outH);
            if (dCx > 1.0e-5f) {
                const float2 poseUv = projectPoseUv(uv, dCx, pose.currToPred, pose);
                float2 camOff = make_float2(poseUv.x - uv.x, poseUv.y - uv.y);
                const float camMag = sqrtf(camOff.x * camOff.x + camOff.y * camOff.y);
                const float kMaxCam = 0.15f;  // bound bad-depth blow-ups
                const float camScale = (camMag > kMaxCam) ? (kMaxCam / camMag) : 1.0f;
                const float cw = refine * foveRefine * camScale;
                srcUv.x -= camOff.x * cw;
                srcUv.y -= camOff.y * cw;
            }
        }

        if (!uvInBounds(srcUv)) srcUv = uv;  // disocclusion -> raw curr
        outColor = sampleAt(currSurf, make_float2(clamp01(srcUv.x), clamp01(srcUv.y)));
    }

    // Foveation feather band: fade the warped sweet-spot color toward the
    // reduced-res raw periphery (4px blocks) so the inner/outer boundary is a
    // soft gradient, not a hard ring.
    if (foveBlend > 0.0f) {
        const int sx = static_cast<int>(x) & ~3;
        const int sy = static_cast<int>(y) & ~3;
        uchar4 rawC;
        surf2Dread(&rawC, nearestSurf, sx * 4, sy);
        const float inv = 1.0f / 255.0f;
        outColor.x += (rawC.x * inv - outColor.x) * foveBlend;
        outColor.y += (rawC.y * inv - outColor.y) * foveBlend;
        outColor.z += (rawC.z * inv - outColor.z) * foveBlend;
    }

    const uchar4 packed = pack_color(outColor, outputFormat);
    surf2Dwrite(packed, outSurf, static_cast<int>(x * sizeof(uchar4)), static_cast<int>(y));
}

extern "C" __global__ void aer_v2_warp_1x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<1>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_2x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<2>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_4x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<4>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_8x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<8>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_16x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<16>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_32x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<32>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
extern "C" __global__ void aer_v2_warp_64x_depth(
    cudaTextureObject_t prevColorTex, cudaTextureObject_t currColorTex,
    const short2* flowDevPtr, uint32_t flowStride, uint32_t flowGridSize,
    cudaTextureObject_t engineMvTex, float engineMvScaleX, float engineMvScaleY,
    uint32_t engineMvW, uint32_t engineMvH,
    cudaTextureObject_t prevDepthTex, cudaTextureObject_t currDepthTex,
    const void* costMapDevPtr, uint32_t costStride,
    cudaSurfaceObject_t outSurf, uint32_t outW, uint32_t outH, uint32_t outputFormat,
    WarpPoseParams pose, float blend) {
    aer_v2_warp_depth_impl<64>(prevColorTex, currColorTex, flowDevPtr, flowStride, flowGridSize,
        engineMvTex, engineMvScaleX, engineMvScaleY, engineMvW, engineMvH,
        prevDepthTex, currDepthTex, costMapDevPtr, costStride,
        outSurf, outW, outH, outputFormat, pose, blend);
}
