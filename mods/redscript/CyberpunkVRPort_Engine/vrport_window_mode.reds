// CyberpunkVRPort -- NO EXCLUSIVE FULLSCREEN IN THE SETTINGS MENU.
//
// The monitor shows the headset's picture in a window the port fits to it, and exclusive fullscreen is refused
// at the swapchain (Hooks/SwapChain.cpp: SetFullscreenState(TRUE) returns DXGI_ERROR_NOT_CURRENTLY_AVAILABLE);
// every launch starts borderless (Core/FirstLaunch.cpp ApplyLaunchWindowMode). Picked in the game's menu,
// Fullscreen still reached that refusal: reported after 0.2.0, the game minimised itself and the controls
// stopped -- the error is the one DXGI gives a window that is not in front, and the game waits for focus.
//
// So the window mode selector steps over Fullscreen: a press that lands on it goes on to the next mode the
// same way. Windowed and borderless stay as the game has them.

@wrapMethod(SettingsSelectorControllerListName)
private func ChangeValue(forward: Bool) -> Void {
  wrappedMethod(forward);
  if Equals(this.m_realValue.GetName(), n"WindowMode") && Equals(this.m_realValue.GetGroupPath(), n"/video/display") &&
      Equals(this.m_realValue.GetValue(), n"Fullscreen") {
    wrappedMethod(forward);
  }
}
