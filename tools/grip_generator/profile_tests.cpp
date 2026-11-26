#include "lathe_profile.hpp"
#include <iostream>
using namespace gripgen;
static Mesh Tube(int axis,float length,float radius,float centerB,float centerC){
    Mesh mesh;constexpr unsigned sides=64;
    for(unsigned end=0;end<2;++end)for(unsigned i=0;i<sides;++i){
        std::array<float,3> p{};const float angle=6.283185307f*float(i)/sides;
        p[axis]=end?length:0;p[(axis+1)%3]=centerB+radius*std::cos(angle);p[(axis+2)%3]=centerC+radius*std::sin(angle);
        mesh.vertices.push_back({p[0],p[1],p[2]});
    }
    for(unsigned i=0;i<sides;++i){const unsigned j=(i+1)%sides;mesh.triangles.push_back({i,j,i+sides});mesh.triangles.push_back({j,j+sides,i+sides});}
    return mesh;
}
int main(){
    try {
        // A long side has no vertices in fourteen of sixteen slices. Edge
        // clipping must still recover the same cylinder in all three axes.
        for(int axis=0;axis<3;++axis){
            const auto profile=LatheProfile(Tube(axis,.8f,.025f,.17f,-.08f));
            Require(!profile.is_null()&&profile.at("axis")==axis,"tube not recognized");
            Require(std::abs(profile.at("center_b").get<float>()-.17f)<1e-6f,"profile center shifted");
            for(const auto& r:profile.at("radii"))Require(std::abs(r.get<float>()-.025f)<1e-6f,"sparse side lost between vertex rings");
        }
        auto separate=Tube(2,.2f,.025f,0,0);
        for(auto& vertex:separate.vertices)if(vertex.z>.1f)vertex.z=.02f;
        auto top=Tube(2,.02f,.025f,0,0);const auto base=uint32_t(separate.vertices.size());
        for(auto v:top.vertices){v.z+=.18f;separate.vertices.push_back(v);}
        for(auto t:top.triangles){for(auto& i:t)i+=base;separate.triangles.push_back(t);}
        Require(LatheProfile(separate).is_null(),"disconnected pieces filled with invented material");
        Require(LatheProfile(Tube(2,2.f,.025f,0,0)).is_null(),"oversized prop accepted");
        std::cout<<"mesh profile: sparse edges, axis/center, disconnected gap and size gate PASS\n";return 0;
    }catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
}
