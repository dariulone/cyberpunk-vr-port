#pragma once
#include <openxr/openxr.h>
#include <vector>

namespace cvr::eye_tracking {
enum class State { Unavailable, Waiting, Tracking, ProjectionUnavailable, Error };
State GetState();
void ChooseExtensions(const std::vector<XrExtensionProperties>&, std::vector<const char*>&);
void CreateAction(XrInstance, XrSystemId, XrActionSet);
void CreateSpace(XrSession);
// Views and gaze must describe the same predicted instant. Gaze is located in VIEW space.
void Sample(XrSession, XrSpace viewSpace, XrTime, bool focusedAndSynced, const XrView*, unsigned count);
void Clear();
void Shutdown(); // after the frame thread stops, before destroying the session/action set
}
