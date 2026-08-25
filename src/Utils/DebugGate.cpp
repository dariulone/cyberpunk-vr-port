// debug_gate.cpp - every probe, census, dump and diagnostic log in the mod, in one list,
// behind the DEBUG tick-box in the launcher.
//
// WHY THE GATE IS A ONE-SHOT AND NOT A TEST AT EVERY READ.
//
// These flags sit in the hottest paths there are: a per-draw-call census, a per-CBV memcpy,
// a per-node dispatch hook. Turning each `if (flag)` into `if (flag && debugOn)` would put a
// second global load in all of them for a condition that cannot change during a session --
// the launcher writes it before the game starts. So the flags keep their plain test and
// ApplyLauncherDebugGate() simply zeroes them once, at plugin init, when DEBUG is unticked.
//
// The consequence is deliberate and useful: with DEBUG off you can still switch a single
// probe on live from x64dbg and it works, which is exactly how a one-off "why is this frame
// wrong" question gets answered without restarting the game.
//
// SOURCE DEFAULTS ARE THE "ON" VALUE. Ticking DEBUG turns EVERYTHING on rather than leaving
// you to hunt for which of thirty switches the answer lives behind. That is the point of the
// box, and it is why this file lists them: adding a probe means adding a line here, and
// forgetting to means the probe stays dark with DEBUG ticked.
//
// BE WARNED that everything on at once is not free. SightAxisProbe alone calls
// GetGPUVirtualAddress and takes a mutex on every CopyBufferRegion, descriptor-table bind and
// resource creation -- thousands of times a frame -- and CamCbProbe memcpys 848 bytes under a
// mutex for every constant-buffer view over 768 bytes (114195 of those in ten seconds,
// measured). DEBUG is for diagnosis, not for playing.

#include <cstdint>

extern void Log(const char* fmt, ...);
extern "C" int GetLauncherDebug();

// The verbose per-frame channel in vr_core.cpp -- not exported, so it is reached by extern.
extern volatile int g_verboseLog;

// ---- the flags ------------------------------------------------------------------------------
// Declared, not defined: each lives in the translation unit that uses it.
extern "C" {
// sync_stereo.cpp -- render-graph probes and censuses
// NOT gated: CyberpunkVR_StereoLog is the [stereo] channel itself, and it carries the
// hook-install confirmations. Silencing it would make a stereo module that failed to
// load look exactly like one that loaded fine. It has always shipped at 1 and costs
// about 50 KB a session; the probes below are what actually generate volume.
// The per-second and per-frame REPORT channels. Measured over one session without DEBUG ticked,
// these five produced 1.5 MB of the plugin log by themselves, and the CET mods another 4 MB; they
// were never in this list, so they shipped at 1 for everyone. Gated, not deleted: every one of them
// is the instrument some past bug was found with, and each is one checkbox away.
extern __declspec(dllexport) int  CyberpunkVR_XrRateLog;        // [xrrate]/[xrloop]/[xrcycle]/[xrorder]/[xrcadence]
extern __declspec(dllexport) int  CyberpunkVR_XrDeepDiag;       // the deep blocks inside those
extern __declspec(dllexport) int  CyberpunkVR_VrikRateLog;      // [vrik]/[rebind]/[heading]/[bodyyaw]/[yawphase]
extern __declspec(dllexport) int32_t CyberpunkVR_TemporalScan;  // [temporal]/[wide]
extern __declspec(dllexport) int32_t CyberpunkVR_CascFitProbe;  // [cascfit]/[cascrec]
extern __declspec(dllexport) int32_t CyberpunkVR_BlockDiffSize; // [blkwho]/[blkdiff]
extern __declspec(dllexport) int32_t CyberpunkVR_StageProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_LumaProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_CapCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_NodeCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_DrawCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_DispatchCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_IndirectCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_LightCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_CullCountProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_TileProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_ExpoProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_PsoProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_RtMapProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_CbvProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_CamCbProbe;
// The repeating half of the cascade-sampling report. The LEND itself is not listed here on purpose: it is a
// fix, not a probe, and it must work in a normal play session. Its first success logs one line regardless.
extern __declspec(dllexport) int32_t CyberpunkVR_CascSampleProbe;
// Every PIX marker the port writes into the command lists. Under the gate because the markers are for a
// CAPTURE, not for play: with DEBUG unticked they cost nothing at all, and a marker per node per view per
// command list is not free in a frame that has thousands of them.
extern __declspec(dllexport) int32_t CyberpunkVR_CaptureMarkers;
extern __declspec(dllexport) int32_t CyberpunkVR_CmdListHunt;
extern __declspec(dllexport) int32_t CyberpunkVR_CascSideCensus;
extern __declspec(dllexport) int32_t CyberpunkVR_ViewRectProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_EnvPtrProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_HudNodeProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_VolumeNodeProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_GradeCbProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_GradeUpProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_GradingProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_SkyProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_CloudLightProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_SightAxisProbe;
extern __declspec(dllexport) int32_t CyberpunkVR_SightPsDump;
extern __declspec(dllexport) int32_t CyberpunkVR_VisionDump;
extern __declspec(dllexport) int32_t CyberpunkVR_VisionMap;
extern __declspec(dllexport) int32_t CyberpunkVR_ViewDataDiff;
extern __declspec(dllexport) int32_t CyberpunkVR_DebugRtvPickLog;
extern __declspec(dllexport) uint32_t CyberpunkVR_LodThreshOverrideEnable;   // uint32_t, see below

// openxr_present.cpp
extern __declspec(dllexport) int CyberpunkVR_VrikRateLog;

// OpenXRFrameLoop.cpp -- the deep frame diagnostics: [xrwarp], [xrsrc], [xrage], [xreye], [xrgap],
// [xrcap], their printing AND the per-frame sampling behind them (a clock read per present and per
// submit). Built for the second-eye hunt, which is closed.
//
// NOT to be confused with CyberpunkVR_XrRateLog, which is absent from this table ON PURPOSE and says
// so where it is defined: [xrrate] and [xrloop] are the port's headline measurement and its
// frame-loop contract, and have to be readable in an ordinary session.
extern __declspec(dllexport) int CyberpunkVR_XrDeepDiag;
}

