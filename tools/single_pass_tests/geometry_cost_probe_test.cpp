#include "Render/GeometryCostProbe.hpp"
#include <cstdio>
#include <cstdlib>

using namespace cvr::stereo::geometry_cost;

static int failures = 0;
#define CHECK(x) do { if (!(x)) { std::printf("FAIL %s:%d %s\n", __FILE__, __LINE__, #x); ++failures; } } while (0)

int main() {
    constexpr uint32_t re = kRenderElementsNode;
    // Off and malformed controls never touch a draw.
    CHECK(Decide(0, 0, re, 0) == Action::Forward);
    CHECK(Decide(0x100u | 1u | kSide0, 0, re, 0) == Action::Forward);  // unknown bit (0x40 / 0x80 are the eye halves)
    CHECK(Decide(3u | kSide0 | kSide1, 0, re, 0) == Action::Forward);  // invalid action
    CHECK(Decide(kSide0 | kSide1, 0, re, 1) == Action::Forward);       // no action
    CHECK(Decide(1u, 0, re, 0) == Action::Forward);                    // no side
    // Side selection is exact.
    CHECK(Decide(1u | kSide1, 0, re, 1) == Action::EmptyScissor);
    CHECK(Decide(1u | kSide1, 0, re, 0) == Action::Forward);
    CHECK(Decide(2u | kSide0, 0, re, 0) == Action::Skip);
    CHECK(Decide(2u | kSide0, 0, re, 1) == Action::Forward);
    CHECK(Decide(2u | kSide0 | kSide1, 0, re, 1) == Action::Skip);
    CHECK(Decide(2u | kSide0 | kSide1, 0, re, -1) == Action::Forward);  // unknown view
    CHECK(Decide(2u | kSide0 | kSide1, 0, re, 2) == Action::Forward);
    // Node selection: RenderElements by default, explicit filter otherwise, unknown node never.
    CHECK(Decide(2u | kSide0, 0, 0x20C3ACu, 0) == Action::Forward);
    CHECK(Decide(2u | kSide0, 0x20C3ACu, 0x20C3ACu, 0) == Action::Skip);
    CHECK(Decide(2u | kSide0, 0x20C3ACu, re, 0) == Action::Forward);
    CHECK(Decide(2u | kSide0, 0, 0, 0) == Action::Forward);

    // Scissor restoration returns exactly the engine's rectangles for the same list only.
    ScissorState state;
    int32_t out[ScissorState::kMax][4]{};
    const int a = 1, b = 2;
    CHECK(state.Restore(&a, out) == 0);
    const int32_t two[2][4]{{1, 2, 3, 4}, {5, 6, 7, 8}};
    state.Track(&a, 2, two);
    CHECK(state.Restore(&b, out) == 0);
    CHECK(state.Restore(&a, out) == 2);
    CHECK(out[0][0] == 1 && out[0][3] == 4 && out[1][0] == 5 && out[1][3] == 8);
    state.Forget(&b);
    CHECK(state.Restore(&a, out) == 2);
    state.Forget(&a);
    CHECK(state.Restore(&a, out) == 0);
    int32_t many[40][4]{};
    for (int i = 0; i < 40; ++i) many[i][0] = i;
    state.Track(&b, 40, many);
    CHECK(state.Restore(&b, out) == ScissorState::kMax);
    CHECK(out[ScissorState::kMax - 1][0] == int32_t(ScissorState::kMax - 1));

    if (failures) return EXIT_FAILURE;
    std::printf("geometry_cost_probe: all checks passed\n");
    return EXIT_SUCCESS;
}
