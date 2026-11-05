// afw_replay: PureDark's AFW plugin (PDAFWPlugin.dll) run on frames captured from the game, scored against the eye
// the game really rendered.
//
//   afw_replay.exe PLUGIN.dll CAPTURE_DIR OUT_DIR [--view rh|lh] [--depth reversed|standard] [--mode combined|alternate|previous]
//                  [--mv normal|other] [--mvsign 1|-1] [--debug]
//
// CAPTURE_DIR is one framegen capture (Framegen/FrameCapture.cpp: three consecutive frames of the engine's stereo,
// both eyes rendered at the same moment, with colour, depth, motion vectors and both cameras). The replay takes
// turns the way AFW runs live: frame 0 gives the plugin the LEFT eye and asks for the right, frame 1 the right and
// asks for the left, frame 2 the left again. Every made eye has the real one beside it, so each step is scored
// (PSNR over the whole eye and over its middle 80%), and the made eyes are written raw (RGBA8) for pictures.
//
// The camera conventions the plugin wants are not documented: UEVR hands it Unreal's matrices through its own
// to_reverseZ. The view space (right- or left-handed) and the depth convention of the projection are options, so
// the replay can be run over all of them and the one that scores is the one to use in the port.
#include <windows.h>
#include <d3d12.h>
#include <dxgi1_6.h>
#include <wrl/client.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>
#include <vector>
#include "Render/AfwPlugin.hpp"

using Microsoft::WRL::ComPtr;

