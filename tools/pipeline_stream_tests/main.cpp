// Pipeline state streams (src/Render/ViewInstancedPipeline.cpp, or its Rust port rust/crates/cvr-render/src/pipeline_stream.rs
// through the same file's forwarders): random streams -- valid ones, repeated and unknown types, truncated ones -- read,
// described, offset, edited, rebuilt with view instancing and owned; random classic descriptions turned into streams and
// owned. Every result is printed by its fields (never padding) with pointers named by the test buffer they point into.
//
//   pipeline_stream_tests trace   one line: the cases and the FNV-1a hash of every result line (cmake/ParityTrace.cmake)
//   pipeline_stream_tests dump    every result line
#include "Render/ViewInstancedPipeline.hpp"
#include <cstdarg>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <string>
#include <vector>

using namespace cvr::stereo;

#if CVR_RUST_VRIK
// cvr_rust's settings parser asks the C++ half for its subsystems; nothing here parses settings.
extern "C" bool cvr_cpp_settings_parse_subsystem(int, const char*, void*) { return false; }
#endif

namespace {
uint64_t g_state = 0x9E3779B97F4A7C15ull;
uint64_t Next() {
    g_state ^= g_state << 13;
    g_state ^= g_state >> 7;
    g_state ^= g_state << 17;
    return g_state;
}
uint32_t Below(uint32_t n) { return n ? uint32_t(Next() % n) : 0; }
bool Chance(uint32_t percent) { return Below(100) < percent; }

std::string g_out;
bool g_dump = false;
uint64_t g_hash = 0xcbf29ce484222325ull;
void Line(const std::string& s) {
    for (unsigned char c : s) {
        g_hash ^= c;
        g_hash *= 0x100000001b3ull;
    }
    g_hash ^= '\n';
    g_hash *= 0x100000001b3ull;
    if (g_dump) std::printf("%s\n", s.c_str());
}
std::string Fmt(const char* f, ...) {
    char b[512];
    va_list a;
    va_start(a, f);
    vsnprintf(b, sizeof b, f, a);
    va_end(a);
    return b;
}
uint64_t Fnv(const void* p, size_t n) {
    uint64_t h = 0xcbf29ce484222325ull;
    for (size_t i = 0; i < n; ++i) {
        h ^= static_cast<const uint8_t*>(p)[i];
        h *= 0x100000001b3ull;
    }
    return h;
}

// The buffers the streams point into: pointers print as their buffer's name and offset.
struct Arena {
    std::vector<std::vector<uint8_t>> blobs;
    std::vector<std::vector<D3D12_INPUT_ELEMENT_DESC>> inputs;
    std::vector<std::vector<D3D12_SO_DECLARATION_ENTRY>> outputs;
    std::vector<std::vector<UINT>> strides;
    std::vector<std::vector<D3D12_VIEW_INSTANCE_LOCATION>> views;
    std::vector<std::string> names;
} g_arena;
std::string Name(const void* p) {
    if (!p) return "null";
    const auto in = [&](const void* base, size_t bytes) {
        return p >= base && p < static_cast<const uint8_t*>(base) + (bytes ? bytes : 1);
    };
    for (size_t i = 0; i < g_arena.blobs.size(); ++i)
        if (in(g_arena.blobs[i].data(), g_arena.blobs[i].size()))
            return Fmt("blob%zu+%zu", i, size_t(static_cast<const uint8_t*>(p) - g_arena.blobs[i].data()));
    for (size_t i = 0; i < g_arena.inputs.size(); ++i)
        if (in(g_arena.inputs[i].data(), g_arena.inputs[i].size() * sizeof(D3D12_INPUT_ELEMENT_DESC))) return Fmt("inputs%zu", i);
    for (size_t i = 0; i < g_arena.outputs.size(); ++i)
        if (in(g_arena.outputs[i].data(), g_arena.outputs[i].size() * sizeof(D3D12_SO_DECLARATION_ENTRY))) return Fmt("so%zu", i);
    for (size_t i = 0; i < g_arena.strides.size(); ++i)
        if (in(g_arena.strides[i].data(), g_arena.strides[i].size() * sizeof(UINT))) return Fmt("strides%zu", i);
    for (size_t i = 0; i < g_arena.views.size(); ++i)
        if (in(g_arena.views[i].data(), g_arena.views[i].size() * sizeof(D3D12_VIEW_INSTANCE_LOCATION))) return Fmt("views%zu", i);
    return Fmt("ptr%llx", static_cast<unsigned long long>(reinterpret_cast<uintptr_t>(p)));
}
const char* NewName() {
    static const char* pool[] = {"POSITION", "TEXCOORD", "NORMAL", "TANGENT", "COLOR", "SV_Position", "BLENDWEIGHT"};
    return pool[Below(7)];
}
D3D12_SHADER_BYTECODE Shader(bool allowEmpty = true) {
    if (allowEmpty && Chance(15)) return {};
    auto& blob = g_arena.blobs.emplace_back(4 + Below(60));
    for (auto& b : blob) b = uint8_t(Next());
    return {blob.data(), blob.size()};
}
D3D12_INPUT_LAYOUT_DESC Input() {
    if (Chance(15)) return {};
    auto& v = g_arena.inputs.emplace_back(1 + Below(5));
    for (auto& e : v) {
        e.SemanticName = Chance(10) ? nullptr : NewName();
        e.SemanticIndex = Below(4);
        e.Format = DXGI_FORMAT(Below(100));
        e.InputSlot = Below(16);
        e.AlignedByteOffset = Below(64);
        e.InputSlotClass = D3D12_INPUT_CLASSIFICATION(Below(2));
        e.InstanceDataStepRate = Below(3);
    }
    return {v.data(), UINT(v.size())};
}
D3D12_STREAM_OUTPUT_DESC StreamOutput() {
    D3D12_STREAM_OUTPUT_DESC d{};
    if (Chance(50)) return d;
    auto& o = g_arena.outputs.emplace_back(1 + Below(3));
    for (auto& e : o) {
        e.Stream = Below(4);
        e.SemanticName = Chance(10) ? nullptr : NewName();
        e.SemanticIndex = Below(3);
        e.StartComponent = BYTE(Below(4));
        e.ComponentCount = BYTE(Below(5));
        e.OutputSlot = BYTE(Below(4));
    }
    d.pSODeclaration = o.data();
    d.NumEntries = UINT(o.size());
    if (Chance(70)) {
        auto& s = g_arena.strides.emplace_back(1 + Below(3));
        for (auto& x : s) x = Below(256);
        d.pBufferStrides = s.data();
        d.NumStrides = UINT(s.size());
    }
    d.RasterizedStream = Below(4);
    return d;
}
D3D12_VIEW_INSTANCING_DESC Views() {
    D3D12_VIEW_INSTANCING_DESC d{};
    auto& v = g_arena.views.emplace_back(1 + Below(4));
    for (auto& x : v) x = {Below(4), Below(4)};
    d.ViewInstanceCount = UINT(v.size());
    d.pViewInstanceLocations = Chance(85) ? v.data() : nullptr;
    d.Flags = D3D12_VIEW_INSTANCING_FLAGS(Below(2));
    return d;
}
void RandomBytes(void* p, size_t n) {
    for (size_t i = 0; i < n; ++i) static_cast<uint8_t*>(p)[i] = uint8_t(Next());
}

// The ABI, written out independently of either implementation.
size_t Align(size_t n, size_t a) { return (n + a - 1) & ~(a - 1); }
template <class T> std::pair<size_t, size_t> L() {
    const size_t payload = Align(4, alignof(T));
    return {payload, Align(payload + sizeof(T), 8)};
}
std::pair<size_t, size_t> PartLayout(int t) {
    switch (t) {
    case 0: return L<void*>();
    case 1: case 2: case 3: case 4: case 5: case 6: case 24: case 25: return L<D3D12_SHADER_BYTECODE>();
    case 7: return L<D3D12_STREAM_OUTPUT_DESC>();
    case 8: return L<D3D12_BLEND_DESC>();
    case 9: case 18: return L<UINT>();
    case 10: return L<D3D12_RASTERIZER_DESC>();
    case 11: return L<D3D12_DEPTH_STENCIL_DESC>();
    case 12: return L<D3D12_INPUT_LAYOUT_DESC>();
    case 13: case 14: case 16: case 20: return L<UINT>();
    case 15: return L<D3D12_RT_FORMAT_ARRAY>();
    case 17: return L<DXGI_SAMPLE_DESC>();
    case 19: return L<D3D12_CACHED_PIPELINE_STATE>();
    case 21: return L<D3D12_DEPTH_STENCIL_DESC1>();
    case 22: return L<D3D12_VIEW_INSTANCING_DESC>();
    default: return {8, 16};   // an unknown type: some bytes after it
    }
}
// A payload's fields as text (padding never read).
std::string Payload(int t, const uint8_t* v) {
    switch (t) {
    case 0: { void* p; std::memcpy(&p, v, 8); return Fmt("root %s", Name(p).c_str()); }
    case 1: case 2: case 3: case 4: case 5: case 6: case 24: case 25: case 19: {
        D3D12_SHADER_BYTECODE s; std::memcpy(&s, v, sizeof s);
        return Fmt("code %s %zu", Name(s.pShaderBytecode).c_str(), s.BytecodeLength);
    }
    case 7: {
        D3D12_STREAM_OUTPUT_DESC d; std::memcpy(&d, v, sizeof d);
        return Fmt("so %s %u %s %u %u", Name(d.pSODeclaration).c_str(), d.NumEntries, Name(d.pBufferStrides).c_str(), d.NumStrides, d.RasterizedStream);
    }
    case 11: {
        D3D12_DEPTH_STENCIL_DESC d; std::memcpy(&d, v, sizeof d);
        return Fmt("ds %d %d %d %d %u %u %llx %llx", d.DepthEnable, int(d.DepthWriteMask), int(d.DepthFunc), d.StencilEnable, d.StencilReadMask,
                   d.StencilWriteMask, static_cast<unsigned long long>(Fnv(&d.FrontFace, sizeof d.FrontFace)),
                   static_cast<unsigned long long>(Fnv(&d.BackFace, sizeof d.BackFace)));
    }
    case 21: {
        D3D12_DEPTH_STENCIL_DESC1 d; std::memcpy(&d, v, sizeof d);
        return Fmt("ds1 %d %d %d %d %u %u %llx %llx %d", d.DepthEnable, int(d.DepthWriteMask), int(d.DepthFunc), d.StencilEnable, d.StencilReadMask,
                   d.StencilWriteMask, static_cast<unsigned long long>(Fnv(&d.FrontFace, sizeof d.FrontFace)),
                   static_cast<unsigned long long>(Fnv(&d.BackFace, sizeof d.BackFace)), d.DepthBoundsTestEnable);
    }
    case 12: {
        D3D12_INPUT_LAYOUT_DESC d; std::memcpy(&d, v, sizeof d);
        return Fmt("il %s %u", Name(d.pInputElementDescs).c_str(), d.NumElements);
    }
    case 22: {
        D3D12_VIEW_INSTANCING_DESC d; std::memcpy(&d, v, sizeof d);
        return Fmt("vi %u %s %d", d.ViewInstanceCount, Name(d.pViewInstanceLocations).c_str(), int(d.Flags));
    }
    default: {
        const auto size = PartLayout(t).second - PartLayout(t).first;
        size_t bytes = size;
        switch (t) {
        case 8: bytes = sizeof(D3D12_BLEND_DESC); break;
        case 10: bytes = sizeof(D3D12_RASTERIZER_DESC); break;
        case 15: bytes = sizeof(D3D12_RT_FORMAT_ARRAY); break;
        case 17: bytes = sizeof(DXGI_SAMPLE_DESC); break;
        case 9: case 13: case 14: case 16: case 18: case 20: bytes = 4; break;
        default: break;
        }
        return Fmt("raw %llx", static_cast<unsigned long long>(Fnv(v, bytes)));
    }
    }
}
// A stream read by the test's own decoder: each part's type, offset and payload fields.
std::string Decode(const uint8_t* bytes, size_t size) {
    std::string out;
    size_t at = 0;
    while (at + 4 <= size) {
        int t;
        std::memcpy(&t, bytes + at, 4);
        const auto [payload, total] = PartLayout(t);
        if (at + total > size) { out += Fmt(" |cut %d@%zu", t, at); break; }
        out += Fmt(" |%d@%zu %s", t, at, Payload(t, bytes + at + payload).c_str());
        at += total;
    }
    if (at != size) out += Fmt(" |tail %zu", size - at);
    return out;
}

struct Stream {
    std::vector<uint8_t> bytes;
};
void Put(Stream& s, int t) {
    const auto [payload, total] = PartLayout(t);
    const size_t at = s.bytes.size();
    s.bytes.resize(at + total);
    RandomBytes(s.bytes.data() + at, total);   // the padding too: every byte of the stream is defined
    std::memcpy(s.bytes.data() + at, &t, 4);
    uint8_t* v = s.bytes.data() + at + payload;
    switch (t) {
    case 0: { void* p = Chance(20) ? nullptr : reinterpret_cast<void*>(uintptr_t(0x10000 * (1 + Below(64)))); std::memcpy(v, &p, 8); break; }
    case 1: case 2: case 3: case 4: case 5: case 6: case 24: case 25: {
        auto c = Shader(t != 1 || Chance(10)); std::memcpy(v, &c, sizeof c); break;
    }
    case 19: { auto c = Shader(); std::memcpy(v, &c, sizeof c); break; }
    case 7: { auto d = StreamOutput(); std::memcpy(v, &d, sizeof d); break; }
    case 12: { auto d = Input(); std::memcpy(v, &d, sizeof d); break; }
    case 22: { auto d = Views(); std::memcpy(v, &d, sizeof d); break; }
    case 15: { D3D12_RT_FORMAT_ARRAY a; RandomBytes(&a, sizeof a); a.NumRenderTargets = Chance(90) ? Below(9) : 9 + Below(4); std::memcpy(v, &a, sizeof a); break; }
    default: break;   // random payload
    }
}
Stream RandomStream() {
    static const int pool[] = {0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 24, 25};
    Stream s;
    const uint32_t parts = 1 + Below(12);
    bool used[32]{};
    if (Chance(85)) { Put(s, 1); used[1] = true; }
    for (uint32_t i = 0; i < parts; ++i) {
        int t = pool[Below(sizeof pool / sizeof *pool)];
        if (Chance(3)) t = Chance(50) ? 23 : (Chance(50) ? 30 + int(Below(40)) : -1);
        if (t >= 0 && t < 32 && used[t] && !Chance(5)) continue;
        if (t >= 0 && t < 32) used[t] = true;
        if (t == 22 && !Chance(30)) continue;
        Put(s, t);
    }
    if (Chance(5) && s.bytes.size() > 4) s.bytes.resize(4 + Below(uint32_t(s.bytes.size() - 4)));   // truncated
    return s;
}

std::string Info(const PipelineStreamInfo& i) {
    return Fmt("vs %s %zu ps %s %zu root %s rt %llx dsv %d blend %llx ds %d %d %d %d %u %u samples %u/%u il %s %u flags %d%d%d%d%d%d forced %u topo %d f %d",
               Name(i.vs.pShaderBytecode).c_str(), i.vs.BytecodeLength, Name(i.ps.pShaderBytecode).c_str(), i.ps.BytecodeLength,
               Name(i.root).c_str(), static_cast<unsigned long long>(Fnv(&i.targets, sizeof i.targets)), int(i.depthFormat),
               static_cast<unsigned long long>(Fnv(&i.blend, sizeof i.blend)), i.depth.DepthEnable, int(i.depth.DepthWriteMask),
               int(i.depth.DepthFunc), i.depth.StencilEnable, i.depth.StencilReadMask, i.depth.StencilWriteMask, i.samples.Count,
               i.samples.Quality, Name(i.input.pInputElementDescs).c_str(), i.input.NumElements, i.haveBlend, i.haveDepth, i.depthBounds,
               i.otherStages, i.viewInstancing, i.streamOutput, i.forcedSamples, int(i.topology), int(i.flags));
}
std::string Offsets(const PipelineStreamOffsets& o) {
    const auto s = [](size_t v) { return v == SIZE_MAX ? std::string("-") : std::to_string(v); };
    return "vs " + s(o.vs) + " ps " + s(o.ps) + " cs " + s(o.cs) + " cached " + s(o.cached) + " ms " + s(o.ms) + " as " + s(o.as);
}
// An owned stream: its parts with what they point at, by content (the owned copies live elsewhere).
std::string Owned(const std::vector<uint8_t>& bytes) {
    std::string out;
    size_t at = 0;
    while (at + 4 <= bytes.size()) {
        int t;
        std::memcpy(&t, bytes.data() + at, 4);
        const auto [payload, total] = PartLayout(t);
        const uint8_t* v = bytes.data() + at + payload;
        switch (t) {
        case 1: case 2: case 3: case 4: case 5: case 6: case 24: case 25: case 19: {
            D3D12_SHADER_BYTECODE s; std::memcpy(&s, v, sizeof s);
            out += Fmt(" |%d code %d %zu %llx", t, s.pShaderBytecode != nullptr, s.BytecodeLength,
                       static_cast<unsigned long long>(s.pShaderBytecode ? Fnv(s.pShaderBytecode, s.BytecodeLength) : 0));
            break;
        }
        case 12: {
            D3D12_INPUT_LAYOUT_DESC d; std::memcpy(&d, v, sizeof d);
            out += Fmt(" |il %u", d.NumElements);
            for (UINT i = 0; i < d.NumElements && d.pInputElementDescs; ++i) {
                const auto& e = d.pInputElementDescs[i];
                out += Fmt(" [%s%u %d %u %u %d %u]", e.SemanticName ? e.SemanticName : "(null)", e.SemanticIndex, int(e.Format), e.InputSlot,
                           e.AlignedByteOffset, int(e.InputSlotClass), e.InstanceDataStepRate);
            }
            break;
        }
        case 7: {
            D3D12_STREAM_OUTPUT_DESC d; std::memcpy(&d, v, sizeof d);
            out += Fmt(" |so %u %u %u", d.NumEntries, d.NumStrides, d.RasterizedStream);
            for (UINT i = 0; i < d.NumEntries && d.pSODeclaration; ++i) {
                const auto& e = d.pSODeclaration[i];
                out += Fmt(" [%u %s%u %u %u %u]", e.Stream, e.SemanticName ? e.SemanticName : "(null)", e.SemanticIndex, e.StartComponent,
                           e.ComponentCount, e.OutputSlot);
            }
            for (UINT i = 0; i < d.NumStrides && d.pBufferStrides; ++i) out += Fmt(" s%u", d.pBufferStrides[i]);
            break;
        }
        case 22: {
            D3D12_VIEW_INSTANCING_DESC d; std::memcpy(&d, v, sizeof d);
            out += Fmt(" |vi %u %d", d.ViewInstanceCount, int(d.Flags));
            for (UINT i = 0; i < d.ViewInstanceCount && d.pViewInstanceLocations; ++i)
                out += Fmt(" (%u,%u)", d.pViewInstanceLocations[i].ViewportArrayIndex, d.pViewInstanceLocations[i].RenderTargetArrayIndex);
            break;
        }
        default: out += Fmt(" |%d %s", t, Payload(t, v).c_str()); break;
        }
        at += total;
    }
    return out;
}

D3D12_GRAPHICS_PIPELINE_STATE_DESC RandomGraphics() {
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};
    d.pRootSignature = Chance(20) ? nullptr : reinterpret_cast<ID3D12RootSignature*>(uintptr_t(0x10000 * (1 + Below(64))));
    d.VS = Shader(Chance(10));
    d.PS = Shader();
    d.DS = Chance(10) ? Shader() : D3D12_SHADER_BYTECODE{};
    d.HS = Chance(10) ? Shader() : D3D12_SHADER_BYTECODE{};
    d.GS = Chance(10) ? Shader() : D3D12_SHADER_BYTECODE{};
    d.StreamOutput = Chance(20) ? StreamOutput() : D3D12_STREAM_OUTPUT_DESC{};
    RandomBytes(&d.BlendState, sizeof d.BlendState);
    d.SampleMask = UINT(Next());
    RandomBytes(&d.RasterizerState, sizeof d.RasterizerState);
    d.DepthStencilState.DepthEnable = Below(2);
    d.DepthStencilState.DepthWriteMask = D3D12_DEPTH_WRITE_MASK(Below(2));
    d.DepthStencilState.DepthFunc = D3D12_COMPARISON_FUNC(1 + Below(8));
    d.DepthStencilState.StencilEnable = Below(2);
    d.DepthStencilState.StencilReadMask = UINT8(Next());
    d.DepthStencilState.StencilWriteMask = UINT8(Next());
    RandomBytes(&d.DepthStencilState.FrontFace, sizeof d.DepthStencilState.FrontFace);
    RandomBytes(&d.DepthStencilState.BackFace, sizeof d.DepthStencilState.BackFace);
    d.InputLayout = Input();
    d.IBStripCutValue = D3D12_INDEX_BUFFER_STRIP_CUT_VALUE(Below(3));
    d.PrimitiveTopologyType = D3D12_PRIMITIVE_TOPOLOGY_TYPE(Below(5));
    d.NumRenderTargets = Chance(95) ? Below(9) : 9;
    for (auto& f : d.RTVFormats) f = DXGI_FORMAT(Below(120));
    d.DSVFormat = DXGI_FORMAT(Below(120));
    d.SampleDesc = {1 + Below(8), Below(3)};
    d.NodeMask = Below(2);
    d.CachedPSO = Chance(30) ? D3D12_CACHED_PIPELINE_STATE{Shader(false).pShaderBytecode, 16} : D3D12_CACHED_PIPELINE_STATE{};
    d.Flags = D3D12_PIPELINE_STATE_FLAGS(Below(4));
    return d;
}

