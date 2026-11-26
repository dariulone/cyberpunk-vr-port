#include "Natives/HandReferenceFrame.hpp"
#include <algorithm>
#include <array>
#include <cstdlib>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <unordered_map>
#include <vector>
using namespace cvr::handspace;
struct Sample {double t;Vector error;float physicalGap;bool touching,world,pressed;};
struct Phase {std::string name;double begin,end;std::vector<Sample> samples[2];};
static double Length(Vector v){return std::sqrt(LengthSquared(v));}
static double Percentile(std::vector<double> v,double q){
    if(v.empty())return 0;std::sort(v.begin(),v.end());return v[std::min(v.size()-1,size_t(q*(v.size()-1)))];
}
int main(int argc,char**argv){
    if(argc!=3){std::cerr<<"trace.csv phases.tsv\n";return 2;}
    std::ifstream phasesFile(argv[2]);std::vector<Phase> phases;
    for(Phase p;phasesFile>>p.name>>p.begin>>p.end;)phases.push_back(p);
    std::ifstream input(argv[1]);if(!input||phases.empty())return 3;
    std::string line;std::unordered_map<std::string,size_t> fields;
    while(std::getline(input,line)){
        if(line.starts_with("t,side,")){
            fields.clear();std::istringstream row(line);std::string key;size_t i=0;
            while(std::getline(row,key,',')){if(!key.empty()&&key.back()=='\r')key.pop_back();fields[key]=i++;}continue;
        }
        if(fields.empty())continue;
        std::vector<double> v;const char* at=line.c_str();
        while(*at){char* next{};const auto x=std::strtod(at,&next);if(next==at)break;v.push_back(x);at=next;if(*at==',')++at;else break;}
        if(v.size()!=fields.size())continue;
        const auto get=[&](const char* key){return v.at(fields.at(key));};
        const double t=get("t");const int side=int(get("side"));if(side<0||side>1)continue;
        for(auto& phase:phases)if(t>=phase.begin&&t<=phase.end){
            auto& rows=phase.samples[side];if(!rows.empty()&&t-rows.back().t<.004)break;
            Rotation q{float(get("trot_x")),float(get("trot_y")),float(get("trot_z")),float(get("trot_w"))};
            if(!Normalize(q))break;
            const Vector correction{float(get("drawn_x")-get("track_x")),float(get("drawn_y")-get("track_y")),float(get("drawn_z")-get("track_z"))};
            const Vector gap{float(get("drawn_x")-get("raw_x")),float(get("drawn_y")-get("raw_y")),float(get("drawn_z")-get("raw_z"))};
            rows.push_back({t,Rotate(Conjugate(q),correction),float(Length(gap)),get("touching")!=0,get("on_world")!=0,get("pressed")!=0});break;
        }
    }
    std::cout<<"phase,side,n,offset_mean_mm,local_deviation_p95_mm,local_jitter_p95_mm,local_jitter_max_mm,draw_vs_physics_p95_mm,touch_fraction,world_fraction,pressed_fraction\n"<<std::fixed<<std::setprecision(3);
    for(const auto& phase:phases)for(int side=0;side<2;++side){
        const auto& rows=phase.samples[side];if(rows.empty())continue;
        Vector mean{};double offset=0,touch=0,world=0,pressed=0;
        for(const auto& r:rows){mean=mean+r.error;offset+=Length(r.error);touch+=r.touching;world+=r.world;pressed+=r.pressed;}
        mean=mean*(1.f/float(rows.size()));std::vector<double> deviation,jitter,gaps;
        for(size_t i=0;i<rows.size();++i){
            const auto& r=rows[i];deviation.push_back(Length(r.error-mean)*1000);gaps.push_back(r.physicalGap*1000);
            if(i>=2){const auto& a=rows[i-2];const auto& b=rows[i-1];
                if(b.t>a.t&&r.t-b.t<.1&&b.t-a.t<.1){const auto expected=b.error+(b.error-a.error)*float((r.t-b.t)/(b.t-a.t));jitter.push_back(Length(r.error-expected)*1000);}}
        }
        const double n=double(rows.size());
        std::cout<<phase.name<<','<<side<<','<<rows.size()<<','<<offset/n*1000<<','<<Percentile(deviation,.95)<<','<<Percentile(jitter,.95)<<','<<Percentile(jitter,1)<<','<<Percentile(gaps,.95)<<','<<touch/n<<','<<world/n<<','<<pressed/n<<'\n';
    }
}
