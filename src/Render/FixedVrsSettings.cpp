#include "Render/FixedVrs.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::vrs {
Settings Normalize(Settings s) {
    const auto percent=[](float x,float fallback){return std::isfinite(x)?std::clamp(x,20.f,100.f):fallback;};
    s.mode=std::clamp(s.mode,0,2);s.width=percent(s.width,70);s.height=percent(s.height,65);
    const float x=(100-s.width)*.5f,y=(100-s.height)*.5f;
    s.offsetX=std::clamp(std::isfinite(s.offsetX)?s.offsetX:0.f,-x,x);
    s.offsetY=std::clamp(std::isfinite(s.offsetY)?s.offsetY:0.f,-y,y);
    return s;
}
bool ParseSetting(const char* line,Settings& s) {
    int eyeTracked{};
    if(sscanf_s(line,"xr_vrs_eye_tracked = %d",&eyeTracked)==1){s.eyeTracked=eyeTracked!=0;return true;}
    if(sscanf_s(line,"xr_vrs = %d",&s.mode)==1)return true;
    if(sscanf_s(line,"xr_vrs_width = %f",&s.width)==1)return true;
    if(sscanf_s(line,"xr_vrs_height = %f",&s.height)==1)return true;
    if(sscanf_s(line,"xr_vrs_offset_x = %f",&s.offsetX)==1)return true;
    return sscanf_s(line,"xr_vrs_offset_y = %f",&s.offsetY)==1;
}
void WriteSettings(FILE* file,Settings s) {
    s=Normalize(s);
    std::fprintf(file,"xr_vrs=%d\nxr_vrs_width=%.2f\nxr_vrs_height=%.2f\nxr_vrs_offset_x=%.2f\nxr_vrs_offset_y=%.2f\n",
        s.mode,s.width,s.height,s.offsetX,s.offsetY);
    std::fprintf(file,"xr_vrs_eye_tracked=%d\n",int(s.eyeTracked));
}
Settings GazeRegion(Settings s,float x,float y) {
    s=Normalize(s);
    // Cache immutable maps on a 1/16-image grid. Enlarge the fine region by the
    // maximum rounding error on BOTH edges so quantization cannot blur the gaze.
    constexpr float step=6.25f;
    s.width=std::min(100.f,s.width+step);s.height=std::min(100.f,s.height+step);
    s.offsetX=std::round((x-.5f)*100/step)*step;
    s.offsetY=std::round((y-.5f)*100/step)*step;
    return Normalize(s);
}
std::vector<uint8_t> BuildRates(Settings s,unsigned w,unsigned h,unsigned tile,bool additional) {
    if(!w || !h || w>16384 || h>16384 || tile<8 || tile>32 || (tile&(tile-1)))return {};
    s=Normalize(s);const unsigned columns=(w+tile-1)/tile,rows=(h+tile-1)/tile;
    std::vector<uint8_t> result(size_t(columns)*rows,D3D12_SHADING_RATE_1X1);
    if(!s.mode)return result;
    const float left=w*(.5f+(s.offsetX-s.width*.5f)/100),right=left+w*s.width/100;
    const float top=h*(.5f+(s.offsetY-s.height*.5f)/100),bottom=top+h*s.height/100;
    // Every tile intersecting the center is fine. A coarse tile never straddles it.
    for(unsigned y=0;y<rows;++y)for(unsigned x=0;x<columns;++x) {
        const float x0=float(x*tile),y0=float(y*tile),x1=float(std::min(w,(x+1)*tile)),y1=float(std::min(h,(y+1)*tile));
        const auto intersects=[&](float marginX,float marginY){return x1>left-marginX && x0<right+marginX && y1>top-marginY && y0<bottom+marginY;};
        if(intersects(0,0))continue;
        result[size_t(y)*columns+x]=uint8_t(s.mode==2 && additional && !intersects(w*.08f,h*.08f)?D3D12_SHADING_RATE_4X4:D3D12_SHADING_RATE_2X2);
    }
    return result;
}
std::vector<uint8_t> BuildStereoRates(const std::array<Settings,2>& regions,unsigned w,unsigned h,unsigned tile,bool additional) {
    if(w%2)return {};
    // Sample each half on the full target's tile grid, including non-aligned seams.
    auto result=BuildRates({},w,h,tile,additional);if(result.empty())return result;
    const unsigned columns=(w+tile-1)/tile,rows=(h+tile-1)/tile,half=w/2;
    for(unsigned y=0;y<rows;++y)for(unsigned x=0;x<columns;++x) {
        uint8_t rate=D3D12_SHADING_RATE_4X4;
        for(unsigned eye=0;eye<2;++eye) {
            const float x0=float(std::max(x*tile,eye*half))-eye*half;
            const float x1=float(std::min((x+1)*tile,(eye+1)*half))-eye*half;
            if(x1<=x0)continue;
            const auto s=Normalize(regions[eye]);
            const float l=half*(.5f+(s.offsetX-s.width*.5f)/100),r=l+half*s.width/100;
            const float t=h*(.5f+(s.offsetY-s.height*.5f)/100),b=t+h*s.height/100;
            const auto hits=[&](float mx,float my){return x1>l-mx && x0<r+mx && y*tile<b+my && (y+1)*tile>t-my;};
            const auto eyeRate=!s.mode || hits(0,0)?D3D12_SHADING_RATE_1X1:
                s.mode==2 && additional && !hits(half*.08f,h*.08f)?D3D12_SHADING_RATE_4X4:D3D12_SHADING_RATE_2X2;
            rate=std::min(rate,uint8_t(eyeRate));
        }
        result[size_t(y)*columns+x]=rate;
    }
    return result;
}
bool EligiblePipeline(const D3D12_GRAPHICS_PIPELINE_STATE_DESC& d) {
    if(d.NumRenderTargets<3 || d.NumRenderTargets>4 || !d.PS.BytecodeLength || !d.VS.BytecodeLength ||
       !d.DepthStencilState.DepthEnable || d.SampleDesc.Count!=1 || d.SampleDesc.Quality ||
       d.BlendState.AlphaToCoverageEnable || d.RasterizerState.ForcedSampleCount || d.GS.BytecodeLength || d.HS.BytecodeLength || d.DS.BytecodeLength)return false;
    for(UINT i=0;i<d.NumRenderTargets;++i) {
        const auto& b=d.BlendState.RenderTarget[d.BlendState.IndependentBlendEnable?i:0];
        if(b.BlendEnable || b.LogicOpEnable)return false;
    }
    return true;
}
}
