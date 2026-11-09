Texture2DArray<float4> PreviousFrame : register(t0);
Texture2DArray<float4> CurrentFrame : register(t1);
Texture2D<int2> BackwardFlow : register(t2);
Texture2D<uint> FlowAuxiliary : register(t3);
Texture2D<int2> ForwardFlow : register(t4);
Texture2D<uint> ForwardAuxiliary : register(t5);
Texture2DArray<float2> GameMotionVectors : register(t6);
#ifdef XRFG_HYBRID
// The game's depth beside its vectors, at the same render resolution (V505).
Texture2DArray<float> GameDepth : register(t7);
// t9 was the depth search's tiles (V530-V554); the midpoint splat at t11
// replaced them in V555.
// V539: the coverage mask (BuildCoverageMask below): per vector texel of
// the previous frame's image, the surface of the current frame that came
// from there - its vector, and how near or how fast it is - so a pixel
// whose A sample lies under another surface knows it is background that
// surface has uncovered, however far the surface has moved since.
Texture2DArray<uint> CoverageMask : register(t10);
// V554: the depth search's midpoint splat (BuildDepthSplat below), a slice
// per synthetic output of this eye: per vector texel of the generated
// image, the surface of the current frame that occupies it at the generated
// instant - the nearest by the game's depth - as the mask's word: its
// displacement to the previous frame, and how near it is.
Texture2DArray<uint> DepthSplat : register(t11);
#endif
// Linear filtering with clamped addressing, a static sampler in the root
// signature: one fetch per bilinear tap where four loads and three lerps used
// to be written by hand (V458; measured 0.10 ms per eye less composition on
// an RTX 5090, same image).
SamplerState LinearClamp : register(s0);
RWTexture2D<float4> PackedColor : register(u0);
// The NVIDIA pack is compiled twice: once for BGRA8 inputs, and once with
// XRFG_NVIDIA_LUMA_INPUT for R8 inputs, which the engine takes as grayscale
// and which is a quarter of the bytes for the same flow - it matches on
// luma either way. The C++ side picks the pipeline by the format the driver
// accepted at creation.
#ifdef XRFG_NVIDIA_LUMA_INPUT
RWTexture2D<float> NvidiaPreviousColor : register(u1);
RWTexture2D<float> NvidiaCurrentColor : register(u2);
#else
RWTexture2D<float4> NvidiaPreviousColor : register(u1);
RWTexture2D<float4> NvidiaCurrentColor : register(u2);
#endif
#ifdef XRFG_COVERAGE_MASK
// The pass that writes the coverage mask (V539), on the pack's register,
// which that pass does not use.
RWTexture2DArray<uint> CoverageMaskOut : register(u0);
#endif
#ifdef XRFG_DEPTH_SPLAT
// V554: the pass that writes the depth splat, every eye's and output's
// slice, on the pack's register likewise.
RWTexture2DArray<uint> DepthSplatOut : register(u0);
#endif
#ifdef XRFG_EYE_ORDER
// V546: the eye-order measurement (MeasureEyeOrder below), in a descriptor
// block of its own: the other stream's vectors on the coverage mask's
// register and its verdict on the pack's, neither of which it uses.
Texture2DArray<float2> AlternateMotionVectors : register(t10);
RWByteAddressBuffer EyeOrderVerdict : register(u0);
#endif

struct CameraMapping {
    float4 TargetToSourceRotation;
    float4 SourceTangents;
    float4 TargetTangents;
    float4 SourceRect;
    float4 TargetRect;
};

cbuffer SynthesisParameters : register(b0) {
    uint Width;
    uint Height;
    uint ArraySize;
    uint PackedWidth;
    uint PackedHeight;
    uint PackedEyeStride;
    uint FlowWidth;
    uint FlowHeight;
    uint FlowBlockSize;
    uint ViewIndex;
    uint UseGameMotion;
    uint GameMotionPadding;
    float4 GameMotionRect;
    float2 GameMotionScale;
    float2 GameMotionJitterDelta;
    CameraMapping PreviousMappings[2];
    uint Slice;
    // Bit 0 is the repeated-capture flag. Bit 1 asks the pack shaders to
    // sRGB-encode the flow input. Bits 8-23 carry the synthetic's position
    // between the two captures in 1/65535ths - 0 at the previous capture, 1
    // at the current. They share a slot because the root signature is full:
    // 62 constants plus two descriptor tables is the whole 64-DWORD budget,
    // leaving no room for a 63rd.
    uint SynthesisFlags;
};

uint repeated_capture_flag() {
    return SynthesisFlags & 1u;
}

// The images are a flat panel's (V510): see choose_midpoint_motion.
bool flat_panel() {
    return (SynthesisFlags & 4u) != 0u;
}

// V539: bit 4, this eye's coverage mask is at t10.
bool coverage_mask_available() {
    return (SynthesisFlags & 16u) != 0u;
}

// V554: bit 5, this eye's depth splat is at t11, a slice per synthetic
// output; bits 27-28 which output this draw (or splat pass) is. (Bit 3 and
// bits 24-26 were the depth tiles' until V555.)
bool depth_splat_available() {
    return (SynthesisFlags & 32u) != 0u;
}
uint depth_splat_output() {
    return (SynthesisFlags >> 27) & 3u;
}

// The tray's live tuning window (xrfg::live_tuning), for the hybrid
// composition only since V507 (the built-in values below are
// live_tuning::kDefaults, the user's DI2 set of 2026-10-19): the synthesizer uploads its block, header and
// all, into a small buffer at t8 for every hybrid pair - the window's values
// while it is open, the built-in ones otherwise - so an accessor reads one
// uniform word. A buffer without the magic (a null view) yields the built-in
// values too. The other compositions have no tunable values left: the
// repeating-pattern lock, which the flow composition carried in its free
// game-motion constants, left with V507.
//
// The hybrid composition (compiled with XRFG_HYBRID): the game's vectors and
// the optical flow both, chosen per pixel. Its game-motion constants carry
// the guide rectangle for every draw, so the DLSS output rectangle travels
// packed, 16 bits a value: UseGameMotion = x | y << 16, GameMotionPadding =
// width | height << 16, bits 15 and 31 the depth flags.
#ifdef XRFG_HYBRID
ByteAddressBuffer HybridTuning : register(t8);

static const uint kTuningMagic = 0x4C565446u;
static const uint kTuningVersion = 11u;
// Field indices: xrfg::live_tuning::Block's order after its header.
static const uint kTuningAgree = 0u;
static const uint kTuningChoiceSlope = 1u;
static const uint kTuningVectorsSlope = 2u;
static const uint kTuningFlowRuleSlope = 3u;
static const uint kTuningEdgeReach = 4u;
static const uint kTuningEdgeSolves = 5u;
static const uint kTuningEdgeSurfaceAgree = 6u;
static const uint kTuningRevealedError = 7u;
static const uint kTuningVectorSnap = 8u;
static const uint kTuningDepthSearch = 9u;
static const uint kTuningDepthNearer = 10u;
static const uint kTuningDepthAgree = 11u;
static const uint kTuningTint = 12u;
static const uint kTuningMaskBand = 13u;

bool tuning_present() {
    return HybridTuning.Load(0u) == kTuningMagic && HybridTuning.Load(4u) == kTuningVersion;
}

uint tuning_value(uint index, uint built_in) {
    return tuning_present() ? HybridTuning.Load(12u + 4u * index) : built_in;
}

// How far the last synthesize_midpoint's two warped samples disagreed, blurred
// a little, and lerped toward the pose-only blend's own disagreement where it
// fell back to that: what the hybrid chooses by. 1 where it had no samples.
static float last_disagreement = 1.0;
// Diagnostic tint (V539, the tray's tuning window, off at every start):
// what the vectors branch did with the last pixel. 0 plain vectors, 1 covered background (A alone), 2 revealed
// background (B alone), 3 the camera alone, 4 a motion edge not verified
// (B lerped with the blend), 5 a second surface found, 6 the depth search's
// nearer surface.
static uint last_vectors_case = 0u;
// And the coverage mask's verdict for the last pixel: 0 not consulted, 1 no
// mask, 2 nothing of B came from the A sample, 3 the pixel's own surface,
// 4 another surface.
static uint last_mask_state = 0u;
// Why a mask verdict of another surface was not acted on: 0 it was, 1 the
// camera alone explains the pixel, 2 it is covered background, 3 a repeated
// capture. fxc evaluates both sides of &&, so the verdict is read for these
// pixels too.
static uint last_mask_gate = 0u;
// 0 off, 1 tinted by last_vectors_case (and the flow's share), 2 by
// last_mask_state.
uint hybrid_tint_mode() { return min(tuning_value(kTuningTint, 0u), 2u); }
// V543: the coverage mask's band, a share of each surface's motion.
float coverage_mask_band() { return float(min(tuning_value(kTuningMaskBand, 0u), 50u)) / 100.0; }

// Where the vectors' warped samples disagree by less, they are taken as they
// are and the flow is not asked.
float hybrid_agree_threshold() {
    return float(min(tuning_value(kTuningAgree, 62u), 62u)) / 255.0;
}
float hybrid_choice_slope() {
    return float(clamp(tuning_value(kTuningChoiceSlope, 8u), 1u, 15u));
}
// Game motion's disagreements are mostly resampling at sharp detail, which a
// gentle slope keeps on the warp (0.3's value).
float game_motion_confidence_slope() {
    return float(clamp(tuning_value(kTuningVectorsSlope, 3u), 1u, 15u));
}
float hybrid_flow_rule_slope() {
    return float(clamp(tuning_value(kTuningFlowRuleSlope, 8u), 1u, 15u));
}
float edge_reach_factor() {
    return float(clamp(tuning_value(kTuningEdgeReach, 100u), 100u, 200u)) / 100.0;
}
uint edge_search_solves() { return min(tuning_value(kTuningEdgeSolves, 1u), 4u); }
float edge_surface_agree() {
    return float(min(tuning_value(kTuningEdgeSurfaceAgree, 50u), 250u)) / 1000.0;
}
float revealed_error_limit() {
    return float(min(tuning_value(kTuningRevealedError, 0u), 250u)) / 1000.0;
}
float vector_snap_pixels() {
    return float(min(tuning_value(kTuningVectorSnap, 0u), 40u)) / 10.0;
}
bool depth_search_enabled() { return tuning_value(kTuningDepthSearch, 1u) != 0u; }
float depth_nearer_factor() {
    return 1.0 + float(min(tuning_value(kTuningDepthNearer, 50u), 50u)) / 100.0;
}
float depth_agree_limit() {
    return float(min(tuning_value(kTuningDepthAgree, 250u), 250u)) / 1000.0;
}
#else
// The vectors-only composition runs the built-in values.
float game_motion_confidence_slope() { return 3.0; }
float edge_reach_factor() { return 1.25; }
uint edge_search_solves() { return 2u; }
float edge_surface_agree() { return 0.05; }
float vector_snap_pixels() { return 1.0; }
#endif

// Edge ghosting reduction is off on this branch (V497): a constant lets the
// compiler drop the search from every composition, and its values are the
// built-in ones, the set chosen in the headset on V475 (2026-10-17).
bool edge_ghosting_reduction_enabled() { return false; }
uint midpoint_search_rings() { return 3u; }
uint midpoint_survivor_limit() { return 1u; }
float midpoint_spread_trigger() { return 8.0; }
float midpoint_error_trigger() { return 24.0 / 255.0; }
float midpoint_survivor_margin() { return 2.0 / 255.0; }
float fixed_point_tolerance() { return 1.0; }
float fixed_point_speed_tolerance() { return 0.1; }

// The source views are the swapchain's own format. On an sRGB swapchain a
// Load returns linear light, which is what the composition needs to write
// back through its sRGB render target - but written as-is into the 8-bit
// flow input it leaves the darkest tenth of the perceptual range in two or
// three codes, and the optical flow matches on that. Filtering and the luma
// weighting stay in linear light; the encode is applied to the packed value.
bool encode_srgb_flow_input() {
    return (SynthesisFlags & 2u) != 0u;
}

float3 linear_to_srgb(float3 linear_color) {
    float3 low = linear_color * 12.92;
    float3 high = 1.055 * pow(max(linear_color, 0.0), 1.0 / 2.4) - 0.055;
    return linear_color <= 0.0031308 ? low : high;
}

float4 pack_flow_input_color(float4 color) {
    float4 packed = saturate(color);
    if (encode_srgb_flow_input()) {
        packed.rgb = linear_to_srgb(packed.rgb);
    }
    return packed;
}

