// The numerical fitter uses the production finger solver and the offline FPP
// skinning implementation. Scripts only select inputs and collect these files.
#define main GraspVisualHarnessMain
#include "../grasp_tests/harness.cpp"
#undef main
#include <filesystem>
#include <iomanip>
#include <iostream>
using Json=nlohmann::json;

namespace {
struct Candidate {
    Request request{};
    float rotation[kSlots][4]{};
    float clearance[6]{};
    float precisionGap[3]{},precisionZ[3]{};
    float angles[17]{},thumb[4]{};
    float d{},n{},t{},tilt{};
    double score=1e30;
};
static hm::Mesh handMeshes[2];
static std::vector<size_t> padVertices[2][3];
static void RequireFit(bool ok,const char* why){if(!ok)throw std::runtime_error(why);}
static void ResetSides(){
    ResetBuffer();
    for(auto& side:g_side){side.active=false;side.blend=0;side.palm=false;side.rev=0;side.solved=~0u;
        side.person=side.personOn=side.shownOn=false;side.style=0;side.req=Request{};}
    cvr::anim::GraspFingers(g_buf.data());
}
static Request RequestFor(int side,const Json& profile,float sampleZ,bool reverse,uint64_t meshHash=0){
    const float low=profile.at("low").get<float>(),high=profile.at("high").get<float>(),L=.5f*(high-low);
    const auto& radii=profile.at("radii");RequireFit(radii.size()==16&&L>.01f,"invalid radial profile");
    std::vector<float> points;float widest=0;
    for(int slice=0;slice<16;++slice){
        const float radius=radii.at(slice).get<float>();widest=std::max(widest,radius);
        for(int a=0;a<16;++a){const float angle=6.283185307f*a/16;
            points.insert(points.end(),{radius*std::cos(angle),radius*std::sin(angle),-L+2*L*(slice+.5f)/16});}
    }
    const float handInItem[3]{widest,0,sampleZ};cvr::anim::GraspPart part;
    const bool selected=cvr::anim::GraspPartOf(points.data(),int(points.size()/3),L,handInItem,part);
    Request request;request.shape=kCylinder;request.half[0]=request.half[1]=selected?part.radius:widest;request.half[2]=selected?part.half:L;
    request.ends=selected?part.ends:15;request.part=selected;request.lathe=part.lathe;
    auto& state=g_side[side];float q[4]{0,0,0,1},at[3];ToFrame(state.pm.slot,state.pm.skin,at);
    if(reverse){float normal[3];Unrot(state.pm.slot.q,state.pm.n,normal);AxisAngle(normal,kPi,q);}
    const float localOffset[3]{0,0,sampleZ-(selected?part.z:0.f)};float offset[3];Rot(q,localOffset,offset);Sub(at,offset,at);
    int kind{};RequireFit(Seat(side,request.shape,request.half,at,q,nullptr,request.obj,&kind,request.ends,request.part,&request.lathe,meshHash),"no reference palm");
    return request;
}
static Candidate Evaluate(int side,const Request& seed,float d,float n,float t,float tilt){
    ResetSides();auto& state=g_side[side];Candidate candidate;candidate.request=seed;
    candidate.d=d;candidate.n=n;candidate.t=t;candidate.tilt=tilt;
    Pose placed=Compose(state.pm.slot,seed.obj);
    const float* axes[3]{state.pm.d,state.pm.n,state.pm.t};const float offsets[3]{d,n,t};
    for(int axis=0;axis<3;++axis)Mad(placed.p,axes[axis],offsets[axis],placed.p);
    if(tilt!=0){float turn[4],rotated[4];AxisAngle(state.pm.n,tilt*kDeg,turn);VRIK_QuatMul(turn,placed.q,rotated);std::copy_n(rotated,4,placed.q);VRIK_QuatNorm(placed.q);}
    ToFrame(state.pm.slot,placed.p,candidate.request.obj.p);float inverse[4];VRIK_QuatConj(state.pm.slot.q,inverse);VRIK_QuatMul(inverse,placed.q,candidate.request.obj.q);
    state.req=candidate.request;state.active=true;state.blend=1;++state.rev;
    cvr::anim::GraspFingers(g_buf.data());
    std::memcpy(candidate.rotation,state.rot,sizeof(candidate.rotation));
    for(int i=0;i<17;++i)candidate.angles[i]=CyberpunkVR_DebugGraspAngles[side][i];
    for(int i=0;i<4;++i)candidate.thumb[i]=CyberpunkVR_DebugGraspThumb[side][i];
    const int hand=side?g_VRRightBoneIdx:g_VRLeftBoneIdx;
    hm::Skin(handMeshes[side],hand);Scene scene;scene.pm=state.pm;scene.rq=state.req;scene.obj=Compose(state.pm.slot,state.req.obj);
    hm::Clearance(handMeshes[side],scene,candidate.clearance);
    const int kind=int(candidate.angles[0]);const bool precision=kind==kPinch||kind==kTripod;
    double score=300*(double(d)*d+double(n)*n+double(t)*t)+.0003*tilt*tilt;
    for(int finger=0;finger<6;++finger){
        RequireFit(std::isfinite(candidate.clearance[finger]),"non-finite skin clearance");
        const double penetration=std::max(0.f,-candidate.clearance[finger]-.001f);score+=1e6*penetration*penetration;
        if(finger>=1&&(!precision||finger<=3)){
            const double gap=std::max(0.f,candidate.clearance[finger]-.004f);score+=2e4*gap*gap;
        }
    }
    const double padGap=std::max(0.f,candidate.thumb[2]-.004f*1000)/1000;
    score+=2e4*padGap*padGap;
    const double goal=std::max(0.f,candidate.thumb[3]-35.f)/1000;score+=4000*goal*goal;
    const float lower[4]{-5,-25,0,-5},upper[4]{70,65,60,80};
    for(int joint=0;joint<4;++joint){const float angle=candidate.angles[joint+1];const float over=std::max({0.f,lower[joint]-angle,angle-upper[joint]});score+=.2*over*over;}
    if(precision){
        // A stem grasp must actually contact the stem. Whole-object clearance
        // alone rewards holding the glass by its foot, a different grasp.
        const auto& mesh=handMeshes[side];
        for(int digit=0;digit<3;++digit){
            float nearest=1e9f,zBest=0;
            for(size_t vertex:padVertices[side][digit]){
                float local[3];ToFrame(scene.obj,&mesh.sv[vertex*3],local);
                float within[3]{local[0],local[1],local[2]};
                const float limit=std::max(.001f,scene.rq.half[2]-.003f);
                if(PrecisionStem(scene.rq.shape,scene.rq.half,scene.rq.ends,scene.rq.part))within[2]=std::clamp(local[2],-limit,limit);
                const float radial=scene.rq.lathe.knots>=2?LatheSdf(scene.rq.lathe,within):Sdf(scene.rq.shape,scene.rq.half,within);
                const float distance=std::hypot(radial,local[2]-within[2]);
                if(distance<nearest){nearest=distance;zBest=local[2];}
            }
            candidate.precisionGap[digit]=nearest;candidate.precisionZ[digit]=zBest;
            const double gap=std::max(0.f,nearest-.003f);score+=8e4*gap*gap;
        }
        // The unoccupied fingers should curl comfortably clear of the foot,
        // rather than remain extended solely because the initial seat blocked
        // their roots. This term never overrides actual skin penetration.
        for(int finger=2;finger<4;++finger){const float mcp=candidate.angles[5+3*finger];const float pip=candidate.angles[6+3*finger];score+=.0004*(std::max(0.f,40-mcp)*std::max(0.f,40-mcp)+std::max(0.f,45-pip)*std::max(0.f,45-pip));}
    }
    candidate.score=score;return candidate;
}
static Json Describe(const Candidate& candidate,int side){
    Json rotations=Json::array(),delta=Json::array();
    for(int slot=0;slot<kSlots;++slot){
        rotations.push_back({candidate.rotation[slot][0],candidate.rotation[slot][1],candidate.rotation[slot][2],candidate.rotation[slot][3]});
        const int bone=s_fingers[side][slot];float inverse[4],q[4];VRIK_QuatConj(s_ref[bone],inverse);VRIK_QuatMul(inverse,candidate.rotation[slot],q);VRIK_QuatNorm(q);delta.push_back({q[0],q[1],q[2],q[3]});
    }
    return {{"score",candidate.score},{"shift_dnt_m",{candidate.d,candidate.n,candidate.t}},{"tilt_degrees",candidate.tilt},
        {"kind",int(candidate.angles[0])},{"skin_clearance_m",candidate.clearance},{"precision_pad_gap_m",candidate.precisionGap},{"precision_pad_z_m",candidate.precisionZ},{"thumb_diagnostics",candidate.thumb},{"joint_angles_degrees",candidate.angles},
        {"item_in_slot",{{"position",candidate.request.obj.p},{"rotation",candidate.request.obj.q}}},{"local_rotations",rotations},{"reference_deltas",delta}};
}
static bool Accept(const Candidate& candidate){
    for(float clearance:candidate.clearance)if(!std::isfinite(clearance)||clearance<-.0015f)return false;
    if(int(candidate.angles[0])==kTripod||int(candidate.angles[0])==kPinch)
        for(float gap:candidate.precisionGap)if(!std::isfinite(gap)||gap>.004f)return false;
    for(int finger=0;finger<4;++finger)
        if(candidate.angles[7+3*finger]>.8f*candidate.angles[6+3*finger]+15.1f)return false;
    const float lower[4]{-5,-25,0,-5},upper[4]{70,65,60,80};
    for(int j=0;j<4;++j)if(candidate.angles[j+1]<lower[j]-.1f||candidate.angles[j+1]>upper[j]+.1f)return false;
    return true;
}
static void Preview(int side,const Candidate& candidate,const std::filesystem::path& path){
    const auto fitted=Evaluate(side,candidate.request,0,0,0,0); // apply the selected seat using the production solve
    (void)fitted;auto& state=g_side[side];Scene scene;scene.pm=state.pm;scene.rq=state.req;scene.obj=Compose(state.pm.slot,state.req.obj);scene.mesh=&handMeshes[side];
    const int size=320;std::vector<uint8_t> pixels(size_t(size)*size*5*3);
    float center[3];Mad(scene.pm.skin,scene.pm.d,.045f,center);Mad(center,scene.pm.n,.025f,center);
    const float views[5][3]{{1,0,0},{-1,0,0},{0,1,0},{.5f,.6f,.6f},{0,0,-1}};
    for(int view=0;view<5;++view){float fwd[3];for(int k=0;k<3;++k)fwd[k]=scene.pm.t[k]*views[view][0]+scene.pm.d[k]*views[view][1]+scene.pm.n[k]*views[view][2];Unit(fwd);Render(scene,center,.32f,size,pixels,view*size,5*size,fwd,view==2?scene.pm.t:scene.pm.d);}
    std::ofstream file(path,std::ios::binary);RequireFit(bool(file),"cannot write pose preview");file<<"P6\n"<<5*size<<' '<<size<<"\n255\n";file.write(reinterpret_cast<const char*>(pixels.data()),std::streamsize(pixels.size()));
}
}
int main(int argc,char** argv){
    try {
        RequireFit(argc>=4,"usage: grip_pose_baker RIG PROFILE_JSON OUTPUT_PREFIX [sample_z_m] [reverse_axis_0_or_1] [--verify-runtime]");
        g_buf.assign(800*48,0);for(int b=0;b<800;++b)reinterpret_cast<float*>(&g_buf[b*48+VRIK_ROT_OFF])[3]=1;
        LoadRig(argv[1]);const auto directory=std::filesystem::path(argv[1]).parent_path();
        hm::LoadNames((directory/"mesh/rig_names.txt").string().c_str());ResetBuffer();
        RequireFit(hm::Load(handMeshes[0],(directory/"mesh/hand_l.bin").string().c_str(),0,g_VRLeftBoneIdx),"left mesh unavailable");
        RequireFit(hm::Load(handMeshes[1],(directory/"mesh/hand_r.bin").string().c_str(),1,g_VRRightBoneIdx),"right mesh unavailable");
        ResetSides();
        for(int side=0;side<2;++side){
          hm::Skin(handMeshes[side],side?g_VRRightBoneIdx:g_VRLeftBoneIdx);
          for(int digit=0;digit<3;++digit){
            const auto& mesh=handMeshes[side];const int* slots=digit==0?kThumbSlots:kFingers[digit-1].slot;
            const int distal=s_fingers[side][slots[2]],hand=side?g_VRRightBoneIdx:g_VRLeftBoneIdx;
            float reference[kSlots][4];ReferenceFingers(g_buf.data(),side,reference,nullptr);
            ChainBones chain;RequireFit(ChainOf(g_buf.data(),side,hand,slots,reference,chain),"finger reference chain unavailable");
            float rotations[3][4],joints[4][3],hinge[3][3];Pose frames[3];
            for(int k=0;k<3;++k)std::copy_n(chain.local[k].q,4,rotations[k]);Joints(chain,rotations,joints,frames);
            float toward[3];std::copy_n(g_side[side].pm.n,3,toward);
            if(digit==0)Sub(g_side[side].pm.skin,joints[1],toward);
            RequireFit(Hinges(joints,frames,toward,hinge),"finger reference hinge unavailable");
            float segment[3],axis[3],padNormal[3];Sub(joints[3],joints[2],segment);Rot(frames[2].q,hinge[2],axis);Cross(axis,segment,padNormal);Unit(padNormal);
            for(size_t vertex=0;vertex<mesh.group.size();++vertex){
                if(mesh.group[vertex]!=digit+1)continue;float influence=0;
                for(int slot=0;slot<8;++slot){const auto joint=mesh.j[vertex*8+slot];
                    const float share=(mesh.a[joint]==distal?1-mesh.mix[joint]:0)+(mesh.b[joint]==distal?mesh.mix[joint]:0);
                    influence+=mesh.w[vertex*8+slot]*share;}
                if(influence>=.4f&&Dot(&mesh.sn[vertex*3],padNormal)>.2f)padVertices[side][digit].push_back(vertex);
            }
            RequireFit(!padVertices[side][digit].empty(),"finger pad skin unavailable");
          }
        }
        std::ifstream input(argv[2]);Json document;input>>document;const float sample=argc>4?std::stof(argv[4]):0.f;
        const std::string meshHash=document.value("hash",std::string{});
        const bool reverse=argc>5&&std::stoi(argv[5])!=0;
        const bool verify=argc>6&&std::string_view(argv[6])=="--verify-runtime";
        if(verify){
            std::ifstream expectedFile(std::string(argv[3])+".json");Json expected;expectedFile>>expected;
            RequireFit(!meshHash.empty(),"runtime verification needs a depot hash");const auto hash=std::stoull(meshHash,nullptr,16);
            for(int side=0;side<2;++side){
                ResetSides();const auto request=RequestFor(side,document.at("round_profile"),sample,reverse,hash);
                const auto actual=Evaluate(side,request,0,0,0,0);const auto& reference=expected.at("hands").at(side).at("fitted");
                for(int k=0;k<3;++k)RequireFit(std::abs(actual.request.obj.p[k]-reference.at("item_in_slot").at("position").at(k).get<float>())<1e-5f,"runtime fitted position differs");
                double dot=0;for(int k=0;k<4;++k)dot+=actual.request.obj.q[k]*reference.at("item_in_slot").at("rotation").at(k).get<float>();
                RequireFit(std::abs(dot)>.99999,"runtime fitted orientation differs");
                for(int slot=0;slot<kSlots;++slot){dot=0;for(int k=0;k<4;++k)dot+=actual.rotation[slot][k]*reference.at("local_rotations").at(slot).at(k).get<float>();RequireFit(std::abs(dot)>.99999,"runtime finger pose differs");}
                RequireFit(Accept(actual),"runtime pose violates fitted contact/angle limits");
            }
            std::cout<<"runtime fitted seat and all nineteen finger rotations match both generated hands\n";return 0;
        }
        Json output={{"rig",argv[1]},{"profile",argv[2]},{"mesh_hash",meshHash},{"sample_z_m",sample},{"reverse_axis",reverse},{"hands",Json::array()}};
        std::ofstream fits(std::string(argv[3])+".inc");RequireFit(bool(fits),"cannot write fitted seat include");
        fits<<"// Generated by tools/grip_generator/pose_baker.cpp; do not edit.\n"<<std::scientific<<std::setprecision(9);
        for(int side=0;side<2;++side){
            ResetSides();const Request seed=RequestFor(side,document.at("round_profile"),sample,reverse);
            const Candidate baseline=Evaluate(side,seed,0,0,0,0);Candidate best=baseline;
            for(float d:{-.02f,-.01f,0.f,.01f,.02f})for(float n:{-.01f,0.f,.01f,.02f})for(float t:{-.025f,-.0125f,0.f,.0125f,.025f})for(float tilt:{-12.f,0.f,12.f}){
                const auto candidate=Evaluate(side,seed,d,n,t,tilt);if(candidate.score<best.score)best=candidate;
            }
            for(float step:{.005f,.0025f,.00125f})for(int iteration=0;iteration<3;++iteration){
                bool improved=false;
                for(int dimension=0;dimension<3;++dimension)for(float sign:{-1.f,1.f}){
                    float shift[3]{best.d,best.n,best.t};shift[dimension]=std::clamp(shift[dimension]+sign*step,-.035f,.035f);
                    const auto candidate=Evaluate(side,seed,shift[0],shift[1],shift[2],best.tilt);if(candidate.score<best.score){best=candidate;improved=true;}
                }
                if(!improved)break;
            }
            const bool accepted=Accept(best);
            auto row=Json{{"side",side},{"accepted",accepted},{"baseline",Describe(baseline,side)},{"fitted",Describe(best,side)}};output["hands"].push_back(row);
            if(accepted&&!meshHash.empty()){
                size_t consumed{};const auto hash=std::stoull(meshHash,&consumed,16);RequireFit(consumed==meshHash.size(),"invalid mesh hash");
                const auto& palm=g_side[side].pm;
                const Pose placed=Compose(palm.slot,best.request.obj);const float z[3]{0,0,1};float axis[3];Rot(placed.q,z,axis);
                const int axisSign=Dot(axis,palm.t)>=0?1:-1;
                fits<<"{0x"<<std::hex<<hash<<std::dec<<"ull, "<<side<<", "<<best.request.shape<<", "<<int(best.angles[0])<<", "<<best.request.ends<<", "<<(best.request.part?"true":"false")<<", "<<axisSign<<", {";
                for(int k=0;k<3;++k){if(k)fits<<", ";fits<<best.request.half[k]<<'f';}
                fits<<"}, "<<palm.halfD<<"f, "<<palm.halfT<<"f, {"<<best.d<<"f, "<<best.n<<"f, "<<best.t<<"f}, "<<best.tilt<<"f},\n";
            }
            std::cout<<"side "<<side<<" score "<<baseline.score<<" -> "<<best.score<<" shift "<<best.d<<' '<<best.n<<' '<<best.t<<" tilt "<<best.tilt<<'\n'<<std::flush;
            Preview(side,best,std::string(argv[3])+(side?"_right.ppm":"_left.ppm"));
        }
        std::ofstream file(std::string(argv[3])+".json");RequireFit(bool(file),"cannot write fitted poses");file<<output.dump(2)<<'\n';return 0;
    }catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
}
