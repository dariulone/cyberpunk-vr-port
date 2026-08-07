// openvr_trackers.cpp - body tracking through the OpenVR API (fallback provider).
//
// WHY THIS EXISTS: SteamVR's OpenXR extension XR_HTCX_vive_tracker_interaction
// only enumerates genuine HTC Vive Trackers. Panda trackers (and other
// emulated trackers) appear in SteamVR's "Manage Vive Trackers" panel and work
// in every OpenVR game, but the HTCX extension reports zero devices for them
// (confirmed from a live log: enumerate -> XR_SUCCESS with count=0 for the
// whole session). OpenVR, however, sees them as TrackedDeviceClass_GenericTracker
// devices with full poses -- that is the path every FBT game on OpenVR uses,
// and the path this provider uses.
//
// WHAT IT DOES:
//   1. loads openvr_api.dll (shipped next to the game exe by the mod install)
//      and connects as a read-only OpenVR client (Background; Overlay/Utility
//      as fallbacks) alongside the game's OpenXR session,
//   2. scans for connected GenericTracker devices once a second,
//   3. assigns devices to body roles: BY SERIAL NUMBER when the tracker
//      names its role (the "human://LEFT_FOOT" style convention -- reliable
//      even with feet together), otherwise GEOMETRICALLY -- feet = the two
//      lowest trackers, left/right by which side of the body midline they
//      are on, waist = the remaining tracker nearest hip height on the
//      midline. The assignment is refreshed by every T-pose auto-calibration
//      (explicit rescan) and lazily retried for unmapped roles (trackers
//      woken up mid-session),
//   4. reads GetDeviceToAbsoluteTrackingPose each frame and writes the SAME
//      members the HTCX provider writes (m_trackers / m_trackerStagePos /
//      m_trackerStageValid), skipping any role HTCX already filled this frame.
//
// COORDINATES: OpenVR is right-handed, +Y up, -Z forward, meters -- identical
// to OpenXR (see the comment block at the top of openvr.h), and the OpenVR
// standing universe is floor-relative. NOTE: the mod's OpenXR reference space
// is NOT floor-relative (its origin sits at head level), so OpenVR poses must
// never be mixed with OpenXR-frame head poses. The HMD-local relative pose is
// frame-independent, so this provider builds it purely inside OpenVR (tracker
// and HMD both from GetDeviceToAbsoluteTrackingPose); the stage-space copy
// for calibration is the standing-universe pose verbatim.
#include "openxr_manager.h"
#include "openxr_internal.h"
#include "thirdparty/openvr.h"
#include <windows.h>
#include <cstring>
#include <vector>
#include <algorithm>

namespace {
    constexpr uint32_t kNoDevice = 0xFFFFFFFFu;
    constexpr const char* kRoleName[3] = { "L foot", "R foot", "waist" };

    // Foot/waist classification thresholds (stage-space Y, meters). Feet on
    // shoes read ~0.05-0.10 m; a belt puck reads ~0.85-1.10 m standing and
    // ~0.5 m crouched, so the bands overlap nothing a human leg can do.
    constexpr float kFootMaxY   = 0.40f;
    constexpr float kWaistMinY  = 0.40f;
    constexpr float kWaistMaxY  = 1.30f;
    constexpr float kMinFootSeparation = 0.04f;  // |latL - latR| below this is ambiguous

    using VR_InitInternalFn = uintptr_t (VR_CALLTYPE*)(vr::EVRInitError*, vr::EVRApplicationType);
    using VR_GetGenericInterfaceFn = void* (VR_CALLTYPE*)(const char*, vr::EVRInitError*);
    using VR_GetVRInitErrorAsEnglishDescriptionFn = const char* (VR_CALLTYPE*)(vr::EVRInitError);

    bool FileExistsLocal(const char* path) {
        return path && GetFileAttributesA(path) != INVALID_FILE_ATTRIBUTES;
    }

