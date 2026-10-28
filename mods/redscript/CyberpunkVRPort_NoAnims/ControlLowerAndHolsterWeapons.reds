// CyberpunkVRPort -- FIREARMS STAY IN YOUR HANDS. Folded into this mod on 2026-08-20; everything below
// the second banner is Rivarez's original file, byte for byte, authorship banner included.
//
// WHY THE PORT NEEDS IT. The game lowers a drawn firearm after a while of no weapon input, and then
// unequips it entirely. The port's aiming and firing go through the controllers, so from the state
// machine's point of view the player is not using the weapon at all -- and the gun lowers itself in
// your hands, or vanishes. This file pins the raised/lowered state instead, raises on a fire or aim
// tap, and lowers on the holster button.
//
// HOW IT SITS WITH CyberpunkVRPort_WeaponUp. Ours was written FROM this file -- its own header says so
// -- because Rivarez's covers firearms only, behind an IsFirearms gate, and states that it does not
// work for melee. WeaponUp adds the melee half on the same two hook points. Checked rather than
// assumed, across all 26 wraps in this port:
//
//   * both wrap DefaultTransition.ShouldEnterSafe. Redscript CHAINS wraps rather than rejecting them,
//     and the two gates are mutually exclusive (a firearm in the right hand vs a melee weapon), so
//     only one of them ever answers for a given weapon.
//   * the @replaceMethod here is PublicSafeEvents.RequestWeaponUnequipNotifyUpperBody; ours is
//     MeleePublicSafeEvents.OnTick. Different classes, so no duplicate replace.
//   * ReadyEvents.OnTick and EquipmentBaseTransition.HandleWeaponEquip are wrapped by this file and by
//     vrport_no_anims.reds. Both are wraps; they chain.
//
// WHERE IT CAME FROM. A loose file in r6\scripts (v1.1, dated 2023-12-16) that the port silently
// depended on. The loose copy was DELETED in the same change -- with both present there would be two
// @replaceMethod on PublicSafeEvents.RequestWeaponUnequipNotifyUpperBody, which does not compile.
//
// THIRD-PARTY CODE. Written by Rivarez, not by this project. The banner and the contact address below
// are left exactly as they were; if the port is redistributed, that is the author to ask.
//
// ============================== ORIGINAL FILE FOLLOWS, UNMODIFIED ==============================

/* ============== Control lower and holster weapons for Cyberpunk 2077 ==============
                                      v.1.1
   ==================== Created by Rivarez (rivarez@list.ru) ========================

    M     M      A      DDDD   EEEEE     I  N    N     U    U   SSSS    SSSS   RRRR
    M M M M     A A     D   D  E         I  N N  N     U    U  S       S       R   R
    M  M  M    A   A    D   D  EEEE      I  N  N N     U    U   SSSS    SSSS   RRRR
    M     M   A A A A   D   D  E         I  N   NN     U    U       S       S  R  R
    M     M  A       A  DDDD   EEEEE     I  N    N      UUUU    SSSS    SSSS   R   R
*/


// 2.5 (dual wielding): each hand's weapon state machine keeps its own timestamp, RightTurnOffPublicSafeTimeStamp /
// LeftTurnOffPublicSafeTimeStamp (WeaponTransition.GetTurnOffPublicSafeTimeStamp), in the context both share. The
// 2.31 name TurnOffPublicSafeTimeStamp is no longer read by anything but this file: the game lowered the weapon on
// its own timer again and a manual lower never reached it. Everything here is about the right hand's firearm
// (IsFirearms reads AttachmentSlots.WeaponRight), so it is the right hand's timestamp.
public func VRPortSafeTimeStamp() -> CName {
  return n"RightTurnOffPublicSafeTimeStamp";
}
// true if the weapon is lowered (or should be)
public func IsWeaponLowered(stateContext: ref<StateContext>) -> Bool
{
  let safeTimeStamp: Float = stateContext.GetFloatParameter(VRPortSafeTimeStamp(), true);
  return safeTimeStamp == 0.0;
}

// true if the weapon is raised (or should be)
public func IsWeaponRaised(stateContext: ref<StateContext>) -> Bool
{
  let safeTimeStamp: Float = stateContext.GetFloatParameter(VRPortSafeTimeStamp(), true);
  return safeTimeStamp == 9999999.0;
}

/* The weapon is lowered (not holstered) once an interval has passed since TurnOffPublicSafeTimeStamp, so the
   variable gets a value from which that interval never runs out.
   DontSetIfLowered - leave it alone if the weapon was lowered by hand (with the button) */
public func SetWeaponRaised(stateContext: ref<StateContext>, DontSetIfLowered: Bool) -> Void
{
  if DontSetIfLowered && IsWeaponLowered(stateContext)
  {
    return;
  };
  stateContext.SetPermanentFloatParameter(VRPortSafeTimeStamp(), 9999999.0, true);
}

/* The other way round: TurnOffPublicSafeTimeStamp is set to zero so the interval since it always exceeds the
   one after which the weapon is lowered. That is what lowers the weapon. */
