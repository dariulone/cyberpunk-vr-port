// CameraMath -- FOV, IPD, and turning a quaternion into the basis the engine's camera wants.
//
// Small, and every function in it is a measured relationship rather than a formula from a book:
//
//   * The engine's FOV field is VERTICAL. The horizontal one derives from it and the render aspect, and
//     getting that backwards is the mistake that made the image look right at one resolution and wrong
//     at every other.
//   * GetDesiredHalfIpd is half the IPD because the two eyes are placed symmetrically about the camera
//     position, not because half an IPD means anything on its own.
//   * WriteRenderCameraBasis and ApplyFinalCameraOrientationFromQuat write ROWS, and the row order was
//     established by writing a known rotation and reading back what moved.

#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>
#include <MinHook.h>
#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Camera/CameraState.hpp"
#include "Hooks/Hook.hpp"

// The exports (CyberpunkVR_CoherentHandAnchor, CyberpunkVR_CamFinalRowOrder) are src/Camera/CameraLinkState.cpp's; the runtime's
// view separation comes through src/Camera/CameraServices.cpp, which tools/vrik_tests replaces.
extern "C" int CyberpunkVR_CamFinalRowOrder;
extern "C" float cvr_cpp_xr_runtime_ipd();

float GetWorldScale() {
    // Uniform world scale. Multiplies both the eye separation and the head-
    // translation gain, so lowering it makes the world appear bigger.
    const float ws = g_liveControls.xrWorldScale;
    return (ws > 0.0f) ? ws : 1.0f;
}

float GetDesiredHalfIpd() {
    // Auto per-person/per-headset: the half-IPD comes straight from the OpenXR
    // runtime view separation, so it already adapts to whoever is wearing the HMD.
    const float runtimeIpd = cvr_cpp_xr_runtime_ipd();
    const float halfIpd = runtimeIpd > 0.001f ? runtimeIpd * 0.5f : 0.032f;
    // Eye-separation = runtime half-IPD x ipdScale x worldScale x stereoScale.
    // The neutral baseline is ipdScale=1.0 (raw runtime IPD, typically
    // +-0.033 m). The old 1.5 value exaggerated depth
    // and distorted perceived world scale even when the camera FOV was correct.
    // Keep xr_stereo_scale as an optional taste multiplier, but default the core
    // IPD path to 1:1 with the runtime.
    float ipdScale = g_liveControls.xrIpdScale;
    if (!(ipdScale > 0.0f)) {
        ipdScale = 1.0f;  // guard zero-init window / bad values (honest runtime IPD)
    }
    float stereoScale = g_liveControls.xrStereoScale;
    if (!(stereoScale > 0.0f)) {
        stereoScale = 1.0f;  // guard zero-init window / bad values
    }
    return halfIpd > 0.0001f ? halfIpd * GetWorldScale() * ipdScale * stereoScale : 0.0f;
}

// The overlay lives in this DLL too, so it can have the half-IPD straight rather than via a
// shared slot that may or may not have been written yet.
extern "C" float CyberpunkVRPort_HalfIpd() { return GetDesiredHalfIpd(); }

// COHERENT HAND ANCHOR. The arms hang off a view pose, and measurement put that pose 33 ms old
// at solve time while the hand offsets it is combined with were 21 ms old. A hand position is
// only reconstructed correctly when the head pose and the head-relative offset come from the
// SAME instant; mix two instants and the hand lands in the wrong WORLD place, by the head motion
// in between -- which is why it wobbles when the head moves and sits still when it does not.
//
// Locate publishes a framed recipe through CameraLink. Model-space calibration
// follows entity yaw; physical tracking follows entityYaw - bodyRealign. (The switch, CyberpunkVR_CoherentHandAnchor, is
// CameraLinkState.cpp's.)

float cvr::camera::BodyAnchorYaw(float fallback) {
    if (g_isInVehicle || !CyberpunkVR_EngineBodyYawValid) return fallback;
    const float z=CyberpunkVR_EngineBodyYawZ, w=CyberpunkVR_EngineBodyYawW;
    if (!std::isfinite(z) || !std::isfinite(w) || z*z+w*w<0.5f) return fallback;
    return 2.0f*std::atan2(z,w);
}

static bool IsFiniteFloat(float value) {
    return std::isfinite(value);
}

bool IsPlausibleUnitVector3(const float* v) {
    if (!v) return false;
    if (!IsFiniteFloat(v[0]) || !IsFiniteFloat(v[1]) || !IsFiniteFloat(v[2])) return false;

    const float lenSq = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    return lenSq > 0.25f && lenSq < 4.0f;
}

