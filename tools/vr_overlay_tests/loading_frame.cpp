#include "Runtimes/CapturedPresentation.hpp"
#include <iostream>
#include <stdexcept>
using namespace cvr::capture;
void Check(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
int main()try{
    // A loading image already has the stationary panel drawn in stereo. It
    // must remain a projection even if the native menu flag is still raised.
    const auto loading=Classify(true,true,false,true);
    Check(loading==Presentation::Loading,"loading picture became a menu/dark curtain");
    Check(Classify(true,true,true,true)==Presentation::Notice,"boot notice lost its own pose mode");
    Check(Classify(false,false,false,true)==Presentation::Dark,"leaving-lobby curtain became gameplay");
    Check(Classify(true,false,false,false)==Presentation::Menu,"native menu lost its quad");
    Check(Classify(false,false,false,false)==Presentation::Scene,"gameplay lost stereo");

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
