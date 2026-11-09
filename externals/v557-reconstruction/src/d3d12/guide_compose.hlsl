// V516: one eye's game motion from MSFS 2024's two foveated DLSS views.
//
// With foveated rendering MSFS evaluates DLSS per view - per eye a periphery
// over the whole field at FoveatedRenderingScale of the eye's pixels, and an
// inset at full density where the eye looks - and composites them into the
// eye image itself. V515 placed the periphery's vectors over the whole eye
// (5.7 eye pixels a vector in the user's setup). Here the inset's vectors,
// two eye pixels a vector, replace them inside the inset, and the result is
// written as one field in eye pixels at half the eye's resolution, with its
// depth, which the hybrid composition then reads as any guide.
//
// The inset follows the eyes, so its field moves between frames. Whether
// MSFS's inset vectors lead into the previous frame's inset (they include
// the move: reading A) or stay in the current inset's frame (reading B) is
// measured every frame: both readings are compared with the periphery's
// vectors over the inset, and the composition takes the one that agrees.
//
// Same root signature as frame_synthesis.hlsl: an SRV table t0-t9, a UAV
// table u0-u2 and root constants at b0.

Texture2DArray<float2> PeripheryVectors : register(t0);
Texture2DArray<float> PeripheryDepth : register(t1);
Texture2DArray<float2> InsetVectors : register(t2);
Texture2DArray<float> InsetDepth : register(t3);
RWTexture2DArray<float2> ComposedVectors : register(u0);
RWTexture2DArray<float> ComposedDepth : register(u1);
// Per eye four words: reading A's summed error, B's, the samples, unused.
RWByteAddressBuffer Verdict : register(u2);

cbuffer ComposeConstants : register(b0) {
    uint FieldWidth;
    uint FieldHeight;
    uint Slice;
    // Bit 0 the inset is present, 1 its previous field is known, 2 the
    // periphery's depth is bound, 3 the inset's.
    uint Flags;
    float4 EyeRect;              // the eye's image rectangle, image pixels
    float4 PeripheryMotionRect;  // the periphery's vectors under EyeRect, texels
    float2 PeripheryScale;       // periphery vector -> image pixels
    float2 InsetVectorScale;     // inset vector -> inset output pixels
    float4 InsetRect;            // the inset's field now, image pixels
    float4 InsetPreviousRect;    // and at its previous evaluation
    float4 InsetMotionRect;      // the inset's vectors, texels
    float2 InsetOutputSize;      // the inset's DLSS output, pixels
    float InsetMargin;           // image pixels kept off the inset's border
    uint VerdictBase;            // this eye's words in Verdict, bytes
};

static const float kErrorCap = 32.0;
static const uint kMeasureGrid = 32;

int2 nearest_texel(float4 rect, float2 uv) {
    float2 coordinate = rect.xy + uv * rect.zw;
    float2 maximum = rect.xy + rect.zw - 1.0;
    return int2(clamp(floor(coordinate), rect.xy, maximum));
}

// The periphery's motion at an image pixel, in image pixels.
float2 periphery_motion(float2 pixel) {
    float2 uv = (pixel - EyeRect.xy) / max(EyeRect.zw, float2(1.0, 1.0));
    int2 texel = nearest_texel(PeripheryMotionRect, uv);
    return PeripheryVectors.Load(int4(texel, 0, 0)) * PeripheryScale;
}

float periphery_depth(float2 pixel) {
    float2 uv = (pixel - EyeRect.xy) / max(EyeRect.zw, float2(1.0, 1.0));
    return PeripheryDepth.Load(int4(nearest_texel(PeripheryMotionRect, uv), 0, 0));
}

bool inside_inset(float2 pixel) {
    return (Flags & 1u) != 0u &&
        all(pixel >= InsetRect.xy + InsetMargin) &&
        all(pixel <= InsetRect.xy + InsetRect.zw - InsetMargin);
}

