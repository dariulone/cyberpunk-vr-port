#include "Runtimes/CyberFingerGameMenu.hpp"
#include "Runtimes/CyberFingerUi.hpp"
#include "Render/DesktopMirror.hpp"
#include <stdexcept>
#include <iostream>
HWND g_gameHwnd{};bool cfEnabled=true;
namespace cvr::cyberfinger { MenuContext testContext{};bool Enabled(){return cfEnabled;} MenuContext Context(){return testContext;} }
bool GetDesktopPhysicalClientRect(HWND,RECT&,POINT&){return false;}
void Check(bool v,const char* message){if(!v)throw std::runtime_error(message);}
int main()try {
    using namespace cvr::cfmenu;
    cvr::vrui::Tracking t;t.valid=true;t.cyberfinger=true;t.preferLeft=true;
    t.head={{0,0,0,1},{0,0,0}};
    for(auto& h:t.hands){h.valid=true;h.aim={{0,0,0,1},{0,0,-.2f}};}
    const XrPosef panel{{0,0,0,1},{0,0,-2}};
    auto frame=[&]{Hands(t,true);Panel(panel,2,1);return Current();};
    t.hands[0].trigger=1;
    auto state=frame();Check(state.active && state.hit && !state.down,"held trigger on entry clicked");
    t.hands[0].trigger=0;frame();t.hands[0].trigger=1;
    Check(frame().down,"left trigger did not select ray target");
    t.hands[0].valid=false;Check(!frame().down,"lost hand retained mouse button");
    t.hands[0].valid=true;Check(!frame().down,"reacquiring held trigger clicked");
    t.hands[0].trigger=0;frame();
    t.hands[1].aim.position.x=.8f;t.hands[1].trigger=1;
    state=frame();Check(state.down && std::abs(state.u-.9f)<.001f,"other hand trigger did not claim full-width menu");
    t.hands[1].trigger=0;frame();
    t.hands[0].stickY=1;state=frame();Check(state.focus && !state.hit && state.buttons==1,"left stick did not claim directional focus");
    t.hands[0].stickY=0;frame();t.hands[0].trigger=1;
    state=frame();Check(state.focus && state.buttons==0x1000 && !state.down,"trigger focus activation also generated mouse press");
    t.hands[0].trigger=0;frame();t.hands[1].aim.position.x=.4f;
    state=frame();Check(state.hit && !state.focus,"ray movement did not restore pointer");
    // Start neutral: held entry controls never become native actions.
    Reset();t={};t.valid=true;t.cyberfinger=true;t.preferLeft=true;
    for(auto& h:t.hands){h.valid=true;h.aim={{0,0,0,1},{0,0,-.2f}};}
    frame();
    t.hands[1].grip=1;state=frame();Check(state.buttons==0x4000,"physical grab did not generate X");
    Check(frame().buttons==0x4000,"holding Grip did not keep X held");
    t.hands[1].grip=0;Check(frame().buttons==0,"releasing Grip retained X");
    t.menuLeft=true;Check(frame().buttons==0,"Menu fired before release");
    t.menuLeft=false;Check(frame().buttons==0x2000,"Menu release did not generate Back");
    Reset();frame();t.menuRight=true;Check(frame().buttons==0,"right Menu fired before release");
    t.menuRight=false;Check(frame().buttons==0x2000,"right Menu did not generate Back");
    // Both-Menu is reserved for the VR overlay, with no native Back on release.
    Reset();frame();t.menuLeft=true;frame();t.menuRight=true;Check(frame().buttons==0,"VR chord generated Back");
    t.menuLeft=false;Check(frame().buttons==0,"partial chord release generated Back");
    t.menuRight=false;Check(frame().buttons==0,"VR chord release generated Back");
    Reset();t.menuLeft=true;t.hands[0].grip=1;t.hands[1].grip=1;
    Check(frame().buttons==0,"entry with held Menu/Grip generated native actions");
    t.menuLeft=false;t.hands[0].grip=t.hands[1].grip=0;
    Check(frame().buttons==0,"releasing entry controls generated Back");
    // Either trigger remains A when no laser intersects the menu.
    for(auto& h:t.hands)h.aim.position.x=5;
    frame();t.hands[1].trigger=1;
    state=frame();Check(state.buttons==0x1000 && !state.down,"trigger without ray target did not generate A");
    Hands(t,false);Check(!Current().active,"closed menu retained pointer ownership");
    using cvr::cyberfinger::MenuContext;using cvr::cyberfinger::testContext;
    for(bool left:{false,true}) {
        Reset();t={};t.cyberfinger=t.valid=true;t.preferLeft=left;
        const int dominant=left?0:1,other=1-dominant;
        for(auto& h:t.hands){h.valid=true;h.aim={{0,0,0,1},{0,0,-.2f}};}
        testContext=MenuContext::Map;frame();
        t.hands[other].trigger=1;state=frame();
        Check(state.buttons==0x0040 && !state.down,"map off-hand trigger clicked instead of jump to V");
        t.hands[other].trigger=0;frame();
        t.hands[dominant].grip=1;state=frame();
        Check(state.secondary && !(state.buttons&0x4000),"map grip was not right-click");
        t.hands[dominant].grip=0;t.hands[other].grip=1;
        Check(frame().buttons&0x8000,"map other Grip did not open journal");
        t.hands[dominant].stickY=.8f;state=frame();
        Check(state.zoom>.79f && !state.focus,"dominant map stick navigated instead of zooming");
        t.hands[other].grip=0;t.hands[1].stickX=.8f;
        state=frame();Check((state.buttons&0x0200)!=0 && state.zoom>.79f,"map disabled right-stick next tab or lost zoom");
        t.hands[1].stickX=0;frame();t.hands[1].stickX=-.8f;
        Check(frame().buttons&0x0100,"map disabled right-stick previous tab");
        testContext=MenuContext::Journal;t.hands[other].trigger=1;
        Check(frame().buttons==0,"context change replayed held trigger or Grip");
        t.hands[other].trigger=0;frame();t.hands[other].trigger=1;
        Check(frame().buttons==0x0080,"journal Tab did not sort");
    }
    testContext=MenuContext::PictureFrame;frame();
    auto pose=PanelPose(panel,1);auto moved=panel;moved.position.x=3;
    Check(PanelPose(moved,1).position.x==pose.position.x,"picture frame followed head movement");
    Check(PanelPose(moved,2).position.x==3,"recenter retained stale panel origin");
    testContext=MenuContext::General;frame();
    t.cyberfinger=false;frame();Check(!Current().active,"non-CF controller entered custom menu path");
    t.cyberfinger=true;frame();cfEnabled=false;Check(!Current().active,"inactive CF binding retained pointer ownership");
    // Dispatch deliberately has no foreground game/window and cannot insert input.
    DispatchPointer();
    std::cout<<"CyberFinger game menu bridge passed\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
