#include "Render/HiddenArea.hpp"
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <string>
#include <vector>

using namespace cvr::hidden_area;
static int failures=0;
#define CHECK(x) do{if(!(x)){std::printf("FAIL %s:%d %s\n",__FILE__,__LINE__,#x);++failures;}}while(0)

static void Settings_() {
    Settings s;
    CHECK(ParseSetting("xr_hidden_area=1",s) && s.enabled==1);
    CHECK(ParseSetting("xr_hidden_area_radius = 120.5",s) && std::fabs(s.radius-120.5f)<1e-4f);
    CHECK(!ParseSetting("xr_hidden_areas=1",s));
    CHECK(Normalize({1,10}).radius==MinRadius && Normalize({1,1000}).radius==MaxRadius);
    CHECK(Normalize({1,NAN}).radius==Settings{}.radius && Normalize({7,100}).enabled==1);
    char buffer[256]{};
    FILE* f=nullptr;tmpfile_s(&f);CHECK(f!=nullptr);if(!f)return;
    WriteSettings(f,{1,99.94f});std::rewind(f);
    Settings back;char line[128];
    while(std::fgets(line,sizeof line,f))ParseSetting(line,back);
    std::fclose(f);(void)buffer;
    CHECK(back.enabled==1 && std::fabs(back.radius-99.9f)<1e-3f);
}

static void HiddenTest() {
    const Fov square{-1,1,1,-1};
    CHECK(!Hidden(square,100,0,0));
    CHECK(Hidden(square,100,.9f,.9f));        // corner beyond the inscribed ellipse
    CHECK(!Hidden(square,142,.99f,.99f));      // radius above sqrt(2) keeps the whole panel
    CHECK(Hidden(square,142,1.01f,0));         // outside the panel is always hidden
    const Fov asym{-1.376f,.839f,.966f,-1.428f};
    CHECK(!Hidden(asym,100,-1.37f,0) && !Hidden(asym,100,.83f,0) && Hidden(asym,100,.85f,0));
}

// Every pixel centre inside a rectangle must be hidden, rectangles stay inside the target and never
// overlap, so the clear can only ever touch masked pixels.
static void Exact(const std::vector<Rect>& rects,const Fov& f,float radius,uint32_t w,uint32_t h,float tx,float ty) {
    std::vector<uint8_t> used(size_t(w)*h);
    for(const auto& r:rects) {
        CHECK(r.left>=0 && r.top>=0 && r.right<=int32_t(w) && r.bottom<=int32_t(h) && r.left<r.right && r.top<r.bottom);
        for(int32_t y=r.top;y<r.bottom;++y)for(int32_t x=r.left;x<r.right;++x) {
            const float px=x+.5f,py=y+.5f;
            if(!Hidden(f,radius,(2*px/w-1)*tx,(1-2*py/h)*ty)){CHECK(!"visible pixel inside a rectangle");return;}
            if(used[size_t(y)*w+x]++){CHECK(!"overlapping rectangles");return;}
        }
    }
}

static void Rects() {
    const Fov square{-1,1,1,-1};
    auto r=BuildRects(square,100,256,256,1,1,8,0);
    const double c=Coverage(r,256,256);
    CHECK(c>.15 && c<.215);                   // inscribed ellipse hides 21.5%, minus partial cells
    Exact(r,square,100,256,256,1,1);
    const auto eroded=BuildRects(square,100,256,256,1,1,8,1);
    CHECK(Coverage(eroded,256,256)<c);
    Exact(eroded,square,100,256,256,1,1);
    CHECK(BuildRects(square,145,256,256,1,1,8,1).empty());
    // A canted panel inside a symmetric covering frustum: everything outside the panel goes.
    const Fov asym{-1.376f,.839f,.966f,-1.428f};
    const float t=1.428f;
    auto outside=BuildRects(asym,145,512,512,t,t,8,0);
    const double expected=1-(2.215*2.394)/(2.856*2.856);
    CHECK(std::fabs(Coverage(outside,512,512)-expected)<.03);
    Exact(outside,asym,145,512,512,t,t);
    // Fully hidden rows merge into one rectangle per run.
    CHECK(outside.size()<20);
    CHECK(BuildRects(square,100,0,256,1,1,8,1).empty() && BuildRects(square,100,256,256,0,1,8,1).empty());
    // The live Pico 4 case (DLSS input 1485 x 1485, 104 deg panel) across the whole slider.
    const float t52=std::tan(52*3.14159265f/180);
    const Fov pico{-t52,t52,t52,-t52};
    for(float radius=MinRadius;radius<=MaxRadius;radius+=5) {
        const auto live=BuildRects(pico,radius,1485,1485,t52,t52,8,1);
        Exact(live,pico,radius,1485,1485,t52,t52);
        std::printf("radius %.0f: %zu rects, %.1f%% hidden\n",radius,live.size(),100*Coverage(live,1485,1485));
    }
}

