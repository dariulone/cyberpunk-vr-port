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
std::atomic<bool> offensiveMode{true};
std::atomic<uint64_t> fistsUntil{}, fistBlockUntil{};
std::atomic<uint64_t> conversationUntil{};
std::atomic<uint64_t> uiUntil{},interactionUntil{},lootUntil{};
std::atomic<uint64_t> subtitleUntil{};
// Pack the expiry and message together so the compositor sees one event.
std::atomic<uint64_t> modeNoticePacket{};
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
    const bool available=command!=Command::DrawFists || OffensiveMode();
    const auto result=Enabled() && available && GetTickCount64()<commandUntil?command:Command::None;
    command=Command::None;commandUntil=0;return result;
}
bool OffensiveMode(){return offensiveMode.load(std::memory_order_relaxed);}
void SetOffensiveMode(bool enabled){offensiveMode.store(enabled,std::memory_order_relaxed);}
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
bool NoteFistsEquipped(bool equipped) {
    const auto now=GetTickCount64();
    fistsUntil.store(equipped ? now+150 : 0);
    if (!equipped) fistBlockUntil.store(0);
    return equipped && now<fistBlockUntil.load();
}
bool FistsEquipped() { return GetTickCount64()<fistsUntil.load(); }
void SetFistBlock(bool active) { fistBlockUntil.store(active ? GetTickCount64()+150 : 0); }

void RevealModeNotice(ModeNotice notice) {
    if (Enabled()) modeNoticePacket.store(((GetTickCount64()+2000) << 2) | static_cast<unsigned>(notice));
}
const char* ModeNoticeText() {
    const auto packet = modeNoticePacket.load();
    if (GetTickCount64() >= (packet >> 2)) return nullptr;
    switch (static_cast<ModeNotice>(packet & 3)) {
        case ModeNotice::Off: return "Offensive mode OFF";
        case ModeNotice::On: return "Offensive mode ON";
        case ModeNotice::Holstering: return "Putting weapons away...";
        default: return nullptr;
    }
}
const char* HudRevealReason(){
    if(!Enabled() || !HudHidden())return nullptr;
    if(GetTickCount64()<(modeNoticePacket.load() >> 2))return "offensive mode";
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
void ConversationInput(bool enabled,float rightY,float leftTrigger,float rightTrigger,bool driverClick) {
    std::lock_guard lock(mutex);
    const bool active=enabled && ConversationActive();
    const int action=navigation.Step(active,rightY,leftTrigger,rightTrigger,driverClick,GetTickCount64());
    if(!active)pending.store(0);
    else if(action){int empty=0;pending.compare_exchange_strong(empty,action);}
}
}
