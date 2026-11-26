#pragma once
#include <cstddef>
#include <cstdint>
#include <string_view>
namespace cvr::hud {
enum class Channel : unsigned { Main, Interaction, Basilisk, Surveillance, HitMarker, Count };
constexpr size_t ChannelCount=static_cast<size_t>(Channel::Count);
constexpr size_t Index(Channel c){return static_cast<size_t>(c);}
constexpr uint64_t EntryHash(std::string_view name){uint64_t h=14695981039346656037ull;for(unsigned char c:name){h^=c;h*=1099511628211ull;}return h;}
constexpr Channel EntryChannel(uint64_t name,bool sniperNest=false){
    if(name==EntryHash("interactions_root"))return Channel::Interaction;
    if(name==EntryHash("hud tank"))return Channel::Basilisk;
    if(name==EntryHash("camera_hud") || name==EntryHash("scanner") ||
       name==EntryHash("scanner_details"))return Channel::Surveillance;
    if(sniperNest && name==EntryHash("briefing_sequence_player"))return Channel::Surveillance;
    // Hit and kill markers: their own head-locked window (free-look cone 0), not the main HUD's cone.
    if(name==EntryHash("hit marker"))return Channel::HitMarker;
    return Channel::Main;
}
constexpr bool NeedsSeparateWindow(uint64_t name){
    return name==EntryHash("new_phone") || name==EntryHash("songbird_holocall") ||
           name==EntryHash("subtitles") || name==EntryHash("interactions_root") ||
           name==EntryHash("Notifications_Journal") || name==EntryHash("hud tank") ||
           name==EntryHash("q304_dossier") || name==EntryHash("q304_imprint_active_indicator") ||
           name==EntryHash("briefing_sequence_player") || name==EntryHash("scanner_details") ||
           name==EntryHash("camera_hud") || name==EntryHash("scanner") || name==EntryHash("hit marker");
}
constexpr bool DelayedFollow(Channel c){return c==Channel::Main;}
}
