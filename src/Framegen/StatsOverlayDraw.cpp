#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
// The frame generation's statistics panel: a small persistent bitmap in a five-by-seven pixel font (Framegen/
// StatsOverlay.hpp shows it). The C++ build's; the Rust build's is rust/crates/cvr-gpu/src/framegen_overlay.rs, the same
// C ABI, and tools/framegen_overlay_tests compares the two pixel for pixel.
#if !CVR_RUST_VRIK
#include "Framegen/Framegen.hpp"
#include <windows.h>
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstring>
#include <cmath>
#include <string>

namespace cvr::framegen {
namespace {
// Five columns, seven rows; all text is a bounded diagnostic alphabet.
const uint8_t* Glyph(char c) {
    static constexpr uint8_t digits[][5]={{62,81,73,69,62},{0,66,127,64,0},{66,97,81,73,70},{33,65,69,75,49},{24,20,18,127,16},
        {39,69,69,69,57},{60,74,73,73,48},{1,113,9,5,3},{54,73,73,73,54},{6,73,73,41,30}};
    static constexpr uint8_t letters[][5]={{126,17,17,17,126},{127,73,73,73,54},{62,65,65,65,34},{127,65,65,34,28},
        {127,73,73,73,65},{127,9,9,9,1},{62,65,73,73,122},{127,8,8,8,127},{0,65,127,65,0},{32,64,65,63,1},
        {127,8,20,34,65},{127,64,64,64,64},{127,2,12,2,127},{127,4,8,16,127},{62,65,65,65,62},{127,9,9,9,6},
        {62,65,81,33,94},{127,9,25,41,70},{70,73,73,73,49},{1,1,127,1,1},{63,64,64,64,63},{31,32,64,32,31},
        {63,64,56,64,63},{99,20,8,20,99},{7,8,112,8,7},{97,81,73,69,67}};
    static constexpr uint8_t lowercase[][5]={{32,84,84,84,120},{127,72,68,68,56},{56,68,68,68,32},{56,68,68,72,127},
        {56,84,84,84,24},{8,126,9,1,2},{12,82,82,82,62},{127,8,4,4,120},{0,68,125,64,0},{32,64,68,61,0},
        {127,16,40,68,0},{0,65,127,64,0},{124,4,24,4,120},{124,8,4,4,120},{56,68,68,68,56},{124,20,20,20,8},
        {8,20,20,24,124},{124,8,4,4,8},{72,84,84,84,32},{4,63,68,64,32},{60,64,64,32,124},{28,32,64,32,28},
        {60,64,48,64,60},{68,40,16,40,68},{12,80,80,80,60},{68,100,84,76,68}};
    static constexpr uint8_t blank[]={0,0,0,0,0},dot[]={0,96,96,0,0},dash[]={8,8,8,8,8};
    static constexpr uint8_t percent[]={99,19,8,100,99},colon[]={0,54,54,0,0},slash[]={32,16,8,4,2};
    static constexpr uint8_t plus[]={8,8,62,8,8},leftParen[]={0,28,34,65,0},rightParen[]={0,65,34,28,0};
    if(c>='0' && c<='9')return digits[c-'0'];if(c>='A' && c<='Z')return letters[c-'A'];
    if(c>='a' && c<='z')return lowercase[c-'a'];
    if(c=='+')return plus;if(c=='(')return leftParen;if(c==')')return rightParen;
    if(c=='%')return percent;if(c==':')return colon;if(c=='/')return slash;
    return c=='.' ? dot : (c=='-' ? dash : blank);
}
struct Canvas {
    uint32_t* pixels;int width,height;
    void Rect(int x,int y,int w,int h,uint32_t color) {
        for(int row=std::max(0,y);row<std::min(height,y+h);++row)
            if(x<width && x+w>0)std::fill(pixels+row*width+std::max(0,x),pixels+row*width+std::min(width,x+w),color);
    }
    void Text(int x,int y,const char* text,uint32_t color=0xFFF2F2F2u,int scale=2) {
    for(const char* p=text;*p && x+5*scale<=width;++p,x+=6*scale) {
        const auto* glyph=Glyph(*p);
        for(int col=0;col<5;++col)for(int row=0;row<7;++row)if(glyph[col]&(1u<<row))
            for(int dy=0;dy<scale;++dy)for(int dx=0;dx<scale;++dx)
                if(y+row*scale+dy>=0 && y+row*scale+dy<height && x+col*scale+dx>=0)pixels[(y+row*scale+dy)*width+x+col*scale+dx]=color;
    }
    }
    void TextFit(int y,const char* text,int scale=2) {
        if(std::strlen(text)*6*scale>size_t(width-24))scale=1;
        Text(12,y,text,0xFFF2F2F2u,scale);
    }
    void Panel() {
        for(int y=0;y<height;++y)for(int x=0;x<width;++x) {
            const int dx=std::max({12-x,x-(width-13),0}),dy=std::max({12-y,y-(height-13),0});
            const bool corner=dx || dy;const int radius=dx*dx+dy*dy;
            pixels[y*width+x]=radius>144 ? 0 : ((corner && radius>100) || x<2 || y<2 || x>=width-2 || y>=height-2 ? 0xFFA4A4A4u : 0xDA21170Fu);
        }
    }
    void Graph(int x,int y,int w,int h,const std::array<float,GraphPoints>& values,double budget,uint32_t color) {
        Rect(x-1,y-1,w+2,h+2,0xFF686868);Rect(x,y,w,h,0xFF21170F);
        const double ceiling=std::clamp(std::max(33.34,budget*2),20.0,100.0);
        const int line=y+h-1-int(std::clamp(budget/ceiling,0.0,1.0)*(h-1));Rect(x,line,w,1,0xFF666666);
        for(unsigned i=0;i<GraphPoints;++i) {
            const int left=x+int(i*w/GraphPoints),right=x+int((i+1)*w/GraphPoints);
            const int bar=std::clamp(int(values[i]/ceiling*(h-1)),0,h-1);
            Rect(left,y+h-bar-1,std::max(1,right-left),bar+1,values[i]>budget*1.2 ? 0xFF526CFFu : color);
        }
    }
    void Cores(int x,int y,int w,int h,const HardwareStatistics& hardware) {
        if(!hardware.coreCount || hardware.coreMaximum<0) {Text(x,y,"--");return;}
        for(unsigned i=0;i<hardware.coreCount;++i) {
            const int left=x+int(i*w/hardware.coreCount),right=x+int((i+1)*w/hardware.coreCount);
            Rect(left,y,std::max(1,right-left-2),h,0xFF44382B);
            const int used=int(std::clamp(hardware.cores[i],0.0f,100.0f)*h/100);
            Rect(left,y+h-used,std::max(1,right-left-2),used,hardware.cores[i]>90?0xFF526CFFu:0xFF98EEA5u);
        }
    }
};
// BackendName (Framegen.cpp), here so the drawing stands alone for its test.
const char* Name(Backend value) {
    switch(value) {
        case Backend::Nvidia:return "NVIDIA OFA + FidelityFX";
        case Backend::DlssG:return "NVIDIA DLSS-G";
        default:return "FidelityFX";
    }
}
std::string Number(double value,bool known=true,unsigned precision=1) {
    if(!known || value<0 || !std::isfinite(value))return "--";
    char text[32]{};std::snprintf(text,sizeof(text),precision ? "%.1f" : "%.0f",value);return text;
}
void Draw(Canvas c,const Settings& settings,const Statistics& stats,uint32_t hour,uint32_t minute,Backend backend) {
    constexpr double gb=1024.0*1024*1024;const auto& hardware=stats.hardware;
    c.Panel();char row[128]{};
    const auto low1=Number(stats.low1Fps,stats.low1Fps>0),low01=Number(stats.low01Fps,stats.low01Fps>0);
    const auto gpu=Number(hardware.gpuUsage),cpu=Number(hardware.cpuUsage),temperature=Number(hardware.gpuTemperature,true,0);
    const auto ram=Number(hardware.ramUsed/gb,hardware.ramTotal!=0),ramTotal=Number(hardware.ramTotal/gb,hardware.ramTotal!=0);
    const auto vram=Number(hardware.gpuMemoryUsed/gb,hardware.gpuMemoryValid),vramTotal=Number(hardware.gpuMemoryTotal/gb,hardware.gpuMemoryTotal!=0);
    if(settings.overlayLayout==0) {
        std::snprintf(row,sizeof(row),"cyberpunk-vr-port  %.1f MIN",stats.elapsedSeconds/60);c.Text(12,10,row);
        std::snprintf(row,sizeof(row),"%02u:%02u",hour,minute);c.Text(c.width-76,10,row);
        std::snprintf(row,sizeof(row),"REAL %.1f  FG %.1f  OUT %.1f",stats.realFps,stats.generatedFps,stats.outputFps);c.Text(12,36,row);
        std::snprintf(row,sizeof(row),"AVG %.1f FPS   MIN %.1f   MAX %.1f",stats.averageFps,stats.minimumFps,stats.maximumFps);c.TextFit(60,row);
        std::snprintf(row,sizeof(row),"1%% LOW %s  0.1%% LOW %s  %dS",low1.c_str(),low01.c_str(),settings.historySeconds);c.Text(12,84,row);
        std::snprintf(row,sizeof(row),"GPU %.1fMS AVG %.1f",stats.gpuMs,stats.gpuAverageMs);c.Text(12,108,row);
        std::snprintf(row,sizeof(row),"CPU %.1fMS AVG %.1f",stats.cpuMs,stats.cpuAverageMs);c.Text(372,108,row);
        c.Graph(12,130,330,76,stats.gpuGraph,stats.budgetMs,0xFF98EEA5u);c.Graph(372,130,330,76,stats.cpuGraph,stats.budgetMs,0xFF98ECFFu);
        std::snprintf(row,sizeof(row),"P99 %s  P99.9 %s",Number(stats.gpuP99Ms,stats.gpuP99Ms>0).c_str(),Number(stats.gpuP999Ms,stats.gpuP999Ms>0).c_str());c.Text(12,214,row);
        std::snprintf(row,sizeof(row),"P99 %s  P99.9 %s",Number(stats.cpuP99Ms,stats.cpuP99Ms>0).c_str(),Number(stats.cpuP999Ms,stats.cpuP999Ms>0).c_str());c.Text(372,214,row);
        std::snprintf(row,sizeof(row),"GPU %s%%  %sC",gpu.c_str(),temperature.c_str());c.Text(12,238,row);
        std::snprintf(row,sizeof(row),"CPU %s%% CORE MAX %s%%",cpu.c_str(),Number(hardware.coreMaximum,true,0).c_str());c.Text(372,238,row);
        std::snprintf(row,sizeof(row),"VRAM %s/%s GB",vram.c_str(),vramTotal.c_str());c.Text(12,262,row);
        std::snprintf(row,sizeof(row),"RAM %s/%s GB",ram.c_str(),ramTotal.c_str());c.Text(372,262,row);
        std::snprintf(row,sizeof(row),"APP VRAM %s GB",Number(hardware.gpuProcessUsed/gb,hardware.gpuProcessMemoryValid).c_str());c.Text(12,286,row);
        std::snprintf(row,sizeof(row),"APP RAM %.1f GB",hardware.appWorkingSet/gb);c.Text(372,286,row);
        std::snprintf(row,sizeof(row),"CORES %u",hardware.coreCount);c.Text(12,312,row);c.Cores(126,310,576,20,hardware);
        std::snprintf(row,sizeof(row),"FG MEM %.0fM GEN %.1fMS",stats.vramBytes/(1024.0*1024),stats.generationMs);c.Text(12,340,row);
        std::snprintf(row,sizeof(row),"REPEAT %.1f%% SKIP %llu",stats.repeatPercent,stats.skipped);c.Text(372,340,row);
    } else if(settings.overlayLayout==1) {
        std::snprintf(row,sizeof(row),"REAL %.1f FG %.1f OUT %.1f",stats.realFps,stats.generatedFps,stats.outputFps);c.Text(12,10,row);
        std::snprintf(row,sizeof(row),"AVG %.1f MIN %.1f MAX %.1f",stats.averageFps,stats.minimumFps,stats.maximumFps);c.TextFit(34,row);
        std::snprintf(row,sizeof(row),"1%% LOW %s 0.1%% %s %dS",low1.c_str(),low01.c_str(),settings.historySeconds);c.TextFit(58,row);
        std::snprintf(row,sizeof(row),"GPU %s%% %sC %.1fMS",gpu.c_str(),temperature.c_str(),stats.gpuMs);c.Text(12,84,row);
        c.Graph(12,106,396,58,stats.gpuGraph,stats.budgetMs,0xFF98EEA5u);
        std::snprintf(row,sizeof(row),"CPU %s%% %.1fMS",cpu.c_str(),stats.cpuMs);c.Text(12,174,row);
        c.Graph(12,196,396,58,stats.cpuGraph,stats.budgetMs,0xFF98ECFFu);
        std::snprintf(row,sizeof(row),"VRAM %s/%s GB",vram.c_str(),vramTotal.c_str());c.Text(12,266,row);
        std::snprintf(row,sizeof(row),"RAM %s/%s GB",ram.c_str(),ramTotal.c_str());c.Text(12,290,row);
        c.Cores(12,312,396,12,hardware);
    } else {
        std::snprintf(row,sizeof(row),"REAL %.1f FG %.1f",stats.realFps,stats.generatedFps);c.Text(12,10,row);
        std::snprintf(row,sizeof(row),"AVG %.1f MIN %.1f MAX %.1f",stats.averageFps,stats.minimumFps,stats.maximumFps);c.TextFit(34,row);
        std::snprintf(row,sizeof(row),"OUT %.1f FPS",stats.outputFps);c.Text(12,58,row);
        std::snprintf(row,sizeof(row),"1%% %s  0.1%% %s",low1.c_str(),low01.c_str());c.Text(12,82,row);
        std::snprintf(row,sizeof(row),"GPU %s%% CPU %s%%",gpu.c_str(),cpu.c_str());c.Text(12,106,row);
        std::snprintf(row,sizeof(row),"G %.1fMS C %.1fMS",stats.gpuMs,stats.cpuMs);c.Text(12,130,row);
    }
    const bool detailed=settings.overlayLayout==0;
    const int footer=detailed?378:(settings.overlayLayout==1?344:164);
    c.Rect(12,footer-10,c.width-24,1,0xFF686868);
    std::snprintf(row,sizeof(row),"CPU: %s",hardware.cpuName[0]?hardware.cpuName:"--");c.TextFit(footer,row,detailed?2:1);
    std::snprintf(row,sizeof(row),"GPU: %s",hardware.gpuName[0]?hardware.gpuName:"--");c.TextFit(footer+(detailed?24:14),row,detailed?2:1);
    const char* generator=Name(backend);
    std::snprintf(row,sizeof(row),"FG: %s",settings.enabled?generator:"Disabled");c.TextFit(footer+(detailed?48:32),row);
}
}
}  // namespace cvr::framegen

extern "C" void cvr_framegen_overlay_draw(uint32_t* pixels,int width,int height,const cvr::framegen::Settings* settings,
                                          const cvr::framegen::Statistics* stats,uint32_t hour,uint32_t minute,int backend) {
    cvr::framegen::Draw({pixels,width,height},*settings,*stats,hour,minute,cvr::framegen::Backend(backend));
}
#endif
#endif
