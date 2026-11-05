#pragma once
#include <cstdint>
#include <openxr/openxr.h>
struct VRControllerState;
namespace cvr::swimming {
struct Input { float forward{};bool water{};float ascend{};bool dive{};bool boost{}; };
// Water posture is independent of the gesture switch and controller tracking.
bool Active();
void BindNativeInput(void* locomotion);
float NativeAction(int action);
bool ReadSteering(XrVector3f* direction,int* state);
void PublishState(uintptr_t player,int state,int fast,bool paused,bool waterContext);
void ResetInput();
Input UpdateInput(const VRControllerState& controllers,bool manualBack);
bool SprintInput(bool wanted);
float DebugValue(int index);
// The gesture value a state machine's script read of `name` (a CName hash) gets added, 0 for none: only the
// player's machines (`owner`, compared, never followed) and only MoveY/ToggleSprint/Jump/Dive (Dive alone for a
// just-pressed read).
float ScriptAction(uintptr_t owner,uint64_t name,bool justPressed);
// The player GetPlayer() gave the Swimming mod's last state publish (every frame), as an instance address.
bool IsPlayer(uintptr_t owner);
// Those reads replaced natively (src/Natives/SwimmingScript.cpp): before the Lua mods' onInit, once.
bool InstallScriptHooks();
bool ScriptHooksActive();
}
