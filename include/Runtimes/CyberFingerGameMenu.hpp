#pragma once
#include "Overlay/VrInteraction.hpp"
#include <cstdint>
namespace cvr::cfmenu {
struct Snapshot {
    bool active{},hit{},down{},secondary{},focus{},ray{};
    float u{},v{},zoom{};uint16_t buttons{};uint64_t tick{};
    XrVector3f start{},end{},head{};
};
// XR thread publishes the same tracked hands and exact quad as the compositor.
void Hands(const cvr::vrui::Tracking&,bool enabled);
void Panel(const XrPosef&,float width,float height);
XrPosef PanelPose(const XrPosef& fallback,uint64_t origin,const cvr::vrui::Settings& = {});
XrExtent2Df PanelSize(XrExtent2Df legacy,const cvr::vrui::Settings&);
Snapshot Current();
void Reset();
// Temporarily yield to an overlay without forgetting the native panel pose.
void SuspendInput();
// Physical mouse activity yields the pointer until a deliberate controller
// action. Tagged laser-generated Win32 messages never claim desktop ownership.
inline constexpr uintptr_t PointerTag=0x43565250;
void DesktopMouse(uint32_t message,int x,int y,uintptr_t extraInfo);
// Game-thread delivery, limited to the game's foreground window. Cursor hooks
// use its full render-space coordinate, including regions outside the PC crop.
void DispatchPointer();
bool Cursor(float& u,float& v);
}
