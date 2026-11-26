#include "Anim/BladeContactPose.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/LeftHanded.hpp"
#include "Core/VrCoreShared.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/Vector4.hpp>
#include <algorithm>
#include <cmath>
#include <mutex>

namespace cvr::blade {
namespace {
struct Pose { float delta[3]{}; unsigned long long at{}; int side{-1}; };
Pose published;
std::mutex lock;
std::atomic<bool> active{};

void Write(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* result, int64_t) {
    int32_t side = -1; RED4ext::Vector4 d{};
    RED4ext::GetParameter(frame, &side); RED4ext::GetParameter(frame, &d); frame->code++;
    const float m2 = d.X*d.X + d.Y*d.Y + d.Z*d.Z;
    const bool accepted = side >= 0 && side <= 1 && d.W > .5f && std::isfinite(m2) && m2 <= .011f;
    std::lock_guard guard(lock);
    published = accepted ? Pose{{d.X,d.Y,d.Z},GetTickCount64(),side} : Pose{};
    active.store(accepted,std::memory_order_release);
    if (result) *result = accepted;
}
void Register() {
    auto* f = RED4ext::CGlobalFunction::Create("VRBladePoseOffset","VRBladePoseOffset",&Write);
    f->flags = {.isNative=true,.isStatic=true};
    f->AddParam("Int32","side"); f->AddParam("Vector4","modelOffset"); f->SetReturnType("Bool");
    RED4ext::CRTTISystem::Get()->RegisterFunction(f);
}
}
void RegisterPoseNative() { RED4ext::CRTTISystem::Get()->AddPostRegisterCallback(Register); }

void ApplyPose(int side, float target[3]) {
    if (!active.load(std::memory_order_acquire) || !g_hasWeaponEquipped || g_isInVehicle || g_menuModeValue!=0 ||
        cvr::cyberware::SelectedKind()!=5 || side!=(cvr::anim::WeaponInLeftHand()?0:1)) return;
    Pose p;
    { std::lock_guard guard(lock); p=published; }
    const auto age=GetTickCount64()-p.at;
    if (p.side!=side || age>=160) return;
    const float fade=std::min(1.f,float(160-age)/80.f);
    for (int i=0;i<3;++i) target[i]+=p.delta[i]*fade;
}
}
