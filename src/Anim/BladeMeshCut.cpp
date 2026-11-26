#include "Anim/BladeMeshCut.hpp"
#include <algorithm>
#include <cmath>
#include <limits>

namespace cvr::blade::mesh {
namespace {
Point Delta(const Point& left,const Point& right) {
    return {left[0]-right[0],left[1]-right[1],left[2]-right[2]};
}
double Dot(const Point& left,const Point& right) {
    return left[0]*right[0]+left[1]*right[1]+left[2]*right[2];
}
bool Normalize(Point& value) {
    const auto length=std::sqrt(Dot(value,value));
    if(!(length>1e-8) || !std::isfinite(length)) return false;
    for(auto& element:value) element/=length;
    return true;
}
Point Cross(const Point& left,const Point& right) {
    return {left[1]*right[2]-left[2]*right[1],left[2]*right[0]-left[0]*right[2],left[0]*right[1]-left[1]*right[0]};
}
struct Polygon {
    std::array<Point,16> vertices{};
    unsigned count{};
    void Add(const Point& point) { vertices[count++]=point; }
};
}
SweepSamples SampleSweep(const std::array<Point,4>& endpoints) {
    SweepSamples rays{{{endpoints[0],endpoints[1]}, {}, {endpoints[2],endpoints[3]}}};
    for(unsigned end=0;end<2;++end) for(unsigned axis=0;axis<3;++axis)
        rays[1][end][axis]=(rays[0][end][axis]+rays[2][end][axis])*.5;
    return rays;
}
bool ResolveSweep(const SweepSamples& samples,const std::array<double,3>& fractions,const Triangle& normals,Cut& cut) {
    // Do not bridge a missed midpoint. On entry or exit, join only the
    // confirmed half of the stroke to the midpoint.
    const auto hit=[&](unsigned i) {return fractions[i]>=0 && fractions[i]<=1;};
    if(!hit(1)) return false;
    const unsigned first=hit(0)?0:1, last=hit(2)?2:1;
    if(first==last || Dot(normals[first],normals[last])<.4 ||
       Dot(normals[first],normals[1])<.4 || Dot(normals[last],normals[1])<.4) return false;
    auto point=[&](unsigned i) {
        auto p=samples[i][0];
        for(unsigned axis=0;axis<3;++axis) p[axis]+=fractions[i]*(samples[i][1][axis]-p[axis]);
        return p;
    };
    cut.from=point(first);cut.to=point(last);
    const auto span=Delta(cut.to,cut.from);
    if(!(Dot(span,span)<=.15*.15)) return false;
    for(unsigned axis=0;axis<3;++axis) cut.normal[axis]=normals[first][axis]+normals[last][axis];
    return true;
}
bool MakeVolume(const Cut& cut,Volume& volume) {
    Point tangent=Delta(cut.to,cut.from),normal=cut.normal;
    const double length=std::sqrt(Dot(tangent,tangent));
    if(!(length>.001 && length<=1.5 && cut.halfWidth>0 && cut.halfWidth<=.1 &&
         cut.depth>0 && cut.depth<=1.5 && cut.outside>=0 && cut.outside<=.2) ||
       !Normalize(tangent) || !Normalize(normal)) return false;
    Point side=Cross(normal,tangent);
    if(!Normalize(side)) return false;
    normal=Cross(tangent,side);
    for(unsigned axis=0;axis<3;++axis) {
        volume.center[axis]=(cut.from[axis]+cut.to[axis])*.5+normal[axis]*(cut.outside-cut.depth)*.5;
        if(!std::isfinite(volume.center[axis])) return false;
    }
    volume.axes={tangent,side,normal};
    volume.extent={length*.5,cut.halfWidth,(cut.depth+cut.outside)*.5};
    volume.outside=cut.outside;
    return true;
}
bool Subtract(const Triangle& positions,const Volume& volume,std::vector<Triangle>& faces,std::vector<Triangle>* covered,bool diamond) {
    faces.clear();
    if(covered) covered->clear();
    std::array<Point,10> distances;
    const unsigned planeCount=diamond?10:6;
    for(unsigned plane=0;plane<6;++plane) {
        const unsigned axis=plane/2;
        for(unsigned vertex=0;vertex<3;++vertex) {
            const double coord=Dot(Delta(positions[vertex],volume.center),volume.axes[axis]);
            if(!std::isfinite(coord)) return false;
            distances[plane][vertex]=volume.extent[axis]+(plane%2?coord:-coord);
        }
        if(*std::max_element(distances[plane].begin(),distances[plane].end())<=0) return false;
    }
    for(unsigned plane=6;plane<planeCount;++plane) {
        for(unsigned v=0;v<3;++v) {
            const auto rel=Delta(positions[v],volume.center);
            const double x=Dot(rel,volume.axes[0])/volume.extent[0], y=Dot(rel,volume.axes[1])/volume.extent[1];
            distances[plane][v]=1.-((plane-6)&1?x:-x)-((plane-6)&2?y:-y);
        }
    }
    Polygon inside;
    inside.Add({1,0,0});inside.Add({0,1,0});inside.Add({0,0,1});
    std::array<Polygon,10> outside{};
    for(unsigned plane=0;plane<planeCount;++plane) {
        Polygon next;
        const auto& distance=distances[plane];
        for(unsigned index=0;index<inside.count;++index) {
            const auto& first=inside.vertices[index];
            const auto& second=inside.vertices[(index+1)%inside.count];
            const double from=Dot(first,distance),to=Dot(second,distance);
            if(from>=0) next.Add(first);
            if(from<=0) outside[plane].Add(first);
            if((from<0 && to>0)||(from>0 && to<0)) {
                const double ratio=from/(from-to);
                Point edge;
                for(unsigned axis=0;axis<3;++axis) edge[axis]=first[axis]+ratio*(second[axis]-first[axis]);
                next.Add(edge);outside[plane].Add(edge);
            }
        }
        if(next.count<3) return false;
        inside=next;
    }
    auto area=[](const Point& first,const Point& second,const Point& third) {
        return (second[1]-first[1])*(third[2]-first[2])-(second[2]-first[2])*(third[1]-first[1]);
    };
    double removed=0;
    for(unsigned index=1;index+1<inside.count;++index) removed+=area(inside.vertices[0],inside.vertices[index],inside.vertices[index+1]);
    if(!(removed>1e-9)) return false;
    if(covered) for(unsigned i=1;i+1<inside.count;++i)
        if(area(inside.vertices[0],inside.vertices[i],inside.vertices[i+1])>1e-10)
            covered->push_back({inside.vertices[0],inside.vertices[i],inside.vertices[i+1]});
    for(const auto& polygon:outside)
        for(unsigned index=1;index+1<polygon.count;++index)
            if(area(polygon.vertices[0],polygon.vertices[index],polygon.vertices[index+1])>1e-10)
                faces.push_back({polygon.vertices[0],polygon.vertices[index],polygon.vertices[index+1]});
    return true;
}
bool MarkFace(const Triangle& face,const Volume& footprint,bool openSeam,
              std::vector<Triangle>& outside,std::vector<Triangle>& ink,bool diamond) {
    if(!Subtract(face,footprint,outside,&ink,diamond)) return false;
    if(!openSeam) return true;
    Volume slit=footprint;
    slit.extent[1]*=.40;
    auto interpolate=[](const Triangle& source,const Point& weights) {
        Point p{};
        for(unsigned i=0;i<3;++i) for(unsigned j=0;j<3;++j) p[j]+=weights[i]*source[i][j];
        return p;
    };
    std::vector<Triangle> covered,remaining;
    covered.swap(ink);
    for(const auto& polygon:covered) {
        Triangle positions;
        for(unsigned i=0;i<3;++i) positions[i]=interpolate(face,polygon[i]);
        if(!Subtract(positions,slit,remaining)) {ink.push_back(polygon);continue;}
        for(const auto& piece:remaining)
            ink.push_back({interpolate(polygon,piece[0]),interpolate(polygon,piece[1]),interpolate(polygon,piece[2])});
    }
    return true;
}
double SegmentDistance(const Volume& first,const Volume& second) {
    const auto difference=Delta(first.center,second.center);
    return std::sqrt(Dot(difference,difference));
}
bool ProjectSurface(const Point& point,const Point& normal,const Triangle& face,double& distance,Point& weights) {
    const auto first=Delta(face[1],face[0]),second=Delta(face[2],face[0]);
    const auto perpendicular=Cross(normal,second);
    const double determinant=Dot(first,perpendicular);
    if(std::abs(determinant)<1e-11) return false;
    const auto delta=Delta(point,face[0]);const auto cross=Cross(delta,first);
    const double u=Dot(delta,perpendicular)/determinant,v=Dot(normal,cross)/determinant;
    if(!(u>=-1e-7 && v>=-1e-7 && u+v<=1.0000001)) return false;
    const double travel=Dot(second,cross)/determinant;
    if(!(travel>=-.12 && travel<=.02) || std::abs(travel)>=std::abs(distance)) return false;
    distance=travel;weights={1.-u-v,u,v};return true;
}
bool SegmentHit(const Point& from,const Point& to,const Triangle& face,double& fraction,Point& normal) {
    const auto ray=Delta(to,from),left=Delta(face[1],face[0]),right=Delta(face[2],face[0]);
    const auto side=Cross(ray,right);
    const double det=Dot(left,side);
    if(std::abs(det)<1e-12) return false;
    const auto offset=Delta(from,face[0]),cross=Cross(offset,left);
    const double u=Dot(offset,side)/det,v=Dot(ray,cross)/det;
    if(!(u>=-1e-8 && v>=-1e-8 && u+v<=1.00000001)) return false;
    const double t=Dot(right,cross)/det;
    if(!(t>=0 && t<=1 && t<fraction)) return false;
    auto candidate=Cross(left,right);
    if(!Normalize(candidate)) return false;
    if(Dot(candidate,ray)>0) for(auto& value:candidate) value=-value;
    fraction=t;normal=candidate;return true;
}

bool ClosestSurfacePoint(const Triangle& face,const Point& seed,Point& point,Point& normal) {
    normal=Cross(Delta(face[1],face[0]),Delta(face[2],face[0]));
    if(!Normalize(normal)) return false;
    const double distance=Dot(Delta(seed,face[0]),normal);
    for(unsigned k=0;k<3;++k) point[k]=seed[k]-distance*normal[k];
    bool inside=true;
    Point closest{};
    double best=std::numeric_limits<double>::infinity();
    for(unsigned i=0;i<3;++i) {
        const auto edge=Delta(face[(i+1)%3],face[i]),relative=Delta(point,face[i]);
        if(Dot(Cross(edge,relative),normal)<0) inside=false;
        const double t=std::clamp(Dot(relative,edge)/Dot(edge,edge),0.,1.);
        Point candidate=face[i];
        for(unsigned k=0;k<3;++k) candidate[k]+=t*edge[k];
        const auto diff=Delta(candidate,point);
        const auto gap=Dot(diff,diff);
        if(gap<best) {best=gap;closest=candidate;}
    }
    if(!inside) point=closest;
    return true;
}

void LocateStamp(const SweepSamples& rays,const Point& seed,const Point& contactNormal,const Triangle& face,SurfaceHit& hit,
                 double shellDepth,const Point* shadingNormal,const Point* blendWeights) {
    constexpr double margin=.015;
    bool nearSweep=true;
    for(unsigned k=0;k<3;++k) {
        const double low=std::min({face[0][k],face[1][k],face[2][k]}),high=std::max({face[0][k],face[1][k],face[2][k]});
        if(low>seed[k]+.16 || high<seed[k]-.16) return;
        if(low>std::max({rays[0][0][k],rays[0][1][k],rays[2][0][k],rays[2][1][k]})+margin ||
           high<std::min({rays[0][0][k],rays[0][1][k],rays[2][0][k],rays[2][1][k]})-margin) nearSweep=false;
    }
    if(blendWeights && !nearSweep) return;
    const auto edge1=Delta(face[1],face[0]),edge2=Delta(face[2],face[0]);
    const Point area=Cross(edge1,edge2);
    Point normal=area;
    if(!Normalize(normal)) return;
    if(shadingNormal ? Dot(normal,*shadingNormal)<0 : Dot(normal,Delta(rays[2][1],rays[2][0]))>0)
        for(auto& n:normal) n=-n;
    auto consider=[&](const Point& p,bool exact,bool onSegment=true) {
        const auto diff=Delta(p,seed);const auto d=Dot(diff,diff);
        if(d>.16*.16 || (hit.exact && !exact) || (hit.onSegment && !onSegment) ||
           (hit.found && hit.exact==exact && hit.onSegment==onSegment && d>=hit.seedDistanceSquared)) return;
        if(blendWeights) {
            // Use the skin influence at the intersection. A vertex on the
            // far edge of the same triangle can belong entirely to another bone.
            const auto rel=Delta(p,face[0]);
            const auto invArea=1./Dot(area,area);
            const double u=Dot(Cross(rel,edge2),area)*invArea,v=Dot(Cross(edge1,rel),area)*invArea;
            if(Dot({1.-u-v,u,v},*blendWeights)<1.) return;
        }
        hit={p,normal,d,true,exact,onSegment};
    };
    auto lerp=[](const Ray& ray,double t){
        Point p;for(unsigned k=0;k<3;++k)p[k]=ray[0][k]+t*(ray[1][k]-ray[0][k]);return p;
    };
    auto intersect=[&](const Ray& ray,const Triangle& tri) {
        double t=2;Point ignored;
        if(SegmentHit(ray[0],ray[1],tri,t,ignored)) consider(lerp(ray,t),true);
    };
    // The native entry sometimes lies on an inflated collision shell. Keep
    // its tangent coordinates when projecting onto a facing triangle of the
    // same animated region. Any confirmed blade intersection takes priority.
    if(!blendWeights && !hit.onSegment && Dot(contactNormal,normal)>.4) {
        double depth=1.;Point weights;
        if(ProjectSurface(seed,contactNormal,face,depth,weights) && (shellDepth<=0.0 || -depth<shellDepth)) {
            Point surface=seed;for(unsigned k=0;k<3;++k) surface[k]+=contactNormal[k]*depth;
            consider(surface,false,false);
        } else if(shellDepth>0.0) {
            // A seam or an appearance mask can end the face just beside the
            // shell ray. Admit only its actual near-side edge within the graze margin.
            Point point,unused;
            if(ClosestSurfacePoint(face,seed,point,unused)) {
                const auto delta=Delta(point,seed);
                const double along=Dot(delta,contactNormal);
                if(along>=-.12 && along<=.02 && -along<shellDepth &&
                   Dot(delta,delta)-along*along<=margin*margin) consider(point,false,false);
            }
        }
    }
    if(!nearSweep) return;
    for(const auto& ray:rays) intersect(ray,face);
    // Mesh edges can cross the motion between sampled blade segments.
    const Triangle sweep[]{ {rays[0][0],rays[0][1],rays[2][1]}, {rays[0][0],rays[2][1],rays[2][0]} };
    for(const auto& tri:sweep) for(unsigned v=0;v<3;++v) {
        intersect({face[v],face[(v+1)%3]},tri);
        intersect({tri[v],tri[(v+1)%3]},face);
    }
    if(blendWeights || hit.exact) return;
    // A shallow hit on the native collision shell can precede contact with
    // the render surface. Project a point of the finite segment by at most 15 mm.
    for(const auto& ray:rays) {
        const auto dir=Delta(ray[1],ray[0]);const double length2=Dot(dir,dir);
        if(length2<1e-12) continue;
        const auto p=lerp(ray,std::clamp(Dot(Delta(seed,ray[0]),dir)/length2,0.,1.));
        double gap=margin;Point weights;
        if(ProjectSurface(p,normal,face,gap,weights)) {
            Point surface=p;for(unsigned k=0;k<3;++k)surface[k]+=gap*normal[k];
            consider(surface,false);
        }
    }
}
}
