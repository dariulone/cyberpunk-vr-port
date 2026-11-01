// The loading screen, hung in the dark (Render/LoadingScreen.hpp).
#include "Render/LoadingScreen.hpp"
#include <windows.h>
#include <algorithm>
#include <cmath>

extern void Log(const char*, ...);

namespace cvr::loading {
void Screen::Begin(const XrPosef& head, const Placement& at, float opacity) {
    const unsigned long long now = GetTickCount64();
    // Placed only by a located head: until there is one, the dark alone (Record).
    if (!up_ && cvr::render::LocatedPose(head)) {
        // Straight ahead of where the player looks as it comes up, level, at the eyes' height.
        const auto& q = head.orientation;
        const float fx = -2.0f * (q.x * q.z + q.y * q.w), fz = 2.0f * (q.x * q.x + q.y * q.y) - 1.0f;
        const float yaw = std::atan2(-fx, -fz);
        at_ = at;
        if (at.caption) caption_.Set(*at.caption);
        anchor_.orientation = {0.0f, std::sin(yaw * 0.5f), 0.0f, std::cos(yaw * 0.5f)};
        anchor_.position = {head.position.x - std::sin(yaw) * at.distance, head.position.y,
                            head.position.z - std::cos(yaw) * at.distance};
        up_ = true;
        held_ = false;
        Log("[loading-screen] up: %s %.1f m ahead at yaw %.0f deg, eyes at (%.2f, %.2f, %.2f)\n",
            at.heading ? "at the intro's heading" : "hung", at.distance, yaw * 57.29578f, head.position.x,
            head.position.y, head.position.z);
    }
    if (up_ && at_.heading) at_.heading->Update(head);
    const float age = held_ ? float(now - firstFrame_) / 1000.0f : 0.0f;
    const float k = std::clamp(age / (std::max)(at_.fadeIn, 0.01f), 0.0f, 1.0f);
    opacity_ = k * k * (3.0f - 2.0f * k);
    shown_ = std::clamp(opacity, 0.0f, 1.0f);
}

void Screen::Hold() {
    if (!held_) firstFrame_ = GetTickCount64();
    held_ = true;
    fresh_ = true;
}

bool Screen::EnsurePicture(ID3D12Device* device, UINT width, UINT height, DXGI_FORMAT format) {
    if (picture_ && width == pictureWidth_ && height == pictureHeight_ && format == pictureFormat_) return true;
    // A picture of another size: the old one may still be read by a capture list in flight, so it is kept till the end.
    if (picture_) retired_.push_back(std::move(picture_));
    UINT levels = 1;
    while ((std::max)(width, height) >> levels) ++levels;
    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width;
    desc.Height = height;
    desc.DepthOrArraySize = 1;
    desc.MipLevels = static_cast<UINT16>(levels);
    desc.Format = format;
    desc.SampleDesc.Count = 1;
    desc.Flags = D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
    const D3D12_HEAP_PROPERTIES heap{D3D12_HEAP_TYPE_DEFAULT};
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &desc,
                                               D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE, nullptr,
                                               IID_PPV_ARGS(&picture_)))) {
        Log("[loading-screen] picture %ux%u (%u levels) not made\n", width, height, levels);
        return false;
    }
    pictureWidth_ = width;
    pictureHeight_ = height;
    pictureFormat_ = format;
    levels_ = levels;
    Log("[loading-screen] picture %ux%u, %u levels\n", width, height, levels);
    return true;
}

