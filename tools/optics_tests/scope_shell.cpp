// Produce a VR copy of the native housing, opening only the lens channel.
// The packed vertex streams and all authored appearances stay byte-identical.
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstring>
#include <fstream>
#include <iostream>
#include <iterator>
#include <stdexcept>
#include <vector>

struct Point { double x,z,y; };
double cross(Point a,Point b,Point c) {
    return (b.x-a.x)*(c.z-a.z)-(b.z-a.z)*(c.x-a.x);
}
std::vector<Point> hull(std::vector<Point> points) {
    std::sort(points.begin(),points.end(),[](Point a,Point b){return a.x!=b.x ? a.x<b.x : a.z<b.z;});
    points.erase(std::unique(points.begin(),points.end(),[](Point a,Point b){return a.x==b.x && a.z==b.z;}),points.end());
    std::vector<Point> result;
    auto append=[&](Point p,size_t start){
        while(result.size()>start+1 && cross(result[result.size()-2],result.back(),p)<=0) result.pop_back();
        result.push_back(p);
    };
    for(auto p:points) append(p,0);
    const auto start=result.size()-1;
    for(auto it=points.rbegin()+1;it!=points.rend();++it) append(*it,start);
    result.pop_back();
    if(result.size()<3) throw std::runtime_error("degenerate aperture");
    return result;
}
struct Mesh {
    std::vector<char> bytes;
    double scale[3]{},origin[3]{};
    size_t positions{},stride{},count{},indices{},indexCount{};
    Mesh(const char* file):bytes{} {
        std::ifstream f(file,std::ios::binary);
        if(!f) throw std::runtime_error("missing vertex/index buffer");
        bytes.assign(std::istreambuf_iterator<char>(f),{});
        for(auto& v:scale) std::cin>>v;
        for(auto& v:origin) std::cin>>v;
    }
    void chunk() {
        if(!(std::cin>>positions>>stride>>count>>indices>>indexCount) || count<3 || stride<6 ||
           positions+(count-1)*stride+6>bytes.size() || indices+indexCount*2>bytes.size() || indexCount%3)
            throw std::runtime_error("invalid render chunk");
    }
    Point point(size_t i) const {
        if(i>=count) throw std::runtime_error("invalid vertex index");
        int16_t xyz[3];
        std::memcpy(xyz,bytes.data()+positions+i*stride,sizeof(xyz));
        return {xyz[0]*scale[0]/32767.+origin[0],xyz[2]*scale[2]/32767.+origin[2],xyz[1]*scale[1]/32767.+origin[1]};
    }
};
bool separated(const Point* polygon,size_t count,Point a,Point b) {
    for(size_t i=0;i<count;i++) if(cross(a,b,polygon[i])>=-1.e-11) return false;
    return true;
}
bool blocks(const std::vector<Point>& aperture,Point tri[3]) {
    // An end face at the far end of the tube is visible inside the entrance
    // pupil even where its orthographic projection falls outside the pupil.
    // Keep its sidewall, omit the closure and its bevel from the VR copy.
    const double ax=tri[1].x-tri[0].x,ay=tri[1].y-tri[0].y,az=tri[1].z-tri[0].z;
    const double bx=tri[2].x-tri[0].x,by=tri[2].y-tri[0].y,bz=tri[2].z-tri[0].z;
    const double nx=ay*bz-az*by,ny=az*bx-ax*bz,nz=ax*by-ay*bx;
    if(ny*ny>.10*(nx*nx+nz*nz)) return true;
    for(size_t i=0;i<aperture.size();++i)
        if(separated(tri,3,aperture[i],aperture[(i+1)%aperture.size()])) return false;
    if(cross(tri[0],tri[1],tri[2])<0) std::swap(tri[1],tri[2]);
    for(int i=0;i<3;++i)
        if(separated(aperture.data(),aperture.size(),tri[i],tri[(i+1)%3])) return false;
    return true;
}
int main(int argc,char** argv) try {
    if(argc!=4) throw std::runtime_error("expected aperture, housing and output buffers");
    Mesh lens(argv[1]);lens.chunk();
    std::vector<Point> points;
    for(size_t i=0;i<lens.count;++i) points.push_back(lens.point(i));
    const auto aperture=hull(points);
    Mesh shell(argv[2]);
    size_t chunks{},removed{},total{};
    if(!(std::cin>>chunks) || !chunks) throw std::runtime_error("missing housing chunks");
    while(chunks--) {
        shell.chunk();
        for(size_t i=0;i<shell.indexCount;i+=3) {
            auto ptr=shell.bytes.data()+shell.indices+2*i;
            uint16_t ix[3];
            std::memcpy(ix,ptr,sizeof(ix));
            Point tri[]{shell.point(ix[0]),shell.point(ix[1]),shell.point(ix[2])};
            if(blocks(aperture,tri)) {
                ix[1]=ix[2]=ix[0];std::memcpy(ptr,ix,sizeof(ix));++removed;
            }
            ++total;
        }
    }
    if(!removed || removed==total) throw std::runtime_error("expected an aperture and exterior geometry");
    std::ofstream out(argv[3],std::ios::binary);
    if(!out.write(shell.bytes.data(),shell.bytes.size())) throw std::runtime_error("write failed");
    std::cout<<removed<<" obstructing faces, "<<total-removed<<" exterior faces retained\n";
    return 0;
} catch(const std::exception& e) { std::cerr<<e.what()<<'\n';return 1; }
