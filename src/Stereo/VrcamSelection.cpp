// VrcamSelection -- deciding WHICH second-eye camera this session uses, and noticing when the
// answer changes.
//
// Four coupled fields (component name, virtual-camera name, the identity hash derived from it, and
// the resolution) are resolved together from vrcam.json and the launcher ini, because resolving any
// one of them alone is how the render target and the component came to disagree. The watcher thread
// exists because the CET side can enable a different camera after we have already read the file, and
// because the launcher pick arrives after our init-time read.
//
// Lifted out of src/Stereo/SyncStereo.cpp as the first of its subsystems, now that the file-wide
// anonymous namespace is a named one and these functions can be called across a file boundary.

#include "Stereo/SyncStereo.hpp"
#include "Core/Paths.hpp"
#include "Utils/StereoLog.hpp"
#include "Stereo/VrcamConfig.hpp"   // vrcam.json access + CName hashing, shared with the launcher
#include "Render/ColorBlit.hpp"   // HUD debug overlay on the mirror image
#include <windows.h>
#include <d3d12.h>
#include <d3d11.h>
#include <d3d11on12.h>
#include <dxgi1_4.h>
#include <intrin.h>
#include <array>
#include <atomic>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <vector>
#include "MinHook.h"
#include "Utils/LogThrottle.hpp"
#include "Stereo/StereoInternal.hpp"

extern "C" int GetLaunchEyeSize(int* width, int* height);   // Core/VrCore.cpp