    // Same lookup the manager's runtime-JSON resolver uses: openvr_api.dll next
    // to the game exe first (the mod install ships it there), then the plain
    // DLL search path.
    HMODULE LoadOpenVRModule() {
        char gameDir[MAX_PATH]{};
        if (GetModuleFileNameA(nullptr, gameDir, MAX_PATH) > 0) {
            char* lastSlash = strrchr(gameDir, '\\');
            if (lastSlash) {
                *lastSlash = '\0';
                char localPath[MAX_PATH]{};
                const int n = snprintf(localPath, sizeof(localPath), "%s\\openvr_api.dll", gameDir);
                if (n > 0 && n < static_cast<int>(sizeof(localPath)) && FileExistsLocal(localPath)) {
                    HMODULE mod = LoadLibraryA(localPath);
                    if (mod) {
                        Log("OpenXRManager[FBT-OpenVR]: openvr_api.dll loaded from game dir.\n");
                        return mod;
                    }
                }
            }
        }
        HMODULE mod = LoadLibraryA("openvr_api.dll");
        if (mod) Log("OpenXRManager[FBT-OpenVR]: openvr_api.dll loaded from search path.\n");
        return mod;
    }

    const char* AppTypeName(vr::EVRApplicationType t) {
        switch (t) {
        case vr::VRApplication_Background: return "Background";
        case vr::VRApplication_Overlay:    return "Overlay";
        case vr::VRApplication_Utility:    return "Utility";
        default:                           return "?";
        }
    }

    // OpenVR HmdMatrix34_t: row-major 3x4, rotation in the 3x3, translation in
    // column 3. Both OpenVR and OpenXR are right-handed +Y up / -Z forward, so
    // the rotation converts to a quaternion directly, no axis juggling.
    void PoseFromOpenVRMatrix(const vr::HmdMatrix34_t& m, XrVector3f& outPos, XrQuaternionf& outQuat) {
        outPos.x = m.m[0][3];
        outPos.y = m.m[1][3];
        outPos.z = m.m[2][3];

        const float r00 = m.m[0][0], r01 = m.m[0][1], r02 = m.m[0][2];
        const float r10 = m.m[1][0], r11 = m.m[1][1], r12 = m.m[1][2];
        const float r20 = m.m[2][0], r21 = m.m[2][1], r22 = m.m[2][2];
        const float trace = r00 + r11 + r22;
        float x, y, z, w;
        if (trace > 0.0f) {
            const float s = sqrtf(trace + 1.0f) * 2.0f;
            w = 0.25f * s;
            x = (r21 - r12) / s;
            y = (r02 - r20) / s;
            z = (r10 - r01) / s;
        } else if (r00 > r11 && r00 > r22) {
            const float s = sqrtf(1.0f + r00 - r11 - r22) * 2.0f;
            w = (r21 - r12) / s;
            x = 0.25f * s;
            y = (r01 + r10) / s;
            z = (r02 + r20) / s;
        } else if (r11 > r22) {
            const float s = sqrtf(1.0f + r11 - r00 - r22) * 2.0f;
            w = (r02 - r20) / s;
            x = (r01 + r10) / s;
            y = 0.25f * s;
            z = (r12 + r21) / s;
        } else {
            const float s = sqrtf(1.0f + r22 - r00 - r11) * 2.0f;
            w = (r10 - r01) / s;
            x = (r02 + r20) / s;
            y = (r12 + r21) / s;
            z = 0.25f * s;
        }
        const float len = sqrtf(x * x + y * y + z * z + w * w);
        if (len > 1e-6f) {
            outQuat.x = x / len; outQuat.y = y / len; outQuat.z = z / len; outQuat.w = w / len;
        } else {
            outQuat.x = outQuat.y = outQuat.z = 0.0f; outQuat.w = 1.0f;
        }
    }

    bool PoseUsable(const vr::TrackedDevicePose_t& p) {
        return p.bDeviceIsConnected && p.bPoseIsValid &&
               p.eTrackingResult == vr::TrackingResult_Running_OK;
    }
} // namespace

