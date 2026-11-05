#pragma once
// The body's proportions from its standing height -- what the VRIK calibration takes from the player's height.
//
// Segment lengths as fractions of the standing height H (Drillis & Contini 1966, as tabulated in Winter,
// Biomechanics and Motor Control of Human Movement, fig. 4.1): eyes 0.936 H over the floor, the shoulder joint
// 0.818 H, upper arm 0.186 H, forearm 0.146 H. The shoulder joints' half distance is the T-pose calibration's
// 0.105 of the arm span (Runtimes/OpenXRCalibration.cpp; a span is about the height): Winter's biacromial
// breadth, 0.259 H, is across the acromions, and the joint centres sit inside them.

namespace cvr::body {

constexpr float kEyeHeight = 0.936f;
constexpr float kShoulderHeight = 0.818f;
constexpr float kUpperArm = 0.186f;
constexpr float kForearm = 0.146f;
constexpr float kShoulderHalfWidth = 0.105f;
// The shoulder joints behind the eyes, in metres (the T-pose calibration's).
constexpr float kShoulderBehindEyes = 0.05f;
// The heights the setup takes: arms of 0.47 .. 0.73 m, inside what the arm-bone scaling accepts.
constexpr float kMinHeight = 1.40f;
constexpr float kMaxHeight = 2.20f;

struct Proportions {
    float height;             // metres
    float eyeHeight;          // eyes over the floor
    float shoulderBelowEyes;  // the shoulder joints under the eyes
    float shoulderHalfWidth;  // one shoulder joint from the body's middle
    float armLength;          // shoulder joint to wrist
};

inline Proportions FromHeight(float height) {
    return {height, kEyeHeight * height, (kEyeHeight - kShoulderHeight) * height, kShoulderHalfWidth * height,
            (kUpperArm + kForearm) * height};
}

// The standing height whose eyes are this high over the floor.
inline float HeightFromEyes(float eyeHeight) { return eyeHeight / kEyeHeight; }

}  // namespace cvr::body
