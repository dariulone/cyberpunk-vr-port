#include "Runtimes/CyberFingerUi.hpp"
#include "Runtimes/CyberFingerMenuPolicy.hpp"
#include "Runtimes/CyberFingerInput.hpp"
#include <windows.h>
#include <atomic>
#include <mutex>
extern std::atomic<int> g_sceneTier;
namespace cvr::cyberfinger {
namespace {
std::atomic<bool> hidden{};
std::atomic<uint64_t> conversationUntil{};
std::atomic<uint64_t> uiUntil{},interactionUntil{},lootUntil{};
std::atomic<uint64_t> subtitleUntil{};
std::atomic<MenuContext> menuContext{};
std::atomic<int> pending{};
std::mutex mutex;
ConversationNavigation navigation;
Command command{};uint64_t commandUntil{};
}
void RequestCommand(Command value) {
    std::lock_guard lock(mutex);
    command=Enabled() && (value!=Command::Cyberware || g_sceneTier.load(std::memory_order_relaxed)<=1)?value:Command::None;
    commandUntil=GetTickCount64()+500;
}
Command TakeCommand() {
    std::lock_guard lock(mutex);
    const auto result=Enabled() && GetTickCount64()<commandUntil?command:Command::None;
    command=Command::None;commandUntil=0;return result;
}
void ToggleHud(){if(Enabled())hidden.store(!hidden.load());}
void SetHudHidden(bool value){if(Enabled())hidden.store(value);}
bool HudHidden(){return hidden.load();}
bool ConversationActive(){return Enabled() && GetTickCount64()<conversationUntil.load(std::memory_order_acquire);}
MenuContext Context(){return Enabled() && GetTickCount64()<uiUntil.load(std::memory_order_acquire)?menuContext.load():MenuContext::General;}
bool InteractionActive(){const auto now=GetTickCount64();return Enabled() && (now<interactionUntil.load() || now<lootUntil.load());}
void UiTick(MenuContext context,bool interaction) {
    const auto now=GetTickCount64();menuContext.store(context);
    uiUntil.store(Enabled()?now+250:0,std::memory_order_release);
    interactionUntil.store(Enabled() && interaction?now+250:0);
}
void LootTick(bool visible){lootUntil.store(Enabled() && visible?GetTickCount64()+250:0);}
void SubtitleTick(bool visible){subtitleUntil.store(Enabled() && visible?GetTickCount64()+250:0);}
const char* HudRevealReason(){
    if(!Enabled() || !HudHidden())return nullptr;
    if(ConversationActive())return "dialogue choices";
    if(GetTickCount64()<subtitleUntil.load())return "subtitles";
    if(InteractionActive())return "interaction / loot";
    if(Context()==MenuContext::PictureFrame)return "picture frame";
    // Tiers 2/3 are staged/limited GAMEPLAY, not proof of an active conversation.
    // Visible choices/subtitles cover speech there; only 4/5 are cinematics.
    return g_sceneTier.load(std::memory_order_relaxed)>=4?"cinematic":nullptr;
}
bool HudVisible(){return !Enabled() || !HudHidden() || HudRevealReason()!=nullptr;}
int ConversationTick(bool visible) {
    if(!Enabled() || !visible){conversationUntil.store(0,std::memory_order_release);pending.store(0);return 0;}
    const auto now=GetTickCount64();
    if(now>=conversationUntil.load(std::memory_order_acquire))pending.store(0);
    conversationUntil.store(now+250,std::memory_order_release);
    return pending.exchange(0,std::memory_order_acq_rel);
}
void ConversationInput(bool enabled,float rightY,float rightTrigger) {
    std::lock_guard lock(mutex);
    const bool active=enabled && ConversationActive();
    const int action=navigation.Step(active,rightY,rightTrigger,GetTickCount64());
    if(!active)pending.store(0);
    else if(action){int empty=0;pending.compare_exchange_strong(empty,action);}
}
}
