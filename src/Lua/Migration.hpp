#pragma once

// From an install whose Lua mods ran in Cyber Engine Tweaks to the plugin's host, before the host loads anything.
namespace cvr::lua {

// Each mod in the plugin's lua\ folder that CET's mods folder also has leaves CET's folder (CET would run it a
// second time): files with the same bytes as the plugin's are deleted; the player's data (files only CET's copy has,
// and data files that differ from the shipped default: settings, recordings) moves into the plugin's copy, the
// default it replaces kept aside; an edited or older script and CET's own files (its logs, the mod's sqlite db) are
// kept aside in legacy\cet\<mod>. The first time, config\lua_bindings.json takes CET's overlay key and these mods'
// binds from CET's bindings.json (the overlay key is Home without one).
void MigrateFromCet();

}  // namespace cvr::lua
