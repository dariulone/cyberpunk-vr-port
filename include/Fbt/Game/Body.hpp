#pragma once
// FULL-BODY TRACKING ON V's BODY, in the player's pose pass (Hooks/AnimPose.cpp, right after the port places the body
// under the headset, on foot): the shared IK stack (src/Fbt/FbtIk) on the game's bone buffer, with the goals from the
// trackers through the calibration.
//
// Anchoring (the goals into the game's model space, the entity's frame): each goal keeps its real offset from the
// headset horizontally and in yaw, placed from the game camera (as the port places the hands), and its height from
// the room's floor (feet on the floor; the spine absorbs a camera height that differs a little from the scaled eye).
//
// It drives the hips (waist tracker) and the legs (feet trackers), with the hips giving way to the feet (4f); with the
// spine setting also the spine and head, on the headset (the head goal relative to the camera in all three axes, so
// V's eyes sit on the view; the spine bridges it to the hips, which also give way to the head). The arms stay the
// port's and solve from the shoulders this leaves. The leg bones written are marked as the body solve's own
// (VRIK_MarkLegOwned) so the port's model cache replays them; Hips and the spine are the cache's already.
#include <cstdint>

namespace cvr::fbt::game {

// The setting (menu "* V's legs follow the trackers"; fbt_settings.ini, legs=0/1), off by default. It acts once the
// calibration has run in this session.
bool LegsEnabled();
void SetLegsEnabled(bool on);
// "* V's spine and head follow the headset" (fbt_settings.ini, spine=0/1), off by default; acts with the legs on.
bool SpineEnabled();
void SetSpineEnabled(bool on);
// "* V's arms follow the controllers" (fbt_settings.ini, arms=0/1), off by default; acts with the legs and spine on.
bool ArmsEnabled();
void SetArmsEnabled(bool on);

// THE ARMS (arms v2, the lab's: the elbow from the hand's position, the hand's rotation onto the wrist, V's arms
// scaled to the calibrated length, the clavicle protracting): in place of the port's VRIK_SolveArm, on the port's
// final hand target -- after its wheel grab, reload hand-off, hand stops, two-hand grip -- so all of those still
// apply. side 0 left, 1 right; target / handRot: the hand bone's model transform. False (the port solves) when the
// arms setting is off or the body solve did not run this pass.
bool SolveArm(uint8_t* boneBuf, int side, const float target[3], const float handRot[4]);

// THE CAMERA AT V's EYES. The game's first-person camera stands at the game's own eye height (about 1.79 m for male V,
// against 1.71 m for V's eye joints) plus the head's movement; with the spine on, V's eyes are the headset's, so the
// camera patch (Hooks/PatchCamera.cpp, where the port adds the head translation, both eyes) sets the view's height to
// the entity's plus V's eye height as calibrated from the floor: an absolute target, nothing fed back. False when off
// (the switches, off foot, no calibration, the solve not run for 200 ms). The port's body base and squat do not see it
// (published before; the headset's own height).
bool CameraEyeHeight(float* modelZ);
// Full-body tracking drives V's body: the body solve ran within the last 200 ms (the legs on, calibrated, on foot).
// The port's own body sources stand down meanwhile: the player's tracked body is the body (Anim/CharacterRig.cpp,
// the clip's torso through a slide, a fall, a jump or a landing).
bool BodyActive();

// camModelPos / camModelRot: the game camera in V's model space (VRIK_ComputeCamModel), the camera frame x right,
// y forward, z up.
void SolveLowerBody(uint8_t* boneBuf, const float camModelPos[3], const float camModelRot[4]);

}  // namespace cvr::fbt::game