namespace cvr {
namespace detail {

// THE LAUNCHER PICK ARRIVES AFTER WE HAVE ALREADY READ IT.
//
// vrport-launcher.ini is written by the dxgi proxy's dialog, and that dialog runs when the
// swapchain is created. This plugin is a RED4ext plugin: it initialises long before that. So the
// read below, done once at init, returns the resolution of the PREVIOUS session, and the VRCAM
// component is picked one launch behind -- pick 3072 and the log says
//     [vrcam] launcher picked 2560x2560 -> switching component ...
// while a hundred lines later the same log says
//     CreateSwapChainForHwnd override: 1024x768 -> 3072x3072
// The file was never wrong; we simply looked at it too early.
//
// So the read is a function now, and the watcher below repeats it every poll. Whatever the
// resolution turns out to be once the swapchain exists, the component follows it.
static bool vrcam_launcher_resolution(int* w, int* h) {
    // Once this launch's eye size is decided it is the one: the pick may have been fitted to what the
    // runtime takes (FitLauncherResolutionToRuntime), and the file keeps the pick.
    if (GetLaunchEyeSize(w, h)) return true;
    char iniPath[MAX_PATH] = {};
    if (strcpy_s(iniPath, cvr::paths::FileA(cvr::paths::Folder::Config, "vrport-launcher.ini").c_str()) != 0)
        return false;
    FILE* f = fopen(iniPath, "rb");
    if (!f) return false;
    char buf[512] = {};
    const size_t n = fread(buf, 1, sizeof(buf) - 1, f);
    fclose(f);
    buf[n] = 0;
    int lw = 0, lh = 0;
    if (const char* p = strstr(buf, "width=")) lw = atoi(p + 6);
    if (const char* p = strstr(buf, "height=")) lh = atoi(p + 7);
    if (lw <= 0 || lh <= 0) return false;
    if (w) *w = lw;
    if (h) *h = lh;
    return true;
}


static void vrcam_apply_selection(const char* component, const char* camera) {
    strncpy_s(g_vrcam_component, sizeof(g_vrcam_component), component, _TRUNCATE);
    strncpy_s(g_vrcam_camera, sizeof(g_vrcam_camera), camera, _TRUNCATE);
    g_vrcam_ctx_key.store(cname_hash(g_vrcam_camera));
    int w = 0, h = 0;
    // Resolution off the COMPONENT name, falling back to the camera's -- both carry <W>x<H>.
    if (vrcam_parse_resolution(g_vrcam_component, &w, &h) ||
        vrcam_parse_resolution(g_vrcam_camera, &w, &h)) {
        g_vrcam_sel_w.store((uint32_t)w);
        g_vrcam_sel_h.store((uint32_t)h);
    }
    cvr::log("[vrcam] selection applied: component=%s camera=%s key=0x%016llX res=%dx%d%s",
            g_vrcam_component, g_vrcam_camera,
            (unsigned long long)g_vrcam_ctx_key.load(), w, h,
            g_vrcam_pick_authoritative ? " (from launcher resolution)" : "");
}

// Read the selection file and derive the view key from it. Called once from
// sync_stereo_init(), i.e. before any hook can observe a view.
void load_vrcam_selection() {
    std::string text;
    char path[MAX_PATH] = {};
    if (!vrcam_config_read(&text, path, sizeof(path))) {
        cvr::log("[vrcam] no vrcam.json (looked at %s) -> keeping default component=%s camera=%s "
                "key=0x%016llX", path, g_vrcam_component, g_vrcam_camera,
                (unsigned long long)g_vrcam_ctx_key);
        return;
    }
    std::string comp;
    if (!json_find_string(text, "component", &comp)) {
        cvr::log("[vrcam] %s has no \"component\" field -> keeping default %s",
                path, g_vrcam_component);
        return;
    }

    // THE LAUNCHER PICK WINS over a stale "component" field.
    //
    // Two files describe one decision: vrport-launcher.ini carries the resolution the player
    // chose, vrcam.json carries which VRCAM component is active. When they disagree the
    // symptom is silent and total -- the view key is hashed from the CAMERA name, so a
    // 2444 key against a running 2560 view matches nothing: no second eye, no mirror window,
    // and every log line still cheerfully naming the component from the file. Exactly the
    // "component is definitely on but there is no second eye" case.
    //
    // So: if the launcher's resolution names a component the entity actually has (it must be
    // in the authored "components" list -- selecting one that was never imported would render
    // nothing at all), that one wins, and it is written back so the CET side, which reads the
    // same file to decide what to enable, agrees with us.
    {
        int lw = 0, lh = 0;
        vrcam_launcher_resolution(&lw, &lh);
        if (lw > 0 && lh > 0) {
            char wanted[128] = {};
            _snprintf_s(wanted, sizeof(wanted), _TRUNCATE, "vrcam_%dx%d", lw, lh);
            if (comp != wanted) {
                std::vector<std::string> authored;
                json_find_string_array(text, "components", &authored);
                bool exists = false;
                for (const std::string& a : authored) if (a == wanted) { exists = true; break; }
                if (exists) {
                    cvr::log("[vrcam] launcher picked %dx%d -> switching component %s -> %s",
                            lw, lh, comp.c_str(), wanted);
                    comp = wanted;
                    // Authoritative from here on: the watcher must not adopt a stale live
                    // camera over this, or the selection oscillates and never settles.
                    g_vrcam_pick_authoritative = true;
                    if (vrcam_config_write_component(wanted))
                        cvr::log("[vrcam] vrcam.json updated so the CET side enables the same one");
                    else
                        cvr::log("[vrcam] WARNING: could not write vrcam.json -- the CET side may "
                                "still enable something else, leaving no matching view");
                } else {
                    cvr::log("[vrcam] launcher picked %dx%d but %s is not in the authored list "
                            "-> keeping %s (import the component, then add it to \"components\")",
                            lw, lh, wanted, comp.c_str());
                }
            }
        }
    }
    // The camera name is what the view key is hashed from, so it MUST match the component.
    // An explicit "virtualCamera" is only honoured when it agrees with the component; a stale
    // one (e.g. the component was changed and that field was not) is ignored with a warning
    // instead of silently producing a key that matches no view -- the failure mode there is
    // "no stereo and no mirror window", with every log line still naming the right component.
    char derived[128] = {};
    const bool can_derive = vrcam_derive_camera(comp.c_str(), derived, sizeof(derived));
    std::string cam;
    const bool explicit_cam = json_find_string(text, "virtualCamera", &cam);
    if (explicit_cam && can_derive && cam != derived) {
        cvr::log("[vrcam] WARNING: \"virtualCamera\"=%s does not match component %s "
                "(expected %s) -> using %s", cam.c_str(), comp.c_str(), derived, derived);
        cam = derived;
    } else if (!explicit_cam) {
        if (!can_derive) {
            cvr::log("[vrcam] component %s has no \"vrcam_\" prefix and no \"virtualCamera\" "
                    "given -> cannot derive the camera name, keeping default %s",
                    comp.c_str(), g_vrcam_camera);
            return;
        }
        cam = derived;
    }
    vrcam_apply_selection(comp.c_str(), cam.c_str());
    CyberpunkVR_DebugVrcamConfigLoaded = 1;
    cvr::log("[vrcam] selection source: %s", path);
}

// The camera name above is DERIVED from the component name, which assumes the asset follows
// the vrcam_feed_<suffix> convention. When it does not -- a component renamed in WolvenKit whose
// virtualCameraName kept the old value -- the key matches no view and the entire VR path goes
// quiet (no stereo, no mirror) while every log line still shows the right component.
// So the authored name is treated as a HINT and the ground truth comes from the live entity:
// modules/vrcam_select.lua reads virtualCameraName off the component it actually enabled and
// writes it to bridge/vrcam_active.txt; this watcher adopts it.
void vrcam_active_watcher() {
    char path[MAX_PATH] = {};
    if (!vrcam_bridge_path("vrcam_active.txt", path, sizeof(path))) return;
    std::string last;
    uint64_t disagreeSince = 0;      // first tick of a disagreement that produced no nodes
    uint64_t lastRequest = 0;        // rate-limit the re-request writes
    uint64_t lastNodeHits = 0;
    int seenW = 0, seenH = 0;         // last launcher resolution acted on
    // A name the previous session wrote is not a live camera. Until vrcam_select.lua writes this
    // session's, the file still holds the last one; adopting it swapped the right selection for a
    // component that may no longer exist (seen after the Quest 3 ladder changed: 2944x2560 adopted
    // in the menu, the real 2648x2864 only once gameplay rewrote the file).
    FILETIME processStart{}, unusedTimes[3]{};
    const bool haveStart = GetProcessTimes(GetCurrentProcess(), &processStart, &unusedTimes[0],
                                           &unusedTimes[1], &unusedTimes[2]) != FALSE;
    for (;;) {
        Sleep(500);

        // RE-READ THE LAUNCHER PICK. See vrcam_launcher_resolution: the init-time read happens
        // before the proxy's dialog has written the file, so it always returns the previous
        // session's resolution. Here the swapchain exists and the file is current.
        {
            int lw = 0, lh = 0;
            if (vrcam_launcher_resolution(&lw, &lh) && (lw != seenW || lh != seenH)) {
                seenW = lw; seenH = lh;
                char wanted[128] = {};
                _snprintf_s(wanted, sizeof(wanted), _TRUNCATE, "vrcam_%dx%d", lw, lh);
                if (strcmp(wanted, g_vrcam_component) != 0) {
                    std::string text;
                    char cfg[MAX_PATH] = {};
                    std::vector<std::string> authored;
                    bool exists = false;
                    if (vrcam_config_read(&text, cfg, sizeof(cfg)) &&
                        json_find_string_array(text, "components", &authored)) {
                        for (const std::string& a : authored)
                            if (a == wanted) { exists = true; break; }
                    }
                    char camera[128] = {};
                    if (exists && vrcam_derive_camera(wanted, camera, sizeof(camera))) {
                        cvr::log("[vrcam] launcher resolution is %dx%d (read after the swapchain "
                                 "existed) -> switching %s -> %s", lw, lh, g_vrcam_component, wanted);
                        vrcam_apply_selection(wanted, camera);
                        g_vrcam_pick_authoritative = true;
                        vrcam_config_write_component(wanted);
                        vrcam_bridge_write("vrcam_enable.txt", "1");
                        last.clear();
                        disagreeSince = 0;
                        continue;
                    }
                    if (!exists)
                        cvr::log("[vrcam] launcher resolution is %dx%d but %s is not in the "
                                 "authored \"components\" list -> keeping %s",
                                 lw, lh, wanted, g_vrcam_component);
                }
            }
        }
        if (haveStart) {
            WIN32_FILE_ATTRIBUTE_DATA attributes{};
            if (!GetFileAttributesExA(path, GetFileExInfoStandard, &attributes) ||
                CompareFileTime(&attributes.ftLastWriteTime, &processStart) < 0)
                continue;
        }
        FILE* f = fopen(path, "rb");
        if (!f) continue;
        char buf[128] = {};
        const size_t n = fread(buf, 1, sizeof(buf) - 1, f);
        fclose(f);
        buf[n] = '\0';
        for (char* p = buf; *p; ++p)                      // trim trailing whitespace/newlines
            if (*p == '\r' || *p == '\n' || *p == ' ' || *p == '\t') { *p = '\0'; break; }
        if (buf[0] == '\0') continue;                     // empty = VRCAM disabled, keep the key
        if (last == buf) continue;
        last = buf;
        const uint64_t key = cname_hash(buf);
        if (key == g_vrcam_ctx_key.load()) {
            cvr::log("[vrcam] live camera confirms key: %s", buf);
            continue;
        }
        if (g_vrcam_pick_authoritative) {
            // The launcher resolution decided this, so CET is the one out of step -- do NOT
            // adopt. Adopting is what made the selection oscillate between the two resolutions
            // on every poll, and the view key with it, so nothing downstream ever settled.
            // Re-state the request and give CET a few seconds to catch up.
            // Give up on EVIDENCE, never on a stopwatch alone.
            //
            // VRCAM lives on the player entity, so it only exists in gameplay: a plain attempt
            // counter burns through its budget in the main menu -- against a stale name left in
            // vrcam_active.txt by the previous session -- and abandons a perfectly good pick
            // before the game has even loaded. So the clock only counts while our own view is
            // producing NOTHING (node hits frozen at zero) and something else is being reported
            // live. In the menu nothing renders either way, and the moment gameplay starts with
            // the right component the hits move and this resets.
            const uint64_t nodeHits = CyberpunkVR_DebugVrcamNodeHits;
            if (nodeHits != lastNodeHits) { lastNodeHits = nodeHits; disagreeSince = 0; }
            if (nodeHits != 0) { last.clear(); continue; }   // our view IS rendering -- fine
            const uint64_t nowTick = GetTickCount64();
            if (!disagreeSince) disagreeSince = nowTick;
            if (nowTick - disagreeSince < 30000) {
                if (nowTick - lastRequest >= 2000) {         // re-state, but not every poll
                    lastRequest = nowTick;
                    cvr::log("[vrcam] live camera is %s but the launcher picked %s -> NOT "
                            "adopting; re-requesting %s", buf, g_vrcam_camera, g_vrcam_component);
                    vrcam_config_write_component(g_vrcam_component);
                    vrcam_bridge_write("vrcam_enable.txt", "1");
                }
                last.clear();               // re-evaluate on the next poll
                continue;
            }
            // It never switched. Almost always this means the component the launcher's
            // resolution names is not actually ON THE PLAYER ENTITY -- listing it in
            // vrcam.json does not create it; it has to be imported. Say so once, then take
            // the live camera so there IS a second eye, rather than holding out for a
            // component that is never coming and showing nothing at all.
            cvr::log("[vrcam] GIVING UP on %s -- 30 s of gameplay with another camera live and "
                    "zero nodes of our own, so the entity most likely has no such component "
                    "(listing it in vrcam.json does not create it; the asset has to be "
                    "imported). Falling back to the live camera %s.",
                    g_vrcam_component, buf);
            g_vrcam_pick_authoritative = false;
        }
        disagreeSince = 0;
        cvr::log("[vrcam] live camera is %s (key 0x%016llX), not %s -> adopting the live one "
                "(no launcher pick to honour); check virtualCameraName on component %s",
                buf, (unsigned long long)key, g_vrcam_camera, g_vrcam_component);
        vrcam_apply_selection(g_vrcam_component, buf);
    }
}

}  // namespace detail
}  // namespace cvr
