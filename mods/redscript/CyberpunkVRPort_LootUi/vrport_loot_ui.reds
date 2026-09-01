// CyberpunkVRPort -- the loot panel and its detailed description, laid out for the square VR view.
//
// WHAT IS WRONG WITHOUT THIS. The loot HUD is authored for a wide screen: the plate with the item name,
// the count and the "take" button sits a little below centre, and the detailed description hangs off to
// the RIGHT, where a 3072x3072 VR view puts it at the edge of vision. Asked for: the plate at 0.70, the
// description at 0.60, centred, and BELOW the take button.
//
// WHY THE DESCRIPTION IS REPARENTED AND NOT MOVED. Measured in the running game with the loot window
// open: the position of `tooltipsContainer` belongs to the native tooltip placement. A margin set to
// 0/0/0/0 came back as -871/-257 within two seconds with no input at all, and the value is recomputed
// from the widget the tooltip is attached to, so it varies per item and per hovered row. Anchor,
// anchorPoint, scale and translation are NOT touched. So this cannot be fixed on that widget, and it
// cannot be baked into the .inkwidget either -- the asset's values are overwritten at runtime.
//
// `inkWidget.Reparent` moves the description into the loot plate instead, which is a vertical panel: it
// then lands under the plate's own content by the panel's own layout, centred, with no coordinates to
// guess at.
//
// WHERE THE REPARENT HAS TO HAPPEN, and the first version of this file got it wrong. Doing it from the
// loot controller's update looked right and did nothing: the plate came out at 0.70 and the description
// kept its old parent. The reason is in the game's own tooltipsManager -- `ShowTooltips` begins with
// `HideTooltips()`, and that reparents EVERY tooltip back into `tooltipsContainer` (tooltipsManager.swift
// line 424) before setting the data. So anything done earlier in the frame is undone by the show itself.
// The last script step of a show is the controller's own `Show()`, so that is where it belongs -- and it
// also means the cleanup is the game's: when the tooltip hides, `HideTooltips` puts the widget back where
// it belongs, so nothing of ours has to remember the old parent (which redscript could not read anyway).
//
// THE SCALES MULTIPLY, which is why the description carries 0.86 rather than 0.60: it is a child of the
// plate now, and the plate is at 0.70. 0.86 * 0.70 = 0.60, the number approved on the picture.
//
// HOW THE RIGHT TOOLTIP IS IDENTIFIED. Not by position, not by "the first one found": the game tags the
// loot display context itself (looting.swift does `displayContext.AddTag(n"Looting")`) and
// ItemDisplayContextData carries HasTag. The HUD has a second widget with the same controller -- the
// active weapon slot's tooltip -- and it keeps its own place because its context has no such tag.
//
// WHY THE PLATE TRAVELS THROUGH THE PLAYER. Redscript has no access to a widget's parent and none to the
// ink system, so neither side can find the other by walking the tree. Both do have the player: the loot
// controller owns it, and the tooltip's data carries it (GetPlayerAsPuppet), so the plate is handed over
// in a field on PlayerPuppet -- the same way the scanner HUD module already passes its panels around.

@addField(PlayerPuppet) public let vrpLootPlate: wref<inkCompoundWidget>;

// Set on the tooltip controller itself, so `Show()` knows whether this instance is the loot one.
@addField(ItemTooltipCommonController) public let vrpIsLoot: Bool;

// The plate's scale, and the description's scale RELATIVE to it (0.86 * 0.70 = 0.60 on screen).
func VRPortLootPlateScale() -> Float {
  return 0.70;
}

func VRPortLootTipScale() -> Float {
  return 0.86;
}

// How far above its slot the description sits, set on the picture in four steps: 500 put it exactly in the
// middle of the view, 800 was still too low, 1600 was close, 1800 is what was approved.
func VRPortLootTipLift() -> Float {
  return 1800.0;
}

// And how far below its authored place the plate itself goes. The description is a CHILD of the plate, so
// this shift carries it along -- which is why the description's own translation adds it back below, and
// both numbers here mean what they say on screen rather than something relative.
func VRPortLootPlateDrop() -> Float {
  return 200.0;
}

// The height the description was measured at when that lift was approved (731x395). The lift is corrected
// by the difference so a longer item text does not push the block somewhere else.
func VRPortLootTipRefHeight() -> Float {
  return 395.0;
}

