#include "Runtimes/CapturedPresentation.hpp"
#include "Runtimes/LoadingPanel.hpp"
#include <iostream>
#include <stdexcept>
using namespace cvr::capture;
void Check(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
int main()try{
    auto handoff = LobbyReturn::Waiting;
    for (bool covered : {true, false, true, true}) handoff = ObserveReturn(handoff, covered);
    Check(handoff == LobbyReturn::Waiting, "one clear tick exposed the loading artwork to stereo");
    handoff = ObserveReturn(ObserveReturn(handoff, false), false);
    Check(handoff == LobbyReturn::Ready && ObserveReturn(handoff, true) == LobbyReturn::Ready,
          "a later menu or curtain rearmed the completed handoff");
    Check(ObserveReturn(LobbyReturn::Idle, true) == LobbyReturn::Idle, "booting the lobby started a return");
    // A load has its own stationary quad even when the native menu flag is raised.
    const auto loading=Classify(true,true,false,true);
    Check(loading==Presentation::Loading,"loading picture became a menu/dark curtain");
    Check(Classify(true,true,true,true)==Presentation::Notice,"boot notice lost its own pose mode");
    Check(Classify(false,false,false,true)==Presentation::Dark,"leaving-lobby curtain became gameplay");
    Check(Classify(true,false,false,false)==Presentation::Menu,"native menu lost its quad");
    Check(Classify(false,false,false,false)==Presentation::Scene,"gameplay lost stereo");

    const auto local = reinterpret_cast<XrSpace>(1);
    const auto view = reinterpret_cast<XrSpace>(2);
    XrSwapchainSubImage art{};
    art.swapchain = reinterpret_cast<XrSwapchain>(3);
    art.imageRect.extent = {1870, 2104};
    auto panel = cvr::loading::Panel(art, 16.f/9, local, view, nullptr, nullptr);
    Check(panel.space == view && panel.pose.position.z == -3, "startup without tracking");
    XrPosef head{{0, 0, 0, 1}, {1, 1.6f, 2}};
    panel = cvr::loading::Panel(art, 16.f/9, local, view, &head, &panel);
    Check(panel.space == local && panel.pose.position.z == -1 && panel.pose.position.y == head.position.y,
        "first valid locate did not anchor the loading art");
    Check(panel.eyeVisibility == XR_EYE_VISIBILITY_BOTH && panel.subImage.swapchain == art.swapchain &&
        panel.subImage.imageRect.extent.height == 2104 && std::abs(panel.size.width - 3.2f) < .001f,
        "mono artwork must cover the full quad at its design aspect");
    head.position.z += 2;
    panel = cvr::loading::Panel(art, 16.f/9, local, view, &head, &panel);
    Check(panel.pose.position.z == -1, "walking dragged the loading panel");
    const auto nextSpace = reinterpret_cast<XrSpace>(4);
    panel = cvr::loading::Panel(art, 16.f/9, nextSpace, view, &head, &panel);
    Check(panel.space == nextSpace && panel.pose.position.z == 1, "stale anchor after reference-space change");

    // Simulate a long load: a runtime tick keeps submitting the same pair
    // while the game stops presenting. It must not expire at one second.
    for(int second=0;second<=45;++second) {
        const int64_t age=int64_t(second)*1000000000;
        Check(CanHold(loading,age),"loading frame expired during a stall");
        Check(CanHold(Presentation::Notice,age),"startup notice expired");
        Check(CanHold(Presentation::Dark,age),"loading curtain expired");
        Check(CanHold(Presentation::Scene,age)==(second<=1),"gameplay stall timeout changed");
    }
    Check(!CanHold(loading,-1),"invalid display timestamp accepted");

    // Menu -> loading -> gameplay: an old eye can still be in the pool and
    // only one serial behind. It must never accompany the new kind of image.
    const Presentation kinds[]{Presentation::Scene,Presentation::Menu,Presentation::Loading,Presentation::Notice,Presentation::Dark};
    for(auto frame:kinds)for(auto eye:kinds) {
        Check(CanGenerate(frame,eye)==(frame==Presentation::Scene && eye==Presentation::Scene),
            "buffered frame generation crossed a menu/loading transition");
        Check(CanPair(frame,eye,102,102,3)==(frame==eye),"exact pair classification mismatch");
        Check(CanPair(frame,eye,102,101,3)==(frame==Presentation::Scene && eye==frame),"transition reused incompatible eye");
        Check(!CanPair(frame,eye,102,103,3),"future eye paired");
        Check(!CanPair(frame,eye,102,0,3),"unpublished eye paired");
    }
    Check(CanPair(Presentation::Scene,Presentation::Scene,102,99,3),"normal bounded gameplay reuse lost");
    Check(!CanPair(Presentation::Scene,Presentation::Scene,102,98,3),"stale gameplay eye reused");
    Check(!CanPair(Presentation::Scene,Presentation::Scene,102,101,0),"disabled reuse ignored");
    std::cout<<"Loading presentation, 45-second stall and cross-transition eye pairing passed\n";
    return 0;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
