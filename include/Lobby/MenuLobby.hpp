#pragma once
#include <cstdint>

// THE MAIN MENU AS A PLACE. Under engine stereo the main menu is shown inside the world it already has (the main menu
// world, base\worlds\04_main_menu, with the menu's own player puppet) instead of over its 2D backdrop:
//  - the engine keeps rendering that world behind the menu (worlduiRuntimeSystemUI +0x264, the switch the HDR
//    calibration screen's SetRenderGameInBackground writes; without it the world pauses ~140 frames after the menu
//    opens, sub_1404C32AC);
//  - the composition gives the eyes that world instead of the menu's backdrop texture (the inkPreGameMenuState preset
//    of composition_params.inkfullscreencomposition: mainRenderFactor 0, backgroundTextureFactor 1);
//  - the menu's own pictures, the pyramid shot and its videos, are stopped (mods/redscript/CyberpunkVRPort_EngineStereo)
//    and the game's main menu is out of view, cursor included (mods/cet/CyberpunkVRPort_Lobby);
//  - the player standing in that world gets the game's own input: gameplay input on, the main menu's UI input
//    context set aside, so the pad walks and turns the puppet through the game's locomotion;
//  - the world layer every in-world screen is drawn through is switched on -- the main menu leaves it inactive
//    (sub_140607414) -- so the port's world-space UI works there.
namespace cvr::lobby {
// xr_lobby, default on.
bool Enabled();
void SetEnabled(bool);
// xr_lobby_ambient, default on: the lobby's ambience plays. While the lobby can be the main menu the game's own menu
// music is held (it does not start under the intro); the lobby plays its own sounds.
bool Ambient();
void SetAmbient(bool);
// Engine stereo, the main menu up, xr_lobby on, not stepped aside.
bool Active();
// The lobby steps aside for one of the game's own main menu screens (new game, settings: mods/cet/
// CyberpunkVRPort_Lobby leaves the menu's root for them): the menu is the game's again until the script takes it
// back. Never outlives the main menu: the next one starts with the lobby up (Tick).
void Suspend(bool);
bool Suspended();
// The main menu's controller came up (main thread, Natives/EngineStereoMenu.cpp): the UI system is looked up here.
void NoteMainMenu();
// The runtime UI system's menu flag (+0x128, Hooks/MenuMode.cpp) is the pause the render switch above lifts: while the
// lobby holds that switch on, the game writes 0 there although the main menu is still open. True for that system then.
bool HoldsRender(const void* runtimeUi);
// The main menu's world is being composed into the eyes right now (Active, the render held on, a composition block
// given the world within the last half second). The menu's display gates let the world through as a scene then:
// both eyes rendered, split into the eyes and head-tracked, instead of one picture on the menu panel.
bool WorldShown();
// THE HEADSET SHOWS NOTHING ON THE WAY INTO THE LOBBY, AND OUT OF IT. The eyes are shut the way the VR menu's blink
// shuts them (Runtimes/OpenXRFrameLoop.cpp):
//  - for every frame presented while a loading screen is up outside the main menu (Stereo/EngineStereo.hpp
//    LoadingShown; in the main menu the loading layer stays active with nothing on it) before the main menu has been
//    up in this launch: the main menu's world loads under one before any script runs, and the start is the intro's,
//    in the dark. Later loading screens are hung in that dark instead (LoadingInSpace). Asked when the frame is
//    captured and carried with it (Runtimes/OpenXRCapture.cpp): a load stalls for longer than the layer is known to
//    be up between two of its ticks, and the frame loop submits the last frame again meanwhile. And from the moment
//    the main menu closes under the lobby (a save chosen, a new game begun: NoteMainMenuClosed) until the loading
//    screen is up -- the frames between are the lobby's world, no longer composed into the eyes, on the menu panel.
//    At most 5 s: a menu that closes without a load (the game quits) leaves nothing to wait for.
bool LoadingDark();
// A loading screen up outside the main menu once the main menu has been up in this launch -- a save loading, a new
// game, a fast travel, the way back after a death -- is hung in the dark before the player (Render/LoadingScreen.hpp)
// rather than shutting the eyes (CyberpunkVR_LoadingInSpace 0 shuts them, as before).
bool LoadingInSpace();
// The main menu's controller was uninitialised (main thread, Natives/EngineStereoMenu.cpp), before engine stereo
// hears of it.
void NoteMainMenuClosed();
//  - while the lobby comes up (the curtain, 0 open .. 1 shut): the main menu is up before its world is ready and the
//    player is in place -- for that while the headset would show the menu's window, then the hall from wherever the
//    player first stood -- so from the moment the main menu comes up (NoteMainMenu) the eyes are shut until the
//    Lobby mod opens them (VRLobbyCurtain) -- the first time in a launch through its intro (Lobby/Intro.hpp), the
//    curtain drawn into the frame over the world (Render/IntroOverlay.hpp). Open whenever the lobby is not Active
//    (switched off, stepped aside). A script that never answers does not keep them shut: 20 s after the menu came up
//    without a word they open.
float Curtain();
void SetCurtain(float closure);
// Once per game Present: keeps the engine rendering the world behind the main menu while Active, and hands the switch
// back to the game when the menu goes or the lobby is switched off.
void Tick();
// The composition block (432 bytes, already a writable copy; Stereo/Grading.cpp FlattenHudComposite): the main
// menu's world in place of its backdrop texture. True when it changed the block.
bool ComposeWorld(float* block);
// Every resource the frame graph resolves (Stereo/SyncStereo.cpp): the composition node's inputs are recorded, the
// menu's UI among them, for the menu panel.
void ObserveResolve(uint32_t logical, uint32_t physical);
}  // namespace cvr::lobby
