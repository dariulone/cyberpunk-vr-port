#pragma once

// THE PORT'S SCRIPTS, WITHOUT REDSCRIPT INSTALLED (stage 5 of docs/native-frameworks-plan-20261030.md).
//
// The port's scripts are scripts\**\*.reds in the plugin's folder, handed to RED4ext's script compilation like any
// plugin's. What compiles them is redscript's compiler, scc_lib.dll -- the one redscript installs in engine\tools when
// it is installed, else the plugin's own, built from redscript's sources (MIT) into redscript\:
//
//   redscript installed   nothing else happens: its compiler compiles the game's scripts, every mod's and the port's.
//   not installed         the plugin switches the engine's Scripts/EnableCompilation on (redscript's installer does it
//                         in engine\config\base\scripts.ini) and points the engine's start of scc.exe at redscript\;
//                         RED4ext, which runs the compiler in process from the scc_lib.dll beside that path, then
//                         compiles with the plugin's -- r6\scripts and every plugin's scripts, as redscript would.
//
// Copies of the port's scripts an earlier release put in r6\scripts\CyberpunkVRPort_* are taken out first: compiled
// beside the plugin's they would define everything twice.
#include <RED4ext/Api/v1/PluginHandle.hpp>
#include <RED4ext/Api/v1/Sdk.hpp>

namespace cvr::scripts {

// At load (the only time RED4ext takes a plugin's script paths).
void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk);

}  // namespace cvr::scripts