void OpenXRManager::InitOpenVRTrackerSupport() {
    // Called from Init() right after InitViveTrackerSupport. Read-only client:
    // we never submit frames or claim input, we only read tracking poses, so a
    // Background-type connection coexists with the game's OpenXR session.
    if (m_openvrSystem) return;
    if (!IsRuntimeSteamVR()) {
        Log("OpenXRManager[FBT-OpenVR]: runtime is not SteamVR -- OpenVR tracker path off.\n");
        return;
    }

    HMODULE mod = LoadOpenVRModule();
    if (!mod) {
        Log("OpenXRManager[FBT-OpenVR]: openvr_api.dll not found -- OpenVR tracker path off.\n");
        return;
    }
    auto initInternal = reinterpret_cast<VR_InitInternalFn>(GetProcAddress(mod, "VR_InitInternal"));
    auto getInterface = reinterpret_cast<VR_GetGenericInterfaceFn>(GetProcAddress(mod, "VR_GetGenericInterface"));
    auto errorDesc = reinterpret_cast<VR_GetVRInitErrorAsEnglishDescriptionFn>(
        GetProcAddress(mod, "VR_GetVRInitErrorAsEnglishDescription"));
    if (!initInternal || !getInterface) {
        Log("OpenXRManager[FBT-OpenVR]: openvr_api.dll exports missing -- OpenVR tracker path off.\n");
        FreeLibrary(mod);
        return;
    }

    vr::EVRInitError err = vr::VRInitError_None;
    uintptr_t token = 0;
    vr::EVRApplicationType usedType = vr::VRApplication_Background;
    const vr::EVRApplicationType tryTypes[3] = {
        vr::VRApplication_Background, vr::VRApplication_Overlay, vr::VRApplication_Utility
    };
    for (vr::EVRApplicationType t : tryTypes) {
        err = vr::VRInitError_None;
        token = initInternal(&err, t);
        if (err == vr::VRInitError_None && token != 0) {
            usedType = t;
            break;
        }
    }
    if (err != vr::VRInitError_None || token == 0) {
        Log("OpenXRManager[FBT-OpenVR]: VR_Init failed (%d: %s) -- OpenVR tracker path off.\n",
            static_cast<int>(err), errorDesc ? errorDesc(err) : "?");
        FreeLibrary(mod);
        return;
    }

    vr::IVRSystem* sys = nullptr;
    const char* usedVersion = nullptr;
    const char* versions[3] = { "IVRSystem_022", "IVRSystem_021", "IVRSystem_020" };
    for (const char* v : versions) {
        err = vr::VRInitError_None;
        void* iface = getInterface(v, &err);
        if (err == vr::VRInitError_None && iface) {
            sys = reinterpret_cast<vr::IVRSystem*>(iface);
            usedVersion = v;
            break;
        }
    }
    if (!sys) {
        Log("OpenXRManager[FBT-OpenVR]: no usable IVRSystem interface (%d: %s) -- OpenVR tracker path off.\n",
            static_cast<int>(err), errorDesc ? errorDesc(err) : "?");
        FreeLibrary(mod);
        return;
    }

    m_openvrModule = mod;
    m_openvrSystem = sys;
    Log("OpenXRManager[FBT-OpenVR]: connected as %s app, %s -- OpenVR tracker path ON.\n",
        AppTypeName(usedType), usedVersion);
}

