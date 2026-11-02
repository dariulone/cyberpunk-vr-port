// CyberpunkVRPort -- THE CYBERWARE LOOK RESTORED ON A SYSTEM THAT IS NO LONGER IN A GAME.
//
// CyberwareCustomizationSystem.OnRestored puts the saved cyberware look back on the player:
// RestoreCyberwareCustomization -> SetIndexForCustomizationSwitcher (native). The native first asks the
// player system whether the controlled puppet is the player, through the system's own game instance
// (ScriptableSystem +0x520), with no check. Measured on a return to the main menu (2026-11-02, two seconds
// after GotoMainMenu, the script frames read from the dump: OnRestored -> RestoreCyberwareCustomization):
// it ran on a system whose game instance was already gone, and read through a null pointer
// (Cyberpunk2077.exe +0x21962B4, in the native at +0x2196910).
//
// So both entry points do nothing while the system has no game instance. GameInstance.IsValid is exactly
// that pointer's test. A system of a game being played always has one, so a save's own restore is untouched;
// the next load restores the look on that game's own system.

@wrapMethod(CyberwareCustomizationSystem)
private func OnRestored(saveVersion: Int32, gameVersion: Int32) -> Void {
  if !GameInstance.IsValid(this.GetGameInstance()) {
    return;
  }
  wrappedMethod(saveVersion, gameVersion);
}

@wrapMethod(CyberwareCustomizationSystem)
public final func RestoreCyberwareCustomization(forcePlayerCustomization: Bool) -> Void {
  if !GameInstance.IsValid(this.GetGameInstance()) {
    return;
  }
  wrappedMethod(forcePlayerCustomization);
}
