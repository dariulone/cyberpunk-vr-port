// stereo_check.exe -- do the two eyes of a captured frame show the same world?
//
//   stereo_check.exe CAPTURE_DIR OUT_DIR [--frame N] [--jitter minus|plus|none] [--sbs-width 2560]
//
// CAPTURE_DIR is a frame capture of the plugin (CyberpunkVR_FramegenCapture: capture.json and f<N>_e<eye>_{color,
// depth,motion}.raw per eye -- both eyes' colour at the eye's output size, the engine depth at its render size and
// each eye's camera). Every depth texel of one eye is unprojected through its camera (reversed depth, its OpenXR fov
// window) and projected into the other eye; where the other eye's depth there agrees (the point is visible to both)
// the two colours are compared. A stereo-correct renderer leaves only view-dependent differences (specular,
// reflections at their own parallax) and disocclusions; one eye missing an object, a different LOD, a screen-space
// effect computed from the wrong eye (SSR, AO, shadows reading the other half) or a leak across the side-by-side seam
// shows up as a region of large differences or of geometry that does not agree.
//
// Writes OUT_DIR/stereo.json (metrics both ways), L.jpg / R.jpg (each eye at its own size and aspect), SBS.jpg (both,
// scaled to --sbs-width), heat_L.png / heat_R.png (per depth texel of that eye: grey = colour difference x4,
// red = difference > 32 of 255, blue = the point is hidden in the other eye or outside it, black = sky).
#define NOMINMAX
#include <windows.h>
#include <wincodec.h>
#include <wrl/client.h>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <limits>
#include <map>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#pragma comment(lib, "windowscodecs.lib")
#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "oleaut32.lib")

using Microsoft::WRL::ComPtr;

// ---- minimal JSON (objects, arrays, numbers, strings, literals) --------------------------------------------------
struct Json {
    enum Type { Null, Number, String, Array, Object, Bool } type{Null};
    double number{};
    std::string text;
    std::vector<Json> items;
    std::map<std::string, Json> fields;
    const Json& operator[](const char* key) const {
        static const Json none;
        auto it = fields.find(key);
        return it == fields.end() ? none : it->second;
    }
    const Json& operator[](size_t i) const {
        static const Json none;
        return i < items.size() ? items[i] : none;
    }
    double num(double fallback = 0) const { return type == Number ? number : (type == Bool ? number : fallback); }
};
struct Parser {
    const char* p;
    void ws() { while (*p == ' ' || *p == '\n' || *p == '\r' || *p == '\t') ++p; }
    Json value() {
        ws();
        Json v;
        if (*p == '{') {
            v.type = Json::Object; ++p; ws();
            if (*p == '}') { ++p; return v; }
            for (;;) {
                ws(); std::string key = string(); ws(); if (*p == ':') ++p;
                v.fields[key] = value(); ws();
                if (*p == ',') { ++p; continue; }
                if (*p == '}') ++p;
                break;
            }
        } else if (*p == '[') {
            v.type = Json::Array; ++p; ws();
            if (*p == ']') { ++p; return v; }
            for (;;) {
                v.items.push_back(value()); ws();
                if (*p == ',') { ++p; continue; }
                if (*p == ']') ++p;
                break;
            }
        } else if (*p == '"') {
            v.type = Json::String; v.text = string();
        } else if (!std::strncmp(p, "true", 4)) { v.type = Json::Bool; v.number = 1; p += 4; }
        else if (!std::strncmp(p, "false", 5)) { v.type = Json::Bool; v.number = 0; p += 5; }
        else if (!std::strncmp(p, "null", 4)) { p += 4; }
        else { v.type = Json::Number; char* end = nullptr; v.number = std::strtod(p, &end); p = end > p ? end : p + 1; }
        return v;
    }
    std::string string() {
        std::string out;
        if (*p != '"') return out;
        ++p;
        while (*p && *p != '"') { if (*p == '\\' && p[1]) ++p; out += *p++; }
        if (*p == '"') ++p;
        return out;
    }
};

// ---- vectors / cameras ---------------------------------------------------------------------------------------------
struct V3 { double x{}, y{}, z{}; };
static V3 operator+(V3 a, V3 b) { return {a.x + b.x, a.y + b.y, a.z + b.z}; }
static V3 operator-(V3 a, V3 b) { return {a.x - b.x, a.y - b.y, a.z - b.z}; }
static V3 operator*(V3 a, double s) { return {a.x * s, a.y * s, a.z * s}; }
static double dot(V3 a, V3 b) { return a.x * b.x + a.y * b.y + a.z * b.z; }
static V3 vec(const Json& j) { return {j[size_t(0)].num(), j[size_t(1)].num(), j[size_t(2)].num()}; }

