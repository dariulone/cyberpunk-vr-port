#pragma once
#include <cstdint>
#include "Anim/CyberwareDynamics.hpp"

// The player base graph feeds SharedMetaPose in the downstream deformation
// graphs. VRIK belongs after this producer, before those consumers run.
inline constexpr uint64_t VRIK_PlayerBaseGraph=0x7c8ef3ac7af95509ull;
extern int g_VRCyberwareFamily; // 0 none, 1 gorilla, 2 mantis, 3 monowire, 4 launcher
extern "C" { __declspec(dllexport) extern int CyberpunkVR_NativeCyberwarePasses; }
bool VRIK_UsesNativeCyberware();
inline bool VRIK_IsNativeCyberwareConsumer(uint64_t graph) {
    return VRIK_UsesNativeCyberware() && graph!=VRIK_PlayerBaseGraph;
}
void VRIK_PublishNativeCyberwarePose(uint8_t* buffer,uint64_t graph);
void VRIK_ConfigureCyberwareRig(const char* const* names,int count,const uint8_t* reference,int referenceCount);
void VRIK_ConfigureCyberwareTracks(const char* const* names,int count,const float* reference);
void VRIK_ApplyCyberwarePose(uint8_t* buffer,float* tracks=nullptr,uint32_t trackCount=0);
void VRIK_ApplyWireTracks(float* tracks,uint32_t count);
namespace cvr::cyberware {
struct Frame {
    int family{},count[2]{};
    Vec3 points[2][Wire::Count]{},bodyOrigin{};
    Vec3 wrists[2]{};
    float handRotation[2][4]{};
    float deployment[2]{};
    uint32_t epoch{};
    uint64_t stamp{};
};
bool ReadFrame(Frame& out);
// The line the selected launcher's shot leaves on, in the world (vrport_launcher_aim.reds fires along it).
bool LauncherLine(float* origin,float* direction);
void SetWirePlanes(int side,uint32_t epoch,const std::array<Plane,Wire::Count>& planes);
void MusclePulse(int side,float duration);
void Select(int kind);
int SelectedKind();
void PublishGrips(bool left,bool right);
void CollisionInfo(float* values);
float WireDeployment(int side);
}
