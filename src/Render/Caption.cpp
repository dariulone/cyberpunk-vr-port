// Lines of text under a hung screen (Render/Caption.hpp).
#include "Render/Caption.hpp"
#include <d2d1.h>
#include <dwrite.h>
#include <algorithm>
#include <cmath>
#include <cstring>

extern void Log(const char*, ...);

namespace cvr::render {
namespace {
using Microsoft::WRL::ComPtr;

// The face: a plain sans the system has (Windows 10 on); DirectWrite falls back to another for letters it lacks.
constexpr wchar_t kFamily[] = L"Bahnschrift";

float Decode(float v) { return v <= 0.04045f ? v / 12.92f : std::pow((v + 0.055f) / 1.055f, 2.4f); }
uint8_t Encode(float v) {
    v = std::clamp(v, 0.0f, 1.0f);
    const float s = v <= 0.0031308f ? v * 12.92f : 1.055f * std::pow(v, 1.0f / 2.4f) - 0.055f;
    return static_cast<uint8_t>(std::lround(s * 255.0f));
}

// The level below: each texel the mean of the 2 x 2 above it (the edge's own where the level above is odd).
std::vector<float> Halve(const std::vector<float>& above, UINT w, UINT h, UINT& halfW, UINT& halfH) {
    halfW = (std::max)(1u, w / 2);
    halfH = (std::max)(1u, h / 2);
    std::vector<float> out(size_t(halfW) * halfH);
    for (UINT y = 0; y < halfH; ++y) {
        const UINT y0 = (std::min)(2 * y, h - 1), y1 = (std::min)(2 * y + 1, h - 1);
        for (UINT x = 0; x < halfW; ++x) {
            const UINT x0 = (std::min)(2 * x, w - 1), x1 = (std::min)(2 * x + 1, w - 1);
            out[size_t(y) * halfW + x] = 0.25f * (above[size_t(y0) * w + x0] + above[size_t(y0) * w + x1] +
                                                  above[size_t(y1) * w + x0] + above[size_t(y1) * w + x1]);
        }
    }
    return out;
}
}  // namespace

void Caption::Set(const CaptionText& text) {
    const auto same = [&] {
        if (text.lines.size() != text_.lines.size() || text.size != text_.size || text.light != text_.light ||
            text.rightToLeft != text_.rightToLeft || text.locale != text_.locale)
            return false;
        for (size_t i = 0; i < text.lines.size(); ++i)
            if (text.lines[i].text != text_.lines[i].text || text.lines[i].row != text_.lines[i].row) return false;
        return true;
    };
    if (picture_ && same()) return;
    text_ = text;
    dirty_ = true;
    failed_ = false;
}

bool Caption::Typeset(std::vector<float>& light) {
    if (text_.lines.empty()) return false;
    float first = text_.lines.front().row, last = first;
    for (const auto& line : text_.lines) {
        first = (std::min)(first, line.row);
        last = (std::max)(last, line.row);
    }
    top_ = first - text_.size;
    bottom_ = last + text_.size;
    width_ = static_cast<UINT>(std::lround(kScreenHeight * 16.0f / 9.0f));
    height_ = static_cast<UINT>(std::ceil(bottom_ - top_));

    ComPtr<IDWriteFactory> write;
    ComPtr<ID2D1Factory> d2d;
    if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory),
                                   reinterpret_cast<IUnknown**>(write.GetAddressOf()))) ||
        FAILED(D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, d2d.GetAddressOf())))
        return false;
    ComPtr<IDWriteTextFormat> format;
    if (FAILED(write->CreateTextFormat(kFamily, nullptr, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL,
                                       DWRITE_FONT_STRETCH_NORMAL, text_.size, text_.locale.c_str(), &format)))
        return false;
    format->SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
    format->SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
    format->SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
    if (text_.rightToLeft) format->SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT);

    // White on black into a top-down 32-bit DIB: each texel's light is how much of the letters cover it.
    BITMAPINFO info{};
    info.bmiHeader.biSize = sizeof(info.bmiHeader);
    info.bmiHeader.biWidth = static_cast<LONG>(width_);
    info.bmiHeader.biHeight = -static_cast<LONG>(height_);
    info.bmiHeader.biPlanes = 1;
    info.bmiHeader.biBitCount = 32;
    info.bmiHeader.biCompression = BI_RGB;
    void* bits = nullptr;
    HDC dc = CreateCompatibleDC(nullptr);
    HBITMAP bitmap = dc ? CreateDIBSection(dc, &info, DIB_RGB_COLORS, &bits, nullptr, 0) : nullptr;
    bool ok = false;
    if (bitmap && bits) {
        HGDIOBJ was = SelectObject(dc, bitmap);
        const auto props = D2D1::RenderTargetProperties(
            D2D1_RENDER_TARGET_TYPE_SOFTWARE, D2D1::PixelFormat(DXGI_FORMAT_B8G8R8A8_UNORM, D2D1_ALPHA_MODE_PREMULTIPLIED),
            96.0f, 96.0f);
        ComPtr<ID2D1DCRenderTarget> target;
        ComPtr<ID2D1SolidColorBrush> white;
        const RECT whole{0, 0, static_cast<LONG>(width_), static_cast<LONG>(height_)};
        if (SUCCEEDED(d2d->CreateDCRenderTarget(&props, &target)) && SUCCEEDED(target->BindDC(dc, &whole)) &&
            SUCCEEDED(target->CreateSolidColorBrush(D2D1::ColorF(1.0f, 1.0f, 1.0f, 1.0f), &white))) {
            target->BeginDraw();
            target->Clear(D2D1::ColorF(0.0f, 0.0f, 0.0f, 1.0f));
            target->SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            for (const auto& line : text_.lines) {
                ComPtr<IDWriteTextLayout> layout;
                if (FAILED(write->CreateTextLayout(line.text.c_str(), static_cast<UINT32>(line.text.size()), format.Get(),
                                                   float(width_), 2.0f * text_.size, &layout)))
                    continue;
                target->DrawTextLayout(D2D1::Point2F(0.0f, line.row - top_ - text_.size), layout.Get(), white.Get());
            }
            ok = SUCCEEDED(target->EndDraw());
        }
        GdiFlush();
        if (ok) {
            float decode[256];
            for (int i = 0; i < 256; ++i) decode[i] = Decode(float(i) / 255.0f) * text_.light;
            light.resize(size_t(width_) * height_);
            const auto* texel = static_cast<const uint8_t*>(bits);
            for (size_t i = 0; i < light.size(); ++i) light[i] = decode[texel[i * 4 + 2]];
        }
        SelectObject(dc, was);
    }
    if (bitmap) DeleteObject(bitmap);
    if (dc) DeleteDC(dc);
    return ok;
}