struct Camera {
    V3 position, right, up, forward;
    double l{}, r{}, u{}, d{};          // tangents of the fov window
    double nearPlane{}, farPlane{};
    bool inverted{}, infinite{};
    double jitter[2]{};
    double Depth(double raw) const {    // view distance along forward from the stored depth, <= 0 for sky
        if (raw <= 1e-7) return inverted ? -1 : -1;
        if (infinite) return inverted ? nearPlane / raw : -1;
        const double n = nearPlane, f = farPlane;
        double A, B;
        if (inverted) { B = n * f / (f - n); A = -n / (f - n); }
        else { B = -n * f / (f - n); A = f / (f - n); }
        const double z = B / (raw - A);
        return z > 0 ? z : -1;
    }
};
static Camera camera(const Json& eye) {
    Camera c;
    c.position = vec(eye["position"]); c.right = vec(eye["right"]); c.up = vec(eye["up"]); c.forward = vec(eye["forward"]);
    const Json& fov = eye["fov"];
    c.l = std::tan(fov[size_t(0)].num()); c.r = std::tan(fov[size_t(1)].num());
    c.u = std::tan(fov[size_t(2)].num()); c.d = std::tan(fov[size_t(3)].num());
    c.nearPlane = eye["planes"][size_t(0)].num(); c.farPlane = eye["planes"][size_t(1)].num();
    c.inverted = eye["inverted"].num() != 0; c.infinite = eye["infinite"].num() != 0;
    c.jitter[0] = eye["jitter"][size_t(0)].num(); c.jitter[1] = eye["jitter"][size_t(1)].num();
    return c;
}

// ---- images --------------------------------------------------------------------------------------------------------
struct Image {
    int w{}, h{}, format{};
    std::vector<uint8_t> raw;
};
struct Rgb {
    int w{}, h{};
    std::vector<uint8_t> px;   // RGB8, display encoded
};
static bool ReadFile(const std::wstring& path, std::vector<uint8_t>& out) {
    std::ifstream f(path, std::ios::binary);
    if (!f) return false;
    out.assign(std::istreambuf_iterator<char>(f), {});
    return true;
}
static float Half(uint16_t h) {
    const uint32_t s = (h >> 15) & 1, e = (h >> 10) & 31, m = h & 1023;
    float v;
    if (e == 0) v = std::ldexp(float(m), -24);
    else if (e == 31) v = m ? std::numeric_limits<float>::quiet_NaN() : std::numeric_limits<float>::infinity();
    else v = std::ldexp(float(m | 1024), int(e) - 25);
    return s ? -v : v;
}
static float Small(uint32_t bits, int mantissa) {   // unsigned 11/10-bit float (R11G11B10)
    const uint32_t e = bits >> mantissa, m = bits & ((1u << mantissa) - 1);
    if (e == 0) return std::ldexp(float(m), -14 - mantissa);
    if (e == 31) return 65504.0f;
    return std::ldexp(float(m | (1u << mantissa)), int(e) - 15 - mantissa);
}
static uint8_t Encode(float linear) {
    linear = std::clamp(std::isfinite(linear) ? linear : 0.f, 0.f, 1.f);
    const float s = linear <= 0.0031308f ? linear * 12.92f : 1.055f * std::pow(linear, 1 / 2.4f) - 0.055f;
    return uint8_t(std::lround(s * 255));
}
// DXGI formats the eye snapshots come in. UNORM / _SRGB bytes are display encoded as stored; float formats are
// linear and get the sRGB curve (clamped: a check of agreement, not a tone mapper).
static bool ToRgb(const Image& in, Rgb& out) {
    out.w = in.w; out.h = in.h; out.px.resize(size_t(in.w) * in.h * 3);
    const size_t n = size_t(in.w) * in.h;
    auto px = out.px.data();
    switch (in.format) {
    case 27: case 28: case 29:   // R8G8B8A8
        if (in.raw.size() < n * 4) return false;
        for (size_t i = 0; i < n; ++i) { px[3 * i] = in.raw[4 * i]; px[3 * i + 1] = in.raw[4 * i + 1]; px[3 * i + 2] = in.raw[4 * i + 2]; }
        return true;
    case 87: case 88: case 90: case 91:   // B8G8R8A8 / X8
        if (in.raw.size() < n * 4) return false;
        for (size_t i = 0; i < n; ++i) { px[3 * i] = in.raw[4 * i + 2]; px[3 * i + 1] = in.raw[4 * i + 1]; px[3 * i + 2] = in.raw[4 * i]; }
        return true;
    case 23: case 24: case 25: {  // R10G10B10A2
        if (in.raw.size() < n * 4) return false;
        const auto* v = reinterpret_cast<const uint32_t*>(in.raw.data());
        for (size_t i = 0; i < n; ++i) for (int c = 0; c < 3; ++c) px[3 * i + c] = uint8_t(((v[i] >> (10 * c)) & 1023) * 255 / 1023);
        return true;
    }
    case 10: case 11: {           // R16G16B16A16_FLOAT
        if (in.raw.size() < n * 8) return false;
        const auto* v = reinterpret_cast<const uint16_t*>(in.raw.data());
        for (size_t i = 0; i < n; ++i) for (int c = 0; c < 3; ++c) px[3 * i + c] = Encode(Half(v[4 * i + c]));
        return true;
    }
    case 26: {                    // R11G11B10_FLOAT
        if (in.raw.size() < n * 4) return false;
        const auto* v = reinterpret_cast<const uint32_t*>(in.raw.data());
        for (size_t i = 0; i < n; ++i) {
            px[3 * i] = Encode(Small(v[i] & 2047, 6));
            px[3 * i + 1] = Encode(Small((v[i] >> 11) & 2047, 6));
            px[3 * i + 2] = Encode(Small(v[i] >> 22, 5));
        }
        return true;
    }
    default:
        return false;
    }
}
static bool Depth(const Image& in, std::vector<float>& out) {
    const size_t n = size_t(in.w) * in.h;
    if (in.raw.size() < n * 4) return false;
    out.resize(n);
    std::memcpy(out.data(), in.raw.data(), n * 4);   // R32_FLOAT / R32_TYPELESS / D32 as stored
    return true;
}

