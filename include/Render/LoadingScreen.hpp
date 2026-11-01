#pragma once
#include "Render/Caption.hpp"
#include "Render/SpaceQuad.hpp"
#include <vector>

// THE LOADING SCREEN, HUNG IN THE DARK. While a save loads -- a new game begins, a fast travel, the way back after a
// death -- the game shows its loading screen over the whole window: the animated art, the progress bar, the spinner.
// Squeezed into each eye it would be a flat sheet pressed to the face, following the head; shutting the eyes on it
// (Lobby/MenuLobby.hpp LoadingDark) leaves the player in the dark with no sign of progress. Here it is a screen hung
// in the void before the player: each eye's image is the dark with the screen drawn into it through that eye's pose
// and frustum (Render/SpaceQuad.hpp), so it stands at its distance, stays where it is when the head moves and the eyes
// fuse it in stereo. Its picture is the window as the game drew it this frame, the part the screen is laid out in, so
// the progress it shows is the game's own. When: Lobby/MenuLobby.hpp LoadingInSpace; drawn by the capture
// (Runtimes/OpenXRCapture.cpp). Capture thread only.
//
// The boot's screen, the intro's notice (Lobby/Intro.hpp) with the port's word under it (Render/Caption.hpp), is not
// hung in stereo: it is one picture to both eyes at the intro's heading, as the intro's logo after it is.
namespace cvr::loading {
// Where a screen hangs: this far before the eyes (m), this tall -- or, given a width, that wide whatever its aspect --
// coming up out of the dark over `fadeIn` s; with `caption`, those lines under what it shows. `heading`: at that
// heading, kept up by each frame's head, one picture to both eyes (Render/SpaceQuad.hpp Ahead); without, hung where
// the eyes were.
struct Placement {
    float distance = 3.0f, height = 1.8f, width = 0.0f, fadeIn = 0.4f;
    const cvr::render::CaptionText* caption = nullptr;
    cvr::render::FreeLookHeading* heading = nullptr;
};
// A loading screen: a 16:9 screen 3.2 m wide at 3 m, some 56 degrees, the whole of it in view with room around it.
constexpr Placement kLoadingScreen{};

class Screen {
public:
    // A frame of the screen: `head` is its head pose in the tracking space, `opacity` how much of it shows. The first
    // frame of a screen with a located head (Render/SpaceQuad.hpp LocatedPose) places it before the head, level.
    void Begin(const XrPosef& head, const Placement& at = kLoadingScreen, float opacity = 1.0f);
    // Placed: until then its frames are the dark alone.
    bool Up() const { return up_; }
    // A frame without one: the next loading screen is placed anew. Not a time without frames -- a load stalls the game
    // for seconds, and the screen stays where it hung.
    void End() { up_ = held_ = false; }
    // The window copy the screen is drawn from holds a picture of this screen, taken this frame. The screen is shown
    // from the first one on, fading in.
    void Hold();
    bool Held() const { return held_; }
    // One eye's image -- `target`, a render target now, written through `view` -- made the dark with the screen in it
    // (once it holds a picture) as seen from `pose` with `fov`. `window` is the game's window as drawn this frame, a
    // shader resource now, read through `windowView`; `uv` the part of it the screen shows, `aspect` that part's width
    // over its height as laid out. The first eye after Hold() takes the screen's picture from it.
    bool Record(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* target, DXGI_FORMAT view,
                const D3D12_RECT& rect, const XrPosef& pose, const XrFovf& fov, ID3D12Resource* window,
                DXGI_FORMAT windowView, const float uv[4], float aspect);
    void Shutdown();

private:
    // The screen's picture: its part of the window at its own size with all its mips below (the screen is about a third
    // of that size on the eye).
    bool EnsurePicture(ID3D12Device* device, UINT width, UINT height, DXGI_FORMAT format);
    bool BuildPicture(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* window,
                      DXGI_FORMAT windowView, const float uv[4]);

    cvr::render::SpaceQuads quads_;
    cvr::render::Caption caption_;
    Microsoft::WRL::ComPtr<ID3D12Resource> picture_;
    std::vector<Microsoft::WRL::ComPtr<ID3D12Resource>> retired_;
    UINT pictureWidth_ = 0, pictureHeight_ = 0, levels_ = 0;
    DXGI_FORMAT pictureFormat_ = DXGI_FORMAT_UNKNOWN;
    Placement at_{};
    // Where it hangs (a screen without a heading).
    XrPosef anchor_{{0, 0, 0, 1}, {0, 0, 0}};
    unsigned long long firstFrame_ = 0;
    bool up_ = false, held_ = false, fresh_ = false, ready_ = false;
    float opacity_ = 0.0f, shown_ = 1.0f;
};
}  // namespace cvr::loading
