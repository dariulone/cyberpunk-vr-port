// CyberpunkVRPort -- the VR menu's journal watch let go with the player (the watch itself: CyberpunkVRPort_Engine/vrport_menu_journal.reds).

@wrapMethod(PlayerPuppet) protected cb func OnDetach() -> Bool {
  if IsDefined(this.vrMenuJournalWatch) {
    this.vrMenuJournalWatch.Detach();
    this.vrMenuJournalWatch = null;
  };
  return wrappedMethod();
}