public func SetWeaponLowered(stateContext: ref<StateContext>) -> Void
{
  stateContext.SetPermanentFloatParameter(VRPortSafeTimeStamp(), 0.0, true);
}


// ============================================================================================================================================
// Every method that sets TurnOffPublicSafeTimeStamp is replaced and writes our own value for it, as described above
@wrapMethod(EquipmentBaseTransition)
  protected final const func HandleWeaponEquip(scriptInterface: ref<StateGameScriptInterface>, stateContext: ref<StateContext>, stateMachineInstanceData: StateMachineInstanceData, item: ItemID, isLeftWeapon: Bool) -> Void
  {
    wrappedMethod(scriptInterface, stateContext, stateMachineInstanceData, item, isLeftWeapon);
    SetWeaponRaised(stateContext, false);
  }

@wrapMethod(ReadyEvents)
  protected final func OnTick(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void
  {
    wrappedMethod(timeDelta, stateContext, scriptInterface);
    SetWeaponRaised(stateContext, true);
  }

@wrapMethod(PublicSafeToReadyEvents)
  protected final func OnExit(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void
  {
    wrappedMethod(stateContext, scriptInterface);
    SetWeaponRaised(stateContext, true);
  }

@wrapMethod(QuickMeleeEvents)
  protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void
  {
    wrappedMethod(stateContext, scriptInterface);
    SetWeaponRaised(stateContext, false);
  }

@wrapMethod(ShootEvents)
  protected final func OnEnter(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void
  {
    wrappedMethod(stateContext, scriptInterface);
    SetWeaponRaised(stateContext, false);
  }
// ============================================================================================================================================

// so the weapon is not holstered automatically after a while
@replaceMethod(PublicSafeEvents)
  private final func RequestWeaponUnequipNotifyUpperBody(stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Void {
  }

// true for a firearm
public func IsFirearms(scriptInterface: ref<StateGameScriptInterface>) -> Bool
{
  let weapon: ref<WeaponObject> = scriptInterface.GetTransactionSystem().GetItemInSlot(scriptInterface.executionOwner, t"AttachmentSlots.WeaponRight") as WeaponObject;
  return IsDefined(weapon) && weapon.IsRanged();
}

// raise and lower the weapon on a button press
@wrapMethod(UpperBodyEventsTransition)
  protected final func UpdateSwitchItem(timeDelta: Float, stateContext: ref<StateContext>, scriptInterface: ref<StateGameScriptInterface>) -> Bool
  {
    let player = scriptInterface.executionOwner as PlayerPuppet;
    if IsDefined(player) && player.vrLauncherConsumeSwitchRelease {
      this.ResetEquipVars(stateContext);
      if scriptInterface.GetActionValue(n"SwitchItem") <= 0.00 {
        player.vrLauncherConsumeSwitchRelease = false;
      };
      return false;
    };

    if IsFirearms(scriptInterface)
    {
      // raised by the fire or aim button
      let FireBtnTapped: Bool = scriptInterface.IsActionJustTapped(n"RangedAttack");
      let AimBtnTapped: Bool = scriptInterface.GetActionValue(n"CameraAim") > 0.00;
      if FireBtnTapped || AimBtnTapped
      {
        SetWeaponRaised(stateContext, false);
        return false;
      };

      // a raised weapon is lowered by the holster/draw button
      let holsterBtnTapped: Bool = scriptInterface.IsActionJustTapped(n"HolsterWeapon");
      if holsterBtnTapped && IsWeaponRaised(stateContext)
      {
        SetWeaponLowered(stateContext);
        return false;
      };
    };

    return wrappedMethod(timeDelta, stateContext, scriptInterface);
  }

/* Allows or forbids the weapon to leave the lowered state. This keeps the weapon from being raised automatically
   on entering certain zones, and raises it in dangerous zones */
@wrapMethod(PublicSafeDecisions)
  private final const func ShouldLeaveSafe(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool
  {
    if IsFirearms(scriptInterface)
    {
      if IsWeaponRaised(stateContext)  // the weapon should be raised
      {
        return true;                   // so it may leave the lowered state
      };
      if IsWeaponLowered(stateContext)  // the weapon should be lowered
      {
        return false;                   // so it may not leave the lowered state
      };
    };

    return wrappedMethod(stateContext, scriptInterface);
  }

/* Unlike the method above, this one allows or forbids the weapon to enter the lowered state. This keeps the weapon
   from being lowered automatically on entering these zones, and lowers it in dangerous zones */
@wrapMethod(DefaultTransition)
  protected final const func ShouldEnterSafe(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>, const opt maxDistanceSquared: Float) -> Bool
  {
    if IsFirearms(scriptInterface)
    {
      if IsWeaponRaised(stateContext)  // the weapon should be raised
      {
        return false;                  // so it may not be lowered
      };
      if IsWeaponLowered(stateContext)  // the weapon should be lowered
      {
        return true;                    // so it may enter the lowered state
      };
    };

    return wrappedMethod(stateContext, scriptInterface, maxDistanceSquared);
  }