#pragma once
#include <openxr/openxr.h>
#include <vector>

// FINGER TRACKING (xr_grip_from_fingers) for hand-tracked fingers: Valve Index controllers and finger-tracking gloves
// that present themselves as Index controllers.
//
// The grip action is a 0..1 value that the port's gestures (holster, scanner, magazine, ladder, wheel, throw) read as
// "gripping" from about 0.5. Gloves derive both squeeze value and force from finger curl, and no driver threshold
// separates a relaxed, comfortably bent hand from a grip: gestures fire by accident.
//
// With the switch on, grip comes from the hand skeleton instead (OpenXR XR_EXT_hand_tracking, unobstructed motion range
// where XR_EXT_hand_joints_motion_range is available): grip is ON when at least THREE of the index, middle, ring and
// little fingers are bent past kOn, OFF again when fewer than three stay above kOff. Three, not four: a glove sensor
// can read a squeezed finger as fully open for seconds, and one such finger must not block the grip. The thumb is
// ignored, so a fist and a thumbs-up both grip; a relaxed hand never does. A relaxed hand's third-highest finger reads
// up to ~0.5, a fist's 0.8 and up. Until a hand has sent finger data it keeps the normal grip; after that, pauses in
// the data keep the last state. The same curls drive V's fingers (xr_finger_bones, Anim/HandGestures.cpp).
namespace cvr::fingergrip {

constexpr float kOn  = 0.72f;   // three of the four fingers at least this bent: grip
constexpr float kOff = 0.58f;   // fewer than three above this: release

// Fingers in order index, middle, ring, little; curl 0 = straight, 1 = fully bent.
constexpr bool Update(bool gripping, const float curl[4]) {
    // The third-highest bend = the least of the best three: one finger may read open.
    float least = curl[0], second = 2.0f;
    for (int f = 1; f < 4; ++f) {
        if (curl[f] < least) { second = least; least = curl[f]; }
        else if (curl[f] < second) second = curl[f];
    }
    return gripping ? second >= kOff : second >= kOn;
}

namespace detail {
constexpr float kFist[4]    = {0.95f, 0.97f, 0.96f, 0.93f};
constexpr float kRelaxed[4] = {0.45f, 0.55f, 0.60f, 0.62f};
constexpr float kStuck[4]   = {0.95f, 0.97f, 0.96f, 0.00f};   // a fist whose little-finger sensor reads open
constexpr float kTwoOpen[4] = {0.95f, 0.97f, 0.00f, 0.00f};   // two fingers out: not a grip
constexpr float kLogFist[4] = {0.82f, 0.84f, 1.00f, 1.00f};   // a measured glove fist
constexpr float kLoosen[4]  = {0.75f, 0.80f, 0.78f, 0.70f};   // a held fist relaxing a little
static_assert(Update(false, kFist), "a fist grips (the thumb is not looked at)");
static_assert(!Update(false, kRelaxed), "a relaxed, comfortably bent hand does not grip");
static_assert(Update(false, kStuck), "one finger reading open does not block the grip");
static_assert(!Update(false, kTwoOpen), "three of the four fingers must be closed");
static_assert(Update(false, kLogFist), "a measured glove fist grips");
static_assert(Update(true, kLoosen), "a held grip survives a little loosening (hysteresis)");
static_assert(!Update(true, kRelaxed), "a relaxed hand releases");
}

// Instance creation: append the hand-tracking extensions to `enabled` when the switch is on and the runtime offers them.
void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled);
// After xrCreateSession / before xrDestroySession.
void OnSessionCreated(XrInstance instance, XrSession session);
void OnSessionDestroying();
// Per frame and hand (0 = left, 1 = right): -1 = no finger data yet (use the normal grip), else 0 or 1.
int Evaluate(int hand, XrSpace base, XrTime time);
// The bend Evaluate last measured, for V's fingers (xr_finger_bones, Anim/HandGestures.cpp): thumb, index, middle,
// ring, little; 0 = straight, 1 = fully bent. False until that hand has sent finger data; then the last values.
// Any thread.
bool TrackedCurl(int hand, float curl[5]);
// The finger spread for V's hand (xr_finger_spread): thumb-index, index-middle, middle-ring, ring-little, 0 touching ..
// 1 apart, measured from the hand joints. False when none is fresher than 250 ms. Any thread.
bool TrackedSplay(int hand, float splay[4]);
}
