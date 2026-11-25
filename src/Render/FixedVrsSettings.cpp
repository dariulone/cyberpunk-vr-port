#include "Render/FixedVrs.hpp"
#include <algorithm>
#include <cmath>

namespace cvr::vrs {
namespace {
void QuantizeRect(float lowX,float highX,float lowY,float highY,unsigned domX0,unsigned domX1,unsigned domY1,unsigned tile,int out[4]) {
    const int minX=int(domX0/tile),maxX=int((domX1+tile-1)/tile);
    const int minY=0,maxY=int((domY1+tile-1)/tile);
    const int fx=lowX>=float(domX1)?maxX:(lowX<float(domX0)?minX:std::max(minX,int(std::floor(lowX/float(tile)))));
    const int ex=highX<=float(domX0)?minX:(highX>=float(domX1)?maxX:std::min(maxX,int(std::ceil(highX/float(tile)))));
    const int fy=lowY>=float(domY1)?maxY:(lowY<0.f?minY:std::max(minY,int(std::floor(lowY/float(tile)))));
    const int ey=highY<=0.f?minY:(highY>=float(domY1)?maxY:std::min(maxY,int(std::ceil(highY/float(tile)))));
    if(fx>=ex || fy>=ey) {
        out[0]=out[1]=out[2]=out[3]=0;
    } else {
        out[0]=fx;out[1]=ex;out[2]=fy;out[3]=ey;
    }
}
EyeTileSignature SigForEye(Settings s,float baseX,float eyeW,unsigned domX0,unsigned domX1,unsigned targetH,unsigned tile,bool additional) {
    s=Normalize(s);
    if(!s.mode)return EyeTileSignature{0,{},{}};
    const int effMode=(s.mode==2 && additional)?2:1;
    const float left=baseX+eyeW*(.5f+(s.offsetX-s.width*.5f)/100.f),right=left+eyeW*s.width/100.f;
    const float h=float(targetH);
    const float top=h*(.5f+(s.offsetY-s.height*.5f)/100.f),bottom=top+h*s.height/100.f;
    EyeTileSignature sig;
    sig.mode=effMode;
    QuantizeRect(left,right,top,bottom,domX0,domX1,targetH,tile,sig.inner);
    if(effMode==2)QuantizeRect(left-eyeW*.08f,right+eyeW*.08f,top-h*.08f,bottom+h*.08f,domX0,domX1,targetH,tile,sig.outer);
    return sig;
}
}
Settings Normalize(Settings s) {
    const auto percent=[](float x,float fallback){return std::isfinite(x)?std::clamp(x,20.f,100.f):fallback;};
    s.mode=std::clamp(s.mode,0,2);s.width=percent(s.width,70);s.height=percent(s.height,65);
    const float x=(100-s.width)*.5f,y=(100-s.height)*.5f;
    s.offsetX=std::clamp(std::isfinite(s.offsetX)?s.offsetX:0.f,-x,x);
    s.offsetY=std::clamp(std::isfinite(s.offsetY)?s.offsetY:0.f,-y,y);
    return s;
}
bool ParseSetting(const char* line,Settings& s) {
    if(sscanf_s(line,"xr_vrs = %d",&s.mode)==1)return true;
    int eyeTracked=0;if(sscanf_s(line,"xr_vrs_eye_tracked = %d",&eyeTracked)==1){s.eyeTracked=eyeTracked!=0;return true;}
    if(sscanf_s(line,"xr_vrs_width = %f",&s.width)==1)return true;
    if(sscanf_s(line,"xr_vrs_height = %f",&s.height)==1)return true;
    if(sscanf_s(line,"xr_vrs_offset_x = %f",&s.offsetX)==1)return true;
    return sscanf_s(line,"xr_vrs_offset_y = %f",&s.offsetY)==1;
}
void WriteSettings(FILE* file,Settings s) {
    s=Normalize(s);
    std::fprintf(file,"xr_vrs=%d\nxr_vrs_eye_tracked=%d\nxr_vrs_width=%.2f\nxr_vrs_height=%.2f\nxr_vrs_offset_x=%.2f\nxr_vrs_offset_y=%.2f\n",
        s.mode,s.eyeTracked?1:0,s.width,s.height,s.offsetX,s.offsetY);
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
std::vector<uint8_t> BuildStereoRates(Settings leftEye,Settings rightEye,unsigned targetW,unsigned targetH,unsigned tile,bool additional) {
    if(targetW<2 || !targetH || targetW>16384 || targetH>16384 || tile<8 || tile>32 || (tile&(tile-1)))return {};
    leftEye=Normalize(leftEye);rightEye=Normalize(rightEye);
    const unsigned columns=(targetW+tile-1)/tile,rows=(targetH+tile-1)/tile;
    std::vector<uint8_t> result(size_t(columns)*rows,D3D12_SHADING_RATE_1X1);
    if(!leftEye.mode && !rightEye.mode)return result;
    const float leftW=float(targetW-targetW/2),rightX=leftW,rightW=float(targetW/2),h=float(targetH);
    const auto rateForEye=[&](const Settings& s,float baseX,float eyeW,float x0,float x1,float y0,float y1)->uint8_t {
        if(!s.mode)return D3D12_SHADING_RATE_1X1;
        const float left=baseX+eyeW*(.5f+(s.offsetX-s.width*.5f)/100.f),right=left+eyeW*s.width/100.f;
        const float top=h*(.5f+(s.offsetY-s.height*.5f)/100.f),bottom=top+h*s.height/100.f;
        const auto intersects=[&](float marginX,float marginY){return x1>left-marginX && x0<right+marginX && y1>top-marginY && y0<bottom+marginY;};
        if(intersects(0.f,0.f))return D3D12_SHADING_RATE_1X1;
        return uint8_t(s.mode==2 && additional && !intersects(eyeW*.08f,h*.08f)?D3D12_SHADING_RATE_4X4:D3D12_SHADING_RATE_2X2);
    };
    for(unsigned y=0;y<rows;++y)for(unsigned x=0;x<columns;++x) {
        const float x0=float(x*tile),x1=float(std::min(targetW,(x+1)*tile)),y0=float(y*tile),y1=float(std::min(targetH,(y+1)*tile));
        if(x1<=leftW)result[size_t(y)*columns+x]=rateForEye(leftEye,0.f,leftW,x0,x1,y0,y1);
        else if(x0>=rightX)result[size_t(y)*columns+x]=rateForEye(rightEye,rightX,rightW,x0,x1,y0,y1);
        else result[size_t(y)*columns+x]=std::min(rateForEye(leftEye,0.f,leftW,x0,leftW,y0,y1),rateForEye(rightEye,rightX,rightW,rightX,x1,y0,y1));
    }
    return result;
}
EyeTileSignature ComputeTileSignature(Settings s,unsigned w,unsigned h,unsigned tile,bool additional) {
    if(!w || !h || w>16384 || h>16384 || tile<8 || tile>32 || (tile&(tile-1)))return {};
    return SigForEye(s,0.f,float(w),0,w,h,tile,additional);
}
StereoTileSignature ComputeStereoTileSignature(Settings leftEye,Settings rightEye,unsigned targetW,unsigned targetH,unsigned tile,bool additional) {
    if(targetW<2 || !targetH || targetW>16384 || targetH>16384 || tile<8 || tile>32 || (tile&(tile-1)))return {};
    const unsigned leftPx=targetW-targetW/2;
    return {SigForEye(leftEye,0.f,float(leftPx),0,leftPx,targetH,tile,additional),SigForEye(rightEye,float(leftPx),float(targetW/2),leftPx,targetW,targetH,tile,additional)};
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