void OpenXRManager::RemapOpenVRTrackerRoles(bool explicitRescan) {
    // Geometric device->role assignment. Called on tracker-set changes, lazily
    // every 5 s while any role is unmapped, and with explicitRescan=true at
    // the start of every T-pose auto-calibration (the user-facing "assign my
    // trackers" moment: standing straight, feet apart, arms out).
    m_lastOpenvrRemap = FbtNowSeconds();
    if (!m_openvrSystem) return;
    vr::IVRSystem* sys = reinterpret_cast<vr::IVRSystem*>(m_openvrSystem);

    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount];
    sys->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding, 0.0f,
                                         poses, vr::k_unMaxTrackedDeviceCount);
    if (!PoseUsable(poses[vr::k_unTrackedDeviceIndex_Hmd])) {
        Log("OpenXRManager[FBT-OpenVR]: remap skipped -- HMD pose unavailable.\n");
        return;
    }

    XrVector3f headPos{};
    XrQuaternionf headQuat{};
    PoseFromOpenVRMatrix(poses[vr::k_unTrackedDeviceIndex_Hmd].mDeviceToAbsoluteTracking, headPos, headQuat);
    // Body midline = head facing projected onto the floor plane.
    XrVector3f right = RotateVector(headQuat, XrVector3f{ 1.0f, 0.0f, 0.0f });
    right.y = 0.0f;
    const float rightLen = sqrtf(right.x * right.x + right.z * right.z);
    if (rightLen < 1e-3f) {
        Log("OpenXRManager[FBT-OpenVR]: remap skipped -- HMD pointing straight up/down.\n");
        return;
    }
    right.x /= rightLen;
    right.z /= rightLen;

    if (explicitRescan) {
        m_openvrRoleDevice[0] = m_openvrRoleDevice[1] = m_openvrRoleDevice[2] = kNoDevice;
        Log("OpenXRManager[FBT-OpenVR]: T-pose rescan -- re-identifying trackers.\n");
    }

    // --- Pass 0: serial-number assignment. Trackers that carry their body
    // role in the serial string (the "human://LEFT_FOOT" style convention)
    // map DIRECTLY to that role -- no geometry needed. This is what makes a
    // feet-together T-pose reliable: the geometric midline test below goes
    // ambiguous when both feet touch the centre line, the serial never does.
    // Unmapped roles fall through to the geometric passes. Runs before the
    // candidate list is built so serially-mapped devices stay out of it.
    for (uint32_t d = 1; d < vr::k_unMaxTrackedDeviceCount; ++d) {
        if (sys->GetTrackedDeviceClass(d) != vr::TrackedDeviceClass_GenericTracker) continue;
        if (!PoseUsable(poses[d])) continue;
        bool alreadyMapped = false;
        for (int i = 0; i < kBodyTrackerCount; ++i)
            if (m_openvrRoleDevice[i] == d) alreadyMapped = true;
        if (alreadyMapped) continue;
        char serial[128] = { 0 };
        sys->GetStringTrackedDeviceProperty(d, vr::Prop_SerialNumber_String, serial, sizeof(serial), nullptr);
        char lc[128] = { 0 };
        for (int i = 0; i < 127 && serial[i]; ++i)
            lc[i] = (serial[i] >= 'A' && serial[i] <= 'Z') ? (serial[i] + 32) : serial[i];
        int role = -1;
        if (strstr(lc, "left_foot") || strstr(lc, "left-foot") || strstr(lc, "leftfoot")) role = 0;
        else if (strstr(lc, "right_foot") || strstr(lc, "right-foot") || strstr(lc, "rightfoot")) role = 1;
        else if (strstr(lc, "waist")) role = 2;
        if (role < 0 || m_openvrRoleDevice[role] != kNoDevice) continue;
        m_openvrRoleDevice[role] = d;
        Log("OpenXRManager[FBT-OpenVR]: mapped %s <- dev %u (serial \"%s\").\n",
            kRoleName[role], d, serial);
    }

    struct Cand { uint32_t dev; float y; float lat; };
    std::vector<Cand> cands;
    for (uint32_t d = 1; d < vr::k_unMaxTrackedDeviceCount; ++d) {
        if (sys->GetTrackedDeviceClass(d) != vr::TrackedDeviceClass_GenericTracker) continue;
        if (!PoseUsable(poses[d])) continue;
        bool alreadyMapped = false;
        for (int i = 0; i < kBodyTrackerCount; ++i)
            if (m_openvrRoleDevice[i] == d) alreadyMapped = true;
        if (alreadyMapped) continue;
        const vr::HmdMatrix34_t& m = poses[d].mDeviceToAbsoluteTracking;
        const float lat = (m.m[0][3] - headPos.x) * right.x + (m.m[2][3] - headPos.z) * right.z;
        cands.push_back({ d, m.m[1][3], lat });
    }
    std::sort(cands.begin(), cands.end(), [](const Cand& a, const Cand& b) { return a.y < b.y; });

    auto eraseCand = [&](uint32_t dev) {
        for (size_t k = 0; k < cands.size(); ++k)
            if (cands[k].dev == dev) { cands.erase(cands.begin() + k); return; }
    };
    auto lateralOf = [&](uint32_t dev) {
        const vr::HmdMatrix34_t& m = poses[dev].mDeviceToAbsoluteTracking;
        return (m.m[0][3] - headPos.x) * right.x + (m.m[2][3] - headPos.z) * right.z;
    };

    // --- Feet: the lowest trackers. Left/right by which side of the midline
    // they are on (lat < 0 = left, since `right` is the head's +X).
    const bool needL = (m_openvrRoleDevice[0] == kNoDevice);
    const bool needR = (m_openvrRoleDevice[1] == kNoDevice);
    if (needL && needR) {
        std::vector<Cand> feet;
        for (const Cand& c : cands)
            if (c.y < kFootMaxY) feet.push_back(c);
        if (feet.size() >= 2) {
            Cand a = feet[0], b = feet[1];
            if (fabsf(a.lat - b.lat) < kMinFootSeparation) {
                Log("OpenXRManager[FBT-OpenVR]: feet ambiguous (both at midline, |dlat|=%.3f) -- stand with feet apart and T-pose calibrate.\n",
                    fabsf(a.lat - b.lat));
            } else {
                const Cand& lf = (a.lat < b.lat) ? a : b;
                const Cand& rf = (a.lat < b.lat) ? b : a;
                m_openvrRoleDevice[0] = lf.dev;
                m_openvrRoleDevice[1] = rf.dev;
                eraseCand(lf.dev);
                eraseCand(rf.dev);
                Log("OpenXRManager[FBT-OpenVR]: mapped L foot <- dev %u (y=%.2f lat=%.2f), R foot <- dev %u (y=%.2f lat=%.2f).\n",
                    lf.dev, lf.y, lf.lat, rf.dev, rf.y, rf.lat);
            }
        } else if (feet.size() == 1) {
            // Single foot tracker: side guessed from the midline sign.
            const Cand& f = feet[0];
            const int role = (f.lat <= 0.0f) ? 0 : 1;
            m_openvrRoleDevice[role] = f.dev;
            eraseCand(f.dev);
            Log("OpenXRManager[FBT-OpenVR]: mapped %s <- dev %u (y=%.2f lat=%.2f, single tracker guess).\n",
                kRoleName[role], f.dev, f.y, f.lat);
        }
    } else if (needL != needR) {
        // One foot already mapped: the other must read near the SAME height
        // (rejects a crouch-low waist puck) and on the correct side.
        const int haveRole = needL ? 1 : 0;
        const int wantRole = needL ? 0 : 1;
        const uint32_t haveDev = m_openvrRoleDevice[haveRole];
        const float haveY = poses[haveDev].mDeviceToAbsoluteTracking.m[1][3];
        const float haveLat = lateralOf(haveDev);
        for (const Cand& c : cands) {
            if (c.y >= kFootMaxY) break;
            const bool sideOk = (wantRole == 0) ? (c.lat < haveLat) : (c.lat > haveLat);
            if (fabsf(c.y - haveY) < 0.25f && sideOk) {
                m_openvrRoleDevice[wantRole] = c.dev;
                Log("OpenXRManager[FBT-OpenVR]: mapped %s <- dev %u (y=%.2f lat=%.2f, paired with dev %u).\n",
                    kRoleName[wantRole], c.dev, c.y, c.lat, haveDev);
                eraseCand(c.dev);
                break;
            }
        }
    }

    // --- Waist: the remaining tracker nearest hip height, closest to midline.
    if (m_openvrRoleDevice[2] == kNoDevice) {
        int best = -1;
        for (size_t k = 0; k < cands.size(); ++k) {
            if (cands[k].y < kWaistMinY || cands[k].y > kWaistMaxY) continue;
            if (best < 0 || fabsf(cands[k].lat) < fabsf(cands[best].lat)) best = static_cast<int>(k);
        }
        if (best >= 0) {
            const Cand& w = cands[best];
            m_openvrRoleDevice[2] = w.dev;
            Log("OpenXRManager[FBT-OpenVR]: mapped waist <- dev %u (y=%.2f lat=%.2f).\n", w.dev, w.y, w.lat);
            eraseCand(w.dev);
        } else if (explicitRescan) {
            Log("OpenXRManager[FBT-OpenVR]: no waist-height tracker found (fine for 2-point feet-only tracking).\n");
        }
    }
}

