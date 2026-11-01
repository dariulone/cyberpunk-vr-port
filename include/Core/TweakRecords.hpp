#pragma once

// THE PORT'S TWEAKDB RECORDS, WITHOUT TWEAKXL (stage 2 of docs/native-frameworks-plan-20261030.md).
//
// The records the port adds or changes -- the belt's attachment slots and props, the magazines, the laser attack,
// the physical crouch, the lobby body, the cigarette -- are written in TweakXL's YAML (config\tweaks\**\*.yaml in the
// plugin's folder) and applied here, right after the game loads its TweakDB (the same moment TweakXL applies its
// own), through the TweakDB the engine keeps: a record with $base is created as a copy of every flat its base has, a
// record with $type is created with that type's flats, and each property given is written as a flat of the type the
// record already holds for it. Arrays take TweakXL's operators (!append, !append-once, !prepend, !prepend-once,
// !remove). The names of the records made here are registered with the engine, so they read back as names.
//
// What YAML cannot say -- flats derived from other flats, edits that depend on what a definition holds -- is a
// native tweak (AddNativeTweak), run right after the records with the same TweakDB.
//
// TweakXL itself (another mod's) is untouched: the port's files are not where it looks, and it applies its own as
// before. Copies of the port's files an earlier release put in r6\tweaks are taken out, so nothing is applied twice.
#include <RED4ext/Api/v1/PluginHandle.hpp>
#include <RED4ext/Api/v1/Sdk.hpp>
#include <RED4ext/NativeTypes.hpp>
#include <RED4ext/Scripting/Stack.hpp>

#include <vector>

namespace RED4ext {
struct TweakDB;
}

namespace cvr::tweak_records {

// At load: takes earlier releases' copies out of r6\tweaks and hooks the TweakDB load.
void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk);

using NativeTweak = void (*)(RED4ext::TweakDB* db);
// Runs `tweak` each time the TweakDB loads, after the port's records. From a static initializer is fine.
void AddNativeTweak(NativeTweak tweak);

// Flat writes committed together. A flat that is new to the TweakDB is an insert into its sorted array of ~2 million
// ids, which moves the array's tail -- one insert for all of them instead of one each.
class FlatWrites {
public:
    explicit FlatWrites(RED4ext::TweakDB* db) : db_(db) {}
    ~FlatWrites() { Commit(); }
    FlatWrites(const FlatWrites&) = delete;
    FlatWrites& operator=(const FlatWrites&) = delete;

    // The flat's value as the TweakDB holds it now (writes not yet committed are not seen); type null when absent.
    RED4ext::CStackType Get(RED4ext::TweakDBID flat) const;
    // A value of `type` for the flat; an equal value already there is left alone. False when the buffer is full.
    bool Set(RED4ext::TweakDBID flat, RED4ext::rtti::IType* type, void* value);
    void Commit();

private:
    RED4ext::TweakDB* db_;
    std::vector<RED4ext::TweakDBID> pending_;
};

}  // namespace cvr::tweak_records
