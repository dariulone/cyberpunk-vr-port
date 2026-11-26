//! The port's natives in the game's RTTI (formerly src/Natives/Registration.cpp): the registration table -- which
//! names the script VM can call, their signatures, and what they point at -- and the late completion of the
//! signatures whose types RTTI cannot make yet. A native is defined in Rust (rust/crates/cvr-rig, cvr-xr: #[no_mangle])
//! or, until it moves, in C++ (src/Natives, declared with C linkage by include/Natives/NativeFunctions.hpp); either way
//! it is named here by its symbol.
//!
//! TWO KINDS OF ENTRY. `now` is registered with whatever of its signature RTTI can make at PostRegisterTypes, the
//! type found by its name's plain hash as the SDK's AddParam/SetReturnType find it: a type that is not there yet is
//! left out, and the scripts' own declaration fills it in (the script loader gives a native a script declares the
//! declaration's signature). `later` is completed as soon as its types exist: a composite type ("handle:entEntity",
//! "array:handle:inkLayerWrapper") is made on first request from its inner type once its name is in the name pool,
//! and a class the scripts declare is only there once they are loaded. CET converts its Lua arguments by the
//! signature, so the parameters go in only all together, never with a gap, and what the script loader filled in is
//! left as it is.
use core::ffi::{c_void, CStr};
use cvr_host::log;
use red4ext_abi::api::GameApplication;
use red4ext_abi::cname::CName;
use red4ext_abi::function::{function_flags, GlobalFunction, NativeFn};
use red4ext_abi::rtti::{IType, RttiSystem};
use red4ext_abi::stack::StackFrame;
use std::sync::Mutex;

struct Param {
    ty: &'static CStr,
    name: &'static CStr,
    optional: bool,
}
struct Native {
    name: &'static CStr,
    native: NativeFn,
    returns: Option<&'static CStr>,
    params: &'static [Param],
    later: bool,
}
const fn p(ty: &'static CStr, name: &'static CStr) -> Param {
    Param { ty, name, optional: false }
}
const fn opt(ty: &'static CStr, name: &'static CStr) -> Param {
    Param { ty, name, optional: true }
}
const fn now(name: &'static CStr, native: NativeFn, returns: Option<&'static CStr>, params: &'static [Param]) -> Native {
    Native {
        name,
        native,
        returns,
        params,
        later: false,
    }
}
const fn later(name: &'static CStr, native: NativeFn, returns: Option<&'static CStr>, params: &'static [Param]) -> Native {
    Native {
        name,
        native,
        returns,
        params,
        later: true,
    }
}

