// THE LAUNCHER'S READY STATE, IN THE LEFT-HAND CYBERWARE STATE MACHINE'S DEFINITION.
//
// The arm launcher waits in a state of the port's own, vrLauncherReady (VrLauncherReadyDecisions / VrLauncherReadyEvents
// in mods/redscript/CyberpunkVRPort_Cyberware/vrport_cyberware.reds), and the machine only knows a state its definition
// names: the native PSM loader enumerates playerStateMachineDefinitions.LeftHandCyberware.states (loader RVA 0x674E5C),
// and a transition target alone does not register one. So the definition gets the state as the game writes its own:
// its name in the list -- the package-local name, as a qualified one resolves OnEnter but not the outgoing transitions
// -- its flats beside the others', taken from leftHandCyberwareWaitForUnequip where they are the same, and an edge to
// it at the head of each state it is entered from.
//
// This ran as a TweakXL scriptable tweak; it is a native tweak of the plugin now (Core/TweakRecords.hpp), at the same
// moment, with the same result.
#include "Core/TweakRecords.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/RTTISystem.hpp>
#include <RED4ext/TweakDB.hpp>

#include <algorithm>
#include <cstring>
#include <string>
#include <vector>

extern void Log(const char* fmt, ...);

namespace {

using RED4ext::CString;
using RED4ext::DynArray;
using RED4ext::TweakDBID;

const std::string kRoot = "playerStateMachineLeftHandCyberware.";
const std::string kState = "vrLauncherReady";

DynArray<CString>* Strings(const RED4ext::CStackType& value) {
    return value.type && std::strcmp(value.type->GetName().ToString(), "array:String") == 0
               ? static_cast<DynArray<CString>*>(value.value)
               : nullptr;
}

bool Contains(const DynArray<CString>& list, const char* text) {
    return std::any_of(list.begin(), list.end(), [&](const CString& s) { return std::strcmp(s.c_str(), text) == 0; });
}

// The edge to the ready state, first of `state`'s transitions. A definition whose lists disagree is left alone rather
// than guessed at.
void AddReadyEdge(cvr::tweak_records::FlatWrites& writes, RED4ext::rtti::IType* stringsType,
                  RED4ext::rtti::IType* boolsType, const std::string& state) {
    const std::string prefix = kRoot + state + ".";
    DynArray<CString>* targets = Strings(writes.Get(TweakDBID(prefix + "transitionTo")));
    DynArray<CString>* conditions = Strings(writes.Get(TweakDBID(prefix + "transitionCondition")));
    if (!targets || !conditions || targets->size() == 0 || targets->size() != conditions->size() ||
        Contains(*targets, kState.c_str()))
        return;
    DynArray<CString> newTargets(*targets), newConditions(*conditions);
    newTargets.Emplace(newTargets.begin(), kState.c_str());
    newConditions.Emplace(newConditions.begin(), "=");
    writes.Set(TweakDBID(prefix + "transitionTo"), stringsType, &newTargets);
    writes.Set(TweakDBID(prefix + "transitionCondition"), stringsType, &newConditions);
    for (const char* field : {"transitionConditionOverridesEnterCondition", "transitionSkipInternalCondition"}) {
        const RED4ext::CStackType flags = writes.Get(TweakDBID(prefix + field));
        if (flags.type != boolsType || static_cast<DynArray<bool>*>(flags.value)->size() == 0) continue;
        DynArray<bool> newFlags;
        newFlags.PushBack(false);
        for (bool b : *static_cast<DynArray<bool>*>(flags.value)) newFlags.PushBack(b);
        writes.Set(TweakDBID(prefix + field), boolsType, &newFlags);
    }
}

void AddLauncherReadyState(RED4ext::TweakDB* db) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    RED4ext::rtti::IType* stringsType = rtti->GetType("array:String");
    RED4ext::rtti::IType* boolsType = rtti->GetType("array:Bool");
    RED4ext::rtti::IType* boolType = rtti->GetType("Bool");
    cvr::tweak_records::FlatWrites writes(db);
    const TweakDBID statesFlat("playerStateMachineDefinitions.LeftHandCyberware.states");
    DynArray<CString>* states = Strings(writes.Get(statesFlat));
    if (!states || states->size() == 0) {
        Log("[tweaks] the left-hand cyberware machine lists no states: no launcher ready state\n");
        return;
    }
    // The states it is entered from: the machine's own, as listed before the ready state joins them.
    std::vector<std::string> from;
    for (const char* state : {"leftHandCyberwareEquip", "leftHandCyberwareQuickAction", "leftHandCyberwareChargeAction",
                              "leftHandCyberwareChargeRepeatAction", "leftHandCyberwareWaitForUnequip"})
        from.emplace_back(state);
    if (!Contains(*states, kState.c_str())) {
        DynArray<CString> listed(*states);
        listed.PushBack(kState.c_str());
        writes.Set(statesFlat, stringsType, &listed);
    }
    const std::string ready = kRoot + kState + ".", like = kRoot + "leftHandCyberwareWaitForUnequip.";
    for (const char* field : {"stateBodyClassName", "affinity", "alias", "forceInputContext",
                              "transitionConditionOverridesEnterCondition", "transitionSkipInternalCondition"}) {
        const RED4ext::CStackType value = writes.Get(TweakDBID(like + field));
        if (value.type) writes.Set(TweakDBID(ready + field), value.type, value.value);
    }
    for (const auto& [field, on] : {std::pair{"hasEnterCondition", true}, {"hasExitCondition", false},
                                    {"hasOnEnter", true}, {"hasOnExit", true}, {"hasOnUpdate", true}}) {
        bool value = on;
        writes.Set(TweakDBID(ready + field), boolType, &value);
    }
    DynArray<CString> targets, conditions;
    for (const char* target : {"leftHandCyberwareUnequip", "leftHandCyberwareCharge", "leftHandCyberwareQuickAction"}) {
        targets.PushBack(target);
        conditions.PushBack("=");
    }
    writes.Set(TweakDBID(ready + "transitionTo"), stringsType, &targets);
    writes.Set(TweakDBID(ready + "transitionCondition"), stringsType, &conditions);
    for (const std::string& state : from) AddReadyEdge(writes, stringsType, boolsType, state);
    writes.Commit();
}

const bool kRegistered = (cvr::tweak_records::AddNativeTweak(&AddLauncherReadyState), true);

}  // namespace
