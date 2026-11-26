#if !CVR_RUST_VRIK
// The C++ build's natives; the Rust build's are in rust/crates/cvr-rig/src/vrik/wheel_grab.rs.
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
#endif
