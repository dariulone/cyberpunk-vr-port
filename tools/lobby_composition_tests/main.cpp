#include <array>
#include <algorithm>
#include <cstdint>
#include <iostream>
#include <iterator>
#include <limits>
#include <stdexcept>
#include "Lobby/CompositionBlock.hpp"

using Block = std::array<float, 108>;
using cvr::lobby::detail::CompositionBlock;

void Check(bool ok, const char* message) {
    if (!ok) throw std::runtime_error(message);
}

Block CapturedLobby() {
    // Relevant fields from a read-only capture of game 2.5 on 2026-11-14:
    // Quest Pro / SteamVR, RX 7900 XT. Unused stack/pointer words are omitted.
    // The old fingerprint rejected all 25,339 sampled lobby frames (reason 4).
    Block b{};
    b[4] = b[8] = 1.0f;
    const float grading[]{0.0f, 1.0f, .75f, 1.0f, 0.0f, 1.527f, .3f, .8f,
                          .35f, .73f, 1.0f, .0003f, -.005f, -.0025f,
                          -.0108f, -.0068f, -.0126f, -.0073f};
    std::copy(std::begin(grading), std::end(grading), b.begin() + 16);
    std::copy_n(b.begin() + 16, 8, b.begin() + 88);
    b[66] = 4160.0f;
    b[67] = 2104.0f;
    return b;
}

int main() {
    try {
        auto captured = CapturedLobby();
        Check(CompositionBlock(captured.data(), true) == 0, "captured lobby becomes a flat SBS menu");
        Check(CompositionBlock(captured.data(), false) == 4, "short layout accepted outside DrawHUD");

        // Another frame from the same DrawHUD node: the meaningful prefix is
        // unchanged, but rows 24..26 contain stack pointers/counters/pose bytes.
        auto animated = captured;
        const uint32_t padding[]{0x890971b0, 0x121, 0xa85e8, 0, 0xfcbe35a7, 0x34cf781,
                                 0xfe559186, 0x3dae3b60, 0xbd9e502e, 0xbf28a0d3, 0x3f3e5a96, 0x7ff6};
        std::memcpy(animated.data() + 96, padding, sizeof(padding));
        Check(CompositionBlock(animated.data(), true) == 0, "changing stack padding makes the lobby flicker to a panel");
        Check(CompositionBlock(animated.data(), false) == 4, "unrelated node accepted with arbitrary tail");

        auto repeated = captured;
        std::copy_n(repeated.begin() + 16, 18, repeated.begin() + 88);
        Check(CompositionBlock(repeated.data()) == 0, "original repeated-grading layout rejected");

        for (auto valid : {captured, repeated, animated}) {
            valid[106] = std::numeric_limits<float>::quiet_NaN();
            valid[107] = std::numeric_limits<float>::infinity();
            Check(CompositionBlock(valid.data(), true) == 0, "unused row 26 padding affects recognition");
            for (int i : {4, 5, 6, 7, 8}) {
                for (float value : {std::numeric_limits<float>::denorm_min(), -1.0f, 2.0f,
                                    std::numeric_limits<float>::quiet_NaN()}) {
                    auto bad = valid; bad[i] = value;
                    Check(CompositionBlock(bad.data(), true) == 2, "non-weight shader words accepted");
                }
            }
            for (int i : {66, 67}) {
                for (float value : {0.0f, 63.0f, 16385.0f, 2104.5f,
                                    std::numeric_limits<float>::quiet_NaN(),
                                    std::numeric_limits<float>::infinity()}) {
                    auto bad = valid; bad[i] = value;
                    Check(CompositionBlock(bad.data(), true) == 3, "invalid render target accepted");
                }
            }
        }
        for (int i = 88; i < 96; ++i) {
            auto bad = captured; bad[i] += .125f;
            Check(CompositionBlock(bad.data(), true) == 4, "unrecognized partial grading layout accepted");
        }
        for (int i : {26, 27}) {
            for (float value : {-1.0f, 5.0f, std::numeric_limits<float>::quiet_NaN()}) {
                auto bad = captured; bad[i] = value;
                Check(CompositionBlock(bad.data(), true) == 5, "invalid primary grading accepted");
            }
        }
        Check(CompositionBlock(Block{}.data()) != 0, "empty shader constants accepted");
        std::cout << "PASS captured and original lobby layouts, padding, unrelated data guards\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << e.what() << '\n';
        return 1;
    }
}
