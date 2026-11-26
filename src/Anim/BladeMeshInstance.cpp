#include "Anim/BladeMeshInstance.hpp"
#include "Anim/BladeMeshBlob.hpp"
#include "Utils/TestMarker.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/ResourceLoader.hpp>
#include <RED4ext/ResourceDepot.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entSkinnedMeshComponent.hpp>
#include <RED4ext/Scripting/Natives/animRig.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimationControlBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/SkinningBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/SlotComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/mesh/MeshAppearance.hpp>
#include <RED4ext/Scripting/Natives/Generated/rend/RenderMeshBlob.hpp>
#include <sstream>
#include <atomic>
#include <mutex>
#include <limits>
#include <unordered_map>
#include <unordered_set>

bool ClassIsA(RED4ext::CClass* type, RED4ext::CName name);
extern void Log(const char*,...);

namespace cvr::blade {
namespace {
using namespace RED4ext;
constexpr uint64_t prefix=0xd3a6c50000000000ULL;
constexpr unsigned capacity=9; // Eight owners and one slot for replacing a resident mesh.
constexpr size_t maxSegments=12;
struct Segment {
    mesh::Volume volume; CName bone;bool swept{};std::array<mesh::Point,4> rays{};
    uint32_t serial{};mesh::Mark mark{};uint64_t visibleChunks{UINT64_MAX};
    double shellDepth{};
    std::bitset<256> skinRegion;
};
bool TraceCut() {static const bool on=cvr::TestMarker("cpvr_mesh_cut_trace.flag");return on;}
struct Copy {
    std::atomic<uint64_t> path{}, source{};
    std::mutex lock;
    WeakHandle<ent::SkinnedMeshComponent> owner;
    ResourcePath previous;
    uint64_t touched{}, originalMask{}, managedMask{};
    std::vector<Segment> history;
    std::vector<Segment> pending;
    SharedPtr<ResourceToken<CMesh>> token;
    mesh::Counts counts{};
    bool ready{},finished{};
};
Copy copies[capacity];
std::atomic<bool> enabled{}, working{};
uint32_t generation{};
Copy* Lookup(uint64_t path) {
    if((path&0xffffff0000000000ULL)!=prefix) return nullptr;
    const auto slot=unsigned(path&255);
    return slot<capacity && copies[slot].path.load(std::memory_order_acquire)==path?&copies[slot]:nullptr;
}
void Reset(Copy& copy) {
    copy.path.store(0,std::memory_order_release);copy.token={};copy.owner={};copy.history.clear();copy.pending.clear();
    copy.ready=copy.finished=false;copy.counts={};copy.managedMask=0;
}
Copy* FreeCopy(bool replacing) {
    Copy* first=nullptr;
    for(auto& copy:copies) if(!copy.path.load()) {
        if(replacing) return &copy;
        if(first) return first;
        first=&copy;
    }
    return nullptr;
}
bool StartCopy(Copy& copy,const Handle<ent::SkinnedMeshComponent>& component,ResourcePath original,std::vector<Segment> history) {
    ResourcePath path;
    path.hash=prefix|(uint64_t(++generation)<<8)|uint64_t(&copy-copies);
    if(ResourceDepot::Get()->ResourceExists(path)) return false;
    if(TraceCut()) {
        const auto& stroke=history.back();
        Log("[mesh-cut] queue source=%llu key=%llu segments=%zu serial=%u bone=%s component=%s tick=%llu mask=%016llx region=%zu\n",
            original.hash,path.hash,history.size(),stroke.serial,stroke.bone.ToString(),component->name.ToString(),
            GetTickCount64(),stroke.visibleChunks,stroke.skinRegion.count());
    }
    uint64_t originalMask=component->chunkMask, managedMask=0;
    if(auto* old=Lookup(component->mesh.path.hash)) {
        std::lock_guard guard(old->lock);
        originalMask=old->originalMask;managedMask=old->managedMask;
    }
    {
        std::lock_guard guard(copy.lock);
        copy.owner=component;copy.previous=component->mesh.path;copy.history=std::move(history);
        copy.originalMask=originalMask;copy.managedMask=managedMask;copy.touched=GetTickCount64();
        copy.source.store(original.hash);copy.path.store(path.hash,std::memory_order_release);
    }
    auto token=ResourceLoader::Get()->LoadAsync<CMesh>(path);
    std::lock_guard guard(copy.lock);
    if(!token) {Reset(copy);return false;}
    copy.token=std::move(token);working.store(true,std::memory_order_release);return true;
}
uint64_t PaintMask(const Copy& copy,uint64_t current) {
    uint64_t mask=(current&~copy.managedMask)|(copy.originalMask&copy.managedMask);
    for(auto [parent,ink]:copy.counts.inkChunks) {
        const uint64_t bit=uint64_t(1)<<ink;
        if(mask&(uint64_t(1)<<parent)) mask|=bit; else mask&=~bit;
    }
    return mask;
}
void Poll(IScriptable*,CStackFrame* frame,void*,int64_t) {
    ++frame->code;
    if(!working.load(std::memory_order_acquire)) return;
    bool any=false;
    for(auto& copy:copies) {
        if(!copy.path.load()) continue;
        std::unique_lock guard(copy.lock,std::try_to_lock);
        if(!guard || !copy.token || !copy.token->IsFinished()) {any=true;continue;}
        auto owner=copy.owner.Lock();
        if(!owner || !owner->owner || !copy.token->IsLoaded()) {Reset(copy);continue;}
        if(copy.finished && owner->mesh.path.hash!=copy.path.load()) {Reset(copy);continue;}
        auto mesh=copy.token->Get();
        if(!copy.finished) {
            if(owner->mesh.path!=copy.previous || !mesh || !owner->meshHandle ||
               mesh->path.hash!=copy.path.load() || owner->meshHandle.instance==mesh.instance) {Reset(copy);continue;}
            if(copy.ready && mesh->renderResource && mesh->renderResource!=owner->meshHandle->renderResource) {
                owner->chunkMask=PaintMask(copy,owner->chunkMask);
                for(auto [parent,ink]:copy.counts.inkChunks) copy.managedMask|=uint64_t(1)<<ink;
                owner->mesh.path.hash=copy.path.load();copy.finished=true;any=true;
                if(TraceCut()) Log("[mesh-cut] reload key=%llu tick=%llu\n",copy.path.load(),GetTickCount64());
                guard.unlock();ReloadCutMesh(owner);continue;
            }
            if(copy.pending.empty()) {Reset(copy);continue;}
        }
        any=true;
        if(copy.finished && owner->meshHandle.instance==mesh.instance) {
            auto mask=PaintMask(copy,owner->chunkMask);
            if(mask!=owner->chunkMask) {
                owner->chunkMask=mask;guard.unlock();ReloadCutMesh(owner);continue;
            }
        }
        if(copy.pending.empty() || (copy.finished && owner->meshHandle.instance!=mesh.instance)) continue;
        auto* next=copy.finished?FreeCopy(true):&copy;
        if(!next) continue;
        auto history=copy.history;
        history.insert(history.end(),copy.pending.begin(),copy.pending.end());copy.pending.clear();
        if(history.size()>maxSegments) history.erase(history.begin(),history.end()-maxSegments);
        ResourcePath source;source.hash=copy.source.load();
        if(!copy.finished) Reset(copy);
        guard.unlock();
        StartCopy(*next,owner,source,std::move(history));
    }
    working.store(any,std::memory_order_release);
}
bool Append(Copy& copy,const Segment& stroke) {
    const auto& last=copy.pending.empty()?copy.history.back():copy.pending.back();
    if(last.mark==stroke.mark && last.serial==stroke.serial && last.bone==stroke.bone && mesh::SegmentDistance(last.volume,stroke.volume)<.018) return false;
    if(copy.pending.size()>=maxSegments) copy.pending.erase(copy.pending.begin());
    copy.pending.push_back(stroke);copy.touched=GetTickCount64();return true;
}
// A new NPC can take the least recently marked resident slot. Restore the
// evicted component to its own source before reusing the private depot slot.
Copy* EvictResident(const ent::Entity* incoming) {
    Copy* oldest=nullptr;
    for(auto& copy:copies) {
        auto owner=copy.owner.Lock();
        if(copy.finished && copy.pending.empty() && owner && owner->owner!=incoming &&
           owner->mesh.path.hash==copy.path.load() && (!oldest || copy.touched<oldest->touched)) oldest=&copy;
    }
    if(!oldest) return nullptr;
    std::unique_lock guard(oldest->lock);
    auto owner=oldest->owner.Lock();
    if(!owner || owner->mesh.path.hash!=oldest->path.load()) return nullptr;
    owner->mesh.path.hash=oldest->source.load();
    owner->chunkMask=(owner->chunkMask&~oldest->managedMask)|(oldest->originalMask&oldest->managedMask);
    Reset(*oldest);guard.unlock();ReloadCutMesh(owner);
    return oldest;
}
bool Queue(const Handle<ent::SkinnedMeshComponent>& component,const Segment& stroke) {
    const auto original=component->mesh.path;
    if(original.IsEmpty()) return false;
    for(auto& copy:copies) if(copy.path.load()) {
        std::lock_guard guard(copy.lock);
        if(!copy.finished && copy.previous==original && copy.owner.Lock().instance==component.instance) return Append(copy,stroke);
    }
    if(auto* old=Lookup(original.hash)) {
        std::lock_guard guard(old->lock);
        return old->finished && old->owner.Lock().instance==component.instance && Append(*old,stroke);
    }
    auto* available=FreeCopy(false);
    if(!available) available=EvictResident(component->owner);
    return available && StartCopy(*available,component,original,{stroke});
}

// Snapshot palette membership before the private mesh loads on its worker.
using BoneNames=std::unordered_set<uint64_t>;
BoneNames SkinRegion(const ent::Entity& target,CName source,const ent::SlotComponent& slots,CName bone) {
    BoneNames names{bone.hash};
    CName controller;
    for(const auto& handle:target.components) {
        if(handle && handle->name==source && ClassIsA(handle->GetType(),ent::AnimatedComponent::NAME)) {
            const auto& binding=static_cast<const ent::AnimatedComponent*>(handle.instance)->controlBinding;
            if(binding && binding->enabled) controller=binding->bindName;
            break;
        }
    }
    for(const auto& handle:target.components) {
        if(!handle || !ClassIsA(handle->GetType(),ent::AnimatedComponent::NAME)) continue;
        const auto* animated=static_cast<const ent::AnimatedComponent*>(handle.instance);
        // Retail skin components often bind to "root", while a rig driven
        // by root supplies the muscle/twist hierarchy in their mesh palettes.
        const auto& binding=animated->controlBinding;
        if(animated->name!=source && animated->name!=controller && (!binding || !binding->enabled ||
           (binding->bindName!=source && (!controller || binding->bindName!=controller)))) continue;
        const auto& token=animated->rig.token;
        if(!token || !token->IsLoaded()) continue;
        const auto* rig=token->resource.GetPtr();
        if(!rig || !rig->parentIndices || rig->boneNames.size()>32768 ||
           std::find(rig->boneNames.begin(),rig->boneNames.end(),bone)==rig->boneNames.end()) continue;
        // Each helper belongs to its nearest ancestor with a hit slot. In
        // particular, forearm and wrist vertices do not belong to an upper arm.
        std::vector<int8_t> roots(rig->boneNames.size(),-1);
        for(unsigned i=0;i<roots.size();++i) {
            const auto name=rig->boneNames[i];
            if(name==bone) roots[i]=1;
            else for(const auto& slot:slots.slots) if(slot.boneName==name) {roots[i]=0;break;}
        }
        for(unsigned i=0;i<roots.size();++i) {
            int parent=int(i);
            for(size_t steps=0;steps<roots.size() && parent>=0 && size_t(parent)<roots.size();++steps) {
                if(roots[parent]>=0) {
                    if(roots[parent]) names.insert(rig->boneNames[i].hash);
                    break;
                }
                parent=rig->parentIndices[parent];
            }
        }
    }
    return names;
}
void SubmitArgs(CStackFrame* frame,uint32_t* result,bool swept,bool mark=false) {
    Handle<ent::Entity> target; CName bone; Vector4 from,to,normal; float depth{}; uint32_t serial{};
    GetParameter(frame,&target);GetParameter(frame,&bone);GetParameter(frame,&from);GetParameter(frame,&to);
    GetParameter(frame,&normal);GetParameter(frame,&depth);GetParameter(frame,&serial);
    Segment segment;
    segment.swept=swept;
    if(swept) for(auto& point:segment.rays) {
        Vector4 value;GetParameter(frame,&value);point={value.X,value.Y,value.Z};
    }
    int32_t kind{};
    if(mark) GetParameter(frame,&kind);
    ++frame->code;
    if(!result) return;
    *result=0;
    if(mark && (kind<1 || kind>3)) return;
    segment.mark=static_cast<mesh::Mark>(kind);
    if(mark) segment.shellDepth=normal.W;
    if(!target || !serial || !enabled.load() || !ClassIsA(target->GetType(),"NPCPuppet")) return;
    segment.bone=bone;
    segment.serial=serial;
    mesh::Cut cut{{from.X,from.Y,from.Z},{to.X,to.Y,to.Z},{normal.X,normal.Y,normal.Z},.004,depth,.012};
    if(!mesh::MakeVolume(cut,segment.volume)) return;
    if(mark && TraceCut()) {
        const auto& p=segment.rays;
        Log("[mesh-cut] stroke serial=%u bone=%s kind=%d from=(%g,%g,%g) to=(%g,%g,%g) normal=(%g,%g,%g) shell=%g depth=%g rays=(%g,%g,%g;%g,%g,%g;%g,%g,%g;%g,%g,%g)\n",
            serial,bone.ToString(),kind,from.X,from.Y,from.Z,to.X,to.Y,to.Z,normal.X,normal.Y,normal.Z,normal.W,depth,
            p[0][0],p[0][1],p[0][2],p[1][0],p[1][1],p[1][2],p[2][0],p[2][1],p[2][2],p[3][0],p[3][1],p[3][2]);
    }
    std::vector<ent::SkinnedMeshComponent*> candidates;
    const ent::SlotComponent* slots=nullptr;
    for(const auto& handle:target->components) {
        if(mark && handle && handle->name=="HitRepresentation" && ClassIsA(handle->GetType(),ent::SlotComponent::NAME))
            slots=static_cast<const ent::SlotComponent*>(handle.instance);
        if(!handle || !handle->isEnabled || !ClassIsA(handle->GetType(),ent::SkinnedMeshComponent::NAME)) continue;
        if(handle->name=="AppearanceProxyMesh" || ClassIsA(handle->GetType(),"entAppearanceProxyMeshComponent")) continue;
        auto* skin=static_cast<ent::SkinnedMeshComponent*>(handle.instance);
        if(!skin->meshHandle || !skin->renderProxy || !skin->meshHandle->renderResource ||
           std::find(skin->meshHandle->boneNames.begin(),skin->meshHandle->boneNames.end(),bone)==skin->meshHandle->boneNames.end()) continue;
        candidates.push_back(skin);
    }
    auto priority=[](auto* component) {
        const auto* name=component->name.ToString();
        return name && (std::strncmp(name,"t0_",3)==0 || std::strncmp(name,"h0_",3)==0 || std::strncmp(name,"a0_",3)==0);
    };
    std::stable_partition(candidates.begin(),candidates.end(),priority);
    std::unordered_map<uint64_t,BoneNames> regions;
    for(auto* component:candidates) {
        segment.visibleChunks=component->chunkMask;
        if(mark && slots) {
            segment.skinRegion.reset();
            if(component->skinning && component->skinning->enabled) {
                const auto source=component->skinning->bindName;
                auto [it,added]=regions.try_emplace(source.hash);
                if(added) it->second=SkinRegion(*target.instance,source,*slots,bone);
                const auto& palette=component->meshHandle->boneNames;
                for(unsigned i=0;i<palette.size() && i<segment.skinRegion.size();++i)
                    segment.skinRegion[i]=it->second.contains(palette[i].hash);
            }
        }
        if(Queue(Handle<ent::SkinnedMeshComponent>(component),segment)) ++*result;
    }
}
void Submit(IScriptable*,CStackFrame* frame,uint32_t* result,int64_t) {SubmitArgs(frame,result,false);}
void SubmitSweep(IScriptable*,CStackFrame* frame,uint32_t* result,int64_t) {SubmitArgs(frame,result,true);}
void SubmitMark(IScriptable*,CStackFrame* frame,uint32_t* result,int64_t) {SubmitArgs(frame,result,true,true);}

void SurfacePoint(IScriptable*,CStackFrame* frame,DynArray<Vector4>* result,int64_t) {
    Handle<ent::Entity> target;CName bone;uint32_t serial{};
    GetParameter(frame,&target);GetParameter(frame,&bone);GetParameter(frame,&serial);++frame->code;
    if(!result) return;
    result->Clear();
    if(!target || !serial) return;
    double nearest=std::numeric_limits<double>::infinity();
    Vector4 position{},normal{};
    for(auto& copy:copies) {
        if(!copy.path.load()) continue;
        std::lock_guard guard(copy.lock);
        auto owner=copy.owner.Lock();
        const auto& c=copy.counts;
        if(!copy.finished || !owner || owner->owner!=target.instance || !owner->isEnabled || !owner->renderProxy ||
           !owner->meshHandle || owner->meshHandle->path.hash!=copy.path.load() || !owner->meshHandle->renderResource ||
           c.contactEdit<0 || !(owner->chunkMask&(uint64_t(1)<<c.contactChunk)) ||
           !(owner->chunkMask&(uint64_t(1)<<c.contactInkChunk))) continue;
        const auto& chunks=owner->meshHandle->renderResource->chunks;
        if(c.contactInkChunk>=chunks.size() || chunks[c.contactInkChunk].numIndices<3) continue;
        const auto& appearance=owner->appearanceHandle;
        static const CName inkMaterial("vrport_surface_ink");
        if(appearance && (c.contactInkChunk>=appearance->chunkMaterials.size() ||
           appearance->chunkMaterials[c.contactInkChunk]!=inkMaterial)) continue;
        const auto& stroke=copy.history[c.contactEdit];
        if(stroke.bone!=bone || stroke.serial!=serial || c.entryDistanceSquared>=nearest) continue;
        nearest=c.entryDistanceSquared;
        position=Vector4(float(c.contact[0]),float(c.contact[1]),float(c.contact[2]),float(stroke.mark));
        normal=Vector4(float(c.normal[0]),float(c.normal[1]),float(c.normal[2]),float(c.inkTriangles));
    }
    if(std::isfinite(nearest)) {
        result->PushBack(position);
        result->PushBack(normal);
    }
}

void InspectPair(IScriptable*, CStackFrame* stack, CString* result, int64_t) {
    Handle<ent::Entity> target, control;
    GetParameter(stack,&target); GetParameter(stack,&control); ++stack->code;
    if (!result) return;
    *result = "";
    if (!target || !control || target.instance==control.instance) return;
    std::ostringstream summary;
    for (const auto& component : target->components) {
        if (!component || !component->isEnabled || !ClassIsA(component->GetType(),ent::SkinnedMeshComponent::NAME)) continue;
        auto* skin = static_cast<ent::SkinnedMeshComponent*>(component.instance);
        auto mesh=skin->meshHandle;
        if (!mesh || !mesh->renderResource || !skin->renderProxy) continue;
        bool shared=false, separateProxy=false;
        for (const auto& other : control->components) {
            if (!other || !other->isEnabled || !ClassIsA(other->GetType(),ent::SkinnedMeshComponent::NAME)) continue;
            const auto* otherSkin=static_cast<ent::SkinnedMeshComponent*>(other.instance);
            if (otherSkin->meshHandle.instance!=mesh.instance || !otherSkin->renderProxy) continue;
            shared=true;
            separateProxy=otherSkin->renderProxy.GetPtr()!=skin->renderProxy.GetPtr();
            break;
        }
        auto blob=mesh->renderResourceBlob;
        const auto* geometry=blob && blob->GetType()->name==rend::RenderMeshBlob::NAME ?
            static_cast<const rend::RenderMeshBlob*>(blob.instance) : nullptr;
        const auto* name=component->name.ToString();
        uint64_t vertices=0,indices=0;
        for(const auto& chunk:mesh->renderResource->chunks) {vertices+=chunk.numVertices;indices+=chunk.numIndices;}
        summary<<(name?name:"?")<<' '<<skin->mesh.path.hash<<' '<<shared<<' '<<separateProxy<<' '
            <<mesh->renderResource->chunks.size()<<' '<<bool(geometry)<<' '
            <<(geometry && geometry->renderBuffer.buffer.data?geometry->renderBuffer.buffer.size:0)<<' '
            <<(geometry?geometry->header.vertexBufferSize:0)<<' '<<(geometry?geometry->header.indexBufferSize:0)<<' '
            <<vertices<<' '<<indices<<'\n';
    }
    *result=summary.str().c_str();
}
}
uint64_t MeshDepotSource(uint64_t requested) {
    auto* copy=Lookup(requested);return copy?copy->source.load(std::memory_order_acquire):requested;
}
void EnableMeshCopies(bool on) {enabled.store(on);}
void BeforePrivateMeshUpload(RED4ext::CMesh* mesh) {
    auto* copy=mesh?Lookup(mesh->path.hash):nullptr;
    if(!copy) return;
    std::vector<mesh::Edit> edits;
    {
        std::lock_guard guard(copy->lock);
        if(copy->path.load()!=mesh->path.hash || copy->ready) return;
        for(const auto& segment:copy->history) {
            const auto found=std::find(mesh->boneNames.begin(),mesh->boneNames.end(),segment.bone);
            if(found==mesh->boneNames.end()) return;
            const int bone=int(found-mesh->boneNames.begin());
            auto region=segment.skinRegion;
            if(bone<int(region.size())) region.set(bone);
            edits.push_back({segment.volume,bone,segment.swept,segment.rays,segment.mark,
                             segment.visibleChunks,segment.shellDepth,region});
        }
    }
    // The token cannot finish until this upload returns. Gameplay can collect
    // subsequent segments while the worker prepares these vertex streams.
    mesh::Counts counts{};bool ready=false;
    try { ready=mesh::CutPrivateBlob(*mesh,edits,counts); }
    catch(const std::bad_alloc&) { return; }
    std::lock_guard guard(copy->lock);
    if(copy->path.load()!=mesh->path.hash) return;
    copy->ready=ready;copy->counts=counts;
    if(TraceCut()) Log("[mesh-cut] prepared source=%llu ready=%d split=%u inserted=%u ink=%u shellProjection=%d noOverlap=%d key=%llu tick=%llu\n",
        copy->source.load(),copy->ready,counts.split,counts.inserted,counts.inkTriangles,counts.projectedFromShell,
        counts.noOverlap,mesh->path.hash,GetTickCount64());
}
void RegisterMeshInstanceProbe() {
    auto* point=RED4ext::CGlobalFunction::Create("VRBladePaintPoint","VRBladePaintPoint",&SurfacePoint);
    point->flags={.isNative=true,.isStatic=true};point->SetReturnType("array:Vector4");
    point->AddParam("handle:entEntity","target");point->AddParam("CName","bone");point->AddParam("Uint32","serial");
    RED4ext::CRTTISystem::Get()->RegisterFunction(point);
    auto* read=RED4ext::CGlobalFunction::Create("VRBladeMeshInstanceProbe","VRBladeMeshInstanceProbe",&InspectPair);
    read->flags={.isNative=true,.isStatic=true}; read->SetReturnType("String");
    read->AddParam("handle:entEntity","target"); read->AddParam("handle:entEntity","control");
    RED4ext::CRTTISystem::Get()->RegisterFunction(read);
    for(int mode=0;mode<3;++mode) {
        const char* name=mode==2?"VRBladeQueueSurfaceMark":mode==1?"VRBladeQueueMeshSweep":"VRBladeQueueMeshCut";
        auto* submit=RED4ext::CGlobalFunction::Create(name,name,mode==2?&SubmitMark:mode==1?&SubmitSweep:&Submit);
        submit->flags={.isNative=true,.isStatic=true}; submit->SetReturnType("Uint32");
        submit->AddParam("handle:entEntity","target");submit->AddParam("CName","bone");
        for(const char* param:{"from","to","normal"}) submit->AddParam("Vector4",param);
        submit->AddParam("Float","depth");submit->AddParam("Uint32","serial");
        if(mode) for(const char* param:{"oldStart","oldEnd","start","end"}) submit->AddParam("Vector4",param);
        if(mode==2) submit->AddParam("Int32","kind");
        RED4ext::CRTTISystem::Get()->RegisterFunction(submit);
    }
    auto* poll=RED4ext::CGlobalFunction::Create("VRBladeMeshPoll","VRBladeMeshPoll",&Poll);
    poll->flags={.isNative=true,.isStatic=true}; RED4ext::CRTTISystem::Get()->RegisterFunction(poll);
}
}
