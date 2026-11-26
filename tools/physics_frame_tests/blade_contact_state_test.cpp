#include "Anim/BladeContactState.hpp"
#include "Anim/BladeAttachment.hpp"

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>
#include <utility>

using cvr::blade::ContactInput;
using cvr::blade::DampSlide;
using cvr::blade::EvaluateContact;
using cvr::blade::Material;

static void Check(bool condition, const char* message) {
    if (!condition) {
        std::cerr << message << '\n';
        std::exit(1);
    }
}

static void CheckAnimatedAttachment() {
    using namespace cvr::handspace;
    const Rotation identity{},quarterTurn{0,0,.70710678f,.70710678f};
    const Vector origin{},localStation{.02f,0,0},base{.02f,0,-.55f},tip{.02f,0,.20f};
    const Vector initialWrist{-.03f,.04f,-.55f};
    cvr::blade::BladeAttachment pin;
    pin.BindWrist(localStation,initialWrist,identity);
    pin.BindRail(origin,identity,identity);
    cvr::blade::RailTravel slide;
    auto overlap=[&](Vector grip,Rotation wristQ,Vector target,Rotation slotQ) {
        const auto inverse=Conjugate(slotQ);
        auto bound=Rotate(inverse,grip+Rotate(wristQ,base-initialWrist)-target);
        auto end=Rotate(inverse,grip+Rotate(wristQ,tip-initialWrist)-target);
        ContactInput input{};
        const float a[]{bound.x,bound.y,bound.z},b[]{end.x,end.y,end.z};
        for(int i=0;i<3;++i) {input.bladeStart[i]=a[i];input.bladeEnd[i]=b[i];}
        input.edgeDirection[1]=1;
        const float center[]{.02f,0,0},up[]{0,0,1};
        return cvr::blade::EvaluateCapsule(input,center,up,.08f,.09f).penetrationDepth;
    };
    // The animated actor steps almost a metre with the controller still. The
    // old render wrist misses every capsule; translation and rotation of the
    // actual slot both carry the material point of the pinned visible weapon.
    const Vector target{.82f,-.38f,.025f},animated=target+Rotate(quarterTurn,localStation);
    slide.Sample({.13f,0,0},{0,0,1},false);
    auto goal=pin.Goal(animated,quarterTurn,slide.total);
    auto wrist=pin.Wrist(goal,quarterTurn);
    Check(LengthSquared(wrist+Rotate(quarterTurn,pin.wristLocal)-animated)<1.e-10f,"slot and visible material point diverged under rotation");
    Check(overlap(initialWrist,identity,target,quarterTurn)==0 && overlap(wrist,quarterTurn,target,quarterTurn)>.2f,
          "target reaction retained only the joint rather than visible capsule overlap");
    // A locked flat press contributes no travel. In edge mode only actual
    // travel along the free physical axis changes the target-local station.
    slide.Sample({.13f,.04f,.01f},{0,1,0},true);
    goal=pin.Goal(animated,quarterTurn,slide.total);
    Check(LengthSquared(goal-animated-Rotate(quarterTurn,{0,.04f,0}))<1.e-10f,"flat or shaft solver error entered the edge offset");
    // Reacquiring a new body starts its solver odometer at zero.
    pin.BindRail(origin,quarterTurn,quarterTurn);
    Check(LengthSquared(pin.Goal(animated,quarterTurn,origin)-goal)<1.e-10f,"rebind reset the occupied station");
    const auto withdrawn=pin.Wrist(pin.Goal(animated,quarterTurn,{0,0,-.7f}),quarterTurn);
    Check(overlap(withdrawn,quarterTurn,target,quarterTurn)==0,"presentation pin prevented withdrawal past the tip");
}

