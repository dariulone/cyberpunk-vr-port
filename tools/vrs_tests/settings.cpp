#include "Render/FixedVrs.hpp"
#include "Runtimes/EyeGazeProjection.hpp"
#include <algorithm>
#include <cmath>
#include <limits>
#include <stdexcept>
#include <iostream>
using namespace cvr::vrs;
static void Check(bool value){if(!value)throw std::runtime_error("VRS policy assertion failed");}
int main()try{
    Check(Settings{}.mode==0);
    auto s=Normalize({9,std::nanf(""),150,100,std::nanf("")});
    Check(s.mode==2 && s.width==70 && s.height==100 && s.offsetX==15 && s.offsetY==0);
    Check(BuildRates({},256,256,0,true).empty());Check(BuildRates({},256,256,12,true).empty());
    for(unsigned tile:{8u,16u,32u})for(unsigned w:{65u,1484u,2560u})for(unsigned h:{65u,1484u,2560u}) {
        for(auto test:{Settings{0},Settings{1,70,65,0,0},Settings{2,50,40,-25,30},Settings{2,100,100,0,0}}) {
            for(bool additional:{false,true}) {
                const auto map=BuildRates(test,w,h,tile,additional);const auto columns=(w+tile-1)/tile;
                Check(map.size()==size_t(columns)*((h+tile-1)/tile));
                for(unsigned y=0;y<h;++y)for(unsigned x=0;x<w;++x) {
                    const auto rate=map[size_t(y/tile)*columns+x/tile];
                    Check(rate==0 || rate==5 || (additional && rate==10));
                    const float dx=(float(x)+.5f)/w*100-50-test.offsetX,dy=(float(y)+.5f)/h*100-50-test.offsetY;
                    if(!test.mode || (std::abs(dx)<test.width*.5f && std::abs(dy)<test.height*.5f))Check(rate==0);
                }
            }
        }
    }
    Settings parsed;
    Check(ParseSetting("xr_vrs = 2",parsed));Check(ParseSetting("xr_vrs_width=80",parsed));
    Check(ParseSetting("xr_vrs_height=60",parsed));Check(ParseSetting("xr_vrs_offset_x=-5",parsed));Check(ParseSetting("xr_vrs_offset_y=10",parsed));
    Check(!ParseSetting("xr_vrs_extra=1",parsed));Check(parsed==Settings{2,80,60,-5,10});
    Check(!Settings{}.eyeTracked);Check(ParseSetting("xr_vrs_eye_tracked=1",parsed) && parsed.eyeTracked);
    FILE* file{};Check(tmpfile_s(&file)==0 && file);WriteSettings(file,parsed);rewind(file);
    Settings roundTrip;char line[256];while(fgets(line,sizeof(line),file))Check(ParseSetting(line,roundTrip));fclose(file);
    Check(roundTrip==parsed);
    float gx{},gy{};
    using cvr::eye_tracking::Project;
    Check(Project({0,0,0,1},-2,1,-1,1,gx,gy) && std::abs(gx-2.f/3)<1e-5f && gy==.5f);
    Check(Project({0,0,0,1},-1,2,-1,1,gx,gy) && std::abs(gx-1.f/3)<1e-5f);
    Check(Project({0,-std::sin(.2f),0,std::cos(.2f)},-1,1,-1,1,gx,gy) && gx>.5f);
    Check(Project({std::sin(.2f),0,0,std::cos(.2f)},-1,1,-1,1,gx,gy) && gy<.5f);
    Check(!Project({0,1,0,0},-1,1,-1,1,gx,gy));Check(!Project({0,0,0,0},-1,1,-1,1,gx,gy));
    Check(!Project({0,0,0,1},std::nanf(""),1,-1,1,gx,gy));
    Check(cvr::eye_tracking::FreshSample(1000000000,950000000));
    Check(!cvr::eye_tracking::FreshSample(1000000000,0));Check(!cvr::eye_tracking::FreshSample(1000000000,800000000));
    Check(!cvr::eye_tracking::FreshSample(1000000000,1100000000));
    // Every point in the requested moving rectangle stays fine after grid rounding,
    // clipping at the eye edge, and a stereo seam that cuts through a hardware tile.
    for(unsigned tile:{8u,16u,32u})for(unsigned w:{130u,512u})for(float x:{0.f,.23f,.5f,.77f,1.f}) {
        constexpr unsigned h=192;
        const std::array regions{GazeRegion({2,30,25},x,.31f),GazeRegion({2,30,25},1-x,.69f)};
        const auto map=BuildStereoRates(regions,w,h,tile,true);Check(!map.empty());
        for(unsigned py=0;py<h;++py)for(unsigned px=0;px<w;++px) {
            const unsigned eye=px/(w/2);
            const float nx=(float(px%(w/2))+.5f)/(w/2),ny=(py+.5f)/h;
            if(std::abs(nx-(eye?1-x:x))<.15f && std::abs(ny-(eye?.69f:.31f))<.125f)
                Check(map[size_t(py/tile)*((w+tile-1)/tile)+px/tile]==0);
        }
    }
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};d.VS.BytecodeLength=d.PS.BytecodeLength=1;d.NumRenderTargets=3;d.SampleDesc.Count=1;d.DepthStencilState.DepthEnable=TRUE;
    Check(EligiblePipeline(d));d.BlendState.RenderTarget[0].BlendEnable=TRUE;Check(!EligiblePipeline(d));d.BlendState={};
    d.NumRenderTargets=1;Check(!EligiblePipeline(d));d.NumRenderTargets=4;d.SampleDesc.Count=4;Check(!EligiblePipeline(d));
    d.SampleDesc.Count=1;d.GS.BytecodeLength=1;Check(!EligiblePipeline(d));
    std::cout<<"PASS VRS center coverage, tile sizes, edge tiles, offsets, unsupported rates, defaults, parsing and opaque-pipeline gates\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
