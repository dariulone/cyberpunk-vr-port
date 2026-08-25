// FirstLaunch -- the game settings this port was tuned against.
//
// Applied once, gated on a flag file. It exists because a fresh install's defaults produce a VR image
// that is wrong in ways that read as bugs in this mod: the wrong FOV, the wrong upscaler, motion blur
// and depth of field on. Every value here was chosen by testing in the headset.
//
// It is a one-shot rather than an enforcement: after the first launch the settings are the player's.

#include <windows.h>
#include <psapi.h>
#include <xinput.h>
#include "Utils/SharedSlots.hpp"   // CyberpunkVR_Hands_Shared slot map (single source of truth)
#include <cstdint>
#include <cstddef>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <cmath>
#include <share.h>
#include "Utils/AobScanner.hpp"
#include "Overlay/LiveControlsUi.hpp"
#include "Overlay/LauncherDialog.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Runtimes/RuntimeFovCorrection.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/ScriptGameInstance.hpp>
#include <iostream>
#include <MinHook.h>
#include "Hooks/SwapChain.hpp"
#include "Utils/LogThrottle.hpp"
#include "Hooks/Trampoline.hpp"
#include "Utils/MemorySafe.hpp"
#include "Core/Telemetry.hpp"
#include "Core/LiveControls.hpp"
#include "Core/VrCoreShared.hpp"
#include "Core/CoreInternal.hpp"
#include "Camera/CameraLink.hpp"
#include "Hooks/Hook.hpp"

// Rewrite the one key in place. PersistLiveControlsUiState rewrites the whole file, but that only
// runs when the overlay saves; this has to survive a launch where the player never opens it, and
// it must not throw away anything else the file carries.
static bool WriteFirstLaunchFlag(int value) {
    char buf[16384] = {};
    size_t len = 0;
    if (FILE* f = _fsopen(g_liveControlPath, "rb", _SH_DENYNO)) {
        len = fread(buf, 1, sizeof(buf) - 1, f);
        fclose(f);
    }
    buf[len] = '\0';

    char out[sizeof(buf) + 32] = {};
    const char* key = "first_launch";
    const size_t keyLen = strlen(key);
    // Only at the start of a line, so the word inside a comment cannot be mistaken for the key.
    char* at = nullptr;
    for (char* p = buf; *p; ++p) {
        if ((p == buf || p[-1] == '\n') && strncmp(p, key, keyLen) == 0) { at = p; break; }
    }
    if (at) {
        const char* tail = strchr(at, '\n');
        if (!tail) tail = at + strlen(at);
        const size_t head = static_cast<size_t>(at - buf);
        memcpy(out, buf, head);
        const int n = _snprintf_s(out + head, sizeof(out) - head, _TRUNCATE,
                                  "%s=%d%s", key, value, tail);
        if (n < 0) return false;
    } else {
        _snprintf_s(out, sizeof(out), _TRUNCATE, "%s%s%s=%d\n",
                    buf, (len && buf[len - 1] != '\n') ? "\n" : "", key, value);
    }

    FILE* w = nullptr;
    if (fopen_s(&w, g_liveControlPath, "wb") != 0 || !w) return false;
    const size_t want = strlen(out);
    const size_t got = fwrite(out, 1, want, w);
    fclose(w);
    return got == want;
}

