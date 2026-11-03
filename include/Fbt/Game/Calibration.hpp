#pragma once
// FULL-BODY TRACKING CALIBRATION in the game (menu: "* Calibrate full-body tracking"), the shared full-body IK's
// (src/Fbt/FbtCalibration, the same as the FBT IK lab's), in three steps:
//   1 head (a pull of both triggers, after releasing them): look ahead at the horizon line (drawn; green when the gaze
//     is level, the pull counts only then) -> eye height, head reference, heading;
//   2 body (a pull): stand straight, feet as V's, arms relaxed along the body, looking down at the trackers -> scale,
//     body frame, the trackers' offsets to V's hips and feet (with step 1's head);
//   3 arms, the T-pose (no pull): a latin cross stands in front of the player, its top at the eyes, its arms at the
//     shoulders; dots show the head and the two controllers on it, as a mirror would. Once all three are on their marks
//     (green) a countdown runs; the arms are measured from the widest span between the hands over it (Calibrator::
//     MeasureArmsOut: the span minus V's shoulder width, halved -- no shoulder estimate, no torso twist). Leaving a
//     mark restarts it; cancelling keeps V's arms.
// V is held in its reference pose meanwhile. The result is logged ([fbt-calib]) and saved to fbt_calibration.ini.
// The triggers still reach the game.
#include "Fbt/FbtCalibration.hpp"
#include "Fbt/Game/Trackers.hpp"
#include <openxr/openxr.h>
#include <string>

namespace cvr::fbt::game {

using Calibration = wrpuppets::fbt::Calibration;
// In game only: not in the main menu's lobby, which has a setup of its own (the weapon hand, its T-pose, the height),
// and needs a player (a loaded save). StartCalibration refuses otherwise.
bool CalibrationAvailable();
void StartCalibration();
void CancelCalibration();
bool Calibrating();
int CalibrationStep();   // 0 off, 1 head, 2 body, 3 arms (the T-pose on the cross)
constexpr float kCalibrationLevelDeg = 8.0f;   // step 1: the gaze within this of level
// Each frame, from the controller loop: the raw trigger value per hand (0 left, 1 right).
void NoteTrigger(int hand, float value);
// The calibration captured in this session (none until then: the belt and the foot straps move between sessions,
// so a saved one would be wrong; fbt_calibration.ini is only a record).
bool ReadCalibration(Calibration* out);
// V's skeleton (from the rig file) and the calibrator on it; false until V's rig has loaded. Render/pose threads:
// the objects are replaced only when V's body type changes.
bool Rig(const wrpuppets::fbt::Skeleton** skeleton, const wrpuppets::fbt::Calibrator** calibrator);
// STAGE -> V's model axes (x right, y forward, z up), as the lab's VrSession maps it.
wrpuppets::fbt::RoomPose ToRoom(bool valid, const XrPosef& pose);
// Step 3's cross and dots (STAGE, metres: x right, y up, -z forward), for the helpers to draw; false outside step 3.
struct CrossView {
    XrVector3f base{};                        // the cross's foot, on the floor in front of the player
    XrVector3f right{};                       // its right axis as the player sees it (horizontal, unit)
    float tipY = 0, barY = 0, halfSpan = 0;   // its top (the eyes) and arms (the shoulders) over the floor; the arms' half length
    XrVector3f dot[3]{};                      // the head, the left hand, the right hand laid on the cross's plane
    int state[3]{-1, -1, -1};                 // -1 not tracked, 0 off its mark, 1 near, 2 on it
    float countdown = -1;                     // seconds left while all three hold, -1 otherwise
};
bool ReadCross(CrossView* out);
constexpr float kCrossHoldSeconds = 4.0f;

// The texts shown in the view, in the menu's language: the CET menu sets them (VRFbtSetCalibrationText) from its
// translation tables; English until it does. Slots 1..3 the steps' instructions (4 unused), 5..7 why a step was not taken.
constexpr int kRefuseLevel = 5, kRefuseArm = 6, kRefuseTracking = 7, kStepTextSlots = 8;
void SetStepText(int slot, const char* text);
std::string StepText(int slot);
// Why the last pull was not taken, for 3 s after it ("" otherwise): drawn under the step's instruction.
std::string RefusalText();

// From Trackers.cpp, after each locate while calibrating: the devices this frame (stage: the STAGE space they and the
// head are located in).
void CalibrationTick(XrSpace view, XrSpace stage, XrTime time, const cvr::trackers::Pose poses[cvr::trackers::kRoleCount],
                     const cvr::trackers::Pose hands[2]);

}  // namespace cvr::fbt::game