ID3D12Resource* Caption::Picture(ID3D12Device* device, ID3D12GraphicsCommandList* list) {
    if (!dirty_) return picture_.Get();
    if (failed_ || text_.lines.empty() || !device || !list) return nullptr;
    const ULONGLONG started = GetTickCount64();
    std::vector<float> top;
    if (!Typeset(top)) {
        failed_ = true;
        Log("[caption] the lines were not set: no caption\n");
        return nullptr;
    }
    if (picture_) retired_.push_back(std::move(picture_));
    if (upload_) retired_.push_back(std::move(upload_));
    levels_ = 1;
    while ((std::max)(width_, height_) >> levels_) ++levels_;

    D3D12_RESOURCE_DESC desc{};
    desc.Dimension = D3D12_RESOURCE_DIMENSION_TEXTURE2D;
    desc.Width = width_;
    desc.Height = height_;
    desc.DepthOrArraySize = 1;
    desc.MipLevels = static_cast<UINT16>(levels_);
    desc.Format = Format();
    desc.SampleDesc.Count = 1;
    std::vector<D3D12_PLACED_SUBRESOURCE_FOOTPRINT> footprints(levels_);
    UINT64 total = 0;
    device->GetCopyableFootprints(&desc, 0, levels_, 0, footprints.data(), nullptr, nullptr, &total);
    D3D12_RESOURCE_DESC buffer{};
    buffer.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER;
    buffer.Width = total;
    buffer.Height = 1;
    buffer.DepthOrArraySize = 1;
    buffer.MipLevels = 1;
    buffer.SampleDesc.Count = 1;
    buffer.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    const D3D12_HEAP_PROPERTIES gpuHeap{D3D12_HEAP_TYPE_DEFAULT}, uploadHeap{D3D12_HEAP_TYPE_UPLOAD};
    void* mapped = nullptr;
    const D3D12_RANGE noRead{0, 0};
    if (FAILED(device->CreateCommittedResource(&gpuHeap, D3D12_HEAP_FLAG_NONE, &desc, D3D12_RESOURCE_STATE_COPY_DEST,
                                               nullptr, IID_PPV_ARGS(&picture_))) ||
        FAILED(device->CreateCommittedResource(&uploadHeap, D3D12_HEAP_FLAG_NONE, &buffer,
                                               D3D12_RESOURCE_STATE_GENERIC_READ, nullptr, IID_PPV_ARGS(&upload_))) ||
        FAILED(upload_->Map(0, &noRead, &mapped))) {
        picture_.Reset();
        upload_.Reset();
        failed_ = true;
        Log("[caption] picture %ux%u not made\n", width_, height_);
        return nullptr;
    }
    picture_->SetName(L"CVR_caption");
    // Each level into its place in the upload buffer, sRGB, then copied into the picture.
    std::vector<float> level = std::move(top);
    UINT w = width_, h = height_;
    for (UINT mip = 0; mip < levels_; ++mip) {
        const auto& at = footprints[mip];
        auto* rows = static_cast<uint8_t*>(mapped) + at.Offset;
        for (UINT y = 0; y < h; ++y) {
            uint8_t* texel = rows + size_t(y) * at.Footprint.RowPitch;
            for (UINT x = 0; x < w; ++x, texel += 4) {
                const uint8_t v = Encode(level[size_t(y) * w + x]);
                texel[0] = texel[1] = texel[2] = v;
                texel[3] = 255;
            }
        }
        D3D12_TEXTURE_COPY_LOCATION from{}, to{};
        from.pResource = upload_.Get();
        from.Type = D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT;
        from.PlacedFootprint = at;
        to.pResource = picture_.Get();
        to.Type = D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX;
        to.SubresourceIndex = mip;
        list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
        if (mip + 1 < levels_) {
            UINT halfW = 0, halfH = 0;
            level = Halve(level, w, h, halfW, halfH);
            w = halfW;
            h = halfH;
        }
    }
    upload_->Unmap(0, nullptr);
    D3D12_RESOURCE_BARRIER barrier{};
    barrier.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    barrier.Transition.pResource = picture_.Get();
    barrier.Transition.Subresource = D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES;
    barrier.Transition.StateBefore = D3D12_RESOURCE_STATE_COPY_DEST;
    barrier.Transition.StateAfter = D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE;
    list->ResourceBarrier(1, &barrier);
    dirty_ = false;
    Log("[caption] %zu line(s) set: %ux%u, %u levels, %llu ms\n", text_.lines.size(), width_, height_, levels_,
        GetTickCount64() - started);
    return picture_.Get();
}

void Caption::Shutdown() {
    picture_.Reset();
    upload_.Reset();
    retired_.clear();
    dirty_ = !text_.lines.empty();
    failed_ = false;
}
}  // namespace cvr::render
