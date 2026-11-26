//! FSR3's and Neural Rendering's passes as main compiled them (main's 2e123a4a3: src/Framegen/Fsr3Dilate.cpp,
//! Fsr3Extrapolate.cpp and src/Render/NeuralRender.cpp, HLSL through D3DCompile at first use, cs_5_0, flags 0): the
//! reference of the WARP bench below for the DXIL made from Rust (rust/shaders/passes/src/fsr3.rs, neural_render.rs),
//! built with the tests only.

/// Fsr3Dilate.cpp's kDilateHLSL: Clear, ReconstructAndDilate, Resolve.
/// `source`'s entry compiled as the C++ build did (D3DCompile at first use, cs_5_0, flags 0).
pub(crate) fn compile_cs(source: &str, entry: &str, what: &str) -> Option<windows::Win32::Graphics::Direct3D::ID3DBlob> {
    use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
    let entry_c = std::ffi::CString::new(entry).ok()?;
    let mut code = None;
    let mut error = None;
    // SAFETY: the source and the names live through the call; the blobs come back owned.
    let ok = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            None,
            None,
            None,
            windows::core::PCSTR(entry_c.as_ptr().cast()),
            windows::core::s!("cs_5_0"),
            0,
            0,
            &mut code,
            Some(&mut error),
        )
    }
    .is_ok();
    if !ok {
        let text = error
            .map(|e| {
                // SAFETY: the error blob's own text.
                unsafe { String::from_utf8_lossy(core::slice::from_raw_parts(e.GetBufferPointer() as *const u8, e.GetBufferSize())).into_owned() }
            })
            .unwrap_or_default();
        cvr_host::log!("[{what}] compile {entry} failed: {text}\n");
        return None;
    }
    code
}
pub(crate) const DILATE: &str = r#"
cbuffer K : register(b0) {
    uint2 RenderSize;
    float2 MvScale;
    uint InvertedDepth;
    float ClearValue;
};
Texture2D<float>  Depth          : register(t0);
Texture2D<float2> MotionVectors  : register(t1);
RWTexture2D<float>  DilatedDepthOut   : register(u0);
RWTexture2D<float2> DilatedMvOut      : register(u1);
RWTexture2D<uint>   PrevDepthScratch  : register(u2);
RWTexture2D<float>  PrevDepthOut      : register(u3);

bool IsOnScreen(int2 p, int2 size) { return all(p >= 0) && all(p < size); }
bool DepthCloser(float a, float b) { return InvertedDepth != 0 ? (a > b) : (a < b); }

static const int2 kTaps[9] = {
    int2(0,0), int2(1,0), int2(0,1), int2(0,-1), int2(-1,0),
    int2(-1,1), int2(1,1), int2(-1,-1), int2(1,-1)
};

[numthreads(8,8,1)]
void Clear(uint3 id : SV_DispatchThreadID) {
    if (id.x >= RenderSize.x || id.y >= RenderSize.y) return;
    PrevDepthScratch[id.xy] = asuint(ClearValue);
}

[numthreads(8,8,1)]
void Resolve(uint3 id : SV_DispatchThreadID) {
    if (id.x >= RenderSize.x || id.y >= RenderSize.y) return;
    PrevDepthOut[id.xy] = asfloat(PrevDepthScratch[id.xy]);
}

