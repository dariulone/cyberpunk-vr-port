#include "Stereo/ScopeRrOptions.hpp"
#include <cassert>
#include <cstring>
#include <iostream>
int main() {
    using namespace cvr::scope_dlss;
    sl::DLSSDOptions original{};
    original.mode=sl::DLSSMode::eUltraPerformance;
    original.outputWidth=original.outputHeight=341;
    original.worldToCameraView[0].x=7;
    original.cameraViewToWorld[3].z=9;
    auto scope=original;
    assert(MatchRrMode(kViewport,scope,341,341,341,341));
    assert(scope.mode==sl::DLSSMode::eDLAA);
    // No options other than mode may change, including matrices and presets.
    scope.mode=original.mode;
    assert(std::memcmp(&scope,&original,sizeof(scope))==0);
    auto unchanged=[&](uint32_t vp,uint32_t iw,uint32_t ih,uint32_t ow,uint32_t oh){
        auto value=original;
        assert(!MatchRrMode(vp,value,iw,ih,ow,oh));
        assert(std::memcmp(&value,&original,sizeof(value))==0);
    };
    unchanged(0,341,341,341,341); // MAIN never changes
    unchanged(1,341,341,341,341); // VRCAM never changes
    unchanged(kViewport,114,114,341,341); // real upscale preserves mode
    unchanged(kViewport,341,341,1024,1024);
    unchanged(kViewport,341,340,341,341);
    unchanged(kViewport,0,0,0,0);
    original.structVersion=4;unchanged(kViewport,341,341,341,341);
    original.structVersion=3;original.mode=sl::DLSSMode::eOff;unchanged(kViewport,341,341,341,341);
    std::cout<<"PASS RR scope 1:1 correction; MAIN/VRCAM, true upscale, matrices, presets and disabled/future options preserved\n";
}
