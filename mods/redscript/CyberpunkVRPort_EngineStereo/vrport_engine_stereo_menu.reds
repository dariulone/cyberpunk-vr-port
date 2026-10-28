// CyberpunkVRPort: frontend lifetime notifications.
// The native plugin keeps initial splashes mono until the last frontend closes,
// then leaves native stereo enabled. Menu lifetimes also select the flat panel.

native func VREngineStereoMenuShown() -> Void;
native func VREngineStereoMenuClosed() -> Void;

@wrapMethod(SingleplayerMenuGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VREngineStereoMenuShown();
  return result;
}

@wrapMethod(SingleplayerMenuGameController)
protected cb func OnUninitialize() -> Bool {
  VREngineStereoMenuClosed();
  return wrappedMethod();
}

// The startup Load Game screen replaces SingleplayerMenuGameController.
// Keep a full-window menu capture until this screen also closes.
@wrapMethod(LoadGameMenuGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VREngineStereoMenuShown();
  return result;
}

@wrapMethod(LoadGameMenuGameController)
protected cb func OnUninitialize() -> Bool {
  VREngineStereoMenuClosed();
  return wrappedMethod();
}
