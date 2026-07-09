// [SELFWARP] Rotational reprojection pass -- the RealVR-parity submit.
//
// RealVR never hands the compositor a stale frame at a stale pose: every display
// frame it re-warps the newest game frame to the CURRENT head pose on the PC
// (reverse: CP2077_FRAME_BRIDGE "reprojection shaders", copy_eye_render_targets_
// to_cuda pose-delta math). We do the same for the mono path: per output pixel,
// build the ray in the DECLARED (live lens) frustum, rotate it by
// conj(q_render) * q_live, and sample the source through the CONTENT's true
// symmetric projection. Submitted pose+fov are the LIVE ones, so the compositor's
// own timewarp has ~zero residual work -- which also collapses every
// pose-space/frustum inconsistency of the legacy path into a constant (and thus
// invisible) offset instead of a head-speed-proportional shimmer.
//
// Modeled on SharpenPass: records a fullscreen draw onto the CALLER's command
// list; the caller owns all resource transitions (src -> PIXEL_SHADER_RESOURCE,
// dst -> RENDER_TARGET). Two constant-buffer slots (one per eye) so both eye
// draws can be recorded into one command list without stomping constants.
#pragma once

#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <mutex>
#include <wrl.h>

class WarpPass {
public:
    WarpPass() = default;
    ~WarpPass();

    // (device, output color format, output width/height). Multi-call safe.
    bool EnsureInitialized(ID3D12Device* device,
                           DXGI_FORMAT colorFormat,
                           uint32_t width,
                           uint32_t height);

    // Record a fullscreen rotational-reprojection draw of `srcColor` into
    // `dstColor` for `eyeIndex` (selects the CB/SRV/RTV slot).
    //   renderOrientation : content camera orientation (the pose the frame was
    //                       rendered with), same space as liveOrientation.
    //   liveOrientation   : head orientation to warp TO (this tick's locate).
    //   srcFov            : the content's TRUE projection (symmetric tan-space).
    //   dstFov            : the DECLARED output frustum (live runtime lens fov).
    bool RecordWarp(ID3D12GraphicsCommandList* cmdList,
                    ID3D12Resource* srcColor,
                    ID3D12Resource* dstColor,
                    int eyeIndex,
                    const XrQuaternionf& renderOrientation,
                    const XrQuaternionf& liveOrientation,
                    const XrFovf& srcFov,
                    const XrFovf& dstFov);

    void Shutdown();

private:
    std::mutex m_mutex;
    Microsoft::WRL::ComPtr<ID3D12Device> m_device;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> m_rootSig;
    Microsoft::WRL::ComPtr<ID3D12PipelineState> m_pso;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_srvHeap;   // 2 slots (per eye)
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> m_rtvHeap;   // 2 slots (per eye)
    Microsoft::WRL::ComPtr<ID3D12Resource> m_cb;              // 2 x 256B slots
    UINT m_srvDescSize = 0;
    UINT m_rtvDescSize = 0;
    DXGI_FORMAT m_colorFormat = DXGI_FORMAT_UNKNOWN;
    uint32_t m_width = 0;
    uint32_t m_height = 0;
};
