// Exercise production source-track mapping with magazine indices from the Lua configs.
#include "Anim/RigTrackMapping.hpp"
#include <array>
#include <cstdlib>
#include <iostream>

int main(int argc, char** argv) {
    if (argc != 2) return 2;
    const int track = std::atoi(argv[1]);
    if (track < 0 || track >= 2) return 2;
    // All seven audited magazine rigs name source 0 showMagazine and source 1 showMagazineReload.
    // Their destination slots are assigned by the pose mapping, not by the weapon config.
    for (uint32_t seated = 0; seated < 8; ++seated) {
        for (uint32_t carried = 0; carried < 8; ++carried) {
            if (seated == carried) continue;
            const uint32_t pairs[] = {0, seated, 1, carried};
            std::array<float, 8> pose;
            pose.fill(42.0f);
            pose[seated] = 1.0f;
            pose[carried] = 0.0f;
            volatile float observed[2]{}, requested[2]{};
            volatile int enabled[2]{};
            enabled[track] = 1;
            if (!cvr::anim::ApplyRigTracks(pairs, 2, 2, pose.data(), 8, observed, requested, enabled, 2)) return 3;
            if (pose[seated] != 0.0f || pose[carried] != 0.0f) {
                std::cerr << "showTrack=" << track << " leaves the seated magazine visible (source 0 -> destination "
                          << seated << "), while the ghost overlay is drawn\n";
                return 1;
            }
            if (observed[0] != 1.0f || observed[1] != 0.0f) return 4;
            for (uint32_t i = 0; i < pose.size(); ++i)
                if (i != seated && i != carried && pose[i] != 42.0f) return 5;
            // Releasing the override lets the next animation restore the seated magazine.
            enabled[track] = 0;
            pose[seated] = 1.0f;
            cvr::anim::ApplyRigTracks(pairs, 2, 2, pose.data(), 8, observed, requested, enabled, 2);
            if (pose[seated] != 1.0f) return 6;
        }
    }
    std::cout << "PASS source showMagazine hidden/restored across 56 destination mappings\n";
}
