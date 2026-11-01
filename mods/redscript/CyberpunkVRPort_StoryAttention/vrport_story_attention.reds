// Manual Ofrenda clue transition adapted from Ajson44/cyberpunk-vr-port,
// commit 886aa90b395b75741e66823e3f9c0e333461bf6c (MIT).
// The same identity/state checks guard BOTH the input offer and the write.
@addMethod(GameObject)
public func CyberpunkVRPortManualFocusClueIndex() -> Int32 {
  let index: Int32;
  let i: Int32 = 0;
  let book: Bool = false;
  if !IsDefined(this.m_scanningComponent) { return -1; };
  index = this.m_scanningComponent.GetAvailableClueIndex();
  if index < 0
    || !this.m_scanningComponent.IsScanned()
    || this.m_scanningComponent.GetScanningProgress() < 1.0
    || !this.m_scanningComponent.IsAnyClueEnabled()
    || this.m_scanningComponent.IsClueInspected()
    || this.m_scanningComponent.IsActiveClueUsingAutoInspect() {
    return -1;
  };
  let components = VREntityComponents(this);
  while i < ArraySize(components) {
    if IsDefined(components[i]) && StrContains(NameToString(components[i].GetName()), "q110_haitian_book_a") {
      book = true;
      break;
    };
    i += 1;
  };
  if !book { return -1; };
  return index;
}

@addMethod(GameObject)
public func CyberpunkVRPortInspectManualFocusClue(clueIndex: Int32) -> Bool {
  if clueIndex < 0 || this.CyberpunkVRPortManualFocusClueIndex() != clueIndex { return false; };
  this.m_scanningComponent.SetClueState(clueIndex, true, true, true, false);
  return this.m_scanningComponent.IsClueInspected();
}
