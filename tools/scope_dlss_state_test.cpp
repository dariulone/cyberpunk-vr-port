#include "Stereo/ScopeDlssState.hpp"
#include <sl_core_types.h>
#include <cassert>
#include <iostream>
#include <stdexcept>
using namespace cvr::scope_dlss;

int main() {
    std::array<uint8_t,0x4b0> native{};
    for(size_t i=0;i<native.size();++i) native[i]=uint8_t(i*13+7);
    static_assert(sizeof(sl::BaseStructure)==32);
    sl::ViewportHandle initial(0u);
    std::memcpy(native.data()+0x480,&initial,sizeof(initial));
    Write(native.data(),kToken,uint64_t(37));
    const auto main=native;
    State scope; scope.Select(0x1000,0x2000); scope.PrepareFrame(37);
    assert(scope.reset);
    {
        Swap swap(native.data(),scope);
        assert(Read<uint32_t>(native.data(),kViewportOffset)==kViewport);
        sl::ViewportHandle actual(0u);
        std::memcpy(&actual,native.data()+0x480,sizeof(actual));
        assert(uint32_t(actual)==kViewport);
        assert(actual.structVersion==sl::kStructVersion1);
        assert(actual.structType==sl::ViewportHandle::s_structType);
        assert(Read<uint64_t>(native.data(),kToken)==UINT64_MAX);
        assert(native[0x43b]==0 && native[0x43c]==0); // no scope FG/Reflex options
        for(size_t i=kCache;i<kOptions;++i) assert(native[i]==0);
        native[kCamera]=42; // camera writer
        scope.constants=true;
    }
    assert(native==main);
    for(uint32_t frame=37;frame<537;++frame) {
        scope.PrepareFrame(frame);
        assert(!scope.reset);
        {
            Swap swap(native.data(),scope);
            assert(native[kCamera]==42);
            // Configure tiny scope, retaining independent cached dimensions.
            if(frame>37) assert(Read<uint32_t>(native.data(),0x3e0)==341);
            Write(native.data(),0x3d0,114u);
            Write(native.data(),0x3e0,341u);
            native[kOptions]=0; // native configure may change global options
        }
        assert(native==main); // MAIN dimensions, camera, options, token, identity
    }
    // A menu quality change invalidates the scope cache, without losing camera.
    native[kOptions]^=1;
    const auto changed=native;
    {
        Swap swap(native.data(),scope);
        assert(native[kCamera]==42);
        assert(Read<uint32_t>(native.data(),0x3e0)==0);
        native[kCamera]=43;
    }
    assert(native==changed);
    // C++ unwinding restores native state as well.
    try { Swap swap(native.data(),scope); native[kCamera]=44; throw std::runtime_error("test"); }
    catch(const std::runtime_error&) {}
    assert(native==changed);
    scope.PrepareFrame(900); assert(scope.reset); // holster/load gap
    scope.PrepareFrame(1); assert(scope.reset); // frame clock reset
    scope.frame=UINT32_MAX; scope.PrepareFrame(0); assert(!scope.reset);
    scope.Select(0x1000,0x3000); assert(!scope.constants && scope.reset);
    scope.constants=true; scope.Select(0x4000,0x3000); assert(!scope.constants);
    std::cout<<"PASS: 500 alternating scope/main frames, configuration changes, history gaps, owner replacement, restoration on unwind\n";
}
