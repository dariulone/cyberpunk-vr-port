#if !CVR_RUST_VRIK
// The C++ build's natives; the Rust build's are in rust/crates/cvr-rig/src/vrik/story_attention.rs.
#include "Natives/NativeFunctions.hpp"
#include "Quest/StoryAttention.hpp"
#include "Core/VrCoreShared.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>

namespace {
cvr::quest::ObjectKey Key(const RED4ext::Handle<RED4ext::ent::Entity>& object) {
    return {reinterpret_cast<uintptr_t>(object.instance),reinterpret_cast<uintptr_t>(object.refCount)};
}
}
void VRStoryAttentionUpdate(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::ent::Entity> player;
    int32_t started{},done{},locomotion{-1},tier{},workspot{},vehicle{},choices{},paused{1};
    RED4ext::GetParameter(frame,&player);RED4ext::GetParameter(frame,&started);RED4ext::GetParameter(frame,&done);
    RED4ext::GetParameter(frame,&locomotion);RED4ext::GetParameter(frame,&tier);RED4ext::GetParameter(frame,&workspot);
    RED4ext::GetParameter(frame,&vehicle);RED4ext::GetParameter(frame,&choices);RED4ext::GetParameter(frame,&paused);frame->code++;
    cvr::quest::AttentionContext context{};
    if(player && !paused) {
        context.player=Key(player);context.entity=player->entityID.hash;context.stamp=GetTickCount64();
        context.mode=cvr::quest::SelectAttention(started,done,locomotion,tier,workspot!=0,g_isInVehicle,vehicle,choices!=0);
    }
    cvr::quest::PublishAttention(context);if(out)*out=static_cast<int32_t>(context.mode);
}
void VRManualClueUpdate(RED4ext::IScriptable*,RED4ext::CStackFrame* frame,int32_t* out,int64_t) {
    RED4ext::Handle<RED4ext::ent::Entity> player,clue;
    RED4ext::GetParameter(frame,&player);RED4ext::GetParameter(frame,&clue);frame->code++;
    const bool pressed=cvr::quest::UpdateManualClue(Key(player),Key(clue));if(out)*out=pressed ? 1:0;
}
#endif
