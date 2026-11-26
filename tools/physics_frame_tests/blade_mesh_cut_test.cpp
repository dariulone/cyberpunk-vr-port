#include "Anim/BladeMeshCut.hpp"
#include <algorithm>
#include <cmath>
#include <cstdlib>
#include <iostream>
#include <limits>

using namespace cvr::blade::mesh;
static void Check(bool ok,const char* message) {
    if(!ok) {std::cerr<<message<<'\n';std::exit(1);}
}
int main() {
    Volume volume{};
    Check(MakeVolume({{-.05,0,0},{.05,0,0},{0,0,1},.005,.025,.01},volume),"valid swept channel");
    const Triangle surface{{{-.2,-.2,0},{.2,-.2,0},{0,.2,0}}};
    std::vector<Triangle> pieces;
    Check(Subtract(surface,volume,pieces) && pieces.size()>1,"internal cut must create new faces");
    double remaining=0;
    for(const auto& piece:pieces) {
        for(const auto& point:piece) {
            Check(point[0]>=-1e-9 && point[1]>=-1e-9 && point[2]>=-1e-9,"out of triangle");
            Check(std::abs(point[0]+point[1]+point[2]-1)<1e-9,"bad barycentrics");
        }
        const auto& first=piece[0];const auto& second=piece[1];const auto& third=piece[2];
        const double fraction=(second[1]-first[1])*(third[2]-first[2])-(second[2]-first[2])*(third[1]-first[1]);
        Check(fraction>0,"reversed winding");
        remaining+=fraction*.08;
    }
    Check(std::abs(remaining-.079)<1e-10,"removed area is not the bounded footprint");
    std::vector<Triangle> ink;
    auto area=[](const std::vector<Triangle>& faces) {
        double sum{};
        for(const auto& t:faces) sum+=((t[1][1]-t[0][1])*(t[2][2]-t[0][2])-(t[1][2]-t[0][2])*(t[2][1]-t[0][1]))*.08;
        return sum;
    };
    Check(MarkFace(surface,volume,false,pieces,ink) && std::abs(area(ink)-.001)<1e-10,"stamp footprint area");
    Check(std::abs(area(pieces)+area(ink)-.08)<1e-10,"stamp duplicated or lost surface");
    Check(MarkFace(surface,volume,false,pieces,ink,true) && std::abs(area(ink)-.0005)<1e-10,"distinct diamond puncture silhouette");
    Check(MarkFace(surface,volume,true,pieces,ink) && std::abs(area(ink)-.0006)<1e-10,"trail rim covered the open seam");
    for(const auto& tri:ink) for(const auto& weights:tri)
        Check(std::abs(weights[0]+weights[1]+weights[2]-1)<1e-10 && *std::min_element(weights.begin(),weights.end())>=-1e-10,
              "ink left its source triangle");
    auto behind=surface;for(auto& pt:behind)pt[2]=-.026;
    Check(!Subtract(behind,volume,pieces) && pieces.empty(),"cut appeared on back surface");
    auto beyond=surface;for(auto& pt:beyond)pt[0]+=.3;
    Check(!Subtract(beyond,volume,pieces),"cut exceeded stroke endpoints");
    Check(Subtract({{{0,0,0},{.001,0,0},{0,.001,0}}},volume,pieces) && pieces.empty(),"fully consumed face retained");
    auto invalid=surface;invalid[1][2]=std::numeric_limits<double>::quiet_NaN();
    Check(!Subtract(invalid,volume,pieces),"invalid geometry accepted");
    Check(!MakeVolume({{0,0,0},{0,0,.1},{0,0,1},.005,.01,.01},volume),"missing tangent accepted");
    double distance=1;
    Point weights{};
    Check(ProjectSurface({0,0,.06},{0,0,1},surface,distance,weights) && std::abs(distance+.06)<1e-9,"skin below collision proxy");
    Check(!ProjectSurface({0,0,.10},{0,0,1},surface,distance,weights),"farther layer overrode nearer skin");
    distance=1;
    Check(!ProjectSurface({0,0,.13},{0,0,1},surface,distance,weights),"unbounded projection onto remote surface");
    Check(!ProjectSurface({.25,.25,.01},{0,0,1},surface,distance,weights),"projection outside triangle");
    double fraction=2;
    Point normal;
    Check(SegmentHit({0,0,.05},{0,0,-.15},surface,fraction,normal) && std::abs(fraction-.25)<1e-9 && normal[2]>0,"physical blade entry");
    Check(!SegmentHit({0,0,.3},{0,0,.02},surface,fraction,normal),"tip has not reached mesh");
    Check(!SegmentHit({0,0,.1},{0,0,-.1},surface,fraction,normal),"rear surface replaced first intersection");
    const auto sweep=SampleSweep({{{0,0,.1},{0,0,-.1},{.10,0,.1},{.10,0,-.1}}});
    const Triangle between{{{.022,-.01,0},{.028,-.01,0},{.025,.01,0}}};
    SurfaceHit exact;
    LocateStamp(sweep,{.025,0,0},{0,0,1},between,exact);
    Check(exact.found && exact.exact && std::abs(exact.point[2])<1e-9,"missed triangle between XR samples");
    SurfaceHit graze;
    const auto grazing=SampleSweep({{{0,-.02,.009},{0,.02,.009},{0,-.02,.009},{0,.02,.009}}});
    LocateStamp(grazing,{0,0,.009},{0,0,1},surface,graze);
    Check(graze.found && !graze.exact && std::abs(graze.point[2])<1e-9,"graze floated off the render face");
    SurfaceHit miss;
    auto far=grazing;for(auto& ray:far) for(auto& p:ray)p[2]=.05;
    LocateStamp(far,{0,0,.16},{0,0,1},surface,miss);Check(!miss.found,"painted beyond the native projection interval");
    SurfaceHit projected;
    LocateStamp(far,{0,0,.05},{0,0,1},surface,projected);
    Check(projected.found && !projected.onSegment && std::abs(projected.point[2])<1e-9,"shell projection left the surface");
    Triangle normals{{{0,0,1},{0,0,1},{0,0,1}}};
    Cut partial{};
    Check(ResolveSweep(sweep,{.5,.5,2.},normals,partial) && partial.from[0]==0 && partial.to[0]==.05,"stroke leaving the mesh");
    Check(ResolveSweep(sweep,{2.,.5,.5},normals,partial) && partial.from[0]==.05 && partial.to[0]==.10,"stroke entering the mesh");
    Check(!ResolveSweep(sweep,{.5,2.,.5},normals,partial),"bridged a gap in surface coverage");
    normals[1]={0,0,-1};
    Check(!ResolveSweep(sweep,{.5,.5,.5},normals,partial),"bridged opposing surfaces");
    std::cout<<"finite mesh clipping: winding, area, interior slit, complete removal, miss and depth PASS\n";
}
