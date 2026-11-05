// CyberpunkVRPort -- the weapon state machine, reported to the plugin.
//
// Native bridge for the non-VRIK ADS muzzle stabilizer (see src/Anim/AdsMuzzleStabilizer.cpp).
// Ported from dabinn's TofuExpress (797a2a95).
//
// Three facts are only visible from here: the weapon's PlayerStateMachine value, where 5 is the real
// ranged Ready state and therefore the only pose worth learning a hip aim from; the ADS
// AimInTimeRemaining, which is authored by AimingStateEvents and marks the exact end of the raise
// being corrected; and whether the PublicSafeToReady raise is running, which must NOT be sampled as
// an aim.
//
// This file intentionally stays in the global namespace: the plugin registers plain global natives.

native func SetVRWeaponPoseState(weaponState: Int32, aimInRemaining: Float) -> Int32;
native func SetVRWeaponRaiseTransition(active: Int32) -> Int32;

// IS THE GAME SPRINTING, published from SprintEvents (in vrport_no_anims.reds) for the sprint gesture in
// the plugin. That gesture drives ToggleSprint_Button, a TOGGLE, so it has to know whether the toggle is
// currently on -- otherwise it presses again and turns its own sprint off, which is exactly what a held
// detent did: a stutter standing, and nothing but a stand-up from a crouch.
//
// DECLARED HERE, in the module-less file, for the reason this file's header already gives: the plugin
// registers plain global natives, and a native declared inside a module is looked up by its
// module-qualified name and refused.
native func SetVRSprintActive(active: Int32) -> Int32;

// The state of the weapon in hand: the right hand's, or with the left-handed controls a lone one in the left hand
// (CyberpunkVRPort_LeftHanded).
func VRPortPublishWeaponPoseState(scriptInterface: ref<StateGameScriptInterface>) -> Void {
  if !IsDefined(scriptInterface) || !IsDefined(scriptInterface.localBlackboard) { return; }
  SetVRWeaponPoseState(
    scriptInterface.localBlackboard.GetInt(VRLeftHandedWeapon.InHand(scriptInterface.executionOwner)
      ? GetAllBlackboardDefs().PlayerStateMachine.LeftWeapon : GetAllBlackboardDefs().PlayerStateMachine.RightWeapon),
    scriptInterface.localBlackboard.GetFloat(GetAllBlackboardDefs().PlayerStateMachine.AimInTimeRemaining)
  );
}

@wrapMethod(ReadyEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  SetVRWeaponRaiseTransition(0);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(SafeEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  SetVRWeaponRaiseTransition(0);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(PublicSafeEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  SetVRWeaponRaiseTransition(0);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(PublicSafeToReadyEvents)
protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  SetVRWeaponRaiseTransition(1);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(AimingStateEvents)
protected func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(AimingStateEvents)
protected final func OnUpdate(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(timeDelta, stateContext, scriptInterface);
  VRPortPublishWeaponPoseState(scriptInterface);
}

@wrapMethod(AimingStateEvents)
protected func OnExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  VRPortPublishWeaponPoseState(scriptInterface);
}
