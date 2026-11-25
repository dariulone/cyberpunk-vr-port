#include "Runtimes/CyberFingerInput.hpp"
#include "Runtimes/CyberFingerUi.hpp"
#include "Core/Paths.hpp"
#include <fstream>
#include <chrono>
#include <thread>
#include <stdexcept>
#include <iostream>
void Log(const char*,...){}
namespace cvr::paths { std::string FileA(Folder,const char*){return "binding-activity-fixture.ini";} }
void Check(bool v,const char* message){if(!v)throw std::runtime_error(message);}
int main()try {
    using namespace cvr::cyberfinger;
    {std::ofstream f("binding-activity-fixture.ini");f<<"enabled=1\n";}
    Configure();Check(Configured() && !Enabled(),"config alone changed other controllers");
    Check(StickActionThreshold()==.8f,"existing INI did not receive CF stick default");
    Check(HudScale()==1.f,"inactive profile resized another controller's HUD");
    NoteBindingActivity(false);Check(!Enabled(),"inactive named actions enabled CF path");
    NoteBindingActivity(true);Check(Enabled(),"active CF binding did not enable profile");
    Check(HudScale()==1.25f,"CF default HUD size missing");
    NoteBindingActivity(false);Check(!Enabled(),"controller disconnect retained CF overrides");
    NoteBindingActivity(true);std::this_thread::sleep_for(std::chrono::milliseconds(520));
    Check(!Enabled(),"stalled XR input retained CF overrides");
    {std::ofstream f("binding-activity-fixture.ini");f<<"enabled=0\nstick_action_threshold=0.75\n";}
    Configure();NoteBindingActivity(true);Check(!Configured() && !Enabled(),"disabled profile acted on active binding");
    Check(StickActionThreshold()==.75f,"configured stick threshold was not loaded");
    std::remove("binding-activity-fixture.ini");
    std::cout<<"CyberFinger binding activity gate passed\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
