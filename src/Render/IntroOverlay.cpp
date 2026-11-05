// The lobby's curtain and intro, drawn into the frame (Render/IntroOverlay.hpp).
#include "Render/IntroOverlay.hpp"
#include "Lobby/Intro.hpp"
#include "Render/HudPanel.hpp"
#include <windows.h>
#include <algorithm>
#include <cmath>

extern void Log(const char*, ...);

namespace cvr::lobby::intro {
namespace {
// The view an sRGB-encoded image is read or written through so that blending happens in linear light: the 8-bit ones
// as sRGB (a swapchain's buffer and a typeless resource may be viewed so), the rest as they are.
DXGI_FORMAT LinearView(DXGI_FORMAT format) {
    switch (format) {
    case DXGI_FORMAT_R8G8B8A8_UNORM:
    case DXGI_FORMAT_R8G8B8A8_TYPELESS: return DXGI_FORMAT_R8G8B8A8_UNORM_SRGB;
    case DXGI_FORMAT_B8G8R8A8_UNORM:
    case DXGI_FORMAT_B8G8R8A8_TYPELESS: return DXGI_FORMAT_B8G8R8A8_UNORM_SRGB;
    case DXGI_FORMAT_R10G10B10A2_TYPELESS: return DXGI_FORMAT_R10G10B10A2_UNORM;
    case DXGI_FORMAT_R16G16B16A16_TYPELESS: return DXGI_FORMAT_R16G16B16A16_FLOAT;
    default: return format;
    }
}
}  // namespace

bool Overlay::Begin(float curtain, const XrPosef& head) {
    curtain_ = std::clamp(curtain, 0.0f, 1.0f);
    shown_ = Latest();
    const bool picture = shown_ && shown_->texture && shown_->texture->resource && shown_->opacity > 0.0f;
    // The heading is set by a located head only: until one comes the picture waits (Record draws it then).
    auto& heading = Heading();
    heading.Update(head);
    if (picture && heading.Valid() && loggedIntro_ != shown_->intro) {
        loggedIntro_ = shown_->intro;
        const auto desc = shown_->texture->resource->GetDesc();
        Log("[lobby-intro] intro %llu: picture %llux%u format %u, drawn into the frame at the intro's heading, eyes at "
            "(%.2f, %.2f, %.2f)\n",
            static_cast<unsigned long long>(shown_->intro), static_cast<unsigned long long>(desc.Width), desc.Height,
            static_cast<unsigned>(desc.Format), head.position.x, head.position.y, head.position.z);
    }
    return curtain_ > 0.0f || picture;
}

bool Overlay::Record(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* target,
                     const D3D12_RECT& rect, const XrPosef& pose, const XrFovf& fov, bool picture, uint32_t slot) {
    if (!target) return false;
    const cvr::render::SpaceQuads::Target eye{target, LinearView(target->GetDesc().Format), rect};
    bool ok = true;
    if (curtain_ > 0.0f) {
        const float black[4] = {0.0f, 0.0f, 0.0f, curtain_};
        ok &= quads_.Fill(device, list, eye, black);
    }
    const auto& shown = shown_;
    const auto& heading = Heading();
    if (picture && shown && shown->texture && shown->texture->resource && shown->opacity > 0.0f && heading.Valid()) {
        auto* texture = shown->texture->resource.Get();
        const auto desc = texture->GetDesc();
        // At the intro's heading and the distance the script gives it, the same to both eyes: its size there is how
        // large it looks, and the distance growing flies it off.
        cvr::render::SpaceQuad quad;
        quad.pose = cvr::render::Ahead(pose, heading.Orientation(), shown->distance);
        quad.height = shown->height;
        quad.width = shown->height * float(desc.Width) / float(std::max<UINT>(1, desc.Height));
        ok &= quads_.Draw(device, list, eye, pose, fov, texture, LinearView(desc.Format), quad,
                          cvr::render::Blend::Light, shown->opacity, shown->blackPoint);
        held_[slot % held_.size()] = shown->texture;
    }
    return ok;
}

void Overlay::Shutdown() {
    quads_.Shutdown();
    held_ = {};
    shown_.reset();
    loggedIntro_ = 0;
}
}  // namespace cvr::lobby::intro
