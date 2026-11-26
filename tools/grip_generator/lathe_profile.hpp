#pragma once
#include "cr2w_mesh.hpp"
namespace gripgen {
// A conservative radial profile for the existing round-prop grasp path. Every
// triangle edge is clipped to each axial slice: long sides cannot disappear
// between sparse vertex rings, and there is no sampling step to tune.
inline Json LatheProfile(const Mesh& mesh){
    constexpr int slices=16;
    if(mesh.vertices.empty()||mesh.triangles.empty())return nullptr;
    std::array<float,3> lo{INFINITY,INFINITY,INFINITY},hi{-INFINITY,-INFINITY,-INFINITY};
    const auto point=[](Vec3 v){return std::array<float,3>{v.x,v.y,v.z};};
    for(auto vertex:mesh.vertices){const auto v=point(vertex);for(int k=0;k<3;++k){lo[k]=std::min(lo[k],v[k]);hi[k]=std::max(hi[k],v[k]);}}
    const std::array<float,3> extent{hi[0]-lo[0],hi[1]-lo[1],hi[2]-lo[2]};
    const int axis=int(std::max_element(extent.begin(),extent.end())-extent.begin()),b=(axis+1)%3,c=(axis+2)%3;
    const float section=std::max(extent[b],extent[c]);
    // Whole-mesh profiles are only for things the current hand grasp can hold.
    // Large environment geometry remains in the catalog for contact/climbing.
    if(section<.004f||extent[axis]>1.f||std::abs(extent[b]-extent[c])>.25f*section||extent[axis]<1.2f*section)return nullptr;
    const float cb=.5f*(lo[b]+hi[b]),cc=.5f*(lo[c]+hi[c]),width=extent[axis]/slices;
    std::array<float,slices> radius{};std::array<bool,slices> seen{};
    const auto take=[&](int slice,const std::array<float,3>& v){
        radius[slice]=std::max(radius[slice],std::hypot(v[b]-cb,v[c]-cc));seen[slice]=true;
    };
    for(const auto& tri:mesh.triangles)for(int edge=0;edge<3;++edge){
        auto u=point(mesh.vertices[tri[edge]]),v=point(mesh.vertices[tri[(edge+1)%3]]);
        if(u[axis]>v[axis])std::swap(u,v);
        const int first=std::clamp(int((u[axis]-lo[axis])/width),0,slices-1),last=std::clamp(int((v[axis]-lo[axis])/width),0,slices-1);
        const float span=v[axis]-u[axis];
        for(int slice=first;slice<=last;++slice){
            if(span<1e-8f){take(slice,u);take(slice,v);continue;}
            const float low=std::max(u[axis],lo[axis]+width*slice),high=std::min(v[axis],lo[axis]+width*(slice+1));
            if(high<low)continue;
            for(float z:{low,high}){
                const float t=std::clamp((z-u[axis])/span,0.f,1.f);std::array<float,3> p{};
                for(int k=0;k<3;++k)p[k]=u[k]+(v[k]-u[k])*t;take(slice,p);
            }
        }
    }
    // Disconnected geometry must not become an invented solid between parts.
    if(std::find(seen.begin(),seen.end(),false)!=seen.end())return nullptr;
    return {{"axis",axis},{"low",lo[axis]},{"high",hi[axis]},{"center_b",cb},{"center_c",cc},{"radii",radius}};
}
}
