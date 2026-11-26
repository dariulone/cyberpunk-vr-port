#include "Natives/ClimbSupport.hpp"
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <initializer_list>

using namespace cvr::climb_support;
static void Check(bool ok, const char* what) {
    if (!ok) { std::fprintf(stderr, "FAIL: %s\n", what); std::exit(1); }
}
struct Box {
    Vector lo, hi;
    bool TopUnderPad(Vector p) const {
        return p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y && p.z >= hi.z && p.z-hi.z < kCapRadius;
    }
    bool Touches(Vector p) const {
        const Vector closest{std::clamp(p.x, lo.x, hi.x), std::clamp(p.y, lo.y, hi.y), std::clamp(p.z, lo.z, hi.z)};
        return LengthSquared(closest-p) < kSeamRadius*kSeamRadius;
    }
};
int main() {
    // Two finite faces meeting at (10,20) with different heights at the points of contact.
    std::array<Box,2> boxes{{{{9.99f,19.f,0.f},{10.f,20.f,2.f}}, {{9.f,19.99f,0.f},{10.f,20.f,2.f}}}};
    int seams = 0;
    const auto classify = [&](std::initializer_list<Hit> hits) {
        return Classify(hits.begin(), int(hits.size()), [&](int a, int b, Vector seam) {
            ++seams;
            return boxes.at(a).Touches(seam) && boxes.at(b).Touches(seam);
        }, [&](int index, Vector pad) { return boxes.at(index).TopUnderPad(pad); });
    };
    const Hit face{{10,19.95f,1},{1,0,0},0,true};
    Hit side{{9.97f,20,1.04f},{0,1,0},1,false};
    Check(classify({face,side}) == Feature::Edge && classify({side,face}) == Feature::Edge,"split exterior corner in either order");
    boxes[1].hi.x -= .006f;
    Check(classify({face,side}) == Feature::Edge,"small cooked seam still supported");
    boxes[1].hi.x -= .020f;
    Check(classify({face,side}) == Feature::None,"disconnected side behind wall cannot supply a corner");
    side.shape = 0;
    Check(classify({face,side}) == Feature::Edge,"one-piece exterior corner");
    side.point = {10.05f,20.f,1};
    Check(classify({face,side}) == Feature::None,"one-sided behind-plane evidence rejected");
    Hit inside = face; inside.point.y = 20.04f;
    Check(classify({inside,side}) == Feature::None,"inside corner");
    Hit back{{9.95f,19.95f,1},{-1,0,0},0,true};
    Check(classify({face,back}) == Feature::Edge,"opposite faces of a narrow rail");
    back.shape = 1;
    Check(classify({face,back}) == Feature::None,"opposite faces on disconnected sheets");
    Hit top{{10,19.95f,1.02f},{0,0,1},1,true};
    Check(classify({top}) == Feature::Top,"ledge or rail top without finger contacts");
    top.close = false;
    Check(classify({face,top}) == Feature::None,"separate near top cannot borrow wall contact");
    top.shape = 0;
    Check(classify({face,top}) == Feature::Top,"rail side within contact range with top within reach");
    // No worker wrap is an input: dynamic/NPC/other-hand touches or an earlier valid
    // frame cannot add evidence to a fresh single-face (or empty) static snapshot.
    Check(classify({face}) == Feature::None && classify({}) == Feature::None,"no previous hold carried into later smooth or empty query");
    Hit parallel = face; parallel.point.y += .07f;
    Check(classify({face,parallel}) == Feature::None,"many points on a smooth wall");
    boxes[0].hi.z = 1.033f;
    Check(classify({face}) == Feature::Top,"side-only touch just below the rail's top with fingers over the lip");
    boxes[0].hi.z = 1.08f;
    Check(classify({face}) == Feature::None,"distant top not within hooking reach");
    boxes[1] = boxes[0]; boxes[1].hi.z = 1.032f;
    Check(classify({face}) == Feature::None,"neighboring rail cannot prove a cap on the wall");
    Check(seams >= 4,"two-piece fixtures did not actually query seams");
    std::puts("static climb support: tops, rails, connected split corners, inside corners, disconnected and fresh wall-only contacts PASS");
}
