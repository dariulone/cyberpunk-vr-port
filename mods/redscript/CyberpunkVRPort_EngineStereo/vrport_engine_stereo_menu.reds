// CyberpunkVRPort — ENGINE STEREO STARTS AT THE MAIN MENU.
//
// In engine stereo (xr_engine_stereo=1) the plugin keeps the engine mono until the main menu is first shown: splash
// screens, intro videos and the first loading are one picture in the headset. The main menu's controller tells the
// plugin when it is up; from then on the engine renders both eyes (src/Stereo/EngineStereoSdk.cpp). It also says when
// the main menu goes: under it the loading layer stays active, but only a loading screen without the main menu is
// drawn side by side (src/Stereo/EngineStereo.cpp LoadingSideBySide). Outside engine stereo the calls do nothing.
//
// THE MAIN MENU AS A PLACE (xr_lobby, src/Lobby/MenuLobby.cpp): the plugin shows the menu inside the main menu's own
// world, so the menu's 2D backdrop is hidden and only the menu itself stays over the world. The backdrop is two widgets
// of the menu's root: bganim, a video, and backgroundContainer, the pyramid shot with its light overlays and the fire
// and smoke videos. The container's contents are spawned after OnInitialize (2026-10-27: hidden one by one here, they
// all came up visible), so the container itself is hidden, which hides whatever arrives in it.

native func VREngineStereoMenuShown() -> Void;
native func VREngineStereoMenuClosed() -> Void;
native func VRLobbyActive() -> Bool;

func VRLobbyHideBackdrop(w: wref<inkWidget>, depth: Int32) -> Void {
  let video: wref<inkVideo>;
  let comp: wref<inkCompoundWidget>;
  let name: CName;
  let i: Int32;
  if !IsDefined(w) || depth > 4 {
    return;
  };
  name = w.GetName();
  if Equals(name, n"bganim") || Equals(name, n"backgroundContainer") {
    video = w as inkVideo;
    if IsDefined(video) {
      video.Stop();
    };
    w.SetVisible(false);
    return;
  };
  comp = w as inkCompoundWidget;
  if !IsDefined(comp) {
    return;
  };
  i = 0;
  while i < comp.GetNumChildren() {
    VRLobbyHideBackdrop(comp.GetWidgetByIndex(i), depth + 1);
    i += 1;
  };
}

@wrapMethod(SingleplayerMenuGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let top: wref<inkWidget>;
  VREngineStereoMenuShown();
  if VRLobbyActive() {
    // The backdrop is drawn by the main menu's window, not by this controller: start from the window's root.
    top = this.GetRootWidget();
    while IsDefined(top.GetParentWidget()) {
      top = top.GetParentWidget();
    };
    VRLobbyHideBackdrop(top, 0);
  };
  return result;
}

@wrapMethod(SingleplayerMenuGameController)
protected cb func OnUninitialize() -> Bool {
  VREngineStereoMenuClosed();
  return wrappedMethod();
}
