#include "Anim/FaceMapping.hpp"
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <vector>
using namespace cvr::face;
void Check(bool pass,const char* message){if(!pass){std::fprintf(stderr,"FAIL %s\n",message);std::exit(1);}}
bool Near(float a,float b){return std::abs(a-b)<.0001f;}
struct Rig {
    std::vector<uint64_t> names;
    TrackRange range;
    explicit Rig(unsigned prefix) {
        names.resize(prefix,NameHash("envelope"));range.mainBegin=prefix;
        // Deliberately different from Controls' order, with an unrelated track.
        for(auto name:{"eye_r_dir_in","lips_r_corner_up","unrelated","eye_l_blink","lips_l_corner_up",
                       "eye_l_dir_in","eye_l_dir_out","jaw_mid_open","eye_l_brows_raise_in"})
            names.push_back(NameHash(name));
        range.mainEnd=range.overrideBegin=unsigned(names.size());
        names.push_back(NameHash("lips_l_corner_upAnimOverrideWeight"));
        range.overrideEnd=range.outputBegin=unsigned(names.size());
        names.push_back(NameHash("lips_l_corner_upLipsyncPoseOutput"));
        range.outputEnd=unsigned(names.size());
    }
};
int main() {
    Settings enabled;enabled.enabled=true;
    Input input;
    input.channels[SmileL]={.8f,100,true};input.channels[LowerConfidence]={1,100,true};
    for(unsigned prefix:{13u,23u}) { // Player and mirror have different envelope counts.
        Rig rig(prefix);auto map=Resolve(rig.names,rig.range);
        std::vector<float> tracks(rig.names.size(),.2f);const auto original=tracks;
        Check(Apply(map,tracks,input,enabled,100)==1,"only available fresh channels applied");
        Check(Near(tracks[prefix+4],.8f),"smile resolved by name on either rig");
        Check(Near(tracks[rig.range.overrideBegin],1) && Near(tracks[rig.range.outputBegin],0),"native lip contribution suppressed only for controlled track");
        for(unsigned i=0;i<tracks.size();++i)
            if(i!=prefix+4 && i!=rig.range.overrideBegin && i!=rig.range.outputBegin)
                Check(tracks[i]==original[i],"unrelated native tracks preserved");
        tracks=original;
        Check(Apply(map,tracks,input,enabled,450)==1 && Near(tracks[prefix+4],.5f),"stale tracking blends with current native value");
        tracks=original;
        Check(Apply(map,tracks,input,enabled,550)==0 && tracks==original,"expired tracking hands back exact native pose");
        auto disabled=enabled;disabled.enabled=false;
        Check(!Apply(map,tracks,input,disabled,100) && tracks==original,"disabled adapter preserves native pose");
        disabled=enabled;disabled.mouth=false;
        Check(!Apply(map,tracks,input,disabled,100) && tracks==original,"mouth toggle isolates feature");
        auto missing=input;missing.channels[LowerConfidence]={};
        Check(!Apply(map,tracks,missing,enabled,100),"missing confidence cannot animate a face");
        missing.channels[LowerConfidence]={.49f,100,true};
        Check(!Apply(map,tracks,missing,enabled,100),"unreliable confidence yields to animation");
        missing.channels[LowerConfidence]={1,0,true};missing.channels[SmileL].at=500;
        Check(!Apply(map,tracks,missing,enabled,500),"fresh mouth cannot revive stale confidence");
        auto invalid=rig.range;invalid.mainEnd=invalid.overrideEnd+1;
        Check(!Apply(Resolve(rig.names,invalid),tracks,input,enabled,100),"overlapping native ranges rejected");
        invalid=rig.range;invalid.outputEnd=unsigned(rig.names.size())+1;
        Check(!Apply(Resolve(rig.names,invalid),tracks,input,enabled,100),"out of range layout rejected");
        std::vector<uint64_t> unknown(rig.names.size(),NameHash("unknown"));
        Check(!Apply(Resolve(unknown,rig.range),tracks,input,enabled,100),"unsupported names leave native animation untouched");
        tracks[prefix+4]=std::numeric_limits<float>::quiet_NaN();
        Check(!Apply(map,tracks,input,enabled,100),"invalid native value not overridden");
    }
    Rig rig(13);auto map=Resolve(rig.names,rig.range);
    std::vector<float> tracks(rig.names.size(),0);
    Input eyes;
    eyes.channels[LookLeftL]={.7f,100,true};eyes.channels[LookLeftR]={.6f,100,true};
    eyes.channels[UpperConfidence]={1,100,true};
    Check(Apply(map,tracks,eyes,enabled,100)==2,"bilateral eye movement");
    Check(Near(tracks[19],.7f) && Near(tracks[13],.6f) && tracks[18]==0,"leftward gaze means left eye out, right eye in");
    eyes.channels[BrowInnerL]={.9f,100,true};
    Apply(map,tracks,eyes,enabled,100);Check(tracks[21]==0,"brows opt in separately");
    enabled.brows=true;Apply(map,tracks,eyes,enabled,100);Check(Near(tracks[21],.9f),"brow toggle activates mapped control");
    std::puts("PASS rig-specific mapping, eye direction, feature isolation, confidence and native fallback");
}
