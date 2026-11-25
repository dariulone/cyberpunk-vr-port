#pragma once
#include "Overlay/VrInteraction.hpp"
#include <string>
#include <vector>

namespace cvr::vrui {
Settings GetSettings();
void SetSettings(Settings);
bool ParseSetting(const char*,Settings&);
void WriteSettings(void*,const Settings&);
bool Visible();
bool CapturesInput();
void SetExternalCapture(bool);
void Toggle();
// CF right pink-wrist press, or other controllers' L3+R3 hold: ask the world menu
// to open/close. F10/Insert remains the separate settings overlay.
bool ConsumeMenuSummon();
bool ConsumeHudToggle();
void SelectMenuItem(unsigned);
void Close(bool resume=true);
void Recenter();
bool ConsumePlacementSave();
void UpdateTracking(const Tracking&);
int PollCommand(); // 1 open panel, 2 close panel, 100+ selected game action
void BridgeUpdate(bool ready,bool inGame,const char* labels);
std::vector<std::string> MenuItems();
struct PointerEvent {float x=-1,y=-1,wheel=0;bool down=false;int direction=0;bool activate=false,focus=false,secondary=false,back=false;};
int ConsumeTabStep();
std::vector<PointerEvent> ConsumePointerEvents();
struct View {XrPosef pose{{0,0,0,1},{}};XrVector3f rayStart{},rayEnd{},headPosition{};float width{},height{},hold{};Hit hit{};bool valid=false,ray=false,dragging=false,cyberfinger=false;};
View GetView();
}
