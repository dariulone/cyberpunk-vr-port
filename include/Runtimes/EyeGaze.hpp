#pragma once
#include "Runtimes/EyeGazeMath.hpp"
#include <openxr/openxr.h>
#include <cstdint>
#include <vector>

namespace cvr::eyegaze {

struct Snapshot {
    TrackingState state = TrackingState::Unavailable;
    bool extensionEnabled = false;
    bool systemSupported = false;
    float offsetX[2]{};
    float offsetY[2]{};
    float blend = 0.0f;
    uint64_t validSamples = 0;
};

// OpenXR lifecycle hooks (called by OpenXRManager / OpenXRFrameLoop).
void ChooseExtensions(const std::vector<XrExtensionProperties>& available, std::vector<const char*>& enabled);
void InspectSystem(XrInstance instance, XrSystemId systemId);
void CreateActions(XrInstance instance);
XrActionSet ActionSet();
void OnSessionCreated(XrSession session);
void OnSessionStopped();
void OnSessionDestroying();
void OnInteractionProfileChanged();
void Shutdown();

// Per-frame update on the XR frame loop right after xrSyncActions.
void Update(XrSession session, XrSessionState sessionState, XrSpace viewSpace, XrTime locateTime,
            bool canLocate, const FrustumTangents eyeWindows[2], bool windowsValid, float ipdMeters);

// Thread-safe lock-free consumers (FoveatedDlss, UI, and future VRS).
Snapshot GetSnapshot();
bool ResolveEyeOffsets(unsigned physicalEye, float fallbackX, float fallbackY,
                       float maxAbsX, float maxAbsY, float& outX, float& outY);
bool ResolveEyeOffsets(unsigned physicalEye, float fallbackX, float fallbackY,
                       float& outX, float& outY);

void SetClockForTesting(uint64_t (*clockUs)());

}  // namespace cvr::eyegaze