// Rec. 709 weights give relative luminance from linear values; the encode
// then spreads it over the codes the way the swapchain spreads colour.
float pack_flow_input_luma(float4 color) {
    const float3 luma_weights = float3(0.2126, 0.7152, 0.0722);
    float luma = dot(saturate(color).rgb, luma_weights);
    if (encode_srgb_flow_input()) {
        luma = linear_to_srgb(luma.xxx).x;
    }
    return luma;
}

// An unset field decodes to 0.5, which is the fixed midpoint this shader
// used before the layer began deriving it.
float synthesis_fraction() {
    uint packed = (SynthesisFlags >> 8) & 0xffffu;
    return packed == 0u ? 0.5 : float(packed) / 65535.0;
}

float3 rotate_by_quaternion(float4 quaternion, float3 input_vector) {
    return input_vector + 2.0 * cross(
        quaternion.xyz,
        cross(quaternion.xyz, input_vector) + quaternion.w * input_vector);
}

struct MappedCoordinate {
    float2 coordinate;
    float valid;
    float padding;
};

MappedCoordinate map_target_to_source(
    float2 target_coordinate,
    CameraMapping mapping) {
    MappedCoordinate output;
    output.coordinate = float2(0.0, 0.0);
    output.valid = 0.0;
    output.padding = 0.0;
    float2 target_minimum = mapping.TargetRect.xy;
    float2 target_extent = mapping.TargetRect.zw;
    float2 target_maximum = target_minimum + target_extent - 1.0;
    bool target_inside =
        target_coordinate.x >= target_minimum.x &&
        target_coordinate.y >= target_minimum.y &&
        target_coordinate.x <= target_maximum.x &&
        target_coordinate.y <= target_maximum.y;

    float2 target_uv =
        (target_coordinate - target_minimum + 0.5) / target_extent;
    float2 target_tangent = float2(
        lerp(mapping.TargetTangents.x, mapping.TargetTangents.y, target_uv.x),
        lerp(mapping.TargetTangents.z, mapping.TargetTangents.w, target_uv.y));
    float3 target_ray = float3(target_tangent, -1.0);
    float3 source_ray = rotate_by_quaternion(
        mapping.TargetToSourceRotation,
        target_ray);
    bool source_in_front = source_ray.z < -1.0e-5;
    float inverse_depth = 1.0 / max(-source_ray.z, 1.0e-5);
    float2 source_tangent = source_ray.xy * inverse_depth;
    float source_width = mapping.SourceTangents.y - mapping.SourceTangents.x;
    float source_height = mapping.SourceTangents.w - mapping.SourceTangents.z;
    bool valid_fov = source_width > 1.0e-7 && abs(source_height) > 1.0e-7;
    source_width = max(source_width, 1.0e-7);
    source_height = source_height < 0.0
                        ? min(source_height, -1.0e-7)
                        : max(source_height, 1.0e-7);
    float2 source_uv = float2(
        (source_tangent.x - mapping.SourceTangents.x) / source_width,
        (source_tangent.y - mapping.SourceTangents.z) / source_height);
    float2 source_minimum = mapping.SourceRect.xy;
    float2 source_extent = mapping.SourceRect.zw;
    float2 source_maximum = source_minimum + source_extent;
    output.coordinate = source_minimum + source_uv * source_extent - 0.5;
    output.valid = target_inside && source_in_front && valid_fov &&
        output.coordinate.x >= source_minimum.x - 0.5 &&
        output.coordinate.y >= source_minimum.y - 0.5 &&
        output.coordinate.x <= source_maximum.x - 0.5 &&
        output.coordinate.y <= source_maximum.y - 0.5
            ? 1.0
            : 0.0;
    return output;
}

MappedCoordinate map_source_to_target(
    float2 source_coordinate,
    CameraMapping mapping) {
    MappedCoordinate output;
    output.coordinate = float2(0.0, 0.0);
    output.valid = 0.0;
    output.padding = 0.0;
    float2 source_minimum = mapping.SourceRect.xy;
    float2 source_extent = mapping.SourceRect.zw;
    float2 source_maximum = source_minimum + source_extent - 1.0;
    bool source_inside =
        source_coordinate.x >= source_minimum.x &&
        source_coordinate.y >= source_minimum.y &&
        source_coordinate.x <= source_maximum.x &&
        source_coordinate.y <= source_maximum.y;

    float2 source_uv =
        (source_coordinate - source_minimum + 0.5) / source_extent;
    float2 source_tangent = float2(
        lerp(mapping.SourceTangents.x, mapping.SourceTangents.y, source_uv.x),
        lerp(mapping.SourceTangents.z, mapping.SourceTangents.w, source_uv.y));
    float3 source_ray = float3(source_tangent, -1.0);
    float4 source_to_target_rotation = float4(
        -mapping.TargetToSourceRotation.xyz,
        mapping.TargetToSourceRotation.w);
    float3 target_ray = rotate_by_quaternion(
        source_to_target_rotation,
        source_ray);
    bool target_in_front = target_ray.z < -1.0e-5;
    float inverse_depth = 1.0 / max(-target_ray.z, 1.0e-5);
    float2 target_tangent = target_ray.xy * inverse_depth;
    float target_width = mapping.TargetTangents.y - mapping.TargetTangents.x;
    float target_height = mapping.TargetTangents.w - mapping.TargetTangents.z;
    bool valid_fov = target_width > 1.0e-7 && abs(target_height) > 1.0e-7;
    target_width = max(target_width, 1.0e-7);
    target_height = target_height < 0.0
                        ? min(target_height, -1.0e-7)
                        : max(target_height, 1.0e-7);
    float2 target_uv = float2(
        (target_tangent.x - mapping.TargetTangents.x) / target_width,
        (target_tangent.y - mapping.TargetTangents.z) / target_height);
    float2 target_minimum = mapping.TargetRect.xy;
    float2 target_extent = mapping.TargetRect.zw;
    float2 target_maximum = target_minimum + target_extent;
    output.coordinate = target_minimum + target_uv * target_extent - 0.5;
    output.valid = source_inside && target_in_front && valid_fov &&
        output.coordinate.x >= target_minimum.x - 0.5 &&
        output.coordinate.y >= target_minimum.y - 0.5 &&
        output.coordinate.x <= target_maximum.x - 0.5 &&
        output.coordinate.y <= target_maximum.y - 0.5
            ? 1.0
            : 0.0;
    return output;
}

// Coordinates are pixel indices, texel centres on integers. Clamped
// addressing returns the edge texel past the border, as the coordinate clamp
// of the hand-written version did, and on sRGB views the hardware filters
// after decoding, as its loads did.
float4 bilinear_previous_source(float2 coordinate, uint slice) {
    float2 uv = (coordinate + 0.5) / float2(float(Width), float(Height));
    return PreviousFrame.SampleLevel(LinearClamp, float3(uv, float(slice)), 0.0);
}

float4 bilinear_current_source(float2 coordinate, uint slice) {
    float2 uv = (coordinate + 0.5) / float2(float(Width), float(Height));
    return CurrentFrame.SampleLevel(LinearClamp, float3(uv, float(slice)), 0.0);
}

// The ratio the flow input was packed at, relative to the source image.
// One when the input is full resolution. Derived from the width alone and
// applied to both axes: the scale is uniform, and the packed height carries
// the stacked eyes on the FidelityFX path, so a height ratio would not
// describe it.
float2 flow_input_scale() {
    float ratio = min(
        1.0,
        float(PackedWidth) / max(float(Width), 1.0));
    return float2(ratio, ratio);
}

float2 flow_input_coordinate(float2 full_resolution_coordinate) {
    return (full_resolution_coordinate + 0.5) *
        flow_input_scale() - 0.5;
}

struct CameraSample {
    float4 color;
    float valid;
    float3 padding;
};

CameraSample sample_previous_target(
    float2 target_coordinate,
    uint slice,
    uint view_index) {
    CameraSample output;
    output.color = float4(0.0, 0.0, 0.0, 1.0);
    output.valid = 0.0;
    output.padding = float3(0.0, 0.0, 0.0);
    MappedCoordinate mapped = map_target_to_source(
        target_coordinate,
        PreviousMappings[view_index]);
    if (mapped.valid > 0.5) {
        output.color = bilinear_previous_source(mapped.coordinate, slice);
        output.valid = 1.0;
    }
    return output;
}

CameraSample sample_current_target(
    float2 target_coordinate,
    uint slice,
    uint view_index) {
    CameraSample output;
    output.color = float4(0.0, 0.0, 0.0, 1.0);
    output.valid = 0.0;
    output.padding = float3(0.0, 0.0, 0.0);
    float2 target_minimum = PreviousMappings[view_index].TargetRect.xy;
    float2 target_maximum = target_minimum +
        PreviousMappings[view_index].TargetRect.zw;
    bool valid = target_coordinate.x >= target_minimum.x - 0.5 &&
                 target_coordinate.y >= target_minimum.y - 0.5 &&
                 target_coordinate.x <= target_maximum.x - 0.5 &&
                 target_coordinate.y <= target_maximum.y - 0.5;
    if (valid) {
        output.color = bilinear_current_source(target_coordinate, slice);
        output.valid = 1.0;
    }
    return output;
}

[numthreads(8, 8, 1)]
void PackFlowInput(uint3 thread_id : SV_DispatchThreadID) {
    if (thread_id.x >= PackedWidth || thread_id.y >= PackedHeight) {
        return;
    }

    float4 current_color = float4(0.0, 0.0, 0.0, 1.0);
    uint slice = thread_id.y / PackedEyeStride;
    uint local_y = thread_id.y - slice * PackedEyeStride;
    // A reduced input is resampled rather than point-sampled, so the flow
    // sees a filtered image instead of every fourth pixel.
    float2 input_scale = flow_input_scale();
    if (slice >= ArraySize) {
        PackedColor[thread_id.xy] = pack_flow_input_color(current_color);
        return;
    }
    if (PackedWidth < Width) {
        float2 source_coordinate =
            (float2(float(thread_id.x), float(local_y)) + 0.5) /
                max(input_scale, float2(1.0e-6, 1.0e-6)) - 0.5;
        if (source_coordinate.y < float(Height)) {
            current_color =
                bilinear_current_source(source_coordinate, slice);
        }
    } else if (thread_id.x < Width && local_y < Height) {
        current_color = CurrentFrame.Load(
            int4(int2(thread_id.x, local_y), int(slice), 0));
    }
    PackedColor[thread_id.xy] = pack_flow_input_color(current_color);
}

// The view whose target rectangle holds this pixel, for the pack, which runs
// per slice rather than per view. An array swapchain gives each view its own
// slice with the same rectangle, so among the views that contain the pixel
// the one on this slice wins; a double-wide has disjoint rectangles on one
// slice. 2 when no view claims the pixel.
uint pack_view_for_pixel(float2 target_coordinate, uint slice) {
    uint view = 2u;
    [unroll] for (uint v = 0; v < 2; ++v) {
        float4 rect = PreviousMappings[v].TargetRect;
        float2 minimum = rect.xy - 0.5;
        float2 maximum = rect.xy + rect.zw - 0.5;
        bool inside = rect.z > 0.0 && rect.w > 0.0 &&
            target_coordinate.x >= minimum.x && target_coordinate.y >= minimum.y &&
            target_coordinate.x <= maximum.x && target_coordinate.y <= maximum.y;
        if (inside && (view == 2u || v == slice)) {
            view = v;
        }
    }
    return view;
}

[numthreads(8, 8, 1)]
void PackNvidiaFlowInput(uint3 thread_id : SV_DispatchThreadID) {
    if (thread_id.x >= PackedWidth || thread_id.y >= PackedHeight) {
        return;
    }

    float4 previous_color = float4(0.0, 0.0, 0.0, 1.0);
    float4 current_color = float4(0.0, 0.0, 0.0, 1.0);
    uint slice = Slice;
    bool downscaled = PackedWidth < Width || PackedHeight < Height;
    // The previous frame reaches the engine warped into the current camera
    // through the OpenXR pose delta, so what it estimates is the residual
    // scene motion rather than the head rotation with the scene motion on
    // top. Head turns no longer exceed its search, its cost no longer reads
    // "large displacement" everywhere under rotation, and the previous
    // pair's vectors (temporal hints) stay small and correlated. The
    // composition reads the previous endpoint through the same mapping, so
    // the two agree by construction. Where this camera sees beyond the
    // previous frame the input is black; the composition already treats
    // those pixels as uncovered.
    if (slice < ArraySize) {
        float2 target_coordinate = downscaled
            ? (float2(thread_id.xy) + 0.5) / flow_input_scale() - 0.5
            : float2(thread_id.xy);
        if (target_coordinate.x < float(Width) &&
            target_coordinate.y < float(Height)) {
            uint view_index = pack_view_for_pixel(target_coordinate, slice);
            if (view_index < 2u) {
                CameraSample previous = sample_previous_target(
                    target_coordinate, slice, view_index);
                if (previous.valid >= 0.5) {
                    previous_color = previous.color;
                }
            } else {
                previous_color =
                    bilinear_previous_source(target_coordinate, slice);
            }
            current_color = downscaled
                ? bilinear_current_source(target_coordinate, slice)
                : CurrentFrame.Load(int4(int2(thread_id.xy), int(slice), 0));
        }
    }
#ifdef XRFG_NVIDIA_LUMA_INPUT
    // One channel is a quarter of the bytes of four, and the engine matches
    // structure, not colour.
    NvidiaPreviousColor[thread_id.xy] = pack_flow_input_luma(previous_color);
    NvidiaCurrentColor[thread_id.xy] = pack_flow_input_luma(current_color);
#else
    NvidiaPreviousColor[thread_id.xy] = pack_flow_input_color(previous_color);
    NvidiaCurrentColor[thread_id.xy] = pack_flow_input_color(current_color);
#endif
}

