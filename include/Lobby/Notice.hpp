#pragma once
#include "Render/Caption.hpp"

// THE NOTICE, IN THE GAME'S LANGUAGE. The intro's first stage (Lobby/Intro.hpp) is the game's legal notice -- the bink
// the boot splash would have played -- in the language the game shows its text in, and under it the port's word: that
// it is an experimental fan-made mod, not CD PROJEKT RED's. The boot's loading screen plays the bink: the port's
// archive names the English one where the game's ink settings have their logo loop (cp_2077.inkgamesettings,
// initLoadingScreenLogoLoopVideo: tools/vr_lobby_assets/boot_screens.py), and the plugin the player's own as those
// settings load (Install). The screen has no ink of its own to put the word on -- the loading layer plays the bink by
// itself while the game boots -- so the plugin draws it under the screen as it hangs it in the dark (Render/
// Caption.hpp). The language is the player's settings file's, /language OnScreen: the game takes its own from the same
// file as it starts, and its settings system answers only its defaults while the game boots.
namespace cvr::lobby::notice {
// The port's word, two lines, laid out under the notice as the lobby's intro screen had them: the bink across the
// screen, the lines 250 and 333 px of its 2160 under its middle, 51 px tall.
const cvr::render::CaptionText& Word();
// The game's sliders for effects, master times effects (0..1), as the player's settings file has them.
float EffectsVolume();
// At load, before the game's resources: the ink settings' boot video is set to the player's language as they load
// (Core/ArchiveExtensions.hpp AddFixup).
void Install();
// The game has booted (its main menu is up): the loading screens after the boot -- back to the main menu from a game
// -- loop the game's own logo again.
void BootOver();
}  // namespace cvr::lobby::notice
