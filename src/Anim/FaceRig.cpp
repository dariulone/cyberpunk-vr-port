#include "Anim/FaceTracking.hpp"
#include "Anim/FaceMapping.hpp"
#include "Anim/VrikState.hpp"
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/animAnimatedObject.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/FacialSetup.hpp>
#include "Natives/NativeHelpers.hpp"
#include <MinHook.h>
#include <mutex>
#include <atomic>
#include <cstring>

// Game 2.5, GOG executable SHA-256 d36e74c47592e9c656ae250078cb40b2be63c782af5bfbeff2280d43e653fab4.
// 1B25DC evaluates a graph then calls the existing pose mapper at 1B3C08.
// Its second argument owns the destination pose at +38 (tracks at pose+18).
// 692CF8 (animAnimNode_Sermo::Sample, vtable+128) calls 693728 with:
// {facialSetup*, rigReference*, sourcePose*, lod:u32, dt:f32, track:u16}, outputPose*.
// Source pose: base track count +04, total track capacity +28, float data +38.
// Verified against 3A96F8, 6945BC, 693874. Keep this ABI separate from the mapped
// destination pose used by AnimPose.cpp. Hook installation checks both prologues.
namespace cvr::face {
namespace {
constexpr unsigned MaxTracks=4096,MaxBones=2048;
using EvaluateFn=void(*)(void*,void*);
using SolveFn=void(*)(void*,void*);
EvaluateFn evaluateOriginal{};
SolveFn solveOriginal{};
std::mutex rigMutex;
RED4ext::WeakHandle<RED4ext::ent::Entity> mirrorEntity;
std::atomic<uintptr_t> mirrorTracks{};
std::atomic<uint64_t> expires{},generation{1};
std::atomic<bool> hooksReady{};
std::atomic<uint64_t> ownerPasses{},facialPasses{},writePasses{},faults{},rejected{};
std::atomic<unsigned> mappedCount{};
std::array<std::atomic<bool>,MaxBones> keepBone{};
std::array<uint64_t,MaxBones> mirrorNames{};
unsigned mirrorBoneCount{};
uintptr_t mirrorObject{},mirrorRig{};
thread_local uint64_t ownerGeneration{};
struct Cache {
    uintptr_t rig{},names{},setup{};uint64_t generation{};unsigned count{};
    Mapping mapping;
} cache;
struct Capture {
    alignas(16) uint8_t args[0x28];
    alignas(16) uint8_t pose[0x50];
    alignas(16) std::array<float,MaxTracks> values;
    std::array<uint64_t,MaxTracks> names;
    std::array<uint64_t,MaxBones> facialBones;
    uintptr_t rig{},namesPtr{},setup{};
    unsigned count{},nameCount{},facialBoneCount{};
    TrackRange range;
};
bool CaptureInput(void* raw,Capture& out) {
    __try {
        auto* args=static_cast<uint8_t*>(raw);
        auto* setup=*reinterpret_cast<RED4ext::anim::FacialSetup**>(args);
        auto* rigRef=*reinterpret_cast<RED4ext::anim::Rig***>(args+8);
        auto* source=*reinterpret_cast<uint8_t**>(args+0x10);
        if(!setup || setup->version!=8 || !rigRef || !*rigRef || !source)return false;
        auto* rig=*rigRef;
        const auto count=*reinterpret_cast<uint32_t*>(source+0x28);
        const auto baseCount=*reinterpret_cast<uint32_t*>(source+4);
        const auto names=rig->trackNames.Size();
        auto* values=*reinterpret_cast<float**>(source+0x38);
        if(!values || !names || names>MaxTracks || baseCount<names || count<baseCount || count>MaxTracks)return false;
        const auto& m=setup->info.tracksMapping;
        const unsigned begin=m.numEnvelopes,end=begin+m.numMainPoses,ovEnd=end+m.numLipsyncOverrides;
        if(!m.numMainPoses || ovEnd+m.numMainPoses>names)return false;
        const unsigned used=setup->usedTransformIndices.Size(),bones=rig->boneNames.Size();
        if(used>MaxBones || bones>MaxBones)return false;
        for(unsigned i=0;i<used;++i) {
            const unsigned index=setup->usedTransformIndices[i];
            if(index>=bones)return false;
            out.facialBones[i]=rig->boneNames[index].hash;
        }
        out.count=count;out.nameCount=names;out.facialBoneCount=used;
        out.rig=reinterpret_cast<uintptr_t>(rig);out.namesPtr=reinterpret_cast<uintptr_t>(&rig->trackNames[0]);
        out.setup=reinterpret_cast<uintptr_t>(setup);out.range={begin,end,end,ovEnd,ovEnd,ovEnd+m.numMainPoses};
        std::memcpy(out.args,args,sizeof(out.args));std::memcpy(out.pose,source,sizeof(out.pose));
        std::memcpy(out.values.data(),values,count*sizeof(float));
        for(unsigned i=0;i<names;++i)out.names[i]=rig->trackNames[i].hash;
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER){++faults;return false;}
}
uint64_t Owner(void* instance) {
    if(!hooksReady.load(std::memory_order_relaxed) || GetTickCount64()>expires.load(std::memory_order_acquire))return 0;
    __try {
        auto* pose=*reinterpret_cast<uint8_t**>(static_cast<uint8_t*>(instance)+0x38);
        const auto track=pose?*reinterpret_cast<uintptr_t*>(pose+0x18):0;
        return track && track==mirrorTracks.load(std::memory_order_acquire)?generation.load(std::memory_order_acquire):0;
    } __except(EXCEPTION_EXECUTE_HANDLER){++faults;return 0;}
}
void Evaluate(void* graph,void* instance) {
    const auto previous=ownerGeneration;
    ownerGeneration=Owner(instance);
    if(ownerGeneration)++ownerPasses;
    __try {evaluateOriginal(graph,instance);}
    __finally {ownerGeneration=previous;}
}
void Solve(void* args,void* output) {
    if(!ownerGeneration || ownerGeneration!=generation.load(std::memory_order_acquire)) {solveOriginal(args,output);return;}
    ++facialPasses;
    const auto settings=GetSettings();
    if(!settings.enabled){solveOriginal(args,output);return;}
    // Keep this mirror entity alive for the solve. A recycled buffer or a graph
    // evaluation from an earlier binding must never animate an unrelated NPC.
    RED4ext::Handle<RED4ext::ent::Entity> entity;
    {
        std::lock_guard lock(rigMutex);
        if(ownerGeneration==generation.load(std::memory_order_relaxed) && GetTickCount64()<=expires.load())
            entity=mirrorEntity.Lock();
    }
    if(!entity){solveOriginal(args,output);return;}
    const auto input=GetInput();
    Capture captured;
    if(!CaptureInput(args,captured)){++rejected;solveOriginal(args,output);return;}
    Mapping mapping;
    bool current=false;
    {
        std::lock_guard lock(rigMutex);
        current=ownerGeneration==generation.load(std::memory_order_relaxed);
        if(current) {
        if(cache.generation!=ownerGeneration || cache.rig!=captured.rig || cache.names!=captured.namesPtr ||
           cache.setup!=captured.setup || cache.count!=captured.nameCount) {
            cache={captured.rig,captured.namesPtr,captured.setup,ownerGeneration,captured.nameCount,
                Resolve(std::span(captured.names.data(),captured.nameCount),captured.range)};
            unsigned mapped=0;for(const auto& item:cache.mapping)if(item.main>=0)++mapped;
            mappedCount.store(mapped,std::memory_order_relaxed);
            for(unsigned i=0;i<mirrorBoneCount;++i) {
                const auto name=mirrorNames[i];
                const bool body=name==NameHash("Head") || name==NameHash("Neck") || name==NameHash("Root");
                const bool facial=std::find(captured.facialBones.begin(),captured.facialBones.begin()+captured.facialBoneCount,name)!=
                    captured.facialBones.begin()+captured.facialBoneCount;
                keepBone[i].store(facial && !body,std::memory_order_relaxed);
            }
        }
        mapping=cache.mapping;
        }
    }
    if(!current){solveOriginal(args,output);return;}
    if(!Apply(mapping,std::span(captured.values.data(),captured.count),input,settings,GetTickCount64())) {
        solveOriginal(args,output);return;
    }
    // Feed a private input copy to the original solver. The game's source pose,
    // resource data and unrelated tracks remain untouched; its solver still
    // computes the bones, inbetweens, corrective shapes and wrinkles.
    *reinterpret_cast<float**>(captured.pose+0x38)=captured.values.data();
    *reinterpret_cast<uint8_t**>(captured.args+0x10)=captured.pose;
    solveOriginal(captured.args,output);++writePasses;
}
bool ReadBinding(RED4ext::anim::AnimatedObject* object,uintptr_t& rig,uintptr_t& tracks,
                 std::array<uint64_t,MaxBones>& names,unsigned& count) {
    __try {
        auto* meta=object?object->metaRig:nullptr;
        if(!meta || !meta->boneNames.Size() || meta->boneNames.Size()>MaxBones)return false;
        auto* pose=*reinterpret_cast<uint8_t**>(reinterpret_cast<uint8_t*>(object)+0x18);
        tracks=pose?*reinterpret_cast<uintptr_t*>(pose+0x18):0;
        if(!tracks || tracks==g_PlayerTrackBufA || tracks==g_PlayerTrackBufB)return false;
        rig=reinterpret_cast<uintptr_t>(meta);count=meta->boneNames.Size();
        for(unsigned i=0;i<count;++i)names[i]=meta->boneNames[i].hash;
        return true;
    } __except(EXCEPTION_EXECUTE_HANDLER){++faults;return false;}
}
}
void ClearMirror() {
    if(!mirrorTracks.load(std::memory_order_acquire))return;
    expires.store(0,std::memory_order_release);mirrorTracks.store(0,std::memory_order_release);
    std::lock_guard lock(rigMutex);++generation;mirrorEntity.Reset();mirrorObject=mirrorRig=0;mirrorBoneCount=0;
    mappedCount=0;cache={};for(auto& bone:keepBone)bone=false;
}
void BindMirror(RED4ext::ent::Entity* entity) {
    if(!GetSettings().enabled || !entity){ClearMirror();return;}
    auto* object=FindAnimatedObjectForEntity(entity,"root",false);
    uintptr_t rig{},tracks{};unsigned count{};std::array<uint64_t,MaxBones> names{};
    if(!ReadBinding(object,rig,tracks,names,count)){ClearMirror();return;}
    std::lock_guard lock(rigMutex);
    if(mirrorObject!=reinterpret_cast<uintptr_t>(object) || mirrorRig!=rig || mirrorTracks.load()!=tracks) {
        ++generation;cache={};mappedCount=0;
        for(auto& bone:keepBone)bone=false;
        mirrorObject=reinterpret_cast<uintptr_t>(object);mirrorRig=rig;mirrorBoneCount=count;mirrorNames=names;
    }
    mirrorEntity=RED4ext::Handle<RED4ext::ent::Entity>(entity);
    mirrorTracks.store(tracks,std::memory_order_release);expires.store(GetTickCount64()+750,std::memory_order_release);
}
bool PreserveMirrorBone(unsigned index) {
    return index<MaxBones && mirrorTracks.load(std::memory_order_relaxed) && GetTickCount64()<=expires.load(std::memory_order_relaxed) &&
        keepBone[index].load(std::memory_order_relaxed);
}
Status RigStatus() {
    Status status;status.hooks=hooksReady.load();status.bound=mirrorTracks.load() && GetTickCount64()<=expires.load();
    status.ownerPasses=ownerPasses.load();status.facialPasses=facialPasses.load();status.writes=writePasses.load();
    status.faults=faults.load();status.mapped=mappedCount.load();status.rejected=rejected.load();return status;
}
bool InstallHooks() {
    static bool attempted=false;
    if(attempted)return hooksReady.load();
    attempted=true;
    auto* base=reinterpret_cast<uint8_t*>(GetModuleHandleW(L"Cyberpunk2077.exe"));
    if(!base)return false;
    const auto* dos=reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    if(dos->e_magic!=IMAGE_DOS_SIGNATURE || dos->e_lfanew<=0 || dos->e_lfanew>4096)return false;
    const auto* pe=reinterpret_cast<const IMAGE_NT_HEADERS64*>(base+dos->e_lfanew);
    // These offsets are not a general 2.x API. Refuse other executables before
    // reading target code, even if they happen to share a function prologue.
    if(pe->Signature!=IMAGE_NT_SIGNATURE || pe->FileHeader.TimeDateStamp!=0x6ad55daf ||
       pe->OptionalHeader.SizeOfImage!=0x581b000)return false;
    constexpr uint8_t evalBytes[]{0x48,0x89,0x54,0x24,0x10,0x48,0x89,0x4c,0x24,0x08,0x55,0x53,0x56,0x57,0x41,0x54,0x41,0x55,0x41,0x56,0x41,0x57,0x48,0x8d};
    constexpr uint8_t solveBytes[]{0x48,0x8b,0xc4,0x48,0x89,0x58,0x08,0x48,0x89,0x70,0x10,0x48,0x89,0x78,0x18,0x4c,0x89,0x60,0x20,0x55,0x41,0x56,0x41,0x57};
    auto* evalTarget=base+0x1b25dc;auto* solveTarget=base+0x693728;
    if(std::memcmp(evalTarget,evalBytes,sizeof(evalBytes)) || std::memcmp(solveTarget,solveBytes,sizeof(solveBytes)))return false;
    MH_Initialize();
    if(MH_CreateHook(evalTarget,&Evaluate,reinterpret_cast<void**>(&evaluateOriginal))!=MH_OK)return false;
    if(MH_CreateHook(solveTarget,&Solve,reinterpret_cast<void**>(&solveOriginal))!=MH_OK){MH_RemoveHook(evalTarget);return false;}
    if(MH_EnableHook(solveTarget)!=MH_OK || MH_EnableHook(evalTarget)!=MH_OK) {
        MH_DisableHook(evalTarget);MH_DisableHook(solveTarget);MH_RemoveHook(evalTarget);MH_RemoveHook(solveTarget);return false;
    }
    hooksReady.store(true,std::memory_order_release);return true;
}
}