float2 load_backward_flow_clamped(
    int2 coordinate,
    int2 minimum,
    int2 maximum) {
    return float2(BackwardFlow.Load(int3(clamp(coordinate, minimum, maximum), 0)));
}

float2 load_forward_flow_clamped(
    int2 coordinate,
    int2 minimum,
    int2 maximum) {
    return float2(ForwardFlow.Load(int3(clamp(coordinate, minimum, maximum), 0)));
}

struct FlowGridBounds {
    int2 minimum;
    int2 maximum;
};

FlowGridBounds flow_grid_bounds(
    float4 image_rect,
    uint slice) {
    uint2 image_minimum = uint2(image_rect.xy);
    uint2 image_maximum = image_minimum + uint2(image_rect.zw) - 1U;
    // Always applied: the scale is one when the input is full resolution,
    // so this is the identity on an unscaled path.
    {
        float2 input_scale = flow_input_scale();
        uint2 scaled_minimum = uint2(floor(
            float2(image_minimum) * input_scale));
        uint2 scaled_end = uint2(ceil(
            float2(image_maximum + 1U) * input_scale));
        image_minimum = scaled_minimum;
        image_maximum = max(scaled_minimum, scaled_end - 1U);
    }
    FlowGridBounds bounds;
    bounds.minimum = int2(
        int(image_minimum.x / FlowBlockSize),
        int((slice * PackedEyeStride + image_minimum.y) / FlowBlockSize));
    bounds.maximum = int2(
        int(image_maximum.x / FlowBlockSize),
        int((slice * PackedEyeStride + image_maximum.y) / FlowBlockSize));
    bounds.maximum = min(
        bounds.maximum,
        int2(int(FlowWidth) - 1, int(FlowHeight) - 1));
    return bounds;
}

float2 flow_for_pixel(
    float2 pixel,
    uint slice,
    uint view_index,
    float value_scale,
    bool use_nvidia_input_scale) {
    // The stride is zero where the backend packs eyes separately, so this
    // is the scaled coordinate on that path and the scaled coordinate plus
    // the eye offset on the stacked one.
    float2 scaled = flow_input_coordinate(pixel);
    float2 packed_coordinate = float2(
        scaled.x,
        scaled.y + float(slice * PackedEyeStride));
    float2 flow_coordinate =
        (packed_coordinate + 0.5) / float(FlowBlockSize) - 0.5;
    FlowGridBounds bounds = flow_grid_bounds(
        PreviousMappings[view_index].TargetRect,
        slice);
    float2 bounded = clamp(
        flow_coordinate,
        float2(bounds.minimum),
        float2(bounds.maximum));
    int2 top_left = int2(floor(bounded));
    int2 bottom_right = min(top_left + int2(1, 1), bounds.maximum);
    float2 fraction = bounded - float2(top_left);
    float2 top = lerp(
        load_backward_flow_clamped(
            top_left,
            bounds.minimum,
            bounds.maximum),
        load_backward_flow_clamped(
            int2(bottom_right.x, top_left.y),
            bounds.minimum,
            bounds.maximum),
        fraction.x);
    float2 bottom = lerp(
        load_backward_flow_clamped(
            int2(top_left.x, bottom_right.y),
            bounds.minimum,
            bounds.maximum),
        load_backward_flow_clamped(
            bottom_right,
            bounds.minimum,
            bounds.maximum),
        fraction.x);
    float2 result = lerp(top, bottom, fraction.y) * value_scale;
    // Flow measured on a reduced input has proportionally smaller vectors.
    result /= max(flow_input_scale(), float2(1.0e-6, 1.0e-6));
    float magnitude = length(result);
    return magnitude > 512.0 ? result * (512.0 / magnitude) : result;
}

float2 forward_flow_for_pixel(
    float2 pixel,
    uint slice,
    uint view_index,
    float value_scale,
    // True where the previous input was warped into the current camera in
    // the pack: its pixels then lie in the target rectangle, not the source.
    bool warped_previous) {
    float2 input_scale = flow_input_scale();
    float2 packed_coordinate = flow_input_coordinate(pixel);
    float2 flow_coordinate =
        (packed_coordinate + 0.5) / float(FlowBlockSize) - 0.5;
    FlowGridBounds bounds = flow_grid_bounds(
        warped_previous
            ? PreviousMappings[view_index].TargetRect
            : PreviousMappings[view_index].SourceRect,
        slice);
    float2 bounded = clamp(
        flow_coordinate,
        float2(bounds.minimum),
        float2(bounds.maximum));
    int2 top_left = int2(floor(bounded));
    int2 bottom_right = min(top_left + int2(1, 1), bounds.maximum);
    float2 fraction = bounded - float2(top_left);
    float2 top = lerp(
        load_forward_flow_clamped(
            top_left,
            bounds.minimum,
            bounds.maximum),
        load_forward_flow_clamped(
            int2(bottom_right.x, top_left.y),
            bounds.minimum,
            bounds.maximum),
        fraction.x);
    float2 bottom = lerp(
        load_forward_flow_clamped(
            int2(top_left.x, bottom_right.y),
            bounds.minimum,
            bounds.maximum),
        load_forward_flow_clamped(
            bottom_right,
            bounds.minimum,
            bounds.maximum),
        fraction.x);
    float2 result = lerp(top, bottom, fraction.y) * value_scale /
        max(input_scale, float2(1.0e-6, 1.0e-6));
    float magnitude = length(result);
    return magnitude > 512.0 ? result * (512.0 / magnitude) : result;
}

// The DLSS output rectangle associated with this eye stream. In the
// vectors-only composition the otherwise-unused optical-flow constants carry
// it; the hybrid needs those for the flow, so there it travels packed.
float4 game_motion_output_rect() {
#ifdef XRFG_HYBRID
    // Bits 15 and 31 of the size carry the depth flags (V505).
    return float4(
        float(UseGameMotion & 0xffffu), float(UseGameMotion >> 16u),
        float(GameMotionPadding & 0x7fffu), float((GameMotionPadding >> 16u) & 0x7fffu));
#else
    return float4(
        float(FlowWidth), float(FlowHeight),
        float(FlowBlockSize), float(GameMotionPadding));
#endif
}

float2 game_motion_for_pixel(float2 pixel, uint slice, uint view_index) {
    float4 output_rect = game_motion_output_rect();
    float2 uv = (pixel - output_rect.xy + 0.5) /
        max(output_rect.zw, float2(1.0, 1.0));
    float2 coordinate = GameMotionRect.xy + uv * GameMotionRect.zw - 0.5;
    float2 minimum = GameMotionRect.xy;
    float2 maximum = GameMotionRect.xy + GameMotionRect.zw - 1.0;
    float2 bounded = clamp(coordinate, minimum, maximum);
    int2 top_left = int2(floor(bounded));
    int2 bottom_right = min(top_left + int2(1, 1), int2(maximum));
    float2 fraction = bounded - float2(top_left);
    // Each descriptor block owns one eye's independent mono DLSS resource.
    // Rotation produces a spatially varying field, so nearest-neighbour lookup
    // creates an avoidable endpoint error that uniform translation conceals.
    float2 top = lerp(
        GameMotionVectors.Load(int4(top_left, 0, 0)),
        GameMotionVectors.Load(int4(bottom_right.x, top_left.y, 0, 0)),
        fraction.x);
    float2 bottom = lerp(
        GameMotionVectors.Load(int4(top_left.x, bottom_right.y, 0, 0)),
        GameMotionVectors.Load(int4(bottom_right, 0, 0)),
        fraction.x);
    float2 raw = lerp(top, bottom, fraction.y);
    // DLSS vectors are at render resolution (DI2: 2782 for a 4172 output).
    // Across a motion edge the blend of a building's vector and the
    // background's is a motion neither surface has, and the solve misplaces
    // the strip the edge covers or reveals with it. Where the four texels
    // differ by more than a pixel, the nearest one is taken (V503): render-
    // resolution vectors then measure as full-resolution ones do.
    float2 t00 = GameMotionVectors.Load(int4(top_left, 0, 0));
    float2 t10 = GameMotionVectors.Load(int4(bottom_right.x, top_left.y, 0, 0));
    float2 t01 = GameMotionVectors.Load(int4(top_left.x, bottom_right.y, 0, 0));
    float2 t11 = GameMotionVectors.Load(int4(bottom_right, 0, 0));
    float2 lowest = min(min(t00, t10), min(t01, t11));
    float2 highest = max(max(t00, t10), max(t01, t11));
    if (length((highest - lowest) * GameMotionScale) > vector_snap_pixels()) {
        raw = fraction.y < 0.5 ? (fraction.x < 0.5 ? t00 : t10)
                               : (fraction.x < 0.5 ? t01 : t11);
    }
    float2 result = raw * GameMotionScale + GameMotionJitterDelta;
    float magnitude = length(result);
    return magnitude > 512.0 ? result * (512.0 / magnitude) : result;
}

float nvidia_cost_for_pixel(float2 pixel, uint slice, uint view_index) {
    float2 packed_coordinate = flow_input_coordinate(pixel);
    int2 coordinate = int2(round(
        (packed_coordinate + 0.5) / float(FlowBlockSize) - 0.5));
    FlowGridBounds bounds = flow_grid_bounds(
        PreviousMappings[view_index].TargetRect,
        slice);
    return float(FlowAuxiliary.Load(int3(clamp(
        coordinate,
        bounds.minimum,
        bounds.maximum), 0)) & 0xffU);
}

float nvidia_forward_cost_for_pixel(
    float2 pixel,
    uint slice,
    uint view_index,
    bool warped_previous) {
    float2 packed_coordinate = flow_input_coordinate(pixel);
    int2 coordinate = int2(round(
        (packed_coordinate + 0.5) / float(FlowBlockSize) - 0.5));
    FlowGridBounds bounds = flow_grid_bounds(
        warped_previous
            ? PreviousMappings[view_index].TargetRect
            : PreviousMappings[view_index].SourceRect,
        slice);
    return float(ForwardAuxiliary.Load(int3(clamp(
        coordinate,
        bounds.minimum,
        bounds.maximum), 0)) & 0xffU);
}

bool coordinate_inside_rect(float2 coordinate, float4 rect) {
    float2 minimum = rect.xy - 0.5;
    float2 maximum = rect.xy + rect.zw - 0.5;
    return coordinate.x >= minimum.x && coordinate.y >= minimum.y &&
        coordinate.x <= maximum.x && coordinate.y <= maximum.y;
}

float rgb_error(float4 a, float4 b) {
    float3 difference = abs(a.rgb - b.rgb);
    return max(difference.r, max(difference.g, difference.b));
}

// Fast can report a plausible low-cost match on a different repeated edge.
// Validate against the ORIGINAL endpoints, not just two already-warped samples:
// those can both hit the same unrelated dark/bright patch and agree perfectly.
float fast_endpoint_confidence(
    float2 pixel, float2 backward, uint slice, bool warped_previous) {
    static const float2 offsets[5] = {
        float2(0, 0), float2(-2, 0), float2(2, 0),
        float2(0, -2), float2(0, 2)
    };
    float error = 0.0;
    bool valid = true;
    [unroll] for (uint i = 0; i < 5; ++i) {
        float2 current = pixel + offsets[i];
        float2 previous = current + backward;
        if (!coordinate_inside_rect(current, PreviousMappings[ViewIndex].TargetRect)) {
            valid = false;
        }
        float4 previous_color;
        if (warped_previous) {
            // The previous endpoint is a target coordinate: read it through
            // the mapping, as the pack did when it built the engine's input.
            CameraSample sample = sample_previous_target(previous, slice, ViewIndex);
            if (sample.valid < 0.5) {
                valid = false;
            }
            previous_color = sample.color;
        } else {
            if (!coordinate_inside_rect(previous, PreviousMappings[ViewIndex].SourceRect)) {
                valid = false;
            }
            previous_color = bilinear_previous_source(previous, slice);
        }
        error = max(error, rgb_error(
            bilinear_current_source(current, slice),
            previous_color));
    }
    return valid ? 1.0 - smoothstep(8.0 / 255.0, 48.0 / 255.0, error) : 0.0;
}