[numthreads(8,8,1)]
void ReconstructAndDilate(uint3 id : SV_DispatchThreadID) {
    int2 pos = int2(id.xy);
    if (pos.x >= (int)RenderSize.x || pos.y >= (int)RenderSize.y) return;

    float nearestDepth = Depth.Load(int3(pos, 0));
    int2 nearestCoord = pos;
    [unroll]
    for (int i = 1; i < 9; ++i) {
        int2 p = pos + kTaps[i];
        if (!IsOnScreen(p, (int2)RenderSize)) continue;
        float d = Depth.Load(int3(p, 0));
        if (DepthCloser(d, nearestDepth)) { nearestDepth = d; nearestCoord = p; }
    }

    float2 mv = MotionVectors.Load(int3(nearestCoord, 0)) * MvScale;
    DilatedDepthOut[pos] = nearestDepth;
    DilatedMvOut[pos] = mv;

    float2 mvPixels = mv * float2(RenderSize);
    if (length(mvPixels) <= 0.1) mv = float2(0, 0);
    float2 uv = (float2(pos) + 0.5) / float2(RenderSize);
    float2 reprojPx = (uv + mv) * float2(RenderSize) - 0.5;
    int2 basePos = int2(floor(reprojPx));
    float2 frac = reprojPx - float2(basePos);
    const int2 offs[4] = { int2(0,0), int2(1,0), int2(0,1), int2(1,1) };
    float weights[4] = {
        (1 - frac.x) * (1 - frac.y), frac.x * (1 - frac.y),
        (1 - frac.x) * frac.y,       frac.x * frac.y
    };
    uint depthBits = asuint(nearestDepth);
    [unroll]
    for (int j = 0; j < 4; ++j) {
        if (weights[j] <= 0.01) continue;
        int2 storePos = basePos + offs[j];
        if (!IsOnScreen(storePos, (int2)RenderSize)) continue;
        if (InvertedDepth != 0) {
            InterlockedMax(PrevDepthScratch[storePos], depthBits);
        } else {
            InterlockedMin(PrevDepthScratch[storePos], depthBits);
        }
    }
}
"#;

/// Fsr3Extrapolate.cpp's kWarpHLSL.
pub(crate) const WARP: &str = r#"
cbuffer K : register(b0) {
    uint2 DisplaySize;
    uint2 RenderSize;
    float Factor;
};
Texture2D<float4> Color   : register(t0);
Texture2D<float2> Motion  : register(t1);
RWTexture2D<float4> Output : register(u0);
SamplerState Linear : register(s0);

[numthreads(8,8,1)]
void Warp(uint3 id : SV_DispatchThreadID) {
    if (id.x >= DisplaySize.x || id.y >= DisplaySize.y) return;
    const float2 q = (float2(id.xy) + 0.5) / float2(DisplaySize);
    float2 s = q;
    [unroll]
    for (int i = 0; i < 3; ++i) {
        const int2 m = clamp(int2(s * float2(RenderSize)), int2(0, 0), int2(RenderSize) - 1);
        s = q + Factor * Motion.Load(int3(m, 0));
    }
    Output[id.xy] = Color.SampleLevel(Linear, saturate(s), 0);
}
"#;

/// NeuralRender.cpp's kCodecHLSL: Encode, Decode.
pub(crate) const CODEC: &str = r#"
cbuffer K : register(b0) { uint W; uint H; float PaperWhite; float ColorStrength; uint OffX; uint OffY; };
Texture2D<float4>   Src : register(t0);
RWTexture2D<float4> Dst : register(u0);

float3 SrgbEncode(float3 c){ c=saturate(c); return (c<=0.0031308)?(c*12.92):(1.055*pow(c,1.0/2.4)-0.055); }
float3 SrgbDecode(float3 c){ return (c<=0.04045)?(c/12.92):pow((c+0.055)/1.055,2.4); }

[numthreads(8,8,1)]
void Encode(uint3 id : SV_DispatchThreadID) {
    if (id.x >= W || id.y >= H) return;
    float pw = max(1e-4, PaperWhite);
    float3 c = max(0.0.xxx, Src[id.xy + uint2(OffX, OffY)].rgb) / pw;
    c = c / (1.0 + c);
    Dst[id.xy] = float4(SrgbEncode(c), 1.0);
}

[numthreads(8,8,1)]
void Decode(uint3 id : SV_DispatchThreadID) {
    if (id.x >= W || id.y >= H) return;
    uint2 g = id.xy + uint2(OffX, OffY);
    float pw = max(1e-4, PaperWhite);
    float3 n = SrgbDecode(saturate(Src[id.xy].rgb));
    float4 o = Dst[g];
    float ny = dot(n, float3(0.2126, 0.7152, 0.0722));
    if (ny <= 1e-5) return;
    float3 c = (n / max(1e-4, 1.0 - n)) * pw;
    float oy = dot(max(0.0.xxx, o.rgb), float3(0.2126, 0.7152, 0.0722));
    float cy = dot(max(0.0.xxx, c), float3(0.2126, 0.7152, 0.0722));
    if (oy > 1e-5 && cy > 1e-5) c *= (clamp(cy / oy, 0.0, 4.0) * oy) / cy;
    Dst[g] = float4(max(0.0.xxx, lerp(o.rgb, c, max(0.0, ColorStrength))), o.a);
}
"#;

#[cfg(test)]
mod shader_parity;
