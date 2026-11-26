#define CVR_DYNAMIC_TEXTURE_ORIGINS_EXPORTS
#include "Natives/DynamicTextureOrigins.hpp"
#include "Core/GameBuild.hpp"
#include "Hooks/Hook.hpp"
#include "MinHook.h"
#include <windows.h>
#include <intrin.h>
#include <cstring>

using cvr::texture_origins::Capacity;
using cvr::texture_origins::StackDepth;
extern "C" {
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DynamicTextureOriginsEnable{0};
__declspec(dllexport) std::atomic<uint32_t> CyberpunkVR_DynamicTextureOriginsStatus{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsSequence{0},CyberpunkVR_DynamicTextureOriginsCount{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsObserved{0},CyberpunkVR_DynamicTextureOriginsInFlight{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsDroppedReentrant{0},CyberpunkVR_DynamicTextureOriginsDroppedBusy{0};
__declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_DynamicTextureOriginsOverwritten{0};
__declspec(dllexport) uint64_t CyberpunkVR_DynamicTextureOriginsQpcFrequency{};
__declspec(dllexport) uint32_t CyberpunkVR_DynamicTextureOriginsLayout[8]={1,Capacity,sizeof(cvr::texture_origins::Record),48,76,88,StackDepth,80};
__declspec(dllexport) cvr::texture_origins::Record CyberpunkVR_DynamicTextureOriginsRows[Capacity]{};
}

namespace {
using namespace cvr::texture_origins;
using CreateFn=Handle*(__fastcall*)(Handle*,const Descriptor*);
CreateFn original{};
thread_local bool recording=false;
Writer writer(CyberpunkVR_DynamicTextureOriginsRows,Capacity,CyberpunkVR_DynamicTextureOriginsSequence,
    CyberpunkVR_DynamicTextureOriginsCount,CyberpunkVR_DynamicTextureOriginsDroppedBusy,CyberpunkVR_DynamicTextureOriginsOverwritten);

__declspec(noinline) Handle* Observe(Handle* output,const Descriptor* descriptor,uintptr_t caller) {
    CyberpunkVR_DynamicTextureOriginsInFlight.fetch_add(1,std::memory_order_relaxed);
    struct Flight {~Flight(){CyberpunkVR_DynamicTextureOriginsInFlight.fetch_sub(1,std::memory_order_relaxed);}} flight;
    // The original function executes before any publication lock. A nested
    // native creation is legitimate; the guard covers observer work only.
    const Descriptor input=descriptor?*descriptor:Descriptor{};
    Handle* result=original(output,descriptor);
    CyberpunkVR_DynamicTextureOriginsObserved.fetch_add(1,std::memory_order_relaxed);
    if(recording){CyberpunkVR_DynamicTextureOriginsDroppedReentrant.fetch_add(1,std::memory_order_relaxed);return result;}
    recording=true;
    struct Reentry {~Reentry(){recording=false;}} reentry;
    Record row{};row.descriptor=input;row.caller=caller;row.thread=GetCurrentThreadId();
    if(descriptor)row.flags|=1;
    if(output){row.object=output->object;row.control=output->control;row.flags|=2;}
    if(result==output)row.flags|=4;
    if(row.object){
        // ISerializable's process-unique serial, initialized by6263D0. The
        // returned Handle still belongs to the caller; no AddRef is necessary.
        std::memcpy(&row.objectSerial,reinterpret_cast<const void*>(row.object+0x28),sizeof(row.objectSerial));
        row.flags|=8;
    }
    LARGE_INTEGER now{};QueryPerformanceCounter(&now);row.qpc=static_cast<uint64_t>(now.QuadPart);
    void* frames[StackDepth]{};
    row.stackCount=CaptureStackBackTrace(1,StackDepth,frames,nullptr);
    for(uint32_t i=0;i<row.stackCount;++i)row.stack[i]=reinterpret_cast<uintptr_t>(frames[i]);
    writer.Publish(row);
    return result;
}

Handle* __fastcall Create(Handle* output,const Descriptor* descriptor) {
    if(!CyberpunkVR_DynamicTextureOriginsEnable.load(std::memory_order_relaxed))return original(output,descriptor);
    return Observe(output,descriptor,reinterpret_cast<uintptr_t>(_ReturnAddress()));
}

bool Install() {
    if(!cvr::game_build::Supported() || cvr::game_build::Overridden()){
        CyberpunkVR_DynamicTextureOriginsStatus=2;return false;
    }
    const auto base=reinterpret_cast<uintptr_t>(GetModuleHandleW(nullptr));
    auto* target=reinterpret_cast<void*>(base+0x5EA8A4);
    constexpr uint8_t signature[]={0x48,0x89,0x5c,0x24,0x08,0x48,0x89,0x74,0x24,0x10,0x57,0x48,0x83,0xec,0x30,0x48};
    if(std::memcmp(target,signature,sizeof(signature))){CyberpunkVR_DynamicTextureOriginsStatus=3;return false;}
    if(MH_CreateHook(target,reinterpret_cast<void*>(&Create),reinterpret_cast<void**>(&original))!=MH_OK){
        CyberpunkVR_DynamicTextureOriginsStatus=4;return false;
    }
    LARGE_INTEGER frequency{};QueryPerformanceFrequency(&frequency);
    CyberpunkVR_DynamicTextureOriginsQpcFrequency=static_cast<uint64_t>(frequency.QuadPart);
    if(MH_EnableHook(target)!=MH_OK){
        MH_RemoveHook(target);CyberpunkVR_DynamicTextureOriginsStatus=5;return false;
    }
    CyberpunkVR_DynamicTextureOriginsStatus=1;return true;
}
// BOTH existing surface hooks validate the original factory bytes at Boot83.
// This observer patches that entry only afterward, without touching their hooks.
CVR_HOOK("DynamicTextureOrigins",::cvr::hooks::Stage::Boot,84,Install);
}
