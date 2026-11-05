// VREngineStereoMenuShown() / VREngineStereoMenuClosed(): the main menu's controller was initialised / uninitialised
// (mods/redscript/CyberpunkVRPort_EngineStereo). Engine stereo starts at the first and stays on; both say whether the
// main menu is up (Stereo/EngineStereo.hpp NoteMainMenu, NoteMainMenuClosed). Outside engine stereo they do nothing.
// VRLobbyActive(): the main menu is shown in its world (Lobby/MenuLobby.hpp), so the script hides the menu's backdrop.
// VRLobbySuspend(on): the lobby steps aside for one of the game's own main menu screens, and takes the menu back.
// VRLobbyCurtain(closure): how far the eyes are closed while the lobby comes up, 0 open .. 1 shut (MenuLobby.hpp Curtain).
// VRLobbyAmbient() -> Bool / VRSetLobbyAmbient(on): the lobby's ambience on (xr_lobby_ambient, MenuLobby.hpp Ambient),
// switched from the lobby menu's button and saved as the settings panel saves it.
#include "Natives/NativeFunctions.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include <RED4ext/Scripting/Utils.hpp>
void VREngineStereoMenuShown(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){++f->code;cvr::engine_stereo::NoteMainMenu();cvr::lobby::NoteMainMenu();}
void VREngineStereoMenuClosed(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){++f->code;cvr::lobby::NoteMainMenuClosed();cvr::engine_stereo::NoteMainMenuClosed();}
void VRLobbyActive(RED4ext::IScriptable*,RED4ext::CStackFrame* f,bool* out,int64_t){++f->code;if(out)*out=cvr::lobby::Active();}
void VRLobbySuspend(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){bool on=false;RED4ext::GetParameter(f,&on);++f->code;cvr::lobby::Suspend(on);}
void VRLobbyCurtain(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){float closure=0.0f;RED4ext::GetParameter(f,&closure);++f->code;cvr::lobby::SetCurtain(closure);}
void VRLobbyAmbient(RED4ext::IScriptable*,RED4ext::CStackFrame* f,bool* out,int64_t){++f->code;if(out)*out=cvr::lobby::Ambient();}
void VRSetLobbyAmbient(RED4ext::IScriptable*,RED4ext::CStackFrame* f,void*,int64_t){
    bool on=true;RED4ext::GetParameter(f,&on);++f->code;
    LiveControlsUiState state{};
    GetLiveControlsUiState(&state);
    state.lobbyAmbient=on?1:0;
    SetLiveControlsUiState(&state,1);
}