namespace {

// ---------------------------------------------------------------- the capture

struct Eye {
    int cw = 0, ch = 0, dw = 0, dh = 0, mw = 0, mh = 0;
    std::vector<uint8_t> color, depth, motion;
    float position[3]{}, up[3]{}, right[3]{}, forward[3]{};
    float nearPlane = 0, farPlane = 0;
    float fov[4]{};                         // angleLeft, angleRight, angleUp, angleDown (radians)
    float jitter[2]{}, motionScale[2]{};
    int inverted = 0, infinite = 0;
};
struct Frame { Eye eye[2]; };

std::string ReadText(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    std::stringstream s;
    s << f.rdbuf();
    return s.str();
}
std::vector<uint8_t> ReadBytes(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}
// The capture's JSON is the plugin's own simple writer: arrays of numbers under known keys, in order. Read the n-th
// eye's values for a key by position.
bool Floats(const std::string& text, size_t& at, const char* key, float* out, int n) {
    const std::string k = std::string("\"") + key + "\":[";
    const size_t p = text.find(k, at);
    if (p == std::string::npos) return false;
    const char* c = text.c_str() + p + k.size();
    for (int i = 0; i < n; ++i) {
        char* end = nullptr;
        out[i] = std::strtof(c, &end);
        c = end;
        while (*c == ',' || *c == ' ') ++c;
    }
    return true;
}
bool Ints(const std::string& text, size_t at, const char* key, int* out, int n) {
    float f[8]{};
    size_t a = at;
    if (!Floats(text, a, key, f, n)) return false;
    for (int i = 0; i < n; ++i) out[i] = int(f[i]);
    return true;
}
int IntAfter(const std::string& text, size_t at, const char* key) {
    const std::string k = std::string("\"") + key + "\":";
    const size_t p = text.find(k, at);
    return p == std::string::npos ? 0 : std::atoi(text.c_str() + p + k.size());
}

bool LoadCapture(const std::string& dir, std::vector<Frame>& frames) {
    const std::string text = ReadText(dir + "\\capture.json");
    if (text.empty()) return false;
    size_t at = 0;
    for (int f = 0; f < 3; ++f) {
        Frame frame;
        for (int e = 0; e < 2; ++e) {
            Eye& eye = frame.eye[e];
            const size_t start = text.find("{\"color\":", at);
            if (start == std::string::npos) return false;
            int v[3];
            Ints(text, start, "color", v, 3); eye.cw = v[0]; eye.ch = v[1];
            Ints(text, start, "depth", v, 3); eye.dw = v[0]; eye.dh = v[1];
            Ints(text, start, "motion", v, 3); eye.mw = v[0]; eye.mh = v[1];
            size_t a = start;
            Floats(text, a, "jitter", eye.jitter, 2);
            Floats(text, a, "motionScale", eye.motionScale, 2);
            Floats(text, a, "position", eye.position, 3);
            Floats(text, a, "up", eye.up, 3);
            Floats(text, a, "right", eye.right, 3);
            Floats(text, a, "forward", eye.forward, 3);
            float planes[4]{};
            Floats(text, a, "planes", planes, 4);
            eye.nearPlane = planes[0]; eye.farPlane = planes[1];
            eye.inverted = IntAfter(text, start, "inverted");
            eye.infinite = IntAfter(text, start, "infinite");
            Floats(text, a, "fov", eye.fov, 4);
            char name[64];
            std::snprintf(name, sizeof(name), "\\f%d_e%d_color.raw", f, e); eye.color = ReadBytes(dir + name);
            std::snprintf(name, sizeof(name), "\\f%d_e%d_depth.raw", f, e); eye.depth = ReadBytes(dir + name);
            std::snprintf(name, sizeof(name), "\\f%d_e%d_motion.raw", f, e); eye.motion = ReadBytes(dir + name);
            if (eye.color.size() != size_t(eye.cw) * eye.ch * 4 || eye.depth.size() != size_t(eye.dw) * eye.dh * 4 ||
                eye.motion.size() != size_t(eye.mw) * eye.mh * 4) {
                std::fprintf(stderr, "frame %d eye %d: raw sizes do not match the JSON\n", f, e);
                return false;
            }
            at = start + 10;
        }
        frames.push_back(std::move(frame));
    }
    return true;
}

// ---------------------------------------------------------------- the cameras

struct Options {
    bool rightHanded = true;     // view space: x right, y up, -z forward (else +z forward)
    bool reversedDepth = true;   // projection: near -> 1, far -> 0 (else near -> 0)
    pd::FrameWarpMode mode = pd::CombinedWarping;
    pd::MVType mv = pd::Normal;
    float mvSign = 1.0f;
    bool invertDepthTexture = false;   // hand the plugin 1 - depth
    float mvScale[2] = {1.0f, 1.0f};   // extra factor on InMotionScale (x, y)
    bool history = false;              // also score the made eye's own previous real frame, unwarped
    bool debug = false;
};

// Column-major 4x4 (m[col * 4 + row]).
struct M4 {
    double m[16]{};
    double& at(int r, int c) { return m[c * 4 + r]; }
    double at(int r, int c) const { return m[c * 4 + r]; }
};
M4 Mul(const M4& a, const M4& b) {
    M4 o;
    for (int r = 0; r < 4; ++r)
        for (int c = 0; c < 4; ++c) {
            double s = 0;
            for (int k = 0; k < 4; ++k) s += a.at(r, k) * b.at(k, c);
            o.at(r, c) = s;
        }
    return o;
}
// General 4x4 inverse (cofactors).
M4 Inverse(const M4& a) {
    const double* m = a.m;
    double inv[16];
    inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
    inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
    inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
    inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
    inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
    inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
    inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
    inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
    inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
    inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
    inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
    inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
    inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
    inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
    inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
    inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
    double det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
    M4 o;
    if (std::fabs(det) < 1e-300) return o;
    det = 1.0 / det;
    for (int i = 0; i < 16; ++i) o.m[i] = inv[i] * det;
    return o;
}
pd::Mat4 ToMat(const M4& a) {
    pd::Mat4 o{};
    for (int i = 0; i < 16; ++i) o.m[i] = float(a.m[i]);
    return o;
}

// World -> view for one eye: the rows are the view axes in world terms, the origin the eye.
M4 WorldToView(const Eye& e, const Options& o) {
    const double zs = o.rightHanded ? -1.0 : 1.0;   // the view's +z: backward (rh) or forward (lh)
    const double ax[3][3] = {{e.right[0], e.right[1], e.right[2]},
                             {e.up[0], e.up[1], e.up[2]},
                             {zs * e.forward[0], zs * e.forward[1], zs * e.forward[2]}};
    M4 v;
    for (int r = 0; r < 3; ++r) {
        double t = 0;
        for (int c = 0; c < 3; ++c) {
            v.at(r, c) = ax[r][c];
            t += ax[r][c] * e.position[c];
        }
        v.at(r, 3) = -t;
    }
    v.at(3, 3) = 1;
    return v;
}

// View -> clip for the eye's window (tangents of its OpenXR fov), finite planes.
M4 ViewToClip(const Eye& e, const Options& o) {
    const double l = std::tan(e.fov[0]), r = std::tan(e.fov[1]), u = std::tan(e.fov[2]), d = std::tan(e.fov[3]);
    const double n = e.nearPlane, f = e.farPlane;
    // with the forward distance z (> 0): x_ndc = (2 x / z - (r + l)) / (r - l), likewise y; depth = A + B / z
    double A, B;
    if (o.reversedDepth) { B = n * f / (f - n); A = -n / (f - n); }
    else { B = -n * f / (f - n); A = f / (f - n); }
    const double zs = o.rightHanded ? -1.0 : 1.0;   // forward distance = zs * view z
    M4 p;
    p.at(0, 0) = 2.0 / (r - l);
    p.at(0, 2) = -(r + l) / (r - l) * zs;
    p.at(1, 1) = 2.0 / (u - d);
    p.at(1, 2) = -(u + d) / (u - d) * zs;
    p.at(2, 2) = A * zs;
    p.at(2, 3) = B;
    p.at(3, 2) = zs;
    return p;
}

void FillCamera(const Eye& src, const Eye& dst, const Options& o, pd::CameraData& c) {
    const M4 sv = WorldToView(src, o), dv = WorldToView(dst, o);
    const M4 sp = ViewToClip(src, o), dp = ViewToClip(dst, o);
    c.srcWorldToViewMatrix = ToMat(sv);
    c.srcViewToWorldMatrix = ToMat(Inverse(sv));
    c.srcViewToClipMatrix = ToMat(sp);
    c.srcClipToViewMatrix = ToMat(Inverse(sp));
    c.destWorldToViewMatrix = ToMat(dv);
    c.destViewToWorldMatrix = ToMat(Inverse(dv));
    c.destViewToClipMatrix = ToMat(dp);
    c.destClipToViewMatrix = ToMat(Inverse(dp));
    M4 id;
    for (int i = 0; i < 4; ++i) id.at(i, i) = 1;
    c.camWorldToViewMatrix = c.camViewToWorldMatrix = c.camViewToClipMatrix = c.camClipToViewMatrix = ToMat(id);
}

// ---------------------------------------------------------------- D3D12 copies

struct Upload {
    ComPtr<ID3D12Resource> buffer;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
};

void Barrier(ID3D12GraphicsCommandList* list, ID3D12Resource* r, D3D12_RESOURCE_STATES a, D3D12_RESOURCE_STATES b) {
    if (a == b) return;
    D3D12_RESOURCE_BARRIER v{};
    v.Type = D3D12_RESOURCE_BARRIER_TYPE_TRANSITION;
    v.Transition = {r, D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, a, b};
    list->ResourceBarrier(1, &v);
}

bool StageUpload(ID3D12Device* device, ID3D12Resource* target, const uint8_t* data, int width, int height, int pixelBytes,
                 Upload& up) {
    const auto desc = target->GetDesc();
    UINT rows = 0;
    UINT64 rowBytes = 0, total = 0;
    device->GetCopyableFootprints(&desc, 0, 1, 0, &up.footprint, &rows, &rowBytes, &total);
    D3D12_HEAP_PROPERTIES heap{D3D12_HEAP_TYPE_UPLOAD};
    D3D12_RESOURCE_DESC b{};
    b.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; b.Width = total; b.Height = 1; b.DepthOrArraySize = 1;
    b.MipLevels = 1; b.SampleDesc.Count = 1; b.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &b, D3D12_RESOURCE_STATE_GENERIC_READ, nullptr,
                                               IID_PPV_ARGS(&up.buffer)))) return false;
    uint8_t* map = nullptr;
    up.buffer->Map(0, nullptr, reinterpret_cast<void**>(&map));
    const int copyRows = std::min<int>(height, int(rows));
    const size_t rowCopy = std::min<size_t>(size_t(width) * pixelBytes, size_t(rowBytes));
    for (int y = 0; y < copyRows; ++y)
        std::memcpy(map + up.footprint.Offset + size_t(y) * up.footprint.Footprint.RowPitch, data + size_t(y) * width * pixelBytes, rowCopy);
    up.buffer->Unmap(0, nullptr);
    return true;
}

