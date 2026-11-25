#pragma once
#include "Anim/FaceTracking.hpp"
#include <algorithm>
#include <string>

namespace cvr::face {
constexpr uint64_t NameHash(std::string_view text) {
    uint64_t hash=0xcbf29ce484222325ull;
    for(unsigned char c:text)hash=(hash^c)*0x100000001b3ull;
    return hash;
}
enum Feature { MouthFeature,BlinkFeature,EyesFeature,BrowsFeature };
struct Control { std::string_view track;Channel channel;Feature feature; };
inline constexpr Control Controls[]{
    {"lips_l_corner_up",SmileL,MouthFeature},{"lips_r_corner_up",SmileR,MouthFeature},
    {"jaw_mid_open",Jaw,MouthFeature},
    {"eye_l_blink",BlinkL,BlinkFeature},{"eye_r_blink",BlinkR,BlinkFeature},
    {"eye_l_dir_out",LookLeftL,EyesFeature},{"eye_l_dir_in",LookRightL,EyesFeature},
    {"eye_l_dir_up",LookUpL,EyesFeature},{"eye_l_dir_dn",LookDownL,EyesFeature},
    {"eye_r_dir_in",LookLeftR,EyesFeature},{"eye_r_dir_out",LookRightR,EyesFeature},
    {"eye_r_dir_up",LookUpR,EyesFeature},{"eye_r_dir_dn",LookDownR,EyesFeature},
    {"eye_l_brows_raise_in",BrowInnerL,BrowsFeature},{"eye_r_brows_raise_in",BrowInnerR,BrowsFeature},
    {"eye_l_brows_raise_out",BrowOuterL,BrowsFeature},{"eye_r_brows_raise_out",BrowOuterR,BrowsFeature},
    {"eye_l_brows_lower",BrowDownL,BrowsFeature},{"eye_r_brows_lower",BrowDownR,BrowsFeature}
};
struct TrackRange { unsigned mainBegin{},mainEnd{},overrideBegin{},overrideEnd{},outputBegin{},outputEnd{}; };
struct TrackMap { int main=-1,overrideWeight=-1,output=-1; };
using Mapping=std::array<TrackMap,std::size(Controls)>;
inline Mapping Resolve(std::span<const uint64_t> names,TrackRange range) {
    Mapping result{};
    if(range.mainBegin>range.mainEnd || range.mainEnd>range.overrideBegin ||
       range.overrideBegin>range.overrideEnd || range.overrideEnd>range.outputBegin ||
       range.outputBegin>range.outputEnd || range.outputEnd>names.size())return result;
    for(unsigned k=0;k<result.size();++k) {
        const auto main=NameHash(Controls[k].track);
        const auto overrideHash=NameHash(std::string(Controls[k].track)+"AnimOverrideWeight");
        const auto output=NameHash(std::string(Controls[k].track)+"LipsyncPoseOutput");
        for(unsigned i=range.mainBegin;i<range.mainEnd;++i)if(names[i]==main)result[k].main=int(i);
        for(unsigned i=range.overrideBegin;i<range.overrideEnd;++i)if(names[i]==overrideHash)result[k].overrideWeight=int(i);
        for(unsigned i=range.outputBegin;i<range.outputEnd;++i)if(names[i]==output)result[k].output=int(i);
    }
    return result;
}
inline unsigned Apply(const Mapping& mapping,std::span<float> tracks,const Input& input,const Settings& settings,uint64_t now) {
    if(!settings.enabled)return 0;
    unsigned writes=0;
    for(unsigned k=0;k<mapping.size();++k) {
        const auto& control=Controls[k];const auto& map=mapping[k];
        const bool feature=control.feature==MouthFeature?settings.mouth:control.feature==BlinkFeature?settings.blink:
            control.feature==EyesFeature?settings.eyes:settings.brows;
        const auto confidence=control.feature==MouthFeature?LowerConfidence:UpperConfidence;
        const auto& sample=input.channels[control.channel];
        if(!feature || map.main<0 || size_t(map.main)>=tracks.size() ||
           input.channels[confidence].value<.5f)continue;
        const auto weight=std::min(input.Weight(control.channel,now),input.Weight(confidence,now));
        if(weight<=0 || !std::isfinite(tracks[map.main]))continue;
        tracks[map.main]=std::lerp(tracks[map.main],sample.value,weight);++writes;
        // Prevent the mirror's lipsync contribution adding another expression to
        // the controlled track. Other tracks retain their native animation.
        if(map.overrideWeight>=0 && size_t(map.overrideWeight)<tracks.size() && std::isfinite(tracks[map.overrideWeight]))
            tracks[map.overrideWeight]=std::lerp(tracks[map.overrideWeight],1.f,weight);
        if(map.output>=0 && size_t(map.output)<tracks.size() && std::isfinite(tracks[map.output]))
            tracks[map.output]*=1.f-weight;
    }
    return writes;
}
}
