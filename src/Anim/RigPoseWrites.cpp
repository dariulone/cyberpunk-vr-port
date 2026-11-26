#include "Anim/RigPoseWrites.hpp"
#include "Anim/VrikState.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::anim {
namespace {
SRWLOCK stateLock = SRWLOCK_INIT;
bool ValidKey(int which, int bone) { return which >= 0 && which < VRRIG_N && bone >= 0 && bone < VRIK_MAX_BONES; }
bool Finite(float x, float y, float z) { return std::isfinite(x) && std::isfinite(y) && std::isfinite(z); }

void ResetSlot(int slot) {
    g_RigWriteEnabled[slot] = 0;
    rigWriteTranslation[slot] = false;
    g_RigWriteHaveBase[slot] = g_RigWriteHaveBaseRot[slot] = 0;
    g_RigWriteApplied[slot] = 0;
    g_RigWriteSlot[slot] = g_RigWritePin[slot] = -1;
    g_RigWriteAbs[slot] = g_RigWriteQuatOn[slot] = 0;
    g_RigWriteScale[slot] = g_RigWriteRotAngle[slot] = 0.0f;
    for (int axis = 0; axis < 3; ++axis) {
        g_RigWriteOff[slot][axis] = g_RigWriteBase[slot][axis] = 0.0f;
        g_RigWriteRotAxis[slot][axis] = axis == 1 ? 1.0f : 0.0f;
    }
    for (int axis = 0; axis < 4; ++axis)
        g_RigWriteQuat[slot][axis] = g_RigWriteBaseRot[slot][axis] = axis == 3 ? 1.0f : 0.0f;
}

int Slot(int which, int bone) {
    if (!ValidKey(which, bone)) return -1;
    const int count = g_RigWriteN;
    for (int i = 0; i < count; ++i)
        if (g_RigWriteWhich[i] == which && g_RigWriteBone[i] == bone) return i;
    if (count >= VRRIG_WRITES) return -1;
    ResetSlot(count);
    g_RigWriteWhich[count] = which;
    g_RigWriteBone[count] = bone;
    g_RigWriteN = count + 1;
    return count;
}
}

bool rigWriteTranslation[VRRIG_WRITES]{};
void LockRigPoseState() noexcept { AcquireSRWLockExclusive(&stateLock); }
void UnlockRigPoseState() noexcept { ReleaseSRWLockExclusive(&stateLock); }

int WriteRigOffset(int which, int bone, float x, float y, float z) {
    if (!Finite(x, y, z)) return -1;
    RigPoseStateGuard guard;
    const int slot = Slot(which, bone); if (slot < 0) return -1;
    if (g_RigWriteAbs[slot]) g_RigWriteHaveBase[slot] = 0;
    g_RigWriteAbs[slot] = 0;
    rigWriteTranslation[slot] = true;
    g_RigWriteOff[slot][0] = x; g_RigWriteOff[slot][1] = y; g_RigWriteOff[slot][2] = z;
    g_RigWriteEnabled[slot] = 1;
    return slot;
}

int WriteRigRotation(int which, int bone, float angle, float x, float y, float z) {
    if (!std::isfinite(angle) || !Finite(x, y, z)) return -1;
    const float length = std::hypot(x, y, z);
    if (!std::isfinite(length)) return -1;
    if (length < 1e-6f) { angle = 0; x = z = 0; y = 1; }
    else { x /= length; y /= length; z /= length; }
    RigPoseStateGuard guard;
    const int slot = Slot(which, bone); if (slot < 0) return -1;
    g_RigWriteRotAxis[slot][0] = x; g_RigWriteRotAxis[slot][1] = y; g_RigWriteRotAxis[slot][2] = z;
    g_RigWriteRotAngle[slot] = angle;
    g_RigWriteEnabled[slot] = 1;
    return slot;
}

int WriteRigScale(int which, int bone, float scale) {
    if (!std::isfinite(scale)) return -1;
    RigPoseStateGuard guard;
    const int slot = Slot(which, bone); if (slot < 0) return -1;
    g_RigWriteScale[slot] = std::clamp(scale, 0.0f, 4.0f);
    g_RigWriteEnabled[slot] = 1;
    return slot;
}

int WriteRigPose(int which, int bone, float x, float y, float z,
                 float qi, float qj, float qk, float qr, bool absolute) {
    if (!Finite(x, y, z) || !Finite(qi, qj, qk) || !std::isfinite(qr)) return -1;
    const float length = std::sqrt(qi*qi + qj*qj + qk*qk + qr*qr);
    if (!std::isfinite(length)) return -1;
    RigPoseStateGuard guard;
    const int slot = Slot(which, bone); if (slot < 0) return -1;
    if (g_RigWriteAbs[slot] && !absolute) g_RigWriteHaveBase[slot] = 0;
    g_RigWriteOff[slot][0] = x; g_RigWriteOff[slot][1] = y; g_RigWriteOff[slot][2] = z;
    g_RigWriteAbs[slot] = absolute ? 1 : 0;
    rigWriteTranslation[slot] = true;
    g_RigWriteQuatOn[slot] = length > 1e-4f ? 1 : 0;
    if (length > 1e-4f) {
        g_RigWriteQuat[slot][0] = qi/length; g_RigWriteQuat[slot][1] = qj/length;
        g_RigWriteQuat[slot][2] = qk/length; g_RigWriteQuat[slot][3] = qr/length;
    }
    g_RigWriteEnabled[slot] = 1;
    return slot;
}

bool ReleaseRigWrite(int which, int bone) {
    RigPoseStateGuard guard;
    for (int slot = 0; slot < g_RigWriteN; ++slot) {
        if (g_RigWriteWhich[slot] != which || g_RigWriteBone[slot] != bone) continue;
        g_RigWriteEnabled[slot] = 0;
        rigWriteTranslation[slot] = false;
        g_RigWriteAbs[slot] = g_RigWriteQuatOn[slot] = 0;
        g_RigWriteRotAngle[slot] = g_RigWriteScale[slot] = 0.0f;
        // Retain a known rest base for subsequent gestures on this same bone.
        // ClearRigWrites discards it when switching weapons.
        for (int axis = 0; axis < 3; ++axis) g_RigWriteOff[slot][axis] = 0;
        return true;
    }
    return false;
}

void ClearRigWrites() {
    RigPoseStateGuard guard;
    for (int slot = 0; slot < VRRIG_WRITES; ++slot) ResetSlot(slot);
    for (int rig = 0; rig < VRRIG_N; ++rig)
        for (int track = 0; track < VRRIG_TRACKS; ++track) g_RigTrackOn[rig][track] = 0;
    g_RigWriteN = 0;
}
}
