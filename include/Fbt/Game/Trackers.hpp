#pragma once
#include <openxr/openxr.h>
#include <string>
#include <vector>

// VIVE TRACKERS (xr_vive_trackers) for full-body tracking: waist, left foot and right foot, read through OpenXR
// XR_HTCX_vive_tracker_interaction. SteamVR exposes each tracker with the role assigned in Settings > Controllers >
// Manage Vive Trackers. One pose action with the three role paths as subactions, in the gameplay action set.
//
// The trackers are located in the same space and at the same instant as the hands, and stored the same way:
// head-relative, rel = inverse(headOri) * (trackerPos - headPos). Raw, no filter.
//
// Full-body tracking also works in STAGE space: SteamVR's floor centre from the room setup, +Y up, fixed axes. The
// head and the trackers are located there too, at the same instant. LOCAL (the port's base space) and STAGE differ
// only by a change of base (yaw + translation), which changes only when the runtime moves one of them.
//
// It reads and logs (once a second per role: active, pose, valid-sample rate, dropouts; the head in STAGE and the
// LOCAL->STAGE transform) and feeds the calibration (Fbt/Game/Calibration.hpp). Off by default; with
// xr_vive_trackers=0 the extension is not even enabled. Part of the full-body tracking module (src/Fbt/Game).
//
// The HTCX reading follows purraricat's fork of the port (src/vr/openxr/openxr_vive_trackers.cpp, MIT).
namespace cvr::trackers {

enum Role { kWaist = 0, kLeftFoot = 1, kRightFoot = 2, kRoleCount = 3 };

struct Pose {
    bool valid = false;
    XrVector3f pos{};             // head-relative, OpenXR axes, metres
    XrQuaternionf ori{0, 0, 0, 1};
    bool stageValid = false;
    XrPosef stage{{0, 0, 0, 1}, {0, 0, 0}};   // STAGE space: floor centre, metres
};

// Before xrCreateInstance: adds the extension when xr_vive_trackers=1 and the runtime has it.
void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled);
// After xrCreateInstance.
void OnInstanceCreated(XrInstance instance);
// Inside the gameplay action-set setup, before the action set is attached.
void CreateActions(XrInstance instance, XrActionSet actionSet);
// After the action set is attached to the session.
// handSpaces: the controllers' grip-pose spaces (left, right), located with the trackers for the calibration and helpers.
void OnSessionCreated(XrSession session, const XrSpace handSpaces[2]);
void OnSessionDestroying();
// After xrSyncActions, with the head the hands are made relative to, located at the hands' instant. view = the
// session's VIEW space, for the head in STAGE.
void Locate(XrSpace base, XrSpace view, XrTime time, const XrPosef& head);

bool Read(Role role, Pose* out);
// The headset in STAGE, located with the trackers (the same instant); false when not tracked.
bool ReadHeadStage(XrPosef* out);
// Counts the locates (one per XR frame with the hands): a reader tells a new sample from the one it saw.
unsigned long long LocateCount();

// The controllers' grip poses, head-relative like the trackers (OpenXR axes), for the calibration helpers.
bool ReadHand(int hand, Pose* out);   // 0 left, 1 right

// True while an OpenXR session with the tracker interaction runs (xr_vive_trackers=1 and the runtime has it).
bool Running();

}