void RecordUpload(ID3D12GraphicsCommandList* list, ID3D12Resource* target, D3D12_RESOURCE_STATES state, const Upload& up) {
    Barrier(list, target, state, D3D12_RESOURCE_STATE_COPY_DEST);
    D3D12_TEXTURE_COPY_LOCATION to{target, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    to.SubresourceIndex = 0;
    D3D12_TEXTURE_COPY_LOCATION from{up.buffer.Get(), D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT};
    from.PlacedFootprint = up.footprint;
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    Barrier(list, target, D3D12_RESOURCE_STATE_COPY_DEST, state);
}

struct Readback {
    ComPtr<ID3D12Resource> buffer;
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT footprint{};
    UINT rows = 0;
    UINT64 rowBytes = 0;
};
bool RecordReadback(ID3D12Device* device, ID3D12GraphicsCommandList* list, ID3D12Resource* source,
                    D3D12_RESOURCE_STATES state, Readback& rb) {
    const auto desc = source->GetDesc();
    UINT64 total = 0;
    device->GetCopyableFootprints(&desc, 0, 1, 0, &rb.footprint, &rb.rows, &rb.rowBytes, &total);
    D3D12_HEAP_PROPERTIES heap{D3D12_HEAP_TYPE_READBACK};
    D3D12_RESOURCE_DESC b{};
    b.Dimension = D3D12_RESOURCE_DIMENSION_BUFFER; b.Width = total; b.Height = 1; b.DepthOrArraySize = 1;
    b.MipLevels = 1; b.SampleDesc.Count = 1; b.Layout = D3D12_TEXTURE_LAYOUT_ROW_MAJOR;
    if (FAILED(device->CreateCommittedResource(&heap, D3D12_HEAP_FLAG_NONE, &b, D3D12_RESOURCE_STATE_COPY_DEST, nullptr,
                                               IID_PPV_ARGS(&rb.buffer)))) return false;
    Barrier(list, source, state, D3D12_RESOURCE_STATE_COPY_SOURCE);
    D3D12_TEXTURE_COPY_LOCATION from{source, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX};
    from.SubresourceIndex = 0;
    D3D12_TEXTURE_COPY_LOCATION to{rb.buffer.Get(), D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT};
    to.PlacedFootprint = rb.footprint;
    list->CopyTextureRegion(&to, 0, 0, 0, &from, nullptr);
    Barrier(list, source, D3D12_RESOURCE_STATE_COPY_SOURCE, state);
    return true;
}
std::vector<uint8_t> ReadPixels(const Readback& rb, int width, int height) {
    std::vector<uint8_t> out(size_t(width) * height * 4);
    uint8_t* map = nullptr;
    rb.buffer->Map(0, nullptr, reinterpret_cast<void**>(&map));
    for (int y = 0; y < height && y < int(rb.rows); ++y)
        std::memcpy(out.data() + size_t(y) * width * 4, map + rb.footprint.Offset + size_t(y) * rb.footprint.Footprint.RowPitch,
                    std::min<size_t>(size_t(width) * 4, size_t(rb.rowBytes)));
    D3D12_RANGE none{};
    rb.buffer->Unmap(0, &none);
    return out;
}

// PSNR of RGB, over the whole picture and over its middle 80%.
void Score(const std::vector<uint8_t>& a, const std::vector<uint8_t>& b, int w, int h, double& all, double& mid) {
    double s = 0, sm = 0;
    size_t n = 0, nm = 0;
    const int x0 = w / 10, x1 = w - w / 10, y0 = h / 10, y1 = h - h / 10;
    for (int y = 0; y < h; ++y)
        for (int x = 0; x < w; ++x) {
            const size_t i = (size_t(y) * w + x) * 4;
            double e = 0;
            for (int c = 0; c < 3; ++c) {
                const double d = double(a[i + c]) - double(b[i + c]);
                e += d * d;
            }
            s += e; n += 3;
            if (x >= x0 && x < x1 && y >= y0 && y < y1) { sm += e; nm += 3; }
        }
    auto psnr = [](double sum, size_t count) { const double mse = sum / double(count); return mse <= 0 ? 99.0 : 10.0 * std::log10(255.0 * 255.0 / mse); };
    all = psnr(s, n);
    mid = psnr(sm, nm);
}

void WriteRaw(const std::string& path, const std::vector<uint8_t>& data) {
    std::ofstream f(path, std::ios::binary);
    f.write(reinterpret_cast<const char*>(data.data()), std::streamsize(data.size()));
}

}  // namespace