static void CheckEdgeActuator() {
    using namespace cvr::handspace;
    Vector localEdge;
    Check(cvr::blade::LocalCutEdge({0,.05f,.75f},localEdge) && localEdge.y>0 &&
          std::abs(.05f*localEdge.y+.75f*localEdge.z)<1.e-5f,"off-axis marker changed cutting side");
    Check(!cvr::blade::LocalCutEdge({0,1,0},localEdge),"parallel shaft and edge created a cut frame");
    const Vector shaft{0,0,1};
    Check(cvr::blade::LocalCutEdge(shaft,localEdge),"missing +Y item edge");
    const Rotation quarterX{.70710678f,0,0,.70710678f},quarterZ{0,0,.70710678f,.70710678f};
    const Vector physicalShaft=Rotate(quarterX,shaft),physicalEdge=Rotate(quarterX,localEdge);
    Check(physicalEdge.z>.999f && physicalShaft.y<-.999f,"rotated edge was compared to a stale world shaft");
    const Rotation handFrame=quarterZ,bodyFrame=Multiply(quarterX,handFrame);
    const Vector bodyPoint{2,.9f,-.1f},handAnchor{-.14f,.22f,.57f};
    Rotation wrist;
    for(float advance : {0.f,.008f,.093f}) {
        const auto target=cvr::blade::EdgeDriveWrist(bodyPoint,bodyFrame,handAnchor,handFrame,advance,wrist);
        const auto resultingStation=target+Rotate(wrist,handAnchor);
        Check(LengthSquared(resultingStation-bodyPoint-physicalEdge*advance)<1.e-10f,
              "wrist goal released old tracking error or commanded the wrong edge sign");
    }
    const auto movedFrame=Multiply(quarterZ,bodyFrame);
    const Vector movedPoint=bodyPoint+Vector{.75f,-.3f,.1f};
    const auto target=cvr::blade::EdgeDriveWrist(movedPoint,movedFrame,handAnchor,handFrame,.093f,wrist);
    const auto relative=Rotate(Conjugate(movedFrame),target+Rotate(wrist,handAnchor)-movedPoint);
    Check(LengthSquared(relative-Vector{.093f,0,0})<1.e-10f,"moving target generated a different relative command");
}

