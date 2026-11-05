#include "Quest/StoryAttention.hpp"
#include "Natives/Reach.hpp"
#include "Camera/ExternalView.hpp"
#include "Camera/CameraState.hpp"
#include "Hooks/RoomscaleMove.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Core/VrCoreShared.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/DebugGate.hpp"
#include <cstring>
#include <limits>
#include <mutex>

extern "C" {
void CvrStoryAttentionDetour();
__declspec(dllexport) int CyberpunkVR_StoryAttentionHookReady{};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_StoryAttentionCounters[5]{};
// gated: player calls / Pacifica / workspot / unavailable pose / explicit clue presses
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_ReachTurns{};  // gated: look-at rays turned for a hand
}
namespace {
std::mutex attentionMutex,clueMutex;
cvr::quest::AttentionContext context;
cvr::quest::ManualClueInput clueInput;
cvr::quest::ObjectKey cluePlayer;
bool Gameplay() {
    return g_menuModeValue==0 && !cvr::vrui::CapturesInput() &&
           OpenXRManager::Get().IsSessionRunning() && !DeviceCamActive();
}
bool Install() {
    auto* base=reinterpret_cast<uint8_t*>(GetModuleHandleW(nullptr));
    // 2.5 TargetingUser_UpdateLookAtRay (2F2430): after the source's +228 virtual call.
    // RBX = user state; [RBP-70] = Vector4 + Quaternion. Verify the entire call
    // and both displaced instructions before replacing only those 8 bytes.
    // (2.31: 3F90E9/3F90F7, [RBP-50], displaced mov rax,[rbp-50]; lea rdx,[rsp+50].)
    constexpr size_t kCall=13;
    const uint8_t expected[]={0x4c,0x8b,0xc0,0x48,0x8d,0x55,0x90,0x49,0x8b,0xca,0x41,0xff,0xd1,
                              0x48,0x8b,0x45,0x90,0x48,0x8d,0x55,0xf0};
    if(!base || std::memcmp(base+0x2F2620,expected,sizeof(expected)))return false;
    auto* site=base+0x2F262D;
    auto* relay=static_cast<uint8_t*>(AllocateTrampoline(site,14));if(!relay)return false;
    const auto distance=reinterpret_cast<intptr_t>(relay)-reinterpret_cast<intptr_t>(site+5);
    if(distance<std::numeric_limits<int32_t>::min() || distance>std::numeric_limits<int32_t>::max())return false;
    const uint8_t jump[]={0xff,0x25,0,0,0,0};std::memcpy(relay,jump,6);
    const auto thunk=reinterpret_cast<uintptr_t>(&CvrStoryAttentionDetour);std::memcpy(relay+6,&thunk,8);
    FlushInstructionCache(GetCurrentProcess(),relay,14);
    uint8_t patch[]={0xe8,0,0,0,0,0x90,0x90,0x90};
    const auto offset=static_cast<int32_t>(distance);std::memcpy(patch+1,&offset,4);
    DWORD protection{},ignored{};
    if(!VirtualProtect(site,sizeof(patch),PAGE_EXECUTE_READWRITE,&protection))return false;
    std::memcpy(site,patch,sizeof(patch));
    const bool restored=VirtualProtect(site,sizeof(patch),protection,&ignored)!=0;
    if(!restored) {std::memcpy(site,expected+kCall,sizeof(patch));VirtualProtect(site,sizeof(patch),protection,&ignored);}
    FlushInstructionCache(GetCurrentProcess(),site,sizeof(patch));
    CyberpunkVR_StoryAttentionHookReady=restored ? 1:0;return restored;
}
CVR_HOOK("StoryAttention",::cvr::hooks::Stage::Boot,89,Install);
}

void cvr::quest::PublishAttention(AttentionContext value) {
    std::lock_guard lock(attentionMutex);context=value;
}
bool cvr::quest::UpdateManualClue(ObjectKey player,ObjectKey clue) {
    if(!Gameplay() || !player || player.address!=cvr::roomscale::PlayerIdentity())clue={};
    std::lock_guard lock(clueMutex);const auto now=GetTickCount64();
    if(player!=cluePlayer) {clueInput.Arm({},now);cluePlayer=player;}
    clueInput.Arm(clue,now);const bool pressed=clueInput.Take(clue,now);
    if(pressed)CVR_DIAGNOSTIC(++CyberpunkVR_StoryAttentionCounters[4]);
    return pressed;
}
bool cvr::quest::ConsumeManualClueButton(bool pressed,bool allowed) {
    std::lock_guard lock(clueMutex);
    return clueInput.Button(pressed,allowed && Gameplay(),GetTickCount64());
}
void cvr::quest::SuspendManualClueInput() {
    std::lock_guard lock(clueMutex);clueInput.Suspend();cluePlayer={};
}
extern "C" void CvrStoryAttentionAfter(uintptr_t user,float* transform) {
    if(!Gameplay())return;
    cvr::quest::AttentionContext sample;
    {std::lock_guard lock(attentionMutex);sample=context;}
    // A free hand reaching for something turns the player's own ray onto it from the eyes (Natives/Reach.hpp).
    float point[3];
    const bool reaching=cvr::reach::AimPoint(point);
    if(sample.mode==cvr::quest::Attention::Native && !reaching)return;
    // Native +D1F2E0 locks the weak owner at user+10/+18 before QueueEvent;
    // user+8 is its EntityID. Compare all three, not proximity to the HMD.
    cvr::quest::ObjectKey owner{};uint64_t entity{};
    if(!ReadU64Safe(user+8,&entity) || !ReadPtrSafe(user+0x10,&owner.address) ||
       !ReadPtrSafe(user+0x18,&owner.reference))return;
    const auto player=cvr::roomscale::PlayerIdentity();
    if(!sample.Matches(owner,entity,player,GetTickCount64())) {
        uint64_t playerEntity{};
        if(reaching && player && owner.address==player && ReadU64Safe(player+0x48,&playerEntity) &&
           entity==playerEntity && cvr::reach::TurnToward(transform,point))
            CVR_DIAGNOSTIC(++CyberpunkVR_ReachTurns);
        return;
    }
    CVR_DIAGNOSTIC(++CyberpunkVR_StoryAttentionCounters[0]);
    cvr::camera::MainAimPose pose{};
    if(!cvr::camera::ReadMainAimPose(pose)) {CVR_DIAGNOSTIC(++CyberpunkVR_StoryAttentionCounters[3]);return;}
    std::memcpy(transform,pose.position,sizeof(pose.position));transform[3]=1.f;
    std::memcpy(transform+4,pose.rotation,sizeof(pose.rotation));
    CVR_DIAGNOSTIC(++CyberpunkVR_StoryAttentionCounters[static_cast<uint32_t>(sample.mode)]);
}
