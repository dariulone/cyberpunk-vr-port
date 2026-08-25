// CyberpunkVRPort -- a computer or monitor must not take the camera away from the head.
//
// THE SYMPTOM. Activate a computer and the RIGHT eye (MAIN) is glued: the view will not turn with
// the head. The LEFT eye still looks around normally. That asymmetry is the whole clue -- the left
// eye is this port's own render-to-texture camera on the PLAYER entity, and nothing a device does
// touches it, while MAIN is the game's active camera and the device takes that one over.
//
// WHAT THE GAME DOES, read out of cyberpunk/devices/core/deviceBase.swift:
//
//   line 465   m_cameraZoomComponent = GetComponent(ri, n"cameraZoomComponent") as CameraComponent
//              m_cameraZoomComponent.SetIsHighPriority(true)
//   line 1266  EvaluateCameraZoomState -> ToggleCameraZoom(true) when advanced interaction goes on
//   line 1347  m_cameraZoomComponent.Activate(blendTime)          <- the camera hand-over
//              PushGameContext(UIGameContext.DeviceZoom)
//
// So the device carries its OWN CameraComponent, marked high priority, and activating it makes the
// device the active camera for a one-second blend. The port writes the HMD pose into the PLAYER's
// camera; it never reaches the device's, so the head stops steering the view. This is not the
// LocateCamera menu suppression -- that is a separate mechanism and is left alone here.
//
// WHAT THIS CHANGES, and it is exactly one call. Activate() is not made. Everything else in that
// method is reproduced verbatim: the two early-outs, the blackboard lookup, m_cameraZoomActive,
// the IsUIZoomDevice listener, and PushGameContext(DeviceZoom). The device screen therefore opens,
// takes input and closes exactly as it did -- only the camera stays with the player, so the head
// works in both eyes.
//
// WHAT YOU GIVE UP. The zoom. Vanilla slid the camera up to the screen for you; now you read the
// screen from where you are standing, which in VR is what leaning in is for. If that turns out to
// be the wrong trade, flip VRPortDeviceCamZoomWanted() back to true and the vanilla behaviour
// returns with no other change.
//
// WHY NOT SetHasUICameraZoom(false), which is the game's own switch for a device with no zoom: it
// is read by HasDirectActionsActive() and by ComputerControllerPS.IsInSleepMode(), so turning it
// off changes which actions a device offers and whether it sleeps, not just the camera. And
// AllowsUICameraZoomDynamicSwitch() gates writing it at all, so on a device that forbids the
// switch the change would silently not apply. One call is a smaller thing to be wrong about.
//
// Deactivate() is deliberately still called on the way out. If anything else in the game activated
// that camera -- a quest does, through QuestForceCameraZoom -- releasing it must keep working.

module CyberpunkVRPort.DeviceCam

// SetVRDeviceScreen is declared in vrport_device_natives.reds, NOT here, and that is not tidiness.
// This file is `module CyberpunkVRPort.DeviceCam`, and a native declared inside a module is looked up
// by its module-qualified name, so the game refuses the file outright while the plugin registers a
// plain global. The sibling has no module line, and a global func is callable from in here.

// true = vanilla: the device takes the camera and slides it to the screen.
// false = the camera stays with the player and the head keeps steering the view in both eyes.
public func VRPortDeviceCamZoomWanted() -> Bool {
  return false;
}

@replaceMethod(Device)
protected final func ToggleCameraZoom(toggle: Bool, opt instant: Bool) -> Void {
  let blackboard: ref<IBlackboard>;
  let blendTime: Float;
  if !IsDefined(this.m_cameraZoomComponent) {
    return;
  };
  if toggle && this.m_cameraZoomActive {
    return;
  };
  if !toggle && !this.m_cameraZoomActive {
    return;
  };
  blackboard = GameInstance.GetBlackboardSystem(this.GetGame()).GetLocalInstanced(GameInstance.GetPlayerSystem(this.GetGame()).GetLocalPlayerControlledGameObject().GetEntityID(), GetAllBlackboardDefs().PlayerStateMachine);
  if toggle {
    if instant {
      blendTime = 0.00;
    } else {
      blendTime = 1.00;
    };
    // THE ONE LINE THIS MOD EXISTS FOR. Vanilla calls Activate(blendTime) here, which makes the
    // device's own high-priority CameraComponent the active camera and freezes MAIN against the
    // head. Skipped unless the zoom is asked for.
    if VRPortDeviceCamZoomWanted() {
      this.m_cameraZoomComponent.Activate(blendTime);
    };
    this.m_cameraZoomActive = true;
    SetVRDeviceScreen(1);
    if IsDefined(blackboard) {
      this.m_ZoomUIListenerID = blackboard.RegisterListenerBool(GetAllBlackboardDefs().PlayerStateMachine.IsUIZoomDevice, this, n"OnIsUIZoomDeviceChange");
    };
    GameInstance.GetUISystem(this.GetGame()).PushGameContext(UIGameContext.DeviceZoom);
  } else {
    if instant {
      blendTime = 0.00;
    } else {
      blendTime = 0.50;
    };
    this.m_cameraZoomComponent.Deactivate(blendTime);
    this.m_cameraZoomActive = false;
    SetVRDeviceScreen(0);
    GameInstance.GetUISystem(this.GetGame()).PopGameContext(UIGameContext.DeviceZoom);
  };
}