// The screen's part of the window, at its own size, into the picture's top level, and each level below the mean of
// the one above: the screen is read through them by each pixel's footprint, so the window's text and edges, about
// three times smaller on the eye than in the window, do not alias.
bool Screen::BuildPicture(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* window,
                          DXGI_FORMAT windowView, const float uv[4]) {
    const auto source = window->GetDesc();
    const UINT width = (std::max)(1u, static_cast<UINT>(std::lround((uv[2] - uv[0]) * double(source.Width))));
    const UINT height = (std::max)(1u, static_cast<UINT>(std::lround((uv[3] - uv[1]) * double(source.Height))));
    if (!EnsurePicture(device, width, height, windowView)) return false;
    const float whole[4] = {0.0f, 0.0f, 1.0f, 1.0f};
    for (UINT level = 0; level < levels_; ++level) {
        D3D12_RESOURCE_BARRIER barrier{};
        barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
        barrier.Transition.pResource = picture_.Get();
        barrier.Transition.Subresource = level;
        barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
        barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_RENDER_TARGET;
        list->ResourceBarrier(1, &barrier);
        const LONG w = static_cast<LONG>((std::max)(1u, width >> level));
        const LONG h = static_cast<LONG>((std::max)(1u, height >> level));
        const cvr::render::SpaceQuads::Target target{picture_.Get(), windowView, {0, 0, w, h}, level};
        const bool ok = level == 0 ? quads_.Resample(device, list, target, window, windowView, 0, uv)
                                   : quads_.Resample(device, list, target, picture_.Get(), windowView, level - 1, whole);
        std::swap(barrier.Transition.StateBefore, barrier.Transition.StateAfter);
        list->ResourceBarrier(1, &barrier);
        if (!ok) return false;
    }
    return true;
}

bool Screen::Record(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* target, DXGI_FORMAT view,
                    const D3D12_RECT& rect, const XrPosef& pose, const XrFovf& fov, ID3D12Resource* window,
                    DXGI_FORMAT windowView, const float uv[4], float aspect) {
    const cvr::render::SpaceQuads::Target eye{target, view, rect};
    const float dark[4] = {0.0f, 0.0f, 0.0f, 1.0f};
    if (fresh_ && window) {
        // A new picture of the window this frame: the first eye drawn makes it, before its own target is bound.
        fresh_ = false;
        ready_ = BuildPicture(device, list, window, windowView, uv);
    }
    if (!quads_.Fill(device, list, eye, dark)) return false;
    if (!up_ || !held_ || !ready_) return true;
    if (at_.heading && !at_.heading->Valid()) return true;
    cvr::render::SpaceQuad screen;
    screen.pose = at_.heading ? cvr::render::Ahead(pose, at_.heading->Orientation(), at_.distance) : anchor_;
    aspect = std::max(aspect, 0.1f);
    screen.height = at_.width > 0.0f ? at_.width / aspect : at_.height;
    screen.width = screen.height * aspect;
    if (!quads_.Draw(device, list, eye, pose, fov, picture_.Get(), pictureFormat_, screen, cvr::render::Blend::Over,
                     opacity_ * shown_))
        return false;
    auto* lines = at_.caption ? caption_.Picture(device, list) : nullptr;
    if (!lines) return true;
    // The lines under what the screen shows, at its scale: their band of it, as wide as they were set across it, down
    // the screen's own down (the space's for a screen hung level).
    const float px = screen.height / cvr::render::Caption::kScreenHeight;
    const float drop = 0.5f * (caption_.Top() + caption_.Bottom()) * px;
    const XrVector3f down = RotateVector(screen.pose.orientation, {0.0f, -drop, 0.0f});
    cvr::render::SpaceQuad band;
    band.pose = screen.pose;
    band.pose.position = {band.pose.position.x + down.x, band.pose.position.y + down.y, band.pose.position.z + down.z};
    band.width = caption_.Width() * px;
    band.height = (caption_.Bottom() - caption_.Top()) * px;
    return quads_.Draw(device, list, eye, pose, fov, lines, caption_.Format(), band, cvr::render::Blend::Light,
                       opacity_ * shown_);
}

void Screen::Shutdown() {
    quads_.Shutdown();
    caption_.Shutdown();
    picture_.Reset();
    retired_.clear();
    pictureWidth_ = pictureHeight_ = levels_ = 0;
    pictureFormat_ = DXGI_FORMAT_UNKNOWN;
    firstFrame_ = 0;
    up_ = held_ = fresh_ = ready_ = false;
    opacity_ = 0.0f;
    shown_ = 1.0f;
}
}  // namespace cvr::loading
