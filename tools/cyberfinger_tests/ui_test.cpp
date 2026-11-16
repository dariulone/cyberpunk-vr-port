#include "Runtimes/CyberFingerUi.hpp"
#include "Runtimes/CyberFingerMenuPolicy.hpp"
#include "Runtimes/CyberFingerInput.hpp"
#include <atomic>
#include <chrono>
#include <thread>
#include <iostream>
#include <stdexcept>
std::atomic<int> g_sceneTier{};bool cfEnabled=true;
namespace cvr::cyberfinger {bool Enabled(){return cfEnabled;}}
void Check(bool v,const char* message){if(!v)throw std::runtime_error(message);}
int main()try {
    using namespace cvr::cyberfinger;
    RequestCommand(Command::Inventory);
    std::this_thread::sleep_for(std::chrono::milliseconds(35));
    Check(TakeCommand()==Command::Inventory && TakeCommand()==Command::None,"menu request lost across XR/game ticks or repeated");
    RequestCommand(Command::Cyberware);
    Check(TakeCommand()==Command::Cyberware,"dedicated cyberware action lost");
    g_sceneTier=2;RequestCommand(Command::Cyberware);
    Check(TakeCommand()==Command::None,"cyberware activated during a restricted scene");g_sceneTier=0;
    RequestCommand(Command::Pause);cfEnabled=false;
    Check(TakeCommand()==Command::None,"menu request leaked to another controller");cfEnabled=true;
    Check(TakeCommand()==Command::None,"disabled request replayed after reconnect");
    RequestCommand(Command::Inventory);std::this_thread::sleep_for(std::chrono::milliseconds(520));
    Check(TakeCommand()==Command::None,"stale menu request replayed after a load");
    struct State {
        uint16_t buttons=0x1000,cyberFingerButtons=0;
        float leftThumbX=.4f,leftThumbY=.8f,rightThumbX=-.7f,rightThumbY=1.f;
        float leftTrigger=.9f,rightTrigger=1.f,leftGrip=.2f,rightGrip=.3f;
        bool leftHandValid=true,rightHandValid=true;
    } raw;
    ConversationCapture capture;
    capture.Step(true,true,raw.rightThumbY,raw.rightTrigger);
    const auto captured=capture.Apply(raw);
    Check(captured.leftThumbX==raw.leftThumbX && captured.leftThumbY==raw.leftThumbY &&
          captured.rightThumbX==raw.rightThumbX && captured.buttons==raw.buttons,"dialogue blocked walking, turning or jump");
    Check(captured.rightThumbY==0 && captured.rightTrigger==0 && captured.leftTrigger==raw.leftTrigger,
          "dialogue failed to reserve only right vertical/trigger input");
    for(bool leftHanded:{false,true}) {
        const auto game=ForGame(captured,leftHanded,0);
        Check(game.leftThumbY==raw.leftThumbY && game.rightThumbX==raw.rightThumbX && game.rightThumbY==0,
              "handedness stole movement or reintroduced dash/crouch");
        Check(leftHanded?(game.leftTrigger==0 && game.rightTrigger==raw.leftTrigger):
                          (game.rightTrigger==0 && game.leftTrigger==raw.leftTrigger),"confirmation leaked to aim/fire");
        struct Pad {int sThumbLX=12000,sThumbLY=24000,sThumbRX=-20000,sThumbRY=30000;
            int bLeftTrigger=230,bRightTrigger=255,wButtons=0x1000;} pad;
        capture.ApplyGamepad(pad,leftHanded,false);
        Check(pad.sThumbLX==12000 && pad.sThumbLY==24000 && pad.sThumbRX==-20000 &&
              pad.sThumbRY==0 && pad.wButtons==0x1000,"final output blocked movement/turning/jump");
        Check(leftHanded?(pad.bLeftTrigger==0 && pad.bRightTrigger==255):
                         (pad.bRightTrigger==0 && pad.bLeftTrigger==230),"final weapon override escaped confirmation capture");
    }
    capture.Step(true,false,1,1);Check(capture.vertical && capture.trigger,"dialogue exit replayed held controls");
    capture.Step(true,false,0,1);Check(!capture.vertical && capture.trigger,"axis release failed or released trigger claim");
    capture.Step(true,false,0,0);Check(!capture.trigger,"released trigger stayed claimed");
    capture.Step(true,true,1,1);capture.Step(false,false,1,1);
    Check(capture.Apply(raw).rightThumbY==1 && capture.Apply(raw).rightTrigger==1,"inactive CF profile changed other controllers");
    Check(HudVisible(),"HUD starts hidden");ToggleHud();Check(!HudVisible(),"HUD toggle missing");
    UiTick(MenuContext::General,true);Check(HudVisible() && InteractionActive() && !ConversationActive(),"prompt did not reveal HUD independently of dialogue");
    UiTick(MenuContext::General,false);Check(!HudVisible(),"prompt lost hidden preference");
    LootTick(true);Check(HudVisible() && InteractionActive(),"loot did not reveal HUD");LootTick(false);
    for(bool handed:{false,true})for(int layout:{0,1,2}) {
        Check(MouseHand(handed,layout)==(handed && layout==2?0:1),"mouse hand conflicts with movement stick");
    }
    InteractionButtons actions;
    Check(!actions.Step(true,true,1,true),"held entry interaction leaked");
    actions.Step(true,true,0,false);
    Check(actions.Step(true,true,1,true)==0xc000,"take/use not X/Y");
    Check(actions.Step(true,true,1,true)==0xc000,"hold interaction lost");
    Check(!actions.Step(true,false,1,true) && actions.claimTrigger && actions.claimGrip,"interaction exit leaked held aim/melee");
    actions.Step(true,false,0,false);Check(!actions.claimTrigger && !actions.claimGrip,"release retained utility claim");
    ConversationTick(true);Check(HudVisible() && ConversationActive(),"dialogue failed to reveal HUD");
    ConversationInput(true,0,0);ConversationInput(true,-1,0);
    Check(ConversationTick(true)==2 && ConversationTick(true)==0,"choice scroll lost or duplicated");
    ConversationInput(true,0,1);Check(ConversationTick(true)==3,"confirm not delivered");
    ConversationInput(true,0,1);Check(ConversationTick(true)==0,"held trigger repeated confirm");
    ConversationTick(false);Check(!ConversationActive() && !HudVisible(),"dialogue exit lost hidden HUD preference");
    for(int tier:{1,2,3}) {
        g_sceneTier=tier;Check(!HudVisible() && !HudRevealReason(),"staged gameplay forced the hidden HUD visible");
        SubtitleTick(true);Check(HudVisible() && HudRevealReason() && !ConversationActive(),"speech without choices did not reveal HUD independently of controls");
        SubtitleTick(false);Check(!HudVisible(),"subtitle exit lost hidden preference");
    }
    SubtitleTick(true);std::this_thread::sleep_for(std::chrono::milliseconds(270));
    Check(!HudVisible(),"stalled subtitle producer kept HUD visible");
    g_sceneTier=4;Check(HudVisible(),"cinematic failed to reveal HUD");g_sceneTier=0;
    SetHudHidden(false);Check(!HudHidden() && HudVisible(),"F10 Show did not change preference");
    SetHudHidden(true);Check(HudHidden() && !HudVisible(),"F10 Hide did not change preference");
    ConversationTick(true);ConversationInput(false,0,0);ConversationInput(true,0,0);
    ConversationInput(true,0,1);
    std::this_thread::sleep_for(std::chrono::milliseconds(270));
    Check(!ConversationActive() && ConversationTick(true)==0,"stale queued confirmation replayed");
    ConversationTick(false);cfEnabled=false;
    Check(HudVisible() && !ConversationActive(),"CF hidden state affected another controller");
    ToggleHud();SetHudHidden(false);cfEnabled=true;Check(!HudVisible(),"inactive profile toggled stored HUD preference");
    std::cout<<"CyberFinger HUD and conversation state passed\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
