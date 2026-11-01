#pragma once
#include <cstdint>
inline constexpr int VRIK_TRANS_OFF = 0;
inline constexpr int VRIK_ROT_OFF = 16;
inline constexpr int VRIK_MAX_BONES = 800;
extern int16_t g_VRBoneParent[800];
extern volatile int g_VRBoneCount, g_VRRightBoneIdx, g_VRLeftBoneIdx, g_VRSmokeCigIdx, g_VRSmokeLighterIdx;
