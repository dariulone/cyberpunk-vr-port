#include <nlohmann/json.hpp>
#include <algorithm>
#include <array>
#include <cmath>
#include <fstream>
#include <iostream>
#include <map>
#include <regex>
#include <stdexcept>
#include <string>
#include <vector>
using J=nlohmann::json;
using V=std::array<double,3>;using Q=std::array<double,4>;
constexpr double pi=3.14159265358979323846;
V add(V a,V b){for(int k=0;k<3;++k)a[k]+=b[k];return a;}
V sub(V a,V b){for(int k=0;k<3;++k)a[k]-=b[k];return a;}
V mul(V a,double s){for(auto&x:a)x*=s;return a;}
double dot(V a,V b){return a[0]*b[0]+a[1]*b[1]+a[2]*b[2];}
V cross(V a,V b){return {a[1]*b[2]-a[2]*b[1],a[2]*b[0]-a[0]*b[2],a[0]*b[1]-a[1]*b[0]};}
double norm(V a){return std::sqrt(dot(a,a));}
V unit(V a){double n=norm(a);if(!std::isfinite(n)||n<1e-8)throw std::runtime_error("degenerate direction");return mul(a,1/n);}
V v3(const J& a){return {a.at(0).get<double>(),a.at(1).get<double>(),a.at(2).get<double>()};}
Q normalize(Q q){double n=0;for(double x:q)n+=x*x;if(!std::isfinite(n)||n<1e-12)throw std::runtime_error("invalid quaternion");for(auto&x:q)x/=std::sqrt(n);return q;}
Q q4(const J&a){return normalize(a.get<Q>());}
Q qm(Q a,Q b){return {a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1],a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0],a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3],a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2]};}
Q inv(Q q){return {-q[0],-q[1],-q[2],q[3]};}
Q axis(V v,double angle){v=unit(v);double s=std::sin(angle*.5);return {v[0]*s,v[1]*s,v[2]*s,std::cos(angle*.5)};}
V rotate(Q q,V v){auto t=mul(cross({q[0],q[1],q[2]},v),2);return add(v,add(mul(t,q[3]),cross({q[0],q[1],q[2]},t)));}
Q expq(V a){double n=norm(a);return n<1e-9?Q{0,0,0,1}:axis(a,n);}
V logq(Q q){q=normalize(q);if(q[3]<0)for(auto&x:q)x=-x;V v{q[0],q[1],q[2]};double n=norm(v);return n<1e-9?V{}:mul(v,2*std::atan2(n,q[3])/n);}
Q ypr(double y,double p,double r){return normalize(qm(qm(axis({0,1,0},y*pi/180),axis({1,0,0},p*pi/180)),axis({0,0,-1},r*pi/180)));}
Q slerp(Q a,Q b,double t){double d=0;for(int k=0;k<4;++k)d+=a[k]*b[k];if(d<0){d=-d;for(auto&x:b)x=-x;}double wa=1-t,wb=t;if(d<.9995){double th=std::acos(std::clamp(d,-1.,1.)),s=std::sin(th);wa=std::sin((1-t)*th)/s;wb=std::sin(t*th)/s;}Q q;for(int k=0;k<4;++k)q[k]=a[k]*wa+b[k]*wb;return normalize(q);}
Q basis(V normal,V along){
 normal=unit(normal);along=unit(sub(along,mul(normal,dot(normal,along))));V across=unit(cross(along,normal));
 double m[3][3];for(int k=0;k<3;++k){m[k][0]=across[k];m[k][1]=along[k];m[k][2]=normal[k];}
 Q q{};double tr=m[0][0]+m[1][1]+m[2][2];
 if(tr>0){double s=std::sqrt(tr+1)*2;q={(m[2][1]-m[1][2])/s,(m[0][2]-m[2][0])/s,(m[1][0]-m[0][1])/s,.25*s};}
 else {int i=0;for(int k=1;k<3;++k)if(m[k][k]>m[i][i])i=k;int j=(i+1)%3,k=(i+2)%3;double s=std::sqrt(1+m[i][i]-m[j][j]-m[k][k])*2;q[i]=.25*s;q[j]=(m[j][i]+m[i][j])/s;q[k]=(m[k][i]+m[i][k])/s;q[3]=(m[k][j]-m[j][k])/s;}
 return normalize(q);
}
V solve(double a[3][4]){
 for(int k=0;k<3;++k){int p=k;for(int r=k+1;r<3;++r)if(std::abs(a[r][k])>std::abs(a[p][k]))p=r;
  if(std::abs(a[p][k])<1e-5)throw std::runtime_error("blocked/singular hand calibration");for(int j=k;j<4;++j)std::swap(a[k][j],a[p][j]);
  double d=a[k][k];for(int j=k;j<4;++j)a[k][j]/=d;for(int r=0;r<3;++r)if(r!=k){d=a[r][k];for(int j=k;j<4;++j)a[r][j]-=d*a[k][j];}}
 return {a[0][3],a[1][3],a[2][3]};
}
J pose(V p,Q q){return J{{"p",p},{"q",q}};}
J head(double height,double pitch=-45){return J{{"x",0},{"y",height},{"z",0},{"yaw",0},{"pitch",pitch*pi/180},{"roll",0}};}
J initial(const J&in){V p=v3(in.at("player").at("position")),f=v3(in.at("player").at("forward"));f[2]=0;f=unit(f);
 return {{"spawn",add(p,mul(f,in.value("spawnDistance",1.1)))},{"head",head(1.7,-35)},
 {"hands",J::array({pose({-.25,-.3,-.25},ypr(-90,0,-90)),pose({.25,-.3,-.25},ypr(-90,0,-90))})}};
}
J limb(const J&in){const auto&s=in.at("sample");const auto&slots=s.at("slots");std::string part=in.value("part","shin"),side=in.value("limbSide","right");
 bool left=side=="left",shin=part=="shin",trunk=part=="trunk";std::string a=trunk?"Hips":shin?(left?"LegLeft":"LegRight"):(left?"LeftForeArm":"RightForeArm");
 std::string b=trunk?"Chest":shin?(left?"LeftFoot":"RightFoot"):(left?"LeftHand":"RightHand");
 V pa=v3(slots.at(a)),pb=v3(slots.at(b)),mid=add(pa,mul(sub(pb,pa),in.value("fraction",shin?.72:.55)));
 V hip=v3(slots.at("Hips")),out=sub(mid,hip);out[2]=0;out=unit(out);V segment=sub(pb,pa);segment[2]=0;segment=unit(segment);
 V along={-segment[1],segment[0],0};if(dot(along,mul(out,-1))<0)along=mul(along,-1);
 V stand=add(mid,mul(out,in.value("standOff",.32)));
 if(in.value("pairCentre",false)){V pair=mul(add(v3(slots.at("LeftForeArm")),v3(slots.at("RightForeArm"))),.5);out=sub(pair,hip);out[2]=0;out=unit(out);stand=add(pair,mul(out,.30));}
 stand[2]=v3(s.at("player").at("position"))[2];
 return {{"mid",mid},{"goal",add(mid,{0,0,trunk?.12:shin?.04:.03})},{"safe",add(mid,{0,0,trunk?.30:.20})},{"stand",stand},
 {"forward",mul(out,-1)},{"yaw",std::atan2(out[0],-out[1])*180/pi},{"normal",V{0,0,-1}},{"along",along},{"segment",trunk?1:left?(shin?12:8):(shin?13:9)}};
}
J secondGrip(const J&in){const auto&hand=in.at("sample").at("hands").at(1);const auto&f=hand.at("frame");const auto&g=hand.at("grasp").at("fast");
 V point=v3(hand.at("nativePose").at("p")),along=rotate(q4(f[1]),v3(g["axis"]));
 V across=cross(along,{0,0,1});if(norm(across)<.1)across={1,0,0};across=unit(across);V mid=add(point,mul(across,in.value("offset",.10)));
 V query=add(mid,{0,0,.25});
 if(in.contains("surface")){V surface=v3(in["surface"]),out=sub(v3(in.at("query")),surface);out=norm(out)>.001?unit(out):V{0,0,1};
  return {{"goal",add(surface,mul(out,.01))},{"safe",add(surface,mul(out,.16))},{"normal",mul(out,-1)},{"along",v3(f[6])},{"query",query}};}
 return {{"goal",add(mid,{0,0,.07})},{"safe",add(mid,{0,0,.23})},{"normal",V{0,0,-1}},{"along",v3(f[6])},{"query",query}};
}
J massProbes(const J&in){V palm=v3(in.at("palm"));J points=J::array(),rays=J::array();
 if(in.contains("mid"))points.push_back(in["mid"]);
 for(const char* name:{"LegRight","RightFoot","LegLeft","LeftFoot","RightForeArm","LeftForeArm","Hips","Chest"})
  if(in.at("slots").contains(name))points.push_back(in["slots"][name]);
 for(const auto&point:points){V p=v3(point);rays.push_back(J::array({palm,p}));rays.push_back(J::array({add(p,{0,0,.25}),add(p,{0,0,-.25})}));}
 points.push_back(palm);return {{"points",points},{"rays",rays}};
}
J posture(const J&in){const std::string text=in.at("diagnostic");
 auto vec=[&](const std::string&label){std::smatch match;std::regex expression(label+R"(\s*=\s*\(([-+0-9.eE]+),\s*([-+0-9.eE]+),\s*([-+0-9.eE]+)\))");
  if(!std::regex_search(text,match,expression))throw std::runtime_error("missing posture diagnostic "+label);
  return V{std::stod(match[1]),std::stod(match[2]),std::stod(match[3])};};
 V right=vec(R"(IK shoulder\(model\))"),left=vec(R"(LK shoulder\(model\))"),camera=vec("camPos"),foot=vec("entityPos");
 std::smatch lengths;if(!std::regex_search(text,lengths,std::regex(R"(avatar arm FK R/L\s*=\s*([-+0-9.eE]+)/([-+0-9.eE]+))")))throw std::runtime_error("missing measured FK arm lengths");
 return {{"shoulders",J::array({left,right})},{"lengths",J::array({std::stod(lengths[2]),std::stod(lengths[1])})},{"cameraWorld",camera},{"footWorld",foot},
  {"actualEyeHeight",camera[2]-foot[2]},{"shoulderHeight",J::array({left[2],right[2]})},{"lengthMeaning","current FK segment sum; stance ranking only, no production cap override"}};
}
J standCandidates(const J&in){const auto&plan=in.at("plan");V mid=v3(plan["mid"]),out=mul(v3(plan["forward"]),-1);double floor=in.at("floor");J candidates=J::array();
 const int side=in.value("side",1);const auto&measure=in.at("measurement");V shoulder=v3(measure.at("shoulders").at(side));double available=measure.at("lengths").at(side);
 if(available<.1||available>1.5)throw std::runtime_error("invalid measured avatar arm length");
 V target=v3(plan.at("goal"));
 for(double degrees:{0.,45.,-45.,90.,-90.,180.})for(double front:{-.18,.02,.20,.34})for(double lateral:{0.,shoulder[0]}){
  V away=rotate(axis({0,0,1},degrees*pi/180),out),forward=mul(away,-1),right={forward[1],-forward[0],0};
  V stand=sub(mid,add(mul(right,lateral),mul(forward,front)));stand[2]=floor;J rays=J::array();
  for(V offset:{V{},V{.24,0,0},V{-.24,0,0},V{0,.24,0},V{0,-.24,0}}){V at=add(stand,offset);rays.push_back({{"a",add(at,{0,0,1})},{"b",add(at,{0,0,-1})}});}
  V sh=add(stand,add(mul(right,shoulder[0]),mul(forward,shoulder[1])));sh[2]+=shoulder[2];double distance=norm(sub(target,sh));
  candidates.push_back({{"stand",stand},{"forward",forward},{"yaw",std::atan2(away[0],-away[1])*180/pi},{"rays",rays},
   {"predictedShoulder",sh},{"predictedShoulderToPalm",distance},{"measuredArmLength",available},{"predictedReachExcess",std::max(0.,distance-available)},
   {"frontOffset",front},{"lateralOffset",lateral},{"reachScore",distance+std::max(0.,distance-available)*4}});
 }
 return {{"candidates",candidates},{"floor",floor},{"measurement",measure}};
}
J chooseStand(const J&in){const auto&candidates=in.at("candidates");const auto&hits=in.at("hits");double floor=in.at("floor"),best=1e9;int chosen=-1;
 for(size_t i=0;i<candidates.size();++i){double error=0;bool valid=hits.at(i).size()==5;for(const auto&hit:hits.at(i)){if(hit.is_null()||hit==false){valid=false;break;}error=std::max(error,std::abs(v3(hit)[2]-floor));}
  double score=candidates[i].value("reachScore",0.)+error;
  if(valid&&error<.12&&score<best){best=score;chosen=int(i);}}
 if(chosen<0)throw std::runtime_error("no same-floor standing footprint around limb; relocate test fixture");
 J out=candidates[chosen];out["selectionScore"]=best;out["candidateIndex"]=chosen;out["measurement"]=in.value("measurement",J::object());return out;
}
J probes(const J&in,bool orientation){J poses=J::array();V p=v3(in.at("pose").at("p"));Q q=q4(in.at("pose").at("q"));double h=orientation?10*pi/180:in.value("step",.03);
 poses.push_back(pose(p,q));for(int k=0;k<3;++k){if(orientation){V a{};a[k]=h;poses.push_back(pose(p,normalize(qm(expq(a),q))));}else{V at=p;at[k]+=h;poses.push_back(pose(at,q));}}
 return {{"poses",poses},{"step",h}};
}
V rawPalm(const J&frame){return add(v3(frame.at(2)),rotate(q4(frame.at(3)),v3(frame.at(4))));}
Q rawPalmBasis(const J&frame){
 const Q undoDraw=qm(q4(frame.at(3)),inv(q4(frame.at(1))));
 return basis(rotate(undoDraw,v3(frame.at(5))),rotate(undoDraw,v3(frame.at(6))));
}
J stability(const J&in){const auto&a=in.at("before"),&b=in.at("after");double rawTravel=norm(sub(rawPalm(a),rawPalm(b)));
 double rawTurn=norm(logq(qm(q4(a[3]),inv(q4(b[3])))))*180/pi;
 double drawnTurn=norm(logq(qm(q4(a[1]),inv(q4(b[1])))))*180/pi;
 double rawPalmTurn=norm(logq(qm(rawPalmBasis(a),inv(rawPalmBasis(b)))))*180/pi;
 return {{"stable",rawTravel<.002&&rawTurn<.3&&(!in.value("orientation",false)||rawPalmTurn<.75)},
 {"rawPalmTravel",rawTravel},{"rawTurnDegrees",rawTurn},{"rawPalmTurnDegrees",rawPalmTurn},{"drawnTurnDegrees",drawnTurn}};
}
J correction(const J&in,bool orientation){const auto&frames=in.at("frames");V error{};Q base{},target{};double h=in.at("step");double a[3][4]{};
 if(orientation){base=rawPalmBasis(frames.at(0));target=basis(v3(in.at("normal")),v3(in.at("along")));error=logq(qm(target,inv(base)));
  for(int c=0;c<3;++c){auto q=rawPalmBasis(frames.at(c+1));auto v=mul(logq(qm(q,inv(base))),1/h);for(int r=0;r<3;++r)a[r][c]=v[r];}}
 else{int row=in.value("row",8);bool raw=in.value("rawPalm",false);V b=raw?rawPalm(frames.at(0)):v3(frames.at(0).at(row));error=sub(v3(in.at("target")),b);for(int c=0;c<3;++c){V at=raw?rawPalm(frames.at(c+1)):v3(frames.at(c+1).at(row));V v=mul(sub(at,b),1/h);for(int r=0;r<3;++r)a[r][c]=v[r];}}
 for(int r=0;r<3;++r)a[r][3]=error[r];double e=norm(error),threshold=orientation?6*pi/180:.012;
 J result={{"error",e},{"converged",e<threshold},{"pose",in.at("pose")}};if(e<threshold)return result;
 V delta=solve(a);double n=norm(delta),max=orientation?35*pi/180:.15;if(n>max)delta=mul(delta,max/n);
 V p=v3(in.at("pose").at("p"));Q q=q4(in.at("pose").at("q"));if(orientation)q=normalize(qm(expq(delta),q));else p=add(p,delta);
 result["pose"]=pose(p,q);result["stepLength"]=norm(delta);return result;
}
J face(const J&in){V want=v3(in.at("wanted")),have=v3(in.at("actual"));double error=std::remainder(std::atan2(want[1],want[0])-std::atan2(have[1],have[0]),2*pi);double rate=in.value("rate",-21*pi/180);
 if(std::abs(rate)<.01)throw std::runtime_error("turn calibration produced no rotation");return {{"errorDegrees",error*180/pi},{"done",std::abs(error)<3*pi/180},{"stick",error*rate>0?.6:-.6},{"seconds",std::clamp(std::abs(error/rate),.06,2.)}};
}
J turnRate(const J&in){V before=v3(in.at("before")),after=v3(in.at("after"));double angle=std::remainder(std::atan2(after[1],after[0])-std::atan2(before[1],before[0]),2*pi);
 double seconds=in.at("seconds"),stick=in.at("stick");double rate=in.value("previous",-21*pi/180);
 if(seconds>.02&&std::abs(stick)>.1&&std::abs(angle)>.015)rate=angle/seconds*(.6/stick);
 return {{"rate",rate},{"observedDegrees",angle*180/pi}};
}
J motion(const J&in){std::string mode=in.at("mode");double seconds=in.value("seconds",1.5);int count=std::max(2,int(std::ceil(seconds*60)));J frames=J::array();auto hands=in.at("hands");
 V delta=in.contains("delta")?v3(in["delta"]):V{};double angle=in.value("angleDegrees",45.)*pi/180;
 V pa=v3(hands[0]["p"]),pb=v3(hands[1]["p"]),mid=mul(add(pa,pb),.5),line=sub(pb,pa),turnAxis={0,0,1};
 if(norm(line)>1e-6 && std::abs(dot(unit(line),turnAxis))>.9)turnAxis={0,1,0};
 for(int i=0;i<=count;++i){double t=double(i)/count,e=t*t*(3-2*t);J f={{"t",seconds*t},{"hands",hands}};
  if(mode=="move"){int side=in.value("side",1);f["hands"][side]["p"]=add(v3(hands[side]["p"]),mul(delta,e));}
  else if(mode=="to"){int side=in.value("side",1);const auto&to=in.at("target");f["hands"][side]=pose(add(v3(hands[side]["p"]),mul(sub(v3(to["p"]),v3(hands[side]["p"])),e)),slerp(q4(hands[side]["q"]),q4(to["q"]),e));}
  else if(mode=="twist"){int side=in.value("side",1);f["hands"][side]["q"]=normalize(qm(q4(hands[side]["q"]),axis({0,0,-1},angle*e)));}
  else if(mode=="stand")f["head"]=head(in.value("from",.85)+(in.value("to",1.7)-in.value("from",.85))*e);
  else if(mode=="pair-turn"){Q q=axis(turnAxis,angle*e);for(int k=0;k<2;++k)f["hands"][k]["p"]=add(mid,rotate(q,sub(v3(hands[k]["p"]),mid)));}
  else if(mode=="pair-move"){for(int k=0;k<2;++k)f["hands"][k]["p"]=add(v3(hands[k]["p"]),mul(delta,e));}
  else if(mode=="walk")f["sticks"]={{"left",J::array({0.,t<.5?.6:-.6})},{"right",J::array({0.,0.})}};
  else if(mode=="turn")f["sticks"]={{"left",J::array({0.,0.})},{"right",J::array({t<.5?.6:-.6,0.})}};
  else if(mode!="hold")throw std::runtime_error("unknown motion mode");
  frames.push_back(f);
 }
 return {{"seconds",seconds},{"frames",frames}};
}
double quantile(std::vector<double> a,double p){if(a.empty())return 0;std::sort(a.begin(),a.end());return a[std::min(a.size()-1,size_t(p*double(a.size()-1)))];}
V handSeat(const J&hand){const auto&g=hand.at("grasp").at("fast");return add(v3(g.at("position")),rotate(q4(g.at("rotation")),v3(g.at("grip"))));}
V seatWorld(const J&hand,bool raw){const auto&f=hand.at("frame");return add(v3(f.at(raw?2:0)),rotate(q4(f.at(raw?3:1)),handSeat(hand)));}
J pairGeometry(const J&in){const auto&samples=in.at("samples");std::vector<V> estimates;
 for(const auto&r:samples)if(r.value("phase","")=="same-part-setup-lift"){
  const auto&h=r.at("hands").at(1);const auto&p=h.at("nativePose");estimates.push_back(rotate(inv(q4(p.at("q"))),sub(seatWorld(h,false),v3(p.at("p")))));}
 if(estimates.size()<5)throw std::runtime_error("need five first-holder samples for bounded inferred native grip");
 V inferred{};for(size_t i=estimates.size()-5;i<estimates.size();++i)inferred=add(inferred,estimates[i]);inferred=mul(inferred,.2);double spread=0;
 for(size_t i=estimates.size()-5;i<estimates.size();++i)spread=std::max(spread,norm(sub(estimates[i],inferred)));
 J out={{"rightGripApproximation","inferred from last five single-holder desired seats and sampled native poses; bounded by native contact error/asynchronous sampling, not exact published native local anchor"},
  {"inferredRightGrip",inferred},{"inferenceSpread",spread},{"phases",J::array()}};
 for(const char*name:{"same-part-rest","same-part-rotate"}){std::vector<double> rawWrist,drawnWrist,rawSeat,drawnSeat,lower,leftActual,leftPublished,localSpan;J rows=J::array();
  for(const auto&r:samples)if(r.value("phase","")==name){const auto&a=r.at("hands").at(0);const auto&b=r.at("hands").at(1);
   if(a.at("grasp").at("body")!=a.at("native").at("body"))throw std::runtime_error("left local grip is not in actual body coordinates");
   V leftLocal=v3(a.at("grasp").at("fast").at("grip"));double span=norm(sub(inferred,leftLocal));localSpan.push_back(span);
   double dw=norm(sub(v3(a["frame"][0]),v3(b["frame"][0]))),rw=norm(sub(v3(a["frame"][2]),v3(b["frame"][2])));
   double ds=norm(sub(seatWorld(a,false),seatWorld(b,false))),rs=norm(sub(seatWorld(a,true),seatWorld(b,true)));
   double bound=std::abs(ds-span)*.5;const auto&p=a.at("nativePose");double actual=norm(sub(add(v3(p["p"]),rotate(q4(p["q"]),leftLocal)),seatWorld(a,false)));
   rawWrist.push_back(rw);drawnWrist.push_back(dw);rawSeat.push_back(rs);drawnSeat.push_back(ds);lower.push_back(bound);leftActual.push_back(actual);
   double published=a.at("native").at("contactErrorMicrometres").get<double>()/1e6;leftPublished.push_back(published);
   rows.push_back({{"t",r.at("t")},{"rawWristSpan",rw},{"drawnWristSpan",dw},{"rawDesiredGripSpan",rs},{"drawnDesiredGripSpan",ds},{"approxRigidGripSpan",span},{"minimumMeanErrorFromSpan",bound},{"leftReconstructedError",actual},{"leftPublishedError",published}});
  }
  auto stats=[](const std::vector<double>&a){return J{{"min",quantile(a,0)},{"max",quantile(a,1)},{"p95",quantile(a,.95)}};};
  out["phases"].push_back({{"name",name},{"rawWristSpan",stats(rawWrist)},{"drawnWristSpan",stats(drawnWrist)},{"rawDesiredGripSpan",stats(rawSeat)},{"drawnDesiredGripSpan",stats(drawnSeat)},
   {"approxRigidGripSpan",stats(localSpan)},{"minimumMeanErrorFromSpan",stats(lower)},{"leftReconstructedError",stats(leftActual)},{"leftPublishedError",stats(leftPublished)},{"samples",rows}});
 }
 return out;
}
J analyze(const J&in){J out={{"pass",true},{"phases",J::array()}};
 for(const auto&phase:in.at("phases")){std::string name=phase.at("name");std::vector<J> samples;for(const auto&r:in.at("samples"))if(r.value("phase","")==name)samples.push_back(r);
  J issues=J::array();if(samples.size()<3)issues.push_back("insufficient measured samples");std::vector<double> slips,gaps,contacts;std::array<size_t,2> contactCount{},contactExpected{};std::array<std::vector<double>,2> perHandContact;double reachSeconds=0,travel=0,hipsTravel=0;V firstPlayer{},lastPlayer{},firstHips{};Q firstBody{},lastBody{};bool bodySeen=false,hipsSeen=false;J identities=J::object();
  std::vector<int> required=phase.value("hands",std::vector<int>{});std::string relation=phase.value("relation","");double previousTime=0;
  for(const auto&r:samples){double now=r.at("t");double dt=previousTime>0?now-previousTime:0;previousTime=now;V player=v3(r.at("player").at("position"));if(&r==&samples.front())firstPlayer=player;lastPlayer=player;travel=std::max(travel,norm(sub(player,firstPlayer)));bool reached=false;
   if(r.at("slots").contains("Hips")){V hips=v3(r["slots"]["Hips"]);if(!hipsSeen){firstHips=hips;hipsSeen=true;}hipsTravel=std::max(hipsTravel,norm(sub(hips,firstHips)));}
   for(int side:required){const auto&hand=r.at("hands").at(side);const auto&n=hand.at("native");++contactExpected[side];
    if(!n.value("active",false)||n.value("ageMs",999.)>=200||!hand.at("grasp").value("held",false)||!hand.at("grasp").contains("fast")){issues.push_back("hold missing or stale");continue;}
    if(hand.at("grasp").value("kind",0)!=12)issues.push_back("held object is not a ragdoll");
    J identity=J::array({n["proxy"],n["body"]});auto key=std::to_string(side);if(!identities.contains(key))identities[key]=identity;else if(identities[key]!=identity)issues.push_back("actual held body changed during phase");
    if(n.value("contactMetricAvailable",false)&&n.value("contactValid",false)&&n.contains("contactErrorMicrometres")){
     double error=n.at("contactErrorMicrometres").get<double>()/1000000.;if(std::isfinite(error)&&error>=0&&error<=1e6){contacts.push_back(error);perHandContact[side].push_back(error);++contactCount[side];}}
    const auto&f=hand.at("frame");if(f.size()!=9){issues.push_back("missing actual hand frame");continue;}
    double gap=norm(sub(v3(f[0]),v3(f[2])));gaps.push_back(gap);if(gap>.45)reached=true;
    const auto&g=hand.at("grasp").at("fast");if(hand.contains("referencePose")){const auto&body=hand["referencePose"];V grip=add(v3(body["p"]),rotate(q4(body["q"]),v3(g["grip"])));V seat=add(v3(f[0]),rotate(q4(f[1]),v3(g["center"])));slips.push_back(norm(sub(grip,seat)));}
    if(side==required.front()&&hand.contains("nativePose")){Q q=q4(hand["nativePose"]["q"]);if(!bodySeen){firstBody=q;bodySeen=true;}lastBody=q;}
   }
   if(reached)reachSeconds+=dt;
   if(required.size()==2){const auto&a=r["hands"][0]["native"];const auto&b=r["hands"][1]["native"];bool same=a["proxy"]==b["proxy"]&&a["body"]==b["body"];if(a["proxy"]!=b["proxy"])issues.push_back("hands held different ragdolls");if(relation=="same"&&!same)issues.push_back("same-part scenario did not hold one native body");if(relation=="different"&&same)issues.push_back("different-parts scenario held the same native body");}
   if(phase.value("released",false))for(const auto&h:r["hands"])if(h["native"].value("active",false)||h["grasp"].value("held",false))issues.push_back("grip remained held after drop");
   if(!r.value("dead",false))issues.push_back("test NPC is not dead");
  }
  double duration=samples.size()>1?samples.back()["t"].get<double>()-samples.front()["t"].get<double>():0;
  double angle=bodySeen?norm(logq(qm(lastBody,inv(firstBody))))*180/pi:0;
  if(duration<phase.value("minimumSeconds",0.))issues.push_back("measured interval shorter than required");
  if(travel<phase.value("minimumTravel",0.))issues.push_back("player did not actually move far enough");
  if(hipsTravel<phase.value("minimumCorpseTravel",0.))issues.push_back("corpse hips did not actually move far enough");
  if(angle<phase.value("minimumRotation",0.))issues.push_back("held body did not actually rotate far enough");
  if(reachSeconds<phase.value("minimumReachSeconds",0.))issues.push_back("controller was not actually outside reach long enough");
  J contactCoverage=J::object();for(int side:required){double ratio=contactExpected[side]?double(contactCount[side])/contactExpected[side]:0;
   contactCoverage[std::to_string(side)]={{"valid",contactCount[side]},{"expected",contactExpected[side]},{"ratio",ratio},{"error95",quantile(perHandContact[side],.95)},{"errorMax",quantile(perHandContact[side],1)}};
   if(contactCount[side]<3||ratio<.90)issues.push_back("insufficient exact native contact evidence for hand "+std::to_string(side));
   if(quantile(perHandContact[side],.95)>phase.value("maximumSlip95",.20))issues.push_back("large exact native anchor error for hand "+std::to_string(side));}
  std::vector<std::string> unique=issues.get<std::vector<std::string>>();std::sort(unique.begin(),unique.end());unique.erase(std::unique(unique.begin(),unique.end()),unique.end());
  bool pass=unique.empty();if(!pass)out["pass"]=false;
  out["phases"].push_back({{"name",name},{"pass",pass},{"samples",samples.size()},{"seconds",duration},{"playerTravel",travel},{"hipsTravel",hipsTravel},{"bodyRotationDegrees",angle},
   {"reachSeconds",reachSeconds},{"rawDrawnGap95",quantile(gaps,.95)},{"exactContactError95",quantile(contacts,.95)},{"exactContactErrorMax",quantile(contacts,1)},{"contactByHand",contactCoverage},{"referenceGripSlip95",quantile(slips,.95)},{"referenceGripSlipMax",quantile(slips,1)},{"issues",unique}});
 }
 return out;
}
J restoreCheck(const J&in){double position=norm(sub(v3(in["player"]["position"]),v3(in["original"]["position"])));
 double rotation=norm(logq(qm(q4(in["player"]["rotation"]),inv(q4(in["original"]["rotation"]))))) *180/pi;
 return {{"pass",position<.03&&rotation<3&&in.value("equipment",false)&&in.value("inventory",false)&&in.value("npcRemoved",false)&&in.value("noHold",false)&&in.value("money",false)},
 {"positionErrorMetres",position},{"rotationErrorDegrees",rotation},{"state",in}};
}
J massCheck(const J&in){J out={{"pass",true},{"actors",J::array()}};
 for(const auto&body:in.at("masses")){
  J issues=J::array();double base=body.value("baseline",0.),raw=body.value("solverMass",-1.);int holders=body.value("holders",0);
  if(!std::isfinite(base)||base<=0)issues.push_back("missing independent pre-hold physical mass baseline");
  if(raw<0){if(holders>0||!body.value("previousReleaseVerified",false))issues.push_back("actor mass unavailable without an earlier verified release");}
  else if(holders>0){
   if(!(raw>base))issues.push_back("held actor does not retain its physical solver mass override");
   if(!body.contains("reportedCarryMass")||std::abs(body["reportedCarryMass"].get<double>()-base)>.0001)issues.push_back("reported carry mass differs from original physical mass");
  }else if(std::abs(raw-base)>.0001)issues.push_back("physical mass was not restored after the last holder released");
  bool pass=issues.empty();if(!pass)out["pass"]=false;
  out["actors"].push_back({{"proxy",body.at("proxy")},{"body",body.at("body")},{"holders",holders},{"baseline",base},{"solverMass",raw},{"pass",pass},{"issues",issues}});
 }
 return out;
}
void check(bool ok,const char*why){if(!ok)throw std::runtime_error(why);}
void selfTest(){
 Q world=ypr(37,19,-23);V p{-2281.925,1707.919,18.61};J frames=J::array();double h=.03;V controller{.2,-.6,-.3};
 for(int i=0;i<4;++i){V c=controller;if(i)c[i-1]+=h;J f=J::array();for(int r=0;r<9;++r)f.push_back(J::array({0,0,0,1}));f[8]=add(p,rotate(world,c));frames.push_back(f);}
 V desired=add(p,rotate(world,add(controller,{.04,.08,-.03})));
 J c=correction({{"frames",frames},{"step",h},{"target",desired},{"pose",pose(controller,{0,0,0,1})}},false);
 check(norm(sub(v3(c["pose"]["p"]),add(controller,{.04,.08,-.03})))<1e-8,"3-axis world calibration failed");
 Q aim=ypr(-90,0,-90);J obs=J::array();double angle=10*pi/180;
 for(int i=0;i<4;++i){Q q=aim;if(i){V v{};v[i-1]=angle;q=qm(expq(v),q);}Q measured=qm(world,q);J f=J::array();for(int r=0;r<9;++r)f.push_back(J::array({0,0,0,1}));f[5]=rotate(measured,{0,0,1});f[6]=rotate(measured,{0,1,0});obs.push_back(f);}
 Q wanted=qm(world,qm(axis({1,0,0},.2),aim));c=correction({{"frames",obs},{"step",angle},{"normal",rotate(wanted,{0,0,1})},{"along",rotate(wanted,{0,1,0})},{"pose",pose(controller,aim)}},true);
 check(norm(logq(qm(q4(c["pose"]["q"]),inv(qm(axis({1,0,0},.2),aim)))))<1e-7,"orientation calibration failed");
 J staticRaw=obs[0],movingDraw=staticRaw;const Q rawQ=ypr(20,-10,5),drawA=ypr(0,0,0),drawB=ypr(130,40,60);
 staticRaw[1]=drawA;staticRaw[3]=rawQ;staticRaw[5]=rotate(drawA,{0,0,1});staticRaw[6]=rotate(drawA,{0,1,0});
 movingDraw[1]=drawB;movingDraw[3]=rawQ;movingDraw[5]=rotate(drawB,{0,0,1});movingDraw[6]=rotate(drawB,{0,1,0});
 check(stability({{"before",staticRaw},{"after",movingDraw},{"orientation",true}})["stable"].get<bool>(),"drawn contact rotation contaminated raw orientation calibration");
 J stance=standCandidates({{"plan",{{"mid",V{0,0,.1}},{"goal",V{0,0,.13}},{"forward",V{0,1,0}}}},
  {"floor",0},{"measurement",{{"shoulders",J::array({V{-.2,-.17,.8},V{.2,-.17,.8}})},{"lengths",J::array({.65,.65})}}}});
 J ground=J::array();for(const auto&candidate:stance["candidates"]){J h=J::array();for(int k=0;k<5;++k)h.push_back(V{});ground.push_back(h);}stance["hits"]=ground;
 J choice=chooseStand(stance);check(choice["predictedShoulderToPalm"].get<double>()<stance["candidates"][0]["predictedShoulderToPalm"].get<double>(),"stance chooser ignored actual shoulder distance on equally flat terrain");
 J hands=J::array({pose({-.1,-.4,-.3},{0,0,0,1}),pose({.1,-.4,-.3},{0,0,0,1})});J path=motion({{"mode","pair-turn"},{"hands",hands},{"angleDegrees",35},{"seconds",2}});
 auto end=path["frames"].back()["hands"];check(std::abs(norm(sub(v3(end[0]["p"]),v3(end[1]["p"])))-.2)<1e-10,"two-hand path stretched grip baseline");
 check(end[0]["q"]==hands[0]["q"]&&end[1]["q"]==hands[1]["q"],"two-hand test turned controllers instead of grip line");
 check(!analyze({{"phases",J::array({{{"name","empty"},{"hands",J::array({1})}}})},{"samples",J::array()}})["pass"].get<bool>(),"empty test incorrectly passed");
 J frame=J::array();for(int i=0;i<9;++i)frame.push_back(J::array({0,0,0,1}));frame[2]=J::array({.6,0,0,1});
 J hand={{"frame",frame},{"native",{{"active",true},{"proxy","71234ULL"},{"body",19},{"ageMs",5},{"contactMetricAvailable",true},{"contactValid",true},{"contactErrorMicrometres",5000}}},
  {"grasp",{{"held",true},{"kind",12},{"fast",{{"center",V{}},{"grip",V{}}}}}},
  {"referencePose",pose({0,0,0},{0,0,0,1})},{"nativePose",pose({0,0,0},{0,0,0,1})}};
 J measured=J::array();for(int i=0;i<5;++i)measured.push_back({{"phase","reach"},{"t",10+i},{"dead",true},
  {"player",{{"position",V{}}}},{"slots",{{"Hips",V{}}}},{"hands",J::array({hand,hand})}});
 J phase={{"name","reach"},{"hands",J::array({1})},{"minimumSeconds",3.5},{"minimumReachSeconds",3.}};
 check(analyze({{"phases",J::array({phase})},{"samples",measured}})["pass"].get<bool>(),"valid measured reach rejected");
 auto articulated=measured;for(auto&r:articulated)r["hands"][1]["referencePose"]["p"]=V{0,0,.9};
 check(analyze({{"phases",J::array({phase})},{"samples",articulated}})["pass"].get<bool>(),"reference articulation was mistaken for exact contact slip");
 for(auto&r:articulated)r["hands"][1]["native"]["contactValid"]=false;
 check(!analyze({{"phases",J::array({phase})},{"samples",articulated}})["pass"].get<bool>(),"invalid contact sentinel was treated as zero slip");
 auto changed=measured;changed[2]["hands"][1]["native"]["active"]=false;
 check(!analyze({{"phases",J::array({phase})},{"samples",changed}})["pass"].get<bool>(),"native hold loss incorrectly passed");
 changed=measured;for(auto&r:changed)r["hands"][1]["frame"][2]=J::array({0,0,0,1});
 check(!analyze({{"phases",J::array({phase})},{"samples",changed}})["pass"].get<bool>(),"far-away command without measured separation passed");
 changed=measured;changed[2]["hands"][1]["native"]["body"]=20;
 check(!analyze({{"phases",J::array({phase})},{"samples",changed}})["pass"].get<bool>(),"body identity change incorrectly passed");
 J geometryHand=hand;geometryHand["grasp"]["body"]=19;geometryHand["grasp"]["fast"]["position"]=V{};geometryHand["grasp"]["fast"]["rotation"]=Q{0,0,0,1};
 J geometryRight=geometryHand;geometryRight["frame"][0]=V{.2,0,0};geometryRight["frame"][2]=V{.2,0,0};J geometrySamples=J::array();
 for(int i=0;i<5;++i)geometrySamples.push_back({{"phase","same-part-setup-lift"},{"t",i},{"hands",J::array({geometryHand,geometryRight})}});
 geometryHand["native"]["contactErrorMicrometres"]=0;
 geometrySamples.push_back({{"phase","same-part-rest"},{"t",6},{"hands",J::array({geometryHand,geometryRight})}});
 geometryHand["frame"][0]=V{-.1,0,0};geometryRight["frame"][0]=V{.3,0,0};geometryHand["native"]["contactErrorMicrometres"]=100000;
 geometrySamples.push_back({{"phase","same-part-rotate"},{"t",7},{"hands",J::array({geometryHand,geometryRight})}});
 J geometry=pairGeometry({{"samples",geometrySamples}});
 check(std::abs(geometry["phases"][1]["minimumMeanErrorFromSpan"]["p95"].get<double>()-.1)<1e-8,"rigid 20cm anchors versus 40cm targets did not require 10cm error per hand");
 check(std::abs(geometry["phases"][1]["leftReconstructedError"]["p95"].get<double>()-.1)<1e-8,"exact left local-anchor reconstruction failed");
 phase["hands"]=J::array({0,1});phase["relation"]="different";
 check(!analyze({{"phases",J::array({phase})},{"samples",measured}})["pass"].get<bool>(),"same body presented as different limbs passed");
 phase.erase("relation");phase["minimumTravel"]=.15;phase["minimumCorpseTravel"]=.08;
 check(!analyze({{"phases",J::array({phase})},{"samples",measured}})["pass"].get<bool>(),"unmoved player/corpse passed walking test");
 J mass={{"proxy","68334"},{"body",18},{"holders",2},{"baseline",5},{"solverMass",100},{"reportedCarryMass",5}};
 check(massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"two hands' raw solver mass rejected");
 mass["holders"]=1;check(massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"remaining holder lost solver mass ownership");
 mass["holders"]=0;check(!massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"normalized cached5 hid failed physical100-to5 restore");
 mass["solverMass"]=5;check(massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"physical original mass restore rejected");
 mass["solverMass"]=-1;check(!massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"missing raw mass proved restoration");
 mass["previousReleaseVerified"]=true;check(massCheck({{"masses",J::array({mass})}})["pass"].get<bool>(),"previously verified actor cleanup rejected");
 J restored={{"player",{{"position",V{}},{"rotation",Q{0,0,0,1}}}},{"original",{{"position",V{}},{"rotation",Q{0,0,0,1}}}},{"equipment",true},{"npcRemoved",true},{"noHold",true},{"money",true}};
 check(!restoreCheck(restored)["pass"].get<bool>(),"equipment/currency equality hid unverified full inventory");
 restored["inventory"]=true;check(restoreCheck(restored)["pass"].get<bool>(),"verified complete inventory restore rejected");
 std::cout<<"{\"pass\":true,\"checks\":\"position/orientation Jacobians, rigid paired motion, missing/stale/lost/wrong-body/unmoved evidence rejection\"}\n";
}
int main(int argc,char**argv){try{if(argc<2)throw std::runtime_error("command required");std::string cmd=argv[1];if(cmd=="self-test"){selfTest();return 0;}if(argc!=3)throw std::runtime_error("expected command input.json");std::ifstream file(argv[2]);if(!file)throw std::runtime_error("cannot open input");J in;file>>in;J out;
 if(cmd=="initial")out=initial(in);else if(cmd=="limb")out=limb(in);else if(cmd=="second-grip")out=secondGrip(in);else if(cmd=="mass-probes")out=massProbes(in);else if(cmd=="posture")out=posture(in);else if(cmd=="stand-candidates")out=standCandidates(in);else if(cmd=="choose-stand")out=chooseStand(in);else if(cmd=="stability")out=stability(in);else if(cmd=="position-probes")out=probes(in,false);else if(cmd=="orientation-probes")out=probes(in,true);else if(cmd=="position-solve")out=correction(in,false);else if(cmd=="orientation-solve")out=correction(in,true);else if(cmd=="face")out=face(in);else if(cmd=="turn-rate")out=turnRate(in);else if(cmd=="motion")out=motion(in);else if(cmd=="head")out=head(in.value("height",1.7),in.value("pitch",-45.));else if(cmd=="analyze")out=analyze(in);else if(cmd=="pair-geometry")out=pairGeometry(in);else if(cmd=="restore-check")out=restoreCheck(in);else if(cmd=="mass-check")out=massCheck(in);else throw std::runtime_error("unknown command");std::cout<<out.dump()<<'\n';return 0;
 }catch(const std::exception&e){std::cerr<<e.what()<<'\n';return 2;}}