// Preserve a stationary neighbourhood after the existing XR camera mapping.
// Testing only the centre is insufficient (a moving edge can cross flat pixels).
bool fast_stationary_patch(float2 pixel, uint slice, float4 current) {
    CameraSample previous = sample_previous_target(pixel, slice, ViewIndex);
    static const float2 offsets[4] = {
        float2(-2, 0), float2(2, 0), float2(0, -2), float2(0, 2)
    };
    bool stationary = previous.valid >= 0.5 &&
        rgb_error(previous.color, current) <= 1.0 / 255.0;
    [unroll] for (uint i = 0; i < 4; ++i) {
        CameraSample a = sample_previous_target(pixel + offsets[i], slice, ViewIndex);
        CameraSample b = sample_current_target(pixel + offsets[i], slice, ViewIndex);
        if (a.valid < 0.5 || b.valid < 0.5 || rgb_error(a.color, b.color) > 1.0 / 255.0) {
            stationary = false;
        }
    }
    return stationary;
}

// The flow cell a full-resolution pixel of this view reads, and the pixel at a
// cell's centre: inverses of each other through the input scale, the stacked
// eye offset and the block size.
int2 flow_cell_for_pixel(float2 pixel, uint slice) {
    float2 scaled = flow_input_coordinate(pixel);
    float2 packed = float2(scaled.x, scaled.y + float(slice * PackedEyeStride));
    return int2(floor((packed + 0.5) / float(FlowBlockSize)));
}

float2 pixel_for_flow_cell(int2 cell, uint slice) {
    float2 packed = (float2(cell) + 0.5) * float(FlowBlockSize) - 0.5;
    float2 scaled = float2(packed.x, packed.y - float(slice * PackedEyeStride));
    return (scaled + 0.5) / max(flow_input_scale(), float2(1.0e-6, 1.0e-6)) - 0.5;
}

// The scene's own motion at a current-frame pixel: the flow there, less the
// head's part where the flow input still carries it (FidelityFX).
bool residual_from_raw(
    float2 current_pixel,
    float2 raw,
    bool input_pose_compensated,
    out float2 residual) {
    residual = raw;
    if (input_pose_compensated) {
        return true;
    }
    MappedCoordinate mapped = map_target_to_source(
        current_pixel,
        PreviousMappings[ViewIndex]);
    residual = raw - (mapped.coordinate - current_pixel);
    return mapped.valid >= 0.5;
}

bool residual_flow_at(
    float2 current_pixel,
    float value_scale,
    bool input_pose_compensated,
    out float2 residual) {
    float2 raw = flow_for_pixel(
        current_pixel, Slice, ViewIndex, value_scale, false);
    return residual_from_raw(
        current_pixel, raw, input_pose_compensated, residual);
}

bool residual_flow_of_cell(
    int2 cell,
    FlowGridBounds bounds,
    float value_scale,
    bool input_pose_compensated,
    out float2 residual) {
    bool inside = all(cell >= bounds.minimum) && all(cell <= bounds.maximum);
    float2 raw = float2(BackwardFlow.Load(int3(
        clamp(cell, bounds.minimum, bounds.maximum), 0))) * value_scale /
        max(flow_input_scale(), float2(1.0e-6, 1.0e-6));
    float magnitude = length(raw);
    raw = magnitude > 512.0 ? raw * (512.0 / magnitude) : raw;
    bool mapped = residual_from_raw(
        pixel_for_flow_cell(cell, Slice), raw, input_pose_compensated, residual);
    return inside && mapped;
}

// Whether a vector belongs at an in-between pixel: the current-frame pixel
// it would come from, p - (1 - t) c, carries it itself. The cell under that
// origin, not a bilinear blend: one load, and a blend across the boundary is
// exactly what the test must not average away.
bool vector_lands_on_itself(
    float2 pixel,
    float2 candidate,
    FlowGridBounds bounds,
    float value_scale,
    bool input_pose_compensated) {
    float2 at_origin = float2(0.0, 0.0);
    bool known = residual_flow_of_cell(
        flow_cell_for_pixel(pixel - candidate * (1.0 - synthesis_fraction()), Slice),
        bounds,
        value_scale,
        input_pose_compensated,
        at_origin);
    return known &&
        length(at_origin - candidate) <=
            fixed_point_tolerance() + fixed_point_speed_tolerance() * length(candidate);
}

// Cells whose vectors are tried for an in-between pixel beside a motion
// boundary: the eight neighbours, then rings further out in the eight
// directions. By default three rings are searched, so a moving object up to
// about four cells away (32 screen pixels at the default half-scale NVIDIA
// input) can lend its vector; the live window can stop after the first ring
// or reach on to six and eight cells.
static const int2 kMidpointCandidateCells[40] = {
    int2(-1, -1), int2(0, -1), int2(1, -1),
    int2(-1, 0), int2(1, 0),
    int2(-1, 1), int2(0, 1), int2(1, 1),
    int2(-2, -2), int2(0, -2), int2(2, -2),
    int2(-2, 0), int2(2, 0),
    int2(-2, 2), int2(0, 2), int2(2, 2),
    int2(-4, -4), int2(0, -4), int2(4, -4),
    int2(-4, 0), int2(4, 0),
    int2(-4, 4), int2(0, 4), int2(4, 4),
    int2(-6, -6), int2(0, -6), int2(6, -6),
    int2(-6, 0), int2(6, 0),
    int2(-6, 6), int2(0, 6), int2(6, 6),
    int2(-8, -8), int2(0, -8), int2(8, -8),
    int2(-8, 0), int2(8, 0),
    int2(-8, 8), int2(0, 8), int2(8, 8)
};

// How far apart the vectors of the four cells the bilinear flow at this pixel
// blends are: near zero inside one moving surface, the difference of the two
// motions beside a boundary between them.
float flow_cell_spread(int2 centre_cell, float2 pixel, FlowGridBounds bounds, float value_scale) {
    float2 scaled = flow_input_coordinate(pixel);
    float2 packed = float2(scaled.x, scaled.y + float(Slice * PackedEyeStride));
    float2 flow_coordinate = (packed + 0.5) / float(FlowBlockSize) - 0.5;
    int2 top_left = int2(floor(clamp(
        flow_coordinate, float2(bounds.minimum), float2(bounds.maximum))));
    int2 bottom_right = min(top_left + int2(1, 1), bounds.maximum);
    float2 a = load_backward_flow_clamped(top_left, bounds.minimum, bounds.maximum);
    float2 b = load_backward_flow_clamped(
        int2(bottom_right.x, top_left.y), bounds.minimum, bounds.maximum);
    float2 c = load_backward_flow_clamped(
        int2(top_left.x, bottom_right.y), bounds.minimum, bounds.maximum);
    float2 d = load_backward_flow_clamped(bottom_right, bounds.minimum, bounds.maximum);
    float2 low = min(min(a, b), min(c, d));
    float2 high = max(max(a, b), max(c, d));
    return length(high - low) * value_scale /
        max(flow_input_scale().x, 1.0e-6);
}

struct MidpointMotion {
    float2 residual;
    // The current-frame pixel the vector belongs to: the in-between pixel
    // itself for the vector read there, p - (1 - t) c for a candidate c.
    float2 origin;
    CameraSample previous_sample;
    CameraSample current_sample;
};

void sample_midpoint_endpoints(
    float2 pixel,
    float2 residual,
    inout MidpointMotion motion) {
    float t = synthesis_fraction();
    motion.residual = residual;
    motion.previous_sample = sample_previous_target(
        pixel + residual * t, Slice, ViewIndex);
    motion.current_sample = sample_current_target(
        pixel - residual * (1.0 - t), Slice, ViewIndex);
}

// A flat panel's endpoints disagree in opacity as well as colour (V510).
float panel_endpoint_error(MidpointMotion motion) {
    return motion.previous_sample.valid >= 0.5 &&
            motion.current_sample.valid >= 0.5
        ? max(rgb_error(motion.previous_sample.color, motion.current_sample.color),
              abs(motion.previous_sample.color.a - motion.current_sample.color.a))
        : 1.0e6;
}

float midpoint_endpoint_error(MidpointMotion motion) {
    return motion.previous_sample.valid >= 0.5 &&
            motion.current_sample.valid >= 0.5
        ? rgb_error(motion.previous_sample.color, motion.current_sample.color)
        : 1.0e6;
}

// Edge ghosting reduction: the vector that carries the in-between pixel.
// The flow belongs to the
// current frame's pixels, so the vector read at the in-between pixel itself
// is the motion of whatever the current frame shows there: beside a moving
// edge that is the background where the object has already moved on - the
// object's trailing half then lands as a half-transparent ghost. A vector c
// is right for the in-between pixel when the current-frame pixel it comes
// from, p - (1 - t) c, carries c itself. Started from the vector at p,
// iterating that relation (as the DLSS-vector path does) stays on the
// background at a trailing edge, so the object's own vector is looked for in
// the cells around p instead: each candidate that passes the test is kept if
// its two endpoints agree clearly better than the vector read at p. Pixels
// where the cells around p agree and the endpoints already match skip the
// search, so only motion boundaries and mismatches pay for it.
MidpointMotion choose_midpoint_motion(
    float2 pixel,
    float2 anchored_residual,
    float value_scale,
    bool input_pose_compensated) {
    float t = synthesis_fraction();
    MidpointMotion best = (MidpointMotion)0;
    best.origin = pixel;
    sample_midpoint_endpoints(pixel, anchored_residual, best);
    float best_error = midpoint_endpoint_error(best);

    // The search below is compiled out on this branch (V497): edge ghosting
    // reduction is off, and a dead loop fails the warnings-as-errors build.
#ifdef XRFG_EDGE_GHOSTING_REDUCTION
    int2 centre = flow_cell_for_pixel(pixel, Slice);
    FlowGridBounds bounds = flow_grid_bounds(
        PreviousMappings[ViewIndex].TargetRect,
        Slice);
    float spread = flow_cell_spread(centre, pixel, bounds, value_scale);
    bool search = repeated_capture_flag() == 0 &&
        edge_ghosting_reduction_enabled() &&
        (spread > midpoint_spread_trigger() || best_error > midpoint_error_trigger());

    // Candidates are first held to the fixed-point test, which reads only the
    // small flow texture; the endpoints of at most two survivors are then
    // compared on the full-resolution frames. A wave of pixels runs as long
    // as its slowest pixel, so the expensive part is kept short.
    float2 survivors[3] = {float2(0.0, 0.0), float2(0.0, 0.0), float2(0.0, 0.0)};
    uint survivor_limit = midpoint_survivor_limit();
    uint candidate_limit = 1u + 8u * midpoint_search_rings();
    uint survivor_count = 0;
    float2 last_tried = anchored_residual;
    for (uint i = 0; search && i < candidate_limit && survivor_count < survivor_limit; ++i) {
        float2 candidate = float2(0.0, 0.0);
        bool available = false;
        if (i == 0) {
            // One step of the fixed-point iteration from the vector at p.
            available = residual_flow_at(
                pixel - anchored_residual * (1.0 - t),
                value_scale,
                input_pose_compensated,
                candidate);
        } else {
            available = residual_flow_of_cell(
                centre + kMidpointCandidateCells[i - 1],
                bounds,
                value_scale,
                input_pose_compensated,
                candidate);
        }
        bool fresh = available &&
            length(candidate - anchored_residual) >= 0.5 &&
            length(candidate - last_tried) >= 0.5 &&
            (survivor_count == 0 || length(candidate - survivors[0]) >= 0.5) &&
            (survivor_count < 2 || length(candidate - survivors[1]) >= 0.5);
        [branch] if (fresh) {
            last_tried = candidate;
            if (vector_lands_on_itself(
                    pixel, candidate, bounds, value_scale, input_pose_compensated)) {
                survivors[survivor_count] = candidate;
                ++survivor_count;
            }
        }
    }
    for (uint k = 0; k < survivor_count; ++k) {
        MidpointMotion trial = best;
        trial.origin = pixel - survivors[k] * (1.0 - t);
        sample_midpoint_endpoints(pixel, survivors[k], trial);
        float error = midpoint_endpoint_error(trial);
        if (error + midpoint_survivor_margin() < best_error) {
            best = trial;
            best_error = error;
        }
    }
#endif
    // Flat panels (V510). A HUD quad's transparent parts carry nothing for
    // the flow to follow, so beside a moving icon it reads still: the strip
    // the icon moves into and the strip it leaves both blended the two
    // frames there, half the icon at half its opacity. The icon's own motion
    // is in the flow over the icon, so where the two endpoints disagree -
    // colour or opacity - the vectors read around the pixel are tried and the
    // one whose endpoints agree best is kept: on the leading strip both land
    // on the icon, on the trailing strip both land on the clear background.
    // Pixels whose endpoints already agree, the still and the clear parts of
    // the panel, skip it.
    [branch] if (flat_panel() && repeated_capture_flag() == 0) {
        float panel_error = panel_endpoint_error(best);
        [branch] if (panel_error > 2.0 / 255.0) {
            MidpointMotion still = best;
            still.origin = pixel;
            sample_midpoint_endpoints(pixel, float2(0.0, 0.0), still);
            float still_error = panel_endpoint_error(still);
            if (still_error + 1.0 / 255.0 < panel_error) {
                best = still;
                panel_error = still_error;
            }
            static const float2 kPanelDirections[8] = {
                float2(1.0, 0.0), float2(-1.0, 0.0), float2(0.0, 1.0), float2(0.0, -1.0),
                float2(0.7071, 0.7071), float2(-0.7071, 0.7071),
                float2(0.7071, -0.7071), float2(-0.7071, -0.7071)};
            static const float kPanelReach[4] = {6.0, 12.0, 24.0, 40.0};
            float2 tried[3] = {best.residual, float2(0.0, 0.0), float2(1.0e6, 1.0e6)};
            for (uint ring = 0; ring < 4 && panel_error > 2.0 / 255.0; ++ring) {
                for (uint direction = 0; direction < 8; ++direction) {
                    float2 candidate = float2(0.0, 0.0);
                    bool available = residual_flow_at(
                        pixel + kPanelDirections[direction] * kPanelReach[ring],
                        value_scale,
                        input_pose_compensated,
                        candidate);
                    bool fresh = available && length(candidate) >= 0.5 &&
                        length(candidate - tried[0]) >= 0.5 &&
                        length(candidate - tried[1]) >= 0.5 &&
                        length(candidate - tried[2]) >= 0.5;
                    [branch] if (fresh) {
                        tried[2] = tried[1];
                        tried[1] = candidate;
                        MidpointMotion trial = best;
                        trial.origin = pixel - candidate * (1.0 - t);
                        sample_midpoint_endpoints(pixel, candidate, trial);
                        float error = panel_endpoint_error(trial);
                        if (error + 1.0 / 255.0 < panel_error) {
                            best = trial;
                            panel_error = error;
                        }
                    }
                }
            }
        }
    }
    return best;
}

