#pragma once
#include <windows.h>
#include <d3d12.h>
#include <wrl.h>
#include <string>
#include <vector>

// LINES OF TEXT UNDER A HUNG SCREEN. A screen the port hangs in the dark (Render/LoadingScreen.hpp) can carry a few
// lines of its own under what it shows -- the boot's screen the port's word under the game's legal notice (Lobby/
// Notice.hpp) -- where no ink of the game's is drawn (the boot's loading screen plays its bink with no widgets). The
// lines are set with DirectWrite in the system's fonts, which have every script the game is translated into, shaped,
// with fallback fonts where one lacks letters, and laid out right to left where the language reads so; into a picture
// with all its mips below, made once and uploaded once. Capture thread only.
namespace cvr::render {
struct CaptionLine {
    std::wstring text;
    float row = 0.0f;  // its centre, px below the screen's middle
};
// The lines, laid out in px of the screen they hang under as if it were 2160 px tall (its width at that scale): each
// centred across it, the letters `size` px, white of `light` (linear). `locale` picks the letters' forms where a
// script is shared (the Han of Japanese and of Chinese); `rightToLeft` the reading order.
struct CaptionText {
    std::vector<CaptionLine> lines;
    float size = 51.0f, light = 0.35f;
    std::wstring locale = L"en-us";
    bool rightToLeft = false;
};

class Caption {
public:
    // The screen's height the text is laid out at (px).
    static constexpr float kScreenHeight = 2160.0f;
    // The lines to show; a new text is drawn anew.
    void Set(const CaptionText& text);
    // The lines' picture, made and recorded to be uploaded on `list` the first time; null while there is none. A
    // shader resource once returned.
    ID3D12Resource* Picture(ID3D12Device* device, ID3D12GraphicsCommandList* list);
    DXGI_FORMAT Format() const { return DXGI_FORMAT_R8G8B8A8_UNORM_SRGB; }
    // The picture's band of the screen, px of it below its middle (2160 tall): its top and bottom edges; and its width.
    float Top() const { return top_; }
    float Bottom() const { return bottom_; }
    float Width() const { return float(width_); }
    void Shutdown();

private:
    // The lines set into rows of linear light, the picture's top level.
    bool Typeset(std::vector<float>& light);

    CaptionText text_;
    bool dirty_ = false, failed_ = false;
    UINT width_ = 0, height_ = 0, levels_ = 0;
    float top_ = 0.0f, bottom_ = 0.0f;
    Microsoft::WRL::ComPtr<ID3D12Resource> picture_, upload_;
    std::vector<Microsoft::WRL::ComPtr<ID3D12Resource>> retired_;
};
}  // namespace cvr::render
