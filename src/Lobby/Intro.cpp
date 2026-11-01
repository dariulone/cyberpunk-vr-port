// The lobby intro's picture (Lobby/Intro.hpp), as the script gives it.
//
// VRLobbyIntro(component, distance, height, opacity, blackPoint) -> Bool: the lobby screen whose ink is the intro's
// picture (one of the port's own screens, so it has a texture of its own: Natives/MenuSurface.cpp), how far ahead and
// how tall it is shown (m), how bright (0..1) and the linear light a texel must exceed to show at all. Given every
// frame of the intro; true once the screen's texture is there to show.
// A null component ends the intro.
//
// The notice at the boot (Lobby/Intro.hpp): VRIntroTakeBootNotice(fade) -> Bool and VRIntroHum(on, volume) from the
// lobby's intro.
#include "Lobby/Intro.hpp"
#include "Audio/PortSound.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Lobby/Notice.hpp"
#include "Stereo/EngineStereo.hpp"
#include "Natives/MenuSurface.hpp"
#include "Render/HudPanel.hpp"
#include "Render/SpaceQuad.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/Generated/WorldWidgetComponent.hpp>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <mutex>

extern void Log(const char*, ...);

namespace cvr::lobby::intro {
namespace {
// A picture not given again for this long belongs to an intro whose script has stopped.
constexpr ULONGLONG kStale = 2000;
std::mutex g_mutex;
std::shared_ptr<const Picture> g_picture;
ULONGLONG g_tick = 0;
uint64_t g_intro = 0;
bool g_running = false;

bool Set(RED4ext::WorldWidgetComponent* component, float distance, float height, float opacity, float blackPoint) {
    if (!component) {
        std::lock_guard lock(g_mutex);
        g_picture.reset();
        g_running = false;
        return false;
    }
    auto* texture = cvr::menu_surface::SurfaceTexture(component);
    auto lease = texture ? cvr::hud::Lease(texture, true) : nullptr;
    if (!lease) return false;
    auto picture = std::make_shared<Picture>();
    picture->texture = std::move(lease);
    picture->distance = std::clamp(distance, 0.3f, 50.0f);
    picture->height = std::clamp(height, 0.05f, 20.0f);
    picture->opacity = std::clamp(opacity, 0.0f, 1.0f);
    picture->blackPoint = std::clamp(blackPoint, 0.0f, 0.9f);
    const ULONGLONG now = GetTickCount64();
    std::lock_guard lock(g_mutex);
    if (!g_running || now - g_tick > kStale) ++g_intro;
    g_running = true;
    picture->intro = g_intro;
    g_picture = std::move(picture);
    g_tick = now;
    return true;
}
}  // namespace

std::shared_ptr<const Picture> Latest() {
    std::lock_guard lock(g_mutex);
    if (g_picture && GetTickCount64() - g_tick > kStale) {
        g_picture.reset();
        g_running = false;
    }
    return g_picture;
}

cvr::render::FreeLookHeading& Heading() {
    static cvr::render::FreeLookHeading heading(kFreeLookCone);
    return heading;
}

namespace {
// The notice once its screen has closed, with nothing taking it over: it stays at most this long (ms) -- the main
// menu's world and the lobby come up meanwhile -- and not after the curtain opened without it, nor without the lobby.
constexpr ULONGLONG kBootHold = 30000;
// It fades out (s) when nothing takes it: the fade the intro would give it.
constexpr float kBootFade = 0.8f;
struct BootState {
    bool up = false, held = false, fading = false, shut = false;
    ULONGLONG closed = 0, fadeFrom = 0;
    float fade = kBootFade;
};
std::mutex g_bootMutex;
BootState g_boot;

float Smooth(float x) {
    x = std::clamp(x, 0.0f, 1.0f);
    return x * x * (3.0f - 2.0f * x);
}
}  // namespace

BootFrame Boot() {
    bool hushed = false;
    BootFrame frame;
    static std::atomic<bool> booted{false};
    if (!booted.load(std::memory_order_relaxed) && cvr::engine_stereo::MainMenuUp()) {
        booted = true;
        cvr::lobby::notice::BootOver();
    }
    {
        std::lock_guard lock(g_bootMutex);
        auto& b = g_boot;
        if (!b.held) return frame;
        const ULONGLONG now = GetTickCount64();
        if (b.up && cvr::engine_stereo::MainMenuUp()) {
            // The main menu came up: the boot's loading screen is gone.
            b.up = false;
            b.closed = now;
            Log("[lobby-intro] the boot's screen closed: the main menu is up\n");
        }
        if (!b.fading) {
            const float curtain = cvr::lobby::Curtain();
            if (curtain >= 1.0f) b.shut = true;
            if (!b.up && (!cvr::lobby::Enabled() || now - b.closed > kBootHold || (b.shut && curtain < 1.0f))) {
                b.fading = true;
                b.fadeFrom = now;
                b.fade = kBootFade;
                hushed = true;
                Log("[lobby-intro] the boot's notice ends: nothing took it over\n");
            }
        }
        frame.opacity = 1.0f;
        if (b.fading) {
            const float k = float(now - b.fadeFrom) / (1000.0f * b.fade);
            if (k >= 1.0f) {
                b.held = false;
                return {};
            }
            frame.opacity = 1.0f - Smooth(k);
        }
        frame.shown = true;
        frame.live = b.up;
    }
    if (hushed) Hum(false, 0.0f);
    return frame;
}

void NoteLoadingLayer(bool active) {
    // Only the launch's first screen, before the main menu: a later one (the main menu's world loading under the menu,
    // back to the main menu from a game) is a loading screen like any. It closes as the main menu comes up (Boot).
    static std::atomic<bool> seen{false};
    if (!active || seen.exchange(true, std::memory_order_relaxed) || cvr::engine_stereo::MainMenuUp()) return;
    {
        std::lock_guard lock(g_bootMutex);
        g_boot = {};
        g_boot.up = g_boot.held = true;
    }
    Log("[lobby-intro] the boot's screen is up: its notice hangs in the dark\n");
    if (cvr::lobby::Enabled()) Hum(true, cvr::lobby::notice::EffectsVolume());
}

bool TakeBootNotice(float fade) {
    std::lock_guard lock(g_bootMutex);
    auto& b = g_boot;
    if (!b.held) return false;
    if (!b.fading) {
        b.fading = true;
        b.fadeFrom = GetTickCount64();
        b.fade = std::clamp(fade, 0.05f, 10.0f);
        Log("[lobby-intro] the intro takes the boot's notice over\n");
    }
    return true;
}

void Hum(bool on, float volume) {
    // The lobby's own sound (tools/vr_lobby_assets/lobby_sounds.py): looped from 11 s to 20.6 s, the seam crossfaded
    // over 1.5 s, under a 900 Hz low-pass; it comes up over 1.5 s and goes over 2.5 s.
    cvr::sound::Sound hum;
    if (on) {
        hum.file = "CyberpunkVRPort_Lobby/sounds/intro_loop.ogg";
        hum.volume = 0.55f * std::clamp(volume, 0.0f, 1.0f);
        hum.lowpass = 900.0f;
        hum.loopFrom = 11.0f;
        hum.loopTo = 20.6f;
        hum.crossfade = 1.5f;
    }
    cvr::sound::Channel("lobby_intro", hum, on ? 1.5f : 2.5f);
}
}  // namespace cvr::lobby::intro

