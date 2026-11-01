#pragma once
void VRIK_QuatFromTo(const float* a, const float* b, float* o);
bool VRIK_ReferenceFingerRotation(int bone, float* rotation);
bool VRIK_ReferenceLocal(int bone, float* position, float* rotation);
int VRIK_LadderFingerBone(int side, int slot);
