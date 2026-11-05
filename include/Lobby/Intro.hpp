#pragma once
#include <cstdint>
#include <memory>

// THE INTRO OVER THE VOID. The game starts in the dark, with the intro's two stages:
//  - THE NOTICE, the game's legal notice and the port's word under it, is the boot's own screen: while the game boots
//    into the main menu its loading screen plays the notice's bink in place of the game's logo, in the game's
//    language (Lobby/Notice.hpp). In the headset -- and in the window, as the eyes see it -- that screen is in the
//    dark at the intro's heading (Heading below), one picture to both eyes as the logo after it, the port's word under
//    it (Runtimes/OpenXRCapture.cpp: the window's picture of it, Render/LoadingScreen.hpp), its hum under it, for as
//    long as the game boots; once the screen closes its last picture stays there while the main menu's world comes up
//    behind the curtain (Lobby/MenuLobby.hpp Curtain), until the lobby's intro takes it over (VRIntroTakeBootNotice)
//    and it fades out.
//  - THE LOGO: the lobby's intro (mods/cet/CyberpunkVRPort_Lobby modules/intro.lua) plays the game's animated logo
//    -- its bink, on a lobby screen whose display mesh stays off -- and flies it off ahead while the curtain opens.
//    The picture is drawn into the frame itself, over the curtain (Render/IntroOverlay.hpp), so the window shows it
//    as the eyes do: the screen's ink texture drawn as light, black clear as on the lobby's additive screens, on a
//    quad at the intro's heading, the same to both eyes (Render/SpaceQuad.hpp Ahead), at the distance and height the
//    script gives it each frame. While the curtain opens the world comes up behind it.
namespace cvr::hud { struct TextureLease; }
namespace cvr::render { class FreeLookHeading; }
namespace cvr::lobby::intro {
// THE INTRO'S HEADING, the notice's and the logo's: the logo comes up where the notice was. The head looks about
// within 90 degrees of it and the pictures stay where they are; turned farther, they are brought round after it
// (Render/SpaceQuad.hpp FreeLookHeading). Pinned to the gaze, the intro moved with every turn of the head; hung
// once, it was lost behind a head turned away. Capture thread only.
constexpr float kFreeLookCone = 1.5707963f;
cvr::render::FreeLookHeading& Heading();

struct Picture {
    std::shared_ptr<cvr::hud::TextureLease> texture;
    float distance = 0.0f, height = 0.0f, opacity = 0.0f;
    // Linear light a texel must exceed to show: a bink's dark backdrop is clear air too.
    float blackPoint = 0.0f;
    // One per intro: a new one is logged as its first picture comes.
    uint64_t intro = 0;
};
// The picture the script gave last; null once the intro has ended, or the script has given none for 2 s.
std::shared_ptr<const Picture> Latest();

// Where the notice hangs: as far as the logo starts and as wide as it is shown there (the logo's bink across the
// lobby screen's canvas: modules/intro.lua FLIGHT.from and FLIGHT.heightFrom, the canvas 2580 x 1980). One picture
// to both eyes has no depth of its own: how far it looks is how large it is, 2.3 m tall at 3.4 m (was 2.6 m, which
// sat too close before the eyes).
constexpr float kNoticeDistance = 3.4f, kNoticeWidth = 2.3f * 2580.0f / 1980.0f;

// What a frame shows of the notice: `shown` it is drawn, `live` its screen is up (the window shows it this frame),
// at `opacity`.
struct BootFrame {
    bool shown = false, live = false;
    float opacity = 0.0f;
};
BootFrame Boot();
// The loading layer was ticked, `active` it shows a screen (Stereo/EngineStereo.cpp): the launch's first screen it
// shows is the boot's, which has the notice and its hum from then on, till the main menu comes up.
void NoteLoadingLayer(bool active);
// The lobby's intro takes the notice over: it fades out over `fade` s. False when there is none to take.
bool TakeBootNotice(float fade);
// The notice's hum: a cyberspace hum, muffled -- the game is still loading behind it -- and looped from its middle so
// that it does not keep starting over. `volume` is the game's sliders for effects (master times effects, 0..1).
void Hum(bool on, float volume);
}  // namespace cvr::lobby::intro