extern "C" {
    fn VRWorldMarkerAnchor(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWorldMarkerScreenPoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWorldMarkerProjection(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandEquipment(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLauncherControllerAim(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareChordTake(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREntityComponents(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREntitySkinnedMeshes(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREntityComponentsOf(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREntitySetWorldTransform(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentChangeResource(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentChangeAppearance(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentRefreshAppearance(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentDissolve(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentDissolved(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentDissolveReady(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRComponentKey(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInkLayers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInkLayer(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInkWorldControllers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeatherSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeatherState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyExposureCollect(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyExposureSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyExposureGet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyExposureCount(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnTemplate(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnRecord(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnDelete(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnDeleteTagged(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnManaged(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnedEntity(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnTagged(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSpawnTaggedIDs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMeleeInputFrame(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMeleeThrowFrame(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMeleeThrowEnabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuGesture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuPointer(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWorldTouchPointer(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGripFromFingers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuVendorPoll(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuVendorResolve(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuCapture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInterfaceEnabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorAppearanceFinish(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorAppearanceNative(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponWheel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuBlink(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuSurfaceReady(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuOpenSettings(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSettingsPage(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSettingsEdit(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMenuPlayWav(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSoundChannel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponTriggerEnabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCurrentFingerAction(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRAutopilotChordTake(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponInLeftHand(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLeftHanded(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSceneOwnsBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysicalMelee(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFirstSetupNeeded(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSetLeftHanded(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSetBodyHandsOnly(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyHandsOnly(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSetLeftHandedBindings(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLeftHandedBindings(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCalibrationStart(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCalibrationState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCalibrationProgress(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSuggestedHeight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSetPlayerHeight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPlayerHeight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorDouble(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorDoubleClear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorDoubleStats(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInteractionAction(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRNeckCapSync(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLeftHandedAnimations(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMeleeRealTime(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMeleeTwoHandedActive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareFrame(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwarePoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareWirePlanes(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareMusclePulse(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareSelect(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCyberwareCollisionInfo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReflexTiming(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRMouseYDisabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRShouldScanBodyCapsules(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRVrcamEnableRequest(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRWheelControlState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWheelGamepadProfile(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWheelCabin(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VROverlayCommand(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VROverlayMenu(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VROverlayToggle(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREngineStereoMenuShown(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VREngineStereoMenuClosed(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyActive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbySuspend(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyCurtain(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyAmbient(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSetLobbyAmbient(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyIntro(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRIntroTakeBootNotice(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRIntroHum(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReachAim(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReachUse(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandHolds(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbySaveLooks(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCreationTarget(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRMirrorAppearanceOptions(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCustomizationReset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLobbyResolveAs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudPanelUpdate(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudPanelRootsNative(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudPanelRootsBegin(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudPanelRoot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudPanelSubtitle(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetLeftVRHandValid(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRightVRHandValid(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetLeftVRHandPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRightVRHandPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetLeftVRHandRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRightVRHandRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn IsVRHandLinked(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn ForceHideVRFppArms(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn RestoreVRFppArms(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRRightHandEntity(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpVRFppComponents(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRFppChunkDebugEnabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRFppChunkDebugComponentIndex(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRFppChunkDebugHand(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRFppChunkDebugBits(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRBoneDebugIndex(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn InstallVRAnimPoseHook(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn ArmVRAnimPosePlayer(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRAnimPoseStats(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPlayerCamera(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBdSceneFov(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBdFovWrite(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHudSecondEye(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBdHeadWrite(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSceneCamera(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBraindance(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRInBraindance(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRemoteCamera(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDevCamOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRTakeoverEntity(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRUiPopup(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPopupVisualState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWristGuard(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpPlayerBoneNames(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeFingers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeCaptureFingers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeDumpFingers(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeCig(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeMouthDist(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeMouthDistL(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeAnchorBone(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeMouthAnchor(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeCigScaleY(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRViewWorldPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRViewWorldRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPalmWorldPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPalmCamLocal(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPalmModelPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyBonePos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBoneSnapCount(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBoneModelPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBoneModelRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCamModelPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPlayerEnginePos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPlayerEngineRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDumpBodyFuncs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRProbeBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyProbe(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetVel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetAngVel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetVel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetAngVel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetDims(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetCOM(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandStop(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWorldToModel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWorldDirToModel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPairs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPairUse(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigStatus(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWrite(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteScale(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRTwoHandCapture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRTwoHandStatus(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWeaponClass(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHapticEvent(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRCarryNear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRCarryReach(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerCaptureRight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerStatusRight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerApplyRight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerCapture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerStatus(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRestFingerApply(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReloadFingerClear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReloadFingerSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReloadFingerApply(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRReloadFingerBlend(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigReset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteAbs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigBone(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigSignature(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigTrack(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigTrackWrite(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRecordFK(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteOff(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteClear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRRigWriteDiag(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmallRig(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmallRigUse(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPoseCensus(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponRigUse(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponPartSetIdx(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponPartOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponRigArm(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponPartLocal(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponRigNames(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWeaponRigStatus(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandRawModel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandRawRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandMotion(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandStopModel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandStopRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandRawWorld(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandStopDeadband(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetKinematic(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyIsKinematic(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetSimMasks(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRAttachToSlot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDetachFromSlot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspSeat(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspPin(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspBlend(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspClear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldPicture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldPictureView(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldPictureLetGo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFrameOrderProbe(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldBodyGrasp(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldBodyFilter(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHeldBodyLetGo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGraspShape(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRThingHighlight(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandWeapon(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandWeaponPart(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsTestStack(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsPerson(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsPeople(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsSelfShape(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsGrab(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandFrame(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHeldBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsSelfPad(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysHandsGrabStyle(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladePoseOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandCollision(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugOn(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugViewSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugLine(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyFilterInfo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugProbeBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyPhysXPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunBoxClear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunBoxAdd(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunAnchor(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunPush(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunTouches(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunTouchOf(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGunTouchWay(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRThingOutline(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRProxyBodies(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugRay(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugCapsule(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugPoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugWorld(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDebugFlush(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeQueueSurfaceMark(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladePaintPoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeMeshPoll(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeMeshInstanceProbe(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRVehiclePartNear(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeSlotEdgeSample(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeAnimatedAttachment(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeInsertSlide(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeEdgeStrokeLive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeRequestEdgeSlide(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeDriveEdge(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeEdgePath(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeEdgeSample(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeJointPoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeRenewJoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeApplySlideLoad(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeHoldDepth(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeTrackingBasis(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeReadCapsule(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeReadSlotCapsule(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeEdgeShapes(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeCollectOccupied(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeSameGeometry(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeSameHitData(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeMatchesOccupied(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeSweptContact(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeEntrySweep(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeCapsuleMetrics(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeCapsuleInterval(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeSubmitJoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeClearJoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeTraceKind(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBladeFindPuppetJoint(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRVehicleHandWorld(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRVehicleHandClaim(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRVehicleHandBusy(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHairDangles(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHairPoints(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHairRaw(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHairStrands(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHairForce(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRDangleForce(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRNpcGrip(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRNpcGripState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRNpcGripBind(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRNearBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyOf(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPickCone(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyInfo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySolverMass(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCorpseHold(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCorpseSpeed(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCorpseFreeze(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspCapsule(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGraspSeatTurn(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandHoldBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySurface(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRTraceInfo(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRTraceBody(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodyGetRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRBodySetQueryable(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFingerContact(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFingerBend(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRCarryLead(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFingerClosed(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRFingerWanted(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRHandContactMargin(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGripForce(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPromptPart(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPalmModelRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeMouthWorldPos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeMouthWorldRot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeSmokeOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeCigChunks(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeCigVisualScale(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRSmokeCigVisualScaleY(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeMouthOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeCigOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeFingersL(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeLeftCig(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeRightLighter(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRSmokeCaptureFingersL(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeLighter(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeLighterOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeThumbFlickL(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmokeThumbPressL(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRBindMode(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRBindParams(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRElbowPole(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRElbowSwing(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRHandOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRBindBones(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRHeadBone(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRUseHeadRelative(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRDiagCapture(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn LogVRDiag(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPlayerYaw(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimVTable(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimMemory(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimControllerComponents(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpRuntimeClassFunctions(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRIKAnimInputTestMode(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn UpdateVRIKAnimInputs(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimControllerFunctionDetails(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimControllerListeners(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpInterestingAnimClassProperties(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimationSystemCandidates(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn RunIKTargetAddTest(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestAnimFloatInput(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestAnimFloatPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetPlayerAnimParameter(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetPlayerAnimParameterPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetPlayerAnimParameterPersistentPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetPlayerAnimParameterPersistentLastResult(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestWeaponUserFeatureRoute(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestIKFeatureRoute(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestMeleeIKDataFeatureRoute(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpRootGraphVariables(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootGraphBoolVariableByName(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootGraphFloatVariableByName(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootGraphVectorVariableByName(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestRootGraphFloatPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestRootGraphVectorPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootGraphFloatPresetPersistent(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootGraphVectorPresetPersistent(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRootGraphPersistentLastResult(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpPlayerAnimatedObjectRuntime(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpAnimationSystemLookup(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpRootMetaRigTracks(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestRootMetaRigTrackPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootMetaRigTrackPresetPersistent(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRootMetaRigTrackPersistentLastResult(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn DumpRootAnimatedObjectFloatArrayCandidates(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn TestRootLiveTrackPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetRootLiveTrackPresetPersistent(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetRootLiveTrackPersistentLastResult(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn ReadRootLiveTrackPreset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn InstallWeaponAimHook(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn InstallVRProvInstrument(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRProvOverrideSlot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRProvDump(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn ResetVRProvCounts(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRProvQuatMode(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRMuzzlePos(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRCarryLeft(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRTwoHandSuppress(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRCarryOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRInteractionDepth(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRHudClearance(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRAimHit(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRMuzzleQuat(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWeaponName(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWeaponKick(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSmartWeapon(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRAttachmentRecoil(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRLastShotPeak(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLeftWeaponKick(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRZoomLevel(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysicalOpticsEnabled(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysicalOpticsCameraReady(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysicalOpticsHinge(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRLuaHostGc(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPhysicalOpticsPath(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRMainEyeOffset(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSightZoom(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSightDot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSightApproach(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSightLine(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRPhysicalOpticsProbe(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRReloadOwnedHand(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRTwoHandSlide(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRTwoHandActive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRTwoHandLeftCtrl(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLeftTriggerBlock(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRGrenadeStick(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRWilsonRangeTarget(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRDualWield(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLeftMuzzle(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRDualWieldRecords(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRDualWieldWeapons(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSprintActive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRPhysicalCrouch(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPhysicalCrouchFeedback(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRPhysicalCrouchCameraAnchor(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLadderState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLadderTopRails(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLadderMeasuredTop(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRLadderDebug(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLadderTestGrip(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRSwimmingState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRSwimmingDebug(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLocomotionState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRLocomotionDetailed(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRScannerState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWorldTimeScale(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPairedAnimationState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRBodyCarryBothHands(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRReloadAnimation(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRFinisherActive(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRStoryAttentionUpdate(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRManualClueUpdate(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWeaponPoseState(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRWeaponRaiseTransition(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRMeleeFire(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRTriggerMode(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRCamBoneFreeze(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPairSlew(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRPairLead(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRCamAck(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRMeleeTrigger(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRSharedSlot(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn GetVRWeaponAim(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRKeypadUpdate(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRScannerSlotGet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRScannerSlotSet(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn VRScannerSlotSave(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRMenuOpen(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
    fn SetVRDeviceScreen(context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, a4: i64);
}

/// Every native, in the order the C++ table registered them.
#[rustfmt::skip]
static NATIVES: &[Native] = &[
    now(c"VRWorldMarkerAnchor", VRWorldMarkerAnchor, None, &[p(c"handle:inkWidget", c"widget"), p(c"Vector4", c"position")]),
    now(c"VRWorldMarkerScreenPoint", VRWorldMarkerScreenPoint, Some(c"Vector4"), &[p(c"Vector4", c"position")]),
    now(c"VRWorldMarkerProjection", VRWorldMarkerProjection, None, &[p(c"handle:inkWidget", c"widget"), p(c"handle:inkScreenProjection", c"projection")]),
    now(c"VRHandEquipment", VRHandEquipment, None, &[p(c"Int32", c"occupiedMask")]),
    now(c"VRLauncherControllerAim", VRLauncherControllerAim, Some(c"Bool"), &[]),
    now(c"VRCyberwareChordTake", VRCyberwareChordTake, Some(c"Bool"), &[]),
    later(c"VREntityComponents", VREntityComponents, Some(c"array:handle:entIComponent"), &[p(c"handle:entEntity", c"entity")]),
    later(c"VREntitySkinnedMeshes", VREntitySkinnedMeshes, Some(c"array:handle:entIComponent"), &[p(c"handle:entEntity", c"entity")]),
    later(c"VREntityComponentsOf", VREntityComponentsOf, Some(c"array:handle:entIComponent"), &[p(c"handle:entEntity", c"entity"), p(c"CName", c"className")]),
    later(c"VREntitySetWorldTransform", VREntitySetWorldTransform, Some(c"Bool"), &[p(c"handle:entEntity", c"entity"), p(c"WorldTransform", c"transform")]),
    later(c"VRComponentChangeResource", VRComponentChangeResource, Some(c"Bool"), &[p(c"handle:entIComponent", c"component"), p(c"String", c"path"), p(c"Bool", c"wait")]),
    later(c"VRComponentChangeAppearance", VRComponentChangeAppearance, Some(c"Bool"), &[p(c"handle:entIComponent", c"component"), p(c"CName", c"appearance"), p(c"Bool", c"wait")]),
    later(c"VRComponentRefreshAppearance", VRComponentRefreshAppearance, Some(c"Bool"), &[p(c"handle:entIComponent", c"component")]),
    later(c"VRComponentDissolve", VRComponentDissolve, Some(c"Bool"), &[p(c"handle:entIComponent", c"component"), p(c"Bool", c"hidden"), p(c"Int32", c"mode")]),
    later(c"VRComponentDissolved", VRComponentDissolved, Some(c"Bool"), &[p(c"handle:entIComponent", c"component")]),
    later(c"VRComponentDissolveReady", VRComponentDissolveReady, Some(c"Bool"), &[p(c"handle:entIComponent", c"component")]),
    later(c"VRComponentKey", VRComponentKey, Some(c"Uint64"), &[p(c"handle:entIComponent", c"component")]),
    later(c"VRInkLayers", VRInkLayers, Some(c"array:handle:inkLayerWrapper"), &[]),
    later(c"VRMenuTooltipPrepare", crate::mechanics::menu_tooltip::VRMenuTooltipPrepare, Some(c"Int32"), &[p(c"String", c"path")]),
    later(c"VRMenuTooltipSpawn", crate::mechanics::menu_tooltip::VRMenuTooltipSpawn, Some(c"handle:inkWidget"), &[p(c"handle:inkIWidgetController", c"controller"), p(c"handle:inkWidget", c"parent"), p(c"String", c"path"), p(c"CName", c"libraryItem")]),
    later(c"VRInkLayer", VRInkLayer, Some(c"handle:inkLayerWrapper"), &[p(c"CName", c"name")]),
    later(c"VRInkWorldControllers", VRInkWorldControllers, Some(c"array:whandle:inkIWidgetController"), &[]),
    later(c"VRWeatherSet", VRWeatherSet, Some(c"Bool"), &[p(c"CName", c"weather"), p(c"Float", c"blendTime"), p(c"Uint32", c"priority")]),
    later(c"VRWeatherState", VRWeatherState, Some(c"CName"), &[]),
    later(c"VRLobbyExposureCollect", VRLobbyExposureCollect, Some(c"Int32"), &[p(c"handle:CResource", c"resource")]),
    later(c"VRLobbyExposureSet", VRLobbyExposureSet, None, &[p(c"Float", c"stops")]),
    later(c"VRLobbyExposureGet", VRLobbyExposureGet, Some(c"Float"), &[]),
    later(c"VRLobbyExposureCount", VRLobbyExposureCount, Some(c"Int32"), &[]),
    later(c"VRSpawnTemplate", VRSpawnTemplate, Some(c"entEntityID"), &[p(c"String", c"templatePath"), p(c"CName", c"appearance"), p(c"Vector4", c"position"), p(c"Quaternion", c"orientation"), p(c"array:CName", c"tags")]),
    later(c"VRSpawnRecord", VRSpawnRecord, Some(c"entEntityID"), &[p(c"TweakDBID", c"record"), p(c"CName", c"appearance"), p(c"Vector4", c"position"), p(c"Quaternion", c"orientation"), p(c"array:CName", c"tags")]),
    later(c"VRSpawnDelete", VRSpawnDelete, Some(c"Bool"), &[p(c"entEntityID", c"id")]),
    later(c"VRSpawnDeleteTagged", VRSpawnDeleteTagged, Some(c"Int32"), &[p(c"CName", c"tag")]),
    later(c"VRSpawnManaged", VRSpawnManaged, Some(c"Bool"), &[p(c"entEntityID", c"id")]),
    later(c"VRSpawnedEntity", VRSpawnedEntity, Some(c"handle:entEntity"), &[p(c"entEntityID", c"id")]),
    later(c"VRSpawnTagged", VRSpawnTagged, Some(c"array:handle:entEntity"), &[p(c"CName", c"tag")]),
    later(c"VRSpawnTaggedIDs", VRSpawnTaggedIDs, Some(c"array:entEntityID"), &[p(c"CName", c"tag")]),
    now(c"VRMeleeInputFrame", VRMeleeInputFrame, Some(c"array:Vector4"), &[]),
    now(c"VRMeleeThrowFrame", VRMeleeThrowFrame, Some(c"array:Vector4"), &[]),
    now(c"VRMeleeThrowEnabled", VRMeleeThrowEnabled, Some(c"Bool"), &[]),
    now(c"VRMenuGesture", VRMenuGesture, Some(c"array:Vector4"), &[]),
    now(c"VRMenuPointer", VRMenuPointer, Some(c"array:Vector4"), &[p(c"Vector4", c"center"), p(c"Vector4", c"right"), p(c"Vector4", c"up"), p(c"Vector4", c"size")]),
    now(c"VRWorldTouchPointer", VRWorldTouchPointer, Some(c"array:Vector4"), &[p(c"handle:IScriptable", c"component"), p(c"Vector4", c"centre"), p(c"Vector4", c"normal")]),
    now(c"VRGripFromFingers", VRGripFromFingers, Some(c"Bool"), &[]),
    now(c"VRMenuVendorPoll", VRMenuVendorPoll, Some(c"handle:IScriptable"), &[p(c"Bool", c"enabled"), opt(c"handle:IScriptable", c"system")]),
    now(c"VRMenuVendorResolve", VRMenuVendorResolve, Some(c"Bool"), &[p(c"handle:IScriptable", c"panel"), p(c"Bool", c"accepted")]),
    now(c"VRMenuCapture", VRMenuCapture, Some(c"Bool"), &[p(c"Bool", c"open")]),
    now(c"VRInterfaceEnabled", VRInterfaceEnabled, Some(c"Bool"), &[]),
    now(c"VRWeaponWheel", VRWeaponWheel, Some(c"Int32"), &[p(c"Bool", c"armed")]),
    now(c"VRMenuBlink", VRMenuBlink, Some(c"Bool"), &[p(c"Float", c"closure")]),
    now(c"VRMenuSurfaceReady", VRMenuSurfaceReady, Some(c"Bool"), &[p(c"handle:entIComponent", c"component"), p(c"handle:inkWidget", c"root")]),
    now(c"VRMenuOpenSettings", VRMenuOpenSettings, Some(c"Bool"), &[]),
    now(c"VRSettingsPage", VRSettingsPage, Some(c"String"), &[p(c"Int32", c"page")]),
    now(c"VRSettingsEdit", VRSettingsEdit, Some(c"Bool"), &[p(c"Int32", c"page"), p(c"String", c"id"), p(c"Float", c"value")]),
    now(c"VRMenuPlayWav", VRMenuPlayWav, Some(c"Bool"), &[p(c"String", c"file"), p(c"Float", c"volume")]),
    now(c"VRSoundChannel", VRSoundChannel, Some(c"Bool"), &[p(c"String", c"channel"), p(c"String", c"file"), p(c"Float", c"volume"), p(c"Float", c"lowpass"), p(c"Float", c"loopFrom"), p(c"Float", c"loopTo"), p(c"Float", c"crossfade"), p(c"Float", c"fade")]),
    now(c"VRWeaponTriggerEnabled", VRWeaponTriggerEnabled, Some(c"Bool"), &[]),
    now(c"VRCurrentFingerAction", VRCurrentFingerAction, Some(c"Uint64"), &[]),
    now(c"VRWeaponInLeftHand", VRWeaponInLeftHand, Some(c"Bool"), &[]),
    now(c"VRLeftHanded", VRLeftHanded, Some(c"Bool"), &[]),
    now(c"VRSceneOwnsBody", VRSceneOwnsBody, Some(c"Bool"), &[]),
    now(c"VRPhysicalMelee", VRPhysicalMelee, Some(c"Bool"), &[]),
    now(c"VRFirstSetupNeeded", VRFirstSetupNeeded, Some(c"Bool"), &[]),
    now(c"VRSetLeftHanded", VRSetLeftHanded, Some(c"Bool"), &[p(c"Bool", c"left")]),
    now(c"VRSetBodyHandsOnly", VRSetBodyHandsOnly, Some(c"Bool"), &[p(c"Bool", c"on")]),
    now(c"VRBodyHandsOnly", VRBodyHandsOnly, Some(c"Bool"), &[]),
    now(c"VRSetLeftHandedBindings", VRSetLeftHandedBindings, Some(c"Bool"), &[p(c"Int32", c"mode")]),
    now(c"VRLeftHandedBindings", VRLeftHandedBindings, Some(c"Int32"), &[]),
    now(c"VRCalibrationStart", VRCalibrationStart, Some(c"Bool"), &[p(c"Float", c"seconds")]),
    now(c"VRCalibrationState", VRCalibrationState, Some(c"Int32"), &[]),
    now(c"VRCalibrationProgress", VRCalibrationProgress, Some(c"Float"), &[]),
    now(c"VRSuggestedHeight", VRSuggestedHeight, Some(c"Float"), &[]),
    now(c"VRSetPlayerHeight", VRSetPlayerHeight, Some(c"Bool"), &[p(c"Float", c"meters")]),
    now(c"VRPlayerHeight", VRPlayerHeight, Some(c"Float"), &[]),
    now(c"VRMirrorDouble", VRMirrorDouble, Some(c"Bool"), &[p(c"handle:IScriptable", c"entity"), p(c"Int32", c"flags")]),
    now(c"VRMirrorDoubleClear", VRMirrorDoubleClear, None, &[]),
    now(c"VRMirrorDoubleStats", VRMirrorDoubleStats, Some(c"String"), &[]),
    now(c"VRInteractionAction", VRInteractionAction, Some(c"Bool"), &[p(c"CName", c"action")]),
    now(c"VRNeckCapSync", VRNeckCapSync, Some(c"Int32"), &[]),
    now(c"VRLeftHandedAnimations", VRLeftHandedAnimations, None, &[p(c"handle:IScriptable", c"controller"), p(c"Bool", c"on"), p(c"CName", c"graph"), p(c"CName", c"slot")]),
    now(c"VRMeleeRealTime", VRMeleeRealTime, Some(c"Float"), &[]),
    now(c"VRMeleeTwoHandedActive", VRMeleeTwoHandedActive, Some(c"Bool"), &[]),
    now(c"VRCyberwareFrame", VRCyberwareFrame, Some(c"array:Vector4"), &[]),
    now(c"VRCyberwarePoint", VRCyberwarePoint, Some(c"Vector4"), &[p(c"Int32", c"side"), p(c"Int32", c"index")]),
    now(c"VRCyberwareWirePlanes", VRCyberwareWirePlanes, None, &[p(c"Int32", c"side"), p(c"Int32", c"epoch"), p(c"array:Vector4", c"planes")]),
    now(c"VRCyberwareMusclePulse", VRCyberwareMusclePulse, None, &[p(c"Int32", c"side"), p(c"Float", c"duration")]),
    now(c"VRCyberwareSelect", VRCyberwareSelect, None, &[p(c"Int32", c"kind")]),
    now(c"VRCyberwareCollisionInfo", VRCyberwareCollisionInfo, Some(c"Vector4"), &[]),
    now(c"VRReflexTiming", VRReflexTiming, Some(c"String"), &[p(c"Bool", c"request")]),
    now(c"GetVRMouseYDisabled", GetVRMouseYDisabled, Some(c"Bool"), &[]),
    later(c"VRShouldScanBodyCapsules", VRShouldScanBodyCapsules, Some(c"Bool"), &[p(c"handle:entEntity", c"entity")]),
    now(c"VRVrcamEnableRequest", VRVrcamEnableRequest, Some(c"Int32"), &[]),
    now(c"GetVRWheelControlState", GetVRWheelControlState, Some(c"Int32"), &[]),
    now(c"VRAutopilotChordTake", VRAutopilotChordTake, Some(c"Bool"), &[]),
    now(c"SetVRWheelGamepadProfile", SetVRWheelGamepadProfile, Some(c"Bool"), &[p(c"Float", c"inner"), p(c"Float", c"outer")]),
    now(c"SetVRWheelCabin", SetVRWheelCabin, Some(c"Bool"), &[p(c"Vector4", c"pos"), p(c"Quaternion", c"rot")]),
    now(c"VROverlayCommand", VROverlayCommand, Some(c"Int32"), &[]),
    now(c"VROverlayMenu", VROverlayMenu, Some(c"Bool"), &[p(c"Bool", c"ready"), p(c"Bool", c"inGame"), p(c"String", c"labels")]),
    now(c"VROverlayToggle", VROverlayToggle, None, &[]),
    now(c"VREngineStereoMenuShown", VREngineStereoMenuShown, None, &[]),
    now(c"VREngineStereoMenuClosed", VREngineStereoMenuClosed, None, &[]),
    now(c"VRLobbyActive", VRLobbyActive, Some(c"Bool"), &[]),
    now(c"VRLobbySuspend", VRLobbySuspend, None, &[p(c"Bool", c"on")]),
    now(c"VRLobbyCurtain", VRLobbyCurtain, None, &[p(c"Float", c"closure")]),
    now(c"VRLobbyAmbient", VRLobbyAmbient, Some(c"Bool"), &[]),
    now(c"VRSetLobbyAmbient", VRSetLobbyAmbient, None, &[p(c"Bool", c"on")]),
    now(c"VRLobbyIntro", VRLobbyIntro, Some(c"Bool"), &[p(c"handle:entIComponent", c"component"), p(c"Float", c"distance"), p(c"Float", c"height"), p(c"Float", c"opacity"), p(c"Float", c"blackPoint")]),
    now(c"VRIntroTakeBootNotice", VRIntroTakeBootNotice, Some(c"Bool"), &[p(c"Float", c"fade")]),
    now(c"VRIntroHum", VRIntroHum, None, &[p(c"Bool", c"on"), p(c"Float", c"volume")]),
    now(c"VRReachAim", VRReachAim, Some(c"Bool"), &[p(c"Vector4", c"point")]),
    now(c"VRReachUse", VRReachUse, Some(c"Bool"), &[p(c"Float", c"seconds")]),
    now(c"VRHandHolds", VRHandHolds, Some(c"Bool"), &[p(c"Int32", c"side")]),
    now(c"VRLobbySaveLooks", VRLobbySaveLooks, Some(c"String"), &[]),
    later(c"VRCreationTarget", VRCreationTarget, Some(c"Bool"), &[opt(c"handle:entEntity", c"entity")]),
    later(c"VRMirrorAppearanceOptions", VRMirrorAppearanceOptions, Some(c"Bool"), &[p(c"handle:entEntity", c"entity"), p(c"Bool", c"male")]),
    later(c"VRMirrorAppearanceFinish", VRMirrorAppearanceFinish, Some(c"Bool"), &[p(c"handle:IScriptable", c"panel")]),
    later(c"VRMirrorAppearanceNative", VRMirrorAppearanceNative, Some(c"Bool"), &[p(c"handle:IScriptable", c"panel")]),
    later(c"VRCustomizationReset", VRCustomizationReset, Some(c"Bool"), &[]),
    now(c"VRLobbyResolveAs", VRLobbyResolveAs, None, &[p(c"Bool", c"male")]),
    later(c"VRHudPanelUpdate", VRHudPanelUpdate, Some(c"Int32"), &[p(c"handle:inkWidget", c"root"), p(c"Bool", c"lootVisible")]),
    now(c"VRHudPanelRootsNative", VRHudPanelRootsNative, Some(c"Bool"), &[]),
    now(c"VRHudPanelRootsBegin", VRHudPanelRootsBegin, None, &[]),
    later(c"VRHudPanelRoot", VRHudPanelRoot, Some(c"Bool"), &[p(c"handle:inkWidget", c"root"), p(c"handle:inkWidget", c"window"), p(c"handle:inkUserData", c"info"), p(c"Int32", c"flags"), p(c"Float", c"padding")]),
    later(c"VRHudPanelSubtitle", VRHudPanelSubtitle, Some(c"Bool"), &[p(c"handle:inkWidget", c"root"), p(c"handle:inkWidget", c"window"), p(c"handle:inkUserData", c"info"), p(c"handle:inkWidget", c"panel")]),
    now(c"GetLeftVRHandValid", GetLeftVRHandValid, Some(c"Bool"), &[]),
    now(c"GetRightVRHandValid", GetRightVRHandValid, Some(c"Bool"), &[]),
    now(c"GetLeftVRHandPos", GetLeftVRHandPos, Some(c"Vector4"), &[]),
    now(c"GetRightVRHandPos", GetRightVRHandPos, Some(c"Vector4"), &[]),
    now(c"GetLeftVRHandRot", GetLeftVRHandRot, Some(c"Quaternion"), &[]),
    now(c"GetRightVRHandRot", GetRightVRHandRot, Some(c"Quaternion"), &[]),
    now(c"IsVRHandLinked", IsVRHandLinked, Some(c"Bool"), &[]),
    now(c"ForceHideVRFppArms", ForceHideVRFppArms, Some(c"Int32"), &[]),
    now(c"RestoreVRFppArms", RestoreVRFppArms, Some(c"Int32"), &[]),
    now(c"SetVRRightHandEntity", SetVRRightHandEntity, None, &[p(c"handle:IScriptable", c"entity")]),
    now(c"DumpVRFppComponents", DumpVRFppComponents, Some(c"Int32"), &[]),
    now(c"SetVRFppChunkDebugEnabled", SetVRFppChunkDebugEnabled, None, &[p(c"Int32", c"enabled")]),
    now(c"SetVRFppChunkDebugComponentIndex", SetVRFppChunkDebugComponentIndex, None, &[p(c"Int32", c"index")]),
    now(c"SetVRFppChunkDebugHand", SetVRFppChunkDebugHand, None, &[p(c"Int32", c"hand")]),
    now(c"SetVRFppChunkDebugBits", SetVRFppChunkDebugBits, None, &[p(c"Int32", c"bit0"), p(c"Int32", c"bit1"), p(c"Int32", c"bit2"), p(c"Int32", c"bit3")]),
    now(c"SetVRBoneDebugIndex", SetVRBoneDebugIndex, None, &[p(c"Int32", c"index")]),
    now(c"InstallVRAnimPoseHook", InstallVRAnimPoseHook, Some(c"Int32"), &[]),
    now(c"ArmVRAnimPosePlayer", ArmVRAnimPosePlayer, Some(c"Int32"), &[]),
    now(c"GetVRAnimPoseStats", GetVRAnimPoseStats, Some(c"Int32"), &[p(c"Int32", c"mode")]),
    now(c"VRPlayerCamera", VRPlayerCamera, Some(c"Int32"), &[p(c"Int32", c"active"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"VRBdSceneFov", VRBdSceneFov, Some(c"Int32"), &[p(c"Float", c"fov")]),
    now(c"VRBdFovWrite", VRBdFovWrite, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRHudSecondEye", VRHudSecondEye, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRBdHeadWrite", VRBdHeadWrite, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRSceneCamera", VRSceneCamera, Some(c"Int32"), &[p(c"Int32", c"active"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"qi"), p(c"Float", c"qj"), p(c"Float", c"qk"), p(c"Float", c"qr")]),
    now(c"VRBraindance", VRBraindance, Some(c"Int32"), &[p(c"Int32", c"active"), p(c"Float", c"fov")]),
    now(c"VRInBraindance", VRInBraindance, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"VRRemoteCamera", VRRemoteCamera, Some(c"Int32"), &[p(c"Int32", c"active"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"VRDevCamOffset", VRDevCamOffset, Some(c"Int32"), &[p(c"Float", c"right"), p(c"Float", c"forward"), p(c"Float", c"up")]),
    now(c"VRTakeoverEntity", VRTakeoverEntity, Some(c"Int32"), &[p(c"String", c"id")]),
    now(c"VRUiPopup", VRUiPopup, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"VRPopupVisualState", VRPopupVisualState, Some(c"Bool"), &[p(c"CName", c"state"), p(c"Bool", c"request")]),
    now(c"VRWristGuard", VRWristGuard, Some(c"Int32"), &[p(c"Int32", c"mode")]),
    now(c"DumpPlayerBoneNames", DumpPlayerBoneNames, Some(c"Int32"), &[]),
    now(c"SetVRSmokeFingers", SetVRSmokeFingers, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"VRSmokeCaptureFingers", VRSmokeCaptureFingers, Some(c"Int32"), &[]),
    now(c"VRSmokeDumpFingers", VRSmokeDumpFingers, Some(c"Int32"), &[]),
    now(c"SetVRSmokeCig", SetVRSmokeCig, Some(c"Int32"), &[p(c"Int32", c"enable")]),
    now(c"VRSmokeMouthDist", VRSmokeMouthDist, Some(c"Float"), &[]),
    now(c"VRSmokeMouthDistL", VRSmokeMouthDistL, Some(c"Float"), &[]),
    now(c"SetVRSmokeAnchorBone", SetVRSmokeAnchorBone, Some(c"Int32"), &[p(c"Int32", c"sel")]),
    now(c"SetVRSmokeMouthAnchor", SetVRSmokeMouthAnchor, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"SetVRSmokeCigScaleY", SetVRSmokeCigScaleY, Some(c"Int32"), &[p(c"Float", c"y")]),
    now(c"VRViewWorldPos", VRViewWorldPos, Some(c"Vector4"), &[]),
    now(c"VRViewWorldRot", VRViewWorldRot, Some(c"Quaternion"), &[]),
    now(c"VRPalmWorldPos", VRPalmWorldPos, Some(c"Vector4"), &[p(c"Int32", c"right")]),
    now(c"VRPalmCamLocal", VRPalmCamLocal, Some(c"Vector4"), &[p(c"Int32", c"right")]),
    now(c"VRPalmModelPos", VRPalmModelPos, Some(c"Vector4"), &[p(c"Int32", c"right")]),
    now(c"VRBodyBonePos", VRBodyBonePos, Some(c"Vector4"), &[p(c"Int32", c"slot")]),
    now(c"VRBoneSnapCount", VRBoneSnapCount, Some(c"Int32"), &[]),
    now(c"VRBoneModelPos", VRBoneModelPos, Some(c"Vector4"), &[p(c"Int32", c"idx")]),
    now(c"VRBoneModelRot", VRBoneModelRot, Some(c"Quaternion"), &[p(c"Int32", c"idx")]),
    now(c"VRCamModelPos", VRCamModelPos, Some(c"Vector4"), &[]),
    now(c"VRPlayerEnginePos", VRPlayerEnginePos, Some(c"Vector4"), &[]),
    now(c"VRPlayerEngineRot", VRPlayerEngineRot, Some(c"Quaternion"), &[]),
    now(c"VRDumpBodyFuncs", VRDumpBodyFuncs, Some(c"String"), &[]),
    now(c"VRProbeBody", VRProbeBody, Some(c"String"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodyProbe", VRBodyProbe, Some(c"String"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodyGetVel", VRBodyGetVel, Some(c"Vector4"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodyGetAngVel", VRBodyGetAngVel, Some(c"Vector4"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodySetVel", VRBodySetVel, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Vector4", c"v")]),
    now(c"VRBodySetAngVel", VRBodySetAngVel, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Vector4", c"v")]),
    now(c"VRBodyGetDims", VRBodyGetDims, Some(c"Vector4"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodyGetCOM", VRBodyGetCOM, Some(c"Vector4"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRHandStop", VRHandStop, Some(c"Bool"), &[p(c"Int32", c"right"), p(c"Bool", c"active"), p(c"Vector4", c"worldPos")]),
    now(c"VRWorldToModel", VRWorldToModel, Some(c"Vector4"), &[p(c"Vector4", c"worldPos")]),
    now(c"VRWorldDirToModel", VRWorldDirToModel, Some(c"Vector4"), &[p(c"Vector4", c"worldDir")]),
    now(c"VRPairs", VRPairs, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Int32", c"field")]),
    now(c"VRPairUse", VRPairUse, Some(c"Int32"), &[p(c"Int32", c"index"), p(c"Int32", c"which")]),
    now(c"VRRigStatus", VRRigStatus, Some(c"Int32"), &[p(c"Int32", c"field")]),
    now(c"VRRigWrite", VRRigWrite, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bone"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"VRRigWriteRot", VRRigWriteRot, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bone"), p(c"Float", c"angle"), p(c"Float", c"ax"), p(c"Float", c"ay"), p(c"Float", c"az")]),
    now(c"VRRigWriteScale", VRRigWriteScale, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bone"), p(c"Float", c"scale")]),
    now(c"VRTwoHandCapture", VRTwoHandCapture, Some(c"Int32"), &[]),
    now(c"VRTwoHandStatus", VRTwoHandStatus, Some(c"Int32"), &[]),
    now(c"SetVRWeaponClass", SetVRWeaponClass, None, &[p(c"Int32", c"cls")]),
    now(c"VRHapticEvent", VRHapticEvent, None, &[p(c"Int32", c"kind"), p(c"Int32", c"hands")]),
    now(c"GetVRCarryNear", GetVRCarryNear, Some(c"Int32"), &[]),
    now(c"GetVRCarryReach", GetVRCarryReach, Some(c"Float"), &[]),
    now(c"VRRestFingerCaptureRight", VRRestFingerCaptureRight, Some(c"Int32"), &[]),
    now(c"VRRestFingerStatusRight", VRRestFingerStatusRight, Some(c"Int32"), &[]),
    now(c"VRRestFingerApplyRight", VRRestFingerApplyRight, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRRestFingerCapture", VRRestFingerCapture, Some(c"Int32"), &[]),
    now(c"VRRestFingerStatus", VRRestFingerStatus, Some(c"Int32"), &[]),
    now(c"VRRestFingerApply", VRRestFingerApply, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRReloadFingerClear", VRReloadFingerClear, Some(c"Int32"), &[p(c"Int32", c"hand")]),
    now(c"VRReloadFingerSet", VRReloadFingerSet, Some(c"Int32"), &[p(c"Int32", c"hand"), p(c"String", c"bone"), p(c"Float", c"qx"), p(c"Float", c"qy"), p(c"Float", c"qz"), p(c"Float", c"qw")]),
    now(c"VRReloadFingerApply", VRReloadFingerApply, Some(c"Int32"), &[p(c"Int32", c"hand"), p(c"Int32", c"on")]),
    now(c"VRReloadFingerBlend", VRReloadFingerBlend, Some(c"Int32"), &[p(c"Int32", c"hand"), p(c"Float", c"alpha")]),
    now(c"VRRigReset", VRRigReset, Some(c"Bool"), &[]),
    now(c"VRRigWriteAbs", VRRigWriteAbs, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bone"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"qi"), p(c"Float", c"qj"), p(c"Float", c"qk"), p(c"Float", c"qr"), p(c"Int32", c"abs")]),
    now(c"VRRigBone", VRRigBone, Some(c"Float"), &[p(c"Int32", c"which"), p(c"Int32", c"bone"), p(c"Int32", c"field")]),
    now(c"VRRigSignature", VRRigSignature, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bones"), p(c"Int32", c"i0"), p(c"String", c"n0"), p(c"Int32", c"i1"), p(c"String", c"n1"), p(c"Int32", c"i2"), p(c"String", c"n2"), p(c"Int32", c"i3"), p(c"String", c"n3")]),
    now(c"VRRigTrack", VRRigTrack, Some(c"Float"), &[p(c"Int32", c"which"), p(c"Int32", c"idx")]),
    now(c"VRRigTrackWrite", VRRigTrackWrite, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"idx"), p(c"Float", c"value"), p(c"Int32", c"on")]),
    now(c"VRRecordFK", VRRecordFK, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRRigWriteOff", VRRigWriteOff, Some(c"Int32"), &[p(c"Int32", c"which"), p(c"Int32", c"bone")]),
    now(c"VRRigWriteClear", VRRigWriteClear, Some(c"Bool"), &[]),
    now(c"VRRigWriteDiag", VRRigWriteDiag, Some(c"Int32"), &[p(c"Int32", c"slot"), p(c"Int32", c"field")]),
    now(c"VRSmallRig", VRSmallRig, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Int32", c"field")]),
    now(c"VRSmallRigUse", VRSmallRigUse, Some(c"Int32"), &[p(c"Int32", c"index")]),
    now(c"VRPoseCensus", VRPoseCensus, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Int32", c"field")]),
    now(c"VRWeaponRigUse", VRWeaponRigUse, Some(c"Int32"), &[p(c"Int32", c"index")]),
    now(c"VRWeaponPartSetIdx", VRWeaponPartSetIdx, Some(c"Bool"), &[p(c"Int32", c"slot"), p(c"Int32", c"bone")]),
    now(c"VRWeaponPartOffset", VRWeaponPartOffset, Some(c"Bool"), &[p(c"Int32", c"slot"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"VRWeaponRigArm", VRWeaponRigArm, Some(c"Int32"), &[p(c"handle:IScriptable", c"weapon")]),
    now(c"VRWeaponPartLocal", VRWeaponPartLocal, Some(c"Vector4"), &[p(c"Int32", c"slot")]),
    now(c"VRWeaponRigNames", VRWeaponRigNames, Some(c"String"), &[]),
    now(c"VRWeaponRigStatus", VRWeaponRigStatus, Some(c"Int32"), &[p(c"Int32", c"slot")]),
    now(c"VRHandRawModel", VRHandRawModel, Some(c"Vector4"), &[p(c"Int32", c"right")]),
    now(c"VRHandRawRot", VRHandRawRot, Some(c"Quaternion"), &[p(c"Int32", c"right")]),
    now(c"VRHandMotion", VRHandMotion, Some(c"Vector4"), &[p(c"Int32", c"right"), p(c"Bool", c"spin")]),
    now(c"VRHandStopModel", VRHandStopModel, Some(c"Bool"), &[p(c"Int32", c"right"), p(c"Bool", c"active"), p(c"Vector4", c"modelPos")]),
    now(c"VRHandStopRot", VRHandStopRot, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"active"), p(c"Float", c"qi"), p(c"Float", c"qj"), p(c"Float", c"qk"), p(c"Float", c"qr")]),
    now(c"VRHandRawWorld", VRHandRawWorld, Some(c"Vector4"), &[p(c"Int32", c"right")]),
    now(c"VRHandStopDeadband", VRHandStopDeadband, Some(c"Bool"), &[p(c"Float", c"metres")]),
    now(c"VRBodySetKinematic", VRBodySetKinematic, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Bool", c"kinematic")]),
    now(c"VRBodyIsKinematic", VRBodyIsKinematic, Some(c"Int32"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodyGetPos", VRBodyGetPos, Some(c"Vector4"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodySetPos", VRBodySetPos, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Vector4", c"position"), p(c"Quaternion", c"orientation")]),
    now(c"VRBodySetSimMasks", VRBodySetSimMasks, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Uint64", c"mask1"), p(c"Uint64", c"mask2")]),
    now(c"VRAttachToSlot", VRAttachToSlot, Some(c"Bool"), &[p(c"handle:IScriptable", c"entity"), p(c"handle:IScriptable", c"parent"), p(c"CName", c"slot")]),
    now(c"VRDetachFromSlot", VRDetachFromSlot, Some(c"Bool"), &[p(c"handle:IScriptable", c"entity")]),
    now(c"VRGraspSeat", VRGraspSeat, Some(c"Transform"), &[p(c"Int32", c"side"), p(c"Int32", c"shape"), p(c"Float", c"hx"), p(c"Float", c"hy"), p(c"Float", c"hz"), p(c"Vector4", c"position"), p(c"Quaternion", c"orientation")]),
    now(c"VRGraspSet", VRGraspSet, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"shape"), p(c"Float", c"hx"), p(c"Float", c"hy"), p(c"Float", c"hz"), p(c"Vector4", c"position"), p(c"Quaternion", c"orientation")]),
    now(c"VRGraspPin", VRGraspPin, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Bool", c"pinned")]),
    now(c"VRGraspBlend", VRGraspBlend, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Float", c"blend")]),
    now(c"VRGraspClear", VRGraspClear, Some(c"Bool"), &[p(c"Int32", c"side")]),
    // main's physics hands, carried things and the blade's pull (Registration.cpp, BladeContactPose.cpp)
    now(c"VRHeldPicture", VRHeldPicture, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Int32", c"bone"), p(c"Vector4", c"offset"), p(c"Vector4", c"turn"), p(c"Bool", c"blocked")]),
    now(c"VRHeldPictureView", VRHeldPictureView, Some(c"Vector4"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Int32", c"part")]),
    now(c"VRFrameOrderProbe", VRFrameOrderProbe, Some(c"Bool"), &[p(c"Int32", c"frames")]),
    now(c"VRHeldBody", VRHeldBody, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Int32", c"bone"), p(c"Vector4", c"offset"), p(c"Vector4", c"turn")]),
    now(c"VRHeldBodyGrasp", VRHeldBodyGrasp, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Bool", c"on")]),
    now(c"VRPhysHandWeapon", VRPhysHandWeapon, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"count"), p(c"Vector4", c"at"), p(c"Vector4", c"turn")]),
    now(c"VRPhysHandWeaponPart", VRPhysHandWeaponPart, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"index"), p(c"Vector4", c"lo"), p(c"Vector4", c"hi")]),
    now(c"VRPhysHandsTestStack", VRPhysHandsTestStack, Some(c"Bool"), &[p(c"Vector4", c"at")]),
    now(c"VRPhysHandsPerson", VRPhysHandsPerson, Some(c"Bool"), &[p(c"Int32", c"index"), p(c"Vector4", c"a"), p(c"Vector4", c"b")]),
    now(c"VRPhysHandsPeople", VRPhysHandsPeople, Some(c"Bool"), &[p(c"Int32", c"count")]),
    now(c"VRPhysHandsSelfShape", VRPhysHandsSelfShape, Some(c"Vector4"), &[p(c"Int32", c"index"), p(c"Int32", c"end")]),
    now(c"VRPhysHandsGrab", VRPhysHandsGrab, Some(c"Vector4"), &[p(c"Int32", c"side")]),
    now(c"VRPhysHandFrame", VRPhysHandFrame, Some(c"array:Vector4"), &[p(c"Int32", c"side")]),
    now(c"VRPhysHeldBody", VRPhysHeldBody, Some(c"array:Uint64"), &[p(c"Int32", c"side")]),
    now(c"VRPhysHandsSelfPad", VRPhysHandsSelfPad, Some(c"Bool"), &[p(c"Float", c"metres")]),
    now(c"VRPhysHandsGrabStyle", VRPhysHandsGrabStyle, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"owner"), p(c"Int32", c"style")]),
    now(c"VRHeldBodyFilter", VRHeldBodyFilter, Some(c"Bool"), &[p(c"Int32", c"word0"), p(c"Int32", c"word2")]),
    now(c"VRHeldBodyLetGo", VRHeldBodyLetGo, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Vector4", c"velocity"), p(c"Vector4", c"spin")]),
    now(c"VRThingHighlight", VRThingHighlight, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"fill"), p(c"Int32", c"outline"), p(c"Bool", c"through")]),
    now(c"VRHeldPictureLetGo", VRHeldPictureLetGo, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRBodyGraspShape", VRBodyGraspShape, Some(c"String"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRBladePoseOffset", VRBladePoseOffset, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Vector4", c"modelOffset")]),
    now(c"VRHandCollision", VRHandCollision, Some(c"Int32"), &[]),
    // main's collision view and the gun's boxes (DebugView.cpp)
    now(c"VRDebugOn", VRDebugOn, Some(c"Bool"), &[]),
    now(c"VRDebugViewSet", VRDebugViewSet, Some(c"Bool"), &[p(c"Bool", c"on")]),
    now(c"VRDebugLine", VRDebugLine, Some(c"Bool"), &[p(c"Vector4", c"a"), p(c"Vector4", c"b"), p(c"Int32", c"color")]),
    now(c"VRBodyFilterInfo", VRBodyFilterInfo, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRDebugProbeBody", VRDebugProbeBody, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Vector4", c"world")]),
    now(c"VRBodyPhysXPos", VRBodyPhysXPos, Some(c"Vector4"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRGunBoxClear", VRGunBoxClear, Some(c"Bool"), &[]),
    now(c"VRGunBoxAdd", VRGunBoxAdd, Some(c"Bool"), &[p(c"Vector4", c"lo"), p(c"Vector4", c"hi")]),
    now(c"VRGunAnchor", VRGunAnchor, Some(c"Bool"), &[p(c"Vector4", c"at")]),
    now(c"VRGunPush", VRGunPush, Some(c"Vector4"), &[p(c"Vector4", c"at"), p(c"Vector4", c"turn")]),
    now(c"VRGunTouches", VRGunTouches, Some(c"Int32"), &[p(c"Vector4", c"at"), p(c"Vector4", c"turn")]),
    now(c"VRGunTouchOf", VRGunTouchOf, Some(c"Vector4"), &[p(c"Int32", c"i")]),
    now(c"VRGunTouchWay", VRGunTouchWay, Some(c"Vector4"), &[p(c"Int32", c"i")]),
    now(c"VRThingOutline", VRThingOutline, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Int32", c"color")]),
    now(c"VRProxyBodies", VRProxyBodies, Some(c"Int32"), &[p(c"Int32", c"proxy")]),
    now(c"VRDebugRay", VRDebugRay, Some(c"Bool"), &[p(c"Vector4", c"a"), p(c"Vector4", c"b"), p(c"Int32", c"color")]),
    now(c"VRDebugCapsule", VRDebugCapsule, Some(c"Bool"), &[p(c"Vector4", c"a"), p(c"Vector4", c"b"), p(c"Float", c"radius"), p(c"Int32", c"color")]),
    now(c"VRDebugPoint", VRDebugPoint, Some(c"Bool"), &[p(c"Vector4", c"p"), p(c"Float", c"size"), p(c"Int32", c"color")]),
    now(c"VRDebugWorld", VRDebugWorld, Some(c"Int32"), &[p(c"Vector4", c"center"), p(c"Float", c"radius")]),
    now(c"VRDebugFlush", VRDebugFlush, Some(c"Bool"), &[]),
    // main's blade mesh marks (BladeMeshInstance.cpp)
    later(c"VRBladeQueueSurfaceMark", VRBladeQueueSurfaceMark, Some(c"Uint32"), &[p(c"handle:entEntity", c"target"), p(c"CName", c"bone"), p(c"Vector4", c"from"), p(c"Vector4", c"to"), p(c"Vector4", c"normal"), p(c"Float", c"depth"), p(c"Uint32", c"serial"), p(c"Vector4", c"oldStart"), p(c"Vector4", c"oldEnd"), p(c"Vector4", c"start"), p(c"Vector4", c"end"), p(c"Int32", c"kind")]),
    later(c"VRBladePaintPoint", VRBladePaintPoint, Some(c"array:Vector4"), &[p(c"handle:entEntity", c"target"), p(c"CName", c"bone"), p(c"Uint32", c"serial")]),
    now(c"VRBladeMeshPoll", VRBladeMeshPoll, None, &[]),
    later(c"VRBladeMeshInstanceProbe", VRBladeMeshInstanceProbe, Some(c"String"), &[p(c"handle:entEntity", c"target"), p(c"handle:entEntity", c"control")]),
    later(c"VRVehiclePartNear", VRVehiclePartNear, Some(c"Vector4"), &[p(c"handle:entEntity", c"entity"), p(c"Vector4", c"point"), p(c"Float", c"radius")]),
    // main's blade contact (BladeContactNative.cpp)
    now(c"VRBladeSlotEdgeSample", VRBladeSlotEdgeSample, Some(c"array:Vector4"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial")]),
    now(c"VRBladeAnimatedAttachment", VRBladeAnimatedAttachment, Some(c"Vector4"), &[p(c"Int32", c"side"), p(c"Uint32", c"key"), p(c"Vector4", c"animated"), p(c"Quaternion", c"rotation"), p(c"Vector4", c"station"), p(c"Bool", c"bind")]),
    now(c"VRBladeInsertSlide", VRBladeInsertSlide, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Float", c"resistance")]),
    now(c"VRBladeEdgeStrokeLive", VRBladeEdgeStrokeLive, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Uint32", c"stroke")]),
    now(c"VRBladeRequestEdgeSlide", VRBladeRequestEdgeSlide, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Vector4", c"edge"), p(c"Float", c"load")]),
    now(c"VRBladeDriveEdge", VRBladeDriveEdge, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Vector4", c"localShaft"), p(c"Float", c"advance"), p(c"Float", c"load")]),
    now(c"VRBladeEdgePath", VRBladeEdgePath, Some(c"array:Vector4"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial")]),
    now(c"VRBladeEdgeSample", VRBladeEdgeSample, Some(c"array:Vector4"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial")]),
    now(c"VRBladeJointPoint", VRBladeJointPoint, Some(c"Vector4"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial")]),
    now(c"VRBladeRenewJoint", VRBladeRenewJoint, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial")]),
    now(c"VRBladeApplySlideLoad", VRBladeApplySlideLoad, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Float", c"resistance")]),
    now(c"VRBladeHoldDepth", VRBladeHoldDepth, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Float", c"resistance")]),
    now(c"VRBladeTrackingBasis", VRBladeTrackingBasis, Some(c"Vector4"), &[]),
    later(c"VRBladeReadCapsule", VRBladeReadCapsule, Some(c"array:Vector4"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"CName", c"name"), p(c"handle:gameHitShapeUserData", c"data")]),
    later(c"VRBladeReadSlotCapsule", VRBladeReadSlotCapsule, Some(c"array:Vector4"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"CName", c"name"), p(c"handle:gameHitShapeUserData", c"data"), p(c"CName", c"binding")]),
    later(c"VRBladeEdgeShapes", VRBladeEdgeShapes, Some(c"array:CName"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"Vector4", c"from"), p(c"Vector4", c"to")]),
    later(c"VRBladeCollectOccupied", VRBladeCollectOccupied, Some(c"Vector4"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"Vector4", c"from"), p(c"Vector4", c"to"), p(c"Vector4", c"edge"), p(c"CName", c"pinSlot"), p(c"WorldTransform", c"pinFrame"), p(c"handle:IScriptable", c"sink")]),
    now(c"VRBladeSameGeometry", VRBladeSameGeometry, Some(c"Bool"), &[p(c"handle:ISerializable", c"first"), p(c"handle:ISerializable", c"second")]),
    later(c"VRBladeSameHitData", VRBladeSameHitData, Some(c"Bool"), &[p(c"handle:gameHitShapeUserData", c"first"), p(c"handle:gameHitShapeUserData", c"second")]),
    later(c"VRBladeMatchesOccupied", VRBladeMatchesOccupied, Some(c"Bool"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"gameShapeData", c"hit"), p(c"CName", c"slot"), p(c"handle:ISerializable", c"geometry"), p(c"handle:gameHitShapeUserData", c"data"), p(c"CName", c"material"), p(c"Vector4", c"center"), p(c"Vector4", c"axis")]),
    later(c"VRBladeSweptContact", VRBladeSweptContact, Some(c"Vector4"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"Vector4", c"oldA"), p(c"Vector4", c"oldB"), p(c"Vector4", c"a"), p(c"Vector4", c"b")]),
    later(c"VRBladeEntrySweep", VRBladeEntrySweep, Some(c"Vector4"), &[p(c"handle:gameHitRepresentationComponent", c"component"), p(c"Vector4", c"oldA"), p(c"Vector4", c"oldB"), p(c"Vector4", c"a"), p(c"Vector4", c"b"), p(c"Vector4", c"endGrip")]),
    now(c"VRBladeCapsuleMetrics", VRBladeCapsuleMetrics, Some(c"Vector4"), &[p(c"Vector4", c"start"), p(c"Vector4", c"end"), p(c"Vector4", c"motion"), p(c"Vector4", c"edge"), p(c"Vector4", c"center"), p(c"Vector4", c"axis"), p(c"Int32", c"material")]),
    now(c"VRBladeCapsuleInterval", VRBladeCapsuleInterval, Some(c"Vector4"), &[p(c"Vector4", c"start"), p(c"Vector4", c"end"), p(c"Vector4", c"motion"), p(c"Vector4", c"edge"), p(c"Vector4", c"center"), p(c"Vector4", c"axis"), p(c"Int32", c"material")]),
    later(c"VRBladeSubmitJoint", VRBladeSubmitJoint, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"physicsTraceResult", c"trace"), p(c"Vector4", c"point"), p(c"Vector4", c"edge")]),
    now(c"VRBladeClearJoint", VRBladeClearJoint, None, &[p(c"Int32", c"side")]),
    later(c"VRBladeTraceKind", VRBladeTraceKind, Some(c"Int32"), &[p(c"physicsTraceResult", c"trace")]),
    later(c"VRBladeFindPuppetJoint", VRBladeFindPuppetJoint, Some(c"Bool"), &[p(c"handle:entEntity", c"target"), p(c"Int32", c"side"), p(c"Uint32", c"serial"), p(c"Vector4", c"point"), p(c"Vector4", c"edge")]),
    // main's hands in a car (VehicleHands.cpp)
    now(c"VRVehicleHandWorld", VRVehicleHandWorld, Some(c"Vector4"), &[p(c"Int32", c"side")]),
    now(c"VRVehicleHandClaim", VRVehicleHandClaim, None, &[p(c"Int32", c"side"), p(c"Bool", c"on")]),
    now(c"VRVehicleHandBusy", VRVehicleHandBusy, Some(c"Bool"), &[p(c"Int32", c"side")]),
    // main's hair the hands move (HairDangle.cpp) and the grip on a living person (NpcArmGrip.cpp)
    later(c"VRHairDangles", VRHairDangles, Some(c"String"), &[p(c"handle:entEntity", c"entity")]),
    later(c"VRHairPoints", VRHairPoints, Some(c"String"), &[p(c"handle:entEntity", c"entity")]),
    later(c"VRHairRaw", VRHairRaw, Some(c"String"), &[p(c"handle:entEntity", c"entity"), p(c"Int32", c"particle")]),
    later(c"VRHairStrands", VRHairStrands, Some(c"Int32"), &[p(c"handle:entEntity", c"entity"), p(c"String", c"weights")]),
    later(c"VRHairForce", VRHairForce, Some(c"Int32"), &[p(c"handle:entEntity", c"entity"), p(c"Vector4", c"force")]),
    later(c"VRDangleForce", VRDangleForce, Some(c"Int32"), &[p(c"handle:entEntity", c"entity"), p(c"Vector4", c"force"), p(c"Int32", c"part"), p(c"Int32", c"index")]),
    now(c"VRNpcGrip", VRNpcGrip, Some(c"Float"), &[p(c"handle:IScriptable", c"entity"), p(c"Int32", c"side"), p(c"Int32", c"part"), p(c"Vector4", c"target"), p(c"Vector4", c"turn")]),
    now(c"VRNpcGripState", VRNpcGripState, Some(c"array:Vector4"), &[p(c"Int32", c"side")]),
    now(c"VRNpcGripBind", VRNpcGripBind, Some(c"Float"), &[p(c"handle:IScriptable", c"entity"), p(c"Int32", c"side"), p(c"Int32", c"part"), p(c"Int32", c"segment"), p(c"Int32", c"owner")]),
    // main's bodies a hand reaches (HandBody.cpp's additions)
    now(c"VRNearBody", VRNearBody, Some(c"Vector4"), &[p(c"Vector4", c"center"), p(c"Float", c"radius"), p(c"Float", c"most")]),
    now(c"VRBodyOf", VRBodyOf, Some(c"handle:IScriptable"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRPickCone", VRPickCone, Some(c"Vector4"), &[p(c"Vector4", c"from"), p(c"Vector4", c"a"), p(c"Vector4", c"b"), p(c"Float", c"cone"), p(c"Float", c"range"), p(c"Float", c"most"), p(c"Int32", c"keepProxy"), p(c"Int32", c"keepBody")]),
    now(c"VRBodyInfo", VRBodyInfo, Some(c"Vector4"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRBodySolverMass", VRBodySolverMass, Some(c"Float"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body")]),
    now(c"VRCorpseHold", VRCorpseHold, Some(c"Float"), &[p(c"Int32", c"proxy"), p(c"Int32", c"held"), p(c"Int32", c"other"), p(c"Float", c"z"), p(c"Float", c"dt")]),
    now(c"VRCorpseSpeed", VRCorpseSpeed, Some(c"Float"), &[p(c"Int32", c"proxy")]),
    now(c"VRCorpseFreeze", VRCorpseFreeze, Some(c"Bool"), &[p(c"Int32", c"proxy"), p(c"Bool", c"frozen")]),
    now(c"VRGraspCapsule", VRGraspCapsule, Some(c"Vector4"), &[p(c"Int32", c"side"), p(c"Int32", c"style"), p(c"Vector4", c"center"), p(c"Vector4", c"axis"), p(c"Float", c"halfLength"), p(c"Float", c"radius"), p(c"Float", c"palmGap")]),
    now(c"VRGraspSeatTurn", VRGraspSeatTurn, Some(c"Quaternion"), &[p(c"Int32", c"side")]),
    now(c"VRHandHoldBody", VRHandHoldBody, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Vector4", c"pos"), p(c"Quaternion", c"rot"), p(c"Vector4", c"grip")]),
    now(c"VRBodySurface", VRBodySurface, Some(c"Vector4"), &[p(c"Int32", c"proxy"), p(c"Int32", c"body"), p(c"Vector4", c"point")]),
    now(c"VRTraceInfo", VRTraceInfo, Some(c"Vector4"), &[p(c"physicsTraceResult", c"trace")]),
    now(c"VRTraceBody", VRTraceBody, Some(c"handle:IScriptable"), &[p(c"physicsTraceResult", c"trace")]),
    now(c"VRBodyGetRot", VRBodyGetRot, Some(c"Quaternion"), &[p(c"handle:IScriptable", c"body")]),
    now(c"VRBodySetQueryable", VRBodySetQueryable, Some(c"Bool"), &[p(c"handle:IScriptable", c"body"), p(c"Bool", c"queryable")]),
    // main's fingers against what the hand meets (VrMenu.cpp, HandGestures.cpp)
    now(c"VRFingerContact", VRFingerContact, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"digit"), p(c"Float", c"limit"), p(c"Bool", c"hold")]),
    now(c"VRFingerBend", VRFingerBend, Some(c"Bool"), &[p(c"Int32", c"side"), p(c"Int32", c"digit"), p(c"Float", c"angle")]),
    now(c"VRCarryLead", VRCarryLead, Some(c"Float"), &[]),
    now(c"VRFingerClosed", VRFingerClosed, Some(c"Float"), &[p(c"Int32", c"side"), p(c"Int32", c"digit")]),
    now(c"VRFingerWanted", VRFingerWanted, Some(c"Float"), &[p(c"Int32", c"side"), p(c"Int32", c"digit")]),
    now(c"VRHandContactMargin", VRHandContactMargin, Some(c"Float"), &[]),
    now(c"VRGripForce", VRGripForce, Some(c"Float"), &[p(c"Int32", c"side")]),
    now(c"VRPromptPart", VRPromptPart, Some(c"CName"), &[p(c"CName", c"part")]),
    now(c"VRPalmModelRot", VRPalmModelRot, Some(c"Quaternion"), &[p(c"Int32", c"right")]),
    now(c"VRSmokeMouthWorldPos", VRSmokeMouthWorldPos, Some(c"Vector4"), &[]),
    now(c"VRSmokeMouthWorldRot", VRSmokeMouthWorldRot, Some(c"Quaternion"), &[]),
    now(c"SetVRSmokeSmokeOffset", SetVRSmokeSmokeOffset, Some(c"Int32"), &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll")]),
    now(c"SetVRSmokeCigChunks", SetVRSmokeCigChunks, Some(c"Int32"), &[p(c"handle:GameObject", c"cig"), p(c"Int32", c"count")]),
    now(c"SetVRSmokeCigVisualScale", SetVRSmokeCigVisualScale, Some(c"Int32"), &[p(c"handle:GameObject", c"cig"), p(c"Float", c"y")]),
    now(c"GetVRSmokeCigVisualScaleY", GetVRSmokeCigVisualScaleY, Some(c"Float"), &[p(c"handle:GameObject", c"cig")]),
    now(c"SetVRSmokeMouthOffset", SetVRSmokeMouthOffset, Some(c"Int32"), &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll")]),
    now(c"SetVRSmokeCigOffset", SetVRSmokeCigOffset, Some(c"Int32"), &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll")]),
    now(c"SetVRSmokeFingersL", SetVRSmokeFingersL, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"SetVRSmokeLeftCig", SetVRSmokeLeftCig, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"SetVRSmokeRightLighter", SetVRSmokeRightLighter, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"VRSmokeCaptureFingersL", VRSmokeCaptureFingersL, Some(c"Int32"), &[]),
    now(c"SetVRSmokeLighter", SetVRSmokeLighter, Some(c"Int32"), &[p(c"Int32", c"enable")]),
    now(c"SetVRSmokeLighterOffset", SetVRSmokeLighterOffset, Some(c"Int32"), &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll")]),
    now(c"SetVRSmokeThumbFlickL", SetVRSmokeThumbFlickL, Some(c"Int32"), &[p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll")]),
    now(c"SetVRSmokeThumbPressL", SetVRSmokeThumbPressL, Some(c"Int32"), &[p(c"Float", c"amount")]),
    now(c"SetVRBindMode", SetVRBindMode, Some(c"Int32"), &[p(c"Int32", c"mode")]),
    now(c"SetVRBindParams", SetVRBindParams, Some(c"Int32"), &[p(c"Float", c"scale"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Int32", c"axis"), p(c"Int32", c"hand")]),
    now(c"SetVRElbowPole", SetVRElbowPole, Some(c"Int32"), &[p(c"Float", c"angle"), p(c"Int32", c"hand")]),
    now(c"SetVRElbowSwing", SetVRElbowSwing, Some(c"Int32"), &[p(c"Float", c"gain"), p(c"Int32", c"hand")]),
    now(c"SetVRHandOffset", SetVRHandOffset, Some(c"Int32"), &[p(c"Float", c"pitch"), p(c"Float", c"yaw"), p(c"Float", c"roll"), p(c"Int32", c"hand")]),
    now(c"SetVRBindBones", SetVRBindBones, Some(c"Int32"), &[p(c"Int32", c"leftIdx"), p(c"Int32", c"rightIdx")]),
    now(c"SetVRHeadBone", SetVRHeadBone, Some(c"Int32"), &[p(c"Int32", c"index")]),
    now(c"SetVRUseHeadRelative", SetVRUseHeadRelative, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"SetVRDiagCapture", SetVRDiagCapture, Some(c"Int32"), &[p(c"Int32", c"on")]),
    now(c"LogVRDiag", LogVRDiag, Some(c"Int32"), &[p(c"Float", c"camX"), p(c"Float", c"camY"), p(c"Float", c"camZ"), p(c"Float", c"qi"), p(c"Float", c"qj"), p(c"Float", c"qk"), p(c"Float", c"qr")]),
    now(c"SetVRPlayerYaw", SetVRPlayerYaw, Some(c"Int32"), &[p(c"Float", c"yaw"), p(c"Float", c"ci"), p(c"Float", c"cj"), p(c"Float", c"ck"), p(c"Float", c"cr"), p(c"Float", c"camX"), p(c"Float", c"camY"), p(c"Float", c"camZ"), p(c"Float", c"entX"), p(c"Float", c"entY"), p(c"Float", c"entZ"), p(c"Float", c"eqi"), p(c"Float", c"eqj"), p(c"Float", c"eqk"), p(c"Float", c"eqr")]),
    now(c"DumpAnimVTable", DumpAnimVTable, Some(c"Int32"), &[]),
    now(c"DumpAnimMemory", DumpAnimMemory, Some(c"Int32"), &[]),
    now(c"DumpAnimControllerComponents", DumpAnimControllerComponents, Some(c"Int32"), &[]),
    now(c"DumpRuntimeClassFunctions", DumpRuntimeClassFunctions, Some(c"Int32"), &[]),
    now(c"SetVRIKAnimInputTestMode", SetVRIKAnimInputTestMode, None, &[p(c"Int32", c"mode")]),
    now(c"UpdateVRIKAnimInputs", UpdateVRIKAnimInputs, Some(c"Int32"), &[]),
    now(c"DumpAnimControllerFunctionDetails", DumpAnimControllerFunctionDetails, Some(c"Int32"), &[]),
    now(c"DumpAnimControllerListeners", DumpAnimControllerListeners, Some(c"Int32"), &[]),
    now(c"DumpInterestingAnimClassProperties", DumpInterestingAnimClassProperties, Some(c"Int32"), &[]),
    now(c"DumpAnimationSystemCandidates", DumpAnimationSystemCandidates, Some(c"Int32"), &[]),
    now(c"RunIKTargetAddTest", RunIKTargetAddTest, Some(c"Int32"), &[p(c"Int32", c"mode")]),
    now(c"TestAnimFloatInput", TestAnimFloatInput, Some(c"Int32"), &[p(c"CName", c"inputName"), p(c"Float", c"value"), p(c"Int32", c"route")]),
    now(c"TestAnimFloatPreset", TestAnimFloatPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value"), p(c"Int32", c"route")]),
    now(c"SetPlayerAnimParameter", SetPlayerAnimParameter, Some(c"Int32"), &[p(c"CName", c"inputName"), p(c"Float", c"value")]),
    now(c"SetPlayerAnimParameterPreset", SetPlayerAnimParameterPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"SetPlayerAnimParameterPersistentPreset", SetPlayerAnimParameterPersistentPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"GetPlayerAnimParameterPersistentLastResult", GetPlayerAnimParameterPersistentLastResult, Some(c"Int32"), &[]),
    now(c"TestWeaponUserFeatureRoute", TestWeaponUserFeatureRoute, Some(c"Int32"), &[p(c"Int32", c"route")]),
    now(c"TestIKFeatureRoute", TestIKFeatureRoute, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Int32", c"route")]),
    now(c"TestMeleeIKDataFeatureRoute", TestMeleeIKDataFeatureRoute, Some(c"Int32"), &[p(c"Int32", c"route")]),
    now(c"DumpRootGraphVariables", DumpRootGraphVariables, Some(c"Int32"), &[]),
    now(c"SetRootGraphBoolVariable", SetRootGraphBoolVariableByName, Some(c"Int32"), &[p(c"CName", c"name"), p(c"Bool", c"value")]),
    now(c"SetRootGraphFloatVariable", SetRootGraphFloatVariableByName, Some(c"Int32"), &[p(c"CName", c"name"), p(c"Float", c"value")]),
    now(c"SetRootGraphVectorVariable", SetRootGraphVectorVariableByName, Some(c"Int32"), &[p(c"CName", c"name"), p(c"Vector4", c"value")]),
    now(c"TestRootGraphFloatPreset", TestRootGraphFloatPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"TestRootGraphVectorPreset", TestRootGraphVectorPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Vector4", c"value")]),
    now(c"SetRootGraphFloatPresetPersistent", SetRootGraphFloatPresetPersistent, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"SetRootGraphVectorPresetPersistent", SetRootGraphVectorPresetPersistent, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Vector4", c"value")]),
    now(c"GetRootGraphPersistentLastResult", GetRootGraphPersistentLastResult, Some(c"Int32"), &[]),
    now(c"DumpPlayerAnimatedObjectRuntime", DumpPlayerAnimatedObjectRuntime, Some(c"Int32"), &[]),
    now(c"DumpAnimationSystemLookup", DumpAnimationSystemLookup, Some(c"Int32"), &[]),
    now(c"DumpRootMetaRigTracks", DumpRootMetaRigTracks, Some(c"Int32"), &[]),
    now(c"TestRootMetaRigTrackPreset", TestRootMetaRigTrackPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"SetRootMetaRigTrackPresetPersistent", SetRootMetaRigTrackPresetPersistent, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value")]),
    now(c"GetRootMetaRigTrackPersistentLastResult", GetRootMetaRigTrackPersistentLastResult, Some(c"Int32"), &[]),
    now(c"DumpRootAnimatedObjectFloatArrayCandidates", DumpRootAnimatedObjectFloatArrayCandidates, Some(c"Int32"), &[]),
    now(c"TestRootLiveTrackPreset", TestRootLiveTrackPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value"), p(c"Int32", c"arrayMode")]),
    now(c"SetRootLiveTrackPresetPersistent", SetRootLiveTrackPresetPersistent, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Float", c"value"), p(c"Int32", c"arrayMode")]),
    now(c"GetRootLiveTrackPersistentLastResult", GetRootLiveTrackPersistentLastResult, Some(c"Int32"), &[]),
    now(c"ReadRootLiveTrackPreset", ReadRootLiveTrackPreset, Some(c"Int32"), &[p(c"Int32", c"mode"), p(c"Int32", c"arrayMode")]),
    now(c"InstallWeaponAimHook", InstallWeaponAimHook, Some(c"Int32"), &[]),
    now(c"InstallVRProvInstrument", InstallVRProvInstrument, Some(c"Int32"), &[]),
    now(c"SetVRProvOverrideSlot", SetVRProvOverrideSlot, None, &[p(c"Int32", c"slot")]),
    now(c"GetVRProvDump", GetVRProvDump, Some(c"Float"), &[p(c"Int32", c"idx")]),
    now(c"ResetVRProvCounts", ResetVRProvCounts, None, &[]),
    now(c"SetVRProvQuatMode", SetVRProvQuatMode, None, &[p(c"Int32", c"mode"), p(c"Int32", c"axis")]),
    now(c"SetVRMuzzlePos", SetVRMuzzlePos, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"SetVRCarryLeft", SetVRCarryLeft, None, &[p(c"Int32", c"on")]),
    now(c"SetVRTwoHandSuppress", SetVRTwoHandSuppress, None, &[p(c"Int32", c"on")]),
    now(c"GetVRCarryOffset", GetVRCarryOffset, Some(c"Float"), &[p(c"Int32", c"idx")]),
    now(c"SetVRInteractionDepth", SetVRInteractionDepth, None, &[p(c"Float", c"depth")]),
    now(c"SetVRHudClearance", SetVRHudClearance, None, &[p(c"Float", c"depth")]),
    now(c"SetVRAimHit", SetVRAimHit, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"nx"), p(c"Float", c"ny"), p(c"Float", c"nz")]),
    now(c"SetVRMuzzleQuat", SetVRMuzzleQuat, None, &[p(c"Float", c"i"), p(c"Float", c"j"), p(c"Float", c"k"), p(c"Float", c"r")]),
    now(c"SetVRWeaponName", SetVRWeaponName, None, &[p(c"String", c"name")]),
    now(c"SetVRWeaponKick", SetVRWeaponKick, None, &[p(c"Float", c"kick")]),
    now(c"SetVRSmartWeapon", SetVRSmartWeapon, None, &[p(c"Bool", c"smart")]),
    now(c"SetVRAttachmentRecoil", SetVRAttachmentRecoil, None, &[p(c"Int32", c"side"), p(c"Float", c"scale")]),
    now(c"GetVRLastShotPeak", GetVRLastShotPeak, Some(c"Vector4"), &[p(c"Int32", c"side")]),
    now(c"SetVRLeftWeaponKick", SetVRLeftWeaponKick, None, &[p(c"Float", c"kick"), p(c"Int32", c"cls")]),
    now(c"SetVRZoomLevel", SetVRZoomLevel, None, &[p(c"Float", c"zoom")]),
    now(c"VRPhysicalOpticsEnabled", VRPhysicalOpticsEnabled, Some(c"Bool"), &[]),
    now(c"VRPhysicalOpticsCameraReady", VRPhysicalOpticsCameraReady, Some(c"Bool"), &[]),
    now(c"VRLuaHostGc", VRLuaHostGc, Some(c"Bool"), &[p(c"Bool", c"want")]),
    now(c"SetVRPhysicalOpticsPath", SetVRPhysicalOpticsPath, None, &[p(c"Int32", c"path")]),
    now(c"GetVRMainEyeOffset", GetVRMainEyeOffset, Some(c"Float"), &[]),
    now(c"SetVRSightZoom", SetVRSightZoom, None, &[p(c"Bool", c"active")]),
    now(c"SetVRSightDot", SetVRSightDot, None, &[p(c"Bool", c"active")]),
    now(c"SetVRSightApproach", SetVRSightApproach, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"SetVRSightLine", SetVRSightLine, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"VRPhysicalOpticsProbe", VRPhysicalOpticsProbe, Some(c"Bool"), &[p(c"handle:entIComponent", c"camera")]),
    now(c"VRPhysicalOpticsHinge", VRPhysicalOpticsHinge, Some(c"Bool"), &[p(c"handle:entEntity", c"weapon"), p(c"Bool", c"enabled")]),
    now(c"SetVRReloadOwnedHand", SetVRReloadOwnedHand, None, &[p(c"Int32", c"hand")]),
    now(c"SetVRTwoHandSlide", SetVRTwoHandSlide, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z")]),
    now(c"GetVRTwoHandActive", GetVRTwoHandActive, Some(c"Bool"), &[]),
    now(c"GetVRTwoHandLeftCtrl", GetVRTwoHandLeftCtrl, Some(c"Vector4"), &[]),
    now(c"SetVRLeftTriggerBlock", SetVRLeftTriggerBlock, None, &[p(c"Int32", c"on")]),
    now(c"VRGrenadeStick", VRGrenadeStick, Some(c"Int32"), &[p(c"Int32", c"side")]),
    now(c"VRWilsonRangeTarget", VRWilsonRangeTarget, Some(c"NodeRef"), &[]),
    now(c"SetVRDualWield", SetVRDualWield, None, &[p(c"Int32", c"on")]),
    now(c"SetVRLeftMuzzle", SetVRLeftMuzzle, None, &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Float", c"qi"), p(c"Float", c"qj"), p(c"Float", c"qk"), p(c"Float", c"qr")]),
    now(c"SetVRDualWieldRecords", SetVRDualWieldRecords, None, &[p(c"Int32", c"left"), p(c"Int32", c"right")]),
    now(c"SetVRDualWieldWeapons", SetVRDualWieldWeapons, None, &[p(c"handle:entEntity", c"left"), p(c"handle:entEntity", c"right")]),
    now(c"SetVRSprintActive", SetVRSprintActive, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"GetVRPhysicalCrouch", GetVRPhysicalCrouch, Some(c"Int32"), &[]),
    now(c"SetVRPhysicalCrouchFeedback", SetVRPhysicalCrouchFeedback, Some(c"Int32"), &[p(c"Int32", c"applied"), p(c"Int32", c"actual")]),
    now(c"GetVRPhysicalCrouchCameraAnchor", GetVRPhysicalCrouchCameraAnchor, Some(c"Vector4"), &[]),
    later(c"SetVRLadderState", SetVRLadderState, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"Int32", c"detailed"), p(c"Int32", c"paused"), p(c"handle:IScriptable", c"description")]),
    later(c"SetVRLadderTopRails", SetVRLadderTopRails, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"Vector4", c"ladderPosition"), p(c"Vector4", c"origin"), p(c"Vector4", c"right"), p(c"Vector4", c"normal"), p(c"Vector4", c"up")]),
    later(c"SetVRLadderMeasuredTop", SetVRLadderMeasuredTop, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"Vector4", c"ladderPosition"), p(c"Float", c"top")]),
    now(c"GetVRLadderDebug", GetVRLadderDebug, Some(c"Float"), &[p(c"Int32", c"index")]),
    now(c"SetVRLadderTestGrip", SetVRLadderTestGrip, Some(c"Int32"), &[p(c"Float", c"left"), p(c"Float", c"right"), p(c"Int32", c"milliseconds")]),
    later(c"SetVRSwimmingState", SetVRSwimmingState, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"Int32", c"state"), p(c"Int32", c"fast"), p(c"Int32", c"paused"), p(c"Int32", c"waterContext")]),
    now(c"GetVRSwimmingDebug", GetVRSwimmingDebug, Some(c"Float"), &[p(c"Int32", c"index")]),
    now(c"SetVRLocomotionState", SetVRLocomotionState, Some(c"Int32"), &[p(c"Int32", c"state")]),
    now(c"SetVRLocomotionDetailed", SetVRLocomotionDetailed, Some(c"Int32"), &[p(c"Int32", c"state")]),
    now(c"SetVRScannerState", SetVRScannerState, Some(c"Int32"), &[p(c"Int32", c"state")]),
    now(c"SetVRWorldTimeScale", SetVRWorldTimeScale, Some(c"Int32"), &[p(c"Float", c"scale")]),
    now(c"SetVRPairedAnimationState", SetVRPairedAnimationState, Some(c"Int32"), &[p(c"Int32", c"takedown"), p(c"Int32", c"bodyCarrying"), p(c"Int32", c"bodyDisposal")]),
    now(c"SetVRBodyCarryBothHands", SetVRBodyCarryBothHands, Some(c"Int32"), &[p(c"Bool", c"bothHands")]),
    now(c"SetVRReloadAnimation", SetVRReloadAnimation, Some(c"Int32"), &[p(c"Bool", c"active"), p(c"Float", c"finishIn")]),
    now(c"SetVRFinisherActive", SetVRFinisherActive, Some(c"Int32"), &[p(c"Bool", c"active")]),
    now(c"VRStoryAttentionUpdate", VRStoryAttentionUpdate, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"Int32", c"scanStarted"), p(c"Int32", c"scanDone"), p(c"Int32", c"locomotion"), p(c"Int32", c"tier"), p(c"Int32", c"workspot"), p(c"Int32", c"vehicle"), p(c"Int32", c"choices"), p(c"Int32", c"paused")]),
    now(c"VRManualClueUpdate", VRManualClueUpdate, Some(c"Int32"), &[p(c"handle:gameObject", c"player"), p(c"handle:gameObject", c"clue")]),
    now(c"SetVRWeaponPoseState", SetVRWeaponPoseState, Some(c"Int32"), &[p(c"Int32", c"weaponState"), p(c"Float", c"aimInRemaining")]),
    now(c"SetVRWeaponRaiseTransition", SetVRWeaponRaiseTransition, Some(c"Int32"), &[p(c"Int32", c"active")]),
    now(c"SetVRMeleeFire", SetVRMeleeFire, None, &[p(c"Int32", c"fire")]),
    now(c"SetVRTriggerMode", SetVRTriggerMode, None, &[p(c"Int32", c"mode")]),
    now(c"SetVRCamBoneFreeze", SetVRCamBoneFreeze, None, &[p(c"Int32", c"on")]),
    now(c"SetVRPairSlew", SetVRPairSlew, None, &[p(c"Float", c"rate")]),
    now(c"SetVRPairLead", SetVRPairLead, None, &[p(c"Float", c"ticks")]),
    now(c"SetVRCamAck", SetVRCamAck, None, &[p(c"Float", c"seq")]),
    now(c"GetVRMeleeTrigger", GetVRMeleeTrigger, Some(c"Int32"), &[]),
    now(c"GetVRSharedSlot", GetVRSharedSlot, Some(c"Float"), &[p(c"Int32", c"idx")]),
    // The mechanics whose CET Lua the plugin replaces (rust/crates/cvr-ffi/src/mechanics.rs): their mods yield.
    now(c"VRRustMechanics", crate::mechanics::VRRustMechanics, Some(c"Int32"), &[]),
    now(c"VRRustMechanics2", crate::mechanics::VRRustMechanics2, Some(c"Int32"), &[]),
    now(c"VRIKEquipCyberware", crate::mechanics::VRIKEquipCyberware, Some(c"String"), &[p(c"String", c"kind")]),
    now(c"VRIKCyberwareStatus", crate::mechanics::VRIKCyberwareStatus, Some(c"String"), &[]),
    now(c"VRBallToggle", crate::mechanics::VRBallToggle, None, &[]),
    now(c"VRBallSetCorrection", crate::mechanics::VRBallSetCorrection, None, &[p(c"Bool", c"on")]),
    now(c"VRBallDropTest", crate::mechanics::VRBallDropTest, None, &[p(c"Float", c"height")]),
    now(c"VRBallTestThrow", crate::mechanics::VRBallTestThrow, None, &[p(c"Float", c"vx"), p(c"Float", c"vy"), p(c"Float", c"vz"), p(c"Float", c"hold")]),
    now(c"VRBallReport", crate::mechanics::VRBallReport, Some(c"String"), &[]),
    now(c"VRBallStatus", crate::mechanics::VRBallStatus, Some(c"String"), &[]),
    now(c"VRWorldMapController", crate::mechanics::VRWorldMapController, None, &[p(c"Bool", c"alive")]),
    now(c"VRPhysicalCrouchEnter", crate::mechanics::VRPhysicalCrouchEnter, None, &[p(c"handle:GameObject", c"owner")]),
    now(c"VRSwimmingFast", crate::mechanics::VRSwimmingFast, None, &[p(c"Int32", c"fast")]),
    now(c"VRLadderCapture", crate::mechanics::VRLadderCapture, None, &[p(c"handle:GameObject", c"owner"), p(c"handle:IScriptable", c"description")]),
    now(c"VRLadderClear", crate::mechanics::VRLadderClear, None, &[]),
    now(c"VRCrosshairContainer", crate::mechanics::VRCrosshairContainer, None, &[p(c"handle:IScriptable", c"container")]),
    now(c"VRCrosshairUpdate", crate::mechanics::VRCrosshairUpdate, None, &[]),
    now(c"VRStereoOverlayActive", crate::mechanics::VRStereoOverlayActive, None, &[p(c"Bool", c"active")]),
    now(c"VRBraindanceHudArm", crate::mechanics::VRBraindanceHudArm, None, &[]),
    now(c"VRVrcamReload", crate::mechanics::VRVrcamReload, None, &[]),
    now(c"VRStereoPreGame", crate::mechanics::VRStereoPreGame, Some(c"Bool"), &[]),
    now(c"VRVrcamSettled", crate::mechanics::VRVrcamSettled, Some(c"Bool"), &[]),
    now(c"VRStereoOverlayOpen", crate::mechanics::VRStereoOverlayOpen, Some(c"Bool"), &[]),
    now(c"VRMenuModOpen", crate::mechanics::VRMenuModOpen, None, &[p(c"Bool", c"open")]),
    now(c"VRReachMark", crate::mechanics::VRReachMark, None, &[p(c"Int32", c"hand"), p(c"Vector4", c"pos"), p(c"Float", c"radius"), p(c"Float", c"lift"), p(c"Int32", c"kind"), p(c"Bool", c"ready"), p(c"Vector4", c"from")]),
    now(c"VRReachPin", crate::mechanics::VRReachPin, None, &[p(c"Int32", c"hand"), p(c"Vector4", c"pos"), p(c"Float", c"radius"), p(c"Float", c"lift"), p(c"Int32", c"kind"), p(c"Bool", c"ready"), p(c"Vector4", c"from")]),
    now(c"VRReachHold", crate::mechanics::VRReachHold, None, &[p(c"Vector4", c"point"), p(c"Float", c"seconds")]),
    now(c"VRIKGameAttached", crate::mechanics::VRIKGameAttached, None, &[]),
    now(c"VRIKTrackingEnabled", crate::mechanics::VRIKTrackingEnabled, None, &[p(c"Bool", c"on")]),
    now(c"VRIKLogDiag", crate::mechanics::VRIKLogDiag, None, &[]),
    now(c"VRHandsOnlyDetach", crate::mechanics::VRHandsOnlyDetach, None, &[]),
    now(c"VRHandsOnlyForce", crate::mechanics::VRHandsOnlyForce, None, &[p(c"Int32", c"force"), p(c"Int32", c"scene")]),
    now(c"VRCameraBridgeAttach", crate::mechanics::VRCameraBridgeAttach, None, &[p(c"Bool", c"attached")]),
    now(c"VRCameraBridgeKnobs", crate::mechanics::VRCameraBridgeKnobs, None, &[p(c"Bool", c"sceneGate"), p(c"Bool", c"sceneInTakeover")]),
    now(c"VRCameraBridgeContext", crate::mechanics::VRCameraBridgeContext, Some(c"Int32"), &[]),
    now(c"VRWeaponAttached", crate::mechanics::VRWeaponAttached, None, &[]),
    now(c"VRHolsterRelease", crate::mechanics::VRHolsterRelease, None, &[]),
    now(c"VRBeltAttached", crate::mechanics::VRBeltAttached, None, &[]),
    now(c"VRBeltMagTaken", crate::mechanics::VRBeltMagTaken, None, &[p(c"Bool", c"on")]),
    now(c"VRBeltShellTaken", crate::mechanics::VRBeltShellTaken, None, &[p(c"Bool", c"on"), p(c"Int32", c"side")]),
    now(c"VRBeltReach", crate::mechanics::VRBeltReach, Some(c"Float"), &[p(c"Int32", c"what"), p(c"Int32", c"side")]),
    now(
        c"VRReloadTune",
        crate::mechanics::VRReloadTune,
        None,
        &[
            p(c"Bool", c"hold"),
            p(c"Float", c"dx"),
            p(c"Float", c"dy"),
            p(c"Float", c"dz"),
            p(c"Float", c"rx"),
            p(c"Float", c"ry"),
            p(c"Float", c"rz"),
            p(c"Float", c"stage"),
            p(c"Float", c"seat"),
            p(c"Float", c"lead"),
            p(c"Int32", c"rev"),
        ],
    ),
    now(c"VRReloadTuneJoint", crate::mechanics::VRReloadTuneJoint, None, &[p(c"Int32", c"i"), p(c"Float", c"w")]),
    now(c"VRLobbyMenu", crate::mechanics::VRLobbyMenu, None, &[p(c"Int32", c"what"), p(c"handle:IScriptable", c"object")]),
    now(c"VRLobbySaves", crate::mechanics::VRLobbySaves, None, &[p(c"String", c"names")]),
    now(c"VRMenuHook", crate::mechanics::VRMenuHook, Some(c"Bool"), &[p(c"Int32", c"kind"), p(c"handle:IScriptable", c"a"), p(c"handle:IScriptable", c"b"), p(c"CName", c"name")]),
    now(c"VRMenuLoadNames", crate::mechanics::VRMenuLoadNames, Some(c"Bool"), &[p(c"Float", c"marker"), p(c"String", c"names")]),
    now(c"VRHolsterNear", crate::mechanics::VRHolsterNear, Some(c"Float"), &[p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"z"), p(c"Int32", c"side")]),
    now(c"VRBeltGrenadeReach", crate::mechanics::VRBeltGrenadeReach, None, &[p(c"Bool", c"left"), p(c"Bool", c"right")]),
    now(c"VRBraindanceHudDisarm", crate::mechanics::VRBraindanceHudDisarm, None, &[]),
    now(c"GetVRWeaponAim", GetVRWeaponAim, Some(c"Int32"), &[]),
    now(c"VRKeypadUpdate", VRKeypadUpdate, Some(c"Int32"), &[p(c"handle:inkWidget", c"root")]),
    now(c"VRScannerSlotGet", VRScannerSlotGet, Some(c"Float"), &[p(c"Int32", c"idx"), p(c"Int32", c"comp")]),
    now(c"VRScannerSlotSet", VRScannerSlotSet, Some(c"Int32"), &[p(c"Int32", c"idx"), p(c"Float", c"x"), p(c"Float", c"y"), p(c"Float", c"scale")]),
    now(c"VRScannerSlotSave", VRScannerSlotSave, Some(c"Int32"), &[]),
    now(c"SetVRMenuOpen", SetVRMenuOpen, Some(c"Int32"), &[p(c"Int32", c"open")]),
    now(c"SetVRDeviceScreen", SetVRDeviceScreen, Some(c"Int32"), &[p(c"Int32", c"open")]),
    // The full-body tracking natives (formerly src/Fbt/Game/Natives.cpp's, registered after the table).
    now(c"VRFbtCalibrating", vr_fbt_calibrating, Some(c"Int32"), &[]),
    now(c"VRFbtCalibrationStep", vr_fbt_calibration_step, Some(c"Int32"), &[]),
    now(c"VRFbtSetCalibrationText", vr_fbt_set_calibration_text, Some(c"Bool"), &[p(c"Int32", c"step"), p(c"String", c"text")]),
];

extern "C" {
    /// The full-body tracking calibration (cvr-render fbt_calibration.rs).
    fn cvr_cpp_fbt_calibrating() -> bool;
    fn cvr_cpp_fbt_calibration_step() -> i32;
    fn cvr_fbt_set_step_text(slot: i32, text: *const core::ffi::c_char);
}
/// VRFbtCalibrating() -> Int32: 1 while a calibration runs.
unsafe extern "C" fn vr_fbt_calibrating(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    // SAFETY: the VM's frame and result slot.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<i32>() = i32::from(cvr_cpp_fbt_calibrating());
        }
    }
}
/// VRFbtCalibrationStep() -> Int32: 0 off, 1 head, 2 body, 3 arms.
unsafe extern "C" fn vr_fbt_calibration_step(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    // SAFETY: as above.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.finish_params();
        }
        if !out.is_null() {
            *out.cast::<i32>() = cvr_cpp_fbt_calibration_step();
        }
    }
}
/// VRFbtSetCalibrationText(step, text) -> Bool: the script's localized text for a calibration step.
unsafe extern "C" fn vr_fbt_set_calibration_text(_context: *mut c_void, frame: *mut StackFrame, out: *mut c_void, _a4: i64) {
    let mut step = 0i32;
    let mut text = red4ext_abi::cstring::CString::empty();
    // SAFETY: as above; the text is the engine's, destroyed by its own destructor below.
    unsafe {
        if let Some(frame) = frame.as_mut() {
            frame.get_parameter(&mut step);
            frame.get_parameter(&mut text);
            frame.finish_params();
        }
        cvr_fbt_set_step_text(step, text.c_str());
        if !out.is_null() {
            *out.cast::<bool>() = true;
        }
    }
}

extern "C" {
    fn cvr_cpp_spawn_register_types();
    fn cvr_cpp_spawn_post_register_types();
    fn cvr_cpp_check_sdk_layouts();
}

/// A `later` entry not complete yet: what of its signature is still to go in.
struct Pending {
    function: *mut GlobalFunction,
    returns: Option<&'static CStr>,
    params: &'static [Param],
    name: &'static CStr,
}
// SAFETY: the RTTI registry owns the functions for the process's life; the list is only touched under its lock.
unsafe impl Send for Pending {}
static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());

fn type_name(ty: &CStr) -> &str {
    ty.to_str().unwrap_or("")
}
/// `CBaseFunction::SetReturnType(CName)` with a `const char*`: the type by its name's value, the pool untouched, so
/// a composite name nothing has put there yet is no type.
fn set_return_type(rtti: &RttiSystem, f: &mut GlobalFunction, ty: &CStr) -> bool {
    match rtti.get_type(CName::hashed(type_name(ty))) {
        Some(ty) => f.base.set_return_type_of(ty),
        None => false,
    }
}
/// `CBaseFunction::AddParam(CName, name, false, optional)`, the type found as set_return_type finds it.
fn add_param(rtti: &RttiSystem, f: &mut GlobalFunction, param: &Param) -> bool {
    match rtti.get_type(CName::hashed(type_name(param.ty))) {
        Some(ty) => f.base.add_param_of(ty, param.name, false, param.optional),
        None => false,
    }
}
/// RttiType (include/Natives/RttiType.hpp): the type by name, the name put into the pool first so RTTI can make a
/// composite type from its text.
fn known(rtti: &RttiSystem, ty: &CStr) -> Option<&'static IType> {
    rtti.get_type(CName::add(ty))
}

/// True when complete. What the script loader filled in (a native declared by a script) is left as it is.
fn complete(rtti: &RttiSystem, s: &mut Pending) -> bool {
    // SAFETY: the function is the registry's, alive for the process.
    let f = unsafe { &mut *s.function };
    if s.returns.is_some() && !f.base.return_type.is_null() {
        s.returns = None;
    }
    if let Some(returns) = s.returns {
        if known(rtti, returns).is_some() {
            set_return_type(rtti, f, returns);
            s.returns = None;
        }
    }
    if !s.params.is_empty() && !f.base.params.is_empty() {
        s.params = &[];
    }
    if !s.params.is_empty() && s.params.iter().all(|p| known(rtti, p.ty).is_some()) {
        for param in s.params {
            add_param(rtti, f, param);
        }
        s.params = &[];
    }
    s.returns.is_none() && s.params.is_empty()
}

/// Late signature completion at game states Initialization and Running (rust/crates/cvr-ffi/src/plugin.rs): the
/// signatures whose types were not there at registration, after the scripts are loaded, and again at Running for
/// anything still missing.
pub unsafe extern "C" fn complete_signatures(_app: *mut GameApplication) -> bool {
    let Some(rtti) = RttiSystem::get() else { return true };
    let mut pending = PENDING.lock().unwrap_or_else(|p| p.into_inner());
    if pending.is_empty() {
        return true;
    }
    let before = pending.len();
    pending.retain_mut(|s| !complete(rtti, s));
    if pending.len() != before {
        log::write(&format!(
            "[natives] {} signature(s) completed, {} still waiting for their types\n",
            before - pending.len(),
            pending.len()
        ));
        for s in pending.iter() {
            log::write(&format!("[natives]   {}\n", type_name(s.name)));
        }
    }
    true
}

extern "C" fn register_types() {
    // SAFETY: the C++ spawn system's own type (src/Natives/SpawnSystem.cpp).
    unsafe { cvr_cpp_spawn_register_types() };
}

extern "C" fn post_register_types() {
    // SAFETY: the spawn system's parent class, from the C++ half (src/Natives/SpawnSystem.cpp).
    unsafe { cvr_cpp_spawn_post_register_types() };
    if let Some(rtti) = RttiSystem::get() {
        for native in NATIVES {
            let Some(f) = GlobalFunction::create(native.name, native.name, native.native) else {
                continue;
            };
            f.base.flags = function_flags::IS_NATIVE | function_flags::IS_STATIC;
            let f: *mut GlobalFunction = f;
            if native.later {
                let mut s = Pending {
                    function: f,
                    returns: native.returns,
                    params: native.params,
                    name: native.name,
                };
                if !complete(rtti, &mut s) {
                    PENDING.lock().unwrap_or_else(|p| p.into_inner()).push(s);
                }
            } else {
                // SAFETY: just made, not registered yet.
                let g = unsafe { &mut *f };
                if let Some(returns) = native.returns {
                    set_return_type(rtti, g, returns);
                }
                for param in native.params {
                    add_param(rtti, g, param);
                }
            }
            // SAFETY: made by GlobalFunction::create from PoolRTTIFunction.
            unsafe { rtti.register_function(f) };
        }
    }
    // SAFETY: the SDK classes the port includes against this game's RTTI, logged once: every native class is
    // registered by now (this also runs on a build the gate turns VR off for).
    unsafe { cvr_cpp_check_sdk_layouts() };
}

/// The natives' RTTI callbacks: on Load, before the heavy graphics init, so the RTTI window cannot be missed.
pub fn register() {
    if let Some(rtti) = RttiSystem::get() {
        rtti.add_register_callback(register_types);
        rtti.add_post_register_callback(post_register_types);
    }
}
