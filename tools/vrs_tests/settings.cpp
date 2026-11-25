#include "Render/FixedVrs.hpp"
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <initializer_list>
#include <limits>
#include <stdexcept>
#include <iostream>
#include <utility>
#include <vector>
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
    D3D12_GRAPHICS_PIPELINE_STATE_DESC d{};d.VS.BytecodeLength=d.PS.BytecodeLength=1;d.NumRenderTargets=3;d.SampleDesc.Count=1;d.DepthStencilState.DepthEnable=TRUE;
    Check(EligiblePipeline(d));d.BlendState.RenderTarget[0].BlendEnable=TRUE;Check(!EligiblePipeline(d));d.BlendState={};
    d.NumRenderTargets=1;Check(!EligiblePipeline(d));d.NumRenderTargets=4;d.SampleDesc.Count=4;Check(!EligiblePipeline(d));
    d.SampleDesc.Count=1;d.GS.BytecodeLength=1;Check(!EligiblePipeline(d));
    Check(BuildStereoRates({},{},1,256,16,true).empty());
    Check(BuildStereoRates({},{},16386,256,16,true).empty());
    Check(BuildStereoRates({},{},512,256,0,true).empty());
    Check(BuildStereoRates({},{},512,256,12,true).empty());
    for(unsigned tile:{8u,16u,32u})for(bool additional:{false,true}) {
        const unsigned eyeW=256,targetW=512,h=192;
        const unsigned columns=(targetW+tile-1)/tile,eyeColumns=columns/2,rows=(h+tile-1)/tile;
        for(const auto& [left,right]:std::initializer_list<std::pair<Settings,Settings>>{
            {{2,50,45,20,-10},{2,50,45,-20,10}},
            {{1,60,60,0,0},{2,40,40,0,0}},
        }) {
            const auto stereo=BuildStereoRates(left,right,targetW,h,tile,additional);
            const auto leftMap=BuildRates(left,eyeW,h,tile,additional);
            const auto rightMap=BuildRates(right,eyeW,h,tile,additional);
            Check(stereo.size()==size_t(columns)*rows);
            Check(leftMap.size()==size_t(eyeColumns)*rows && rightMap.size()==size_t(eyeColumns)*rows);
            for(unsigned row=0;row<rows;++row) {
                for(unsigned col=0;col<eyeColumns;++col)
                    Check(stereo[size_t(row)*columns+col]==leftMap[size_t(row)*eyeColumns+col]);
                for(unsigned col=eyeColumns;col<columns;++col)
                    Check(stereo[size_t(row)*columns+col]==rightMap[size_t(row)*eyeColumns+(col-eyeColumns)]);
            }
        }
    }
    for(unsigned targetW:{2970u,2969u}) {
        const unsigned leftW=(targetW+1)/2,rightW=targetW-leftW,h=1484,tile=16;
        const unsigned columns=(targetW+tile-1)/tile,rows=(h+tile-1)/tile,seamCol=leftW/tile;
        for(bool additional:{false,true}) {
            for(const auto& [left,right]:std::initializer_list<std::pair<Settings,Settings>>{
                {{2,40,40,30,0},{2,40,40,0,0}},
                {{2,40,40,0,0},{2,40,40,-30,0}},
                {{2,40,40,30,0},{2,40,40,-30,0}},
                {{1,70,65,0,0},{2,50,40,-25,30}},
            }) {
                const auto stereo=BuildStereoRates(left,right,targetW,h,tile,additional);
                Check(stereo.size()==size_t(columns)*rows);
                for(unsigned y=0;y<h;++y)for(unsigned x=0;x<targetW;++x) {
                    const auto rate=stereo[size_t(y/tile)*columns+x/tile];
                    Check(rate==0 || rate==5 || (additional && rate==10));
                    const bool isLeft=x<leftW;
                    const Settings& eye=isLeft?left:right;
                    const unsigned ex=isLeft?x:x-leftW,ew=isLeft?leftW:rightW;
                    const float dx=(float(ex)+.5f)/ew*100-50-eye.offsetX,dy=(float(y)+.5f)/h*100-50-eye.offsetY;
                    if(!eye.mode || (std::abs(dx)<eye.width*.5f && std::abs(dy)<eye.height*.5f))Check(rate==0);
                }
                if(left.offsetX==30 || right.offsetX==-30)
                    Check(stereo[size_t((h/2)/tile)*columns+seamCol]==0);
            }
        }
    }
    Check(!Settings{}.eyeTracked);
    Check(Settings{2,70,65,0,0,false}.SameAllocation(Settings{2,70,65,12,-8,true}));
    Check(!Settings{2,70,65,0,0,false}.SameAllocation(Settings{1,70,65,0,0,false}));
    Check(!Settings{2,70,65,0,0,false}.SameAllocation(Settings{2,60,65,0,0,false}));
    Check(!Settings{2,70,65,0,0,false}.SameAllocation(Settings{2,70,55,0,0,false}));
    {
        Settings p;
        Check(ParseSetting("xr_vrs_eye_tracked=1",p) && p.eyeTracked);
        Check(ParseSetting("xr_vrs_eye_tracked = 0",p) && !p.eyeTracked);
        auto writeIni=[](FILE* f,const Settings& val){
            if constexpr(requires(FILE* fp,const Settings& sv){WriteSettings(fp,sv);})WriteSettings(f,val);
            else WriteSettings(val,f);
        };
        for(const Settings expected:{Settings{2,62.5f,55.f,11.5f,-7.5f,true},Settings{1,70.f,65.f,-4.f,3.5f,false}}){
            std::filesystem::path fallback;
            FILE* tmp=nullptr;
            if(tmpfile_s(&tmp)!=0 || !tmp){
                fallback=std::filesystem::temp_directory_path()/"vrs_settings_roundtrip_test.ini";
                fopen_s(&tmp,fallback.string().c_str(),"w+b");
            }
            Check(tmp!=nullptr);
            struct FileGuard{
                FILE* f;
                std::filesystem::path path;
                ~FileGuard(){if(f)std::fclose(f);if(!path.empty()){std::error_code ec;std::filesystem::remove(path,ec);}}
            } guard{tmp,fallback};
            writeIni(tmp,expected);
            std::fflush(tmp);
            std::rewind(tmp);
            Settings roundTrip{0,20.f,20.f,0.f,0.f,!expected.eyeTracked};
            char line[512]{};
            while(std::fgets(line,sizeof(line),tmp)){
                size_t len=std::strlen(line);
                while(len && (line[len-1]=='\n' || line[len-1]=='\r'))line[--len]='\0';
                if(len)ParseSetting(line,roundTrip);
            }
            Check(roundTrip==expected);
        }
    }
    const float oxValues[]{-30.0f,-29.83f,-29.79f,-25.0f,-12.3f,-0.2f,-0.05f,0.0f,0.05f,6.25f,12.5f,25.0f,29.79f,29.83f,30.0f};
    const float oyValues[]{-20.0f,0.0f,0.04f,15.0f};
    for(unsigned w:{256u,1485u})for(int mode:{1,2})for(bool additional:{false,true}){
        const unsigned h=192u,tile=16u;
        std::vector<EyeTileSignature> sigs;
        std::vector<std::vector<uint8_t>> maps;
        for(float ox:oxValues)for(float oy:oyValues){
            const Settings st{mode,40.f,40.f,ox,oy,true};
            sigs.push_back(ComputeTileSignature(st,w,h,tile,additional));
            maps.push_back(BuildRates(st,w,h,tile,additional));
        }
        for(size_t i=0;i<sigs.size();++i)for(size_t j=i;j<sigs.size();++j)
            Check((sigs[i]==sigs[j])==(maps[i]==maps[j]));
    }
    for(unsigned targetW:{512u,2970u,2969u})for(int mode:{1,2})for(bool additional:{false,true}){
        const unsigned h=192u,tile=16u;
        std::vector<StereoTileSignature> sigs;
        std::vector<std::vector<uint8_t>> maps;
        for(float ox:oxValues)for(float oy:oyValues){
            const Settings left{mode,40.f,40.f,ox,oy,true};
            const Settings right{mode,40.f,40.f,-ox,-oy,true};
            sigs.push_back(ComputeStereoTileSignature(left,right,targetW,h,tile,additional));
            maps.push_back(BuildStereoRates(left,right,targetW,h,tile,additional));
        }
        if(targetW==512u){
            for(size_t i=0;i<sigs.size();++i)for(size_t j=i;j<sigs.size();++j)
                Check((sigs[i]==sigs[j])==(maps[i]==maps[j]));
        }else{
            size_t distinct=0;
            for(size_t i=0;i<sigs.size();++i){
                bool first=true;
                for(size_t j=0;j<i;++j){if(sigs[i]==sigs[j]){first=false;break;}}
                if(first)++distinct;
                for(size_t j=i;j<sigs.size();++j)
                    if(sigs[i]==sigs[j])Check(maps[i]==maps[j]);
            }
            Check(distinct>=sigs.size()/2);
        }
    }
    std::cout<<"PASS VRS center coverage, tile sizes, edge tiles, offsets, unsupported rates, defaults, parsing, opaque-pipeline gates, stereo maps and Phase 2 eye-tracked tile signatures\n";
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}
