#pragma once
// THE CAPTURE'S STATE IN THE RUST BUILD (rust/crates/cvr-render/src/xr_capture_state.rs): the snapshot pool, the newest
// captured frame, the leases on the pools' resources, the second eye's slots with their serials and labels, the retired
// captured reads -- what OpenXRManager kept under m_presentMutex (m_monoPool, m_monoCapturedFrame, m_captureLeases,
// m_vrcamEyePool, m_retiredCaptureReads).
#if CVR_RUST_VRIK
#include <d3d12.h>
#include <cstddef>
#include <cstdint>
#include <openxr/openxr.h>

namespace cvr::framegen { struct Inputs; }

extern "C" {
// What the submit takes of the newest captured frame: the image and the second eye's picture with a reference and a
// read lease of the caller's own each (ReleaseCapturedRead), the frame generator's inputs with one reference each
// (cvr_framegen_release_inputs).
struct CvrXrCaptureTaken {
    void* texture;
    uint64_t serial;
    double captureMs;
    bool dark;
    uint32_t mainEye;
    XrPosef poses[2];
    XrFovf fovs[2];
    bool hasView[2];
    uint64_t poseIds[2];
    const cvr::framegen::Inputs* inputs[2];
    void* second;
    bool secondReused;
};
bool cvr_xr_capture_ensure_pool(void* device, const D3D12_RESOURCE_DESC* desc);
bool cvr_xr_capture_ensure_eye_pool(void* device, uint32_t width, uint32_t height, int32_t format);
void cvr_xr_capture_end_writes(uintptr_t* writes);
bool cvr_xr_capture_take(bool stereo, uint32_t reuseMax, bool keepInputs, CvrXrCaptureTaken* out);
bool cvr_xr_capture_pin(void* resource);
void cvr_xr_capture_release_read(void* resource, uint64_t fence, uint64_t completed);
uint64_t cvr_xr_capture_serial();
bool cvr_xr_capture_one_turn();
uint64_t cvr_xr_capture_second_eye_serial();
void cvr_xr_capture_forget();
void cvr_xr_capture_shutdown();
bool cvr_xr_capture_ensure_ring(void* device, ID3D12CommandAllocator** allocators, ID3D12GraphicsCommandList** lists,
                                ID3D12Fence** fence, HANDLE* event);
const cvr::framegen::Inputs* cvr_framegen_retain_inputs(const cvr::framegen::Inputs* inputs);
void cvr_framegen_release_inputs(const cvr::framegen::Inputs* inputs);
}
static_assert(sizeof(CvrXrCaptureTaken) == 176 && offsetof(CvrXrCaptureTaken, poses) == 32 &&
                  offsetof(CvrXrCaptureTaken, poseIds) == 128 && offsetof(CvrXrCaptureTaken, second) == 160,
              "mirrored by Taken in rust/crates/cvr-render/src/xr_capture_state.rs");

extern "C" {
// THE SUBMIT'S RESOURCES (rust/crates/cvr-render/src/xr_submit.rs): the eye swapchains' count and the second eye's width
// (for the capture on the game's thread), the serial the runtime last accepted, the swapchains destroyed and the ring
// and RTV heap released with the session.
uint32_t cvr_xr_submit_eye_count();
int32_t cvr_xr_submit_eye1_width();
uint64_t cvr_xr_submit_last_serial();
void cvr_xr_submit_destroy_swapchains();
void cvr_xr_submit_release();
}
namespace cvr::capture {// The capture's write leases, ended when the capture stops without publishing.
class RustWriteRelease {
    uintptr_t (&m_resources)[2];
public:
    explicit RustWriteRelease(uintptr_t (&resources)[2]) : m_resources(resources) {}
    ~RustWriteRelease() { cvr_xr_capture_end_writes(m_resources); }
    RustWriteRelease(const RustWriteRelease&) = delete;
    RustWriteRelease& operator=(const RustWriteRelease&) = delete;
};
}
#endif
