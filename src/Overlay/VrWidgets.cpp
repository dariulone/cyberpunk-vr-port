#include "Overlay/VrWidgets.hpp"
#include "Overlay/UiRec.hpp"
#include <imgui_internal.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <vector>

namespace overlay::widgets {
namespace {
const ImVec4 Red{1,.28f,.29f,1}, Cyan{.30f,.88f,.94f,1}, Yellow{.98f,.94f,.06f,1};
void Outline(ImDrawList* draw,ImVec2 a,ImVec2 b,ImU32 color,bool fill=false) {
    const float cut=std::min(10.0f,(b.y-a.y)*.22f);
    const ImVec2 points[]={{a.x,a.y},{b.x,a.y},{b.x,b.y-cut},{b.x-cut,b.y},{a.x,b.y}};
    if(fill)draw->AddConvexPolyFilled(points,5,color);
    else draw->AddPolyline(points,5,color,ImDrawFlags_Closed,1.2f);
}
void CenterText(ImDrawList* draw,ImVec2 a,ImVec2 b,const char* text,ImU32 color) {
    const auto size=ImGui::CalcTextSize(text);
    const float fit=std::min(1.f,std::max(1.f,b.x-a.x-12)/std::max(1.f,size.x));
    draw->PushClipRect(a,b,true);
    draw->AddText(ImGui::GetFont(),ImGui::GetFontSize()*fit,
        {a.x+std::max(4.0f,(b.x-a.x-size.x*fit)*.5f),a.y+(b.y-a.y-size.y*fit)*.5f},color,text);
    draw->PopClipRect();
}
struct Field {
    ImVec2 a,b;
    ImDrawList* draw;
    float height,arrow,width;
    explicit Field(const char* label) {
        ImGui::BeginGroup();ImGui::PushID(label);
        const ImVec2 start=ImGui::GetCursorScreenPos();
        const float available=std::max(160.0f,ImGui::GetContentRegionAvail().x);
        const char* end=ImGui::FindRenderedTextEnd(label);
        const bool named=end!=label;
        const float labelWidth=named?available*.40f:0;
        const float gap=named?ImGui::GetStyle().ItemInnerSpacing.x:0;
        const auto size=named?ImGui::CalcTextSize(label,end,false,std::max(1.0f,labelWidth-8)):ImVec2{};
        height=std::max(44.0f,ImGui::GetFrameHeight());
        const float rowHeight=std::max(height,size.y+8);
        draw=ImGui::GetWindowDrawList();
        if(named)draw->AddText(ImGui::GetFont(),ImGui::GetFontSize(),{start.x,start.y+(rowHeight-size.y)*.5f},
            ImGui::GetColorU32(Red),label,end,labelWidth-8);
        ImGui::Dummy({labelWidth,rowHeight});
        a={start.x+labelWidth+gap,start.y+(rowHeight-height)*.5f};
        width=std::max(100.0f,available-labelWidth-gap);b={a.x+width,a.y+height};
        arrow=std::min(height,width*.18f);
        Outline(draw,a,b,ImGui::GetColorU32(ImVec4(.065f,.025f,.040f,.9f)),true);
        Outline(draw,a,b,ImGui::GetColorU32(ImVec4(.62f,.18f,.21f,1)));
    }
    ~Field(){ImGui::PopID();ImGui::EndGroup();}
    bool Arrow(bool right,bool enabled=true) {
        const ImVec2 p{right?b.x-arrow:a.x,a.y};
        ImGui::SetCursorScreenPos(p);
        ImGui::PushStyleColor(ImGuiCol_Button,{0,0,0,0});
        ImGui::PushStyleColor(ImGuiCol_ButtonHovered,{.10f,.24f,.27f,.4f});
        ImGui::PushStyleColor(ImGuiCol_ButtonActive,{.16f,.32f,.34f,.7f});
        ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize,0);
        ImGui::BeginDisabled(!enabled);ImGui::PushButtonRepeat(true);
        const bool clicked=ImGui::Button(right?"##next":"##previous",{arrow,height});
        ImGui::PopButtonRepeat();
        const ImU32 color=ImGui::GetColorU32(ImGui::IsItemActive()?Yellow:Cyan);
        const float x=p.x+arrow*.5f,y=p.y+height*.5f,s=std::min(9.0f,height*.16f),sign=right?1.f:-1.f;
        draw->AddTriangle({x+sign*s,y},{x-sign*s,y-s},{x-sign*s,y+s},color,1.4f);
        ImGui::EndDisabled();ImGui::PopStyleVar();ImGui::PopStyleColor(3);
        return clicked;
    }
    ImVec2 TrackMin() const{return {a.x+arrow,a.y};}
    ImVec2 TrackMax() const{return {b.x-arrow,b.y};}
};
template<class T> bool Slider(const char* label,T* value,T minimum,T maximum,const char* format,ImGuiSliderFlags flags,ImGuiDataType type) {
    Field field(label);
    const int precision=type==ImGuiDataType_S32?0:std::clamp(ImParseFormatPrecision(format,3),0,6);
    const T step=type==ImGuiDataType_S32?T(1):T(std::pow(10.0,-precision));
    bool changed=false;
    if(field.Arrow(false,*value>minimum)) {const T next=std::clamp(T(*value-step),minimum,maximum);changed=next!=*value;*value=next;}
    const auto a=field.TrackMin(),b=field.TrackMax();
    char text[96]{},low[96]{},high[96]{};
    ImGui::DataTypeFormatString(low,sizeof(low),type,&minimum,format);
    ImGui::DataTypeFormatString(high,sizeof(high),type,&maximum,format);
    const float grab=std::min(std::max(ImGui::CalcTextSize(low).x,ImGui::CalcTextSize(high).x)+24,std::max(24.f,b.x-a.x-4));
    field.draw->AddLine({a.x+2,(a.y+b.y)*.5f},{b.x-2,(a.y+b.y)*.5f},ImGui::GetColorU32(ImVec4(.44f,.12f,.16f,1)),2);
    ImGui::SetCursorScreenPos(a);ImGui::SetNextItemWidth(b.x-a.x);
    const auto id=ImGui::GetID("##value");
    const bool editing=ImGui::TempInputIsActive(id) || (ImGui::GetIO().KeyCtrl && ImGui::IsMouseHoveringRect(a,b));
    ImGui::PushStyleVar(ImGuiStyleVar_GrabMinSize,grab);
    ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize,0);
    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,ImVec2(4,(field.height-ImGui::GetFontSize())*.5f));
    ImGui::PushStyleColor(ImGuiCol_FrameBg,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_FrameBgHovered,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_FrameBgActive,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_SliderGrab,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_SliderGrabActive,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_Text,editing?Cyan:ImVec4(0,0,0,0));
    changed|=ImGui::SliderScalar("##value",type,value,&minimum,&maximum,format,flags|ImGuiSliderFlags_AlwaysClamp);
    const bool hot=ImGui::IsItemHovered() || ImGui::IsItemActive();
    ImGui::PopStyleColor(6);ImGui::PopStyleVar(3);
    if(!ImGui::TempInputIsActive(id)) {
        const float ratio=maximum>minimum?std::clamp(float(double(*value-minimum)/double(maximum-minimum)),0.f,1.f):0;
        const float usable=std::max(0.f,b.x-a.x-4-grab);
        const ImVec2 left{a.x+2+usable*ratio,a.y+3},right{left.x+grab,b.y-3};
        Outline(field.draw,left,right,ImGui::GetColorU32(ImVec4(.15f,.035f,.05f,1)),true);
        Outline(field.draw,left,right,ImGui::GetColorU32(hot?Yellow:Red));
        ImGui::DataTypeFormatString(text,sizeof(text),type,value,format);
        CenterText(field.draw,left,right,text,ImGui::GetColorU32(Cyan));
    }
    if(field.Arrow(true,*value<maximum)) {const T next=std::clamp(T(*value+step),minimum,maximum);changed|=next!=*value;*value=next;}
    return changed;
}
}
// Each widget is also an item of the VR menu's record pass (Overlay/UiRec.hpp): recording, it hands its
// label, range and value to the recorder instead of drawing.
bool SliderFloat(const char* label,float* v,float lo,float hi,const char* format,ImGuiSliderFlags flags){
    if(cvr::uirec::Recording())return cvr::uirec::SliderFloat(label,v,lo,hi,format);
    return Slider(label,v,lo,hi,format,flags,ImGuiDataType_Float);
}
bool SliderInt(const char* label,int* v,int lo,int hi,const char* format,ImGuiSliderFlags flags){
    if(cvr::uirec::Recording())return cvr::uirec::SliderInt(label,v,lo,hi,format);
    return Slider(label,v,lo,hi,format,flags,ImGuiDataType_S32);
}
bool Combo(const char* label,int* selected,const char* const* items,int count,int) {
    if(cvr::uirec::Recording())return cvr::uirec::Combo(label,selected,items,count);
    Field field(label);bool changed=false;
    if(field.Arrow(false,count>0 && *selected>0)){*selected=std::clamp(*selected-1,0,count-1);changed=true;}
    const auto a=field.TrackMin(),b=field.TrackMax();
    ImGui::SetCursorScreenPos(a);ImGui::SetNextItemWidth(b.x-a.x);
    ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize,0);
    ImGui::PushStyleVar(ImGuiStyleVar_FramePadding,ImVec2(10,(field.height-ImGui::GetFontSize())*.5f));
    ImGui::PushStyleColor(ImGuiCol_FrameBg,{0,0,0,0});
    ImGui::PushStyleColor(ImGuiCol_FrameBgHovered,{.10f,.24f,.27f,.25f});
    ImGui::PushStyleColor(ImGuiCol_FrameBgActive,{.10f,.24f,.27f,.35f});
    const bool open=ImGui::BeginCombo("##list","",ImGuiComboFlags_NoArrowButton|ImGuiComboFlags_HeightLarge);
    ImGui::PopStyleColor(3);ImGui::PopStyleVar(2);
    if(open) {
        for(int i=0;i<count;++i) {
            ImGui::PushID(i);ImGui::PushStyleColor(ImGuiCol_Text,i==*selected?Yellow:Cyan);
            if(ImGui::Selectable(items[i],i==*selected,0,{0,std::max(44.f,ImGui::GetFrameHeight())})) {*selected=i;changed=true;}
            if(i==*selected)ImGui::SetItemDefaultFocus();
            ImGui::PopStyleColor();ImGui::PopID();
        }
        ImGui::EndCombo();
    }
    CenterText(field.draw,a,b,*selected>=0 && *selected<count?items[*selected]:"--",ImGui::GetColorU32(Cyan));
    if(field.Arrow(true,count>0 && *selected<count-1)){*selected=std::clamp(*selected+1,0,count-1);changed=true;}
    return changed;
}
bool Combo(const char* label,int* selected,const char* list,int height) {
    std::vector<const char*> items;
    for(const char* p=list;*p;p+=std::strlen(p)+1)items.push_back(p);
    return Combo(label,selected,items.data(),int(items.size()),height);
}
void Section(const char* label) {
    if(cvr::uirec::Recording()){cvr::uirec::Header(label,true);return;}
    ImGui::Spacing();ImGui::TextColored(Cyan,"%s",label);
    const auto p=ImGui::GetCursorScreenPos();
    ImGui::GetWindowDrawList()->AddLine(p,{p.x+ImGui::GetContentRegionAvail().x,p.y},ImGui::GetColorU32(ImVec4(.25f,.72f,.80f,.65f)));
    ImGui::Dummy({0,5});
}
void Tabs(const char* id,int& selected,const char* const* labels,int count) {
    if(cvr::uirec::Recording()){cvr::uirec::Tabs(id,selected,labels,count);return;}
    ImGui::PushID(id);const float width=(ImGui::GetContentRegionAvail().x-ImGui::GetStyle().ItemSpacing.x*(count-1))/count;
    for(int i=0;i<count;++i) {
        if(i)ImGui::SameLine();ImGui::PushID(i);
        ImGui::PushStyleColor(ImGuiCol_Text,selected==i?Yellow:Cyan);
        if(ImGui::Button(labels[i],{width,std::max(46.f,ImGui::GetFrameHeight())}))selected=i;
        if(selected==i){const auto a=ImGui::GetItemRectMin(),b=ImGui::GetItemRectMax();ImGui::GetWindowDrawList()->AddLine({a.x,b.y},{b.x,b.y},ImGui::GetColorU32(Yellow),2);}
        ImGui::PopStyleColor();ImGui::PopID();
    }
    ImGui::PopID();ImGui::Spacing();
}
void DrawBindings() {
    struct Binding { const char* input;const char* action; };
    const bool recording=cvr::uirec::Recording();
    auto group=[recording](const char* title,const Binding* rows,int count) {
        Section(title);
        if(recording){for(int i=0;i<count;++i)cvr::uirec::Binding(rows[i].input,rows[i].action);return;}
        ImGui::PushStyleVar(ImGuiStyleVar_CellPadding,ImVec2(12,12));
        if(ImGui::BeginTable(title,2,ImGuiTableFlags_SizingStretchProp|ImGuiTableFlags_BordersInnerH)) {
            ImGui::TableSetupColumn("INPUT",ImGuiTableColumnFlags_WidthStretch,.36f);
            ImGui::TableSetupColumn("ACTION",ImGuiTableColumnFlags_WidthStretch,.64f);
            for(int i=0;i<count;++i) {
                ImGui::TableNextRow(0,44);ImGui::TableNextColumn();
                ImGui::PushStyleColor(ImGuiCol_Text,Cyan);ImGui::TextWrapped("%s",rows[i].input);ImGui::PopStyleColor();
                ImGui::TableNextColumn();ImGui::TextWrapped("%s",rows[i].action);
            }
            ImGui::EndTable();
        }
        ImGui::PopStyleVar();
    };
    UI::TextWrapped("VR shortcuts and default game actions. Button names follow the Touch / Xbox convention; game actions use your controller bindings.");
    static const Binding foot[]{
        {"LEFT STICK","Move. Hold fully forward for 0.2 seconds to sprint."},
        {"RIGHT STICK  Left / right","Turn. Uses your snap-turn setting."},
        {"RIGHT STICK  Full up","Dash / dodge once per push."},
        {"RIGHT STICK  Full down","Crouch."},
        {"RIGHT STICK  Press","Rack the weapon / release the slide."},
        {"A  Right controller","Jump."},
        {"B  Right controller","With a weapon: release the magazine. Holstered: Back / close."},
        {"X  Left controller","Interact / reload."},
        {"Y  Left controller","Switch weapon."},
        {"RIGHT TRIGGER","Fire."},
        {"LEFT TRIGGER","Aim / block in melee."},
        {"RIGHT GRIP  At a holster","Equip or holster the weapon at the reached slot."},
        {"LEFT GRIP","Grab the magazine during a reload."},
        {"LEFT L3  Near left ear","Toggle the scanner with a fresh left-stick click. Release and click again to close. Always the physical left hand, including left-handed mode."},
        {"L3 + LEFT GRIP  Hold 0.5 seconds","Activate cyberware once. Release both to use again."},
        {"LEFT MENU BUTTON","Open the game's pause menu."}
    };
    group("ON FOOT",foot,IM_ARRAYSIZE(foot));
    static const Binding swimming[]{
        {"BOTH HANDS  Reach, sweep out, pull back","Swim in the direction you look. Movement starts during the pull; gentle strokes swim normally and fast strokes boost."},
        {"BOTH HANDS  Raise, then push down","Swim upward."},
        {"LEFT STICK  Back","Cancel gesture propulsion. Stick movement takes priority."}
    };
    group("SWIMMING",swimming,IM_ARRAYSIZE(swimming));
    static const Binding scanner[]{
        {"LEFT STICK  Full up / down","Page through quickhacks. Partial movement still walks."},
        {"X  Left controller","Apply the selected quickhack."},
        {"RIGHT TRIGGER","Tag the target while scanning."},
        {"RIGHT STICK  Press","Change scanner tab."},
        {"LEFT TRIGGER + RIGHT STICK","Adjust scanner zoom."}
    };
    group("SCANNER",scanner,IM_ARRAYSIZE(scanner));
    static const Binding dpad[]{
        {"Hold LEFT STICK press + RIGHT STICK","Push the right stick fully up, down, left or right to send that D-pad direction."},
        {"Release without a direction","Send the normal left-stick press (L3)."}
    };
    group("D-PAD SHORTCUT",dpad,IM_ARRAYSIZE(dpad));
    static const Binding driving[]{
        {"Hold X  Left controller","Exit the vehicle."},
        {"A  Right controller","Confirm a dialogue choice. The normal handbrake action remains available."},
        {"LEFT / RIGHT TRIGGER","Brake / throttle."},
        {"GRIP  At the wheel","Grab the wheel or handlebars with that hand. Release to let go."},
        {"GAME CAMERA","VR follows the camera chosen by the game, including third-person scenes. Driving and seated-camera options are in DRIVING."}
    };
    group("DRIVING",driving,IM_ARRAYSIZE(driving));
    static const Binding overlay[]{
        {"L3 + R3  Hold 0.5 second","Open or close the VR menu in front of you (as the right-hand swipe down does)."},
        {"Point + TRIGGER","Select a control."},
        {"STICK  Up / down","Scroll the pointed panel."},
        {"GRIP","Drag the panel."},
        {"GRIP + STICK  Up / down","Move the panel farther away / closer."},
        {"F10 / INSERT","Open or close this overlay from the keyboard. ESC closes it."}
    };
    group("OVERLAY",overlay,IM_ARRAYSIZE(overlay));
    UI::Spacing();UI::TextWrapped("Change game actions in Settings > Key Bindings > Controller. VR gesture settings are in CONTROLS and AVATAR.");
}
}
