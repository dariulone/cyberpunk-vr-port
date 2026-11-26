// Verify captured native FK against the original LOD0 triangles.
#include "pin_mesh.hpp"
#include <cctype>
#include <iostream>
int main(int argc, char **argv) try {
    if (argc != 4 && argc != 5)
        throw std::runtime_error("mesh-json-directory runtime-captures-directory profiles.json [last-time]");
    std::ifstream profileInput(argv[3]);
    Json profiles;
    profileInput >> profiles;
    int total = 0, failures = 0;
    for (const auto &file : std::filesystem::directory_iterator(argv[2])) {
        if (file.path().extension() != ".json")
            continue;
        std::ifstream input(file.path());
        Json capture;
        input >> capture;
        if (!capture.contains("rows") || !capture.contains("kind"))
            continue;
        std::string kind = capture["kind"];
        std::transform(kind.begin(), kind.end(), kind.begin(),
                       [](unsigned char c) { return char(std::tolower(c)); });
        auto body = PinBody::load(argv[1], kind);
        const auto &profile = profiles[std::to_string(capture["side"].get<int>())][kind];
        V opening = vec(profile["originalCentre"]);
        V normal = rot(inv(quat(profile["pinRotation"])), unit(vec(profile["normal"])));
        double worst = 1e9, worstTime = 0, maxForce = 0, ringMargin = 1e9;
        int checked = 0, contactFrames = 0;
        bool hooked = false, released = false;
        for (const auto &row : capture["rows"]) {
            const auto &state = row["contact"];
            if (!state.contains("time"))
                continue;
            double t = state["time"];
            if (argc == 5 && t > std::stod(argv[4]))
                continue;
            T prop = compose({vec(row["anchor"]), quat(row["aq"])}, {vec(row["bodyP"]), quat(row["bodyQ"])});
            T meta{vec(row["thumbMeta"]), quat(row["thumbMetaQ"])};
            T proximal{vec(row["thumbBase"]), quat(row["thumbBaseQ"])};
            T distal{vec(row["thumb"]), quat(row["thumbQ"])};
            double clearance =
                body.clearance(prop, meta, proximal, distal, capture["side"] == 0 ? 1 : -1, true);
            if (clearance < worst) {
                worst = clearance;
                worstTime = t;
            }
            const std::string phase = state["phase"];
            hooked |= phase == "hooked" || phase == "tension";
            released |= phase == "released";
            if (phase == "hooked" || phase == "tension") {
                T ring = compose({vec(row["anchor"]), quat(row["aq"])}, {vec(row["pos"]), quat(row["rot"])});
                V centre = ring.p + rot(ring.q, opening * row["scale"].get<double>());
                V axis = unit(rot(ring.q, normal));
                V segment = rot(distal.q, vec(profile["tip"]));
                double denominator = dot(axis, segment);
                double along = dot(axis, centre - distal.p) / denominator;
                V crossing = distal.p + segment * along;
                double radius = along >= .95   ? .0062 - (along - .95) * .034
                                : along >= .85 ? .0076 - (along - .85) * .014
                                               : .0085;
                double margin = profile["radius"].get<double>() -
                                std::sqrt(dot(centre - crossing, centre - crossing)) -
                                radius * std::sqrt(dot(segment, segment)) / std::abs(denominator);
                ringMargin = std::min(ringMargin, margin);
                if (along < .6 || along > 1.02)
                    ringMargin = -1;
                contactFrames++;
            }
            maxForce = std::max(maxForce, state["force"].get<double>());
            checked++;
        }
        bool sameWeapon = capture["weapon"] == capture["finalWeapon"];
        double flightAgreement = 0, launchSpeed = 0;
        const auto &rows = capture["rows"];
        auto ringCentre = [&](const Json &row) {
            const auto &m = row["mat"];
            double scale = row["scale"];
            return vec(m[3]) + vec(m[0]) * (opening.x * scale) + vec(m[1]) * (opening.y * scale) +
                   vec(m[2]) * (opening.z * scale);
        };
        auto thumbAnchor = [&](const Json &row) {
            V point = vec(row["thumb"]) +
                      rot(quat(row["thumbQ"]), vec(profile["tip"]) * row["contact"]["along"].get<double>());
            return vec(row["player"]) + rot(quat(row["pq"]), point);
        };
        for (size_t i = 1; i + 2 < rows.size(); i++) {
            if (rows[i]["contact"]["phase"] == "released" && rows[i - 1]["contact"]["phase"] == "tension") {
                // The ring leaves with the thumb's motion up to the release; after it the thumb's tip
                // straightens (pin_contact.lua M.snap), which the ring no longer feels.
                V flight = ringCentre(rows[i + 2]) - ringCentre(rows[i]);
                V motion = thumbAnchor(rows[i]) - thumbAnchor(rows[i - 2 > 0 ? i - 2 : 0]);
                flightAgreement = dot(unit(flight), unit(motion));
                launchSpeed = std::sqrt(dot(flight, flight)) /
                              (rows[i + 2]["t"].get<double>() - rows[i]["t"].get<double>());
                break;
            }
        }
        bool pass = checked > 20 && worst >= -.0005 && hooked && released && maxForce >= 22 && sameWeapon &&
                    contactFrames > 2 && ringMargin >= -.0005 && flightAgreement > .9 && launchSpeed > 1.5;
        std::cout << file.path().stem().string() << " " << (pass ? "PASS" : "FAIL")
                  << " clearance=" << worst * 1000 << "mm at " << worstTime << "s force=" << maxForce
                  << " hooked=" << hooked << " released=" << released << " weapon=" << sameWeapon
                  << " frames=" << checked << '\n';
        std::cout << "  ring clearance " << ringMargin * 1000 << " mm across " << contactFrames
                  << " frames\n";
        total++;
        std::cout << "  release speed " << launchSpeed << " m/s, thumb alignment " << flightAgreement << "\n";
        if (!pass)
            failures++;
    }
    if (!total)
        throw std::runtime_error("No native captures");
    return failures ? 1 : 0;
} catch (const std::exception &e) {
    std::cerr << e.what() << '\n';
    return 2;
}
