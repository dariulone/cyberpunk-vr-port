// The button a prompt shows is the one the player presses. The port's XInput merge does not give the game the
// controllers' buttons one to one -- in a car physical A also presses X, the dialogue's confirm, and a held X presses
// B, the exit -- so the game's own prompt can name a button the player does not press. VRPromptPart maps a part of
// the controller icon atlas onto the part of the physical button that presses it (src/Natives/InputPrompts.cpp).
native func VRPromptPart(part: CName) -> CName;

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