// ---- WIC output ----------------------------------------------------------------------------------------------------
static bool Save(const std::wstring& path, const Rgb& image, bool jpeg, float quality = 0.92f) {
    ComPtr<IWICImagingFactory> factory;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory)))) return false;
    ComPtr<IWICStream> stream;
    if (FAILED(factory->CreateStream(&stream)) || FAILED(stream->InitializeFromFilename(path.c_str(), GENERIC_WRITE))) return false;
    ComPtr<IWICBitmapEncoder> encoder;
    if (FAILED(factory->CreateEncoder(jpeg ? GUID_ContainerFormatJpeg : GUID_ContainerFormatPng, nullptr, &encoder)) ||
        FAILED(encoder->Initialize(stream.Get(), WICBitmapEncoderNoCache))) return false;
    ComPtr<IWICBitmapFrameEncode> frame;
    ComPtr<IPropertyBag2> options;
    if (FAILED(encoder->CreateNewFrame(&frame, &options))) return false;
    if (jpeg && options) {
        PROPBAG2 option{}; option.pstrName = const_cast<LPOLESTR>(L"ImageQuality");
        VARIANT value; VariantInit(&value); value.vt = VT_R4; value.fltVal = quality;
        options->Write(1, &option, &value);
    }
    if (FAILED(frame->Initialize(options.Get())) || FAILED(frame->SetSize(image.w, image.h))) return false;
    WICPixelFormatGUID format = GUID_WICPixelFormat24bppBGR;
    if (FAILED(frame->SetPixelFormat(&format))) return false;
    std::vector<uint8_t> bgr(image.px.size());
    for (size_t i = 0; i + 2 < image.px.size(); i += 3) { bgr[i] = image.px[i + 2]; bgr[i + 1] = image.px[i + 1]; bgr[i + 2] = image.px[i]; }
    if (FAILED(frame->WritePixels(image.h, image.w * 3, UINT(bgr.size()), bgr.data()))) return false;
    return SUCCEEDED(frame->Commit()) && SUCCEEDED(encoder->Commit());
}
static Rgb Scale(const Rgb& in, int width) {   // box filter down to `width`, aspect kept
    if (width >= in.w) return in;
    Rgb out; out.w = width; out.h = std::max(1, int(std::lround(double(in.h) * width / in.w)));
    out.px.resize(size_t(out.w) * out.h * 3);
    const double sx = double(in.w) / out.w, sy = double(in.h) / out.h;
    for (int y = 0; y < out.h; ++y) for (int x = 0; x < out.w; ++x) {
        const int x0 = int(x * sx), x1 = std::max(x0 + 1, int((x + 1) * sx)), y0 = int(y * sy), y1 = std::max(y0 + 1, int((y + 1) * sy));
        double acc[3]{}; int count = 0;
        for (int yy = y0; yy < y1 && yy < in.h; ++yy) for (int xx = x0; xx < x1 && xx < in.w; ++xx, ++count)
            for (int c = 0; c < 3; ++c) acc[c] += in.px[(size_t(yy) * in.w + xx) * 3 + c];
        for (int c = 0; c < 3; ++c) out.px[(size_t(y) * out.w + x) * 3 + c] = uint8_t(acc[c] / std::max(count, 1) + .5);
    }
    return out;
}

