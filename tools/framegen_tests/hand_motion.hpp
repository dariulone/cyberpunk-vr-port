#pragma once
#include <algorithm>
#include <array>
#include <cmath>
#include <climits>
#include <cstdint>
#include <cstdio>
#include <stdexcept>
#include <vector>

namespace hand_motion {
struct Point {int x,y;};
struct Region {int x,y,w,h;};
struct Image {
    int w,h;
    std::vector<uint8_t> pixels;
    int at(Point p)const {return pixels[size_t(p.y)*w+p.x];}
    bool inside(Point p,int margin)const {
        return p.x>=margin && p.y>=margin && p.x<w-margin && p.y<h-margin;
    }
};
using Pyramid=std::array<Image,4>;
inline Pyramid Luma(const std::vector<uint8_t>& color,int w,int h) {
    Pyramid pyramid;
    auto& base=pyramid[0];
    base={w,h,std::vector<uint8_t>(size_t(w)*h)};
    for(size_t i=0;i<base.pixels.size();++i) {
        const auto* rgb=&color[4*i];
        base.pixels[i]=uint8_t((77*rgb[0]+150*rgb[1]+29*rgb[2]+128)/256);
    }
    for(size_t level=1;level<pyramid.size();++level) {
        const auto& prev=pyramid[level-1];
        auto& img=pyramid[level];
        img={prev.w/2,prev.h/2,{}};
        img.pixels.resize(size_t(img.w)*img.h);
        for(int y=0;y<img.h;++y)for(int x=0;x<img.w;++x) {
            const size_t i=size_t(2*y)*prev.w+2*x;
            img.pixels[size_t(y)*img.w+x]=uint8_t((prev.pixels[i]+prev.pixels[i+1]+prev.pixels[i+prev.w]+prev.pixels[i+prev.w+1]+2)/4);
        }
    }
    return pyramid;
}
// Exhaustive SAD at 1/8 size, then local refinement down to display pixels.
// No depth or engine vector participates in correspondence selection.
inline bool Track(const Pyramid& from,const Pyramid& to,Point seed,Point& hit) {
    constexpr int patch=4;
    Point delta{};
    for(int level=3;level>=0;--level) {
        const auto& a=from[level];const auto& b=to[level];
        Point p{seed.x>>level,seed.y>>level};
        if(!a.inside(p,patch))return false;
        Point centre{p.x+2*delta.x,p.y+2*delta.y};
        const int radius=level==3?32:4;
        int best=INT_MAX;
        int lo=255,hi=0;
        for(int y=-patch;y<=patch;++y)for(int x=-patch;x<=patch;++x) {
            const int v=a.at({p.x+x,p.y+y});lo=std::min(lo,v);hi=std::max(hi,v);
        }
        if(level==0 && hi-lo<14)return false;
        for(int dy=-radius;dy<=radius;++dy)for(int dx=-radius;dx<=radius;++dx) {
            const Point q{centre.x+dx,centre.y+dy};
            if(!b.inside(q,patch))continue;
            int cost=0;
            for(int yy=-patch;yy<=patch && cost<best;++yy) {
                const auto* pa=&a.pixels[size_t(p.y+yy)*a.w+p.x-patch];
                const auto* pb=&b.pixels[size_t(q.y+yy)*b.w+q.x-patch];
                for(int i=0;i<2*patch+1;++i)cost+=std::abs(int(pa[i])-pb[i]);
            }
            if(cost<best) {best=cost;hit=q;}
        }
        if(best==INT_MAX || (level==0 && best>10*81))return false;
        delta={hit.x-p.x,hit.y-p.y};
    }
    return true;
}
inline double Quantile(std::vector<double> v,double q) {
    const auto i=size_t(q*(v.size()-1));
    std::nth_element(v.begin(),v.begin()+i,v.end());return v[i];
}
struct Correspondence {Point middle,other;};
inline std::vector<Correspondence> Matches(const Pyramid& middle,const Pyramid& other,Region roi) {
    std::vector<Correspondence> result;
    for(int y=roi.y;y<roi.y+roi.h;y+=10)for(int x=roi.x;x<roi.x+roi.w;x+=10) {
        Point p{x,y},q{},back{};
        if(Track(middle,other,p,q) && Track(other,middle,q,back) && std::hypot(back.x-x,back.y-y)<=2)
            result.push_back({p,q});
    }
    return result;
}
template<class Frame>
void Measure(const Frame& current,const Pyramid& middle,const Pyramid& other,Region roi,bool currentIsMiddle) {
    std::vector<double> dx,dy,mx,my,depth,errors,halfX,zero;
    const int tested=((roi.w+9)/10)*((roi.h+9)/10);
    unsigned outliers=0;
    double nearest=INFINITY;
    Point example{},examplePrevious{},motionTexel{};
    for(const auto& [p,q]:Matches(middle,other,roi)) {
        const Point cur=currentIsMiddle?p:q,prev=currentIsMiddle?q:p;
        const double ox=prev.x-cur.x,oy=prev.y-cur.y;
        // Select the jittered render texel corresponding to this unjittered colour pixel.
        const double u=(cur.x+.5)/current.width+double(current.camera.jitter[0])/current.depthWidth;
        const double v=(cur.y+.5)/current.height+double(current.camera.jitter[1])/current.depthHeight;
        auto index=[&](uint32_t w,uint32_t h) {
            return size_t(std::clamp(int(v*h),0,int(h)-1))*w+std::clamp(int(u*w),0,int(w)-1);
        };
        const size_t m=2*index(current.motionWidth,current.motionHeight);
        const double centreDistance=std::hypot(p.x-roi.x-.5*roi.w,p.y-roi.y-.5*roi.h);
        if(centreDistance<nearest) {
            nearest=centreDistance;example=cur;examplePrevious=prev;
            motionTexel={int(m/2%current.motionWidth),int(m/2/current.motionWidth)};
        }
        const double vx=current.motion[m]*current.camera.motionScale[0]*current.width;
        const double vy=current.motion[m+1]*current.camera.motionScale[1]*current.height;
        const double z=current.depth[index(current.depthWidth,current.depthHeight)];
        if(!std::isfinite(vx) || !std::isfinite(vy) || !std::isfinite(z))throw std::runtime_error("nonfinite engine input");
        const double err=std::hypot(ox-vx,oy-vy);
        dx.push_back(ox);dy.push_back(oy);mx.push_back(vx);my.push_back(vy);depth.push_back(z);
        errors.push_back(err);halfX.push_back(std::hypot(ox-.5*vx,oy-vy));zero.push_back(std::hypot(ox,oy));
        if(err>3)++outliers;
    }
    if(errors.empty())throw std::runtime_error("no round-trip texture matches in middle-frame ROI");
    std::printf("matches %zu/%u; median optical (%.1f,%.1f) engine (%.1f,%.1f) px; device Z %.6f; "
        "EPE median %.2f p90 %.2f px, >3px %.1f%%; half-X %.2f zero %.2f px\n",
        errors.size(),tested,Quantile(dx,.5),Quantile(dy,.5),Quantile(mx,.5),Quantile(my,.5),Quantile(depth,.5),
        Quantile(errors,.5),Quantile(errors,.9),100.*outliers/errors.size(),Quantile(halfX,.5),Quantile(zero,.5));
    std::printf("  tracked colour (%d,%d) -> (%d,%d); current per-eye motion texel (%d,%d)\n",
        example.x,example.y,examplePrevious.x,examplePrevious.y,motionTexel.x,motionTexel.y);
}
// Compare the generated midpoint to corresponding endpoint detail translated
// halfway along each measured track. Evaluate an equal-weight, unwarped endpoint
// blend at exactly the same pixels to quantify doubling.
struct MidpointError {double generated,blend;};
inline MidpointError ScoreMidpoint(const Image& middle,const Image& other,const Image& generated,
                          const std::vector<Correspondence>& pairs) {
    if(pairs.empty())throw std::runtime_error("no midpoint tracks");
    double error=0,blend=0;
    for(const auto& [a,b]:pairs) {
        for(int y=-2;y<=2;++y)for(int x=-2;x<=2;++x) {
            const double truth=.5*(middle.at({a.x+x,a.y+y})+other.at({b.x+x,b.y+y}));
            auto sample=[&](const Image& image) {
                const int xx=a.x+b.x+2*x,yy=a.y+b.y+2*y;
                const int l=xx/2,t=yy/2,r=(xx+1)/2,bot=(yy+1)/2;
                return .25*(image.at({l,t})+image.at({r,t})+image.at({l,bot})+image.at({r,bot}));
            };
            error+=std::abs(sample(generated)-truth);
            blend+=std::abs(.5*(sample(middle)+sample(other))-truth);
        }
    }
    return {error/(25*pairs.size()),blend/(25*pairs.size())};
}
}
