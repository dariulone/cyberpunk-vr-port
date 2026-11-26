#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <cmath>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <vector>
using J=nlohmann::json;
constexpr double Pi=3.14159265358979323846;
using V=std::array<double,3>;
namespace optics {
V Add(V a,V b){for(int i=0;i<3;++i)a[i]+=b[i];return a;}
V Scale(V a,double n){for(auto& x:a)x*=n;return a;}
double Dot(V a,V b){double result=0;for(int i=0;i<3;++i)result+=a[i]*b[i];return result;}
double Length(V a){return std::sqrt(Dot(a,a));}
V Vector(const J& value){V out=value.get<V>();for(double x:out)if(!std::isfinite(x))throw std::runtime_error("non-finite optic position/axis");return out;}
V Unit(V a){const auto n=Length(a);if(!std::isfinite(n)||n<1e-9)throw std::runtime_error("invalid optic axis");return Scale(a,1/n);}
V Rotate(const std::array<double,4>& q,V v){
 const V u{q[0],q[1],q[2]},cross{u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]};
 return Add(Add(Scale(u,2*Dot(u,v)),Scale(v,q[3]*q[3]-Dot(u,u))),Scale(cross,2*q[3]));
}
std::array<double,4> Quaternion(const J& value){auto q=value.get<std::array<double,4>>();double n=0;for(double x:q)n+=x*x;
 if(!std::isfinite(n)||n<1e-12)throw std::runtime_error("invalid orientation");for(auto& x:q)x/=std::sqrt(n);return q;}