bool IsPlausibleUnitQuaternion(const float* q) {
    if (!q) return false;
    if (!IsFiniteFloat(q[0]) || !IsFiniteFloat(q[1]) || !IsFiniteFloat(q[2]) || !IsFiniteFloat(q[3])) return false;

    const float lenSq = q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3];
    return lenSq > 0.25f && lenSq < 4.0f;
}

bool IsPlausiblePositionVec4(const float* v) {
    if (!v) return false;
    if (!IsFiniteFloat(v[0]) || !IsFiniteFloat(v[1]) || !IsFiniteFloat(v[2]) || !IsFiniteFloat(v[3])) return false;
    return fabsf(v[3] - 1.0f) < 0.25f;
}

void ComputeRightVectorFromQuaternion(const float* q, float* outRight) {
    if (!q || !outRight) return;

    const float x = q[0];
    const float y = q[1];
    const float z = q[2];
    const float w = q[3];

    outRight[0] = 1.0f - 2.0f * (y * y + z * z);
    outRight[1] = 2.0f * (x * y + z * w);
    outRight[2] = 2.0f * (x * z - y * w);
}

static void BuildGameViewRowsFromQuaternion(const float* q, float* outViewRows) {
    if (!q || !outViewRows) return;

    const float x = q[0];
    const float y = q[1];
    const float z = q[2];
    const float w = q[3];

    outViewRows[0] = 1.0f - 2.0f * (y * y + z * z);
    outViewRows[1] = 2.0f * (x * y + z * w);
    outViewRows[2] = 2.0f * (x * z - y * w);
    outViewRows[3] = 0.0f;

    outViewRows[4] = 2.0f * (x * z + y * w);
    outViewRows[5] = 2.0f * (y * z - x * w);
    outViewRows[6] = 1.0f - 2.0f * (x * x + y * y);
    outViewRows[7] = 0.0f;

    outViewRows[8] = 2.0f * (x * y - z * w);
    outViewRows[9] = 1.0f - 2.0f * (x * x + z * z);
    outViewRows[10] = 2.0f * (y * z + x * w);
    outViewRows[11] = 0.0f;
}

// 1 = transposed. MEASURED, not chosen.
//
// Broke at the site with our write disabled and read both representations of the SAME engine
// camera: the quaternion at rsiPtr+4 and the basis rows at rsiPtr+20. For
// q = (-0.112416, 0.204406, -0.710105, 0.664354) the stored rows came out as
//   row0 (-0.091985,  0.431242,  0.897528)
//   row1 (-0.989412, -0.140871, -0.033687)
//   row2 (-0.111957,  0.891184, -0.439637)
// and building R(q) gives row[r] = (R[r][0], R[r][2], R[r][1]) to within 1e-5 on all nine terms.
// So the stored basis is R with the Y and Z columns exchanged -- the game's "X right, Y forward,
// Z up" against the quaternion's "X right, Y up, Z forward" -- and that is exactly the transpose
// of what BuildGameViewRowsFromQuaternion emits. The original convention was right; writing the
// rows straight was my error. 0 keeps the straight form available for comparison. (CyberpunkVR_CamFinalRowOrder:
// CameraLinkState.cpp.)

// Write the orientation into the RENDER CAMERA's own basis rows, the ones the view-matrix bake
// reads (component +0xC0/+0xD0/+0xE0, i.e. rsiPtr + 20/24/28 floats -- rsiPtr is component+0x70).
//
// Deliberately NOT ApplyFinalCameraOrientationFromQuat: that one fills three different targets in
// three different conventions, and for the +0xC0 block it writes
//   row0 = (viewRows[0], viewRows[4], viewRows[8])
// which is the TRANSPOSE of the basis. BuildGameViewRowsFromQuaternion emits the axes as rows --
// [0..3] right, [4..7] forward, [8..11] up -- so transposing them inverts the rotation, and an
// inverted camera rotation is precisely "the world drags along with your head": the image is
// counter-rotated while the submitted pose turns correctly. That function stays untouched because
// the AER path depends on its other two writes, where the same transpose is only ever applied to
// a near-identity cant correction and therefore never showed.
void WriteRenderCameraBasis(float* rsiPtr, const float* q) {
    if (!rsiPtr || !q) return;
    float viewRows[12] = {};
    BuildGameViewRowsFromQuaternion(q, viewRows);

    float rows[12] = {};
    if (CyberpunkVR_CamFinalRowOrder == 0) {
        for (int i = 0; i < 12; ++i) rows[i] = viewRows[i];
    } else {
        rows[0] = viewRows[0]; rows[1] = viewRows[4]; rows[2] = viewRows[8];  rows[3] = 0.0f;
        rows[4] = viewRows[1]; rows[5] = viewRows[5]; rows[6] = viewRows[9];  rows[7] = 0.0f;
        rows[8] = viewRows[2]; rows[9] = viewRows[6]; rows[10] = viewRows[10]; rows[11] = 0.0f;
    }
    // Preserve whatever the engine keeps in the 4th lane of each row rather than zeroing it.
    for (int r = 0; r < 3; ++r) {
        WriteFloatArraySafe(rsiPtr + 20 + r * 4, rows + r * 4, 3);
    }
    // The quaternion the rows were built from, kept in step at component +0x80 (measured live as
    // a unit quaternion) so any downstream rebuild agrees with the rows.
    WriteFloatArraySafe(rsiPtr + 4, q, 4);
}