// The game's vector at the guide texel under pixel, unfiltered: enough to tell
// surfaces apart.
float2 game_motion_texel(float2 pixel) {
    float4 output_rect = game_motion_output_rect();
    float2 uv = (pixel - output_rect.xy + 0.5) /
        max(output_rect.zw, float2(1.0, 1.0));
    float2 maximum = GameMotionRect.xy + GameMotionRect.zw - 1.0;
    int2 texel = int2(clamp(GameMotionRect.xy + uv * GameMotionRect.zw,
        GameMotionRect.xy, maximum));
    return GameMotionVectors.Load(int4(texel, 0, 0)) * GameMotionScale;
}

// Content that stayed where it was on screen although the game's vector there
// says it moved: a HUD drawn after DLSS has no vectors of its own. It needs
// both: unchanged at the pixel and its neighbours, and not what the vector
// predicts. Correct vectors explain a moving surface however flat or striped
// it is, so only content they do not describe is kept. (0.3's V463 check; a
// striped overlay warped by a stripe period otherwise agrees with itself.)
bool game_motion_static_overlay(float2 pixel, float4 current) {
    int2 texel = int2(pixel + 0.5);
    bool overlay = rgb_error(PreviousFrame.Load(int4(texel, int(Slice), 0)), current) <= 1.0 / 255.0;
    MappedCoordinate moved = (MappedCoordinate)0;
    if (overlay) {
        // Content its vector holds still is explained without a sample, and in
        // a still scene that is nearly every pixel.
        moved = map_source_to_target(pixel + game_motion_texel(pixel), PreviousMappings[ViewIndex]);
        overlay = moved.valid >= 0.5 && length(moved.coordinate - pixel) >= 0.5;
    }
    // Unchanged across the patch next: what moves fails that cheaply.
    int2 offsets[4] = {int2(2, 0), int2(-2, 0), int2(0, 2), int2(0, -2)};
    float4 around[4] = {current, current, current, current};
    bool flat = true;
    if (overlay) {
        int2 limit = int2(int(Width) - 1, int(Height) - 1);
        [unroll] for (uint i = 0; i < 4; ++i) {
            int2 neighbour = clamp(texel + offsets[i], int2(0, 0), limit);
            around[i] = CurrentFrame.Load(int4(neighbour, int(Slice), 0));
            overlay = overlay && rgb_error(PreviousFrame.Load(int4(neighbour, int(Slice), 0)),
                around[i]) <= 1.0 / 255.0;
            flat = flat && rgb_error(around[i], current) <= 1.0 / 255.0;
        }
    }
    if (overlay) {
        // Somewhere in it not what the vector predicts. A repeating pattern
        // can match its own shift at one pixel; a flat patch cannot tell, and
        // its centre has already been asked.
        float unexplained = rgb_error(
            sample_previous_target(moved.coordinate, Slice, ViewIndex).color, current);
        if (unexplained <= 0.1 && !flat) {
            [unroll] for (uint i = 0; i < 4; ++i) {
                unexplained = max(unexplained, rgb_error(sample_previous_target(
                    moved.coordinate + float2(offsets[i]), Slice, ViewIndex).color, around[i]));
            }
        }
        overlay = unexplained > 0.1;
    }
    return overlay;
}

// The game-motion solve below, from start rather than the pixel, for a second
// surface: true, with its A and B samples, if two steps reach its fixed point.
bool game_motion_surface_at(float2 pixel, float2 start, out float4 previous_color,
    out float4 current_color, out float2 endpoint) {
    previous_color = float4(0.0, 0.0, 0.0, 1.0);
    current_color = previous_color;
    endpoint = start;
    float2 previous_coordinate = start;
    bool found = true;
    [unroll] for (uint step = 0; step < 3; ++step) {
        float2 backward = game_motion_for_pixel(endpoint, Slice, ViewIndex);
        MappedCoordinate mapped = map_source_to_target(endpoint + backward, PreviousMappings[ViewIndex]);
        found = found && mapped.valid >= 0.5;
        previous_coordinate = mapped.coordinate;
        float2 next = pixel - (1.0 - synthesis_fraction()) * (mapped.coordinate - endpoint);
        if (step == 2) {
            found = found && length(next - endpoint) < 0.5;
        } else {
            endpoint = next;
        }
    }
    if (found) {
        CameraSample a = sample_previous_target(previous_coordinate, Slice, ViewIndex);
        CameraSample b = sample_current_target(endpoint, Slice, ViewIndex);
        previous_color = a.color;
        current_color = b.color;
        found = a.valid >= 0.5 && b.valid >= 0.5;
    }
    return found;
}

bool game_motion_surface(float2 pixel, float2 start, out float4 previous_color,
    out float4 current_color) {
    float2 endpoint;
    return game_motion_surface_at(pixel, start, previous_color, current_color, endpoint);
}

#ifdef XRFG_HYBRID
bool game_depth_available() { return (GameMotionPadding & 0x8000u) != 0u; }

// How near the surface B shows at `pixel` is, larger nearer, from the game's
// depth at the vectors' texel: inverted depth as it comes, conventional
// depth turned round. Only its order is used.
// The depth texel game_nearness reads for `pixel`.
int2 game_depth_texel(float2 pixel) {
    float4 output_rect = game_motion_output_rect();
    float2 uv = (pixel - output_rect.xy + 0.5) / max(output_rect.zw, float2(1.0, 1.0));
    float2 maximum = GameMotionRect.xy + GameMotionRect.zw - 1.0;
    return int2(clamp(GameMotionRect.xy + uv * GameMotionRect.zw,
        GameMotionRect.xy, maximum));
}

float game_nearness(float2 pixel) {
    float depth = GameDepth.Load(int4(game_depth_texel(pixel), 0, 0));
    return (GameMotionPadding & 0x80000000u) != 0u ? depth : 1.0 - depth;
}

bool nearer(float a, float b) { return a > b * depth_nearer_factor() + 1.0e-6; }

// V539: the mask's texel for a coordinate of A's image: the vectors' texel
// grid over the output rectangle, counted from the rectangle's origin.
int2 coverage_mask_texel(float2 coordinate) {
    float4 output_rect = game_motion_output_rect();
    float2 uv = (coordinate - output_rect.xy + 0.5) / max(output_rect.zw, float2(1.0, 1.0));
    float2 limit = max(ceil(GameMotionRect.zw), float2(1.0, 1.0));
    return int2(clamp(floor(uv * GameMotionRect.zw), float2(0.0, 0.0), limit));
}

// A mask word: bits 24-31 the priority - how near by the game's depth, or
// without depth how fast - and bits 0-23 the vector in quarter pixels, 12
// bits an axis from -512 to +512 (8 bits an axis, +-32, lost a fast strafe:
// background and object both clamped to 32 read as one surface). The
// largest word wins a texel; 0 is an empty one.
// V544: the priority of a surface by the game's depth, on a log scale - 8
// steps a doubling of distance over 32 doublings from the near plane. Taken
// as it comes (V539), inverted depth is the near plane over the distance:
// everything past 255 near planes read 0, a building and the terrain behind
// it tied in MSFS 2024, and the vector bits below decided - the surface
// sliding left won, so background uncovered by one sliding right kept the
// ghost.
float coverage_mask_depth_priority(float nearness) {
    return saturate(1.0 + log2(max(nearness, 2.3283064e-10)) / 32.0);
}

uint coverage_mask_word(float2 motion, float priority) {
    uint2 quarter = uint2(clamp(round(motion * 4.0) + 2048.0, 0.0, 4095.0));
    return (uint(saturate(priority) * 255.0) << 24) | (quarter.x << 12) | quarter.y;
}

float2 coverage_mask_motion(uint word) {
    return (float2(float((word >> 12) & 4095u), float(word & 4095u)) - 2048.0) / 4.0;
}

// Whether A's sample at `coordinate` lies under a surface of B other than
// the one moving by `motion`, this pixel's: that surface covered the spot in
// A, so the pixel is background it has uncovered. False without a mask, and
// where nothing of B came from there.
// The verdict: 1 no mask, 2 nothing of B came from there, 3 the surface
// moving by `motion`, 4 another.
uint coverage_mask_verdict(float2 coordinate, float2 motion) {
    uint verdict = 1u;
    [branch] if (coverage_mask_available()) {
        uint word = CoverageMask.Load(int4(coverage_mask_texel(coordinate), 0, 0));
        verdict = 2u;
        if (word != 0u) {
            float2 recorded = coverage_mask_motion(word);
            float2 own = clamp(motion, -512.0, 512.0);
            verdict = length(recorded - own) > max(1.0, 0.15 * length(recorded)) ? 4u : 3u;
        }
    }
    return verdict;
}

bool coverage_mask_other_surface(float2 coordinate, float2 motion) {
    last_mask_state = coverage_mask_verdict(coordinate, motion);
    return last_mask_state == 4u;
}

// V539: whether the surface a solve found at `endpoint` of B is hidden in A
// where its own vector places it - the coverage mask gives that spot to
// another surface. Such a solve is not that surface in both frames: on a
// trailing edge it is the pixel's own background again, A's sample the
// object over it, and over a dark or flat background the two agreed within
// the search's margin and the blend of them painted the object back over
// what the mask had shown revealed (Returnal: white bands, the ghost).
bool coverage_mask_hides(float2 endpoint) {
    bool hidden = false;
    [branch] if (coverage_mask_available()) {
        float2 backward = game_motion_for_pixel(endpoint, Slice, ViewIndex);
        MappedCoordinate mapped = map_source_to_target(endpoint + backward, PreviousMappings[ViewIndex]);
        hidden = coverage_mask_verdict(mapped.coordinate, backward) == 4u;
    }
    return hidden;
}
#endif

struct FullscreenVertex {
    float4 position : SV_Position;
};

