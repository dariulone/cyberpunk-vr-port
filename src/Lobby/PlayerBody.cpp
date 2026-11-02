// WHICH BODY THE PLAYER SPAWNS IN. Verified against game 2.5 with IDA headless and live (2026-11-02).
//
// `Character.Player_Puppet_Base.genders` holds one entity per gender (inline0 the woman's, inline1 the man's), one of
// them marked `initial`. A puppet's gender is its persistent state's (gamePuppetPS +0x68, entity +0x168), and with
// none there the `initial` entry's (sub_1405EEC80, GetResolvedGenderName sub_1407E1D04). It is WRITTEN that way too:
// sub_142B63D80 stores nothing for the gender `initial` names. So the flag is part of the save format. In the game as
// shipped `initial` is the woman: every woman's save holds no gender, every man's holds Male.
//
// THE FLAG MUST NOT CHANGE, NOT EVEN FOR THE MAIN MENU. 0.2.0 swapped it in the TweakDB so that the lobby would stand
// in a man's body (VRPort.LobbyBody.yaml). Every woman's save then read as a man: the man's puppet, which her
// character customization does not fit -- a bald stranger without her arms. And a man's new game wrote no gender,
// so with the flag back his saves read as a woman.
//
// The player's spawn (sub_1409CE8FC) takes a gender from its request (+72): "Default" in the main menu, where no
// character exists and the character customization system has no state; nothing on a load, which reads the
// persistent state. A gender in the request is what the game does for a new character: it goes through the same
// write and picks that entity. So:
//  - the main menu's spawn asks for the man's, and what that writes lives and dies with the main menu's session;
//  - a load asks for the man's when the save's character is a man's and its persistent state has no gender: a new
//    game started on 0.2.0. The write puts the gender there, and the next save carries it.
// Everything else goes as the game has it. Checked live: both main menu spawns (boot, back from a game) and a load.
#include "Stereo/DetourRegistry.hpp"
#include "Stereo/StereoInternal.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <atomic>
#include <cstdint>

namespace RED4ext::ent { struct Entity; }
RED4ext::ent::Entity* FindPlayerEntity();  // Natives/NativeHelpers.hpp
extern void Log(const char*, ...);

namespace {
constexpr uintptr_t kPlayerSpawnRva = 0x9CE8FC;
constexpr uintptr_t kResolvedGenderRva = 0x5EEC80;  // sub_1405EEC80: the persistent state's, or `initial`'s
constexpr uintptr_t kSetGenderRva = 0x15B4DCC;      // sub_1415B4DCC: gamePuppetPS's, as the game writes it
constexpr size_t kRequestGender = 72;
constexpr size_t kSystems = 64;       // the spawner's game systems
constexpr size_t kJobPersistentId = 7;  // of the job's qwords
constexpr size_t kEntityState = 0x168;  // Handle<gamePuppetPS> on a puppet

constexpr uint64_t kDefault = 0x11326FD2590F4E5EULL;  // CName "Default"
constexpr uint64_t kMale = 0x49F85AAED016DB90ULL;     // CName "Male"
constexpr uint64_t kFemale = 0x040F253D33676EB9ULL;   // CName "Female"

using PlayerSpawnFn = int64_t(__fastcall*)(int64_t, uint64_t*, int64_t, int64_t);
using ResolvedGenderFn = uint64_t*(__fastcall*)(uint64_t*, int64_t, uint64_t);
using SetGenderFn = int64_t(__fastcall*)(void*, uint64_t);

PlayerSpawnFn g_origPlayerSpawn = nullptr;
std::atomic<bool> g_mainMenuBody{false};

uintptr_t Base() { return reinterpret_cast<uintptr_t>(cvr::detail::g_exe_base); }

// The body of the session's character: 1 a man's, 0 a woman's, -1 no character (the main menu).
int CharacterBodyMale() {
    auto* engine = RED4ext::CGameEngine::Get();
    auto* game = engine && engine->framework ? engine->framework->gameInstance : nullptr;
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* type = rtti ? rtti->GetClass("gameuiICharacterCustomizationSystem") : nullptr;
    auto* system = game && type ? game->GetSystem(type) : nullptr;
    auto* getState = type ? type->GetFunction("GetState") : nullptr;
    if (!system || !getState) return -1;
    RED4ext::Handle<RED4ext::IScriptable> state;
    if (!RED4ext::ExecuteFunction(system, getState, &state) || !state) return -1;
    auto* isMale = state->GetType()->GetFunction("IsBodyGenderMale");
    bool male = false;
    if (!isMale || !RED4ext::ExecuteFunction(state.instance, isMale, &male)) return -1;
    return male ? 1 : 0;
}

int64_t __fastcall Detour_PlayerSpawn(int64_t spawner, uint64_t* job, int64_t request, int64_t context) {
    uint64_t& gender = *reinterpret_cast<uint64_t*>(request + kRequestGender);
    const uint64_t asked = gender;
    const int bodyMale = CharacterBodyMale();
    const bool mainMenu = asked == kDefault && bodyMale < 0;
    g_mainMenuBody.store(mainMenu, std::memory_order_relaxed);
    if (mainMenu) {
        gender = kMale;
    } else if (!asked && bodyMale == 1) {
        uint64_t resolved = 0;
        reinterpret_cast<ResolvedGenderFn>(Base() + kResolvedGenderRva)(&resolved, *reinterpret_cast<int64_t*>(spawner + kSystems),
                                                                       job[kJobPersistentId]);
        if (resolved == kFemale) {
            Log("[body] a man's save with no gender in its persistent state (a new game on 0.2.0): repaired\n");
            gender = kMale;
        }
    }
    const int64_t result = g_origPlayerSpawn(spawner, job, request, context);
    gender = asked;  // the request is the game's, and it is read here alone
    return result;
}
}  // namespace

// VRLobbyResolveAs(male): the game takes the main menu's puppet for that body from now on (the variant of what it is
// given, what GetResolvedGenderName says). Nothing outside the main menu: a V's gender is her save's.
void VRLobbyResolveAs(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    bool male = true;
    RED4ext::GetParameter(frame, &male);
    ++frame->code;
    if (!g_mainMenuBody.load(std::memory_order_relaxed)) return;
    auto* player = reinterpret_cast<uint8_t*>(FindPlayerEntity());
    auto* rtti = RED4ext::CRTTISystem::Get();
    auto* type = rtti ? rtti->GetClass("gamePuppetPS") : nullptr;
    auto* state = player ? *reinterpret_cast<RED4ext::IScriptable**>(player + kEntityState) : nullptr;
    if (!state || !type || !state->GetType()->IsA(type)) {
        Log("[body] the main menu's puppet has no persistent state of the known layout\n");
        return;
    }
    reinterpret_cast<SetGenderFn>(Base() + kSetGenderRva)(state, male ? kMale : kFemale);
}

CVR_DETOUR("[body] player spawn sub_1409CE8FC", kPlayerSpawnRva, Detour_PlayerSpawn, g_origPlayerSpawn)
