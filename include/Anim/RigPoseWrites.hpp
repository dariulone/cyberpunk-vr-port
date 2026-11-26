#pragma once

namespace cvr::anim {
// Script requests and animation jobs share the rig tables. The pose hook uses
// an SEH finally block so a stale engine pointer cannot leave this lock held.
void LockRigPoseState() noexcept;
void UnlockRigPoseState() noexcept;
struct RigPoseStateGuard {
    RigPoseStateGuard() noexcept { LockRigPoseState(); }
    ~RigPoseStateGuard() { UnlockRigPoseState(); }
    RigPoseStateGuard(const RigPoseStateGuard&) = delete;
    RigPoseStateGuard& operator=(const RigPoseStateGuard&) = delete;
};

extern bool rigWriteTranslation[]; // guarded by the rig state lock
int WriteRigOffset(int which, int bone, float x, float y, float z);
int WriteRigRotation(int which, int bone, float angle, float x, float y, float z);
int WriteRigScale(int which, int bone, float scale);
int WriteRigPose(int which, int bone, float x, float y, float z,
                 float qi, float qj, float qk, float qr, bool absolute);
bool ReleaseRigWrite(int which, int bone);
void ClearRigWrites();
}