FullscreenVertex FullscreenTriangleVS(uint vertex_id : SV_VertexID) {
    FullscreenVertex output;
    float2 position = vertex_id == 0 ? float2(-1.0, -1.0)
                     : vertex_id == 1 ? float2(-1.0, 3.0)
                                      : float2(3.0, -1.0);
    output.position = float4(position, 0.0, 1.0);
    return output;
}

#ifdef XRFG_HYBRID
// V500. The vectors branch answers an occlusion with one frame's sample, so
// its two warped samples disagree there by construction: one frame shows the
// surface, the other what hides it. The hybrid used to read that disagreement
// as the vectors failing and hand the pixel to the optical flow, whose
// smeared samples agree by accident - the ghost at a revealing edge. These
// check the occlusion instead, so the hybrid judges the answer given.
//
// A's sample at `a` claimed to show the surface that B shows at `surface`:
// traced by that surface's own vector it must sit at `a` in A, so A at `a`
// matches B where the surface is now. The colour error of that match; 1 when
// the trace leaves an image or lands on another motion.
float occluder_error(float2 a, float4 a_color, float2 surface) {
    float error = 1.0;
    float2 motion = game_motion_texel(surface);
    MappedCoordinate then = map_source_to_target(
        surface + game_motion_for_pixel(surface, Slice, ViewIndex), PreviousMappings[ViewIndex]);
    float2 now = a - (then.coordinate - surface);
    if (then.valid >= 0.5 &&
        length(game_motion_texel(now) - motion) <= max(1.0, 0.15 * length(motion))) {
        CameraSample b = sample_current_target(now, Slice, ViewIndex);
        if (b.valid >= 0.5) error = rgb_error(a_color, b.color);
    }
    return error;
}
#endif

float4 synthesize_midpoint(
    FullscreenVertex input,
    float flow_value_scale,
    bool use_nvidia_cost,
    bool use_nvidia_bidirectional,
    bool validate_fast,
    bool use_game_motion_pipeline,
    // The NVIDIA pack warps the previous frame into the current camera, so
    // its flow is the residual already and every previous endpoint is a
    // target coordinate. FidelityFX keeps its own unwarped history, so its
    // flow still carries the pose term and it is subtracted here.
    bool input_pose_compensated) {
    float4 output_color = float4(0.0, 0.0, 0.0, 1.0);
#ifdef XRFG_HYBRID
    last_disagreement = 1.0;
    last_vectors_case = 0u;
    last_mask_state = 0u;
    last_mask_gate = 0u;
#endif
    uint2 integer_pixel = uint2(input.position.xy);
    bool in_bounds = integer_pixel.x < Width && integer_pixel.y < Height &&
        Slice < ArraySize;
    if (in_bounds) {
        float2 pixel = float2(integer_pixel);
        CameraSample current_fallback = sample_current_target(
            pixel,
            Slice,
            ViewIndex);
        output_color = saturate(current_fallback.color);
        MappedCoordinate previous_coverage = map_target_to_source(
            pixel,
            PreviousMappings[ViewIndex]);
        bool scene_changed = repeated_capture_flag() == 0 && !use_game_motion_pipeline &&
            !use_nvidia_cost &&
            (FlowAuxiliary.Load(int3(1, 0, 0)) & 0x0fU) != 0;
        if (previous_coverage.valid >= 0.5 && !scene_changed) {
            bool preserve_stationary = validate_fast && repeated_capture_flag() == 0 &&
                current_fallback.valid >= 0.5 &&
                fast_stationary_patch(pixel, Slice, current_fallback.color);
            // Content the game's vectors do not describe, kept where it is.
            bool static_overlay = !preserve_stationary && use_game_motion_pipeline &&
                repeated_capture_flag() == 0 && current_fallback.valid >= 0.5 &&
                game_motion_static_overlay(pixel, current_fallback.color);
            if (static_overlay) {
                output_color = saturate(current_fallback.color);
#ifdef XRFG_HYBRID
                last_disagreement = 0.0;
#endif
            } else if (preserve_stationary) {
                float4 stationary_previous =
                    sample_previous_target(pixel, Slice, ViewIndex).color;
                output_color = saturate(lerp(
                    stationary_previous,
                    current_fallback.color,
                    synthesis_fraction()));
#ifdef XRFG_HYBRID
                last_disagreement = rgb_error(stationary_previous, current_fallback.color);
#endif
            } else {
                float2 raw_backward = float2(0.0, 0.0);
                // The current-frame pixel the flow vector was read at: the
                // cost, the bidirectional check and the endpoint validation
                // all describe that vector.
                float2 flow_origin = pixel;
                CameraSample previous_sample = (CameraSample)0;
                CameraSample current_sample = (CameraSample)0;
                // Where the two samples were taken, as target coordinates:
                // the hybrid compares the two branches on a small blur
                // around them.
                float2 previous_coordinate = pixel;
                float2 current_coordinate = pixel;
                bool endpoints_valid = true;
                // How far the game-motion solve still was from a fixed point,
                // and its final B-to-A displacement.
                float solve_residual = 0.0;
                float2 last_displacement = float2(0.0, 0.0);
                // The previous frame at the pixel itself, through the pose
                // mapping alone: what a static surface shows there. The stable
                // fallback blends it.
                CameraSample previous_at_pixel = sample_previous_target(
                    pixel, Slice, ViewIndex);
                if (use_game_motion_pipeline) {
                    // A B-to-A vector is attached to its B endpoint. Sampling
                    // it once at the desired midpoint is exact only for a
                    // constant translation field. Yaw is spatially varying:
                    // solve cB = midpoint - 0.5 * (cA(cB) - cB), while mapping
                    // cA through the previous OpenXR camera exactly.
                    float2 current_endpoint = pixel;
                    MappedCoordinate previous_endpoint;
                    [unroll] for (uint iteration = 0; iteration < 3; ++iteration) {
                        raw_backward = repeated_capture_flag() != 0
                            ? float2(0.0, 0.0)
                            : game_motion_for_pixel(
                                current_endpoint, Slice, ViewIndex);
                        previous_endpoint = map_source_to_target(
                            current_endpoint + raw_backward,
                            PreviousMappings[ViewIndex]);
                        if (previous_endpoint.valid < 0.5) {
                            endpoints_valid = false;
                            break;
                        }
                        float2 target_displacement =
                            previous_endpoint.coordinate - current_endpoint;
                        current_endpoint =
                            pixel - (1.0 - synthesis_fraction()) * target_displacement;
                    }
                    if (endpoints_valid) {
                        raw_backward = repeated_capture_flag() != 0
                            ? float2(0.0, 0.0)
                            : game_motion_for_pixel(
                                current_endpoint, Slice, ViewIndex);
                        previous_endpoint = map_source_to_target(
                            current_endpoint + raw_backward,
                            PreviousMappings[ViewIndex]);
                        endpoints_valid = previous_endpoint.valid >= 0.5;
                        last_displacement = previous_endpoint.coordinate - current_endpoint;
                        solve_residual = length(pixel -
                            (1.0 - synthesis_fraction()) * last_displacement - current_endpoint);
                    }
                    if (endpoints_valid) {
                        previous_sample = sample_previous_target(
                            previous_endpoint.coordinate, Slice, ViewIndex);
                        current_sample = sample_current_target(
                            current_endpoint, Slice, ViewIndex);
                        previous_coordinate = previous_endpoint.coordinate;
                        current_coordinate = current_endpoint;
                    }
                } else {
                    raw_backward = repeated_capture_flag() != 0
                        ? float2(0.0, 0.0)
                        : flow_for_pixel(
                            pixel,
                            Slice,
                            ViewIndex,
                            flow_value_scale,
                            use_nvidia_cost);
                    float2 residual_backward = input_pose_compensated
                        ? raw_backward
                        : raw_backward - (previous_coverage.coordinate - pixel);
                    MidpointMotion motion = choose_midpoint_motion(
                        pixel,
                        residual_backward,
                        flow_value_scale,
                        input_pose_compensated);
                    previous_sample = motion.previous_sample;
                    current_sample = motion.current_sample;
                    flow_origin = motion.origin;
                    previous_coordinate = pixel + motion.residual * synthesis_fraction();
                    current_coordinate = pixel - motion.residual * (1.0 - synthesis_fraction());
                    if (input_pose_compensated) {
                        raw_backward = motion.residual;
                    }
                }
                if (endpoints_valid) {
                    if (previous_sample.valid >= 0.5 && current_sample.valid >= 0.5) {
                    float4 flow_midpoint = lerp(
                        previous_sample.color, current_sample.color, synthesis_fraction());
                    float disagreement = max(
                        abs(previous_sample.color.r - current_sample.color.r),
                        max(
                            abs(previous_sample.color.g - current_sample.color.g),
                            abs(previous_sample.color.b - current_sample.color.b)));
                    float confidence = saturate(1.0 - disagreement *
                        (use_game_motion_pipeline ? game_motion_confidence_slope() : 6.0));
                    float4 stable_midpoint = lerp(
                        previous_at_pixel.color,
                        current_fallback.color,
                        synthesis_fraction());
#ifdef XRFG_HYBRID
                    // The hybrid compares its two branches' samples blurred a
                    // little: sharp detail resampled at a fraction of a pixel
                    // disagrees even where the motion is exact.
                    float4 previous_low = 0.0, current_low = 0.0;
                    [unroll] for (uint tap = 0; tap < 4; ++tap) {
                        float2 offset = float2(
                            (tap & 1) != 0 ? 0.5 : -0.5, (tap & 2) != 0 ? 0.5 : -0.5);
                        previous_low += sample_previous_target(
                            previous_coordinate + offset, Slice, ViewIndex).color;
                        current_low += sample_current_target(
                            current_coordinate + offset, Slice, ViewIndex).color;
                    }
                    float compared = rgb_error(previous_low, current_low) * 0.25;
                    float stable_disagreement = rgb_error(
                        previous_at_pixel.color, current_fallback.color);
#endif
                    if (use_nvidia_cost) {
                        float consistency_confidence = 1.0;
                        float cost_confidence = 1.0;
                        if (repeated_capture_flag() == 0) {
                            if (validate_fast) {
                                confidence *= fast_endpoint_confidence(
                                    flow_origin, raw_backward, Slice, input_pose_compensated);
                            }
                            if (use_nvidia_bidirectional) {
                                float2 previous_coordinate = flow_origin + raw_backward;
                                if (coordinate_inside_rect(
                                        previous_coordinate,
                                        input_pose_compensated
                                            ? PreviousMappings[ViewIndex].TargetRect
                                            : PreviousMappings[ViewIndex].SourceRect)) {
                                    float2 raw_forward = forward_flow_for_pixel(
                                        previous_coordinate,
                                        Slice,
                                        ViewIndex,
                                        flow_value_scale,
                                        input_pose_compensated);
                                    float cycle_error = length(
                                        raw_backward + raw_forward);
                                    consistency_confidence =
                                        1.0 - smoothstep(2.0, 6.0, cycle_error);
                                    float maximum_cost = max(
                                        nvidia_cost_for_pixel(
                                            flow_origin,
                                            Slice,
                                            ViewIndex),
                                        nvidia_forward_cost_for_pixel(
                                            previous_coordinate,
                                            Slice,
                                            ViewIndex,
                                            input_pose_compensated));
                                    cost_confidence = saturate(
                                        1.0 - maximum_cost / 255.0);
                                } else {
                                    consistency_confidence = 0.0;
                                    cost_confidence = 0.0;
                                }
                            } else {
                                cost_confidence = saturate(
                                    1.0 - nvidia_cost_for_pixel(
                                        flow_origin,
                                        Slice,
                                        ViewIndex) / 255.0);
                            }
                        }
#ifdef XRFG_HYBRID
                        // The hybrid's flow (V502, 0.3's rule): kept wherever
                        // it explains both frames better than the camera-only
                        // blend, which at a covering edge is the earlier
                        // background half over the later building - the
                        // ghost. NVIDIA's cost and the fast check leaned on
                        // that blend there; only the bidirectional check stays.
                        confidence = saturate(1.0 +
                            (stable_disagreement - disagreement) * hybrid_flow_rule_slope()) *
                            consistency_confidence;
                        output_color = saturate(lerp(
                            stable_midpoint,
                            flow_midpoint,
                            confidence));
                        // It never said how well it did, so the hybrid always
                        // kept the vectors and NVIDIA's flow went unused.
                        last_disagreement = lerp(stable_disagreement, compared, confidence);
#else
                        confidence *= consistency_confidence * cost_confidence;
                        output_color = saturate(lerp(
                            stable_midpoint,
                            flow_midpoint,
                            confidence));
#endif
                    } else if (use_game_motion_pipeline) {
                        // The XR mapping can explain a head turn even when the
                        // game publishes zero scene motion. Do not force a
                        // mismatching guide warp over that exact camera result.
                        bool camera_explains = length(raw_backward) <= 0.1 &&
                            rgb_error(previous_at_pixel.color, current_fallback.color) +
                                1.0 / 255.0 < disagreement;
                        // The game's vectors are trusted: away from a motion
                        // edge, warped samples that disagree are one surface
                        // whose shading changed - a fade, a flash - and their
                        // motion-compensated blend is the frame between.
                        output_color = saturate(camera_explains ? stable_midpoint : flow_midpoint);
#ifdef XRFG_HYBRID
                        last_vectors_case = camera_explains ? 3u : 0u;
#endif
#ifdef XRFG_HYBRID
                        // The error of the answer given (V500): never above
                        // the warped samples', lower where an occlusion the
                        // branch handled explains their disagreement.
                        float explained = camera_explains
                            ? min(compared, stable_disagreement) : compared;
#endif
                        // Background a leading edge is covering shows only in
                        // A. There the solve has no fixed point: B shows the
                        // occluder at the pixel, so it starts inside it and
                        // alternates outside and in. After its odd number of
                        // steps it ends outside, on the covered surface's
                        // displacement, which places A's sample.
                        bool covered_case = !camera_explains && solve_residual >= 0.5;
#ifdef XRFG_HYBRID
                        // V539: A's sample under another surface of B. The
                        // coverage mask says so whatever that surface's speed;
                        // the probes below reach 16 pixels, scaled by this
                        // pixel's displacement - the background's, not the
                        // object's - so background a fast object uncovers
                        // escaped them and kept the blend, a ghost of the
                        // object that widened with its speed.
                        bool mask_revealed = !camera_explains && !covered_case &&
                            repeated_capture_flag() == 0 &&
                            coverage_mask_other_surface(previous_coordinate, raw_backward);
                        last_mask_gate = camera_explains ? 1u : covered_case ? 2u
                                       : repeated_capture_flag() != 0 ? 3u : 0u;
#else
                        const bool mask_revealed = false;
#endif
                        if (covered_case) {
                            CameraSample covered = sample_previous_target(
                                pixel + synthesis_fraction() * last_displacement, Slice, ViewIndex);
                            if (covered.valid >= 0.5) {
                                output_color = saturate(covered.color);
#ifdef XRFG_HYBRID
                                last_vectors_case = 1u;
                                // No surface of B reaches the pixel: the
                                // clearest occlusion the vectors find, and A
                                // alone shows what is there. The hybrid keeps
                                // it (V504). Checked by the occluder tracking
                                // its own vector (V500), it failed in DI2, the
                                // optical flow took the covering edge - the
                                // tint showed it red - and its smear was the
                                // ghost in front of the building.
                                explained = 0.0;
#endif
                            }
                        } else if (!camera_explains && (disagreement > 0.1 || mask_revealed) &&
                                   repeated_capture_flag() == 0) {
                            // Where the motion is not uniform - a vector 16
                            // pixels away differs by a tenth of a pixel -
                            // disagreeing samples are two surfaces instead. A's
                            // is usually hidden behind what B shows, background
                            // a trailing edge uncovers, so B's warped sample
                            // stands in.
                            float2 here = game_motion_texel(
                                pixel - (1.0 - synthesis_fraction()) * last_displacement);
                            // The band a moving edge uncovers or covers is as
                            // wide as the motion, so the probes reach past it:
                            // 16 pixels, or a quarter beyond the displacement
                            // when that is larger. The tolerance grows with the
                            // reach, which keeps it the same gradient.
                            float reach = max(16.0, edge_reach_factor() * max(length(last_displacement), length(here)));
                            float tolerance = 0.1 * reach / 16.0;
                            bool motion_edge = false;
#ifdef XRFG_HYBRID
                            // The other surfaces the probes meet: one of them
                            // is what hides A's sample, when B's is background
                            // a trailing edge uncovers.
                            float revealed = 1.0;
#endif
                            [unroll] for (uint probe = 0; probe < 4; ++probe) {
                                float2 offset = probe == 0 ? float2(reach, 0) : probe == 1 ? float2(-reach, 0)
                                              : probe == 2 ? float2(0, reach) : float2(0, -reach);
                                bool differs = length(game_motion_texel(pixel + offset) - here) > tolerance;
                                motion_edge = motion_edge || differs;
#ifdef XRFG_HYBRID
                                if (differs)
                                    revealed = min(revealed, occluder_error(
                                        previous_coordinate, previous_sample.color, pixel + offset));
#endif
                            }
                            motion_edge = motion_edge || mask_revealed;
                            if (motion_edge) {
                                output_color = saturate(lerp(
                                    current_sample.color, flow_midpoint, confidence));
#ifdef XRFG_HYBRID
                                // A's sample is that surface: B's alone is the
                                // background, and the vectors explain the pair.
                                last_vectors_case = 4u;
                                if (mask_revealed || revealed < revealed_error_limit()) {
                                    output_color = saturate(current_sample.color);
                                    explained = min(explained, mask_revealed ? 0.0 : revealed);
                                    last_vectors_case = 2u;
                                }
#endif
                            }
                            // B's sample may in turn be background that B has
                            // already uncovered but a faster surface in front
                            // still covers at the generated instant. That
                            // surface is another solution of the solve, and the
                            // one whose own A and B samples agree is visible in
                            // both frames, so in front. It is looked for from at
                            // most two starts on another surface, nearest first,
                            // out to 32 pixels or the probes' reach (128 at most).
                            uint solves = 0;
                            bool done = !motion_edge || edge_search_solves() == 0u;
                            float search_reach = max(32.0, reach);
                            [loop] for (uint radius_index = 0; radius_index < 6 && !done; ++radius_index) {
                                float radius = 4.0 * float(1u << radius_index);
                                if (radius > search_reach) break;
                                [loop] for (uint direction = 0; direction < 8 && !done; ++direction) {
                                    float angle = float(direction) * 0.78539816;
                                    float2 start = pixel + radius * float2(cos(angle), sin(angle));
                                    // A start on the same surface finds B's again.
                                    if (length(game_motion_texel(start) - here) > 1.0) {
                                        float4 a, b;
                                        float2 found_at;
                                        ++solves;
                                        bool surface = game_motion_surface_at(pixel, start, a, b, found_at) &&
                                            rgb_error(a, b) < edge_surface_agree();
#ifdef XRFG_HYBRID
                                        surface = surface && !coverage_mask_hides(found_at);
#endif
                                        if (surface) {
                                            output_color = saturate(lerp(a, b, synthesis_fraction()));
#ifdef XRFG_HYBRID
                                            explained = rgb_error(a, b);
                                            last_vectors_case = 5u;
#endif
                                            done = true;
                                        }
                                        done = done || solves >= edge_search_solves();
                                    }
                                }
                            }
                        }
#ifdef XRFG_HYBRID
                        // V505: a nearer surface can reach the pixel at the
                        // generated instant although the one found agrees in
                        // both frames - a thin bar sweeping across background
                        // that both frames show on a lateral move. The
                        // background's samples agree, nothing else asks, and
                        // the bar is missing from every generated frame: half
                        // transparent at 90 Hz. Points along the motion,
                        // nearest first, that the game's depth says are
                        // nearer than the answer start a solve; a nearer
                        // surface that reaches the pixel is shown instead.
                        if (game_depth_available() && depth_search_enabled() && !covered_case &&
                            repeated_capture_flag() == 0) {
                            float here = game_nearness(current_coordinate);
                            // A surface is nearer above this.
                            float start_threshold = here * depth_nearer_factor() + 1.0e-6;
                            [branch] if (depth_splat_available()) {
                                // V554 (the only search since V555; V505-
                                // V554 probed the depth at up to sixteen
                                // points along the motion, skipped by the
                                // V530 tiles): the splat has already found,
                                // per texel, the nearest surface that
                                // occupies it at the generated instant. One
                                // word read; where that surface is nearer
                                // than the answer by the margin, one solve
                                // from where the surface is in B - the
                                // pixel less the generated share of its
                                // displacement - verified as the probes'
                                // solves were. The priority byte is the
                                // mask's log-depth rank, so the compare is
                                // on the rank of the threshold; the solve
                                // then checks the exact depth.
                                uint word = DepthSplat.Load(int4(coverage_mask_texel(pixel),
                                    int(depth_splat_output()), 0));
                                uint rank = uint(saturate(
                                    coverage_mask_depth_priority(start_threshold)) * 255.0);
                                if (word != 0u && (word >> 24u) >= rank) {
                                    float2 displacement = coverage_mask_motion(word);
                                    float2 start = pixel - (1.0 - synthesis_fraction()) * displacement;
                                    float4 a, b;
                                    float2 endpoint;
                                    if (game_motion_surface_at(pixel, start, a, b, endpoint) &&
                                        nearer(game_nearness(endpoint), here) &&
                                        rgb_error(a, b) < depth_agree_limit()) {
                                        output_color = saturate(lerp(a, b, synthesis_fraction()));
                                        explained = rgb_error(a, b);
                                        last_vectors_case = 6u;
                                    }
                                }
                            }
                        }
                        last_disagreement = explained;
#endif
                    } else {
#ifdef XRFG_HYBRID
                        // The hybrid's FidelityFX flow, by the same rule (V502).
                        confidence = saturate(1.0 + (stable_disagreement - disagreement) * hybrid_flow_rule_slope());
#endif
                        output_color = saturate(lerp(
                            stable_midpoint,
                            flow_midpoint,
                            confidence));
#ifdef XRFG_HYBRID
                        last_disagreement = lerp(stable_disagreement, compared, confidence);
#endif
                    }
                    }
                }
            }
        }
    }
    return output_color;
}

