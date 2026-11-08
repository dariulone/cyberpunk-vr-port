// THE GAME'S OWN LIGHT AND STRONG ATTACKS DO NOT START WHILE THE TRACKED HANDS FIGHT.
//
// With arm cyberware or an ordinary melee weapon the strokes are physical (VRCyberwareAttack), and the pad
// hook swallows RT so the flat attack never starts from the controller trigger (Hooks/XInput.cpp,
// physicalCyberware). The keyboard and mouse reach the state machine by another road: a click in the
// simulator window started a flat attack on top of the tracked one, three phantom quick hits in the VR
// training (2026-11-06). Every flat melee attack state (combo, final, strong, sprint, crouch, jump,
// deflect counter) enters through these two questions, so answering them here covers every device.
// The plugin's VRPhysicalMelee is the pad hook's own condition (in gameplay, not head-aimed).
native func VRPhysicalMelee() -> Bool;

@wrapMethod(MeleeTransition)
public final static func WantsToLightAttack(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if IsDefined(scriptInterface.executionOwner as PlayerPuppet) && VRPhysicalMelee() { return false; };
  return wrappedMethod(stateContext, scriptInterface);
}

@wrapMethod(MeleeTransition)
public final static func WantsToStrongAttack(const stateContext: ref<StateContext>, const scriptInterface: ref<StateGameScriptInterface>) -> Bool {
  if IsDefined(scriptInterface.executionOwner as PlayerPuppet) && VRPhysicalMelee() { return false; };
  return wrappedMethod(stateContext, scriptInterface);
}