// ---- FIRST LAUNCH: install the game settings this port was tuned against ----------------------
//
// Cyberpunk's own settings do not live in the game folder. They are a single JSON under
// %LOCALAPPDATA%\CD Projekt Red\Cyberpunk 2077\UserSettings.json, and what a fresh install puts
// there is shaped for a monitor: motion blur, chromatic aberration, film grain, a 16:9 HUD and an
// upscaler preset picked for a flat screen. In a headset those range from unpleasant to unusable,
// and each one is a menu the player would otherwise have to go and find. So the port ships the
// settings it was actually developed and measured against, and installs them ONCE.
//
// Once, and provably once: first_launch lives in vrport.ini and it reads the way it is named --
// 1 means "this is the first launch, do it", and it is CLEARED to 0 only after the copy has
// succeeded, so a failure retries next launch instead of skipping forever. From then on the file
// belongs to the player: change anything in the game's own menus and it stays changed, because we
// never look at it again. Shipping a newer UserSettings.json with a release does not re-apply it
// either. Asking for it again means setting first_launch=1 by hand, a deliberate act.
//
// The file that was there is renamed beside itself with a timestamp, never simply overwritten.
//
// Called from the RED4ext entry, before the game creates its D3D12 device -- the earliest point we
// have. Whether the game has already read its settings by then is not something this can know, so
// the log says plainly that a fresh install may need one more launch for them to take.
// ONE SETTING'S VALUE, out of a UserSettings.json, without a JSON parser and without depending on
// whitespace -- the shipped file and the player's are indented differently (10 spaces against 20), so
// anything keyed on layout would compare two files that say the same thing and call them different.
//
// The shape is fixed by the game: an entry names itself and then carries its value.
//
//     "name": "CascadedShadowsRange",
//     "value": "Low",
//
// So: find the quoted setting name, then the next "value" after it, then the quoted string after that.
// Returns false when the setting is absent, which is treated as "cannot tell" rather than "differs".
static bool ReadSettingValue(const char* aPath, const char* aSetting, char* aOut, size_t aOutLen) {
    if (!aPath || !aSetting || !aOut || aOutLen == 0) return false;
    aOut[0] = '\0';

    FILE* f = _fsopen(aPath, "rb", _SH_DENYNO);
    if (!f) return false;
    fseek(f, 0, SEEK_END);
    const long size = ftell(f);
    fseek(f, 0, SEEK_SET);
    // A UserSettings.json measures 60-100 KB. The cap is a bound on nonsense, not on the game.
    if (size <= 0 || size > 4 * 1024 * 1024) { fclose(f); return false; }
    char* buf = static_cast<char*>(malloc(static_cast<size_t>(size) + 1));
    if (!buf) { fclose(f); return false; }
    const size_t got = fread(buf, 1, static_cast<size_t>(size), f);
    fclose(f);
    buf[got] = '\0';

    bool ok = false;
    char needle[128] = {};
    _snprintf_s(needle, sizeof(needle), _TRUNCATE, "\"%s\"", aSetting);
    if (const char* at = strstr(buf, needle)) {
        if (const char* v = strstr(at, "\"value\"")) {
            const char* q = strchr(v + 7, '"');
            if (q) {
                const char* end = strchr(q + 1, '"');
                if (end && static_cast<size_t>(end - q - 1) < aOutLen) {
                    const size_t n = static_cast<size_t>(end - q - 1);
                    memcpy(aOut, q + 1, n);
                    aOut[n] = '\0';
                    ok = true;
                }
            }
        }
    }
    free(buf);
    return ok;
}

// THE CASCADE GUARD. The shadow cascades are not a taste setting for this port: the second view
// rasterises the SHARED cascade atlas itself, so both views have to agree about it -- see the note in
// src/Stereo/ViewReuse.cpp and CyberpunkVR_CascadeSaveMain. A player (or a driver profile, or a preset
// the game reapplies) moving them off the shipped value is a visible fault in the headset, not a
// preference, so unlike everything else in this file it is CHECKED on every launch rather than installed
// once.
//
// Compared against the SHIPPED file rather than against a hardcoded "Low", so the value lives in exactly
// one place and a future release that ships a different one is followed automatically.
static const char* const kCascadeSettings[] = { "CascadedShadowsRange", "CascadedShadowsResolution" };

static bool CascadesDiffer(const char* aShipped, const char* aPlayers, char* aWhy, size_t aWhyLen) {
    for (const char* name : kCascadeSettings) {
        char want[64] = {}, have[64] = {};
        if (!ReadSettingValue(aShipped, name, want, sizeof(want))) continue;   // not ours to enforce
        if (!ReadSettingValue(aPlayers, name, have, sizeof(have))) continue;   // cannot tell
        if (_stricmp(want, have) != 0) {
            if (aWhy) _snprintf_s(aWhy, aWhyLen, _TRUNCATE, "%s is \"%s\", shipped is \"%s\"",
                                  name, have, want);
            return true;
        }
    }
    return false;
}

