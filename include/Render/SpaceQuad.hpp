#pragma once
#include <windows.h>
#include <d3d12.h>
#include <openxr/openxr.h>
#include <wrl.h>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <map>
#include "Runtimes/HudFollow.hpp"
#include "Utils/XrMath.hpp"

// PICTURES HUNG IN THE TRACKING SPACE, DRAWN INTO AN EYE'S IMAGE. A quad placed in the space the eyes are tracked in is
// drawn into one eye's image through the pose and frustum that image is submitted with: the compositor then shows it
// where it hangs, to either eye, and the eyes fuse it at its distance -- in the headset and in the window alike, since
// it is in the frame. One drawer for every such picture the port puts into a frame (Render/IntroOverlay.hpp,
// Render/LoadingScreen.hpp). Frame-capture thread only.
namespace cvr::render {
struct SpaceQuad {
    XrPosef pose{{0, 0, 0, 1}, {0, 0, 0}};   // its centre; it faces along the pose's +Z, as an OpenXR quad layer
    float width = 0.0f, height = 0.0f;       // m
    float uv[4] = {0.0f, 0.0f, 1.0f, 1.0f};  // the part of the texture on it: u0, v0, u1, v1
};

// A head pose that was located. The first frames of a session can be presented before any locate, with the zero pose
// (no rotation at all, or the identity at the origin): a picture anchored there hangs at the tracking space's origin,
// a head's height below the eyes once they are tracked (the intro 1.7 m low in the simulator, 2026-10-31). A real
// head exactly at the origin with no rotation at all is one frame late to be anchored, nothing more.
inline bool LocatedPose(const XrPosef& pose) {
    const auto& q = pose.orientation;
    const float length = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if (length < 0.5f || length > 1.5f) return false;
    const auto& p = pose.position;
    return !(p.x == 0.0f && p.y == 0.0f && p.z == 0.0f && q.x == 0.0f && q.y == 0.0f && q.z == 0.0f);
}

// A picture the same to both eyes: `distance` m from the eye that sees it (`eye`, the pose its image is drawn through)
// along `heading`, facing back along it. Each eye has it in that one direction, so the two see one picture, as far off
// as the sky -- whatever their own turn (a canted display's eyes look apart).
inline XrPosef Ahead(const XrPosef& eye, const XrQuaternionf& heading, float distance) {
    const XrVector3f ahead = RotateVector(heading, {0.0f, 0.0f, -distance});
    return {heading, {eye.position.x + ahead.x, eye.position.y + ahead.y, eye.position.z + ahead.z}};
}

// A HEADING THE HEAD LOOKS ABOUT FROM. Level, and still while the head turns up to `cone` (rad) either side of it; a
// head turned farther has it caught up, the menus' way (Runtimes/HudFollow.hpp Follow), until it is ahead again. Set by
// the first located head (LocatedPose). One thread.
class FreeLookHeading {
public:
    explicit FreeLookHeading(float cone) : cone_(cone) {}
    // A frame with `head`.
    void Update(const XrPosef& head) {
        const auto now = std::chrono::steady_clock::now();
        const float dt = stamped_ ? std::chrono::duration<float>(now - last_).count() : 0.0f;
        last_ = now;
        stamped_ = true;
        if (!LocatedPose(head)) return;
        const auto& q = head.orientation;
        follow_.Update(cvr::hud::HeadYaw(q.x, q.y, q.z, q.w, follow_.yaw), cone_, dt);
    }
    bool Valid() const { return follow_.valid; }
    XrQuaternionf Orientation() const {
        return {0.0f, std::sin(follow_.yaw * 0.5f), 0.0f, std::cos(follow_.yaw * 0.5f)};
    }

private:
    float cone_;
    cvr::hud::Follow follow_;
    std::chrono::steady_clock::time_point last_{};
    bool stamped_ = false;
};

// How a picture meets what is under it.
enum class Blend {
    Over,    // a solid picture over what is there, as opaque as its opacity (the texture's alpha is not used)
    Light,   // light added to what is there: black is clear, light under the black point dropped
};

class SpaceQuads {
public:
    // An eye's image: the resource, the format it is written through, the eye's rectangle in it (in mip level `mip`).
    struct Target {
        ID3D12Resource* resource = nullptr;
        DXGI_FORMAT view = DXGI_FORMAT_UNKNOWN;
        D3D12_RECT rect{};
        UINT mip = 0;
    };
    // Covers the rectangle with a colour, as much as its alpha.
    bool Fill(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, const float rgba[4]);
    // `quad` in one colour, as much as its alpha, as the eye at `eye` with `fov` sees it (its uv unused).
    bool Solid(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, const XrPosef& eye,
               const XrFovf& fov, const SpaceQuad& quad, const float rgba[4]);
    // `texture`, read through `view`, on `quad` as the eye at `eye` with `fov` sees it: through all its mips, by each
    // pixel's footprint.
    bool Draw(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, const XrPosef& eye,
              const XrFovf& fov, ID3D12Resource* texture, DXGI_FORMAT view, const SpaceQuad& quad, Blend blend,
              float opacity, float blackPoint = 0.0f);
    // The part `uv` of `texture`'s level `mip`, read through `view`, written over the whole target rectangle (one
    // bilinear tap a pixel: to a level half the size, the mean of each 2 x 2). The texture's level is a shader
    // resource now, the target's a render target.
    bool Resample(ID3D12Device* device, ID3D12GraphicsCommandList* list, const Target& target, ID3D12Resource* texture,
                  DXGI_FORMAT view, UINT mip, const float uv[4]);
    void Shutdown();

private:
    struct Pipelines {
        Microsoft::WRL::ComPtr<ID3D12PipelineState> fill, over, light, copy;
    };
    bool EnsureDevice(ID3D12Device* device);
    const Pipelines* PipelinesFor(DXGI_FORMAT format);
    // Binds the target's rectangle; the slot of the descriptors this draw uses.
    UINT Bind(ID3D12GraphicsCommandList* list, const Target& target);
    // `levels` of `texture` from `mip` on as the draw's picture, in the slot's descriptor.
    void BindPicture(ID3D12GraphicsCommandList* list, UINT slot, ID3D12Resource* texture, DXGI_FORMAT view, UINT mip,
                     UINT levels);

    Microsoft::WRL::ComPtr<ID3D12Device> device_;
    Microsoft::WRL::ComPtr<ID3D12RootSignature> root_;
    Microsoft::WRL::ComPtr<ID3DBlob> vs_, psFill_, psOver_, psLight_, psCopy_;
    std::map<DXGI_FORMAT, Pipelines> pipelines_;
    Microsoft::WRL::ComPtr<ID3D12DescriptorHeap> srvHeap_, rtvHeap_;
    UINT srvStride_ = 0, rtvStride_ = 0, cursor_ = 0;
    bool failed_ = false;
};
}  // namespace cvr::render