// ---- the check -----------------------------------------------------------------------------------------------------
struct Eye {
    Camera cam;
    Rgb color;
    std::vector<float> depth;
    int dw{}, dh{};
};
static void Sample(const Rgb& image, double u, double v, double out[3]) {   // bilinear at uv in [0,1]
    const double fx = std::clamp(u * image.w - 0.5, 0.0, image.w - 1.0), fy = std::clamp(v * image.h - 0.5, 0.0, image.h - 1.0);
    const int x0 = int(fx), y0 = int(fy), x1 = std::min(x0 + 1, image.w - 1), y1 = std::min(y0 + 1, image.h - 1);
    const double ax = fx - x0, ay = fy - y0;
    for (int c = 0; c < 3; ++c) {
        auto at = [&](int x, int y) { return double(image.px[(size_t(y) * image.w + x) * 3 + c]); };
        out[c] = (at(x0, y0) * (1 - ax) + at(x1, y0) * ax) * (1 - ay) + (at(x0, y1) * (1 - ax) + at(x1, y1) * ax) * ay;
    }
}
struct Stats {
    size_t texels{}, sky{}, outside{}, hidden{}, compared{};
    std::vector<float> diffs;
    double edgeSum{}, centreSum{}; size_t edgeCount{}, centreCount{};
    double grid[4][4]{}; size_t gridCount[4][4]{};
    std::vector<float> depthErrors;   // |z_other - z_predicted| / z over compared texels
};
static double Percentile(std::vector<float> v, double p) {
    if (v.empty()) return 0;
    const size_t k = std::min(v.size() - 1, size_t(p * (v.size() - 1)));
    std::nth_element(v.begin(), v.begin() + k, v.end());
    return v[k];
}
// From eye `a` into eye `b`. `inner` is the side of a's image towards the nose: 0 = its right edge (left eye),
// 1 = its left edge (right eye).
static Stats Check(const Eye& a, const Eye& b, int inner, int jitterSign, Rgb& heat) {
    Stats s;
    heat.w = a.dw; heat.h = a.dh; heat.px.assign(size_t(a.dw) * a.dh * 3, 0);
    s.diffs.reserve(size_t(a.dw) * a.dh);
    for (int y = 0; y < a.dh; ++y) for (int x = 0; x < a.dw; ++x) {
        const size_t i = size_t(y) * a.dw + x;
        ++s.texels;
        const double z = a.cam.Depth(a.depth[i]);
        uint8_t* h = &heat.px[i * 3];
        if (z <= 0 || z > 5000) { ++s.sky; continue; }
        const double ua = (x + 0.5 + jitterSign * a.cam.jitter[0]) / a.dw, va = (y + 0.5 + jitterSign * a.cam.jitter[1]) / a.dh;
        const double tx = a.cam.l + (a.cam.r - a.cam.l) * ua, ty = a.cam.u - (a.cam.u - a.cam.d) * va;
        const V3 p = a.cam.position + (a.cam.forward + a.cam.right * tx + a.cam.up * ty) * z;
        const V3 rel = p - b.cam.position;
        const double zb = dot(rel, b.cam.forward);
        if (zb <= 0.05) { ++s.outside; h[2] = 160; continue; }
        const double ub = (dot(rel, b.cam.right) / zb - b.cam.l) / (b.cam.r - b.cam.l);
        const double vb = (b.cam.u - dot(rel, b.cam.up) / zb) / (b.cam.u - b.cam.d);
        if (ub < 0 || ub >= 1 || vb < 0 || vb >= 1) { ++s.outside; h[2] = 160; continue; }
        // The other eye's depth where the point lands (its jitter undone the same way).
        const int bx = std::clamp(int((ub - jitterSign * b.cam.jitter[0] / b.dw) * b.dw), 0, b.dw - 1);
        const int by = std::clamp(int((vb - jitterSign * b.cam.jitter[1] / b.dh) * b.dh), 0, b.dh - 1);
        const double zo = b.cam.Depth(b.depth[size_t(by) * b.dw + bx]);
        if (zo <= 0 || std::fabs(zo - zb) > 0.02 * zb + 0.02) { ++s.hidden; h[2] = 255; continue; }
        double ca[3], cb[3];
        Sample(a.color, ua, va, ca);
        Sample(b.color, ub, vb, cb);
        const float diff = float((std::fabs(ca[0] - cb[0]) + std::fabs(ca[1] - cb[1]) + std::fabs(ca[2] - cb[2])) / 3);
        ++s.compared;
        s.diffs.push_back(diff);
        s.depthErrors.push_back(float(std::fabs(zo - zb) / zb));
        const bool edge = inner == 0 ? x >= a.dw * 0.92 : x < a.dw * 0.08;
        (edge ? s.edgeSum : s.centreSum) += diff; ++(edge ? s.edgeCount : s.centreCount);
        const int gx = std::min(3, x * 4 / a.dw), gy = std::min(3, y * 4 / a.dh);
        s.grid[gy][gx] += diff; ++s.gridCount[gy][gx];
        const uint8_t g = uint8_t(std::min(255.f, diff * 4));
        h[0] = diff > 32 ? 255 : g; h[1] = diff > 32 ? 0 : g; h[2] = diff > 32 ? 0 : g;
    }
    return s;
}
static std::string Report(const Stats& s) {
    std::ostringstream o;
    const double n = double(std::max<size_t>(s.compared, 1));
    double mean = 0; size_t over12 = 0, over32 = 0;
    for (float d : s.diffs) { mean += d; over12 += d > 12; over32 += d > 32; }
    mean /= n;
    const double geometric = double(s.compared) / double(std::max<size_t>(s.compared + s.hidden, 1));
    o << "{\"texels\":" << s.texels << ",\"sky\":" << s.sky << ",\"outside\":" << s.outside << ",\"hidden\":" << s.hidden
      << ",\"compared\":" << s.compared << ",\"geometricAgreement\":" << geometric
      << ",\"meanDiff\":" << mean << ",\"p50\":" << Percentile(s.diffs, .5) << ",\"p95\":" << Percentile(s.diffs, .95)
      << ",\"p99\":" << Percentile(s.diffs, .99) << ",\"over12\":" << over12 / n << ",\"over32\":" << over32 / n
      << ",\"innerEdgeMean\":" << (s.edgeCount ? s.edgeSum / s.edgeCount : 0)
      << ",\"centreMean\":" << (s.centreCount ? s.centreSum / s.centreCount : 0)
      << ",\"depthErrorP50\":" << Percentile(s.depthErrors, .5) << ",\"depthErrorP95\":" << Percentile(s.depthErrors, .95)
      << ",\"grid\":[";
    for (int y = 0; y < 4; ++y) for (int x = 0; x < 4; ++x)
        o << (y || x ? "," : "") << (s.gridCount[y][x] ? s.grid[y][x] / s.gridCount[y][x] : 0);
    o << "]}";
    return o.str();
}

