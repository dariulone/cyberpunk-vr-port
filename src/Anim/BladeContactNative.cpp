#include "Anim/BladeContactState.hpp"
#include "Anim/BladeAttachment.hpp"
#include "Anim/BladeMeshInstance.hpp"
#include "Anim/VrikState.hpp"
#include "Natives/PhysHands.hpp"
#include "Natives/DebugView.hpp"
#include "Utils/TestMarker.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/ResourceLoader.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/HitRepresentationComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/HitRepresentationResource.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/HitShape_Capsule.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/HitShape_Sphere.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/HitShapeUserData.hpp>
#include <RED4ext/Scripting/Natives/Generated/game/ShapeData.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldTransform.hpp>
#include <RED4ext/Scripting/Natives/physicsTraceResult.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>

extern void Log(const char*,...);

namespace cvr::blade {
namespace {
using namespace RED4ext;
template<class Visitor>
void VisitShapes(game::HitRepresentationComponent& component, Visitor visit) {
    for (const auto& item : component.appearanceOverrides) if (visit(item.represenationOverride)) return;
    for (const auto& item : component.representations) if (visit(item)) return;
    // An ordinary puppet may have only a resource reference. The flag alone
    // does not describe whether there are component-local representations.
    if (component.resource.path.IsEmpty() || !UsesShapeResource(component.useResourceData,!component.representations.IsEmpty())) return;
    auto* loader=ResourceLoader::Get();
    auto resource=loader->FindToken<game::HitRepresentationResource>(component.resource.path);
    if (!resource) resource=loader->LoadAsync<game::HitRepresentationResource>(component.resource.path);
    if (!resource || !resource->IsLoaded() || !resource->Get()) return;
    for (const auto& item : resource->Get()->overrides) if (visit(item.represenationOverride)) return;
    for (const auto& item : resource->Get()->representations) if (visit(item)) return;
}

bool Capsule(const game::HitShapeContainer& item, Vector4& center, Vector4& axis) {
    if (!item.shape) return false;
    const auto* shape = static_cast<const game::HitShapeBase*>(item.shape.instance);
    const auto type = item.shape->GetType()->name;
    float radius, height = 0;
    if (type == game::HitShape_Capsule::NAME) {
        const auto* capsule = static_cast<const game::HitShape_Capsule*>(shape);
        radius = capsule->radius; height = capsule->height;
    } else if (type == game::HitShape_Sphere::NAME) {
        radius = static_cast<const game::HitShape_Sphere*>(shape)->radius;
    } else return false;
    if (!(radius > 0 && radius <= 1 && height >= 0 && height <= 2)) return false;
    center = shape->localTransform.W; axis = shape->localTransform.X;
    center.W = radius; axis.W = height * .5f;
    return true;
}

template<bool WithBinding>
void ReadCapsule(IScriptable*, CStackFrame* frame, DynArray<Vector4>* result, int64_t) {
    Handle<game::HitRepresentationComponent> component;
    Handle<game::HitShapeUserData> data;
    CName name,binding;
    GetParameter(frame, &component); GetParameter(frame, &name); GetParameter(frame, &data);
    if constexpr (WithBinding) GetParameter(frame,&binding);
    else binding=name=="Neck" ? CName("Head") : name;
    ++frame->code;
    if (!result) return;
    result->Clear();
    if (!component) return;
    CapsuleLookup lookup;
    VisitShapes(*component.instance,[&](const game::HitShapeContainer& item) {
        if(item.name!=name) return false;
        const CName slot=item.slotName.IsNone() ? item.name : item.slotName;
        if(slot!=binding && (WithBinding || slot!=name)) return false;
        Vector4 center, axis;
        if (!Capsule(item,center,axis)) return false;
        return lookup.Consider({center.X,center.Y,center.Z,center.W,axis.X,axis.Y,axis.Z,axis.W},
                               data && item.userData.instance==data.instance);
    });
    if (lookup.found && !lookup.ambiguous) {
        const auto& g=lookup.geometry;
        result->PushBack({g[0],g[1],g[2],g[3]});result->PushBack({g[4],g[5],g[6],g[7]});
    }
}

struct SweepObservation {
    const game::HitShapeContainer& item;
    CName binding;
    WorldTransform frame;
    Vector4 center,axis,a,b;
    SweepContact hit;
};
struct ObservationFrame {
    CName binding;
    WorldTransform frame;
};
struct SweepEndPose {
    Vector4 grip;
    bool occupied{};
};

template<class Visitor>
int VisitSweepContacts(game::HitRepresentationComponent& component,const Vector4 (&ends)[4],Visitor visit,
                       const ObservationFrame* observation=nullptr, SweepEndPose* endPose=nullptr) {
    auto* getSlot=component.GetType()->GetFunction("GetSlotTransform");
    if (!getSlot) return -1;
    int missingSlots=0;
    VisitShapes(component,[&](const game::HitShapeContainer& item) {
        Vector4 center, axis;
        if (!Capsule(item,center,axis)) return false;
        WorldTransform slot{};
        CName binding=item.slotName;
        if (!binding) binding=item.name;
        if(observation && binding && binding==observation->binding) {
            slot=observation->frame;
        } else {
            bool found=false;
            StackArgs_t args;
            args.emplace_back(nullptr,&binding); args.emplace_back(nullptr,&slot);
            if (!ExecuteFunction(&component,getSlot,&found,args) || !found) { ++missingSlots;return false; }
        }
        const auto origin=slot.Position.AsVector4();
        const auto inverse=slot.Orientation.Conjugate();
        Vector4 local[4];
        for (int i=0;i<4;++i) local[i]=inverse.Transform(ends[i]-origin);
        if (endPose && !endPose->occupied) {
            // Include the heel of the shaft and every resolved region, even a
            // neighbor the striking portion never crossed. Use this slot pose.
            const auto grip=inverse.Transform(endPose->grip-origin);
            endPose->occupied=SweepCapsule(&grip.X,&local[3].X,&grip.X,&local[3].X,
                &center.X,&axis.X,center.W,axis.W).depth>0.f;
        }
        const auto hit=SweepCapsule(&local[0].X,&local[1].X,&local[2].X,&local[3].X,&center.X,&axis.X,center.W,axis.W);
        local[2].W=local[3].W=1.f;
        if (hit.depth>0) visit(SweepObservation{item,binding,slot,center,axis,local[2],local[3],hit});
        return false;
    });
    return missingSlots;
}

template<bool CheckExit>
void SweptContact(IScriptable*, CStackFrame* stack, Vector4* result, int64_t) {
    Handle<game::HitRepresentationComponent> component;
    Vector4 ends[4];
    SweepEndPose endPose{};
    GetParameter(stack,&component);
    for (auto& point : ends) GetParameter(stack,&point);
    if constexpr (CheckExit) GetParameter(stack,&endPose.grip);
    ++stack->code;
    if (!result) return;
    *result={};
    if (!component) return;
    const int missing=VisitSweepContacts(*component.instance,ends,[&](const SweepObservation& obs) {
        const auto& hit=obs.hit;
        if (!result->W || hit.time<result->X || (hit.time==result->X && hit.depth>result->Z))
            *result={hit.time,hit.fraction,hit.depth,1};
    },nullptr,CheckExit ? &endPose : nullptr);
    if constexpr (CheckExit) {
        if (result->W && missing==0 && !endPose.occupied) result->W=2.f;
    }
}

void EdgeShapes(IScriptable*, CStackFrame* stack,DynArray<CName>* result,int64_t) {
    Handle<game::HitRepresentationComponent> component;
    Vector4 from,to;
    GetParameter(stack,&component);GetParameter(stack,&from);GetParameter(stack,&to);++stack->code;
    if(!result) return;
    result->Clear();
    const Vector4 path[]{from,to,from,to};
    if(component) VisitSweepContacts(*component.instance,path,[&](const SweepObservation& obs) {
        const CName name=obs.item.name,binding=obs.binding;
        for(uint32_t i=0;i+1<result->Size();i+=2) if((*result)[i]==name && (*result)[i+1]==binding) return;
        result->PushBack(name);result->PushBack(binding);
    });
}

CName ShapeMaterial(const game::HitRepresentationComponent& component,const game::HitShapeContainer& item) {
    return item.physicsMaterial.name ? item.physicsMaterial.name : component.physicsMaterial;
}

// The sink owns these observations for one pose. No global scratch buffer or
// second lookup survives the native call. Acquire strong references in the VM.
void CollectOccupied(IScriptable*,CStackFrame* stack,Vector4* result,int64_t) {
    Handle<game::HitRepresentationComponent> component;
    Handle<IScriptable> sink;
    Vector4 from,to,worldEdge;
    ObservationFrame pin{};
    GetParameter(stack,&component);GetParameter(stack,&from);GetParameter(stack,&to);GetParameter(stack,&worldEdge);
    GetParameter(stack,&pin.binding);GetParameter(stack,&pin.frame);GetParameter(stack,&sink);++stack->code;
    if(!result) return;
    *result={0,0,0,-1};
    if(!component || !sink || sink->GetType()->name!=CName("VrBladeVisiblePose")) return;
    auto* keep=sink->GetType()->GetFunction("VRBladeKeepOccupied");
    if(!keep || keep->params.Size()!=11 || !keep->returnType) return;
    const Vector4 shaft[]{from,to,from,to};
    result->W=static_cast<float>(VisitSweepContacts(*component.instance,shaft,[&](SweepObservation obs) {
        ++result->X;
        if(!obs.item.name || !obs.binding || !obs.item.userData) return;
        auto name=obs.item.name;
        auto geometry=obs.item.shape;
        auto data=obs.item.userData;
        auto material=ShapeMaterial(*component.instance,obs.item);
        const auto edge=obs.frame.Orientation.Conjugate().Transform(worldEdge);
        Vector4 interval{obs.hit.fraction,obs.hit.depth,0,HasContactEdge(&obs.a.X,&obs.b.X,&edge.X) ? 1.f : 0.f};
        StackArgs_t args;
        args.emplace_back(nullptr,&name);args.emplace_back(nullptr,&obs.binding);
        args.emplace_back(nullptr,&geometry);args.emplace_back(nullptr,&data);args.emplace_back(nullptr,&material);
        args.emplace_back(nullptr,&obs.center);args.emplace_back(nullptr,&obs.axis);args.emplace_back(nullptr,&obs.frame);
        args.emplace_back(nullptr,&obs.a);args.emplace_back(nullptr,&obs.b);args.emplace_back(nullptr,&interval);
        bool retained=false;
        if(ExecuteFunction(sink.instance,keep,&retained,args)) {
            ++result->Y;
            if(retained) ++result->Z;
        }
    },&pin));
}

bool SameVector(const Vector4& a,const Vector4& b) {
    return a.X==b.X && a.Y==b.Y && a.Z==b.Z && a.W==b.W;
}

// HitShapeUserDataBase contains the authored reaction zone, shape type, tag,
// protection/weakspot flags, damage modifier and dismemberment part. Compare
// the actual reflected fields (including subclass fields) on copied handles.
bool EquivalentHitData(game::HitShapeUserData* a,game::HitShapeUserData* b) {
    if(!a || !b) return false;
    if(a==b) return true;
    auto* type=a->GetType();
    if(!type || type!=b->GetType()) return false;
    bool sawRegion=false;
    for(auto* cls=type;cls && cls->name!=CName(game::HitShapeUserData::NAME);cls=cls->parent) {
        if(cls->name==CName("HitShapeUserDataBase") && !cls->props.IsEmpty()) sawRegion=true;
        for(auto* prop : cls->props) {
            if(!prop || !prop->type || (prop->flags.inValueHolder && (!a->GetValueHolder() || !b->GetValueHolder())))
                return false;
            if(!prop->type->IsEqual(prop->GetValuePtr<void>(a),prop->GetValuePtr<void>(b),0)) return false;
        }
    }
    return sawRegion;
}

void SameHitData(IScriptable*,CStackFrame* stack,bool* result,int64_t) {
    Handle<game::HitShapeUserData> a,b;
    GetParameter(stack,&a);GetParameter(stack,&b);++stack->code;
    if(result) *result=EquivalentHitData(a.instance,b.instance);
}

void SameGeometry(IScriptable*,CStackFrame* stack,bool* result,int64_t) {
    Handle<ISerializable> first,second;
    GetParameter(stack,&first);GetParameter(stack,&second);++stack->code;
    if(result) *result=first && first.instance==second.instance;
}

void MatchesOccupied(IScriptable*,CStackFrame* stack,bool* result,int64_t) {
    Handle<game::HitRepresentationComponent> component;
    game::ShapeData hit{};
    CName binding,material;
    Handle<ISerializable> geometry;
    Handle<game::HitShapeUserData> data;
    Vector4 center,axis;
    GetParameter(stack,&component);GetParameter(stack,&hit);GetParameter(stack,&binding);GetParameter(stack,&geometry);
    GetParameter(stack,&data);GetParameter(stack,&material);GetParameter(stack,&center);GetParameter(stack,&axis);++stack->code;
    if(!result) return;
    *result=false;
    if(!component || !geometry || !data || !hit.userData || !hit.hitShapeName || !binding) return;
    CallbackShapeMatch match;
    VisitShapes(*component.instance,[&](const game::HitShapeContainer& item) {
        if(item.name!=hit.hitShapeName) return false;
        const auto slot=item.slotName ? item.slotName : item.name;
        Vector4 c,a;
        const bool same=slot==binding && ShapeMaterial(*component.instance,item)==material &&
            Capsule(item,c,a) && SameVector(c,center) && SameVector(a,axis) && EquivalentHitData(item.userData.instance,data.instance);
        match.Consider(same,item.userData.instance==hit.userData.instance);
        return false;
    });
    *result=match.Matches() && (match.exactObserved ||
        (EquivalentHitData(hit.userData.instance,data.instance) && hit.physicsMaterial && (!material || hit.physicsMaterial==material)));
}
void CapsuleResult(CStackFrame* frame, Vector4* result,bool interval) {
    Vector4 start, end, motion, edge, center, axis;
    int32_t material{};
    GetParameter(frame,&start); GetParameter(frame,&end); GetParameter(frame,&motion); GetParameter(frame,&edge);
    GetParameter(frame,&center); GetParameter(frame,&axis); GetParameter(frame,&material); ++frame->code;
    if (!result) return;
    ContactInput input{};
    auto copy = [](float* destination, const Vector4& vector) {
        destination[0] = vector.X; destination[1] = vector.Y; destination[2] = vector.Z;
    };
    copy(input.bladeStart,start); copy(input.bladeEnd,end); copy(input.motionEnd,motion); copy(input.edgeDirection,edge);
    input.material = static_cast<Material>(material);
    const float at[]{center.X,center.Y,center.Z}, direction[]{axis.X,axis.Y,axis.Z};
    const auto metrics = EvaluateCapsule(input, at, direction, center.W, axis.W);
    *result = interval ? Vector4{metrics.centerFraction,metrics.penetrationDepth,0,metrics.valid?1.f:0.f}
        : Vector4{metrics.penetrationDepth, metrics.incidence, metrics.edgeAlignment, metrics.resistance};
}
void CapsuleMetrics(IScriptable*, CStackFrame* frame, Vector4* result,int64_t) {CapsuleResult(frame,result,false);}
void CapsuleInterval(IScriptable*, CStackFrame* frame, Vector4* result,int64_t) {CapsuleResult(frame,result,true);}
void SubmitJoint(IScriptable*, CStackFrame* frame, bool* result, int64_t) {
    int32_t side{}; uint32_t serial{}; physics::TraceResult trace{}; Vector4 point,edge;
    GetParameter(frame,&side); GetParameter(frame,&serial); GetParameter(frame,&trace);
    GetParameter(frame,&point); GetParameter(frame,&edge); ++frame->code;
    const float at[]{point.X,point.Y,point.Z}, axis[]{edge.X,edge.Y,edge.Z};
    const bool dynamic=trace.proxyType==physics::ProxyType::Ragdoll || trace.proxyType==physics::ProxyType::PhysicalSystem;
    const bool requested=physhands::RequestBladeJoint(side,dynamic?serial:0,trace.proxyID,trace.actorIndex,at,axis);
    if(result) *result=requested && physhands::BladeJointBound(side,serial);
}
void ClearJoint(IScriptable*, CStackFrame* frame, void*, int64_t) {
    int32_t side{}; GetParameter(frame,&side); ++frame->code;
    physhands::RequestBladeJoint(side,0,0,0,nullptr,nullptr);
}
void TraceKind(IScriptable*,CStackFrame* frame,int32_t* result,int64_t) {
    physics::TraceResult trace{};GetParameter(frame,&trace);++frame->code;
    if(result) *result=trace.proxyID && trace.proxyID!=UINT32_MAX && trace.actorIndex<65536 ? int32_t(trace.proxyType) : 0;
}
void JointPoint(IScriptable*,CStackFrame* frame,Vector4* result,int64_t) {
    int32_t side{};uint32_t serial{};GetParameter(frame,&side);GetParameter(frame,&serial);++frame->code;
    float point[3]{};
    if(result) *result=physhands::ReadBladeJointPoint(side,serial,point)?Vector4{point[0],point[1],point[2],1}:Vector4{};
}
void RenewJoint(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    int32_t side{};uint32_t serial{};GetParameter(frame,&side);GetParameter(frame,&serial);++frame->code;
    if(result) *result=physhands::RenewBladeJoint(side,serial);
}
template<bool HoldDepth,bool Inserting=false>
void ApplySlideLoad(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    int32_t side{};uint32_t serial{};float resistance{};
    GetParameter(frame,&side);GetParameter(frame,&serial);GetParameter(frame,&resistance);++frame->code;
    if(result) *result=physhands::RenewBladeJoint(side,serial,resistance,HoldDepth,nullptr,Inserting);
}
void TrackingBasis(IScriptable*,CStackFrame* frame,Vector4* result,int64_t) {
    ++frame->code;
    if(!result) return;
    float position[3],orientation[4],yaw{};
    *result=VRIK_ReadCurrentBodyFrame(position,orientation,&yaw) && std::isfinite(yaw)
        ? Vector4{std::cos(yaw),std::sin(yaw),0,1}:Vector4{};
}
void RequestEdgeSlide(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    int32_t side{};uint32_t serial{};Vector4 edge;float load{};
    GetParameter(frame,&side);GetParameter(frame,&serial);GetParameter(frame,&edge);GetParameter(frame,&load);++frame->code;
    const float dir[]{edge.X,edge.Y,edge.Z};
    if(result) *result=physhands::RenewBladeJoint(side,serial,load,false,dir);
}
void DriveEdge(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    int32_t side{};uint32_t serial{};Vector4 localShaft;float advance{},load{};
    GetParameter(frame,&side);GetParameter(frame,&serial);GetParameter(frame,&localShaft);
    GetParameter(frame,&advance);GetParameter(frame,&load);++frame->code;
    if(!result) return;
    Vector localEdge;
    if(!(advance>=0.f && advance<=.35f) || !LocalCutEdge({localShaft.X,localShaft.Y,localShaft.Z},localEdge)) {
        *result=false;return;
    }
    const float dir[]{localEdge.x,localEdge.y,localEdge.z};
    *result=physhands::RenewBladeJoint(side,serial,load,false,dir,false,advance);
}
template<bool WithAxis>
void EdgePath(IScriptable*,CStackFrame* frame,DynArray<Vector4>* result,int64_t) {
    int32_t side{};uint32_t serial{};
    GetParameter(frame,&side);GetParameter(frame,&serial);++frame->code;
    if(!result) return;
    result->Clear();
    float from[3]{},to[3]{},axis[3]{};int stage{};
    const auto stroke=physhands::ReadBladeEdgePath(side,serial,from,to,axis,&stage);
    if(stroke || (WithAxis && stage)) {
        result->PushBack({from[0],from[1],from[2],float(stroke)});
        result->PushBack({to[0],to[1],to[2],1.f});
        if constexpr (WithAxis) result->PushBack({axis[0],axis[1],axis[2],float(stage)});
    }
}
void AnimatedAttachment(IScriptable*,CStackFrame* frame,Vector4* result,int64_t) {
    int32_t side{};uint32_t key{};Vector4 animated,station;Quaternion rotation;bool bind{};
    GetParameter(frame,&side);GetParameter(frame,&key);GetParameter(frame,&animated);GetParameter(frame,&rotation);
    GetParameter(frame,&station);GetParameter(frame,&bind);++frame->code;
    if(!result) return;
    float goal[3]{};
    const int status=physhands::UpdateBladeAttachment(side,key,&animated.X,&rotation.i,&station.X,bind,goal);
    *result={goal[0],goal[1],goal[2],float(status)};
}
void SlotEdgeSample(IScriptable*,CStackFrame* frame,DynArray<Vector4>* result,int64_t) {
    int32_t side{};uint32_t key{};
    GetParameter(frame,&side);GetParameter(frame,&key);++frame->code;
    if(!result) return;
    result->Clear();
    float offset[3],axis[3];
    if(auto stroke=physhands::ReadBladeSlotEdge(side,key,offset,axis)) {
        result->PushBack({offset[0],offset[1],offset[2],float(stroke)});
        result->PushBack({axis[0],axis[1],axis[2],0});
    }
}
void EdgeStrokeLive(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    int32_t side{};uint32_t serial{},stroke{};
    GetParameter(frame,&side);GetParameter(frame,&serial);GetParameter(frame,&stroke);++frame->code;
    if(result) *result=physhands::BladeEdgeStrokeLive(side,serial,stroke);
}
void FindPuppetJoint(IScriptable*,CStackFrame* frame,bool* result,int64_t) {
    Handle<ent::Entity> target;int32_t side=-1;uint32_t serial=0;Vector4 point,edge;
    GetParameter(frame,&target);GetParameter(frame,&side);GetParameter(frame,&serial);
    GetParameter(frame,&point);GetParameter(frame,&edge);++frame->code;
    if(!result) return;
    *result=false;
    if(!target || side<0 || side>1 || !serial || target->status!=EntityStatus::Attached) return;
    using Owner=Handle<ISerializable>(*)(uint32_t,uint32_t);
    static const auto getOwner=reinterpret_cast<Owner>(UniversalRelocBase::Resolve(2394822845));
    if(!getOwner) return;
    const auto owned=[&](uint32_t proxy) {
        if(proxy==0 || proxy==UINT32_MAX || debugview::ProxyKind(proxy)!=12) return false;
        for(uint32_t index=0;index<2;++index) if(getOwner(proxy,index).instance==target.instance) return true;
        return false;
    };
    struct Cached { WeakHandle<ent::Entity> target;uint32_t serial{},proxy{},body{};uint64_t retry{}; };
    static thread_local Cached requests[2];auto& request=requests[side]; const auto now=GetTickCount64();
    if(request.target.Lock().instance!=target.instance || request.serial!=serial) {
        request={};request.target=target;request.serial=serial;
    }
    const bool bound=physhands::BladeJointBound(side,serial);
    if((bound || now<request.retry) && request.proxy && owned(request.proxy)) {
        const float at[]{point.X,point.Y,point.Z},axis[]{edge.X,edge.Y,edge.Z};
        *result=physhands::RequestBladeJoint(side,serial,request.proxy,request.body,at,axis) && bound;return;
    }
    if(now<request.retry) return;
    request.retry=now+125; request.proxy=0;
    debugview::NearBody nearby[16]{};
    const float at[]{point.X,point.Y,point.Z},axis[]{edge.X,edge.Y,edge.Z};
    const int count=debugview::FindNear(point.X,point.Y,point.Z,.08f,nearby,16);
    static const bool trace=cvr::TestMarker("cpvr_blade_owner_trace.flag");
    static thread_local unsigned samples{};
    const bool report=trace && samples++<64;
    if(report) Log("[blade-owner] candidates=%d serial=%u\n",count,serial);
    uint32_t checked[16]{};unsigned visited=0;
    for(int candidate=0;candidate<count;++candidate) {
        auto proxy=nearby[candidate].proxy;
        if(std::find(checked,checked+visited,proxy)!=checked+visited) continue;
        checked[visited++]=proxy;
        if(!owned(proxy)) continue;
        if(report) Log("[blade-owner] matched entity serial=%u proxy=%u\n",serial,proxy);
        uint32_t body{};void* actor{},*scene{};float origin[3]{},gap{};
        if(!debugview::NearestBodyOf(proxy,at,.045f,&body,&actor,&scene,origin,&gap)) continue;
        request.proxy=proxy;request.body=body;
        if(report) Log("[blade-owner] exact candidate serial=%u body=%u gap=%.4f\n",serial,body,gap);
        *result=physhands::RequestBladeJoint(side,serial,proxy,body,at,axis) && physhands::BladeJointBound(side,serial);
        return;
    }
    physhands::RequestBladeJoint(side,0,0,0,nullptr,nullptr);
}
}

void RegisterBladeContactStateNative() {
    RED4ext::CRTTISystem::Get()->AddPostRegisterCallback([] {
        RegisterMeshInstanceProbe();
        auto* mapped=RED4ext::CGlobalFunction::Create("VRBladeSlotEdgeSample","VRBladeSlotEdgeSample",&SlotEdgeSample);
        mapped->flags={.isNative=true,.isStatic=true};mapped->SetReturnType("array:Vector4");
        mapped->AddParam("Int32","side");mapped->AddParam("Uint32","serial");
        RED4ext::CRTTISystem::Get()->RegisterFunction(mapped);
        auto* pin=RED4ext::CGlobalFunction::Create("VRBladeAnimatedAttachment","VRBladeAnimatedAttachment",&AnimatedAttachment);
        pin->flags={.isNative=true,.isStatic=true};pin->SetReturnType("Vector4");
        pin->AddParam("Int32","side");pin->AddParam("Uint32","key");pin->AddParam("Vector4","animated");pin->AddParam("Quaternion","rotation");
        pin->AddParam("Vector4","station");pin->AddParam("Bool","bind");RED4ext::CRTTISystem::Get()->RegisterFunction(pin);
        auto* insert=RED4ext::CGlobalFunction::Create("VRBladeInsertSlide","VRBladeInsertSlide",&ApplySlideLoad<false,true>);
        insert->flags={.isNative=true,.isStatic=true};insert->SetReturnType("Bool");
        insert->AddParam("Int32","side");insert->AddParam("Uint32","serial");insert->AddParam("Float","resistance");
        RED4ext::CRTTISystem::Get()->RegisterFunction(insert);
        auto* live=RED4ext::CGlobalFunction::Create("VRBladeEdgeStrokeLive","VRBladeEdgeStrokeLive",&EdgeStrokeLive);
        live->flags={.isNative=true,.isStatic=true};live->SetReturnType("Bool");
        live->AddParam("Int32","side");live->AddParam("Uint32","serial");live->AddParam("Uint32","stroke");
        RED4ext::CRTTISystem::Get()->RegisterFunction(live);
        auto* cut=RED4ext::CGlobalFunction::Create("VRBladeRequestEdgeSlide","VRBladeRequestEdgeSlide",&RequestEdgeSlide);
        cut->flags={.isNative=true,.isStatic=true};cut->SetReturnType("Bool");
        cut->AddParam("Int32","side");cut->AddParam("Uint32","serial");cut->AddParam("Vector4","edge");cut->AddParam("Float","load");
        RED4ext::CRTTISystem::Get()->RegisterFunction(cut);
        auto* drive=RED4ext::CGlobalFunction::Create("VRBladeDriveEdge","VRBladeDriveEdge",&DriveEdge);
        drive->flags={.isNative=true,.isStatic=true};drive->SetReturnType("Bool");
        drive->AddParam("Int32","side");drive->AddParam("Uint32","serial");drive->AddParam("Vector4","localShaft");
        drive->AddParam("Float","advance");drive->AddParam("Float","load");RED4ext::CRTTISystem::Get()->RegisterFunction(drive);
        for(bool withAxis : {false,true}) {
            const char* name=withAxis ? "VRBladeEdgeSample" : "VRBladeEdgePath";
            auto* path=RED4ext::CGlobalFunction::Create(name,name,withAxis ? &EdgePath<true> : &EdgePath<false>);
            path->flags={.isNative=true,.isStatic=true};path->SetReturnType("array:Vector4");
            path->AddParam("Int32","side");path->AddParam("Uint32","serial");RED4ext::CRTTISystem::Get()->RegisterFunction(path);
        }
        auto* point=RED4ext::CGlobalFunction::Create("VRBladeJointPoint","VRBladeJointPoint",&JointPoint);
        point->flags={.isNative=true,.isStatic=true};point->SetReturnType("Vector4");
        point->AddParam("Int32","side");point->AddParam("Uint32","serial");RED4ext::CRTTISystem::Get()->RegisterFunction(point);
        auto* renew=RED4ext::CGlobalFunction::Create("VRBladeRenewJoint","VRBladeRenewJoint",&RenewJoint);
        renew->flags={.isNative=true,.isStatic=true};renew->SetReturnType("Bool");
        renew->AddParam("Int32","side");renew->AddParam("Uint32","serial");RED4ext::CRTTISystem::Get()->RegisterFunction(renew);
        for (bool hold : {false,true}) {
            const char* name=hold ? "VRBladeHoldDepth" : "VRBladeApplySlideLoad";
            auto* load=RED4ext::CGlobalFunction::Create(name,name,hold ? &ApplySlideLoad<true> : &ApplySlideLoad<false>);
            load->flags={.isNative=true,.isStatic=true}; load->SetReturnType("Bool");
            load->AddParam("Int32","side");load->AddParam("Uint32","serial");load->AddParam("Float","resistance");
            RED4ext::CRTTISystem::Get()->RegisterFunction(load);
        }
        auto* basis=RED4ext::CGlobalFunction::Create("VRBladeTrackingBasis","VRBladeTrackingBasis",&TrackingBasis);
        basis->flags={.isNative=true,.isStatic=true};basis->SetReturnType("Vector4");RED4ext::CRTTISystem::Get()->RegisterFunction(basis);
        auto* read = RED4ext::CGlobalFunction::Create("VRBladeReadCapsule", "VRBladeReadCapsule", &ReadCapsule<false>);
        auto* bound=RED4ext::CGlobalFunction::Create("VRBladeReadSlotCapsule","VRBladeReadSlotCapsule",&ReadCapsule<true>);
        bound->flags={.isNative=true,.isStatic=true};bound->SetReturnType("array:Vector4");
        bound->AddParam("handle:gameHitRepresentationComponent","component");bound->AddParam("CName","name");
        bound->AddParam("handle:gameHitShapeUserData","data");bound->AddParam("CName","binding");
        RED4ext::CRTTISystem::Get()->RegisterFunction(bound);
        auto* shapes=RED4ext::CGlobalFunction::Create("VRBladeEdgeShapes","VRBladeEdgeShapes",&EdgeShapes);
        shapes->flags={.isNative=true,.isStatic=true};shapes->SetReturnType("array:CName");
        shapes->AddParam("handle:gameHitRepresentationComponent","component");shapes->AddParam("Vector4","from");shapes->AddParam("Vector4","to");
        RED4ext::CRTTISystem::Get()->RegisterFunction(shapes);
        auto* collect=CGlobalFunction::Create("VRBladeCollectOccupied","VRBladeCollectOccupied",&CollectOccupied);
        collect->flags={.isNative=true,.isStatic=true};collect->SetReturnType("Vector4");
        collect->AddParam("handle:gameHitRepresentationComponent","component");
        for(const char* param : {"from","to","edge"}) collect->AddParam("Vector4",param);
        collect->AddParam("CName","pinSlot");collect->AddParam("WorldTransform","pinFrame");
        collect->AddParam("handle:IScriptable","sink");CRTTISystem::Get()->RegisterFunction(collect);
        auto* same=CGlobalFunction::Create("VRBladeSameGeometry","VRBladeSameGeometry",&SameGeometry);
        same->flags={.isNative=true,.isStatic=true};same->SetReturnType("Bool");
        same->AddParam("handle:ISerializable","first");same->AddParam("handle:ISerializable","second");
        CRTTISystem::Get()->RegisterFunction(same);
        auto* sameData=CGlobalFunction::Create("VRBladeSameHitData","VRBladeSameHitData",&SameHitData);
        sameData->flags={.isNative=true,.isStatic=true};sameData->SetReturnType("Bool");
        sameData->AddParam("handle:gameHitShapeUserData","first");sameData->AddParam("handle:gameHitShapeUserData","second");
        CRTTISystem::Get()->RegisterFunction(sameData);
        auto* match=CGlobalFunction::Create("VRBladeMatchesOccupied","VRBladeMatchesOccupied",&MatchesOccupied);
        match->flags={.isNative=true,.isStatic=true};match->SetReturnType("Bool");
        match->AddParam("handle:gameHitRepresentationComponent","component");match->AddParam("gameShapeData","hit");
        match->AddParam("CName","slot");match->AddParam("handle:ISerializable","geometry");
        match->AddParam("handle:gameHitShapeUserData","data");match->AddParam("CName","material");
        match->AddParam("Vector4","center");match->AddParam("Vector4","axis");CRTTISystem::Get()->RegisterFunction(match);
        for (bool entry : {false,true}) {
            const char* name=entry ? "VRBladeEntrySweep" : "VRBladeSweptContact";
            auto* sweep=CGlobalFunction::Create(name,name,entry ? &SweptContact<true> : &SweptContact<false>);
            sweep->flags={.isNative=true,.isStatic=true};
            sweep->AddParam("handle:gameHitRepresentationComponent","component");
            for (const char* param : {"oldA","oldB","a","b"}) sweep->AddParam("Vector4",param);
            if (entry) sweep->AddParam("Vector4","endGrip");
            sweep->SetReturnType("Vector4");CRTTISystem::Get()->RegisterFunction(sweep);
        }
        read->flags = {.isNative=true,.isStatic=true};
        read->AddParam("handle:gameHitRepresentationComponent","component"); read->AddParam("CName","name");
        read->AddParam("handle:gameHitShapeUserData","data"); read->SetReturnType("array:Vector4");
        RED4ext::CRTTISystem::Get()->RegisterFunction(read);
        for(bool interval:{false,true}) {
            const char* name=interval?"VRBladeCapsuleInterval":"VRBladeCapsuleMetrics";
            auto* evaluate = RED4ext::CGlobalFunction::Create(name,name,interval?&CapsuleInterval:&CapsuleMetrics);
            evaluate->flags = {.isNative=true,.isStatic=true};
            for (const char* param : {"start","end","motion","edge","center","axis"}) evaluate->AddParam("Vector4",param);
            evaluate->AddParam("Int32","material"); evaluate->SetReturnType("Vector4");
            RED4ext::CRTTISystem::Get()->RegisterFunction(evaluate);
        }
        auto* submit=RED4ext::CGlobalFunction::Create("VRBladeSubmitJoint","VRBladeSubmitJoint",&SubmitJoint);
        submit->flags={.isNative=true,.isStatic=true};
        submit->AddParam("Int32","side");submit->AddParam("Uint32","serial");submit->AddParam("physicsTraceResult","trace");
        submit->AddParam("Vector4","point");submit->AddParam("Vector4","edge");submit->SetReturnType("Bool");
        RED4ext::CRTTISystem::Get()->RegisterFunction(submit);
        auto* clear=RED4ext::CGlobalFunction::Create("VRBladeClearJoint","VRBladeClearJoint",&ClearJoint);
        clear->flags={.isNative=true,.isStatic=true};clear->AddParam("Int32","side");RED4ext::CRTTISystem::Get()->RegisterFunction(clear);
        auto* dynamic=RED4ext::CGlobalFunction::Create("VRBladeTraceKind","VRBladeTraceKind",&TraceKind);
        dynamic->flags={.isNative=true,.isStatic=true};dynamic->AddParam("physicsTraceResult","trace");dynamic->SetReturnType("Int32");
        RED4ext::CRTTISystem::Get()->RegisterFunction(dynamic);
        auto* find=RED4ext::CGlobalFunction::Create("VRBladeFindPuppetJoint","VRBladeFindPuppetJoint",&FindPuppetJoint);
        find->flags={.isNative=true,.isStatic=true};find->AddParam("handle:entEntity","target");find->AddParam("Int32","side");
        find->AddParam("Uint32","serial");find->AddParam("Vector4","point");find->AddParam("Vector4","edge");
        find->SetReturnType("Bool");RED4ext::CRTTISystem::Get()->RegisterFunction(find);
    });
}
}
