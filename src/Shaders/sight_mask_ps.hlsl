// Upscaler history mask of a camera scope's image surface (see lens_mask_ps.hlsl).
//
// The camera scope's surface (including the integrated displays)
// shows the scope camera's own image, already accumulated over frames by the
// camera source resolve. The surface writes no velocity, so the upscaler moved
// its history with the world behind the scope: on a head turn it mixed in the
// image of a frame before and the picture jerked. Drawn after the surface draw
// while the optics path is 'camera'; every other sight material is discarded.
//
// Same input signature as sight_reflex_ps (SV_Position + TEXCOORD0..4).
cbuffer ShaderSpecificConsts : register(b4, space0) { float4 _29_m0[21]; };

struct PSIn {
    float4 pos      : SV_Position;
    float4 TEXCOORD : TEXCOORD0;
    float4 COLOR    : TEXCOORD1;
    float4 NRM_BX   : TEXCOORD2;
    float4 BYZ_TXY  : TEXCOORD3;
    float4 TZ_WPOS  : TEXCOORD4;
};

float main(PSIn IN) : SV_Target {
    if (_29_m0[6].x != -713.25f && _29_m0[6].x != -714.25f && _29_m0[6].x != -716.25f && _29_m0[6].x != -717.25f) discard;
    return 1.0f;
}
