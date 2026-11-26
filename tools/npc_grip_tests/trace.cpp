// Analyze final-FK/consumed-anchor timing without conflating independent Lua reads.
#include <algorithm>
#include <array>
#include <cmath>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <map>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

using Row=std::map<std::string,double>;
std::vector<std::string> Split(const std::string& line){
    std::vector<std::string> out;std::istringstream in(line);std::string item;
    while(std::getline(in,item,','))out.push_back(item);return out;
}
double Distance(const Row& r,const std::string& a,const std::string& b){
    double sum=0;for(const char* axis:{"x","y","z"}){const double d=r.at(a+axis)-r.at(b+axis);sum+=d*d;}return 1000*std::sqrt(sum);
}
double Angle(const Row& r,const std::string& a,const std::string& b){
    double ab=0,aa=0,bb=0;for(const char* axis:{"qx","qy","qz","qw"}){
        const double x=r.at(a+axis),y=r.at(b+axis);ab+=x*y;aa+=x*x;bb+=y*y;
    }
    if(aa<.5||bb<.5)throw std::runtime_error("invalid quaternion");
    return std::acos(std::clamp(std::abs(ab)/std::sqrt(aa*bb),0.,1.))*360/std::acos(-1.);
}
void PredictSpin(Row& r){
    const double x=r.at("give_wx"),y=r.at("give_wy"),z=r.at("give_wz"),dt=r.at("drive_dt");
    const double n=std::sqrt(x*x+y*y+z*z),scale=n>1e-12?std::sin(n*dt*.5)/n:0;
    const double a[4]{x*scale,y*scale,z*scale,std::cos(n*dt*.5)};
    const double b[4]{r.at("pre_qx"),r.at("pre_qy"),r.at("pre_qz"),r.at("pre_qw")};
    r["pred_qx"]=a[3]*b[0]+a[0]*b[3]+a[1]*b[2]-a[2]*b[1];
    r["pred_qy"]=a[3]*b[1]-a[0]*b[2]+a[1]*b[3]+a[2]*b[0];
    r["pred_qz"]=a[3]*b[2]+a[0]*b[1]-a[1]*b[0]+a[2]*b[3];
    r["pred_qw"]=a[3]*b[3]-a[0]*b[0]-a[1]*b[1]-a[2]*b[2];
}
int main(int argc,char** argv){try{
    if(argc!=2)return 2;std::ifstream in(argv[1]);if(!in)throw std::runtime_error("cannot open trace");
    std::string line;if(!std::getline(in,line))throw std::runtime_error("empty trace");const auto header=Split(line);
    std::map<std::string,std::vector<double>> metrics;std::vector<Row> mismatches;size_t all=0,steady=0,newer=0,samePhysics=0;double dropped=0,maxAngle=-1;Row worst;
    while(std::getline(in,line)){
        if(line.empty()||line.starts_with("at,"))continue;const auto items=Split(line);
        if(items.size()!=header.size())throw std::runtime_error("column count mismatch");
        Row r;for(size_t k=0;k<items.size();++k){const double v=std::stod(items[k]);if(!std::isfinite(v))throw std::runtime_error("nonfinite trace");r[header[k]]=v;}
        ++all;dropped=std::max(dropped,r.at("dropped"));if(r.at("weight")<1||r.at("used_rev")<=0)continue;++steady;
        const auto add=[&](const std::string& name,double value){metrics[name].push_back(value);};
        add("hand_used_mm",Distance(r,"hand_","used_"));const double angle=Angle(r,"hand_","used_");add("hand_used_degrees",angle);
        if(angle>maxAngle){maxAngle=angle;worst=r;}
        add("hand_latest_mm",Distance(r,"hand_","latest_"));add("hand_latest_degrees",Angle(r,"hand_","latest_"));
        if(Angle(r,"hand_","latest_")>2)mismatches.push_back(r);
        add("consumed_anchor_age_ms",r.at("at")-r.at("used_at"));add("control_age_ms",r.at("at")-r.at("control_at"));
        if(r.at("latest_rev")!=r.at("used_rev"))++newer;
        if(r.at("physical")){
            add("physics_age_ms",r.at("at")-r.at("physics_at"));add("hand_body_mm",Distance(r,"hand_","body_"));add("hand_body_degrees",Angle(r,"hand_","body_"));
            add("body_motor_mm",Distance(r,"body_","motor_"));add("body_motor_degrees",Angle(r,"body_","motor_"));
            if(r.at("physics_grip")==r.at("used_rev")){++samePhysics;add("same_revision_body_hand_mm",Distance(r,"body_","hand_"));add("same_revision_body_hand_degrees",Angle(r,"body_","hand_"));}
            if(r.contains("drive_flags")&&!(int(r.at("drive_flags"))&16)){
                const int flags=int(r.at("drive_flags")),kind=int(r.at("touch_kind"));
                const bool disabled=kind==2&&!(int(r.at("touch_mask"))&(1<<int(r.at("side"))));
                const std::string group=(flags&2)?(disabled?"disabled_person":kind==2?"person":kind==3?"other_hand":"scene"):"free";
                add("motor_error_degrees_"+group,Angle(r,"body_","motor_"));add("acceleration_gain_"+group,r.at("drive_gain"));
                add("target_clear_"+group,(flags&8)?1:0);
                PredictSpin(r);add("command_end_error_degrees_"+group,Angle(r,"pred_","motor_"));add("simulation_deflection_degrees_"+group,Angle(r,"pred_","body_"));
                double delta=0;for(const char* axis:{"x","y","z"}){const double d=r.at(std::string("actual_w")+axis)-r.at(std::string("give_w")+axis);delta+=d*d;}
                add("simulation_velocity_change_rad_s_"+group,std::sqrt(delta));
            }
        }
    }
    std::cout<<"rows="<<all<<" full_weight="<<steady<<" newer_at_commit="<<newer<<" same_physics_revision="<<samePhysics<<" dropped="<<dropped<<'\n';
    std::cout<<std::setprecision(8)<<"metric,count,mean,p50,p95,p99,max\n";
    for(auto& [name,v]:metrics){std::sort(v.begin(),v.end());double sum=0;for(double x:v)sum+=x;
        std::cout<<name<<','<<v.size()<<','<<sum/v.size()<<','<<v[v.size()/2]<<','<<v[(v.size()-1)*95/100]<<','<<v[(v.size()-1)*99/100]<<','<<v.back()<<'\n';}
    if(steady)std::cout<<"worst_rotation at="<<std::fixed<<std::setprecision(0)<<worst.at("at")<<" used="<<worst.at("used_rev")<<" latest="<<worst.at("latest_rev")<<'\n';
    std::cout<<"first mismatches (at,control_at,used,latest,angle):\n";
    for(size_t i=0;i<std::min(size_t(18),mismatches.size());++i){const auto& r=mismatches[i];
        std::cout<<std::setprecision(0)<<r.at("at")<<','<<r.at("control_at")<<','<<r.at("used_rev")<<','<<r.at("latest_rev")<<','<<std::setprecision(5)<<Angle(r,"hand_","latest_")<<'\n';}
    return steady?0:2;
}catch(const std::exception& e){std::cerr<<e.what()<<'\n';return 1;}}