#ifdef XRFG_HYBRID
// The game's vectors and the optical flow both: per pixel, whichever
// explains both frames better. Content the vectors do not describe - a
// shadow, a reflection, a HUD drawn after DLSS - moves as the image does,
// which the flow follows. Where the vectors already explain both frames the
// flow is not asked; the vectors keep a tie.
float4 synthesize_hybrid(
    FullscreenVertex input,
    float flow_value_scale,
    bool use_nvidia_cost,
    bool use_nvidia_bidirectional,
    bool validate_fast,
    bool input_pose_compensated) {
    float4 vectors = synthesize_midpoint(input, 1.0, false, false, false, true, false);
    float vectors_disagreement = last_disagreement;
    const uint vectors_case = last_vectors_case;
    const uint mask_state = last_mask_state;
    const uint mask_gate = last_mask_gate;
    float vectors_weight = 1.0;
    float4 result = vectors;
    [branch] if (vectors_disagreement >= hybrid_agree_threshold()) {
        float4 flow = synthesize_midpoint(
            input, flow_value_scale, use_nvidia_cost, use_nvidia_bidirectional,
            validate_fast, false, input_pose_compensated);
        float weight = saturate(
            1.0 + (last_disagreement - vectors_disagreement) * hybrid_choice_slope());
        result = lerp(flow, vectors, weight);
        vectors_weight = weight;
    }
    // The diagnostic tint (the tray's tuning window), generated frames only.
    // Mode 1: red the flow, yellow stripes a mix, then by the vectors' case
    // - blue covered, cyan revealed, orange the camera alone, magenta a
    // motion edge not verified, green a second surface, white the depth
    // search's.
    // Mode 2, the coverage mask where it says another surface covers the A
    // sample: green its answer (B alone) kept, white kept then replaced by a
    // surface search, orange stopped because the camera alone explains the
    // pixel, blue stopped as covered background, red a repeated capture;
    // magenta nothing of B came from the A sample, grey no mask bound,
    // untinted the pixel's own surface or the mask not consulted. Stripes
    // over any of these: the flow was mixed in.
    uint tint_mode = hybrid_tint_mode();
    [branch] if (tint_mode != 0u) {
        float3 tint = float3(0.0, 0.0, 0.0);
        float amount = 0.0;
        // Diagonal stripes 6 pixels wide: a mix never reads as a solid colour
        // over a dark or tinted surface.
        uint2 tint_pixel = uint2(input.position.xy);
        bool stripe = (((tint_pixel.x + tint_pixel.y) / 6u) & 1u) != 0u;
        if (tint_mode == 1u) {
            if (vectors_weight < 0.05) {
                tint = float3(1.0, 0.0, 0.0); amount = 0.45;
            } else if (vectors_weight < 0.95) {
                tint = float3(1.0, 1.0, 0.0); amount = stripe ? 0.6 : 0.0;
            } else if (vectors_case == 1u) {
                tint = float3(0.0, 0.25, 1.0); amount = 0.45;
            } else if (vectors_case == 2u) {
                tint = float3(0.0, 1.0, 1.0); amount = 0.45;
            } else if (vectors_case == 3u) {
                tint = float3(1.0, 0.5, 0.0); amount = 0.45;
            } else if (vectors_case == 4u) {
                tint = float3(1.0, 0.0, 1.0); amount = 0.45;
            } else if (vectors_case == 5u) {
                tint = float3(0.0, 1.0, 0.0); amount = 0.45;
            } else if (vectors_case == 6u) {
                tint = float3(1.0, 1.0, 1.0); amount = 0.45;
            }
        } else {
            if (mask_state == 4u) {
                amount = 0.5;
                if (mask_gate == 1u) {
                    tint = float3(1.0, 0.5, 0.0);
                } else if (mask_gate == 2u) {
                    tint = float3(0.0, 0.25, 1.0);
                } else if (mask_gate == 3u) {
                    tint = float3(1.0, 0.0, 0.0);
                } else if (vectors_case == 5u || vectors_case == 6u) {
                    tint = float3(1.0, 1.0, 1.0);
                } else {
                    tint = float3(0.0, 1.0, 0.0);
                }
            } else if (mask_state == 2u) {
                tint = float3(1.0, 0.0, 1.0); amount = 0.5;
            } else if (mask_state == 1u) {
                tint = float3(0.5, 0.5, 0.5); amount = 0.5;
            }
            if (vectors_weight < 0.95 && !stripe) {
                amount = 0.0;
            }
        }
        result.rgb = lerp(result.rgb, tint, amount);
    }
    return result;
}

