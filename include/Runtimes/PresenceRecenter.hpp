#pragma once
#include <openxr/openxr.h>

// RECENTER WHEN THE PLAYER PUTS THE HEADSET BACK ON.
//
// Taking the headset off drops the session out of VISIBLE/FOCUSED into SYNCHRONIZED (SteamVR does this on
// the proximity sensor). While it is there head tracking is not usable (Runtimes/RoomscaleTracking.hpp), so
// the room-scale offset stops following the player: whoever walks away and comes back stands somewhere
// else, facing elsewhere, while the body is still where the old pose left it. The cure is the recenter the
// player would otherwise do by hand (F7, the controller gesture), done when the session is FOCUSED again.
//
// Only a real absence counts. Start-up also passes through SYNCHRONIZED, but from READY, before anything
// was ever shown, and the base pose is captured anyway. The SteamVR dashboard goes FOCUSED -> VISIBLE ->
// FOCUSED without SYNCHRONIZED: the player has not moved, and recentering there would be a surprise.
namespace cvr::presence {
struct AwayTracker { bool away = false; };

// Feed every session state change in order; returns true exactly once per return from an absence.
constexpr bool OnSessionState(AwayTracker& t, XrSessionState previous, XrSessionState current) {
    const bool wasShown = previous == XR_SESSION_STATE_VISIBLE || previous == XR_SESSION_STATE_FOCUSED;
    if (current == XR_SESSION_STATE_SYNCHRONIZED && wasShown) t.away = true;
    if (current == XR_SESSION_STATE_STOPPING || current == XR_SESSION_STATE_IDLE) t.away = false;
    if (current == XR_SESSION_STATE_FOCUSED && t.away) { t.away = false; return true; }
    return false;
}

namespace detail {
constexpr int CountRecenters(const XrSessionState* states, int n) {
    AwayTracker t{}; int count = 0;
    for (int i = 1; i < n; ++i) count += OnSessionState(t, states[i - 1], states[i]) ? 1 : 0;
    return count;
}
// Start-up: IDLE -> READY -> SYNCHRONIZED -> VISIBLE -> FOCUSED. No recenter.
constexpr XrSessionState kStartup[] = {XR_SESSION_STATE_IDLE, XR_SESSION_STATE_READY, XR_SESSION_STATE_SYNCHRONIZED,
                                       XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_FOCUSED};
static_assert(CountRecenters(kStartup, 5) == 0, "start-up must not recenter");
// Dashboard: FOCUSED -> VISIBLE -> FOCUSED. No recenter.
constexpr XrSessionState kDashboard[] = {XR_SESSION_STATE_FOCUSED, XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_FOCUSED};
static_assert(CountRecenters(kDashboard, 3) == 0, "dashboard must not recenter");
// Headset off and on (as logged with a Vive Pro on SteamVR 2.17): 5 -> 4 -> 3 -> 4 -> 5. One recenter.
constexpr XrSessionState kOffOn[] = {XR_SESSION_STATE_FOCUSED, XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_SYNCHRONIZED,
                                     XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_FOCUSED};
static_assert(CountRecenters(kOffOn, 5) == 1, "headset off/on must recenter once");
// Twice off and on: two recenters, each on the return.
constexpr XrSessionState kTwice[] = {XR_SESSION_STATE_FOCUSED, XR_SESSION_STATE_SYNCHRONIZED, XR_SESSION_STATE_FOCUSED,
                                     XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_SYNCHRONIZED, XR_SESSION_STATE_VISIBLE,
                                     XR_SESSION_STATE_FOCUSED};
static_assert(CountRecenters(kTwice, 7) == 2, "each return recenters");
// Off, then the session stops (game closing): nothing on the way out.
constexpr XrSessionState kOffStop[] = {XR_SESSION_STATE_FOCUSED, XR_SESSION_STATE_VISIBLE, XR_SESSION_STATE_SYNCHRONIZED,
                                       XR_SESSION_STATE_STOPPING, XR_SESSION_STATE_IDLE};
static_assert(CountRecenters(kOffStop, 5) == 0, "stopping must not recenter");
}
}
