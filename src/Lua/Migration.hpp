#pragma once

#include <cstdint>

// From an install whose Lua mods ran in Cyber Engine Tweaks to the plugin's host, before the host loads anything.
namespace cvr::lua {

// Each mod in the plugin's lua\ folder that CET's mods folder also has leaves CET's folder (CET would run it a
// second time): files with the same bytes as the plugin's are deleted; the player's data (files only CET's copy has,
// and data files that differ from the shipped default: settings, recordings) moves into the plugin's copy, the
// default it replaces kept aside; an edited or older script and CET's own files (its logs, the mod's sqlite db) are
// kept aside in legacy\cet\<mod>. The first time, config\lua_bindings.json takes these mods' binds from CET's
// bindings.json.
//
// The host's overlay key is Home without CET. Beside CET it is Scroll Lock (Pause if CET has that one), checked at
// every start: on CET's own key one press would open both overlays and these windows would take CET's input, and
// Home moves the caret in CET's console.
//
// Returns CET's overlay key as VKBindings encodes it, 0 when CET has none on disk.
uint64_t MigrateFromCet();

}  // namespace cvr::lua
