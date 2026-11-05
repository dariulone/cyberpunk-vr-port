#include "Natives/NativeFunctions.hpp"
#include "Hooks/LadderInput.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/state/MachineparameterTypeLadderDescription.hpp>

void SetVRLadderState(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> player,description;int32_t detailed=-1,paused=0;
    RED4ext::GetParameter(frame,&player);RED4ext::GetParameter(frame,&detailed);
    RED4ext::GetParameter(frame,&paused);RED4ext::GetParameter(frame,&description);frame->code++;
    cvr::ladder::Geometry geometry{};
    if(description.instance && description.instance->GetType()->name==RED4ext::CName(RED4ext::LadderDescription::NAME)) {
        const auto* d=reinterpret_cast<const RED4ext::LadderDescription*>(description.instance);
        geometry.position={d->position.X,d->position.Y,d->position.Z};geometry.normal={d->normal.X,d->normal.Y,d->normal.Z};
        geometry.up={d->up.X,d->up.Y,d->up.Z};geometry.height=d->topHeightFromPosition;
        geometry.topStep=d->verticalStepTop;
        geometry.firstRung=d->verticalStepBottom;
        uint64_t hash=14695981039346656037ull;
        for(float value:{geometry.position.x,geometry.position.y,geometry.position.z,geometry.normal.x,
            geometry.normal.y,geometry.normal.z,geometry.height,geometry.topStep}) { hash^=uint64_t(int64_t(std::llround(value*1000)));hash*=1099511628211ull; }
        geometry.identity=hash;geometry.Prepare();
    }
    cvr::ladder::Publish(reinterpret_cast<uintptr_t>(player.instance),detailed,paused!=0,geometry.valid ? &geometry:nullptr);
    if(out)*out=1;
}
void SetVRLadderTopRails(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> player;
    RED4ext::Vector4 ladder,origin,right,normal,up;
    RED4ext::GetParameter(frame,&player);RED4ext::GetParameter(frame,&ladder);RED4ext::GetParameter(frame,&origin);
    RED4ext::GetParameter(frame,&right);RED4ext::GetParameter(frame,&normal);RED4ext::GetParameter(frame,&up);frame->code++;
    const bool ok=cvr::ladder::PublishTop(reinterpret_cast<uintptr_t>(player.instance),
        {ladder.X,ladder.Y,ladder.Z},{origin.X,origin.Y,origin.Z},{right.X,right.Y,right.Z},
        {normal.X,normal.Y,normal.Z},{up.X,up.Y,up.Z});
    if(out)*out=ok ? 1:0;
}
// SetVRLadderMeasuredTop(player, ladderPosition, top): where the ladder's highest module ends above its foot.
void SetVRLadderMeasuredTop(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::IScriptable> player;RED4ext::Vector4 ladder;float top{};
    RED4ext::GetParameter(frame,&player);RED4ext::GetParameter(frame,&ladder);RED4ext::GetParameter(frame,&top);frame->code++;
    const bool ok=cvr::ladder::PublishMeasuredTop(reinterpret_cast<uintptr_t>(player.instance),{ladder.X,ladder.Y,ladder.Z},top);
    if(out)*out=ok ? 1:0;
}
void GetVRLadderDebug(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,float* out,int64_t) {
    int32_t index{};RED4ext::GetParameter(frame,&index);frame->code++;
    if(out)*out=cvr::ladder::DebugValue(index);
}
void SetVRLadderTestGrip(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    float left{},right{};int32_t milliseconds{};
    RED4ext::GetParameter(frame,&left);RED4ext::GetParameter(frame,&right);RED4ext::GetParameter(frame,&milliseconds);frame->code++;
    const bool ok=cvr::ladder::SetSimulatorGrip(left,right,milliseconds);if(out)*out=ok ? 1:0;
}