int main() {
    CheckAnimatedAttachment();
    CheckEdgeActuator();
    {
        using cvr::handspace::Vector;
        const cvr::blade::BladeWristLeash leash{{0,0,0},{0,.05f,0}};
        constexpr float wristCap=.12f;
        Vector pose{.012f,0,0};
        Check(leash.Limit(leash.controller,pose) && pose.x==.012f,"small idle motion lost");
        pose={.13f,0,0};
        Check(leash.Limit(leash.controller,pose) && pose.x>.012f && pose.x<wristCap,"NPC reaction dragged a still wrist");
        for(float gap : {.479f,.48f}) {
            pose={gap,0,0};
            Check(leash.Limit(leash.controller,pose) && std::abs(pose.x-wristCap)<1.e-6f,"capped wrist should stay attached through 48 cm");
        }
        pose={.481f,0,0};
        Check(!leash.Limit(leash.controller,pose) && std::abs(pose.x-wristCap)<1.e-6f,"wrist should release beyond 48 cm with travel capped");
        pose={.14f,.46f,0};
        Check(leash.Limit({.14f,.05f,0},pose) && pose.x==.14f && std::abs(pose.y-wristCap)<1.e-6f,"input on one axis excused transverse drag");
        pose={.12f,0,0};
        Check(leash.Limit({.30f,.05f,0},pose) && pose.x==.12f,"insertion resistance was pulled up to raw tracking");
        pose={.5f,0,0};
        Check(leash.Limit({.5f,.05f,0},pose) && pose.x==.5f,"common travel released the contact");
        Check(cvr::blade::ReleaseWrist({.3f,0,0},{.1f,0,0},{-.05f,0,0},0).x==.05f,"release starts at the physics body's displaced position");
        Check(std::abs(cvr::blade::ReleaseWrist({.3f,0,0},{.1f,0,0},{-.05f,0,0},1).x-.3f)<1.e-6f,"release did not return to the free hand");
    }
    const float shaft[]{0,0,1},edge[]{0,1,.04f};
    float freeAxis[3]{};
    Check(cvr::blade::SelectRailAxis(shaft,edge,freeAxis) && freeAxis[1]>.9999f && freeAxis[2]==0,
          "edge mode must lock insertion depth, including a slightly skewed authored edge");
    Check(cvr::blade::SelectRailAxis(shaft,nullptr,freeAxis) && freeAxis[2]==1,"withdrawal must restore shaft motion");
    Check(!cvr::blade::SelectRailAxis(shaft,shaft,freeAxis),"parallel shaft and edge enabled cutting");
    int modes[3]{2,2,2};
    const auto set=[&](int i,int value){modes[i]=value;};
    const auto read=[&](int i){return modes[i];};
    Check(cvr::blade::ConfigureRailTranslations(false,set,read) && modes[0]==2 && !modes[1] && !modes[2],"slide axes");
    Check(cvr::blade::ConfigureRailTranslations(true,set,read) && !modes[0] && !modes[1] && !modes[2],"depth lock axes");
    Check(!cvr::blade::ConfigureRailTranslations(false,set,[&](int i){return i==2 ? 2 : modes[i];}),"bad flat-axis readback");
    const float h[]{1,2,3},reaction[]{1.10f,2.07f,3.02f};
    float shaftPoint[3]{};
    Check(std::abs(cvr::blade::ShaftPoint(h,reaction,shaft,shaftPoint)-.02f)<1e-5f &&
          shaftPoint[0]==1 && shaftPoint[1]==2,"transverse puppet reaction counted as withdrawal");
    using cvr::blade::UsesShapeResource;
    Check(UsesShapeResource(false,false) && UsesShapeResource(true,true) && !UsesShapeResource(false,true),
          "resource-only puppet or component-local override selected incorrectly");
    cvr::blade::CapsuleLookup lookup;
    const std::array<float,8> first{0,0,0,.10f,1,0,0,.12f}, other{0,0,0,.12f,1,0,0,.12f};
    lookup.Consider(first,false);lookup.Consider(first,false);
    Check(lookup.found && !lookup.ambiguous,"cloned user data with unique geometry");
    lookup.Consider(other,false);
    Check(lookup.ambiguous,"ambiguous named capsule accepted");
    Check(lookup.Consider(first,true) && !lookup.ambiguous && lookup.geometry==first,"exact handle lost to name fallback");
    cvr::blade::CallbackShapeMatch match;
    Check(!match.Matches(),"missing observed container accepted");
    match.Consider(true,false);
    Check(match.Matches(),"unique observed container rejected for cloned callback");
    match.Consider(false,false);
    Check(!match.Matches(),"same-name neighbor accepted for cloned callback");
    match.Consider(true,true);
    Check(match.Matches(),"exact observed data lost to a same-name neighbor");
    match.Consider(false,true);
    Check(!match.Matches(),"shared data handle resolved two distinct containers");
    match={};match.Consider(false,true);match.Consider(true,false);
    Check(!match.Matches(),"foreign exact callback assigned to observed shape");
    const float start[]{0,0,0},end[]{0,0,0.75f},flat[]{0,1,0};
    Check(cvr::blade::HasContactEdge(start,end,flat) && !cvr::blade::HasContactEdge(start,end,end)
          && !cvr::blade::HasContactEdge(start,start,flat),"occupied snapshot lost the authored edge projection gate");
    const float bladeHalf[]{.02f,.01f,.35f};
    const float embedded[]{0,0,.30f}, pastTip[]{0,0,.40f}, offBlade[]{.07f,0,.10f};
    Check(cvr::blade::InsideWeaponBox(embedded,bladeHalf), "partial withdrawal detached");
    Check(!cvr::blade::InsideWeaponBox(pastTip,bladeHalf), "shaft slid completely off its joint anchor");
    Check(!cvr::blade::InsideWeaponBox(offBlade,bladeHalf), "separated actor kept joint");
    // At the two bound-05 attempts the D6 point was this far off the visual
    // shaft (x=0,y=0). A native hit capsule must still clip the solver path.
    for (const auto [gap,travel] : {std::pair{.0891f,.017f},std::pair{.0936f,.083f}}) {
        ContactInput press{};
        press.bladeStart[0]=-travel;
        press.bladeStart[1]=press.bladeEnd[1]=gap;
        press.bladeStart[2]=press.bladeEnd[2]=.57f;
        press.edgeDirection[2]=1;
        const float capsule[]{0,gap,.57f}, upAxis[]{0,0,1}, clear[]{0,-gap,.57f};
        auto hit=cvr::blade::SweepCapsule(press.bladeStart,press.bladeEnd,press.bladeStart,press.bladeEnd,capsule,upAxis,.012f,.025f);
        auto interval=cvr::blade::EvaluateCapsule(press,capsule,upAxis,.012f,.025f);
        Check(hit.depth>.001f && interval.valid && std::abs(hit.depth-interval.penetrationDepth)<1e-5f,
              "solver path off the render line failed to clip the contacted capsule");
        Check(!cvr::blade::EvaluateCapsule(press,clear,upAxis,.012f,.025f).valid,
              "edge travel in air authorized a clipped interval");
    }
    const float beforeA[]{-.48f,0,-.4f}, beforeB[]{-.48f,0,.35f};
    // Same finite travel, two different material lines. Only the observed
    // blade station at y=0 enters this capsule. A parallel D6 path 69 mm away misses.
    {
        ContactInput span{};
        const float region[]{0,0,.57f}, along[]{0,0,1};
        span.bladeStart[2]=span.bladeEnd[2]=.57f;
        span.edgeDirection[2]=1;
        span.bladeStart[0]=-.095f;
        span.bladeStart[1]=span.bladeEnd[1]=.069f;
        Check(!cvr::blade::EvaluateCapsule(span,region,along,.025f,.08f).valid,"remote D6 segment clipped animated capsule");
        span.bladeStart[1]=span.bladeEnd[1]=0;
        const auto visible=cvr::blade::EvaluateCapsule(span,region,along,.025f,.08f);
        Check(visible.valid && std::abs(visible.penetrationDepth-.025f)<1.e-5f,"visible station was not clipped at the capsule boundary");
    }
    const float afterA[]{.52f,0,-.4f}, afterB[]{.52f,0,.35f};
    const float origin[]{0,0,0}, up[]{0,0,1};
    auto sweep=[&](const float* a,const float* b,const float* c,const float* d) {
        return cvr::blade::SweepCapsule(a,b,c,d,origin,up,.06f,.10f);
    };
    Check(!sweep(beforeA,beforeB,beforeA,beforeB).depth,"clear pose inside broad proxy produced contact");
    const auto crossingHit=sweep(beforeA,beforeB,afterA,afterB);
    Check(crossingHit.time>.41f && crossingHit.time<.45f && crossingHit.depth>0,"fast crossing lost between two clear poses");
    const auto reverseHit=sweep(afterA,afterB,beforeA,beforeB);
    Check(reverseHit.time>.45f && reverseHit.time<.49f,"reverse sweep lost");
    Check(!sweep(afterA,afterB,afterA,afterB).depth,"reverse turning pose must be independently clear");
    // A different slot can still contain the heel after the active 90% of the
    // shaft has passed through. The complete end pose owns the contact latch.
    const float heelSlot[]{.52f,0,-.37f},strikeStart[]{.52f,0,-.325f};
    auto atHeel=[&](const float* from) {
        return cvr::blade::SweepCapsule(from,afterB,from,afterB,heelSlot,up,.018f,0);
    };
    Check(!atHeel(strikeStart).depth && atHeel(afterA).depth>.03f,
          "end-pose witness dropped a neighboring capsule at the grip");
    const float enterA[]{0,0,-.85f}, enterB[]{0,0,-.20f}, endA[]{0,0,-.55f}, endB[]{0,0,.10f};
    Check(sweep(enterA,enterB,endA,endB).depth>0,"tip-first thrust lost");
    ContactInput flesh{};
    flesh.entry[0] = 0.0f; flesh.exit[0] = 0.08f;
    flesh.bladeStart[0] = -0.10f; flesh.bladeEnd[0] = 0.30f;
    flesh.motionStart[1] = -0.10f; flesh.motionEnd[1] = 0.10f;
    flesh.surfaceNormal[0] = 1.0f; flesh.edgeDirection[1] = 1.0f;
    flesh.material = Material::Flesh;
    const auto shallow = EvaluateContact(flesh);
    Check(shallow.valid, "valid flesh contact rejected");
    Check(std::fabs(shallow.penetrationDepth - 0.08f) < 1.0e-5f, "penetration depth was not measured");
    Check(shallow.edgeAlignment > 0.99f, "edge-aligned travel was not detected");

    flesh.exit[0] = 0.24f;
    const auto deep = EvaluateContact(flesh);
    Check(deep.resistance > shallow.resistance, "deeper contact did not increase resistance");

    flesh.material = Material::Metal;
    const auto metal = EvaluateContact(flesh);
    Check(metal.resistance > deep.resistance, "material resistance did not increase");

    flesh.edgeDirection[2] = 1.0f; flesh.edgeDirection[1] = 0.0f;
    const auto crossEdge = EvaluateContact(flesh);
    Check(crossEdge.edgeAlignment < 0.01f, "cross-edge motion was accepted as an edge cut");
    flesh.edgeDirection[2] = 0; flesh.edgeDirection[1] = 1;
    flesh.bladeEnd[0] = 0.012f;
    Check(std::fabs(EvaluateContact(flesh).penetrationDepth - .012f) < 1e-5f, "shallow tip credited the entire collider");
    flesh.bladeStart[0] = .004f;
    Check(std::fabs(EvaluateContact(flesh).penetrationDepth - .008f) < 1e-5f, "fully embedded short segment");
    flesh.motionEnd[1] = flesh.motionStart[1];
    Check(EvaluateContact(flesh).edgeAlignment == 0, "stationary contact generated motion");
    flesh.motionEnd[1] -= 0.03f;
    Check(EvaluateContact(flesh).edgeAlignment == 0, "back edge generated a cut");
    flesh.surfaceNormal[0] = 0; flesh.surfaceNormal[1] = 1;
    const auto grazing = EvaluateContact(flesh);
    flesh.surfaceNormal[0] = 1; flesh.surfaceNormal[1] = 0;
    Check(grazing.incidence == 0 && grazing.resistance < EvaluateContact(flesh).resistance, "incidence ignored");
    flesh.exit[1] = .1f;
    Check(!EvaluateContact(flesh).valid, "different ray accepted");
    flesh.exit[1] = 0; flesh.exit[0] = -.03f;
    Check(!EvaluateContact(flesh).valid, "reversed interval accepted");
    flesh.exit[0] = .04f; flesh.entry[0] = .03f;
    Check(!EvaluateContact(flesh).valid, "interval beyond tip accepted");
    flesh.entry[0] = std::numeric_limits<float>::quiet_NaN();
    Check(!EvaluateContact(flesh).valid, "NaN entry accepted");
    const float center[]{1,2,3}, along[]{0,0,1};
    ContactInput crossing{};
    crossing.bladeStart[0]=.75f; crossing.bladeStart[1]=2; crossing.bladeStart[2]=3;
    crossing.bladeEnd[0]=1.02f; crossing.bladeEnd[1]=2; crossing.bladeEnd[2]=3;
    crossing.edgeDirection[1]=1; crossing.motionEnd[1]=.1f;
    auto capsuleDepth=[&](float halfHeight){return cvr::blade::EvaluateCapsule(crossing,center,along,.10f,halfHeight);};
    Check(std::abs(capsuleDepth(.15f).penetrationDepth-.12f)<1e-5f,"capsule side entry with tip inside");
    Check(std::abs(.75f+.27f*capsuleDepth(.15f).centerFraction-.96f)<1e-5f,"joint anchor not inside the measured interval");
    crossing.bladeEnd[0]=1.3f;
    Check(std::abs(capsuleDepth(.15f).penetrationDepth-.2f)<1e-5f,"through-cut counted air beyond capsule");
    crossing.bladeStart[2]=crossing.bladeEnd[2]=3.21f;
    Check(std::abs(capsuleDepth(.15f).penetrationDepth-.16f)<1e-5f,"rounded capsule cap");
    crossing.bladeStart[2]=crossing.bladeEnd[2]=3.27f;
    Check(!capsuleDepth(.15f).valid,"extension of cylinder beyond cap");
    crossing.bladeStart[0]=crossing.bladeEnd[0]=1; crossing.bladeStart[2]=2.6f; crossing.bladeEnd[2]=3.4f;
    Check(std::abs(capsuleDepth(.15f).penetrationDepth-.5f)<1e-5f,"axial ray through both caps");
    Check(std::abs(capsuleDepth(0.f).penetrationDepth-.2f)<1e-5f,"degenerate capsule sphere");
    const float slideAxis[]{0,1,0};
    float handSpeed[]{2,1,4},bodySpeed[]{3,0,5};
    Check(!DampSlide(handSpeed,bodySpeed,slideAxis,2,5,0,.016f),"free rail applied drag");
    Check(DampSlide(handSpeed,bodySpeed,slideAxis,2,5,4,.016f),"active rail applied no drag");
    Check(handSpeed[1]<1 && bodySpeed[1]>0 && std::abs(2*handSpeed[1]+5*bodySpeed[1]-2)<1e-6f,"bilateral rail momentum");
    Check(handSpeed[0]==2 && handSpeed[2]==4 && bodySpeed[0]==3 && bodySpeed[2]==5,"drag changed a transverse velocity");
    for(int hz:{30,60,90,144}) {
        float movingProp[]{1,3,5},animatedActor[]{4,1,6};
        for(int i=0;i<hz;++i) DampSlide(movingProp,animatedActor,slideAxis,2,0,4,1.f/hz,true);
        Check(std::abs(movingProp[1]-(1.f+2.f*std::exp(-4.f)))<1e-5f,"animated rail damping changed with physics rate");
        Check(animatedActor[1]==1 && movingProp[0]==1 && movingProp[2]==5,"animated rail modified unrelated motion");
        movingProp[1]=1.f;
        Check(!DampSlide(movingProp,animatedActor,slideAxis,2,0,4,1.f/hz,true),"co-moving hand acquired drag");
    }
    std::cout << "blade contact state: depth, material, incidence, edge alignment and bilateral drag PASS\n";
}
