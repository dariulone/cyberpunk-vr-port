// The cyberware chord (left grip + L3, Hooks/CyberwareChord.cpp) presses the IconicCyberware key while the game window
// is in front. With the headset focused and the window behind, no key reaches the game, so the weapon mod's frame takes
// the chord here and runs what PlayerPuppet.OnAction does for that press: ActivateIconicCyberware, which keeps the
// item, cooldown, focus mode and phone-call checks of the game.
public native func VRCyberwareChordTake() -> Bool;

@addMethod(PlayerPuppet)
public func VRActivateIconicCyberware() -> Void {
  // In a vehicle with a cyberware mask the game ignores the press while the scanner is up (OnAction).
  if this.HasCWMask() && NotEquals(this.m_vehicleState, gamePSMVehicle.Default) &&
      GameInstance.GetBlackboardSystem(this.GetGame()).Get(GetAllBlackboardDefs().UI_Scanner).GetBool(GetAllBlackboardDefs().UI_Scanner.UIVisible) {
    return;
  };
  this.ActivateIconicCyberware();
}
