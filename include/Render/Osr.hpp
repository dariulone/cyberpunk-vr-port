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

#if CVR_RUST_VRIK
#include <atomic>
extern "C" __declspec(dllexport) extern std::atomic<int64_t> CyberpunkVR_OsrStats[3];
extern "C" __declspec(dllexport) extern int32_t CyberpunkVR_OsrTune[4];
#endif

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

struct Stats { uint64_t recorded = 0, failed = 0; const char* last = ""; };

#if CVR_RUST_VRIK
// The reconstruction: rust/crates/cvr-gpu/src/osr.rs.
static_assert(sizeof(Camera) == 84 && sizeof(Request) == 232 && sizeof(Stats) == 24, "mirrored in rust/crates/cvr-gpu/src/osr.rs");
// The exported counters and tuning stay in src/Render/Osr.cpp and go with each request.
extern "C" bool cvr_gpu_osr_record(const Request* request, const void* stats, const int32_t* tune);
extern "C" void cvr_gpu_osr_reset();
extern "C" void cvr_gpu_osr_statistics(Stats* out);
// Records the made eye into request.out. False when nothing was recorded (sizes, formats, a failed init).
inline bool Record(const Request& request) { return cvr_gpu_osr_record(&request, CyberpunkVR_OsrStats, CyberpunkVR_OsrTune); }
// Forget both eyes' history (a new session, a reset).
inline void Reset() { cvr_gpu_osr_reset(); }
inline Stats Statistics() {
    Stats stats;
    cvr_gpu_osr_statistics(&stats);
    return stats;
}
#else
// Records the made eye into request.out. False when nothing was recorded (sizes, formats, a failed init).
bool Record(const Request& request);
// Forget both eyes' history (a new session, a reset).
void Reset();
Stats Statistics();
#endif

}  // namespace cvr::osr