// The inset's vector at an image pixel inside it, in inset output pixels,
// and the pixel's place in the inset's output.
// Reading A: the vector leads into the previous evaluation's inset, which
// sat at InsetPreviousRect. Reading B: it stays in the current inset.
float2 inset_motion(float2 pixel, bool previous_inset) {
    float2 uv = (pixel - InsetRect.xy) / max(InsetRect.zw, float2(1.0, 1.0));
    float2 output_size = max(InsetOutputSize, float2(1.0, 1.0));
    float2 inset_pixel = uv * output_size;
    int2 texel = nearest_texel(InsetMotionRect, uv);
    // In the inset's output pixels.
    float2 vector_pixels = InsetVectors.Load(int4(texel, 0, 0)) * InsetVectorScale;
    float2 reading_a = InsetPreviousRect.xy +
        (inset_pixel + vector_pixels) / output_size * InsetPreviousRect.zw - pixel;
    float2 reading_b = vector_pixels / output_size * InsetRect.zw;
    return previous_inset ? reading_a : reading_b;
}

[numthreads(8, 1, 1)]
void ClearInsetVerdict(uint3 id : SV_DispatchThreadID) {
    Verdict.Store(id.x * 4u, 0u);
}

// A 32 x 32 grid of samples over the inset, both readings against the
// periphery: errors capped so a motion edge (where the coarse periphery is
// wrong for both) cannot decide it.
[numthreads(8, 8, 1)]
void MeasureInsetMotion(uint3 id : SV_DispatchThreadID) {
    if ((Flags & 3u) != 3u || id.x >= kMeasureGrid || id.y >= kMeasureGrid) return;
    float2 inner = max(InsetRect.zw - 2.0 * InsetMargin, float2(1.0, 1.0));
    float2 pixel = InsetRect.xy + InsetMargin + (float2(id.xy) + 0.5) / kMeasureGrid * inner;
    float2 reference = periphery_motion(pixel);
    float error_a = min(length(inset_motion(pixel, true) - reference), kErrorCap);
    float error_b = min(length(inset_motion(pixel, false) - reference), kErrorCap);
    uint ignored;
    Verdict.InterlockedAdd(VerdictBase + 0u, uint(error_a * 16.0), ignored);
    Verdict.InterlockedAdd(VerdictBase + 4u, uint(error_b * 16.0), ignored);
    Verdict.InterlockedAdd(VerdictBase + 8u, 1u, ignored);
}

[numthreads(8, 8, 1)]
void ComposeInsetGuide(uint3 id : SV_DispatchThreadID) {
    if (id.x >= FieldWidth || id.y >= FieldHeight) return;
    float2 pixel = EyeRect.xy +
        (float2(id.xy) + 0.5) / float2(FieldWidth, FieldHeight) * EyeRect.zw;
    float2 motion;
    float depth = 0.0;
    if (inside_inset(pixel)) {
        bool previous_inset = false;
        if ((Flags & 2u) != 0u) {
            uint error_a = Verdict.Load(VerdictBase + 0u);
            uint error_b = Verdict.Load(VerdictBase + 4u);
            uint samples = Verdict.Load(VerdictBase + 8u);
            previous_inset = samples != 0u && error_a < error_b;
        }
        motion = inset_motion(pixel, previous_inset);
        if ((Flags & 8u) != 0u) {
            float2 uv = (pixel - InsetRect.xy) / max(InsetRect.zw, float2(1.0, 1.0));
            depth = InsetDepth.Load(int4(nearest_texel(InsetMotionRect, uv), 0, 0));
        } else if ((Flags & 4u) != 0u) {
            depth = periphery_depth(pixel);
        }
    } else {
        motion = periphery_motion(pixel);
        if ((Flags & 4u) != 0u) depth = periphery_depth(pixel);
    }
    ComposedVectors[uint3(id.xy, Slice)] = motion;
    ComposedDepth[uint3(id.xy, Slice)] = depth;
}
