// CyberpunkVRPort -- the VR menu's journal watch (moved from CyberpunkVRPort_Menu): a script class the journal calls back,
// with the player's field and getter; its detach on the player's OnDetach is Rust's in the Rust build
// (rust/crates/cvr-ffi/src/mechanics/reds_hooks.rs).

// CyberpunkVRPort — THE VR MENU'S EAR ON THE PHONE JOURNAL.
//
// The VR menu (mods/cet/CyberpunkVRPort_Menu) shows the phone's threads the way the game's own
// messenger does, and the messenger knows two things a script can only hear as journal callbacks:
//
//   State      a message or a reply option became active -- the thread changed, draw it again;
//   StateDelay a message was SCHEDULED, `delay` seconds out -- someone is typing. The game shows the
//              typing dots from half that delay (a new sender) until the message lands.
//
// CET cannot register a script callback, so this listens and keeps counters and the last scheduled
// message where Lua can read them every frame:
//
//   let w = Game.GetPlayer():VRMenuJournalWatch()   -- made and attached on the first call
//   w.updates, w.scheduled                           -- bump on each State / StateDelay of a phone entry
//   w.lastDelay, w.lastContact, w.lastConversation, w.lastFromPlayer, w.lastTyping
//
// The watcher lives on the player and stops listening when the player goes.

public class VRMenuJournalWatch extends IScriptable {
  public let updates: Int32;
  public let scheduled: Int32;
  public let lastDelay: Float;
  public let lastContact: Int32;
  public let lastConversation: Int32;
  public let lastFromPlayer: Bool;
  public let lastTyping: Bool;
  private let journal: wref<JournalManager>;

  public final func Attach(journal: ref<JournalManager>) -> Void {
    this.journal = journal;
    journal.RegisterScriptCallback(this, n"OnJournalState", gameJournalListenerType.State);
    journal.RegisterScriptCallback(this, n"OnJournalDelay", gameJournalListenerType.StateDelay);
  }

  public final func Detach() -> Void {
    if IsDefined(this.journal) {
      this.journal.UnregisterScriptCallback(this, n"OnJournalState");
      this.journal.UnregisterScriptCallback(this, n"OnJournalDelay");
      this.journal = null;
    };
  }

  protected cb func OnJournalState(entryHash: Uint32, className: CName, notifyOption: JournalNotifyOption, changeType: JournalChangeType) -> Bool {
    if Equals(className, n"gameJournalPhoneMessage") || Equals(className, n"gameJournalPhoneChoiceGroup") || Equals(className, n"gameJournalPhoneChoiceEntry") {
      this.updates += 1;
    };
  }

  protected cb func OnJournalDelay(entryHash: Uint32, className: CName, notifyOption: JournalNotifyOption, changeType: JournalChangeType, delay: Float) -> Bool {
    let entry: wref<JournalEntry>;
    let message: wref<JournalPhoneMessage>;
    if !Equals(className, n"gameJournalPhoneMessage") || !IsDefined(this.journal) {
      return false;
    };
    entry = this.journal.GetEntry(entryHash);
    message = entry as JournalPhoneMessage;
    this.lastDelay = delay;
    this.lastContact = MessengerUtils.FindContactHash(this.journal, entry);
    this.lastConversation = MessengerUtils.FindConversationHash(this.journal, entry);
    this.lastFromPlayer = IsDefined(message) && Equals(message.GetSender(), gameMessageSender.Player);
    this.lastTyping = IsDefined(message) && NotEquals(message.GetText(), "");
    this.scheduled += 1;
  }
}

@addField(PlayerPuppet) private let vrMenuJournalWatch: ref<VRMenuJournalWatch>;

@addMethod(PlayerPuppet) public func VRMenuJournalWatch() -> ref<VRMenuJournalWatch> {
  let journal: ref<JournalManager>;
  if !IsDefined(this.vrMenuJournalWatch) {
    journal = GameInstance.GetJournalManager(this.GetGame());
    if !IsDefined(journal) {
      return null;
    };
    this.vrMenuJournalWatch = new VRMenuJournalWatch();
    this.vrMenuJournalWatch.Attach(journal);
  };
  return this.vrMenuJournalWatch;
}