namespace {

// A flag and the value it takes when DEBUG is ticked. Most are a plain 1; the two that are not
// say why.
struct DebugFlag {
    const char*    name;
    volatile void* addr;
    int            onValue;
    bool           wide;      // true = int32_t/uint32_t, false = int
};

const DebugFlag kFlags[] = {
    { "XrRateLog",          &CyberpunkVR_XrRateLog,          1, false },
    { "XrDeepDiag",         &CyberpunkVR_XrDeepDiag,         1, false },
    { "VrikRateLog",        &CyberpunkVR_VrikRateLog,        1, false },
    { "TemporalScan",       &CyberpunkVR_TemporalScan,       1, true  },
    { "CascFitProbe",       &CyberpunkVR_CascFitProbe,       1, true  },
    // 384 rather than 1: this one is a SIZE, the fog block's, and it is the value the interior
    // investigation used. Zero means "no block census", which is what DEBUG-off must give.
    { "BlockDiffSize",      &CyberpunkVR_BlockDiffSize,      384, true  },
    { "StageProbe",         &CyberpunkVR_StageProbe,         1, true  },
    { "LumaProbe",          &CyberpunkVR_LumaProbe,          1, true  },
    { "CapCensus",          &CyberpunkVR_CapCensus,          1, true  },
    { "NodeCensus",         &CyberpunkVR_NodeCensus,         1, true  },
    { "DrawCensus",         &CyberpunkVR_DrawCensus,         1, true  },
    { "DispatchCensus",     &CyberpunkVR_DispatchCensus,     1, true  },
    { "IndirectCensus",     &CyberpunkVR_IndirectCensus,     1, true  },
    { "LightCensus",        &CyberpunkVR_LightCensus,        1, true  },
    { "CullCountProbe",     &CyberpunkVR_CullCountProbe,     1, true  },
    { "TileProbe",          &CyberpunkVR_TileProbe,          1, true  },
    { "ExpoProbe",          &CyberpunkVR_ExpoProbe,          1, true  },
    { "PsoProbe",           &CyberpunkVR_PsoProbe,           1, true  },
    { "RtMapProbe",         &CyberpunkVR_RtMapProbe,         1, true  },
    { "CbvProbe",           &CyberpunkVR_CbvProbe,           1, true  },
    { "CamCbProbe",         &CyberpunkVR_CamCbProbe,         1, true  },
    { "CascSampleProbe",    &CyberpunkVR_CascSampleProbe,    1, true  },
    { "CaptureMarkers",     &CyberpunkVR_CaptureMarkers,     1, true  },
    { "CmdListHunt",        &CyberpunkVR_CmdListHunt,        1, true  },
    { "CascSideCensus",     &CyberpunkVR_CascSideCensus,     1, true  },
    { "ViewRectProbe",      &CyberpunkVR_ViewRectProbe,      1, true  },
    { "EnvPtrProbe",        &CyberpunkVR_EnvPtrProbe,        1, true  },
    { "HudNodeProbe",       &CyberpunkVR_HudNodeProbe,       1, true  },
    { "VolumeNodeProbe",    &CyberpunkVR_VolumeNodeProbe,    1, true  },
    { "GradeCbProbe",       &CyberpunkVR_GradeCbProbe,       1, true  },
    { "GradeUpProbe",       &CyberpunkVR_GradeUpProbe,       1, true  },
    { "GradingProbe",       &CyberpunkVR_GradingProbe,       1, true  },
    { "SkyProbe",           &CyberpunkVR_SkyProbe,           1, true  },
    { "CloudLightProbe",    &CyberpunkVR_CloudLightProbe,    1, true  },
    { "SightAxisProbe",     &CyberpunkVR_SightAxisProbe,     1, true  },
    { "SightPsDump",        &CyberpunkVR_SightPsDump,        1, true  },
    { "VisionDump",         &CyberpunkVR_VisionDump,         1, true  },
    // ABSENT UNTIL NOW, and its absence cost three sessions. [vismap] is the only instrument that
    // can say why the scanner outline is missing from the second eye, and it is gated on this flag --
    // which DEBUG did not arm, so every one of those sessions printed nothing and the silence was read
    // as "no surface matched" when it only meant "the probe was off".
    { "VisionMap",          &CyberpunkVR_VisionMap,          1, true  },
    // Mode 2 prints every differing run with BOTH views' values. Mode 1 only shows the
    // MAIN-set/VRCAM-zero holes and is blind to "both have values, different ones", which is
    // the whole class of per-view bug this catches -- so 2 is the useful setting.
    { "ViewDataDiff",       &CyberpunkVR_ViewDataDiff,       2, true  },
    // A countdown, not a switch: it prints this many lines and then stops on its own.
    { "DebugRtvPickLog",    &CyberpunkVR_DebugRtvPickLog,   48, true  },
    { "LodThreshOverride",  &CyberpunkVR_LodThreshOverrideEnable, 1, true },
    // VrikRateLog LEFT THIS TABLE, for the same reason CyberpunkVR_XrRateLog was never in it: it is
    // the headline measurement of the thing being worked on, and it has to be readable in an ORDINARY
    // session. Ticking DEBUG to see it would arm thirty probes, and SightAxisProbe alone takes a mutex
    // per CopyBufferRegion -- so the frame time under DEBUG is not the frame time whose lag is being
    // judged. One line every two seconds, off plain counters, no shared block.
    //   { "VrikRateLog",     &CyberpunkVR_VrikRateLog,        1, false },
    // Gates SAMPLING as well as printing, which no other flag in this table does -- so with DEBUG
    // unticked the six deep [xr*] lines cost nothing at all, not merely nothing in the log. What it
    // does NOT gate is every counter that records a FAULT: the capture path's skip counters, the
    // bounded fence wait, the unpaired-eye count. Those only cost anything when something is already
    // wrong, and a fault counter reading zero because it was switched off is a diagnostic that lies.
    { "XrDeepDiag",         &CyberpunkVR_XrDeepDiag,         1, false },
};

}  // namespace

// Called once from InitStereoOnce, before any hook is installed and long before the first
// frame, so nothing has read a flag yet.
extern "C" void ApplyLauncherDebugGate() {
    const bool on = GetLauncherDebug() != 0;
    int applied = 0;
    for (const DebugFlag& f : kFlags) {
        const int v = on ? f.onValue : 0;
        if (f.wide) *static_cast<volatile int32_t*>(f.addr) = static_cast<int32_t>(v);
        else        *static_cast<volatile int*>(f.addr)     = v;
        ++applied;
    }
    g_verboseLog = on ? 1 : 0;
    Log("Debug gate: launcher DEBUG=%d -> %d probes %s. %s\n",
        on ? 1 : 0, applied, on ? "ARMED" : "silenced",
        on ? "Expect a large log and a heavy frame time; this is for diagnosis, not play."
           : "Flip one live in x64dbg if you need a single probe without restarting.");
}
