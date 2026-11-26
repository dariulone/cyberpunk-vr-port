#pragma once
#include <windows.h>
#include <d3d12.h>
#include <wrl.h>
#include <cstdint>
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

#if CVR_RUST_VRIK
// cvr::render::Caption in rust/crates/cvr-gpu/src/caption.rs: the lines handed over as UTF-16 with their rows.
extern "C" {
struct CvrCaptionLine {
    const wchar_t* text;
    uint32_t length;
    float row;
};
void* cvr_gpu_caption_new();
void cvr_gpu_caption_free(void* caption);
void cvr_gpu_caption_set(void* caption, const CvrCaptionLine* lines, uint32_t count, float size, float light,
                         const wchar_t* locale, bool rightToLeft);
ID3D12Resource* cvr_gpu_caption_picture(void* caption, ID3D12Device* device, ID3D12GraphicsCommandList* list);
void cvr_gpu_caption_extent(const void* caption, float* out);
void cvr_gpu_caption_shutdown(void* caption);
}
#endif

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
#if CVR_RUST_VRIK
    Caption() : rust_(cvr_gpu_caption_new()) {}
    ~Caption() { cvr_gpu_caption_free(rust_); }
    Caption(const Caption&) = delete;
    Caption& operator=(const Caption&) = delete;
    // The picture's band of the screen, px of it below its middle (2160 tall): its top and bottom edges; and its width.
    float Top() const { return Extent(0); }
    float Bottom() const { return Extent(1); }
    float Width() const { return Extent(2); }
    void Shutdown() { cvr_gpu_caption_shutdown(rust_); }

private:
    float Extent(int at) const {
        float extent[3]{};
        cvr_gpu_caption_extent(rust_, extent);
        return extent[at];
    }
    void* rust_;
};
inline void Caption::Set(const CaptionText& text) {
    std::vector<CvrCaptionLine> lines;
    lines.reserve(text.lines.size());
    for (const auto& line : text.lines) lines.push_back({line.text.c_str(), static_cast<uint32_t>(line.text.size()), line.row});
    cvr_gpu_caption_set(rust_, lines.data(), static_cast<uint32_t>(lines.size()), text.size, text.light, text.locale.c_str(),
                        text.rightToLeft);
}
inline ID3D12Resource* Caption::Picture(ID3D12Device* device, ID3D12GraphicsCommandList* list) {
    return cvr_gpu_caption_picture(rust_, device, list);
}
#else
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
#endif
}  // namespace cvr::render