// The asymmetric projection renders the eye's own window: Quest 3, measured -54/+40 deg x +44/-55 deg
// (right eye: the mirror). Rectangles over that window, and the centred ellipse the patched shaders
// get for it, must never take a visible pixel.
static void Window() {
    const Fov eye{-.839f,1.376f,.965f,-1.390f};
    const Fov window=eye;                      // horizontally the window IS the panel; vertically too here
    // Same rectangles as the symmetric overload when the window is centred.
    const Fov square{-1,1,1,-1};
    const auto a=BuildRects(square,100,256,256,1,1,8,1),b=BuildRects(square,100,256,256,square,8,1);
    CHECK(a.size()==b.size());
    for(size_t i=0;i<a.size() && i<b.size();++i)CHECK(a[i].left==b[i].left && a[i].top==b[i].top && a[i].right==b[i].right && a[i].bottom==b[i].bottom);
    for(float radius:{100.0f,105.0f,115.0f}) {
        const uint32_t w=640,h=680;
        const auto rects=BuildRects(eye,radius,w,h,window,8,1);
        std::vector<uint8_t> used(size_t(w)*h);
        for(const auto& r:rects)for(int32_t y=r.top;y<r.bottom;++y)for(int32_t x=r.left;x<r.right;++x) {
            const float tx=window.left+(x+.5f)/w*(window.right-window.left),ty=window.up-(y+.5f)/h*(window.up-window.down);
            if(!Hidden(eye,radius,tx,ty)){CHECK(!"visible pixel inside an off-centre rectangle");goto next;}
        }
        next:
        CHECK(!rects.empty() || radius>=115);
        float s[4];
        CHECK(ShaderEllipseScales(eye,radius,window,s));
        // Every visible pixel inside the centred NDC ellipse (left, right, up, down scales).
        double skipped=0;
        for(uint32_t y=0;y<h;++y)for(uint32_t x=0;x<w;++x) {
            const float nx=2*(x+.5f)/w-1,ny=1-2*(y+.5f)/h;
            const float tx=window.left+(nx+1)*.5f*(window.right-window.left),ty=window.down+(ny+1)*.5f*(window.up-window.down);
            const float e=s[nx<0?0:1]*nx*nx+s[ny>0?2:3]*ny*ny;
            if(e>1){skipped+=1;if(!Hidden(eye,radius,tx,ty)){CHECK(!"shader ellipse skips a visible pixel");x=w;y=h;}}
        }
        std::printf("off-centre window, radius %.0f: %zu rects %.1f%%, shader skips %.1f%%\n",radius,rects.size(),
                    100*Coverage(rects,w,h),100*skipped/(double(w)*h));
    }
}

// THE TRACE: every output of the geometry over a fixed sweep, hashed (FNV-1a over the bits), so the C++ build and the
// Rust build can be compared bit for bit (cmake/ParityTrace.cmake).
struct TraceHash {
    uint64_t value=1469598103934665603ull;
    void Bytes(const void* p,size_t n) {
        const auto* b=static_cast<const unsigned char*>(p);
        for(size_t i=0;i<n;++i){value^=b[i];value*=1099511628211ull;}
    }
};
static int Trace() {
    uint32_t seed=12345;const auto next=[&]{seed=seed*1664525u+1013904223u;return seed;};
    const auto unit=[&]{return float(next()>>8)/float(1u<<24);};
    size_t rects=0,scaleCalls=0;
    TraceHash hr,hc,hs,hh,hy,hn;
    for(int i=0;i<400;++i) {
        const Fov f{-(0.6f+unit()*0.9f),0.6f+unit()*0.9f,0.6f+unit()*0.9f,-(0.6f+unit()*0.9f)};
        const float radius=60+unit()*100;
        const Fov rendered{f.left*(0.8f+unit()*0.4f),f.right*(0.8f+unit()*0.4f),f.up*(0.8f+unit()*0.4f),f.down*(0.8f+unit()*0.4f)};
        const uint32_t w=64+next()%900,h=64+next()%900,cell=1u<<(2+next()%4),margin=next()%3;
        const auto r=BuildRects(f,radius,w,h,rendered,cell,margin);
        rects+=r.size();
        for(const auto& x:r)hr.Bytes(&x,sizeof x);
        const double cov=Coverage(r,w,h);hc.Bytes(&cov,sizeof cov);
        float scales[4]{};const bool ok=ShaderEllipseScales(f,radius,rendered,scales);
        hs.Bytes(&ok,1);if(ok){hs.Bytes(scales,sizeof scales);++scaleCalls;}
        for(int k=0;k<32;++k){const float x=(unit()*2-1)*2,y=(unit()*2-1)*2;const bool hid=Hidden(f,radius,x,y);hh.Bytes(&hid,1);}
        const float tx=0.5f+unit(),ty=0.5f+unit();
        const auto sym=BuildRects(f,radius,w,h,tx,ty,cell,margin);
        for(const auto& x:sym)hy.Bytes(&x,sizeof x);
        const int enabled=int(next()%3);const float rr=unit()*200;
        Settings s{enabled,rr};const auto n=Normalize(s);hn.Bytes(&n,sizeof n);
    }
    std::printf("hidden_area trace rects %016llx coverage %016llx scales %016llx hidden %016llx symmetric %016llx settings %016llx (rects=%zu scales=%zu)\n",
        (unsigned long long)hr.value,(unsigned long long)hc.value,(unsigned long long)hs.value,(unsigned long long)hh.value,
        (unsigned long long)hy.value,(unsigned long long)hn.value,rects,scaleCalls);
    return 0;
}

int main(int argc,char** argv) {
    if(argc>1 && std::string(argv[1])=="trace")return Trace();
    Settings_();HiddenTest();Rects();Window();
    std::printf(failures?"%d failures\n":"all passed\n",failures);
    return failures?1:0;
}
