// AIMING AND BLOCKING AT FULL SPEED (players, 2026-11-05: "with the left trigger held -- block / aim -- V must not
// slow down").
//
// The game walks V slowly (the AimWalk locomotion state, 1.4 m/s measured on the H10 save against 8.3 m/s) while
// AimWalkDecisions' enter condition is armed: aiming down the sights (UpperBody 6 -- the left trigger, LT), blocking
// with a melee weapon (Melee 2 -- LT again; the Blocking perk's IsNotSlowedDuringBlock already lifts that one), focus
// mode (the scanner) and a left-hand cyberware charging. In VR the gun is aimed by the hand and the left trigger is held
// through whole fights, so aiming and blocking no longer slow V: the condition is false for them. The scanner and a
// charging arm keep the game's slow walk, and the game's own rules for sprinting while aiming are not touched.
// ToStand asks this same condition, so an AimWalk already running ends the moment only aiming or blocking keeps it.
@wrapMethod(AimWalkDecisions)
protected const func EnterCondition(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  let bb: ref<IBlackboard> = scriptInterface.localBlackboard;
  if IsDefined(bb) {
    let defs = GetAllBlackboardDefs().PlayerStateMachine;
    let focus: Bool = bb.GetInt(defs.Vision) == 1;
    let charging: Bool = bb.GetInt(defs.LeftHandCyberware) == 5;
    if !focus && !charging {
      return false;
    };
  };
  return wrappedMethod(stateContext, scriptInterface);
}
