// Pure-logic tests for include/Render/StageReplay.hpp (stream, decisions, list-state mirror).
#include "Render/StageReplay.hpp"
#include <cstdio>
#include <cstdlib>

using namespace cvr::stereo::stage_replay;

static int failures = 0;
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #x); ++failures; } } while (0)

static void StreamPayloadAndOverflow() {
    Stream s; s.Reserve(4, 64);
    const uint32_t words[3]{1, 2, 3};
    CHECK(s.Push({Op::Pipeline, 0, 0, 0, 0x1234}));
    CHECK(s.Commands()[0].d == NoPayload);
    CHECK(s.Push({Op::RootConstants, 0, 3, 5}, words, sizeof(words)));
    const auto& c = s.Commands()[1];
    const auto* p = reinterpret_cast<const uint32_t*>(s.Payload(c.d, 12));
    CHECK(p && p[0] == 1 && p[2] == 3);
    CHECK(s.Payload(c.d, 4096) == nullptr);
    CHECK(s.Payload(NoPayload, 1) == nullptr);
    CHECK(s.Push({Op::DrawIndexed, 0, 0, 36}));
    CHECK(s.Draws() == 1 && s.BlockingOps() == 0);
    CHECK(s.Push({Op::Barrier}));
    CHECK(s.BlockingOps() == 1);
    CHECK(!s.Push({Op::Draw}));               // fifth command: over capacity
    CHECK(s.Overflowed());
    CHECK(!s.Push({Op::Draw}));               // sticky
    s.Clear();
    CHECK(!s.Overflowed() && s.Commands().empty() && s.Draws() == 0);
    // Payload overflow is also sticky and leaves the command unrecorded.
    uint8_t big[80]{};
    CHECK(!s.Push({Op::Viewports, 0, 1}, big, sizeof(big)));
    CHECK(s.Overflowed() && s.Commands().empty());
    // Payload offsets are 8-byte aligned.
    s.Clear();
    const uint8_t three[3]{7, 8, 9};
    CHECK(s.Push({Op::Heaps, 0, 1}, three, 3));
    CHECK(s.Push({Op::Heaps, 0, 1}, three, 3));
    CHECK(s.Commands()[1].d == 8);
}

static void Categories() {
    for (uint32_t i = 0; i < OpCount; ++i) {
        const Op op = static_cast<Op>(i);
        const int kinds = int(Replayable(op)) + int(Ignored(op)) + int(Blocking(op));
        CHECK(op == Op::None ? kinds == 0 : kinds == 1);   // every real op is exactly one category
    }
    CHECK(IsDraw(Op::Indirect) && Replayable(Op::Indirect));
    CHECK(Blocking(Op::Barrier) && Blocking(Op::Targets) && Blocking(Op::Query) && Blocking(Op::Other));
    CHECK(Ignored(Op::Marker) && Ignored(Op::Copy));
}

static void Decisions() {
    Settings s{uint32_t(Mode::Replay), (1ull << 9) | (1ull << 7), 1};
    CHECK(Decide(s, 1, 0x33DDA4, 9) == Action::Capture);   // source records natively
    CHECK(Decide(s, 0, 0x33DDA4, 9) == Action::Replay);    // target replays
    CHECK(Decide(s, 0, 0x33DDA4, 8) == Action::Forward);   // stage not selected
    CHECK(Decide(s, 0, 0x332D5C, 9) == Action::Forward);   // not RenderElements
    CHECK(Decide(s, -1, 0x33DDA4, 9) == Action::Forward);  // unknown view
    CHECK(Decide(s, 0, 0x33DDA4, 64) == Action::Forward);  // out-of-mask stage id
    s.mode = uint32_t(Mode::Skip);
    CHECK(Decide(s, 0, 0x33DDA4, 7) == Action::Skip);
    CHECK(Decide(s, 1, 0x33DDA4, 7) == Action::Forward);
    s.source = 0;
    CHECK(Decide(s, 1, 0x33DDA4, 7) == Action::Skip);
    s.mode = uint32_t(Mode::Census);
    CHECK(Decide(s, 0, 0x33DDA4, 7) == Action::Capture && Decide(s, 1, 0x33DDA4, 7) == Action::Capture);
    s.mode = uint32_t(Mode::Off);
    CHECK(Decide(s, 0, 0x33DDA4, 7) == Action::Forward);
    s.mode = uint32_t(Mode::ReplayCamera); s.source = 1;
    CHECK(Decide(s, 0, 0x33DDA4, 9) == Action::Replay);
    CHECK(SameFrame(5, 5, true) && !SameFrame(5, 6, true) && !SameFrame(5, 5, false));
}

