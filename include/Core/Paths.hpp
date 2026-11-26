#pragma once
#include <string>

// ONE FOLDER. Everything the port reads or writes lives in its plugin's own folder,
// red4ext/plugins/cyberpunkvrport/ (docs/native-frameworks-plan-20261030.md):
//
//   shaders/  the shader twins (asym\, stereo_*\, ...) and the port's own shaders
//   grips/    the per-weapon grip and smoke/lighter poses (CyberpunkVR_*Grip*.ini)
//   archive/  the port's archive
//   config/   vrport.ini, vrport-launcher.ini, vrik_calibration.ini, the shipped UserSettings.json
//   log/      cyberpunkvrport.log and every diagnostic dump
//   scripts/  the port's scripts (*.reds), compiled with the game's (Core/ScriptCompiler.hpp)
//   redscript/ the script compiler, built from redscript's sources, for a game without redscript installed
//   lua/      the port's Lua mods, run by the plugin's own Lua host (Lua/Host.hpp), each in its own folder
//
// The folder is the one this module was loaded from, never the game's: nothing depends on where the exe is.
// Rust owns the folders (rust/crates/cvr-host/src/paths.rs); these read them.
namespace cvr::paths {

enum class Folder { Root, Shaders, Grips, Archive, Config, Log, Scripts, Redscript, Lua };

// The folder with a trailing backslash, created on first use.
const std::wstring& Dir(Folder folder);
std::wstring File(Folder folder, const wchar_t* name);
// The same in the ANSI code page, for the C runtime and the Win32 A calls the older code makes.
std::string DirA(Folder folder);
std::string FileA(Folder folder, const char* name);

// The legacy-layout migration and the other-copy checks run in Rust before the C++ half starts
// (rust/crates/cvr-host/src/paths.rs, called by rust/crates/cvr-ffi/src/plugin.rs).

}  // namespace cvr::paths
