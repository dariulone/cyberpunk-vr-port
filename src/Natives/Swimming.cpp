#if !CVR_RUST_VRIK
// The C++ build's natives; the Rust build's are in rust/crates/cvr-rig/src/vrik/swimming_input.rs.
#include "Natives/NativeFunctions.hpp"
#include "Hooks/SwimmingInput.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>

void SetVRSwimmingState(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> player;
    int32_t state=-1,fast=-1,paused=0,waterContext=0;
    RED4ext::GetParameter(frame,&player);
    RED4ext::GetParameter(frame,&state);
    RED4ext::GetParameter(frame,&fast);
    RED4ext::GetParameter(frame,&paused);
    RED4ext::GetParameter(frame,&waterContext);
    frame->code++;
    cvr::swimming::PublishState(reinterpret_cast<uintptr_t>(player.instance),state,fast,paused!=0,waterContext!=0);
    if(out)*out=1;
}
void GetVRSwimmingDebug(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,float* out,int64_t) {
    int32_t index=0;RED4ext::GetParameter(frame,&index);frame->code++;
    if(out)*out=cvr::swimming::DebugValue(index);
}
#endif