static void ShadowMirror() {
    Shadow s;
    CHECK(!s.valid);
    auto* pso = reinterpret_cast<ID3D12PipelineState*>(0x10);
    s.Reset(0x99, pso);
    CHECK(s.valid && s.psoSet && s.pso == pso && s.list == 0x99);
    s.SetRoot(reinterpret_cast<ID3D12RootSignature*>(0x20));
    s.SetView(3, Shadow::Table, 0xABC);
    const uint32_t w[2]{7, 8};
    s.SetConstants(1, 2, w, 4);
    CHECK(s.params[3].kind == Shadow::Table && s.params[3].value == 0xABC);
    CHECK(s.params[1].kind == Shadow::Constants && s.params[1].words[5] == 8 && s.params[1].wordMask == 0x30);
    s.SetRoot(reinterpret_cast<ID3D12RootSignature*>(0x30));     // a root change clears arguments
    CHECK(s.params[3].kind == Shadow::None && s.params[1].kind == Shadow::None);
    s.SetConstants(2, 4, w, 14);                                 // past the mirrored 16 words
    CHECK(s.overflow);
    s.Reset(0x99, nullptr);
    CHECK(!s.overflow && !s.psoSet);
    D3D12_VERTEX_BUFFER_VIEW v[2]{{1, 2, 3}, {4, 5, 6}};
    s.SetVertices(14, 2, v);
    CHECK(s.vertexMask == 0xC000 && s.vertices[15].BufferLocation == 4);
    s.SetVertices(15, 2, v);
    CHECK(s.overflow);
    s.Reset(1, nullptr);
    D3D12_VIEWPORT vp[1]{{0, 0, 100, 50, 0, 1}};
    s.SetViewports(1, vp);
    CHECK(s.viewportsSet && s.viewportCount == 1 && s.viewports[0].Width == 100);
}

static void TouchedOps() {
    Stream s; s.Reserve(16, 256);
    s.Push({Op::Pipeline}); s.Push({Op::RootTable, 0, 0, 2, 0x55}); s.Push({Op::IndexBuffer}); s.Push({Op::DrawIndexed});
    const Touched t = Touched::Of(s);
    CHECK(t.pso && t.params && t.index && !t.root && !t.vertices && !t.viewports && !t.heaps);
    s.Push({Op::RootSignature});
    CHECK(Touched::Of(s).root && Touched::Of(s).params);
}

static void Camera() {
    uint64_t sources[5]{10, 20, 30, 20, 40};
    CHECK(SubstituteCamera(sources, 5, 20, 21, 40, 41) == 3);
    CHECK(sources[1] == 21 && sources[3] == 21 && sources[4] == 41 && sources[0] == 10);
    uint64_t none[2]{1, 2};
    CHECK(SubstituteCamera(none, 2, 0, 5, 0, 6) == 0);    // unknown camera never matches 0
}

void ViewSubstitution() {
    ViewDescriptors from, to;
    from.d = {20, 20, 19, 19, 0, 0};
    to.d = {30, 30, 29, 29, 0, 0};
    uint64_t sources[5]{19, 20, 7, 51, 0};
    CHECK(SubstituteView(sources, 5, from, to) == 2);
    CHECK(sources[0] == 29 && sources[1] == 30 && sources[2] == 7 && sources[3] == 51 && sources[4] == 0);
    CHECK(from.Contains(19) && !from.Contains(7) && !from.Contains(0));
    to.d[ViewDescriptors::ViewVs] = to.d[ViewDescriptors::ViewPs] = 0;   // no target view buffer: cannot rebuild
    uint64_t view[2]{1, 19};
    CHECK(SubstituteView(view, 2, from, to) == -1);
    uint64_t plain[2]{1, 2};
    CHECK(SubstituteView(plain, 2, from, to) == 0);
}

int main() {
    ViewSubstitution();
    StreamPayloadAndOverflow();
    Categories();
    Decisions();
    ShadowMirror();
    TouchedOps();
    Camera();
    if (failures) { std::printf("%d failure(s)\n", failures); return 1; }
    std::printf("stage replay: all checks passed\n");
    return 0;
}
