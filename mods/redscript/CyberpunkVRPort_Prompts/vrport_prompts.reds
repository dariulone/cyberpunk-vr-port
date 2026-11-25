// The button a prompt shows is the one the player presses. The port's XInput merge does not give the game the
// controllers' buttons one to one -- in a car physical A also presses X, the dialogue's confirm, and a held X presses
// B, the exit -- so the game's own prompt can name a button the player does not press. VRPromptPart maps a part of
// the controller icon atlas onto the part of the physical button that presses it (src/Natives/InputPrompts.cpp).
native func VRPromptPart(part: CName) -> CName;

// Keep ownership on the actual Ink widget. CET can return different Lua
// wrappers for the same widget, especially when the HUD recycles choice rows.
class VRCyberFingerCaption extends IScriptable {
  public let original: String;
  public let displayed: String;
  public let control: String;
  public let frame: Uint32;
}

@addField(inkText)
private let vrCFHint: ref<VRCyberFingerCaption>;

@addMethod(inkText)
public final func VRShowControlHint(control: String, frame: Uint32) -> Void {
  let current = this.GetText();
  if Equals(current, "") || Equals(control, "") { return; }
  if !IsDefined(this.vrCFHint) { this.vrCFHint = new VRCyberFingerCaption(); }
  this.vrCFHint.frame = frame;
  if Equals(current, this.vrCFHint.displayed) && Equals(control, this.vrCFHint.control) { return; }
  if NotEquals(current, this.vrCFHint.displayed) {
    this.vrCFHint.original = current;
  }
  let decorated = this.vrCFHint.original + "  [" + control + "]";
  if NotEquals(current, decorated) { this.SetText(decorated); }
  this.vrCFHint.displayed = decorated;
  this.vrCFHint.control = control;
}

@addMethod(inkText)
public final func VRRestoreControlHint(frame: Uint32) -> Void {
  if !IsDefined(this.vrCFHint) || this.vrCFHint.frame == frame { return; }
  if Equals(this.GetText(), this.vrCFHint.displayed) { this.SetText(this.vrCFHint.original); }
  this.vrCFHint = null;
}

@addField(interactionItemLogicController)
public let vrCFAction: CName;
@addField(interactionItemLogicController)
public let vrCFHold: Bool;

@wrapMethod(interactionItemLogicController)
public final func SetData(data: script_ref<InteractionChoiceData>, skillCheck: UIInteractionSkillCheck, isItemBroken: Bool) -> Void {
  wrappedMethod(data, skillCheck, isItemBroken);
  this.vrCFAction = Deref(data).inputAction;
  this.vrCFHold = Deref(data).isHoldAction;
}

// A dialogue line's button (and the scroll-and-confirm hint of the interaction list): its input view's icon part.
@wrapMethod(InteractionsInputView)
public final func SetInputButton(inputPartName: CName) -> Void {
  wrappedMethod(VRPromptPart(inputPartName));
}

@wrapMethod(InteractionsInputView)
public final func ResetInputButton() -> Void {
  wrappedMethod();
  let part: CName = VRPromptPart(this.m_DefaultInputPartName);
  if NotEquals(part, this.m_DefaultInputPartName) {
    inkImageRef.SetTexturePart(this.m_InputImage, part);
  }
}