extern "C" void ApplyFirstLaunchGameSettings() {
    InitRuntimePaths();
    EnsureLiveControlFileExists();
    PollLiveControls();
    // TWO REASONS TO INSTALL, and they are not the same reason.
    //
    // first_launch is the original one-shot: a fresh install has never seen these settings. It is
    // decided here, but the CASCADE GUARD below can only be judged once both file paths are known, so
    // the early-out for "neither applies" happens after they are resolved.
    const bool firstLaunch = (g_liveControls.xrFirstLaunch != 0);

    // The shipped copy sits next to this DLL, which is the only directory the plugin owns.
    char src[MAX_PATH] = {};
    HMODULE self = nullptr;
    if (!GetModuleHandleExA(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                            GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCSTR>(&ApplyFirstLaunchGameSettings), &self) ||
        !self || !GetModuleFileNameA(self, src, MAX_PATH)) {
        Log("FirstLaunch: cannot locate this module -- settings not installed\n");
        return;
    }
    if (char* slash = strrchr(src, '\\')) *(slash + 1) = '\0';
    strcat_s(src, "UserSettings.json");
    if (GetFileAttributesA(src) == INVALID_FILE_ATTRIBUTES) {
        Log("FirstLaunch: no shipped UserSettings.json beside the plugin (%s) -- nothing to do, "
            "leaving first_launch=1\n", src);
        return;
    }

    char local[MAX_PATH] = {};
    if (!GetEnvironmentVariableA("LOCALAPPDATA", local, MAX_PATH)) {
        Log("FirstLaunch: LOCALAPPDATA is not set -- settings not installed\n");
        return;
    }
    char dst[MAX_PATH] = {};
    _snprintf_s(dst, sizeof(dst), _TRUNCATE,
                "%s\\CD Projekt Red\\Cyberpunk 2077\\UserSettings.json", local);

    // Absent means the game has never written its settings here, and dropping ours in would be
    // guessing at a layout we have not seen. Say so and try again next launch.
    if (GetFileAttributesA(dst) == INVALID_FILE_ATTRIBUTES) {
        Log("FirstLaunch: %s does not exist yet -- run the game once, then this applies\n", dst);
        return;
    }

    // NOW both files are known to exist, so the guard can be judged.
    char why[192] = {};
    const bool cascadeDrifted = CascadesDiffer(src, dst, why, sizeof(why));
    if (!firstLaunch && !cascadeDrifted) return;
    if (!firstLaunch) {
        Log("FirstLaunch: cascade guard tripped -- %s. Replacing the whole file, as the shadow\n"
            "             cascades are shared between the two views and cannot be left disagreeing.\n",
            why);
    }

    // THE BACKUP. On a first launch it is timestamped, as it always was -- that file is the player's
    // own pre-VR configuration and is worth keeping every copy of. The guard is a different case: it can
    // fire on any launch, so a timestamp there would leave a new 90 KB file behind every time. One
    // deterministic name, written only if it is not already there, keeps exactly one.
    SYSTEMTIME t{};
    GetLocalTime(&t);
    char bak[MAX_PATH] = {};
    if (firstLaunch) {
        _snprintf_s(bak, sizeof(bak), _TRUNCATE,
                    "%s\\CD Projekt Red\\Cyberpunk 2077\\UserSettings.pre-vr-%04u%02u%02u-%02u%02u%02u.json",
                    local, t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond);
        if (!CopyFileA(dst, bak, TRUE)) {
            Log("FirstLaunch: could not back up %s (err %lu) -- settings NOT installed\n",
                dst, GetLastError());
            return;
        }
    } else {
        _snprintf_s(bak, sizeof(bak), _TRUNCATE,
                    "%s\\CD Projekt Red\\Cyberpunk 2077\\UserSettings.pre-vr-cascade.json", local);
        // TRUE = do not overwrite: the first one is the interesting one, and a failure because it
        // already exists is not a failure at all.
        if (!CopyFileA(dst, bak, TRUE) && GetLastError() != ERROR_FILE_EXISTS) {
            Log("FirstLaunch: could not back up %s (err %lu) -- settings NOT installed\n",
                dst, GetLastError());
            return;
        }
    }
    if (!CopyFileA(src, dst, FALSE)) {
        Log("FirstLaunch: could not write %s (err %lu) -- the backup at %s is untouched\n",
            dst, GetLastError(), bak);
        return;
    }

    // Only now. Clearing the flag before the copy would silently skip it forever.
    g_liveControls.xrFirstLaunch = 0;
    const bool flagged = WriteFirstLaunchFlag(0);
    Log("FirstLaunch: installed the VR game settings\n"
        "             from %s\n"
        "             to   %s\n"
        "             previous settings kept at %s\n"
        "             first_launch=0 %s -- the game may need one more launch to read them\n",
        src, dst, bak, flagged ? "written to vrport.ini" : "COULD NOT BE WRITTEN (will retry)");
}