void Case(int n) {
    g_arena = {};
    Stream s = RandomStream();
    const D3D12_PIPELINE_STATE_STREAM_DESC desc{s.bytes.size(), s.bytes.empty() ? nullptr : s.bytes.data()};
    Line(Fmt("case %d size %zu:%s", n, s.bytes.size(), Decode(s.bytes.data(), s.bytes.size()).c_str()));
    PipelineStreamInfo info{};
    info.depthFormat = DXGI_FORMAT(77);   // left alone on a refusal
    const bool read = ReadPipelineStream(desc, info);
    Line(Fmt(" read %d %s", read, Info(info).c_str()));
    PipelineStreamOffsets offsets;
    offsets.ms = 12345;
    const bool found = FindPipelineStreamOffsets(desc, offsets);
    Line(Fmt(" offsets %d %s", found, Offsets(offsets).c_str()));
    Line(" describe" + DescribePipelineStream(desc));
    std::vector<int> seen;
    std::vector<uint8_t> edited{9, 9};
    const bool edit = EditPipelineStream(desc, edited, [&](D3D12_PIPELINE_STATE_SUBOBJECT_TYPE t, uint8_t* v) {
        seen.push_back(int(t));
        v[0] ^= uint8_t(0x5A + int(t));
    });
    std::string types;
    for (int t : seen) types += Fmt(" %d", t);
    // What the edit changed against the source, byte by byte (the copy's pointers are this process's addresses).
    std::string changed;
    if (edit && edited.size() == s.bytes.size())
        for (size_t i = 0; i < edited.size(); ++i)
            if (edited[i] != s.bytes[i]) changed += Fmt(" %zu^%02x", i, unsigned(edited[i] ^ s.bytes[i]));
    Line(Fmt(" edit %d%s -> %zu:%s", edit, types.c_str(), edited.size(), changed.c_str()));
    for (int b = 0; b < 3; ++b) {
        ViewInstancedLayout layout;
        layout.allowMasking = Chance(50);
        layout.locations = {{{Below(2), Below(2)}, {Below(2), Below(2)}}};
        D3D12_SHADER_BYTECODE vs = Shader(Chance(10));
        D3D12_SHADER_BYTECODE ps = Chance(40) ? Shader(false) : D3D12_SHADER_BYTECODE{};
        if (Chance(5)) ps = {nullptr, 8};
        auto* root = Chance(50) ? reinterpret_cast<ID3D12RootSignature*>(uintptr_t(0x77770000 + 0x1000 * Below(8))) : nullptr;
        std::vector<uint8_t> out{7};
        const bool built = BuildViewInstancedStream(desc, vs, layout, out, ps, root);
        // The view instancing part points at `layout`: named by its role, not its stack address.
        std::string decoded = Decode(out.data(), out.size());
        const std::string where = Name(layout.locations.data());
        for (size_t at; (at = decoded.find(where)) != std::string::npos;) decoded.replace(at, where.size(), "layout");
        Line(Fmt(" build %d vs %s ps %s root %s mask %d -> %zu:%s", built, Name(vs.pShaderBytecode).c_str(), Name(ps.pShaderBytecode).c_str(),
                 Name(root).c_str(), layout.allowMasking, out.size(), decoded.c_str()));
    }
    if (read) {
        OwnedPipelineStream owned;
        const bool own = OwnPipelineStream(desc, owned);
        Line(Fmt(" own %d %zu %s:%s", own, owned.stream.size(), Offsets(owned.offsets).c_str(), Owned(owned.stream).c_str()));
    }
    const auto g = RandomGraphics();
    std::vector<uint8_t> gs{3};
    const bool converted = GraphicsDescStream(g, gs);
    Line(Fmt(" graphics %d %zu:%s", converted, gs.size(), Decode(gs.data(), gs.size()).c_str()));
    OwnedGraphicsDesc og;
    const bool ownG = OwnGraphicsDesc(g, og);
    std::vector<uint8_t> ogs;
    const bool ogConverted = GraphicsDescStream(og.desc, ogs);
    Line(Fmt(" own graphics %d %d cached %d:%s", ownG, ogConverted, og.desc.CachedPSO.pCachedBlob != nullptr, Owned(ogs).c_str()));
}
}  // namespace

int main(int argc, char** argv) {
    const std::string mode = argc > 1 ? argv[1] : "trace";
    g_dump = mode == "dump";
    const int cases = 4000;
    for (int n = 0; n < cases; ++n) Case(n);
    if (!g_dump) std::printf("pipeline streams: %d cases, results %016llx\n", cases, static_cast<unsigned long long>(g_hash));
    return 0;
}
