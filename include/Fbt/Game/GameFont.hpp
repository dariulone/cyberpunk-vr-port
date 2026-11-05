#pragma once
// The game's own font for its text language, read from the game's archives (Fbt/Game/GameFont.cpp), for the texts the
// full-body tracking calibration draws in the view (Fbt/Game/Helpers.cpp).
#include <cstdint>
#include <string>
#include <vector>

namespace cvr::fbt::game {

struct GameTextFont {
    std::string language;          // the game's text language ("en-us", "it-it", "jp-jp", ...)
    std::string path;              // the game's font resource (.fnt) for that language
    std::vector<uint8_t> file;     // the font file inside the game's rendFont (TrueType)
    std::string texts;             // UTF-8: every letter the view's texts use in that language
};
// Blocks up to timeoutMs for the game's resource loader; false (and *why) when there is no usable font.
bool ReadGameTextFont(GameTextFont* out, unsigned timeoutMs, std::string* why);

}  // namespace cvr::fbt::game
