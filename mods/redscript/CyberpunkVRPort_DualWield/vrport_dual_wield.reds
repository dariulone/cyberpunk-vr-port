// CyberpunkVRPort -- dual wielding: each hand's trigger fires the gun in that hand.
//
// The game has ONE fire action for both guns and picks the gun in its ShootState machine: the shot
// alternates between the hands on every release, and both guns fire while aiming (or when both are
// full-auto). In VR each gun sits in its own hand under its own trigger, so the gun is picked by the
// trigger that is squeezed: the left alone fires the left gun, the right alone the right gun, both
// fire both.
//
// The plugin turns both triggers into the game's fire action (src/Hooks/XInput.cpp) and publishes
// kDualWieldActive (shared slot 165) for exactly as long as it does; the reload mod keeps that up
// while the game reports two guns in hand. Without the slot every decision below is the native one.
//
// This file intentionally stays in the global namespace: the plugin registers plain global natives.

native func GetVRSharedSlot(idx: Int32) -> Float;

public abstract class VRDualWieldTriggers {
  // include/Utils/SharedSlots.hpp: kDualWieldActive, 1 plus kDualWieldLeftFires (2) and kDualWieldRightFires (4).
  public static func Active() -> Bool {
    return GetVRSharedSlot(165) > 0.5;
  }

  // The trigger has broken, published by the merge before it lets the press reach the game: the hand is
  // chosen by then, and a finger merely resting on the other trigger chooses nothing.
  public static func Left() -> Bool {
    return (Cast<Int32>(GetVRSharedSlot(165)) / 2) % 2 == 1;
  }

  public static func Right() -> Bool {
    return (Cast<Int32>(GetVRSharedSlot(165)) / 4) % 2 == 1;
  }

  public static func Hurricane() -> Bool {
    return (Cast<Int32>(GetVRSharedSlot(165)) / 8) % 2 == 1;
  }
}

// Heywood Hurricane is ValentinosFrenzy in the game's state machine. A rapid
// pair of squeezes on EACH trigger requests it; all perk, reload, safe-state,
// ammo and equipment checks remain the game's own CanStartValentinosFrenzy.
// Listen for the merged fire action as well as the native QuickMelee binding.
@wrapMethod(ValentinosFrenzyInitDecisions)
protected final func OnAttach(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Void {
  wrappedMethod(stateContext, scriptInterface);
  scriptInterface.executionOwner.RegisterInputListener(this, n"RangedAttack");
}

@wrapMethod(ValentinosFrenzyInitDecisions)
protected cb func OnAction(action: ListenerAction, consumer: ListenerActionConsumer) -> Bool {
  wrappedMethod(action, consumer);
  // A deliberate squeeze can follow a full release of both hands. In that
  // case the previous release was evaluated before the cadence request existed;
  // waiting for this squeeze's release would miss a held last trigger entirely.
  if VRDualWieldTriggers.Active() && Equals(ListenerAction.GetName(action), n"RangedAttack")
      && ListenerAction.IsButtonJustPressed(action) {
    this.EnableOnEnterCondition(true);
  };
}

@wrapMethod(ValentinosFrenzyInitDecisions)
protected final const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let nativeRequest: Bool = wrappedMethod(stateContext, scriptInterface);
  return nativeRequest || (VRDualWieldTriggers.Active() && VRDualWieldTriggers.Hurricane()
    && this.CanStartValentinosFrenzy(stateContext, scriptInterface));
}

// The right gun hands the shot to the left one when only the left trigger is squeezed -- or when both
// are and the right gun cannot fire (reloading, empty), so a held right trigger never blocks the left.
@wrapMethod(RightWeaponDecisions)
protected const final func ToLeftWeapon(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRDualWieldTriggers.Active() && DefaultTransition.HasLeftWeaponEquipped(scriptInterface) {
    return VRDualWieldTriggers.Left()
      && (!VRDualWieldTriggers.Right() || !ShootTransition.IsRightWeaponReady(stateContext, scriptInterface));
  }
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(LeftWeaponDecisions)
protected const final func ToRightWeapon(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRDualWieldTriggers.Active() && DefaultTransition.HasRightWeaponEquipped(scriptInterface) {
    return VRDualWieldTriggers.Right()
      && (!VRDualWieldTriggers.Left() || !ShootTransition.IsLeftWeaponReady(stateContext, scriptInterface));
  }
  return wrappedMethod(stateContext, scriptInterface);
}

// Both guns fire together only while both triggers are squeezed and both guns can fire.
@wrapMethod(BothWeaponsDecisions)
protected const final func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRDualWieldTriggers.Active() {
    return VRDualWieldTriggers.Left() && VRDualWieldTriggers.Right()
      && DefaultTransition.HasRightWeaponEquipped(scriptInterface)
      && DefaultTransition.HasLeftWeaponEquipped(scriptInterface)
      && ShootTransition.IsRightWeaponReady(stateContext, scriptInterface)
      && ShootTransition.IsLeftWeaponReady(stateContext, scriptInterface);
  }
  return wrappedMethod(stateContext, scriptInterface);
}

// Out of "both" as soon as one trigger lets go. The base class also serves Valentino's Frenzy, whose
// states keep their native exits.
@wrapMethod(ExitToNextShootDecisionsBase)
protected const func ToLeftWeapon(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRDualWieldTriggers.Active() && IsDefined(this as BothWeaponsDecisions) {
    return VRDualWieldTriggers.Left() && !VRDualWieldTriggers.Right()
      && ShootTransition.IsLeftWeaponReady(stateContext, scriptInterface);
  }
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(ExitToNextShootDecisionsBase)
protected const func ToRightWeapon(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if VRDualWieldTriggers.Active() && IsDefined(this as BothWeaponsDecisions) {
    return VRDualWieldTriggers.Right() && !VRDualWieldTriggers.Left()
      && ShootTransition.IsRightWeaponReady(stateContext, scriptInterface);
  }
  return wrappedMethod(stateContext, scriptInterface);
}
