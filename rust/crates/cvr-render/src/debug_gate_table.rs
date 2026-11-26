//! The launcher's DEBUG gate's table (formerly src/Utils/DebugGate.cpp's kFlags): every probe, census, dump and
//! diagnostic log, by its exported C name, with the value it takes when DEBUG is ticked -- most a plain 1; the fog
//! block's census size 384, the view-data diff's mode 2 (the one that sees both views' values), the RTV pick log's
//! countdown 48. The gate itself is cvr_host::debug_gate.
//!
//! NOT IN THIS TABLE, ON PURPOSE: CyberpunkVR_IndirectCensus and CyberpunkVR_ExpoProbe. Both carry their own live
//! keys and the gate runs after the first poll, so a row here would write 0 over a key that had just been set and the
//! next ini write-back would persist it. The rule: a flag with its own live key does not belong in the launcher gate.
#![allow(non_upper_case_globals)]
use cvr_host::debug_gate::Flag;

extern "C" {
    static mut CyberpunkVR_RuntimeDiagnostics: i32;
    static mut CyberpunkVR_XrRateLog: i32;
    static mut CyberpunkVR_XrDeepDiag: i32;
    static mut CyberpunkVR_VrikRateLog: i32;
    static mut CyberpunkVR_TemporalScan: i32;
    static mut CyberpunkVR_WideCensus: i32;
    static mut CyberpunkVR_CascFitProbe: i32;
    static mut CyberpunkVR_BlockDiffSize: i32;
    static mut CyberpunkVR_StageProbe: i32;
    static mut CyberpunkVR_LumaProbe: i32;
    static mut CyberpunkVR_CapCensus: i32;
    static mut CyberpunkVR_NodeCensus: i32;
    static mut CyberpunkVR_DrawCensus: i32;
    static mut CyberpunkVR_DispatchCensus: i32;
    static mut CyberpunkVR_LightCensus: i32;
    static mut CyberpunkVR_CullCountProbe: i32;
    static mut CyberpunkVR_TileProbe: i32;
    static mut CyberpunkVR_PsoProbe: i32;
    static mut CyberpunkVR_RtMapProbe: i32;
    static mut CyberpunkVR_CbvProbe: i32;
    static mut CyberpunkVR_CamCbProbe: i32;
    static mut CyberpunkVR_CascSampleProbe: i32;
    static mut CyberpunkVR_CaptureMarkers: i32;
    static mut CyberpunkVR_CmdListHunt: i32;
    static mut CyberpunkVR_CascSideCensus: i32;
    static mut CyberpunkVR_ViewRectProbe: i32;
    static mut CyberpunkVR_EnvPtrProbe: i32;
    static mut CyberpunkVR_HudNodeProbe: i32;
    static mut CyberpunkVR_VolumeNodeProbe: i32;
    static mut CyberpunkVR_GradeUpProbe: i32;
    static mut CyberpunkVR_GradingProbe: i32;
    static mut CyberpunkVR_SkyProbe: i32;
    static mut CyberpunkVR_CloudLightProbe: i32;
    static mut CyberpunkVR_SightAxisProbe: i32;
    static mut CyberpunkVR_SightPsDump: i32;
    static mut CyberpunkVR_VisionDump: i32;
    static mut CyberpunkVR_VisionMap: i32;
    static mut CyberpunkVR_ViewDataDiff: i32;
    static mut CyberpunkVR_DebugRtvPickLog: i32;
    static mut CyberpunkVR_LodThreshOverrideEnable: i32;
}

/// ApplyLauncherDebugGate: called once from InitStereoOnce, before any hook is installed, and again when the launcher's
/// DEBUG box is ticked (the startup gate ran on the previous session's value).
#[no_mangle]
pub extern "C" fn ApplyLauncherDebugGate() {
    let flags = [
        Flag {
            name: c"RuntimeDiagnostics".as_ptr(),
            addr: &raw mut CyberpunkVR_RuntimeDiagnostics,
            on_value: 1,
            wide: false,
        },
        Flag {
            name: c"XrRateLog".as_ptr(),
            addr: &raw mut CyberpunkVR_XrRateLog,
            on_value: 1,
            wide: false,
        },
        Flag {
            name: c"XrDeepDiag".as_ptr(),
            addr: &raw mut CyberpunkVR_XrDeepDiag,
            on_value: 1,
            wide: false,
        },
        Flag {
            name: c"VrikRateLog".as_ptr(),
            addr: &raw mut CyberpunkVR_VrikRateLog,
            on_value: 1,
            wide: false,
        },
        Flag {
            name: c"TemporalScan".as_ptr(),
            addr: &raw mut CyberpunkVR_TemporalScan,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"WideCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_WideCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CascFitProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CascFitProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"BlockDiffSize".as_ptr(),
            addr: &raw mut CyberpunkVR_BlockDiffSize,
            on_value: 384,
            wide: true,
        },
        Flag {
            name: c"StageProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_StageProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"LumaProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_LumaProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CapCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_CapCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"NodeCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_NodeCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"DrawCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_DrawCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"DispatchCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_DispatchCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"LightCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_LightCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CullCountProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CullCountProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"TileProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_TileProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"PsoProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_PsoProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"RtMapProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_RtMapProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CbvProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CbvProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CamCbProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CamCbProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CascSampleProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CascSampleProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CaptureMarkers".as_ptr(),
            addr: &raw mut CyberpunkVR_CaptureMarkers,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CmdListHunt".as_ptr(),
            addr: &raw mut CyberpunkVR_CmdListHunt,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CascSideCensus".as_ptr(),
            addr: &raw mut CyberpunkVR_CascSideCensus,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"ViewRectProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_ViewRectProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"EnvPtrProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_EnvPtrProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"HudNodeProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_HudNodeProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"VolumeNodeProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_VolumeNodeProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"GradeCbProbe".as_ptr(),
            addr: crate::grading_lut::CyberpunkVR_GradeCbProbe.as_ptr(),
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"GradeUpProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_GradeUpProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"GradingProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_GradingProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"SkyProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_SkyProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"CloudLightProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_CloudLightProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"SightAxisProbe".as_ptr(),
            addr: &raw mut CyberpunkVR_SightAxisProbe,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"SightPsDump".as_ptr(),
            addr: &raw mut CyberpunkVR_SightPsDump,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"VisionDump".as_ptr(),
            addr: &raw mut CyberpunkVR_VisionDump,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"VisionMap".as_ptr(),
            addr: &raw mut CyberpunkVR_VisionMap,
            on_value: 1,
            wide: true,
        },
        Flag {
            name: c"ViewDataDiff".as_ptr(),
            addr: &raw mut CyberpunkVR_ViewDataDiff,
            on_value: 2,
            wide: true,
        },
        Flag {
            name: c"DebugRtvPickLog".as_ptr(),
            addr: &raw mut CyberpunkVR_DebugRtvPickLog,
            on_value: 48,
            wide: true,
        },
        Flag {
            name: c"LodThreshOverride".as_ptr(),
            addr: &raw mut CyberpunkVR_LodThreshOverrideEnable,
            on_value: 1,
            wide: true,
        },
    ];
    let on = crate::vr_core_settings::GetLauncherDebug() != 0;
    // SAFETY: every address is a live 32-bit export of this DLL; the verbose channel is the C++ half's.
    unsafe { cvr_host::debug_gate::apply(&flags, on, crate::vr_core_settings::verbose_log_ptr()) };
}