// ---- compare mode: the same eye of two runs (a variant against a base) --------------------------------------------
static bool Load(const std::wstring& path, Rgb& image) {
    ComPtr<IWICImagingFactory> factory;
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr, CLSCTX_INPROC_SERVER, IID_PPV_ARGS(&factory)))) return false;
    ComPtr<IWICBitmapDecoder> decoder;
    if (FAILED(factory->CreateDecoderFromFilename(path.c_str(), nullptr, GENERIC_READ, WICDecodeMetadataCacheOnDemand, &decoder))) return false;
    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(decoder->GetFrame(0, &frame))) return false;
    ComPtr<IWICFormatConverter> converter;
    if (FAILED(factory->CreateFormatConverter(&converter)) ||
        FAILED(converter->Initialize(frame.Get(), GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, nullptr, 0, WICBitmapPaletteTypeCustom)))
        return false;
    UINT w = 0, h = 0;
    converter->GetSize(&w, &h);
    image.w = int(w); image.h = int(h); image.px.resize(size_t(w) * h * 3);
    if (FAILED(converter->CopyPixels(nullptr, w * 3, UINT(image.px.size()), image.px.data()))) return false;
    for (size_t i = 0; i + 2 < image.px.size(); i += 3) std::swap(image.px[i], image.px[i + 2]);
    return true;
}
// Mean / p99 absolute difference (0..255, channels averaged), the share above 16, a 4x4 grid of means, and a heat image
// (difference x8) for L and R of two output folders of this tool.
static int Compare(const std::wstring& a, const std::wstring& b, const std::wstring& out) {
    if (FAILED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) return 1;
    { std::error_code ec; std::filesystem::create_directories(out, ec); }   // the output folder and its parents
    std::ostringstream json;
    json << "{";
    bool ok = true;
    const wchar_t* names[2] = {L"L", L"R"};
    for (int e = 0; e < 2 && ok; ++e) {
        Rgb x, y;
        if (!Load(a + L"\\" + names[e] + L".jpg", x) || !Load(b + L"\\" + names[e] + L".jpg", y)) {
            std::fprintf(stderr, "compare: cannot read %ls.jpg of both folders\n", names[e]); ok = false; break;
        }
        if (x.w != y.w || x.h != y.h) {
            std::fprintf(stderr, "compare: %ls sizes differ (%dx%d vs %dx%d)\n", names[e], x.w, x.h, y.w, y.h); ok = false; break;
        }
        std::vector<float> diffs(size_t(x.w) * x.h);
        Rgb heat; heat.w = x.w; heat.h = x.h; heat.px.assign(x.px.size(), 0);
        double sum = 0, grid[4][4]{}; size_t over = 0, gridCount[4][4]{};
        for (int yy = 0; yy < x.h; ++yy) for (int xx = 0; xx < x.w; ++xx) {
            const size_t i = size_t(yy) * x.w + xx;
            const float d = (std::fabs(float(x.px[3 * i]) - y.px[3 * i]) + std::fabs(float(x.px[3 * i + 1]) - y.px[3 * i + 1]) +
                             std::fabs(float(x.px[3 * i + 2]) - y.px[3 * i + 2])) / 3;
            diffs[i] = d; sum += d; over += d > 16;
            const int gx = std::min(3, xx * 4 / x.w), gy = std::min(3, yy * 4 / x.h);
            grid[gy][gx] += d; ++gridCount[gy][gx];
            const uint8_t g = uint8_t(std::min(255.f, d * 8));
            heat.px[3 * i] = heat.px[3 * i + 1] = heat.px[3 * i + 2] = g;
        }
        ok = Save(out + L"\\diff_" + names[e] + L".png", Scale(heat, 1324), false);
        if (!ok) std::fprintf(stderr, "compare: cannot write diff_%ls.png\n", names[e]);
        const double n = double(diffs.size());
        json << (e ? "," : "") << "\"" << (e ? "R" : "L") << "\":{\"mean\":" << sum / n << ",\"p99\":" << Percentile(diffs, .99)
             << ",\"over16\":" << over / n << ",\"grid\":[";
        for (int gy = 0; gy < 4; ++gy) for (int gx = 0; gx < 4; ++gx)
            json << (gy || gx ? "," : "") << (gridCount[gy][gx] ? grid[gy][gx] / gridCount[gy][gx] : 0);
        json << "]}";
    }
    json << "}\n";
    CoUninitialize();
    if (!ok) return 1;
    std::ofstream(out + L"\\compare.json") << json.str();
    std::printf("%s", json.str().c_str());
    return 0;
}

