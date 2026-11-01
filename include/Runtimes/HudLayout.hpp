#pragma once
#include <array>
#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <string_view>

namespace cvr::hud {
struct ElementDefinition { const char* name; const char* label; const char* key; };
inline constexpr ElementDefinition Elements[] = {
    {"player_health_bar","Health","health"}, {"activity_log","Activity log","activity"},
    {"bottom_hints","Button hints","hints"}, {"ammo_counter","Weapon / ammo","ammo"},
    {"warning","Warning","warning"}, {"cursor_device","Device cursor","device_cursor"},
    {"dpad_hint","Quick slots","quick_slots"}, {"boss_healthbar","Boss health","boss"},
    {"hud_progress_bar","Progress bar","progress"}, {"oxygenbar","Oxygen","oxygen"},
    {"minimap","Minimap","minimap"}, {"wanted_bar","Wanted level","wanted"},
    {"minimap_q305","Alternate minimap","minimap_alternate"},
    {"vehicle_summon_notification","Vehicle summon","vehicle_summon"},
    {"quest_list","Quest tracker","quest"}, {"vehicle scan widget","Vehicle scan","vehicle_scan"},
    {"cooldown","Cooldown","cooldown"}, {"input_hint","Input prompt","input_prompt"},
    {"zone alert notification","Zone alert","zone_alert"}, {"staminabar","Stamina","stamina"},
    {"crouch_indicator","Crouch indicator","crouch"}, {"militech warning","Militech warning","militech"},
    {"new_phone","Phone / messages","phone"}, {"driver_combat_hud","Vehicle combat","vehicle_combat"},
    {"car hud","Vehicle dashboard","vehicle"}, {"dpad_hint_consumables","Consumables","consumables"},
    {"songbird_phone","Songbird audio call","songbird_audio"},
    {"songbird_holocall","Holocall","holocall"}, {"hud_courier_bar","Courier progress","courier"},
    {"radio_custom","Radio","radio"},
    {"subtitles","Subtitles","subtitles"},
    {"interactions_root","Dialogs / interactions","dialogs"},
    {"Notifications_Journal","Journal / area notifications","journal_notifications"},
    {"hud tank","Basilisk HUD","basilisk"},
    {"q304_dossier","Identity dossier","identity_dossier"},
    {"q304_imprint_active_indicator","Identity imprint","identity_imprint"},
    {"briefing_sequence_player","Story briefings / imprint","briefing_sequence"},
    {"scanner_details","Scanner data","scanner_data"},
    {"camera_hud","Surveillance camera HUD","surveillance"},
    {"scanner","Scanner reticle / progress","scanner_reticle"}
};
inline constexpr size_t ElementCount=std::size(Elements);
constexpr uint64_t NameHash(std::string_view name) {
    uint64_t hash=14695981039346656037ull;
    for (unsigned char c:name) { hash^=c; hash*=1099511628211ull; }
    return hash;
}
inline int ElementIndex(uint64_t name) {
    for(size_t i=0;i<ElementCount;++i) if(NameHash(Elements[i].name)==name) return static_cast<int>(i);
    return -1;
}
struct ElementSettings {
    float x=0, y=0; // percent of canvas width / height
    float scale=1, opacity=1;
    int visible=1;
};
using LayoutSettings=std::array<ElementSettings,ElementCount>;
struct ElementStatus { bool available=false; float x=0,y=0,width=0,height=0; };
using LayoutStatus=std::array<ElementStatus,ElementCount>;
struct Rect { float x=0,y=0,width=0,height=0; };
struct LayoutLatch {
    Rect last{},candidate{};
    bool valid=false, haveCandidate=false;
    static bool Same(Rect a,Rect b) {
        return std::abs(a.x-b.x)<0.05f && std::abs(a.y-b.y)<0.05f &&
               std::abs(a.width-b.width)<0.05f && std::abs(a.height-b.height)<0.05f;
    }
    Rect Update(Rect value,bool dirty) {
        // UI layout jobs may be between updating the slot and its parents.
        // Keep a committed rectangle until layout settles or repeats coherently.
        if(!valid || !dirty || (haveCandidate && Same(candidate,value))) {last=value;valid=true;}
        candidate=value;haveCandidate=true;return last;
    }
};
inline ElementSettings Sanitize(ElementSettings s) {
    s.x=std::isfinite(s.x)?std::clamp(s.x,-100.0f,100.0f):0;
    s.y=std::isfinite(s.y)?std::clamp(s.y,-100.0f,100.0f):0;
    s.scale=std::isfinite(s.scale)?std::clamp(s.scale,0.1f,3.0f):1;
    s.opacity=std::isfinite(s.opacity)?std::clamp(s.opacity,0.0f,1.0f):1;
    s.visible=s.visible!=0; return s;
}
inline Rect ApplyElement(Rect r, ElementSettings s,float canvasW,float canvasH) {
    s=Sanitize(s);
    // Scaling around the element centre keeps a size edit from shifting its anchor.
    float w=r.width*s.scale,h=r.height*s.scale;
    float x=r.x+(r.width-w)*0.5f+s.x*canvasW*0.01f;
    float y=r.y+(r.height-h)*0.5f+s.y*canvasH*0.01f;
    // KEEP THE EDIT ON THE CANVAS. Every sprite is drawn into one canvas of canvasW x canvasH, and
    // whatever falls outside it is simply not there: an element near an edge (the phone, the minimap)
    // scaled up around its centre grew past the edge and was cut, and a bigger scale only pushed more of
    // it out. The edited rect is now slid back inside, and shrunk to fit if it is larger than the canvas.
    //
    // The limit is the canvas JOINED WITH the element's own authored rect, so an element the game itself
    // places partly off-canvas (a slide-in animation, an off-screen parked widget) keeps that overhang:
    // with the defaults (scale 1, no offset) the result is exactly the authored rect, as before.
    if(canvasW>0 && canvasH>0) {
        const float minX=std::min(0.0f,r.x),minY=std::min(0.0f,r.y);
        const float maxX=std::max(canvasW,r.x+r.width),maxY=std::max(canvasH,r.y+r.height);
        const float boundW=maxX-minX,boundH=maxY-minY;
        if(w>boundW || h>boundH) {
            const float k=std::min(boundW/w,boundH/h);
            const float cx=x+w*0.5f,cy=y+h*0.5f;
            w*=k;h*=k;x=cx-w*0.5f;y=cy-h*0.5f;
        }
        x=std::clamp(x,minX,maxX-w);
        y=std::clamp(y,minY,maxY-h);
    }
    return {x,y,w,h};
}
LayoutSettings GetLayoutSettings();
void SetLayoutSettings(const LayoutSettings& settings);
LayoutStatus GetLayoutStatus();
void SetLayoutStatus(const LayoutStatus& status);
bool ParseLayoutSetting(const char* line,LayoutSettings& settings);
void SaveLayoutSettings(FILE* file,const LayoutSettings& settings);
}
