#pragma once

#include <Windows.h>

// THE PLUGIN'S LUA HOST (stage 6 of docs/native-frameworks-plan-20261030.md): the port's Lua mods run here instead of
// in Cyber Engine Tweaks, on CET's scripting core (src/Lua/cet, ported from CyberEngineTweaks v1.38.0, MIT) with
// the plugin in place of CET's program: mods in the plugin's lua\ folder, the same API, the same events.
//
// Lifecycle, as in CET: the Lua state and its API are made as the engine loads the scripts' bytecode, onTweak as
// it loads the TweakDB, the mods load and onInit fires when the player is first spawned (the main menu's puppet
// included), onUpdate on the game's main thread each running tick, onShutdown as the game shuts down. onDraw and
// onOverlayOpen / onOverlayClose come from the plugin's overlay.
namespace cvr::lua {

// The runtime's own check: a Lua state made, a chunk run, the result read back. Logged.
bool SelfTest();

// At plugin load: the host is made and hooks the engine's script load, TweakDB load, player spawn and main thread.
void Install();

// The plugin's window procedure, before anything else sees the message: the mods' key binds and the overlay key
// (raw input).
void OnWndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam);

// The mods' windows are open: they take the mouse and keyboard.
bool OverlayOpen();

// Inside the plugin's desktop ImGui frame (render thread), with the output's size: the mods' windows while the
// overlay is open (onDraw).
void Draw(float width, float height);

}  // namespace cvr::lua