// The plate is this controller's own root widget: LootingGameController takes its LootingController with
// this.GetController(), so the game controller and the panel are one widget.
@wrapMethod(LootingGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VRPortLootUiApply(this);
  return result;
}

@wrapMethod(LootingGameController)
protected cb func OnUpdateData(value: Variant) -> Bool {
  let result: Bool = wrappedMethod(value);
  VRPortLootUiApply(this);
  return result;
}

public func VRPortLootUiApply(gc: ref<LootingGameController>) -> Void {
  let plate: wref<inkCompoundWidget> = gc.GetRootWidget() as inkCompoundWidget;
  if !IsDefined(plate) {
    return;
  };
  plate.SetRenderTransformPivot(new Vector2(0.5, 0.5));
  plate.SetScale(new Vector2(VRPortLootPlateScale(), VRPortLootPlateScale()));
  plate.SetTranslation(new Vector2(0.0, VRPortLootPlateDrop()));
  let pp: ref<PlayerPuppet> = gc.GetOwnerEntity() as PlayerPuppet;
  if IsDefined(pp) {
    pp.vrpLootPlate = plate;
  };
}

// Runs for every tooltip in the game, so the tag decides and the flag is cleared when it does not match:
// one controller instance is reused for different contexts.
@wrapMethod(ItemTooltipCommonController)
public func SetData(tooltipData: ref<ATooltipData>) -> Void {
  wrappedMethod(tooltipData);
  this.vrpIsLoot = IsDefined(this.m_displayContext) && this.m_displayContext.HasTag(n"Looting");
}

// The last script step of a show, i.e. after the manager's own HideTooltips() has put the widget back
// into its container. See the note at the top for why nothing earlier in the frame survives.
@wrapMethod(AGenericTooltipController)
public func Show() -> Void {
  wrappedMethod();
  let itemTooltip: ref<ItemTooltipCommonController> = this as ItemTooltipCommonController;
  if !IsDefined(itemTooltip) {
    return;
  };
  if !itemTooltip.vrpIsLoot {
    return;
  };
  let pp: ref<PlayerPuppet> = itemTooltip.m_player;
  if !IsDefined(pp) {
    return;
  };
  let plate: wref<inkCompoundWidget> = pp.vrpLootPlate;
  if !IsDefined(plate) {
    return;
  };
  let tip: wref<inkWidget> = this.GetRootWidget();
  if !IsDefined(tip) {
    return;
  };
  // An explicit index rather than -1: the first version passed -1 and the call did nothing at all.
  tip.Reparent(plate, plate.GetNumChildren());
  tip.SetAnchor(inkEAnchor.TopCenter);
  tip.SetAnchorPoint(new Vector2(0.5, 0.0));
  tip.SetRenderTransformPivot(new Vector2(0.5, 0.0));
  tip.SetScale(new Vector2(VRPortLootTipScale(), VRPortLootTipScale()));
  tip.SetHAlign(inkEHorizontalAlign.Center);

  // AND OUT OF THE PANEL'S LAYOUT, which is the whole reason the plate kept drifting upwards. The panel
  // stacks its children by their UNSCALED desired size, so the description added its full 395 to the
  // plate's height -- 275 became 690, and with the plate anchored to the bottom the item name, the count
  // and the take button all rose with it. Reported as "весь блок поднялся" and then as "уехало и
  // содержимое". A negative top margin of exactly that height removes the contribution: measured, the
  // plate's desired size went back to 1024x275 on the next layout pass, and its content stopped moving.
  //
  // The height is read rather than hardcoded, and the lift is corrected by the difference from the height
  // this was approved at, so the description stays in the same place on screen when an item's text is
  // longer than the one it was tuned on.
  let tipHeight: Float = tip.GetDesiredSize().Y;
  if tipHeight < 50.0 {
    tipHeight = VRPortLootTipRefHeight();
  };
  tip.SetMargin(new inkMargin(0.0, -tipHeight, 0.0, 0.0));
  tip.SetTranslation(new Vector2(0.0,
      -(VRPortLootTipLift() + VRPortLootPlateDrop()) + (tipHeight - VRPortLootTipRefHeight())));
}
