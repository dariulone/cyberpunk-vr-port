#pragma once
// OSR -- the port's own reconstruction of the other eye for an AFW launch (Render/Afw.hpp), in place of PureDark's
// frame warp: written after the technique OFXR-Bridge describes for its "OFXR Stereo Reconstruction"
// (docs/OSR_FULL_WARP.md there; that code is GPL and none of it is used here). The drawn eye's depth is splatted
// forward into the other eye, and so is the other eye's own previous frame; each of the other eye's pixels is then
// traced back into whichever of the two holds the surface its splatted depth says is there (the depths must agree),
// and what neither holds -- background only this eye sees -- takes the background beside it, mirrored across the
// edge. No plugin, about a millisecond of compute (docs/osr-20261104.md).
#include <cstdint>
#include <d3d12.h>
#include <openxr/openxr.h>

namespace cvr::osr {

struct Camera {
    float position[3]{}, right[3]{}, up[3]{}, forward[3]{};
    XrFovf fov{};
    // The depth texture's convention: reversed (near 1, far 0) with a finite far plane unless infinite.
    float nearPlane = 0.02f, farPlane = 16000.0f;
    bool inverted = true, infinite = false;
    // The depth's sub-texel jitter, in depth texels (the colour is resolved, the depth is not).
    float jitter[2]{};
};

struct Request {
    ID3D12Device* device = nullptr;
    ID3D12GraphicsCommandList* list = nullptr;   // recording, on the queue the inputs were made on
    ID3D12Resource* drawn = nullptr;             // the drawn eye's colour (R8G8B8A8 family), in PRESENT state
    ID3D12Resource* depth = nullptr;             // its depth (R32_FLOAT), in NON_PIXEL_SHADER_RESOURCE state
    Camera source, dest;                         // the drawn eye's camera and the made eye's, this frame
    uint32_t eye = 0;                            // the drawn eye
    ID3D12Resource* out = nullptr;               // the made eye (R8G8B8A8 family, colour size), in COPY_SOURCE state
    bool history = true;                         // use the made eye's previous frame
    uint32_t debug = 0;                          // 1: tint history pixels green, filled ones magenta
    float jitterSign = 1.0f;                     // how the jitter is applied to a depth texel (+1, -1, 0)
};

// Records the made eye into request.out. False when nothing was recorded (sizes, formats, a failed init).
bool Record(const Request& request);
// Forget both eyes' history (a new session, a reset).
void Reset();

struct Stats { uint64_t recorded = 0, failed = 0; const char* last = ""; };
Stats Statistics();

}  // namespace cvr::osr