void ApplyFinalCameraOrientationFromQuat(float* rsiPtr, const float* q) {
    if (!rsiPtr || !q || !IsPlausibleUnitQuaternion(q)) return;

    float viewRows[12] = {};
    BuildGameViewRowsFromQuaternion(q, viewRows);

    // Keep the raw quaternion in sync so any downstream camera rebuilds see VR orientation.
    WriteFloatArraySafe(rsiPtr + 4, q, 4);

    float cameraMtx[16] = {};
    if (ReadFloatArraySafe(rsiPtr + 20, cameraMtx, 16)) {
        cameraMtx[0] = viewRows[0];
        cameraMtx[1] = viewRows[4];
        cameraMtx[2] = viewRows[8];
        cameraMtx[4] = viewRows[1];
        cameraMtx[5] = viewRows[5];
        cameraMtx[6] = viewRows[9];
        cameraMtx[8] = viewRows[2];
        cameraMtx[9] = viewRows[6];
        cameraMtx[10] = viewRows[10];
        WriteFloatArraySafe(rsiPtr + 20, cameraMtx, 16);
    }

    WriteFloatArraySafe(rsiPtr + 68, viewRows, 12);

    float viewPacket[16] = {};
    if (ReadFloatArraySafe(rsiPtr + 204, viewPacket, 16)) {
        float scale0 = sqrtf(viewPacket[0] * viewPacket[0] + viewPacket[1] * viewPacket[1] + viewPacket[2] * viewPacket[2]);
        float scale1 = sqrtf(viewPacket[4] * viewPacket[4] + viewPacket[5] * viewPacket[5] + viewPacket[6] * viewPacket[6]);
        if (!IsFiniteFloat(scale0) || scale0 < 0.05f || scale0 > 10.0f) scale0 = 1.0f;
        if (!IsFiniteFloat(scale1) || scale1 < 0.05f || scale1 > 10.0f) scale1 = scale0;

        viewPacket[0] = viewRows[0] * scale0;
        viewPacket[1] = viewRows[1] * scale0;
        viewPacket[2] = viewRows[2] * scale0;
        viewPacket[4] = viewRows[4] * scale1;
        viewPacket[5] = viewRows[5] * scale1;
        viewPacket[6] = viewRows[6] * scale1;
        viewPacket[12] = viewRows[8];
        viewPacket[13] = viewRows[9];
        viewPacket[14] = viewRows[10];

        const int32_t* finalPosFP = reinterpret_cast<const int32_t*>(rsiPtr);
        // finalPosFP is the ABSOLUTE WorldPosition (engine base + the finalPos write
        // above), stored at the true 131072 (17-bit) fixed-point scale. Written as
        // 50/131072 -- NOT 25/65536 -- so the 131072 WorldPosition scale stays explicit
        // and consistent with the rest of the file (the lone 65536 read like a leftover
        // from the old wrong scale and invited a bogus "fix" to 25/131072, which would
        // HALVE this row). 50/131072 is bit-identical to the old 25/65536; net effect is
        // (world meters * 50) for the +0x330 view-position row. This line was never part
        // of the 65536->131072 sweep -- that fixed only the additive worldDelta/ipdShift.
        const float posScaleView = 50.0f / 131072.0f;
        viewPacket[8] = static_cast<float>(finalPosFP[0]) * posScaleView;
        viewPacket[9] = static_cast<float>(finalPosFP[1]) * posScaleView;
        viewPacket[10] = static_cast<float>(finalPosFP[2]) * posScaleView;

        WriteFloatArraySafe(rsiPtr + 204, viewPacket, 16);
    }
}
