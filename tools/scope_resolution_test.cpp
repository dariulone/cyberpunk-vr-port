#include "Stereo/ScopeResolution.hpp"
#include <cassert>
#include <iostream>
#include <stdexcept>
using namespace cvr::scope_dlss;
int main(){
    const uint64_t flags=kRrFeature|(1ull<<6)|(1ull<<34)|(1ull<<63)|1;
    assert(FullResolutionEligible(true,true,flags,1024,1024));
    assert(!FullResolutionEligible(false,true,flags,1024,1024));
    assert(!FullResolutionEligible(true,false,flags,1024,1024)); // MAIN/VRCAM
    assert(!FullResolutionEligible(true,true,flags&~kRrFeature,1024,1024)); // SR only
    assert(!FullResolutionEligible(true,true,flags,0,1024));
    assert(!FullResolutionEligible(true,true,flags,1024,8192));
    uint64_t viewFlags=flags;
    {
        NativeResolutionFlags native(viewFlags);
        assert(!(viewFlags&kResolutionScaleBits));
        assert(viewFlags==(flags&~kResolutionScaleBits)); // keep unrelated graph features
        { NativeResolutionFlags nested(viewFlags); }
        assert(viewFlags==(flags&~kResolutionScaleBits));
    }
    assert(viewFlags==flags); // RR is present again before the graph builds
    try {NativeResolutionFlags native(viewFlags);throw std::runtime_error("native");}
    catch(const std::runtime_error&){}
    assert(viewFlags==flags);
    // Every known scaler bit is suppressed, not only the RR selector. Other
    // upscaler selectors inherited by the view must not reselect MAIN's ratio.
    viewFlags=~uint64_t{};
    {NativeResolutionFlags native(viewFlags);assert(viewFlags==~kResolutionScaleBits);}
    assert(viewFlags==~uint64_t{});
    std::cout<<"PASS scope-only RR eligibility, unrelated flags, nested calls and unwind restoration\n";
}