float4 SynthesizeHybridMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_hybrid(input, 1.0, false, false, false, false);
}

float4 SynthesizeHybridNvidiaMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_hybrid(input, 1.0 / 32.0, true, false, false, true);
}

float4 SynthesizeHybridNvidiaBidirectionalMidpointPS(
    FullscreenVertex input) : SV_Target {
    return synthesize_hybrid(input, 1.0 / 32.0, true, true, false, true);
}

float4 SynthesizeHybridNvidiaFastMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_hybrid(input, 1.0 / 32.0, true, false, true, true);
}

float4 SynthesizeHybridNvidiaFastBidirectionalMidpointPS(
    FullscreenVertex input) : SV_Target {
    return synthesize_hybrid(input, 1.0 / 32.0, true, true, true, true);
}
#endif

float4 SynthesizeMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0, false, false, false, false, false);
}

float4 SynthesizeGameMotionMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0, false, false, false, true, false);
}

// The NVIDIA variants read flow in signed S10.5 fixed point, and from a
// previous input the pack warped into the current camera (PackNvidiaFlowInput).
float4 SynthesizeNvidiaMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0 / 32.0, true, false, false, false, true);
}

float4 SynthesizeNvidiaBidirectionalMidpointPS(
    FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0 / 32.0, true, true, false, false, true);
}

float4 SynthesizeNvidiaFastMidpointPS(FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0 / 32.0, true, false, true, false, true);
}

float4 SynthesizeNvidiaFastBidirectionalMidpointPS(
    FullscreenVertex input) : SV_Target {
    return synthesize_midpoint(input, 1.0 / 32.0, true, true, true, false, true);
}

#ifdef XRFG_COVERAGE_MASK
// V539: the coverage mask, at the vectors' resolution over the output
// rectangle, a slice per eye. ClearCoverageMask empties it; BuildCoverageMask
// writes, for each texel of B's vectors, the surface's word at the texel of
// A's image its vector leads to - the 2 x 2 texels around the point, so no
// gap opens between neighbours - the nearest surface by the game's depth,
// else the fastest, keeping a contested texel.
[numthreads(8, 8, 1)]
void ClearCoverageMask(uint3 id : SV_DispatchThreadID) {
    float2 limit = ceil(GameMotionRect.zw);
    if (float(id.x) <= limit.x && float(id.y) <= limit.y) {
        CoverageMaskOut[uint3(id.xy, Slice)] = 0u;
    }
}

[numthreads(8, 8, 1)]
void BuildCoverageMask(uint3 id : SV_DispatchThreadID) {
    float2 extent = GameMotionRect.zw;
    if (float(id.x) >= extent.x || float(id.y) >= extent.y) return;
    // The output pixel under this texel's centre: game_motion_texel's map,
    // inverted.
    float4 output_rect = game_motion_output_rect();
    float2 uv = (float2(id.xy) + 0.5) / max(extent, float2(1.0, 1.0));
    float2 pixel = output_rect.xy + uv * output_rect.zw - 0.5;
    float2 motion = game_motion_for_pixel(pixel, Slice, ViewIndex);
    MappedCoordinate previous = map_source_to_target(pixel + motion, PreviousMappings[ViewIndex]);
    if (previous.valid < 0.5) return;
    float priority = game_depth_available() ? coverage_mask_depth_priority(game_nearness(pixel))
                                            : min(length(motion), 255.0) / 255.0;
    uint word = coverage_mask_word(motion, priority);
    float2 per_pixel = extent / max(output_rect.zw, float2(1.0, 1.0));
    float2 at = (previous.coordinate - output_rect.xy + 0.5) * per_pixel;
    int2 limit = int2(ceil(extent));
    // V543: a moving surface along its own motion, either way, by the band's
    // share of it and at least a texel (0, V539's mask); the nearer (or
    // faster) surface still keeps a contested texel.
    float2 band = float2(0.0, 0.0);
    if (coverage_mask_band() > 0.0 && length(motion) > 0.5) {
        float2 along = normalize(motion) * max(coverage_mask_band() * length(motion), 1.0 / per_pixel.x);
        band = along * per_pixel;
    }
    uint steps = min(uint(ceil(length(band))), 8u);
    [loop] for (int step = -int(steps); step <= int(steps); ++step) {
        float2 spot = at + (steps != 0u ? band * (float(step) / float(steps)) : float2(0.0, 0.0));
        int2 base = int2(floor(spot - 0.5));
        [unroll] for (uint corner = 0; corner < 4; ++corner) {
            int2 texel = base + int2(corner & 1u, corner >> 1);
            if (all(texel >= 0) && all(texel <= limit)) {
                InterlockedMax(CoverageMaskOut[uint3(uint2(texel), Slice)], word);
            }
        }
    }
}
#endif

#ifdef XRFG_DEPTH_SPLAT
// V554: the depth search's midpoint splat, at the vectors' resolution over
// the output rectangle, a slice per eye and synthetic output (Slice the
// eye, the output in the flags). ClearDepthSplat empties a slice;
// BuildDepthSplat writes, for each texel of B's vectors, the surface's
// word at the texel of the generated image it occupies at the generated
// instant: B's position plus the generated share of its displacement to A,
// the fixed point the probes' solve converges to - the 2 x 2 texels around
// the point, so no gap opens between neighbours - the nearest by the
// game's depth keeping a contested texel. The composition then reads one
// word per pixel instead of probing the depth along its motion, and solves
// once where the word's surface is nearer than its own.
[numthreads(8, 8, 1)]
void ClearDepthSplat(uint3 id : SV_DispatchThreadID) {
    float2 limit = ceil(GameMotionRect.zw);
    if (float(id.x) <= limit.x && float(id.y) <= limit.y) {
        DepthSplatOut[uint3(id.xy, 2u * Slice + depth_splat_output())] = 0u;
    }
}

[numthreads(8, 8, 1)]
void BuildDepthSplat(uint3 id : SV_DispatchThreadID) {
    float2 extent = GameMotionRect.zw;
    if (float(id.x) >= extent.x || float(id.y) >= extent.y) return;
    // The output pixel under this texel's centre: game_motion_texel's map,
    // inverted.
    float4 output_rect = game_motion_output_rect();
    float2 uv = (float2(id.xy) + 0.5) / max(extent, float2(1.0, 1.0));
    float2 pixel = output_rect.xy + uv * output_rect.zw - 0.5;
    float2 motion = game_motion_for_pixel(pixel, Slice, ViewIndex);
    MappedCoordinate previous = map_source_to_target(pixel + motion, PreviousMappings[ViewIndex]);
    if (previous.valid < 0.5) return;
    float2 displacement = previous.coordinate - pixel;
    float priority = coverage_mask_depth_priority(game_nearness(pixel));
    uint word = coverage_mask_word(displacement, priority);
    float2 midpoint = pixel + (1.0 - synthesis_fraction()) * displacement;
    float2 per_pixel = extent / max(output_rect.zw, float2(1.0, 1.0));
    float2 at = (midpoint - output_rect.xy + 0.5) * per_pixel;
    int2 limit = int2(ceil(extent));
    int2 base = int2(floor(at - 0.5));
    uint slice = 2u * Slice + depth_splat_output();
    [unroll] for (uint corner = 0; corner < 4; ++corner) {
        int2 texel = base + int2(corner & 1u, corner >> 1);
        if (all(texel >= 0) && all(texel <= limit)) {
            InterlockedMax(DepthSplatOut[uint3(uint2(texel), slice)], word);
        }
    }
}
#endif

#ifdef XRFG_EYE_ORDER
// V546: which of two DLSS streams nothing else tells apart is this eye's
// (dlss_motion_vectors.hpp, report_dlss_eye_order). Every second texel of
// this stream's vectors (t6) where the other stream's (t10) part from them
// by more than a pixel, the current image's pixel is compared with the
// previous image read at the end of each vector: the eye's own vectors land
// on the surface the pixel shows, the other eye's a disparity to the side.
// EyeOrderVerdict words: 0 the pixels compared, 1 those this stream's vector
// matched better by more than the margin, 2 those the other's did.
float eye_order_tone(float4 color) {
    return sqrt(max(dot(color.rgb, float3(0.2126, 0.7152, 0.0722)), 0.0));
}

[numthreads(1, 1, 1)]
void ClearEyeOrder(uint3 id : SV_DispatchThreadID) {
    EyeOrderVerdict.Store4(0, uint4(0u, 0u, 0u, 0u));
}

// 0 not compared, 1 compared without a winner, 2 this stream's vector won,
// 3 the other's.
uint eye_order_vote(uint2 cell) {
    uint vote = 0u;
    float2 extent = GameMotionRect.zw;
    int2 texel = int2(cell) * 2;
    float4 output_rect = game_motion_output_rect();
    float2 pixel = output_rect.xy +
        (float2(texel) + 0.5) / max(extent, float2(1.0, 1.0)) * output_rect.zw - 0.5;
    float2 limit = float2(float(Width) - 1.0, float(Height) - 1.0);
    bool inside = float(texel.x) < extent.x && float(texel.y) < extent.y &&
        all(pixel >= 0.0) && all(pixel <= limit);
    [branch] if (inside) {
        int2 at = int2(GameMotionRect.xy) + texel;
        float2 own = GameMotionVectors.Load(int4(at, 0, 0)) * GameMotionScale +
            GameMotionJitterDelta;
        float2 other = AlternateMotionVectors.Load(int4(at, 0, 0)) * GameMotionScale +
            GameMotionJitterDelta;
        float2 own_end = pixel + own;
        float2 other_end = pixel + other;
        bool compared = length(own - other) > 1.0 &&
            all(own_end >= 0.0) && all(own_end <= limit) &&
            all(other_end >= 0.0) && all(other_end <= limit);
        [branch] if (compared) {
            float now = eye_order_tone(bilinear_current_source(pixel, Slice));
            float own_error = abs(now - eye_order_tone(bilinear_previous_source(own_end, Slice)));
            float other_error =
                abs(now - eye_order_tone(bilinear_previous_source(other_end, Slice)));
            const float margin = 1.0 / 64.0;
            vote = own_error + margin < other_error ? 2u
                : (other_error + margin < own_error ? 3u : 1u);
        }
    }
    return vote;
}

groupshared uint eye_order_counts[3];

[numthreads(8, 8, 1)]
void MeasureEyeOrder(uint3 id : SV_DispatchThreadID, uint index : SV_GroupIndex) {
    if (index < 3u) eye_order_counts[index] = 0u;
    GroupMemoryBarrierWithGroupSync();
    uint vote = eye_order_vote(id.xy);
    if (vote != 0u) {
        InterlockedAdd(eye_order_counts[0], 1u);
        if (vote == 2u) {
            InterlockedAdd(eye_order_counts[1], 1u);
        } else if (vote == 3u) {
            InterlockedAdd(eye_order_counts[2], 1u);
        }
    }
    GroupMemoryBarrierWithGroupSync();
    if (index == 0u && eye_order_counts[0] != 0u) {
        uint ignored;
        EyeOrderVerdict.InterlockedAdd(0, eye_order_counts[0], ignored);
        EyeOrderVerdict.InterlockedAdd(4, eye_order_counts[1], ignored);
        EyeOrderVerdict.InterlockedAdd(8, eye_order_counts[2], ignored);
    }
}
#endif
