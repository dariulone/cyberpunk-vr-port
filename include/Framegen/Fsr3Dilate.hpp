#pragma once

struct ID3D12Device;
struct ID3D12GraphicsCommandList;
struct ID3D12Resource;

// FSR3-upscaler's "reconstruct dilated velocity + previous depth" pass, ported from AMD's public
// FidelityFX-SDK GPU source (ffx_fsr3upscaler_reconstruct_dilated_velocity_and_previous_depth.h --
// ReconstructAndDilate / FindNearestDepth / ReconstructPrevDepth) rather than pulling in the whole
// fsr3upscaler component: frame interpolation only consumes this pass's OUTPUT (dilatedDepth,
// dilatedMotionVectors, reconstructPrevNearDepth, see Fsr3.hpp), not the upscaler's own
// spatial reconstruction, temporal lock or accumulation.
//
// Ported at RENDER resolution only (the per-eye depth and motion copies, Framegen/Inputs.hpp, are
// render-res) -- the FFX_FSR3UPSCALER_OPTION_LOW_RESOLUTION_MOTION_VECTORS branch of the source, so
// no separate high-res motion-vector upsample position math.
struct Fsr3DilateOutputs {
    ID3D12Resource* dilatedDepth = nullptr;             // R32_FLOAT, render-res
    ID3D12Resource* dilatedMotionVectors = nullptr;     // R16G16_FLOAT, render-res
    ID3D12Resource* reconstructPrevNearDepth = nullptr; // R32_FLOAT, render-res
};

bool Fsr3Dilate_Init(ID3D12Device* device);

// Runs Clear + ReconstructAndDilate for one eye slot (0 or 1), (re)allocating that slot's output
// textures if renderWidth/renderHeight changed since the last call. Caller is responsible for
// barriering rawDepth/rawMotionVectors into a shader-readable state before calling, and for
// barriering the returned outputs out of UNORDERED_ACCESS before anything else reads them.
const Fsr3DilateOutputs* Fsr3Dilate_Run(ID3D12GraphicsCommandList* cmdList, int eyeSlot,
                                                 ID3D12Resource* rawDepth,
                                                 ID3D12Resource* rawMotionVectors, int renderWidth,
                                                 int renderHeight, float motionVectorScaleX,
                                                 float motionVectorScaleY, bool invertedDepth);

void Fsr3Dilate_Shutdown();
