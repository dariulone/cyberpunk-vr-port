#pragma once
#include <openxr/openxr.h>

// HAND TRACKING -- the fingers as the controllers feel them (XR_EXT_hand_tracking; SteamVR answers it for Index
// controllers, source "controller"). Each frame it turns the joints into each finger's bend and publishes that to the
// empty hands' gesture (Anim/HandGestures.hpp). Only ever draws fingers: fire, grip and the buttons stay the actions'.
// A runtime without it, or the setting xr_finger_tracking off, leaves the hands on grip and trigger.
namespace cvr::handtracking {
// The static name to enable if this extension is one hand tracking uses, else null.
const char* Extension(const char* name);
// On the XR frame thread, after xrSyncActions.
void Tick(XrInstance instance, XrSystemId system, XrSession session, XrSpace space, XrTime time);
}