int wmain(int argc, wchar_t** argv) {
    if (argc >= 5 && std::wstring(argv[1]) == L"--compare") return Compare(argv[2], argv[3], argv[4]);
    if (argc < 3) {
        std::fprintf(stderr, "usage: stereo_check CAPTURE_DIR OUT_DIR [--frame N] [--jitter minus|plus|none] [--sbs-width W]\n");
        return 2;
    }
    const std::wstring in = argv[1], out = argv[2];
    int frameIndex = -1, jitterSign = -1, sbsWidth = 2560;
    for (int i = 3; i + 1 < argc; i += 2) {
        const std::wstring key = argv[i], value = argv[i + 1];
        if (key == L"--frame") frameIndex = _wtoi(value.c_str());
        else if (key == L"--jitter") jitterSign = value == L"plus" ? 1 : (value == L"none" ? 0 : -1);
        else if (key == L"--sbs-width") sbsWidth = _wtoi(value.c_str());
    }
    { std::error_code ec; std::filesystem::create_directories(out, ec); }   // the output folder and its parents
    std::vector<uint8_t> text;
    if (!ReadFile(in + L"\\capture.json", text)) { std::fprintf(stderr, "no capture.json\n"); return 1; }
    text.push_back(0);
    Parser parser{reinterpret_cast<const char*>(text.data())};
    const Json root = parser.value();
    const Json& frames = root["frames"];
    if (frames.items.empty()) { std::fprintf(stderr, "no frames\n"); return 1; }
    if (frameIndex < 0) frameIndex = int(frames.items.size() / 2);
    frameIndex = std::min<int>(frameIndex, int(frames.items.size()) - 1);
    const Json& frame = frames[size_t(frameIndex)];
    Eye eyes[2];
    for (int e = 0; e < 2; ++e) {
        const Json& j = frame["eyes"][size_t(e)];
        eyes[e].cam = camera(j);
        wchar_t name[64];
        Image color, depth;
        color.w = int(j["color"][size_t(0)].num()); color.h = int(j["color"][size_t(1)].num()); color.format = int(j["color"][size_t(2)].num());
        depth.w = int(j["depth"][size_t(0)].num()); depth.h = int(j["depth"][size_t(1)].num()); depth.format = int(j["depth"][size_t(2)].num());
        swprintf_s(name, L"\\f%d_e%d_color.raw", frameIndex, e);
        if (!ReadFile(in + name, color.raw) || !ToRgb(color, eyes[e].color)) {
            std::fprintf(stderr, "eye %d colour: unreadable or format %d unsupported\n", e, color.format); return 1;
        }
        swprintf_s(name, L"\\f%d_e%d_depth.raw", frameIndex, e);
        if (!ReadFile(in + name, depth.raw) || !Depth(depth, eyes[e].depth)) {
            std::fprintf(stderr, "eye %d depth unreadable\n", e); return 1;
        }
        eyes[e].dw = depth.w; eyes[e].dh = depth.h;
    }
    if (FAILED(CoInitializeEx(nullptr, COINIT_MULTITHREADED))) return 1;
    Rgb heatL, heatR;
    const Stats leftToRight = Check(eyes[0], eyes[1], 0, jitterSign, heatL);
    const Stats rightToLeft = Check(eyes[1], eyes[0], 1, jitterSign, heatR);
    bool ok = Save(out + L"\\L.jpg", eyes[0].color, true) && Save(out + L"\\R.jpg", eyes[1].color, true) &&
              Save(out + L"\\heat_L.png", heatL, false) && Save(out + L"\\heat_R.png", heatR, false);
    Rgb sbs; sbs.w = eyes[0].color.w + eyes[1].color.w; sbs.h = std::max(eyes[0].color.h, eyes[1].color.h);
    sbs.px.assign(size_t(sbs.w) * sbs.h * 3, 0);
    for (int e = 0; e < 2; ++e) for (int y = 0; y < eyes[e].color.h; ++y)
        std::memcpy(&sbs.px[(size_t(y) * sbs.w + (e ? eyes[0].color.w : 0)) * 3], &eyes[e].color.px[size_t(y) * eyes[e].color.w * 3], size_t(eyes[e].color.w) * 3);
    ok = Save(out + L"\\SBS.jpg", Scale(sbs, sbsWidth), true, 0.9f) && ok;
    std::ostringstream json;
    json << "{\"frame\":" << frame["frame"].num() << ",\"index\":" << frameIndex
         << ",\"colorSize\":[" << eyes[0].color.w << "," << eyes[0].color.h << "],\"depthSize\":[" << eyes[0].dw << "," << eyes[0].dh << "]"
         << ",\"jitterSign\":" << jitterSign
         << ",\"separation\":" << std::sqrt(dot(eyes[1].cam.position - eyes[0].cam.position, eyes[1].cam.position - eyes[0].cam.position))
         << ",\"leftToRight\":" << Report(leftToRight) << ",\"rightToLeft\":" << Report(rightToLeft) << "}\n";
    std::ofstream(out + L"\\stereo.json") << json.str();
    std::printf("%s", json.str().c_str());
    CoUninitialize();
    return ok ? 0 : 1;
}
