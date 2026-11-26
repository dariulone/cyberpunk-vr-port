// Reuse the real grasp implementation and its offline rig boundary stubs.
#define main GraspVisualHarnessMain
#include "harness.cpp"
#undef main
#include <array>
#include <limits>

static void Verify(bool condition,const char* message){if(!condition){std::fprintf(stderr,"FAIL: %s\n",message);std::exit(1);}}
static std::vector<float> Rings(const std::array<float,16>& radii,bool reverse=false){
    std::vector<float> points;
    for(int i=0;i<16;++i)for(int k=0;k<16;++k){
        const float angle=6.283185307f*k/16,z=-.104f+.013f*(i+.5f);
        points.insert(points.end(),{radii[i]*std::cos(angle),radii[i]*std::sin(angle),reverse?-z:z});
    }
    return points;
}
int main(){
    // A narrow internal waist with a foot and bowl, approximately the cooked
    // wine glass. The test exercises geometry selection, not its filename.
    const std::array<float,16> glass{.035f,.004f,.004f,.004f,.004f,.004f,.014f,.027f,.037f,.043f,.046f,.046f,.046f,.045f,.044f,.041f};
    for(bool reverse:{false,true}){
        auto points=Rings(glass,reverse);float hand[3]{.04f,0,reverse?.05f:-.05f};cvr::anim::GraspPart part;
        Verify(cvr::anim::GraspPartOf(points.data(),int(points.size()/3),.104f,hand,part),"internal stem missing");
        Verify(part.stem&&!part.neck&&part.ends==0,"stem exposes a nonexistent free end");
        Verify(std::abs(part.radius-.004f)<1e-6f&&part.half>.025f,"stem dimensions lost");
        Verify(reverse?part.z>0:part.z<0,"reversed stem was not mirrored");
        const float half[3]{part.radius,part.radius,part.half};Verify(PrecisionStem(kCylinder,half,part.ends,true),"precision grasp not selected");
        hand[2]=reverse?-.075f:.075f;
        cvr::anim::GraspPartOf(points.data(),int(points.size()/3),.104f,hand,part);
        Verify(!part.stem,"bowl contact snapped to remote stem or reused prior result");
    }
    std::array<float,16> bottle{};bottle.fill(.035f);for(int i=11;i<16;++i)bottle[i]=.013f;
    for(bool reverse:{false,true}){
        const auto points=Rings(bottle,reverse);float hand[3]{.02f,0,reverse?-.08f:.08f};cvr::anim::GraspPart part;
        Verify(cvr::anim::GraspPartOf(points.data(),int(points.size()/3),.104f,hand,part)&&part.neck&&!part.stem,"bottle neck changed");
        Verify(part.ends==(reverse?1:2),"bottle end changed");
    }
    auto thick=glass;for(int i=1;i<6;++i)thick[i]=.02f;
    auto points=Rings(thick);float hand[3]{.04f,0,-.05f};cvr::anim::GraspPart part;
    cvr::anim::GraspPartOf(points.data(),int(points.size()/3),.104f,hand,part);Verify(!part.stem,"wide waist treated as precision stem");
    points[2]=std::numeric_limits<float>::quiet_NaN();
    Verify(!cvr::anim::GraspPartOf(points.data(),int(points.size()/3),.104f,hand,part),"NaN point accepted");
    Verify(!cvr::anim::GraspPartOf(points.data(),int(points.size()/3),INFINITY,hand,part),"infinite extent accepted");
    std::puts("grasp part: internal stems, reversed axes, near selection, bottle regression and invalid input PASS");
}
