#if !CVR_RUST_VRIK   // the Rust build compiles nothing of this file: what it defined is Rust's
#include "Overlay/OverlayInternal.hpp"
#include "Overlay/VrOverlay.hpp"
#include "Overlay/VrWidgets.hpp"
#include "Overlay/UiRec.hpp"

// Tools: the VR shell's tab to select on its next frame, then -1 (Overlay/ImGuiOverlay.cpp's draw-data dump).
#if !CVR_RUST_VRIK
extern "C" int32_t CyberpunkVR_VrShellTab;
#endif

#if CVR_RUST_VRIK
// THE VR SHELL AND ITS STYLE ARE RUST'S (rust/crates/cvr-render/src/vr_style.rs), ImGui reached through
// Overlay/ImGuiAbi.cpp.
#else
namespace overlay {
namespace {
const ImVec4 Red{1.0f,.28f,.29f,1}, Yellow{.98f,.94f,.06f,1}, Cyan{.30f,.88f,.94f,1};
bool AngularButton(const char* label,ImVec2 size,bool selected=false,bool left=false){
    const auto cursor=ImGui::GetCursorScreenPos();
    const bool pointed=ImGui::IsMouseHoveringRect(cursor,{cursor.x+size.x,cursor.y+size.y});
    ImGui::PushStyleColor(ImGuiCol_Text,selected || pointed?Yellow:Red);
    ImGui::PushStyleColor(ImGuiCol_Button,ImVec4(.10f,.025f,.035f,selected?.8f:0));
    ImGui::PushStyleColor(ImGuiCol_ButtonHovered,ImVec4(.19f,.09f,.025f,1));
    ImGui::PushStyleColor(ImGuiCol_ButtonActive,ImVec4(.28f,.14f,.025f,1));
    if(left){ImGui::PushStyleVar(ImGuiStyleVar_ButtonTextAlign,ImVec2(.06f,.5f));ImGui::PushStyleVar(ImGuiStyleVar_FrameBorderSize,0);}
    const bool clicked=ImGui::Button(label,size);
    if(left)ImGui::PopStyleVar(2);
    const bool hot=ImGui::IsItemHovered() || ImGui::IsItemActive();
    const auto a=ImGui::GetItemRectMin(),b=ImGui::GetItemRectMax();
    auto* d=ImGui::GetWindowDrawList();
    const ImU32 color=ImGui::ColorConvertFloat4ToU32(hot || selected?Yellow:ImVec4(.48f,.13f,.16f,.75f));
    const ImVec2 points[]={{a.x,a.y},{b.x,a.y},{b.x,b.y-10},{b.x-10,b.y},{a.x,b.y}};
    if(!left || hot || selected)d->AddPolyline(points,5,color,ImDrawFlags_Closed,hot || selected?2.0f:1.0f);
    if(hot || selected)d->AddRectFilled({a.x,a.y+8},{a.x+3,b.y-8},color);
    ImGui::PopStyleColor(4);return clicked;
}
}
// The OVERLAY tab, shared with the VR menu's record pass (Overlay/UiRec.hpp).
bool DrawOverlayPage(LiveControlsUiState& state){
    using namespace cvr::vrui;
    auto button=[](const char* label,ImVec2 size){
        return cvr::uirec::Recording()?cvr::uirec::Button(label):AngularButton(label,size);
    };
    bool changed=false;
    auto& v=state.overlay;
    widgets::Section("OVERLAY PANEL");
    changed|=widgets::SliderFloat("Distance",&v.distance,v.minDistance,v.maxDistance,"%.2f m");
    changed|=widgets::SliderFloat("Minimum distance",&v.minDistance,.35f,2,"%.2f m");
    changed|=widgets::SliderFloat("Maximum distance",&v.maxDistance,v.minDistance+.1f,5,"%.2f m");
    changed|=widgets::SliderFloat("Panel size",&v.scale,.6f,1.5f,"%.2fx");
    changed|=widgets::SliderFloat("Text size",&v.fontScale,.8f,1.4f,"%.2fx");
    changed|=widgets::Combo("Pointer hand",&v.hand,"Left\0Right\0Automatic\0");
    changed|=UI::Checkbox("Free look",&v.follow);
    changed|=widgets::SliderFloat("Free-look cone",&v.cone,5,90,"%.0f deg");
    changed|=widgets::SliderFloat("Distance speed",&v.depthSpeed,.1f,2,"%.2f m/s");
    if(button("CENTER PANEL",{260,60})){Recenter();v=GetSettings();changed=true;}
    UI::SameLine();if(button("RESET",{200,60})){v={};SetSettings(v);Recenter();changed=true;}
    UI::TextWrapped("Catch up after 3 seconds at rest when the offset is over 10 degrees inside the cone.");
    widgets::Section("FPS OVERLAY");
    changed|=DrawFpsOverlayControls(state);
    return changed;
}
void ApplyVrStyle(){
    auto& s=ImGui::GetStyle();s=ImGuiStyle{};
    s.WindowPadding={24,22};s.FramePadding={12,10};s.ItemSpacing={14,12};s.ItemInnerSpacing={12,10};
    s.WindowRounding=s.ChildRounding=s.FrameRounding=s.PopupRounding=0;
    s.ScrollbarSize=26;s.GrabMinSize=30;s.WindowBorderSize=0;s.ChildBorderSize=0;s.PopupBorderSize=1;s.FrameBorderSize=1;
    s.TouchExtraPadding={3,3};s.IndentSpacing=28;
    auto* c=s.Colors;
    c[ImGuiCol_Text]={1,.41f,.42f,1};c[ImGuiCol_TextDisabled]={.57f,.29f,.33f,1};
    c[ImGuiCol_WindowBg]={.027f,.032f,.055f,.99f};c[ImGuiCol_ChildBg]={.043f,.033f,.053f,.98f};
    c[ImGuiCol_PopupBg]={.055f,.025f,.04f,1};c[ImGuiCol_Border]={.48f,.12f,.17f,.85f};
    c[ImGuiCol_FrameBg]={.12f,.027f,.043f,1};c[ImGuiCol_FrameBgHovered]={.23f,.065f,.075f,1};c[ImGuiCol_FrameBgActive]={.28f,.10f,.05f,1};
    c[ImGuiCol_Button]=c[ImGuiCol_FrameBg];c[ImGuiCol_ButtonHovered]=c[ImGuiCol_FrameBgHovered];c[ImGuiCol_ButtonActive]=c[ImGuiCol_FrameBgActive];
    c[ImGuiCol_CheckMark]=Cyan;c[ImGuiCol_SliderGrab]=Red;c[ImGuiCol_SliderGrabActive]=Yellow;
    c[ImGuiCol_Header]={.16f,.035f,.055f,1};c[ImGuiCol_HeaderHovered]={.26f,.08f,.09f,1};c[ImGuiCol_HeaderActive]=c[ImGuiCol_HeaderHovered];
    c[ImGuiCol_Separator]={.52f,.12f,.16f,1};c[ImGuiCol_ScrollbarBg]={.035f,.018f,.025f,1};c[ImGuiCol_ScrollbarGrab]={.62f,.14f,.18f,1};
    c[ImGuiCol_TextSelectedBg]={.36f,.12f,.08f,1};
    c[ImGuiCol_TableBorderLight]={.43f,.13f,.17f,.55f};
    c[ImGuiCol_TableBorderStrong]={.25f,.72f,.80f,.65f};
}
bool DrawVrShell(LiveControlsUiState& state,bool& foveatedEditing){
    using namespace cvr::vrui;
    static int selected=6;
    if(CyberpunkVR_VrShellTab>=0 && CyberpunkVR_VrShellTab<7){selected=CyberpunkVR_VrShellTab;CyberpunkVR_VrShellTab=-1;}
    const char* tabs[]={"GENERAL","FRAMEGEN","HUD","CONTROLS","STEREO","AVATAR","OVERLAY"};
    ImGui::SetNextWindowPos({0,0});ImGui::SetNextWindowSize({CanvasWidth,CanvasHeight});
    ImGui::Begin("##VR controls",nullptr,ImGuiWindowFlags_NoDecoration|ImGuiWindowFlags_NoMove|ImGuiWindowFlags_NoSavedSettings);
    auto* background=ImGui::GetWindowDrawList();
    background->AddRectFilledMultiColor({12,12},{358,CanvasHeight-90},IM_COL32(60,10,18,130),IM_COL32(22,12,22,60),IM_COL32(14,10,18,60),IM_COL32(46,6,14,100));
    background->AddLine({12,12},{12,CanvasHeight-91},IM_COL32(242,70,75,240),2);
    ImGui::TextColored(Yellow,"cyberpunk-vr-port");
    ImGui::SameLine(1170);
    if(AngularButton("CLOSE##close_overlay",{220,55}))Close();
    {
        const auto a=ImGui::GetItemRectMin(),b=ImGui::GetItemRectMax();
        const ImVec2 center{b.x-25,(a.y+b.y)*.5f};
        const auto color=ImGui::ColorConvertFloat4ToU32(ImGui::IsItemHovered()?Yellow:Red);
        background->AddLine({center.x-7,center.y-7},{center.x+7,center.y+7},color,2.5f);
        background->AddLine({center.x-7,center.y+7},{center.x+7,center.y-7},color,2.5f);
    }
    ImGui::Separator();
    ImGui::BeginChild("game-actions",{335,-78},false);
    ImGui::PushStyleVar(ImGuiStyleVar_ItemSpacing,ImVec2(14,8));
    const auto items=MenuItems();
    for(size_t i=0;i<items.size();++i){
        ImGui::PushID(int(i));
        const float fit=std::min(1.0f,285.0f/std::max(1.0f,ImGui::CalcTextSize(items[i].c_str()).x));
        ImGui::SetWindowFontScale(fit);
        if(AngularButton(items[i].c_str(),{311,60},false,true))SelectMenuItem(unsigned(i));
        ImGui::SetWindowFontScale(1);
        ImGui::PopID();
    }
    ImGui::PopStyleVar();ImGui::EndChild();ImGui::SameLine();
    ImGui::BeginChild("vr-settings",{0,-78},false);
    float occupied=0;const float available=ImGui::GetContentRegionAvail().x;
    for(int i=0;i<7;++i){
        const float width=ImGui::CalcTextSize(tabs[i]).x+30;
        if(i && occupied+14+width<=available){ImGui::SameLine();occupied+=14;}
        else occupied=0;
        if(AngularButton(tabs[i],{width,54},i==selected))selected=i;
        occupied+=width;
    }
    ImGui::Separator();
    ImGui::BeginChild("content",{0,0},false);ImGui::PushItemWidth(360);
    bool changed=false;
    if(selected==6)changed=DrawOverlayPage(state);
    else changed=DrawLiveControls(state,selected,&foveatedEditing);
    ImGui::PopItemWidth();ImGui::EndChild();ImGui::EndChild();ImGui::Separator();
    ImGui::TextColored(Cyan,"TRIGGER  Select     STICK  Scroll     GRIP  Drag     GRIP + STICK  Distance");
    ImGui::TextDisabled("F10 / INSERT  Toggle     ESC  Close     L3 + R3  Hold 0.5 sec  VR menu");
    ImGui::End();
    auto* draw=ImGui::GetForegroundDrawList();
    draw->AddRectFilled({0,0},{4,4},IM_COL32(76,224,240,255));
    const auto p=ImGui::GetIO().MousePos;
    if(p.x>=0 && p.x<CanvasWidth && p.y>=0 && p.y<CanvasHeight){
        const auto color=GetView().dragging?IM_COL32(252,240,16,255):IM_COL32(76,224,240,255);
        const ImVec2 a=p,b{p.x+23,p.y+8},c{p.x+11,p.y+13};
        draw->AddTriangleFilled(a,b,c,IM_COL32(3,18,25,240));draw->AddTriangle(a,b,c,color,2);
    }
    return changed;
}
}
#endif
#endif