void OpenXRManager::PollOpenVRTrackers(const XrPosef& headPoseStage) {
    // CALLER HOLDS m_handMutex (runs at the tail of PollViveTrackers and writes
    // the same m_trackers / m_trackerStage* members).
    if (!m_openvrSystem) return;
    vr::IVRSystem* sys = reinterpret_cast<vr::IVRSystem*>(m_openvrSystem);

    vr::TrackedDevicePose_t poses[vr::k_unMaxTrackedDeviceCount];
    sys->GetDeviceToAbsoluteTrackingPose(vr::TrackingUniverseStanding, 0.0f,
                                         poses, vr::k_unMaxTrackedDeviceCount);

    const double now = FbtNowSeconds();

    // 1 Hz device scan: count trackers, fingerprint the connected set, log
    // first-seen devices, drop mappings whose device went away.
    if (now - m_lastOpenvrScan > 1.0) {
        m_lastOpenvrScan = now;
        uint64_t mask = 0;
        int n = 0;
        for (uint32_t d = 0; d < vr::k_unMaxTrackedDeviceCount; ++d) {
            if (sys->GetTrackedDeviceClass(d) != vr::TrackedDeviceClass_GenericTracker) continue;
            if (!sys->IsTrackedDeviceConnected(d)) continue;
            mask |= (1ull << d);
            ++n;
            if ((m_openvrDeviceSetMask & (1ull << d)) == 0) {
                char serial[128] = { 0 };
                char model[128] = { 0 };
                sys->GetStringTrackedDeviceProperty(d, vr::Prop_SerialNumber_String, serial, sizeof(serial), nullptr);
                sys->GetStringTrackedDeviceProperty(d, vr::Prop_ModelNumber_String, model, sizeof(model), nullptr);
                Log("OpenXRManager[FBT-OpenVR]: tracker dev %u connected (serial=\"%s\" model=\"%s\").\n", d, serial, model);
            }
        }
        if (mask != m_openvrDeviceSetMask) {
            for (int i = 0; i < kBodyTrackerCount; ++i) {
                if (m_openvrRoleDevice[i] != kNoDevice && (mask & (1ull << m_openvrRoleDevice[i])) == 0) {
                    Log("OpenXRManager[FBT-OpenVR]: %s device %u disconnected -- role unmapped.\n",
                        kRoleName[i], m_openvrRoleDevice[i]);
                    m_openvrRoleDevice[i] = kNoDevice;
                }
            }
            m_openvrDeviceSetMask = mask;
            if (n > 0) RemapOpenVRTrackerRoles(false);
        }
        if (n != m_openvrTrackerCount) {
            m_openvrTrackerCount = n;
            Log("OpenXRManager[FBT-OpenVR]: %d generic tracker(s) connected.\n", n);
        }
    }

    // Lazy remap while any role is unmapped (trackers woken/put on mid-session).
    if ((m_openvrRoleDevice[0] == kNoDevice || m_openvrRoleDevice[1] == kNoDevice ||
         m_openvrRoleDevice[2] == kNoDevice) && now - m_lastOpenvrRemap > 5.0) {
        RemapOpenVRTrackerRoles(false);
    }

    // Per-frame pose fill for every role the HTCX path left invalid.
    //
    // CRITICAL FRAME RULE: the HMD-local relative pose must be built from a
    // head pose in the SAME frame as the tracker poses. The OpenXR head pose
    // passed in (headPoseStage) lives in the mod's OpenXR reference space,
    // whose origin sits at HEAD level, not on the floor -- mixing it with
    // floor-relative OpenVR standing-universe poses once shipped feet glued
    // to head height (relWorld.y ~= 0) and the avatar folded into a U. The
    // head-relative pose is frame-independent, so build it purely inside
    // OpenVR from the OpenVR HMD pose instead; it comes out identical to what
    // a consistent OpenXR pair would produce.
    (void)headPoseStage;
    if (!PoseUsable(poses[vr::k_unTrackedDeviceIndex_Hmd])) {
        for (int i = 0; i < kBodyTrackerCount; ++i) {
            if (m_trackers[i].valid) continue;      // HTCX provider owns this role
            m_trackers[i].valid = false;
            m_trackerStageValid[i] = false;
            m_trackerFilterState[i].initialized = false;
        }
        return;
    }
    XrVector3f headPos{};
    XrQuaternionf headQuat{};
    PoseFromOpenVRMatrix(poses[vr::k_unTrackedDeviceIndex_Hmd].mDeviceToAbsoluteTracking, headPos, headQuat);
    m_openvrHmdStageY = headPos.y;              // floor-relative HMD height for calibration
    const XrQuaternionf headInv = ConjugateQuat(headQuat);

    const float smooth = GetHandTrackingSmooth();
    for (int i = 0; i < kBodyTrackerCount; ++i) {
        if (m_trackers[i].valid) continue;          // HTCX provider owns this role
        m_trackerStageValid[i] = false;
        const uint32_t d = m_openvrRoleDevice[i];
        if (d >= vr::k_unMaxTrackedDeviceCount || !PoseUsable(poses[d])) {
            m_trackers[i].valid = false;
            m_trackerFilterState[i].initialized = false;
            continue;
        }

        XrVector3f stagePos{};
        XrQuaternionf stageQuat{};
        PoseFromOpenVRMatrix(poses[d].mDeviceToAbsoluteTracking, stagePos, stageQuat);
        m_trackerStagePos[i] = stagePos;        // floor-relative, feeds calibration
        m_trackerStageValid[i] = true;

        // HMD-local form, identical to the HTCX/hand pipeline.
        XrVector3f relWorld{
            stagePos.x - headPos.x,
            stagePos.y - headPos.y,
            stagePos.z - headPos.z,
        };
        XrPosef relPose{};
        relPose.position = RotateVector(headInv, relWorld);
        relPose.orientation = MultiplyQuat(headInv, stageQuat);
        XrPosef filtered = FilterTrackerPose(m_trackerFilterState[i], relPose, smooth);

        m_trackers[i].posX = filtered.position.x;
        m_trackers[i].posY = filtered.position.y;
        m_trackers[i].posZ = filtered.position.z;
        m_trackers[i].oriX = filtered.orientation.x;
        m_trackers[i].oriY = filtered.orientation.y;
        m_trackers[i].oriZ = filtered.orientation.z;
        m_trackers[i].oriW = filtered.orientation.w;
        m_trackers[i].valid = true;
    }

    // Overlay count: when HTCX sees nothing, report the OpenVR-connected set.
    if (m_viveTrackerCount.load(std::memory_order_relaxed) == 0 && m_openvrTrackerCount > 0) {
        m_viveTrackerCount.store(
            m_openvrTrackerCount > kBodyTrackerCount ? kBodyTrackerCount : m_openvrTrackerCount,
            std::memory_order_relaxed);
    }
}
