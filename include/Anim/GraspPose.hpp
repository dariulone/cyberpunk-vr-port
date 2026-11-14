#pragma once
#include <cstdint>

// The fingers closed on what a hand holds, from the thing's shape and size (src/Anim/GraspPose.cpp).
namespace cvr::anim {
// Each player pose pass: measures both palms, and writes the grasp of a hand that holds something.
void GraspFingers(uint8_t* boneBuf);
// Whether a hand (0 left, 1 right) holds a thing its fingers are closed on: from VRGraspSet to VRGraspClear.
bool GraspHolding(int side);
// A hand (0 left, 1 right) takes a thing -- `shape` 0 box, 1 sphere, 2 cylinder, 3 cone, `half` its half sizes, at
// `p`, `q` (x y z w) in the HAND bone's frame -- the way it came to it: where the thing lies for its grasp, in the
// hand bone's frame, to `seatP`, `seatQ`, the grasp's kind to `kind`; its fingers close on it from the next pass, at
// the blend GraspEase gives. False when the palm is not measured yet.
// THE WHOLE OF A ROUND THING, for the fingers to rest on where it is not the part the palm is put to (a bottle's shoulder
// under the little finger of a hand round its neck): its radius round the part's own axis at `knots` heights along it
// (ascending, in the part's frame), the solid that profile makes turned round the axis.
struct GraspLathe { int knots = 0; float z[18]{}, r[18]{}; };
// The part of a round long thing a hand closes on (GraspPartOf): a cylinder on the thing's axis at `z`, its half length and
// radius, the ends it may be taken by (as GraspTake's), whether it is the neck, and the whole thing round it.
struct GraspPart { float z = 0.f, half = 0.f, radius = 0.f; int ends = 15; bool neck = false; GraspLathe lathe; };
// `ends` (bits: its -Z end may be taken from above, its +Z end, its -Z end from below, its +Z end) and `part` (a part of
// something bigger, wrapped however thin: GraspPartOf) are a cylinder's; `lathe`, when given, is what the fingers rest on.
bool GraspTake(int side, int shape, const float* half, const float* p, const float* q, float* seatP, float* seatQ,
               int* kind, int ends = 15, bool part = false, const GraspLathe* lathe = nullptr);
// The palm's skin centre in the hand bone's frame (as the last pass measured it). False when not measured yet.
bool GraspPalm(int side, float* skin);
// The part of a round long thing (its hull's `n` points in its own frame, the axis Z through the origin, `halfLength` along
// it) that a hand at `hand` (that frame) closes on, when it has a neck. False: no neck, the whole thing (whose lathe, in
// its own frame, is still given when its profile could be read: out.lathe.knots > 0).
bool GraspPartOf(const float* points, int n, float halfLength, const float* hand, GraspPart& out);
// How far the fingers are on the grasp: 0 tracking, 1 the grasp.
void GraspEase(int side, float blend);
// The hand holds nothing: its fingers are the tracking's again.
void GraspLetGo(int side);
// A PHYSICS HAND HOLDS A PERSON'S PART (any thread; false: not now, the pose pass busy or a thing held): the part's
// capsule -- its segment along its own Z, `halfLength` each way, `radius` -- at `p`, `q` (x y z w) in the HAND bone's
// frame, closed on by `style`: 1 a can, 2 a can with the fingers wide, 3 gently, 4 the palm. As the hold is taken the
// part is SEATED for that grasp -- `*seated` true, `seatP`, `seatQ` where the part should lie in the hand (the same frame):
// the caller moves the hand so. Given every step after: solved again only once the part has been 2.5 cm or 20 degrees off
// its seat for half a second, the fingers eased from one solve to the next. Style 0: let go, eased out.
// palmGap < 0: seated as the grasp wants it (turned for it, 6 cm at the most). palmGap >= 0: the caller brings the part to
// its seat however far that is -- held round (styles 1-3) as a can, across the palm before the knuckles with the fingers
// round it; under the palm (4) along the palm's normal, its surface palmGap off the palm's skin, its way and its place
// along it kept.
bool GraspPerson(int side, int style, const float* p, const float* q, float halfLength, float radius, float* seatP = nullptr,
                 float* seatQ = nullptr, bool* seated = nullptr, float palmGap = -1.0f);
}
