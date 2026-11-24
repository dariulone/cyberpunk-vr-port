// Consume gameplay dodge/crouch only when the VR port reports a physical B press.
// The port's stick dash is a synthesised B and does not raise slot 157.
public class VRArmBInputGuard extends IScriptable {
  private let player: wref<PlayerPuppet>;
  private let dodgeCaptured: Bool;
  private let crouchCaptured: Bool;

  public final func Attach(player: ref<PlayerPuppet>) -> Void {
    this.player = player;
    player.RegisterInputListener(this, n"Dodge");
    player.RegisterInputListener(this, n"ToggleCrouch");
  }

  public final func Detach() -> Void {
    if IsDefined(this.player) { this.player.UnregisterInputListener(this); };
    this.player = null;
    this.dodgeCaptured = false;
    this.crouchCaptured = false;
  }

  protected cb func OnAction(action: ListenerAction, consumer: ListenerActionConsumer) -> Bool {
    let name: CName = ListenerAction.GetName(action);
    let rawDown = GetVRSharedSlot(157) > 0.5;
    if Equals(name, n"Dodge") {
      if rawDown { this.dodgeCaptured = true; };
      if this.dodgeCaptured {
        ListenerActionConsumer.Consume(consumer);
        if ListenerAction.IsButtonJustReleased(action) { this.dodgeCaptured = false; };
        return true;
      };
    } else {
      if Equals(name, n"ToggleCrouch") {
        if rawDown { this.crouchCaptured = true; };
        if this.crouchCaptured {
          ListenerActionConsumer.Consume(consumer);
          if ListenerAction.IsButtonJustReleased(action) { this.crouchCaptured = false; };
          return true;
        };
      };
    };
    return false;
  }
}

@addField(PlayerPuppet)
private let vrArmBInputGuard: ref<VRArmBInputGuard>;

@addMethod(PlayerPuppet)
public final func VRArmBSetInputGuard(enabled: Bool) -> Void {
  if enabled && !IsDefined(this.vrArmBInputGuard) {
    this.vrArmBInputGuard = new VRArmBInputGuard();
    this.vrArmBInputGuard.Attach(this);
  } else {
    if !enabled && IsDefined(this.vrArmBInputGuard) {
      this.vrArmBInputGuard.Detach();
      this.vrArmBInputGuard = null;
    };
  };
}

@wrapMethod(PlayerPuppet)
protected cb func OnDetach() -> Bool {
  if IsDefined(this.vrArmBInputGuard) {
    this.vrArmBInputGuard.Detach();
    this.vrArmBInputGuard = null;
  };
  return wrappedMethod();
}
