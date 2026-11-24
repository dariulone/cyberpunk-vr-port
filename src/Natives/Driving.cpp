#include "Natives/NativeFunctions.hpp"
#include "Anim/WheelGrab.hpp"
#include <RED4ext/Scripting/Utils.hpp>

void GetVRWheelControlState(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    frame->code++;
    if(out)*out=cvr::anim::WheelControlState();
}
void SetVRWheelGamepadProfile(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    float inner{},outer{};
    RED4ext::GetParameter(frame,&inner);RED4ext::GetParameter(frame,&outer);frame->code++;
    const bool accepted=cvr::anim::WheelSetGamepadProfile(inner,outer);
    if(out)*out=accepted;
}
// SetVRWheelCabin(pos, rot) -> Bool: the car's steering_wheel slot in the player's model space (CyberpunkVRPort_Driving,
// each frame while a hand holds the rim).
void SetVRWheelCabin(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,bool* out,int64_t) {
    RED4ext::Vector4 pos{};RED4ext::Quaternion rot{};
    RED4ext::GetParameter(frame,&pos);RED4ext::GetParameter(frame,&rot);frame->code++;
    const float p[3]={pos.X,pos.Y,pos.Z},q[4]={rot.i,rot.j,rot.k,rot.r};
    const bool accepted=cvr::anim::WheelSetCabinSlot(p,q);
    if(out)*out=accepted;
}