void VRIntroTakeBootNotice(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    float fade = 0.0f;
    RED4ext::GetParameter(frame, &fade);
    ++frame->code;
    const bool taken = cvr::lobby::intro::TakeBootNotice(fade);
    if (out) *out = taken;
}

void VRIntroHum(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    bool on = false;
    float volume = 1.0f;
    RED4ext::GetParameter(frame, &on);
    RED4ext::GetParameter(frame, &volume);
    ++frame->code;
    cvr::lobby::intro::Hum(on, volume);
}

void VRLobbyIntro(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    RED4ext::Handle<RED4ext::ent::IComponent> component;
    float distance = 0.0f, height = 0.0f, opacity = 0.0f, blackPoint = 0.0f;
    RED4ext::GetParameter(frame, &component);
    RED4ext::GetParameter(frame, &distance);
    RED4ext::GetParameter(frame, &height);
    RED4ext::GetParameter(frame, &opacity);
    RED4ext::GetParameter(frame, &blackPoint);
    ++frame->code;
    auto* type = RED4ext::CRTTISystem::Get()->GetClass("WorldWidgetComponent");
    auto* widget = component && type && component->GetType()->IsA(type)
                       ? static_cast<RED4ext::WorldWidgetComponent*>(component.GetPtr())
                       : nullptr;
    const bool shown = cvr::lobby::intro::Set(widget, distance, height, opacity, blackPoint);
    if (out) *out = shown;
}
