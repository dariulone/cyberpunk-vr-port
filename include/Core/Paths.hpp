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
namespace cvr::paths {

enum class Folder { Root, Shaders, Grips, Archive, Config, Log, Scripts, Redscript, Lua };

// The folder with a trailing backslash, created on first use.
const std::wstring& Dir(Folder folder);
std::wstring File(Folder folder, const wchar_t* name);
// The same in the ANSI code page, for the C runtime and the Win32 A calls the older code makes.
std::string DirA(Folder folder);
std::string FileA(Folder folder, const char* name);

// Once at load, before anything opens a file: what an earlier release kept next to the exe (vrport.ini,
// vrport-launcher.ini, the grips) or in its own plugin folder (vrik_calibration.ini) moves here. A file already
// here wins and the old one is left where it is.
void MigrateLegacyLayout();

// Another copy of the port's plugin under red4ext/plugins (an old release's CyberpunkVR_Stereo folder left
// behind): its full path, or empty. Two copies hook everything twice, so the second one to find the other
// stays inert.
std::wstring OtherCopy();

// The plugin's file is cyberpunkvrport.dll; up to 0.1.9 it was CyberpunkVR_Stereo.dll, and a release unpacked over
// an older one leaves that file beside this one -- RED4ext loads both, and the older one does not look for us.
// Renamed to .old here; its path when it still runs this launch (already loaded, or the rename failed), so this
// copy stays inert, empty otherwise.
std::wstring RetireOldName();

}  // namespace cvr::paths
