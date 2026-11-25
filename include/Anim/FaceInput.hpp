#pragma once
#include <array>
#include <bit>
#include <cmath>
#include <cstdint>
#include <span>
#include <string_view>

namespace cvr::face {
// Steam Link's ALT/Custom OSC stream. Each value has its own receive timestamp:
// face regions arrive in separate bundles, and one region must not refresh another.
enum Channel : unsigned {
    SmileL, SmileR, BlinkL, BlinkR, Jaw,
    LookLeftL, LookRightL, LookUpL, LookDownL,
    LookLeftR, LookRightR, LookUpR, LookDownR,
    BrowInnerL, BrowInnerR, BrowOuterL, BrowOuterR, BrowDownL, BrowDownR,
    CheekL, CheekR, FrownL, FrownR, PuckerL, PuckerR,
    UpperConfidence, LowerConfidence, ChannelCount
};
inline constexpr std::array<std::string_view,ChannelCount> Addresses{
    "/sl/xrfb/facew/LipCornerPullerL", "/sl/xrfb/facew/LipCornerPullerR",
    "/sl/xrfb/facew/EyesClosedL", "/sl/xrfb/facew/EyesClosedR", "/sl/xrfb/facew/JawDrop",
    "/sl/xrfb/facew/EyesLookLeftL", "/sl/xrfb/facew/EyesLookRightL",
    "/sl/xrfb/facew/EyesLookUpL", "/sl/xrfb/facew/EyesLookDownL",
    "/sl/xrfb/facew/EyesLookLeftR", "/sl/xrfb/facew/EyesLookRightR",
    "/sl/xrfb/facew/EyesLookUpR", "/sl/xrfb/facew/EyesLookDownR",
    "/sl/xrfb/facew/InnerBrowRaiserL", "/sl/xrfb/facew/InnerBrowRaiserR",
    "/sl/xrfb/facew/OuterBrowRaiserL", "/sl/xrfb/facew/OuterBrowRaiserR",
    "/sl/xrfb/facew/BrowLowererL", "/sl/xrfb/facew/BrowLowererR",
    "/sl/xrfb/facew/CheekRaiserL", "/sl/xrfb/facew/CheekRaiserR",
    "/sl/xrfb/facew/LipCornerDepressorL", "/sl/xrfb/facew/LipCornerDepressorR",
    "/sl/xrfb/facew/LipPuckerL", "/sl/xrfb/facew/LipPuckerR",
    "/sl/xrfb/facec/UpperFace", "/sl/xrfb/facec/LowerFace"
};
struct Sample { float value{}; uint64_t at{}; bool present{}; };
struct Input {
    std::array<Sample,ChannelCount> channels{};
    uint64_t accepted{};
    bool Fresh(Channel channel,uint64_t now,uint64_t maxAge=250) const {
        const auto& v=channels[channel];
        return v.present && now>=v.at && now-v.at<=maxAge;
    }
    // Fade into the game's newly evaluated expression on tracking loss, rather
    // than freezing a smile or leaving the eyelids closed. No saved pose is replayed.
    float Weight(Channel channel,uint64_t now) const {
        const auto& v=channels[channel];
        if(!v.present || now<v.at || now-v.at>=450)return 0;
        const auto age=now-v.at;
        return age<=250 ? 1.f : float(450-age)/200.f;
    }
};
namespace detail {
inline uint32_t U32(std::span<const uint8_t> bytes,size_t at) {
    return uint32_t(bytes[at])<<24 | uint32_t(bytes[at+1])<<16 |
        uint32_t(bytes[at+2])<<8 | bytes[at+3];
}
inline bool String(std::span<const uint8_t> bytes,size_t& at,std::string_view& out) {
    const auto start=at;
    while(at<bytes.size() && bytes[at])++at;
    if(at==bytes.size())return false;
    out={reinterpret_cast<const char*>(bytes.data()+start),at-start};
    const auto end=(at+4)&~size_t(3);
    if(end>bytes.size())return false;
    while(at<end)if(bytes[at++])return false;
    return true;
}
inline bool Parse(std::span<const uint8_t> bytes,Input& input,uint64_t now,unsigned depth) {
    if(bytes.empty() || bytes.size()%4 || depth>8)return false;
    if(bytes.size()>=8 && std::string_view(reinterpret_cast<const char*>(bytes.data()),8)==std::string_view("#bundle\0",8)) {
        if(bytes.size()<16)return false;
        for(size_t at=16;at<bytes.size();) {
            if(bytes.size()-at<4)return false;
            const auto size=U32(bytes,at);at+=4;
            if(!size || size>bytes.size()-at || !Parse(bytes.subspan(at,size),input,now,depth+1))return false;
            at+=size;
        }
        return true;
    }
    size_t at=0;
    std::string_view address,tags;
    if(!String(bytes,at,address) || !String(bytes,at,tags) || !address.starts_with('/') || !tags.starts_with(','))return false;
    // Ignore unrelated Steam Link data and future payloads without interpreting it.
    unsigned channel=ChannelCount;
    for(unsigned i=0;i<ChannelCount;++i)if(address==Addresses[i]){channel=i;break;}
    if(channel==ChannelCount)return true;
    if(tags!=",f" || bytes.size()-at!=4)return false;
    const auto value=std::bit_cast<float>(U32(bytes,at));
    if(!std::isfinite(value) || value<0 || value>1)return false;
    input.channels[channel]={value,now,true};++input.accepted;
    return true;
}
}
inline bool Receive(std::span<const uint8_t> packet,Input& input,uint64_t now) {
    // A damaged bundle must not partially publish a new expression.
    auto next=input;
    if(!detail::Parse(packet,next,now,0))return false;
    input=next;return true;
}
}
