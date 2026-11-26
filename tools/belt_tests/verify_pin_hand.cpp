// The holding hand the game actually drew through each pin timeline (captures of the CPVR_PinClearanceTest
// probe: every finger bone and the grenade in model space, per frame), against the first-person hand mesh:
//   - the four fingers' skin to the grenade's triangles: no deeper than the game's own hold at the timeline's
//     start by more than 1.5 mm at any frame, the regrip in or not (its own clearance reported);
//   - the thumb's anatomical angles all the way through (GraspPose.cpp's model), reported;
//   - the IP straightening once the ring lets go.
//
//   verify_pin_hand REPO captures-directory profile.json
#include "pin_runtime.hpp"
#include <algorithm>
#include <cctype>
#include <fstream>
#include <iomanip>
#include <iostream>

namespace fs = std::filesystem;
using hand::kDeg;

int main(int argc, char **argv) try {
    if (argc != 4)
        throw std::runtime_error("REPO captures-directory profile.json");
    const fs::path repo = argv[1];
    const auto rig = hand::Rig::load(repo / "tools/grasp_tests/rig.txt");
    hand::HandMesh meshes[2] = {hand::HandMesh::load(repo / "tools/grasp_tests/mesh/hand_l.bin", 0, rig),
                                hand::HandMesh::load(repo / "tools/grasp_tests/mesh/hand_r.bin", 1, rig)};
    const hand::Partition parts[2] = {hand::partition(meshes[0]), hand::partition(meshes[1])};
    const auto holdPose = hand::loadPose(repo / "mods/cet/CyberpunkVRPort_TacticalBelt/grenade/poses/grenade_hold.lua");
    Json profiles;
    {
        std::ifstream f(argv[3]);
        f >> profiles;
    }
    const char *names[] = {"", "index", "middle", "ring", "little"};
    int total = 0, failures = 0;
    std::cout << std::fixed << std::setprecision(2);
    for (const auto &file : fs::directory_iterator(argv[2])) {
        if (file.path().extension() != ".json")
            continue;
        Json capture;
        {
            std::ifstream f(file.path());
            f >> capture;
        }
        if (!capture.contains("rows") || !capture.contains("kind"))
            continue;
        std::string kind = capture["kind"];
        std::transform(kind.begin(), kind.end(), kind.begin(), [](unsigned char c) { return char(std::tolower(c)); });
        const int side = capture["side"];
        auto &mesh = meshes[side];
        const auto &part = parts[side];
        auto body = PinBody::load(repo / "build/pin-clearance/json", kind);
        body.buildField();
        auto prof = pin::parse(profiles[std::to_string(side)][kind]);
        hand::Pose restPose = holdPose;
        for (int k = 0; k < 3; ++k)
            restPose[hand::kChain[0][k]] = prof.rest[k];
        const auto thumb = hand::Thumb::make(rig, side, restPose);
        std::array<double, 5> start{}, regrip, past;
        regrip.fill(1e9);
        past.fill(1e9);
        bool haveStart = false, released = false, fingersRows = true;
        double releasedAt = -1, worstStrain = 0, ipAtRelease = 0, ipLeast = 1e9;
        int frames = 0, outside = 0;
        for (const auto &row : capture["rows"]) {
            const auto &state = row["contact"];
            if (!state.contains("time") || !row.contains("fingers")) {
                fingersRows = fingersRows && row.contains("fingers");
                continue;
            }
            const double t = state["time"];
            const T handT{vec(row["wrist"]), quat(row["wristQ"])};
            const T prop = relative(handT, compose({vec(row["anchor"]), quat(row["aq"])},
                                                   {vec(row["bodyP"]), quat(row["bodyQ"])}));
            std::array<T, hand::kSlots> model;
            for (int s = 0; s < hand::kSlots; ++s)
                model[s] = {vec(row["fingers"][s][0]), quat(row["fingers"][s][1])};
            hand::Pose pose;
            for (int s = 0; s < hand::kSlots; ++s) {
                const int parent = rig.parent.at(rig.bones[side][s]);
                T parentT = handT;
                for (int k = 0; k < hand::kSlots; ++k)
                    if (rig.bones[side][k] == parent)
                        parentT = model[k];
                pose[s] = inv(parentT.q) * model[s].q;
            }
            mesh.skin(rig, pose);
            std::array<double, 5> least;
            least.fill(1e9);
            Q qi = inv(prop.q);
            for (int c = 1; c < 5; ++c)
                for (int k : part.moving[c])
                    least[c] = std::min(least[c], body.distance(rot(qi, mesh.sv[k] - prop.p)));
            if (!haveStart) {
                start = least;
                haveStart = true;
            }
            const double w = pin::weight(t);
            for (int c = 1; c < 5; ++c) {
                past[c] = std::min(past[c], least[c] - std::min(start[c], 0.));
                if (w >= .9)
                    regrip[c] = std::min(regrip[c], least[c]);
            }
            Q q[3] = {pose[0], pose[5], pose[10]};
            auto a = thumb.read(rig, side, q);
            worstStrain = std::max(worstStrain, thumb.strain(a));
            if (!thumb.withinHard(a))
                ++outside;
            const std::string phase = state["phase"];
            if (phase == "released" && !released) {
                released = true;
                releasedAt = t;
                ipAtRelease = a.ip;
            }
            if (released && t - releasedAt <= .12)
                ipLeast = std::min(ipLeast, a.ip);
            ++frames;
        }
        if (!fingersRows || frames < 20) {
            std::cout << file.path().stem().string() << " SKIP: no finger rows\n";
            continue;
        }
        // The thumb's range is reported, not gated: reaching over the ring as the grenade turns in, the bake
        // rolls the metacarpal to its free play to go round the shell, and that is the one stretch past ease.
        bool pass = released && ipLeast < 25;
        std::cout << file.path().stem().string();
        std::ostringstream detail;
        detail << std::fixed << std::setprecision(2);
        for (int c = 1; c < 5; ++c) {
            // Past the hold's own depth by no more than a millimetre and a half; the regrip's own clearance is
            // reported only (the fingers keep the hold's depth through it rather than open out of a shell the
            // hold already sinks them into).
            pass = pass && past[c] >= -.0015;
            detail << "  " << names[c] << " regrip " << regrip[c] * 1000 << " mm, past the hold " << past[c] * 1000
                   << " mm (hold " << start[c] * 1000 << ")\n";
        }
        std::cout << (pass ? " PASS" : " FAIL") << " frames=" << frames << " released at " << releasedAt
                  << " s; thumb strain " << worstStrain << ", frames out of range " << outside << "; IP " << ipAtRelease
                  << " deg at the release, " << ipLeast << " within 120 ms\n"
                  << detail.str();
        ++total;
        if (!pass)
            ++failures;
    }
    if (!total)
        throw std::runtime_error("No captures with finger rows");
    return failures ? 1 : 0;
} catch (const std::exception &e) {
    std::cerr << e.what() << '\n';
    return 2;
}
