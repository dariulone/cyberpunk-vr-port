#pragma once
#include "Render/SpaceQuad.hpp"
#include <array>
#include <cstdint>
#include <memory>

// THE LOBBY'S CURTAIN AND INTRO, IN THE FRAME. While the lobby comes up its curtain darkens the world, and the first
// time in a launch its intro's picture (Lobby/Intro.hpp) hangs in that dark before the player. Both are drawn into the
// game's own frame as it is presented (Runtimes/OpenXRCapture.cpp): every eye's image gets the curtain and the
// picture -- a quad at the intro's heading (Lobby/Intro.hpp Heading), the same to both eyes (Render/SpaceQuad.hpp
// Ahead), seen through that eye's pose and frustum -- before the frame is copied for the headset and before the window
// shows it.
// So the monitor shows what the headset shows, and nothing of it depends on the runtime's own layers.
namespace cvr::hud { struct TextureLease; }
namespace cvr::lobby::intro {
struct Picture;

class Overlay {
public:
    // What this frame has to draw: the curtain's closure (0 open .. 1 shut) and the intro's picture. `head` is the
    // frame's head pose in the tracking space: it keeps the intro's heading up. False when there is nothing to draw.
    bool Begin(float curtain, const XrPosef& head);
    // One eye: `target` its image, a render target now, holding sRGB-encoded colour; `pose` and `fov` the eye it was
    // rendered for. `picture` false draws the curtain alone (one picture shared by both eyes: a menu's window).
    // Recorded on `list`, which runs before the frame is read; the picture's texture is kept until `slot` comes round
    // again.
    bool Record(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* target, const D3D12_RECT& rect,
                const XrPosef& pose, const XrFovf& fov, bool picture, uint32_t slot);
    void Shutdown();

private:
    cvr::render::SpaceQuads quads_;
    // The picture's texture per capture slot: alive until that slot's work is done.
    std::array<std::shared_ptr<cvr::hud::TextureLease>, 3> held_{};
    std::shared_ptr<const Picture> shown_;
    float curtain_ = 0.0f;
    // The intro last logged.
    uint64_t loggedIntro_ = 0;
};
}  // namespace cvr::lobby::intro
