// CyberpunkVRPort — phone HUD surfaces.
//
// These controllers are not children of the stable HUD regions exposed by the
// base game, so the CET HUD layout pass cannot address them directly. Reparent
// each transient surface into a named top-level slot under the existing VR HUD
// Root. CyberpunkVRPort_HUD/init.lua owns the slot geometry and reads it from
// hud_layout.ini, just like the other F10 HUD controls.

module CyberpunkVRPort.Hud

@if(ModuleExists("Codeware"))
func VrPhoneHudRoot() -> ref<inkCompoundWidget> {
  let inkSystem: ref<inkSystem> = GameInstance.GetInkSystem();
  if !IsDefined(inkSystem) {
    return null;
  }

  let layer: ref<inkLayerWrapper> = inkSystem.GetLayer(n"inkHUDLayer");
  if !IsDefined(layer) {
    return null;
  }

  let window: ref<inkCompoundWidget> = layer.GetVirtualWindow();
  if !IsDefined(window) {
    return null;
  }

  let root: ref<inkCompoundWidget> = window.GetWidgetByPathName(n"Root") as inkCompoundWidget;
  if IsDefined(root) {
    return root;
  }
  return window;
}

@if(ModuleExists("Codeware"))
func VrCreatePhoneHudSlot(
  parent: ref<inkCompoundWidget>,
  slotName: CName,
  x: Float,
  y: Float,
  scale: Float,
  index: Int32
) -> ref<inkCanvas> {
  // Keep the slot object stable across repeated calls/messages. The CET HUD
  // runtime caches top-level widget references and detects additions by child
  // count; replacing a slot with another of the same name and count would leave
  // that cache pointing at the retired widget.
  let existing: ref<inkCanvas> = parent.GetWidgetByPathName(slotName) as inkCanvas;
  if IsDefined(existing) {
    return existing;
  }

  let slot: ref<inkCanvas> = new inkCanvas();
  slot.SetName(slotName);
  slot.SetFitToContent(true);
  slot.SetInteractive(false);
  slot.SetAffectsLayoutWhenHidden(false);
  slot.SetAnchor(inkEAnchor.TopLeft);
  slot.SetAnchorPoint(Vector2(0.0, 0.0));
  slot.SetMargin(inkMargin(x, y, 0.0, 0.0));
  slot.SetScale(Vector2(scale, scale));
  slot.Reparent(parent, index);
  return slot;
}

@if(ModuleExists("Codeware"))
func VrAttachPhoneHudSurface(target: ref<inkCompoundWidget>, slot: ref<inkCanvas>) -> Void {
  if !IsDefined(target) || !IsDefined(slot) {
    return;
  }

  target.SetAnchor(inkEAnchor.TopLeft);
  target.SetAnchorPoint(Vector2(0.0, 0.0));
  target.SetMargin(inkMargin(0.0, 0.0, 0.0, 0.0));
  target.SetScale(Vector2(1.0, 1.0));
  target.Reparent(slot);
}

// Active caller portrait/status panel. 600/600/1.00 is headset-validated.
@if(ModuleExists("Codeware"))
@wrapMethod(HoloAudioCallLogicController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let parent: ref<inkCompoundWidget> = VrPhoneHudRoot();
  if IsDefined(parent) {
    let slot: ref<inkCanvas> = VrCreatePhoneHudSlot(
      parent,
      n"VRPortHolocall",
      600.0,
      600.0,
      1.0,
      63
    );
    VrAttachPhoneHudSurface(this.GetRootCompoundWidget(), slot);
  }
  return result;
}

// Ringing-call accept/decline prompt. 600/1050/1.00 is headset-validated.
@if(ModuleExists("Codeware"))
@wrapMethod(IncomingCallLogicController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let parent: ref<inkCompoundWidget> = VrPhoneHudRoot();
  if IsDefined(parent) {
    let slot: ref<inkCanvas> = VrCreatePhoneHudSlot(
      parent,
      n"VRPortIncomingCall",
      600.0,
      1050.0,
      1.0,
      64
    );
    VrAttachPhoneHudSurface(this.GetRootCompoundWidget(), slot);
  }
  return result;
}

// Brief incoming SMS card created by JournalNotificationQueue. The hook target
// was live-validated at 600/600/1.00; 600/800/0.90 is the refined default.
@if(ModuleExists("Codeware"))
@wrapMethod(MessengerNotification)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let parent: ref<inkCompoundWidget> = VrPhoneHudRoot();
  if IsDefined(parent) {
    let slot: ref<inkCanvas> = VrCreatePhoneHudSlot(
      parent,
      n"VRPortPhoneMessage",
      600.0,
      800.0,
      0.9,
      65
    );
    VrAttachPhoneHudSurface(this.GetRootCompoundWidget(), slot);
  }
  return result;
}

// Expanded READ MESSAGE modal. Live testing validated this controller and the
// top-level slot transform at 600/350/0.75. The integrated default lowers it
// and trims it slightly to 600/500/0.70; F10 keeps X/Y/Size independent.
@if(ModuleExists("Codeware"))
@wrapMethod(PhoneMessagePopupGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  let parent: ref<inkCompoundWidget> = VrPhoneHudRoot();
  if IsDefined(parent) {
    let slot: ref<inkCanvas> = VrCreatePhoneHudSlot(
      parent,
      n"VRPortMessageReader",
      600.0,
      500.0,
      0.7,
      66
    );
    VrAttachPhoneHudSurface(this.GetRootCompoundWidget(), slot);
  }
  return result;
}

// The complete Messages / Contacts browser is owned by the phone-dialer root.
// Live headset evidence validated one absolute margin and scale on this exact
// native root; no nested controller or child-reference transform is used.
@if(ModuleExists("Codeware"))
@addMethod(PhoneDialerLogicController)
public func VrApplyMessengerLayout(x: Float, y: Float, size: Float) -> Void {
  let rootWidget: ref<inkWidget> = this.GetRootWidget();
  let appliedScale: Float = size * 0.5;
  if IsDefined(rootWidget) {
    rootWidget.SetMargin(inkMargin(x, y, 0.0, 0.0));
    rootWidget.SetScale(Vector2(appliedScale, appliedScale));
  }
}

@if(ModuleExists("Codeware"))
@wrapMethod(PhoneDialerLogicController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  // Headset-validated phone-dialer root values. CET re-applies persisted F10
  // values immediately after initialization when its HUD runtime is available.
  this.VrApplyMessengerLayout(1150.0, 500.0, 1.5);
  return result;
}
