// Compile the real observer implementation with local hook-install stubs. This
// executable neither attaches to a game nor calls a game address/MinHook patch.
#include "../../src/Natives/DynamicTextureOrigins.cpp"
#include <array>
#include <cstdlib>
#include <iostream>

namespace cvr::game_build {bool Supported(){return false;}bool Overridden(){return false;}}
namespace cvr::hooks {
Hook::Hook(const char* n,Stage s,int o,InstallFn f,EnabledFn g)
    :name(n),stage(s),order(o),install(f),enabled(g),next(nullptr),installed(false){}
}
extern "C" MH_STATUS WINAPI MH_CreateHook(LPVOID,LPVOID,LPVOID*){return MH_ERROR_UNSUPPORTED_FUNCTION;}
extern "C" MH_STATUS WINAPI MH_EnableHook(LPVOID){return MH_ERROR_UNSUPPORTED_FUNCTION;}
extern "C" MH_STATUS WINAPI MH_RemoveHook(LPVOID){return MH_ERROR_NOT_CREATED;}
namespace {
std::array<uint8_t,0x30> objectBytes;
unsigned calls{};
const Descriptor* passed{};
Handle* __fastcall FakeFactory(Handle* output,const Descriptor* descriptor){
    ++calls;passed=descriptor;*output={reinterpret_cast<uintptr_t>(objectBytes.data()),0x12345678};return output;
}
void Check(bool ok,const char* why){if(!ok){std::cerr<<why<<'\n';std::abort();}}
}
int main(){
    original=&FakeFactory;Descriptor descriptor{768,1024,0,1,6,0x11,0,0};Handle output;
    const uint64_t serial=0xABCDEF;std::memcpy(objectBytes.data()+0x28,&serial,sizeof(serial));
    Check(Create(&output,&descriptor)==&output && passed==&descriptor && calls==1,"disabled observer changed original ABI");
    Check(CyberpunkVR_DynamicTextureOriginsObserved==0 && CyberpunkVR_DynamicTextureOriginsSequence==0,"disabled call performed observer work");
    CyberpunkVR_DynamicTextureOriginsEnable=1;
    Check(Create(&output,&descriptor)==&output && calls==2,"enabled observer changed factory return");
    const auto& row=CyberpunkVR_DynamicTextureOriginsRows[0];
    Check(row.object==output.object && row.control==output.control && row.objectSerial==serial && row.descriptor.width==768 && row.descriptor.height==1024 && row.descriptor.flags==0x11,"native output/descriptor identity not preserved");
    Check(row.caller && row.stackCount && CyberpunkVR_DynamicTextureOriginsCount==1 && CyberpunkVR_DynamicTextureOriginsInFlight==0,"origin publication incomplete");
    recording=true;Check(Create(&output,&descriptor)==&output && calls==3,"reentry did not forward original");recording=false;
    Check(CyberpunkVR_DynamicTextureOriginsCount==1 && CyberpunkVR_DynamicTextureOriginsObserved==2 && CyberpunkVR_DynamicTextureOriginsDroppedReentrant==1 && CyberpunkVR_DynamicTextureOriginsInFlight==0,"reentry coverage not accounted");
    Check(!Install() && CyberpunkVR_DynamicTextureOriginsStatus==2,"unsupported diagnostic install was not refused");
    Check(original==&FakeFactory,"optional install failure damaged existing forwarding");
    std::cout<<"PASS real observer ABI forwarding, disabled path, output identity, reentry and local install refusal\n";
}
