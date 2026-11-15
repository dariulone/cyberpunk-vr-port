#pragma once
#include "Anim/FaceInput.hpp"
#include <cstdint>
namespace RED4ext::ent { struct Entity; }
namespace cvr::face {
struct Settings { bool enabled{},mouth{true},blink{true},eyes{true},brows{}; int port{9015}; };
struct Status {
    bool listening{},hooks{},bound{};
    int socketError{},saveError{};
    unsigned fresh{},mapped{};
    uint64_t packets{},malformed{},ownerPasses{},facialPasses{},writes{},faults{},rejected{};
};
Settings GetSettings();
void SetSettings(Settings settings);
Input GetInput();
Status GetStatus();
void Tick();                         // Game/Lua update thread; bounded nonblocking socket pump.
void Shutdown();
void BindMirror(RED4ext::ent::Entity* entity);
void ClearMirror();
bool PreserveMirrorBone(unsigned index);
void DrawSettings();
bool InstallHooks();
Status RigStatus();
}
