public native func VRWorldMarkerAnchor(widget: ref<inkWidget>, position: Vector4) -> Void;
public native func VRWorldMarkerScreenPoint(position: Vector4) -> Vector4;
@addField(gameaiCyberwareBreachGameController) public let vrBreachCanvas: wref<inkWidget>;
@addField(gameaiCyberwareBreachGameController) public let vrBreachAnchor: Vector4;
@addField(gameaiCyberwareBreachGameController) public let vrBreachVisible: Bool;

@addMethod(gameaiCyberwareBreachGameController)
public func VRBreachGeometry() -> array<Vector4> {
  let result: array<Vector4>;
  let scale: Vector2;
  let size: Vector2;
  if IsDefined(this.vrBreachCanvas) {
    scale = this.vrBreachCanvas.GetScale(); size = this.vrBreachCanvas.GetSize();
    ArrayPush(result, Vector4(scale.X, scale.Y, size.X, size.Y));
    ArrayPush(result, this.vrBreachAnchor);
  };
  return result;
}

// THE BREACH DIAMOND HANGS ON ITS BREACH. The game hands the controller the breach's screen point
// (BreachUIParameters.position), projected with the plain camera system; the port lifts the indicator into the world
// about the point the UI's own camera puts the breach on (VRWorldMarkerAnchor). Under the engine's stereo the UI is
// laid out for one eye and that camera is narrowed to it (NDC across x2), the plain one is not: the diamond was
// placed at half its angle from the centre, and once lifted it slid with every head turn. It is placed at the UI
// camera's point instead (VRWorldMarkerScreenPoint), so the lifted diamond sits on the breach in both eyes. Without
// a fresh UI camera (w 0) the game's placement stays.
@wrapMethod(gameaiCyberwareBreachGameController)
protected cb func OnBreachDataChanged(value: Variant) -> Bool {
  let result = wrappedMethod(value);
  let parameters = FromVariant<ref<BreachUIParameters>>(value);
  // The HUD's own player, not GetPlayer(GetGameInstance()): the global GetGameInstance() is an invalid instance on a
  // game without the script frameworks, the player came back null and the diamond kept the game's placement.
  let player = this.GetPlayerControlledObject() as PlayerPuppet;
  let breach: wref<BreachComponent>;
  let finder: ref<BreachFinderComponent>;
  let world: Vector4;
  let screen: Vector4;
  let canvas = inkWidgetRef.Get(this.m_breachCanvasWRef);
  this.vrBreachVisible = IsDefined(parameters) && parameters.tracking && parameters.visible;
  if !IsDefined(canvas) { return result; };
  if IsDefined(parameters) && parameters.tracking && IsDefined(player) {
    finder = player.GetBreachFinderComponent();
    if IsDefined(finder) { breach = finder.GetTrackedBreachComponent(); };
    if IsDefined(breach) { world = breach.GetPosition(); world.W = 1.00; };
  };
  if world.W > 0.50 {
    screen = VRWorldMarkerScreenPoint(world);
    if screen.W > 0.50 {
      this.ChangeScreenPosition(Vector2(screen.X, screen.Y));
      this.ChangeFluff(Vector2(screen.X, screen.Y), parameters.radius);
    };
  };
  // Scale the complete indicator about its authored pivot, including text.
  // Set an absolute scale: repeated updates must never halve it again.
  canvas.SetScale(Vector2(0.50, 0.50));
  this.vrBreachCanvas = canvas;
  this.vrBreachAnchor = world;
  VRWorldMarkerAnchor(this.GetRootWidget(), world);
  return result;
}
