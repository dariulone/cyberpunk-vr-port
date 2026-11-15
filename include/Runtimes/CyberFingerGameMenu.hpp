#pragma once
#include "Overlay/VrInteraction.hpp"
#include <cstdint>
namespace cvr::cfmenu {
struct Snapshot {
    bool active{},hit{},down{},secondary{},focus{};
    float u{},v{},zoom{};uint16_t buttons{};uint64_t tick{};
    XrVector3f start{},end{},head{};
};
// XR thread publishes the same tracked hands and exact quad as the compositor.
void Hands(const cvr::vrui::Tracking&,bool enabled);
void Panel(const XrPosef&,float width,float height);
XrPosef PanelPose(const XrPosef&,uint64_t origin);
Snapshot Current();
void Reset();
// Game-thread delivery, limited to the game's foreground window. Cursor hooks
// use its full render-space coordinate, including regions outside the PC crop.
void DispatchPointer();
bool Cursor(float& u,float& v);
}