int main(int argc, char** argv) {
    if (argc < 4) {
        std::fprintf(stderr, "usage: afw_replay PLUGIN.dll CAPTURE_DIR OUT_DIR [options]\n");
        return 1;
    }
    Options o;
    for (int i = 4; i < argc; ++i) {
        const std::string a = argv[i];
        const std::string v = i + 1 < argc ? argv[i + 1] : "";
        if (a == "--view") { o.rightHanded = v != "lh"; ++i; }
        else if (a == "--depth") { o.reversedDepth = v != "standard"; ++i; }
        else if (a == "--mode") { o.mode = v == "alternate" ? pd::AlternateEyeWarping : v == "previous" ? pd::PreviousFrameWarping : pd::CombinedWarping; ++i; }
        else if (a == "--mv") { o.mv = v == "other" ? pd::FromOtherEye : pd::Normal; ++i; }
        else if (a == "--mvsign") { o.mvSign = float(std::atof(v.c_str())); ++i; }
        else if (a == "--debug") o.debug = true;
        else if (a == "--invertdepth") o.invertDepthTexture = true;
        else if (a == "--mvscale") { o.mvScale[0] = o.mvScale[1] = float(std::atof(v.c_str())); ++i; }
        else if (a == "--mvscalex") { o.mvScale[0] = float(std::atof(v.c_str())); ++i; }
        else if (a == "--mvscaley") { o.mvScale[1] = float(std::atof(v.c_str())); ++i; }
        else if (a == "--history") o.history = true;
    }
    std::vector<Frame> frames;
    if (!LoadCapture(argv[2], frames)) { std::fprintf(stderr, "cannot read the capture %s\n", argv[2]); return 2; }
    const Eye& e00 = frames[0].eye[0];

    HMODULE dll = LoadLibraryA(argv[1]);
    if (!dll) { std::fprintf(stderr, "cannot load %s (%lu)\n", argv[1], GetLastError()); return 3; }
    auto initDevice = reinterpret_cast<pd::InitDeviceFn>(GetProcAddress(dll, "InitDevice"));
    auto initWarp = reinterpret_cast<pd::InitFrameWarpFn>(GetProcAddress(dll, "InitFrameWarp"));
    auto evaluate = reinterpret_cast<pd::EvaluateFrameWarpFn>(GetProcAddress(dll, "EvaluateFrameWarp"));
    if (!initDevice || !initWarp || !evaluate) { std::fprintf(stderr, "the plugin lacks its exports\n"); return 3; }

    ComPtr<IDXGIFactory6> factory;
    CreateDXGIFactory2(0, IID_PPV_ARGS(&factory));
    ComPtr<IDXGIAdapter1> adapter;
    factory->EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE, IID_PPV_ARGS(&adapter));
    ComPtr<ID3D12Device> device;
    if (FAILED(D3D12CreateDevice(adapter.Get(), D3D_FEATURE_LEVEL_12_0, IID_PPV_ARGS(&device)))) { std::fprintf(stderr, "no D3D12 device\n"); return 4; }
    D3D12_COMMAND_QUEUE_DESC qd{D3D12_COMMAND_LIST_TYPE_DIRECT};
    ComPtr<ID3D12CommandQueue> queue;
    device->CreateCommandQueue(&qd, IID_PPV_ARGS(&queue));

    pd::DeviceParams dp{};
    dp.d3d12Device = device.Get();
    dp.d3d12Queue = queue.Get();
    dp.dxgiAdapter = adapter.Get();
    pd::D3D12RendererAPI* api = initDevice(dp);
    if (!api) { std::fprintf(stderr, "InitDevice failed\n"); return 5; }
    pd::FrameWarpInitParams ip{};
    ip.hmdWidth = e00.cw;
    ip.hmdHeight = e00.ch;
    ip.eyeFormat = DXGI_FORMAT_R8G8B8A8_UNORM;
    ip.backbufferFormat = DXGI_FORMAT_R8G8B8A8_UNORM;
    pd::EyeFrameBuffers eyes = initWarp(ip);
    for (int e = 0; e < 2; ++e) {
        const auto& c = eyes.eyeFrameBuffers[e].color;
        if (!c.pTexture) { std::fprintf(stderr, "InitFrameWarp gave no colour for eye %d\n", e); return 5; }
        const auto d = c.pTexture->GetDesc();
        std::printf("eye %d colour %llux%u fmt %d state 0x%x\n", e, (unsigned long long)d.Width, d.Height, int(d.Format), unsigned(c.initialState));
    }
    // The rendered eye's depth and motion, one pair per eye, kept across frames as UEVR keeps them.
    pd::TextureDesc depth[2], motion[2];
    for (int e = 0; e < 2; ++e) {
        if (!api->CreateTexture(e00.dw, e00.dh, DXGI_FORMAT_R32_FLOAT, D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE, depth[e], true) ||
            !api->CreateTexture(e00.mw, e00.mh, DXGI_FORMAT_R16G16_FLOAT, D3D12_RESOURCE_STATE_ALL_SHADER_RESOURCE, motion[e], true)) {
            std::fprintf(stderr, "CreateTexture failed\n");
            return 5;
        }
    }

    const std::string out = argv[3];
    CreateDirectoryA(out.c_str(), nullptr);
    std::printf("view %s, depth %s%s, mode %d, mv %d, mvsign %.0f\n", o.rightHanded ? "rh" : "lh", o.reversedDepth ? "reversed" : "standard",
                o.invertDepthTexture ? " (texture 1-d)" : "", int(o.mode), int(o.mv), o.mvSign);
    double sumAll = 0, sumMid = 0;
    for (int t = 0; t < 3; ++t) {
        const int n = (t % 2 == 0) ? 0 : 1, other = 1 - n;
        const Eye& src = frames[t].eye[n];
        const Eye& dst = frames[t].eye[other];
        // motion vectors flipped by option (an MV sign test)
        std::vector<uint8_t> mv = src.motion;
        if (o.mvSign < 0) {
            auto* h = reinterpret_cast<uint16_t*>(mv.data());
            for (size_t i = 0; i < mv.size() / 2; ++i) h[i] ^= 0x8000u;
        }
        std::vector<uint8_t> dz = src.depth;
        if (o.invertDepthTexture) {
            auto* f = reinterpret_cast<float*>(dz.data());
            for (size_t i = 0; i < dz.size() / 4; ++i) f[i] = 1.0f - f[i];
        }
        Upload uc, ud, um;
        pd::TextureDesc& color = eyes.eyeFrameBuffers[n].color;
        if (!StageUpload(device.Get(), color.pTexture, src.color.data(), src.cw, src.ch, 4, uc) ||
            !StageUpload(device.Get(), depth[n].pTexture, dz.data(), src.dw, src.dh, 4, ud) ||
            !StageUpload(device.Get(), motion[n].pTexture, mv.data(), src.mw, src.mh, 4, um)) {
            std::fprintf(stderr, "upload failed\n");
            return 6;
        }
        pd::CameraData camera{};
        FillCamera(src, dst, o, camera);
        ID3D12GraphicsCommandList* list = api->BeginCommandList(0);
        RecordUpload(list, color.pTexture, color.initialState, uc);
        RecordUpload(list, depth[n].pTexture, depth[n].initialState, ud);
        RecordUpload(list, motion[n].pTexture, motion[n].initialState, um);
        pd::FrameBufferDesc in;
        in.color = color;
        in.depth = depth[n];
        in.motionVectors = motion[n];
        pd::FrameWarpEvaluateParams p{};
        p.InCmdList = list;
        p.InEyeFrameBuffer = &in;
        p.IsHudlessColor = true;
        p.MotionVectorsType = o.mv;
        p.InMotionScale[0] = src.motionScale[0] * float(src.cw) * o.mvScale[0];
        p.InMotionScale[1] = src.motionScale[1] * float(src.ch) * o.mvScale[1];
        p.Mode = o.mode;
        p.EyeIndex = pd::EyeIndex(n);
        p.CameraData = &camera;
        p.Debug = o.debug;
        evaluate(p);
        api->EndCommandList(0);
        api->WaitIdle(0);
        Readback rb;
        ID3D12GraphicsCommandList* rl = api->BeginCommandList(1);
        const pd::TextureDesc& made = eyes.eyeFrameBuffers[other].color;
        RecordReadback(device.Get(), rl, made.pTexture, made.initialState, rb);
        api->EndCommandList(1);
        api->WaitIdle(1);
        const auto pixels = ReadPixels(rb, dst.cw, dst.ch);
        double all = 0, mid = 0, keepAll = 0, keepMid = 0;
        Score(pixels, dst.color, dst.cw, dst.ch, all, mid);
        // what showing the rendered eye to both eyes would score (the floor any warp must beat)
        Score(src.color, dst.color, dst.cw, dst.ch, keepAll, keepMid);
        std::printf("step %d: eye %d -> eye %d  PSNR %.2f dB (middle %.2f)   [same picture both eyes: %.2f / %.2f]\n", t, n,
                    other, all, mid, keepAll, keepMid);
        if (t > 0) { sumAll += all; sumMid += mid; }
        if (o.history && t > 0) {
            double hAll = 0, hMid = 0;
            Score(frames[t - 1].eye[other].color, dst.color, dst.cw, dst.ch, hAll, hMid);
            std::printf("        the made eye's previous real frame, unwarped: %.2f / %.2f\n", hAll, hMid);
        }
        char name[64];
        std::snprintf(name, sizeof(name), "\\step%d_made_e%d.raw", t, other);
        WriteRaw(out + name, pixels);
    }
    std::printf("RESULT steps 1-2 mean PSNR %.2f dB (middle %.2f)\n", sumAll / 2, sumMid / 2);
    std::fflush(stdout);
    // The plugin owns threads and GPU objects; leave without unloading it.
    TerminateProcess(GetCurrentProcess(), 0);
    return 0;
}