J Bind(const J& input){
 const auto& root=input.at("Data").at("RootChunk");
 if(root.at("boneNames").size()!=1||root.at("boneNames")[0].at("$value")!="scope_hinge")throw std::runtime_error("expected single scope_hinge skin");
 const auto& matrix=root.at("boneRigMatrices").at(0);const char* axes[]{"X","Y","Z"};
 auto transform=[&](V p,bool point){V out{};for(int c=0;c<3;++c){out[c]=point?matrix.at("W").at(axes[c]).get<double>():0;
  for(int r=0;r<3;++r)out[c]+=p[r]*matrix.at(axes[r]).at(axes[c]).get<double>();}return out;};
 V center{},size{};for(int c=0;c<3;++c){const double lo=root.at("boundingBox").at("Min").at(axes[c]),hi=root.at("boundingBox").at("Max").at(axes[c]);center[c]=(lo+hi)*.5;size[c]=hi-lo;}
 const V right=Unit(transform({1,0,0},false)),forward=Unit(transform({0,1,0},false)),up=Unit(transform({0,0,1},false));
 const double trace=right[0]+forward[1]+up[2];std::array<double,4> rotation{};
 if(trace>0){const double s=2*std::sqrt(trace+1);rotation={(forward[2]-up[1])/s,(up[0]-right[2])/s,(right[1]-forward[0])/s,s*.25};}
 else throw std::runtime_error("unexpected Osprey bind rotation; inspect authored frame before extending");
 return {{"bone","scope_hinge"},{"slot","vrp_scope_hinge"},{"meshCenter",center},{"meshSize",size},
         {"point",transform(center,true)},{"forward",Unit(transform({0,1,0},false))},
         {"right",right},{"up",up},{"rotation",rotation},
         {"convention","row-vector mesh * inverseBind * live bone world"}};
}
J Measure(const J& input){
 const auto& camera=input.at("camera");const auto& weapon=input.at("weapon");
 const V eye=Vector(camera.at("p")),forward=Unit(Vector(camera.at("forward"))),right=Unit(Vector(camera.at("right")));
 if(std::abs(Dot(forward,right))>.02)throw std::runtime_error("camera basis is not orthogonal");
 const double half=camera.at("halfIpd"),main=camera.at("mainEyeOffset");
 if(!std::isfinite(half+main)||half<.02||half>.05||std::abs(main)>half+.001)throw std::runtime_error("invalid measured eye offsets");
 const auto q=Quaternion(weapon.at("q"));const V bore=Unit(Rotate(q,{0,1,0}));
 V scope=Add(Vector(weapon.at("p")),Rotate(q,Vector(input.at("scopeOffset")))),axis=bore;
 const bool skinned=input.contains("binding");
 if(skinned){const auto& binding=input.at("binding");const auto& bone=input.at("scopeBone");
  if(!bone.is_object()||bone.at("slot")!=binding.at("slot"))throw std::runtime_error("actual optic skin slot unavailable");
  const auto bq=Quaternion(bone.at("q"));scope=Add(Vector(bone.at("p")),Rotate(bq,Vector(binding.at("point"))));axis=Unit(Rotate(bq,Vector(binding.at("forward"))));}
 const double angle=std::acos(std::clamp(Dot(forward,axis),-1.,1.))*180/Pi;
 J out={{"boreAngleDegrees",std::acos(std::clamp(Dot(forward,bore),-1.,1.))*180/Pi},{"opticAxisAngleDegrees",angle},
        {"opticalFrameSource",skinned?"animated-skin-slot":"weapon-root-only"},{"scopePoint",scope},{"opticForward",axis},{"boreForward",bore},{"eyes",J::array()},
        {"ray",{{"a",eye},{"b",Add(eye,Scale(forward,150))}}},{"aligned",false}};
 double best=1e9;int bestEye=-1;
 for(int side=0;side<2;++side){const V position=Add(eye,Scale(right,(side?half:-half)-main)),delta=Add(position,Scale(scope,-1));
  const double relief=-Dot(delta,axis),distance=Length(Add(delta,Scale(axis,relief)));const bool eligible=relief>=.06&&relief<=.30;
  out["eyes"].push_back({{"eye",side},{"position",position},{"axisDistanceMeters",distance},{"reliefMeters",relief},{"reliefValid",eligible}});
  if(eligible&&distance<best){best=distance;bestEye=side;}}
 if(bestEye>=0){out["bestEye"]=bestEye;out["bestAxisDistanceMeters"]=best;out["aligned"]=angle<=2&&best<=.015;}
 if(input.contains("controllerPosition")){
  // One measured translation toward the right eye at 20 cm relief. Fixed head;
  // legacy controller offsets use right/up/back axes. Never relax alignment.
  const V up=Unit({right[1]*forward[2]-right[2]*forward[1],right[2]*forward[0]-right[0]*forward[2],right[0]*forward[1]-right[1]*forward[0]});
  const V rightEye=Add(eye,Scale(right,half-main));V delta=Add(Add(rightEye,Scale(axis,.20)),Scale(scope,-1));
  const double distance=Length(delta);if(distance>.12)delta=Scale(delta,.12/distance);
  const V local{Dot(delta,right),Dot(delta,up),-Dot(delta,forward)};
  out["nextControllerPosition"]=Add(Vector(input.at("controllerPosition")),local);
  out["controllerCorrectionMeters"]=Length(delta);
 }
 if(input.contains("staticHit")){
  const auto& hit=input.at("staticHit");const bool present=hit.is_array();double distance=present?Length(Add(Vector(hit),Scale(eye,-1))):0;
  out["staticTargetDistanceMeters"]=distance;out["distantStaticTarget"]=present&&distance>=20&&distance<=151;}
 out["scope"]="Measured camera and weapon transforms; alignment and static depth do not establish reticle visibility.";
 return out;
}
}
J Fixtures(const J& input) {
    const auto position=input.at("position").get<V>();auto forward=input.at("forward").get<V>();
    const double norm=std::hypot(forward[0],forward[1]);
    if(!std::isfinite(norm)||norm<.01)throw std::runtime_error("invalid player forward vector");
    forward={forward[0]/norm,forward[1]/norm,0};const V right{forward[1],-forward[0],0};
    J out;out["markers"]=J::array();
    for(const auto local:{V{0,8,1.2},V{-3,20,1.2},V{3,20,1.2}}) {
        V point=position;for(int i=0;i<3;++i)point[i]+=right[i]*local[0]+forward[i]*local[1];point[2]+=local[2];
        out["markers"].push_back(point);
    }
    const auto base=input.at("head");out["headPoses"]=J::array();
    const char* labels[]={"neutral","yaw-plus15","yaw-minus15","roll-plus10","roll-minus10"};
    const double yaw[]={0,15,-15,0,0},roll[]={0,0,0,10,-10};
    for(int i=0;i<5;++i) {auto head=base;head["yaw"]=base.at("yaw").get<double>()-yaw[i]*Pi/180;
        head["roll"]=base.at("roll").get<double>()+roll[i]*Pi/180;
        out["headPoses"].push_back({{"label",labels[i]},{"head",head}});}
    V vehicle=position;for(int i=0;i<3;++i)vehicle[i]+=forward[i]*4;vehicle[2]+=.25;out["vehiclePosition"]=vehicle;
    return out;
}
double Percentile(std::vector<double> values,double fraction) {
    if(values.empty())return 0;std::sort(values.begin(),values.end());
    return values[std::min(values.size()-1,size_t(std::floor(fraction*double(values.size()-1))))];
}
J Distribution(std::vector<double> values) {
    J out={{"count",values.size()}};
    if(values.empty())return out;
    std::sort(values.begin(),values.end());
    auto at=[&](double fraction){return values[std::min(values.size()-1,size_t(std::floor(fraction*double(values.size()-1))))];};
    out["min"]=values.front();out["p50"]=at(.5);out["p95"]=at(.95);out["p99"]=at(.99);out["max"]=values.back();return out;
}
void Increment(J& histogram,const std::string& key){histogram[key]=histogram.value(key,uint64_t(0))+1;}
void PeriodBucket(J& histogram,double spacingMs,double periodMs) {
    if(!(spacingMs>0)){Increment(histogram,"nonIncreasing");return;}
    if(!(periodMs>0)){Increment(histogram,"unknownPeriod");return;}
    const double ratio=spacingMs/periodMs;
    if(!std::isfinite(ratio)||ratio>1000000){Increment(histogram,"outOfRange");return;}
    const auto nearest=std::llround(ratio);
    Increment(histogram,std::abs(ratio-double(nearest))<=.05?std::to_string(nearest):"fractional");
}
struct Correlation {
    size_t count{};double x{},y{},xx{},yy{},xy{};
    void Add(double a,double b){++count;x+=a;y+=b;xx+=a*a;yy+=b*b;xy+=a*b;}
    J Result() const {J out={{"count",count},{"r",nullptr}};if(count>1){const double vx=count*xx-x*x,vy=count*yy-y*y;
        if(vx>1e-12 && vy>1e-12)out["r"]=std::clamp((count*xy-x*y)/std::sqrt(vx*vy),-1.,1.);}return out;}
};
J Trace(const J& input) {
    if(input.value("counterReset",false)||input.value("counterChangedBackwards",false))
        throw std::runtime_error("trace counter reset during capture");
    uint64_t accepted=0,generated=0,real=0,repeats=0,lastReal=0;double first=0,last=0;
    std::vector<double> ages,intervals,predictedIntervals,periods,waitTimes,endTimes,selectTimes;
    struct Timing {std::vector<double> postEnd,predicted;J steps=J::object();double lastPost{},lastPredicted{};bool seen{};};
    std::array<Timing,3> timing;const char* categoryNames[]={"real","generated","repeat"};
    J out,steps=J::object(),transitions=J::object(),serialAdvances=J::object();std::string prefix;
    J endBins=J::object();Correlation endVsPost,selectVsPost,predictedVsPost,previousEndVsPrediction,previousPostVsPrediction;
    double previousEnd=0,previousPost=0;bool previousEndMeasured=false,previousPostMeasured=false;
    double previous=0,lastPredicted=0;uint64_t nonIncreasingPredictions=0,invalidStageDurations=0;
    uint64_t shortPostIntervals=0,longPostIntervals=0,longestRepeatRun=0,repeatRun=0,realSerialRegressions=0,unmeasuredEndSelectResubmits=0;int lastCategory=-1;
    for(const auto& row:input.at("rows")) {
        if(row.size()!=12)throw std::runtime_error("invalid trace row shape");
        for(const auto& value:row)if(!std::isfinite(value.get<double>()))throw std::runtime_error("non-finite trace row");
        const double stamp=row[0];
        if(stamp<=0 || (previous && stamp<=previous))throw std::runtime_error("non-monotonic/torn trace window");
        previous=stamp;
        if(row[6].get<double>()<.5)continue;
        const double now=row[0],capture=row[5],predicted=row[1],periodMs=row[7].get<double>()/1000000.;const int kind=int(row[4].get<double>());
        const auto serial=uint64_t(row[2].get<double>());const double synthetic=row[3];
        if(accepted){
            const double post=now-last,prediction=(predicted-lastPredicted)/1000000.;
            intervals.push_back(post);predictedIntervals.push_back(prediction);PeriodBucket(steps,prediction,periodMs);
            if(!(prediction>0))++nonIncreasingPredictions;
            if(periodMs>0){if(post<periodMs*.5)++shortPostIntervals;if(post>periodMs*1.5)++longPostIntervals;}
            predictedVsPost.Add(prediction,post);
            if(previousEndMeasured)previousEndVsPrediction.Add(previousEnd,prediction);
            if(previousPostMeasured)previousPostVsPrediction.Add(previousPost,prediction);
            previousPost=post;previousPostMeasured=true;
            if(kind!=-2){if(row[10].get<double>()>=0)endVsPost.Add(row[10],post);if(row[11].get<double>()>=0)selectVsPost.Add(row[11],post);}
        }else first=now;
        last=now;lastPredicted=predicted;++accepted;if(periodMs>0)periods.push_back(periodMs);
        if(kind==-2)++unmeasuredEndSelectResubmits;
        previousEndMeasured=kind!=-2 && row[10].get<double>()>=0;previousEnd=row[10];
        for(int stage=9;stage<=11;++stage){
            // ResubmitLastFrame rows encode literal0 for end/select (no timing
            // measurement on that path), so do not treat them as zero-cost calls.
            if(kind==-2 && stage>=10)continue;
            const double value=row[stage];
            if(value<0){++invalidStageDurations;continue;}
            (stage==9?waitTimes:stage==10?endTimes:selectTimes).push_back(value);
            if(stage==10){const double bounds[]={.25,.5,1,2,4,6,8,12,20,40};const char* labels[]={"0-.25",".25-.5",".5-1","1-2","2-4","4-6","6-8","8-12","12-20","20-40","40+"};
                size_t bucket=0;while(bucket<10 && value>=bounds[bucket])++bucket;Increment(endBins,labels[bucket]);}}
        int category=2;
        if(kind==-2 || kind==0)++repeats;
        else if(synthetic>.5){++generated;category=1;}
        else if(serial && serial!=lastReal){
            ++real;category=0;
            if(lastReal){if(serial<lastReal)++realSerialRegressions;else Increment(serialAdvances,std::to_string(serial-lastReal));}
            lastReal=serial;
        }else ++repeats;
        auto& current=timing[category];
        if(current.seen){const double gap=(predicted-current.lastPredicted)/1000000.;current.postEnd.push_back(now-current.lastPost);
            current.predicted.push_back(gap);PeriodBucket(current.steps,gap,periodMs);}
        current.lastPost=now;current.lastPredicted=predicted;current.seen=true;
        if(lastCategory>=0)Increment(transitions,std::string(categoryNames[lastCategory])+"->"+categoryNames[category]);
        lastCategory=category;if(prefix.size()<96)prefix+=category==0?'R':category==1?'G':'P';
        if(category==2){++repeatRun;longestRepeatRun=std::max(longestRepeatRun,repeatRun);}else repeatRun=0;
        if(capture>0 && now>=capture)ages.push_back(now-capture);
    }
    out={{"acceptedCycles",accepted},{"acceptedReal",real},{"acceptedGenerated",generated},{"acceptedRepeated",repeats},
         {"acceptedCycleRate",accepted>1 && last>first?double(accepted-1)*1000/(last-first):0},
         {"sourceAgeMsP50",Percentile(ages,.5)},{"sourceAgeMsP95",Percentile(ages,.95)},
         {"cycleIntervalMsP95",Percentile(intervals,.95)},
         {"inputRowsDropped",input.value("dropped",uint64_t(0))},
         {"completeCapture",input.value("dropped",uint64_t(0))==0},
         {"scope","accepted application submissions; not compositor scanout or unique displayed headset FPS"}};
    out["postEndIntervalMs"]=Distribution(intervals);out["predictedSpacingMs"]=Distribution(predictedIntervals);out["reportedPeriodMs"]=Distribution(periods);
    out["predictedPeriodSteps"]=steps;out["waitMs"]=Distribution(waitTimes);out["endMs"]=Distribution(endTimes);out["fgSelectMs"]=Distribution(selectTimes);
    out["waitMs"]["availability"]=input.contains("waitTimingEnabled")
        ? (input["waitTimingEnabled"].get<bool>()?"enabled-at-capture-endpoints":"disabled-at-capture-endpoints")
        : (waitTimes.empty()||Percentile(waitTimes,1)==0?"unknown-all-zero; XrRateLog gate was not captured":"nonzero-timing-observed; gate not captured");
    out["unmeasuredEndSelectResubmits"]=unmeasuredEndSelectResubmits;
    out["endMsBins"]=endBins;out["durationCorrelations"]={{"endCallVsPostEndInterval",endVsPost.Result()},
        {"fgSelectVsPostEndInterval",selectVsPost.Result()},{"predictedSpacingVsPostEndInterval",predictedVsPost.Result()},
        {"previousEndCallVsNextPredictedSpacing",previousEndVsPrediction.Result()},{"previousPostEndIntervalVsNextPredictedSpacing",previousPostVsPrediction.Result()}};
    out["nonIncreasingPredictions"]=nonIncreasingPredictions;out["invalidStageDurations"]=invalidStageDurations;
    out["postEndShorterThanHalfPeriod"]=shortPostIntervals;out["postEndLongerThanOneAndHalfPeriods"]=longPostIntervals;
    out["transitions"]=transitions;out["patternPrefix"]=prefix;out["longestRepeatRun"]=longestRepeatRun;
    out["realSourceSerialAdvances"]=serialAdvances;out["realSourceSerialRegressions"]=realSerialRegressions;
    out["categoryIntervals"]=J::object();for(int i=0;i<3;++i)out["categoryIntervals"][categoryNames[i]]={{"postEndMs",Distribution(timing[i].postEnd)},
        {"predictedMs",Distribution(timing[i].predicted)},{"predictedPeriodSteps",timing[i].steps}};
    out["clockDomains"]="postEnd/wait/end/select/sourceAge use QPC durations; predicted spacing uses only XrTime differences; no absolute XrTime-QPC subtraction or display-deadline claim";
    return out;
}
J Geometry(const J& input) {
    J out;out["renderEyes"]=J::array();
    const auto& fields=input.at("fields");const auto& values=fields.at("CyberpunkVR_DebugEngineStereoEyes");
    const auto jitter=fields.value("CyberpunkVR_DebugEngineStereoEyeJitter",J::array());
    for(int eye=0;eye<2;++eye) {
        const size_t at=eye*7;const double p00=values.at(at+3),p11=values.at(at+4);
        double p20=values.at(at+5),p21=values.at(at+6);bool unjittered=false;
        if(jitter.size()==8 && jitter[eye*4+2].get<double>()>1 && jitter[eye*4+3].get<double>()>1) {
            p20-=2*jitter[eye*4].get<double>()/jitter[eye*4+2].get<double>();
            p21-=2*jitter[eye*4+1].get<double>()/jitter[eye*4+3].get<double>();unjittered=true;
        }
        if(!(p00>0 && p11>0) || !std::isfinite(p00+p11+p20+p21))throw std::runtime_error("missing/invalid built eye projection");
        const std::array<double,4> tangents{(-1-p20)/p00,(1-p20)/p00,(1-p21)/p11,(-1-p21)/p11};
        std::array<double,4> angles;for(int i=0;i<4;++i)angles[i]=std::atan(tangents[i]);
        J result={{"eye",eye},{"renderFovRadiansLRUD",angles},{"unjittered",unjittered},{"tangentAspect",(tangents[1]-tangents[0])/(tangents[2]-tangents[3])}};
        if(input.contains("xrEvents"))for(const auto& event:input["xrEvents"])if(event.value("ev","")=="endFrame")
            for(const auto& layer:event.value("layers",J::array()))if(layer.value("type","")=="projection")
                for(const auto& view:layer.at("views"))if(view.value("eye",-1)==eye) {
                    const auto& f=view.at("fov");std::array<double,4> submitted{f.at("radL"),f.at("radR"),f.at("radU"),f.at("radD")};
                    double error=0;for(int i=0;i<4;++i)error=std::max(error,std::abs(submitted[i]-angles[i]));
                    result["submittedFovRadiansLRUD"]=submitted;result["maxRenderSubmitAngleErrorRad"]=error;
                    result["submittedRect"]=view.at("rect");
                }
        result["hasSubmissionEvidence"]=result.contains("submittedFovRadiansLRUD");out["renderEyes"].push_back(result);
    }
    out["scope"]="built render frusta; submission comparison requires an in-process xr_probe trace; fixed-pose samples are not frame-paired";
    return out;
}
void Test() {
    J input={{"position",{1,2,3}},{"forward",{0,1,0}},{"head",{{"x",0},{"y",1.7},{"z",0},{"yaw",0},{"pitch",0},{"roll",0}}}};
    const auto f=Fixtures(input);if(f["markers"][0]!=J::array({1.0,10.0,4.2}) || f["headPoses"].size()!=5)throw std::runtime_error("fixed world fixtures incorrect");
    J trace={{"rows",{{100,1,10,0,1,90,1,11111111,1,0,0,0},{111,2,11,1,2,95,1,11111111,2,0,0,0},
        {122,3,10,-1,-2,0,1,11111111,0,0,0,0},{133,4,11,0,3,110,1,11111111,2,0,0,0}}}};
    auto t=Trace(trace);if(t["acceptedReal"]!=2 || t["acceptedGenerated"]!=1 || t["acceptedRepeated"]!=1)throw std::runtime_error("midpoint/repeat/endpoint counted incorrectly");
    trace["rows"][1][0]=90;bool rejected=false;try{Trace(trace);}catch(const std::runtime_error&){rejected=true;}
    if(!rejected)throw std::runtime_error("torn trace timestamps accepted");
    J clocks={{"rows",{{100,1000000000,1,0,1,90,1,10000000,1,8,.1,1},
        {119,1010000000,2,1,2,95,1,10000000,2,7,.2,2},
        {121,1020000000,2,0,3,110,1,10000000,2,8,.3,3},
        {140,1030000000,2,-1,-2,0,1,10000000,0,9,0,0}}}};
    const auto cadence=Trace(clocks);
    if(cadence["postEndIntervalMs"]["p95"]!=19 || cadence["predictedSpacingMs"]["p95"]!=10 || cadence["predictedPeriodSteps"]["1"]!=3 ||
       cadence["categoryIntervals"]["real"]["predictedPeriodSteps"]["2"]!=1 || cadence["patternPrefix"]!="RGRP" ||
       cadence["waitMs"]["max"]!=9 || cadence["endMs"]["max"]!=.3 || cadence["fgSelectMs"]["max"]!=3 ||
       cadence["endMs"]["count"]!=3 || cadence["unmeasuredEndSelectResubmits"]!=1)
        throw std::runtime_error("completion bursts confused with predicted spacing or category cadence");
    for(auto& row:clocks["rows"])row[1]=row[1].get<double>()+999999999999.;
    const auto shifted=Trace(clocks);
    if(shifted["predictedSpacingMs"]!=cadence["predictedSpacingMs"] || shifted["postEndIntervalMs"]!=cadence["postEndIntervalMs"])
        throw std::runtime_error("absolute XrTime offset contaminated clock-domain analysis");
    clocks["rows"][2][1]=clocks["rows"][1][1];
    if(Trace(clocks)["nonIncreasingPredictions"]!=1)throw std::runtime_error("non-increasing XR predictions hidden");
    J geometry={{"fields",{{"CyberpunkVR_DebugEngineStereoEyes",{1,0,0,1,1,0,0,1,0,0,1,1,0,0}}}}};
    auto g=Geometry(geometry);if(g["renderEyes"][0]["hasSubmissionEvidence"]!=false || std::abs(g["renderEyes"][0]["renderFovRadiansLRUD"][0].get<double>()+Pi/4)>1e-9)
        throw std::runtime_error("frusta or missing submission evidence mislabeled");
    J optic={{"camera",{{"p",{0,0,0}},{"forward",{0,1,0}},{"right",{1,0,0}},{"halfIpd",.032},{"mainEyeOffset",0}}},
             {"weapon",{{"p",{-.032,.15,0}},{"q",{0,0,0,1}}}},{"scopeOffset",{0,0,0}},{"staticHit",{0,25,0}}};
    auto aligned=optics::Measure(optic);if(!aligned["aligned"].get<bool>()||aligned["bestEye"]!=0||!aligned["distantStaticTarget"].get<bool>())throw std::runtime_error("aligned left-eye optic rejected");
    optic["camera"]["p"]={.032,0,0};optic["camera"]["mainEyeOffset"]=.032;
    if(!optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("MAIN eye offset invented a phantom eye");
    optic["weapon"]["p"]={-.032,-.15,0};if(optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("eye in front of lens accepted");
    optic["weapon"]["p"]={-.032,.5,0};if(optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("excessive relief accepted");
    optic["weapon"]["p"]={-.032,.15,0};optic["weapon"]["q"]={0,0,std::sin(Pi/4),std::cos(Pi/4)};
    if(optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("sideways scope accepted");
    optic["staticHit"]={.032,2,0};if(optics::Measure(optic)["distantStaticTarget"].get<bool>())throw std::runtime_error("near wall accepted as distant target");
    optic["weapon"]["q"]={0,0,0,1};optic["binding"]={{"slot","vrp_scope_hinge"},{"point",{0,0,0}},{"forward",{0,1,0}}};
    optic["scopeBone"]={{"slot","vrp_scope_hinge"},{"p",{.20,.15,0}},{"q",{0,0,0,1}}};
    if(optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("bind-space point accepted in place of animated aperture");
    optic["scopeBone"]["p"]={-.032,.15,0};if(!optics::Measure(optic)["aligned"].get<bool>())throw std::runtime_error("animated aperture alignment rejected");
}
int main(int argc,char** argv) try {
    if(argc==2 && std::string(argv[1])=="self-test"){Test();std::cout<<"PASS fixed fixtures, cadence semantics and frusta evidence\n";return 0;}
    if(argc!=3)throw std::runtime_error("fixtures|trace|geometry|optics|optics-bind input.json");
    std::ifstream file(argv[2]);J in;file>>in;const std::string command=argv[1];
    J out=command=="fixtures"?Fixtures(in):command=="trace"?Trace(in):command=="geometry"?Geometry(in):command=="optics"?optics::Measure(in):command=="optics-bind"?optics::Bind(in):throw std::runtime_error("unknown command");
    std::cout<<out.dump(2)<<'\n';return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<'\n';return 1;}
