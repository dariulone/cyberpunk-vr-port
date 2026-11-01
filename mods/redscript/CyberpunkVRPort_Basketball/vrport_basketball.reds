// CyberpunkVRPort -- VR basketball. Physics half.
//
// The ball entity is vrbasketball\vr_basketball.ent from cyberpunkvrport.archive: one
// entPhysicalMeshComponent over a mesh whose rigid body was re-authored to real ball values
// (0.624 kg, shell inertia 2/3*m*r^2, damping 0.02, r = 0.1191 m, World Dynamic filter).
//
// What the engine CANNOT give us is bounce. Every one of the 83 materials in
// base\physics\physicsmaterials.physmatlib has restitution 0.2 or 0.05 -- rubber.physmat is 0.2 --
// and PhysX combines a contact pair's restitution (default eAVERAGE), so no per-material edit can
// reach a basketball's e ~= 0.85 without making the whole city bouncy. Instead the normal
// component is topped up here, once per contact, against an ENERGY target rather than a
// delta-v -- aiming at a delta-v always undershot, because by the time the correction lands the
// ball has both slowed and risen:
//
//     budget = (e_target*v_in)^2 / 2  -  g_n * s     s = travel along the normal since contact
//     J      = m * (sqrt(2*budget) - v_n)
//
// The tangential half is left entirely to PhysX: with the corrected shell inertia its
// slide->roll transition and spin are already right, which is where backspin comes from.
//
// e(v) is not a taste knob. Rubber is viscoelastic, so restitution falls with impact speed;
// the NBA rule (drop 1.80 m, rebound 1.20-1.40 m) pins e = 0.82..0.88 at v = 5.94 m/s.
//
// NOTE: intentionally NO `module` statement -- same reason as vrport_smoking.reds: the plugin
// registers its natives at global scope and a module would make redscript look them up qualified.

// ---- plugin natives (CyberpunkVR_Hands.dll) --------------------------------------------------
// The physics body's velocity, read and written directly.
//
// `entPhysicalBodyInterface` carries SetLinearVelocity / GetLinearVelocity / SetAngularVelocity /
// SetMass. None of them appear in engine_re/redscript_ref, and CET cannot call them because their
// RTTI entries have empty parameter descriptors ("requires 0 parameter(s)", result nil). The
// plugin invokes them through a hand-built CStack, which does not consult the descriptor.
// Validated against GetMass: it returns the mesh's authored 0.624 kg through the same path.
//
// Get* return W = 1 when the call went through and W = 0 when it did not, so an unavailable API
// can never be mistaken for a stationary ball.
native func VRBodyGetVel(body: ref<PhysicalBodyInterface>) -> Vector4;
native func VRBodyGetAngVel(body: ref<PhysicalBodyInterface>) -> Vector4;
native func VRBodySetVel(body: ref<PhysicalBodyInterface>, v: Vector4) -> Bool;
native func VRBodySetAngVel(body: ref<PhysicalBodyInterface>, v: Vector4) -> Bool;
// Simulation filter masks. Not used by the ball any more -- it stays fully collidable at all
// times, which is the whole point of steering it instead of teleporting it. Kept because it is
// how "does this bit control world collision" gets answered: with both masks zero the ball falls
// straight through the floor. (mask2 bit 7 is the world bit, measured the same way.)
native func VRBodySetSimMasks(body: ref<PhysicalBodyInterface>, mask1: Uint64, mask2: Uint64) -> Bool;

// The rendered view pose in world space. The FPP camera cannot be used for this: its world
// position ignores HMD positional tracking and lean, so a ball parented to it would swim.
native func VRViewWorldPos() -> Vector4;      // W = 1 valid
native func VRViewWorldRot() -> Quaternion;
// Palm centre (RightHandMiddle1 / LeftHandMiddle1) off the SOLVED avatar skeleton, in MODEL
// space. right: 1 = right, 0 = left. W = 1 once the skeleton has been solved.
//
// Model space is the player entity's own space, verified 2026-08-02 by measurement: the model
// camera reads z 1.600 while the real FPP camera sits 1.599 m above the player origin, and
// player + rotate(playerOrientation, camModel) reproduces the FPP camera to the millimetre.
// So: world = playerPos + rotate(playerOrientation, palmModel).
native func VRPalmModelPos(right: Int32) -> Vector4;
native func VRPalmModelRot(right: Int32) -> Quaternion;
native func VRCamModelPos() -> Vector4;
// The solved skeleton in model space, for the body collision below.
//   0 hips  1 spine  2 chest  3 neck  4 head
//   5 L thigh  6 L knee  7 L foot   8 R thigh  9 R knee  10 R foot
// W = 1 when the slot holds a real bone.
native func VRBodyBonePos(slot: Int32) -> Vector4;

// redscript has Quaternion.Transform (rotate a vector) but no quaternion product, and the world
// hand rotation is playerOrientation * handRotModel. Four lines of Hamilton product.
public class VRBallMath {
  public final static func QMul(a: Quaternion, b: Quaternion) -> Quaternion {
    let r: Quaternion;
    r.i = a.r * b.i + a.i * b.r + a.j * b.k - a.k * b.j;
    r.j = a.r * b.j - a.i * b.k + a.j * b.r + a.k * b.i;
    r.k = a.r * b.k + a.i * b.j - a.j * b.i + a.k * b.r;
    r.r = a.r * b.r - a.i * b.i - a.j * b.j - a.k * b.k;
    return r;
  }
}
// Controller poses in HMD-local OpenXR space (already used by the smoking module).
native func GetRightVRHandPos() -> Vector4;
native func GetLeftVRHandPos() -> Vector4;
native func GetRightVRHandValid() -> Bool;
native func GetLeftVRHandValid() -> Bool;

// ---- tunables that are physical constants, not preferences ----------------------------------
// Keep in sync with tools/make_ball_mesh.py, which writes the same mass into the mesh.
public class VRBallConst {
  public final static func Mass() -> Float { return 0.624; }        // kg, FIBA/NBA size 7
  public final static func Radius() -> Float { return 0.119124681; } // m, from the mesh collider
  // MEASURED in-game 2026-08-02, not assumed: released from 2.370 m above the contact level, the
  // ball rebounded 0.132 m -> e = sqrt(0.132/2.370) = 0.236. (rubber.physmat's authored 0.2 plus
  // whatever PhysX's contact-pair combine does with the ground material.)
  public final static func EngineRestitution() -> Float { return 0.236; }
  public final static func ContactSpeedThreshold() -> Float { return 1.0; } // m/s of |dv| = a hit

  // --- hands -----------------------------------------------------------------------------------
  // The VR avatar has NO physics collision: its hands are an animated mesh PhysX knows nothing
  // about, so a ball would pass straight through a palm. Both the grip and the dribble slap are
  // therefore resolved here, against a sphere we place at the controller.
  public final static func PalmRadius() -> Float { return 0.09; }
  // Grab when the palm sphere overlaps the ball with this much slack.
  public final static func GrabSlack() -> Float { return 0.10; }
  // Reach for picking a ball up off the ground without bending down: squeezing grip with the ball
  // anywhere inside this radius of the palm pulls it into the hand.
  public final static func GrabReach() -> Float { return 1.60; }
  // Restitution of a hand strike. A palm is softer than tarmac; this is what makes a dribble
  // die out instead of running away.
  public final static func PalmRestitution() -> Float { return 0.65; }

  // --- the ball's collision filter ------------------------------------------------------------
  // The ball's authored simulation filter is (376836, 5070), and the two words are measured to do
  // different jobs:
  //
  //   mask2  the WORLD. Clearing bit 7 of it drops the ball straight through the floor.
  //   mask1  CHARACTERS. Clearing it whole lets a carried ball reach 0.099 m from the player's
  //          axis instead of stalling at 0.212 m, with the floor still holding it -- repeated
  //          twice, 0.099 and 0.098.
  //
  // Individual bits of mask1 do nothing on their own (all five were tried), so the contact is
  // permitted by any of them and the word has to go as a whole.
  //
  // Nothing clears mask1 any more. It was a workaround for the player's hit capsule shipping at
  // radius 1.0 m, and that is fixed at the source instead -- vrport_player_hitbox.archive resizes
  // it to 0.20 m, so the ball is supposed to hit it. The measurements are kept because they are
  // what identifies each word, and because the next thing to need a filter change will want them.
  public final static func SimMask1() -> Uint64 { return 376836ul; }
  public final static func SimMask2() -> Uint64 { return 5070ul; }

  // --- tracking sanity ---------------------------------------------------------------------
  // MEASURED 2026-08-02: with the controller resting still, palm speed reads 0.0002-0.008 m/s,
  // and then one frame reported 57.7 m/s because the tracked pose jumped 1.3 m in a single frame.
  // Releasing on that frame threw the ball at 35 N.s. Two guards, both physical rather than
  // arbitrary: a hand cannot move a metre in 30 ms, and a human cannot throw a ball at 12 m/s
  // by wrist alone (an NBA three-pointer leaves at roughly 7-9 m/s).
  public final static func MaxPalmStep() -> Float { return 0.35; }   // m in one frame -> reject
  // Beyond this the ball is lost (one fell to z = -7070 during testing) -- recall it.
  public final static func LostDistance() -> Float { return 80.0; }



  // --- carrying ----------------------------------------------------------------------------
  // Proportional gain of the steer-to-hand controller, in 1/s. 25 closes a 4 cm error at 1 m/s,
  // which is quick enough to look attached without the ball overshooting at 30 fps.
  public final static func CarryGain() -> Float { return 25.0; }
  // Ceiling on the steering command: a grab from across the court flies in at this speed instead
  // of teleporting, and no tracking glitch can turn into a projectile.
  public final static func CarryMaxSpeed() -> Float { return 9.0; }
  // Same idea for orientation, in 1/s.
  public final static func CarrySpin() -> Float { return 12.0; }
  // How hard the hand may pull, in m/s^2. Holding the ball against gravity needs 9.8; throwing it
  // to 8 m/s over a 0.2 s swing needs about 40. 80 leaves headroom for both and is still far below
  // what it takes to push a 0.624 kg ball through a contact, which is the point: a wall, or the
  // player's own chest, wins the argument.
  public final static func CarryAccel() -> Float { return 80.0; }
  // How far the ball may fall behind the palm before the carry limits start scaling up. Inside this the pull
  // stays bounded, which is what keeps a held ball out of walls, out of the player and out of the hand's own
  // collider; beyond it there is nothing being held, so speed and acceleration grow in proportion to the miss
  // and the ball comes back at once instead of creeping in after a dash.
  public final static func CarryRecoverDist() -> Float { return 0.20; }

}

@addField(PlayerPuppet) let m_vrBallId: EntityID;
@addField(PlayerPuppet) let m_vrBallHas: Bool;
// The spawned ball has been seen where it was put (see VRBallTick).
@addField(PlayerPuppet) let m_vrBallPlaced: Bool;
@addField(PlayerPuppet) let m_vrBallPrevPos: Vector4;
@addField(PlayerPuppet) let m_vrBallPrevVel: Vector4;
@addField(PlayerPuppet) let m_vrBallHasPrev: Bool;
@addField(PlayerPuppet) let m_vrBallCorrect: Bool;      // bounce top-up on/off (off = raw engine)
@addField(PlayerPuppet) let m_vrBallLastImpact: Float;  // |v_n| of the last contact, for the log

// Measurement readouts. FTLog goes to the CET console window only -- it reaches no log file, so
// nothing above could be read back after the fact. These fields are the actual instrument: poll
// them over the CETBridge with live_eval, no console and no log parsing involved.
@addField(PlayerPuppet) let m_vrBallEIn: Float;         // closing speed of the last contact
@addField(PlayerPuppet) let m_vrBallEOut: Float;        // separating speed the ENGINE produced
@addField(PlayerPuppet) let m_vrBallEEngine: Float;     // vOut/vIn = the engine's real restitution
@addField(PlayerPuppet) let m_vrBallContacts: Int32;    // contacts seen, to prove the tick runs
@addField(PlayerPuppet) let m_vrBallTestDrop: Float;    // last drop test: fall height (m)
@addField(PlayerPuppet) let m_vrBallTestRebound: Float; // last drop test: rebound height (m)
@addField(PlayerPuppet) let m_vrBallTestE: Float;       // last drop test: sqrt(hOut/hIn)

// Deferred, ENERGY-targeted correction.
//
// At 30-35 fps the ball covers ~0.21 m per frame, more than its own radius, so the correction can
// only be applied a frame or more after the bounce -- by then the ball has already lost speed to
// gravity and gained height. Aiming at a fixed delta-v therefore always undershoots: measured
// 0.687, then 0.699 after deferring by one frame, against a target of 0.795 (a 0.735 m/s deficit,
// i.e. ~2.5 frames of gravity).
//
// So the target is an energy, not a velocity. On the frame the impulse is applied we know exactly
// where the ball is and how fast it is going, and solve for the speed that still reaches the
// intended apex. Any latency cancels out, at any frame rate.
@addField(PlayerPuppet) let m_vrBallPendOn: Bool;
@addField(PlayerPuppet) let m_vrBallPendVIn: Float;   // clean pre-impact closing speed
@addField(PlayerPuppet) let m_vrBallPendN: Vector4;   // contact normal
@addField(PlayerPuppet) let m_vrBallPendP: Vector4;   // where the contact happened

// Hands. 0 = free, 1 = held in the right hand, 2 = in the left.
@addField(PlayerPuppet) let m_vrBallHeld: Int32;
@addField(PlayerPuppet) let m_vrBallPalmR: Vector4;
@addField(PlayerPuppet) let m_vrBallPalmL: Vector4;
@addField(PlayerPuppet) let m_vrBallPalmVelR: Vector4;
@addField(PlayerPuppet) let m_vrBallPalmVelL: Vector4;
@addField(PlayerPuppet) let m_vrBallPalmHasPrev: Bool;
@addField(PlayerPuppet) let m_vrBallGripPrevR: Bool;
@addField(PlayerPuppet) let m_vrBallGripPrevL: Bool;
@addField(PlayerPuppet) let m_vrBallHandScale: Float;  // world scale; 1.0 unless the port rescales
// Wrist -> held ball's centre, in the HAND's own frame. The solver publishes the WRIST, and a ball held at
// the wrist looks like it is stuck to the forearm. Live-tunable per hand: VRBallSetPalmOffset(right, x, y, z).
//
// PER HAND, and that is the whole point: the
// left and right wrist frames are mirrored, so one shared offset lands on opposite sides of the two hands.
@addField(PlayerPuppet) let m_vrBallPalmOffX: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffY: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffZ: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffLX: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffLY: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffLZ: Float;
@addField(PlayerPuppet) let m_vrBallPalmOffInit: Bool;

// Two-frame release.
//
// MEASURED 2026-08-02: with the hand held perfectly still (ball gap 0.000 m for a second), the
// moment the body went dynamic it left at 7.5 m/s. Turning collision back on was ruled out by
// experiment -- 7.55 m/s with collision off, 7.42 m/s with it on. The velocity is already inside
// the actor: PhysX derives a kinematic body's velocity from how it was moved and does not clear
// it while the body sits still, and PhysicalBodyInterface exposes no way to read or zero it.
//
// So the release measures it instead: go dynamic, wait one frame, compute the velocity the ball
// ACTUALLY has from its own displacement, and apply m*(wanted - actual). Whatever the body was
// carrying is cancelled and the throw becomes exactly the hand's swing.
// A short history of the ball's OWN measured velocity, for the release.
//
// Not an estimator -- these are engine readings, one per frame. The window exists because of a human
// timing problem: the grip is let go AFTER the arm has begun to slow and curve, so the velocity at
// the exact instant of release is both slower and aimed elsewhere. Every VR toolkit compensates for
// this; Unity's XR grab interactable calls it "throw smoothing duration" and defaults to 0.25 s.
// The throw takes the FASTEST sample rather than an average, because averaging drags the peak of a
// snap throw back down towards the deceleration that follows it.
@addField(PlayerPuppet) let m_vrBallHist0: Vector4;
@addField(PlayerPuppet) let m_vrBallHist1: Vector4;
@addField(PlayerPuppet) let m_vrBallHist2: Vector4;
@addField(PlayerPuppet) let m_vrBallHist3: Vector4;
@addField(PlayerPuppet) let m_vrBallHist4: Vector4;
@addField(PlayerPuppet) let m_vrBallHist5: Vector4;

// Removed: a "speed can only grow by gravity" invariant used to sit here, rejecting any frame in
// which the ball sped up unexpectedly. It was a guard against the ejections that turned out to come
// from the player's own collision, and it never fired once the carry stopped teleporting the ball
// (rejects stayed 0 in every test). Worse, it silently ate legitimate impulses: a 3 N.s shot from
// the test harness moved the ball zero metres, because the invariant restored the previous velocity
// every frame. Nothing that quietly overrides real physics is worth keeping as insurance.


// ---- synthetic hand, for testing throws without a headset -------------------------------------
// Drives the grab/carry/release path from a scripted trajectory instead of the tracked hand, so a
// throw is exactly reproducible and can be measured over the bridge. Phase 0 = off, 1 = holding
// and moving, 2 = released (watching the flight).
@addField(PlayerPuppet) let m_vrBallFake: Bool;
@addField(PlayerPuppet) let m_vrBallFakePhase: Int32;
@addField(PlayerPuppet) let m_vrBallFakePos: Vector4;
@addField(PlayerPuppet) let m_vrBallFakeVel: Vector4;
@addField(PlayerPuppet) let m_vrBallFakeT: Float;
@addField(PlayerPuppet) let m_vrBallFakeHold: Float;
@addField(PlayerPuppet) let m_vrBallFakeRelPos: Vector4;  // where the ball was released
@addField(PlayerPuppet) let m_vrBallFakeFlight: Float;    // seconds since release
@addField(PlayerPuppet) let m_vrBallFakeRange: Float;     // horizontal distance travelled
@addField(PlayerPuppet) let m_vrBallFakeApex: Float;
@addField(PlayerPuppet) let m_vrBallFakeLandRange: Float;  // horizontal distance at FIRST ground contact
@addField(PlayerPuppet) let m_vrBallFakeLandT: Float;
@addField(PlayerPuppet) let m_vrBallFakeRelContacts: Int32;
@addField(PlayerPuppet) let m_vrBallFakeGap: Float;       // worst ball->hand gap while carried
@addField(PlayerPuppet) let m_vrBallLastThrow: Float;  // speed of the last release, for the report

// Drop test state. phase 0 = idle, 1 = falling, 2 = rising after the first hit.
@addField(PlayerPuppet) let m_vrBallTestPhase: Int32;
@addField(PlayerPuppet) let m_vrBallTestStartZ: Float;
@addField(PlayerPuppet) let m_vrBallTestHitZ: Float;
@addField(PlayerPuppet) let m_vrBallTestApexZ: Float;

// ---- spawn / despawn ------------------------------------------------------------------------
// Spawned through the plugin's spawn system (VRSpawnTemplate, src/Natives/SpawnSystem.cpp): a
// template-only world entity that TransactionSystem cannot place.
@addMethod(PlayerPuppet) public func VRBallEntity() -> ref<Entity> {
  if !this.m_vrBallHas { return null; }
  return VRSpawnedEntity(this.m_vrBallId);
}

@addMethod(PlayerPuppet) public func VRBallMeshComp() -> ref<PhysicalMeshComponent> {
  let ent = this.VRBallEntity();
  if !IsDefined(ent) { return null; }
  let comps = VREntityComponents(ent);
  let i = 0;
  while i < ArraySize(comps) {
    let mc = comps[i] as PhysicalMeshComponent;
    if IsDefined(mc) { return mc; }
    i += 1;
  }
  return null;
}

@addMethod(PlayerPuppet) public func VRBallBody() -> ref<PhysicalBodyInterface> {
  let ent = this.VRBallEntity();
  if !IsDefined(ent) { return null; }
  // Walk the components and cast, rather than FindComponentByType(n"..."): the cast can't be
  // wrong about which CName the native type registry expects.
  let comps = VREntityComponents(ent);
  let i = 0;
  while i < ArraySize(comps) {
    let mc = comps[i] as PhysicalMeshComponent;
    if IsDefined(mc) { return mc.CreatePhysicalBodyInterface(); }
    i += 1;
  }
  return null;
}

// Spawn `dist` metres ahead of the player and `up` metres above their feet.
@addMethod(PlayerPuppet) public func VRBallSpawnAt(dist: Float, up: Float) -> Void {
  if this.m_vrBallHas { this.VRBallDespawn(); }

  let base = this.GetWorldPosition();
  let fwd = this.GetWorldForward();
  let pos: Vector4;
  pos.X = base.X + fwd.X * dist;
  pos.Y = base.Y + fwd.Y * dist;
  pos.Z = base.Z + up;
  pos.W = 1.0;

  let eul: EulerAngles;
  eul.Pitch = 0.0;
  eul.Roll = 0.0;
  eul.Yaw = 0.0;

  this.m_vrBallId = VRSpawnTemplate("vrbasketball\\vr_basketball.ent", n"default", pos, EulerAngles.ToQuat(eul),
                                    [n"VRBasketball"]);
  this.m_vrBallHas = true;
  this.m_vrBallPlaced = false;
  this.m_vrBallHasPrev = false;
  this.m_vrBallTestPhase = 0;
  this.m_vrBallHeld = 0;
  this.m_vrBallPalmHasPrev = false;
  this.m_vrBallPendOn = false;
  if this.m_vrBallHandScale <= 0.01 { this.m_vrBallHandScale = 1.0; }
  FTLog(s"[VRBall] spawned at (\(pos.X), \(pos.Y), \(pos.Z))");
}

// The normal way to get a ball. Bounce correction is ON here (a Bool field defaults to false, and
// without it the ball is a beanbag -- the engine alone returns e ~= 0.23). VRBallSpawnAt is left
// untouched so VRBallDropTest can measure the raw engine behaviour.
@addMethod(PlayerPuppet) public func VRBallSpawn() -> Void {
  this.m_vrBallCorrect = true;
  this.VRBallSpawnAt(1.5, 1.2);
}

@addMethod(PlayerPuppet) public func VRBallDespawn() -> Void {
  if this.m_vrBallHas {
    let b = this.VRBallBody();
    if IsDefined(b) { VRBodySetSimMasks(b, VRBallConst.SimMask1(), VRBallConst.SimMask2()); }
    VRSpawnDelete(this.m_vrBallId);
    this.m_vrBallHas = false;
    this.m_vrBallHasPrev = false;
    this.m_vrBallTestPhase = 0;
    FTLog("[VRBall] despawned");
  }
}

@addMethod(PlayerPuppet) public func VRBallToggle() -> Void {
  if this.m_vrBallHas { this.VRBallDespawn(); } else { this.VRBallSpawn(); }
}

// ---- hands ------------------------------------------------------------------------------------
// World position of a palm. Built from the rendered view pose and the controller's HMD-local
// offset, with OpenXR axes mapped to the game's: (x, y, z)_xr -> (x, -z, y)_game.
// World rotation of the solved hand: playerOrientation * handRotModel.
@addMethod(PlayerPuppet) public func VRBallHandWorldRot(right: Bool) -> Quaternion {
  return VRBallMath.QMul(this.GetWorldOrientation(), VRPalmModelRot(right ? 1 : 0));
}

@addMethod(PlayerPuppet) public func VRBallPalmWorld(right: Bool) -> Vector4 {
  // Test harness takes over the right hand entirely.
  if this.m_vrBallFake && right { return this.m_vrBallFakePos; }

  let pm = VRPalmModelPos(right ? 1 : 0);
  if pm.W < 0.5 { return Vector4(0.0, 0.0, 0.0, 0.0); }
  // Model space is the player entity's space -- verified against the FPP camera, see the native's
  // comment. No view pose involved: VRViewWorldPos turned out to sit a metre below the real eye
  // point, which is what put the ball under the floor and off to the right.
  let base = this.GetWorldPosition();
  let q = this.GetWorldOrientation();
  let w = Quaternion.Transform(q, Vector4(pm.X, pm.Y, pm.Z, 0.0));
  // Wrist -> palm, rotated into the world by the hand's own orientation.
  if !this.m_vrBallPalmOffInit {
    this.m_vrBallPalmOffInit = true;
    // Where a held ball's centre goes, built from measurements rather than tuned by eye:
    //
    //     offset = flesh centre + palm normal * (hand half-thickness 0.020 + ball radius 0.119)
    //                           + palm axis * 0.085
    //
    // Palm axis and normal come from the rig (axis = wrist -> middle knuckle; normal = the part of
    // knuckle -> fingertip perpendicular to it, i.e. the side the fingers close towards). The hand's
    // half-thickness and flesh centre come from the skin: the fists mesh, 649 vertices, half-extents 0.042
    // across the palm and 0.020 through it, flesh centre +0.014/+0.005 off the wrist bone. 0.085 along the
    // axis puts the contact patch over the middle of palm-plus-fingers, which is where a ball this size
    // rests -- a hand reaches 0.16 from the wrist.
    //
    //     right hand   axis (-0.984, -0.016, -0.180)   normal (-0.179, -0.049, +0.983)
    //     left hand    axis (+0.998, -0.060, -0.011)   normal (-0.005, +0.097, -0.995)
    //
    // The two frames are MIRRORED -- the right palm faces +Z, the left -Z -- and this was once a single
    // offset (-0.08, -0.05, +0.12) used for both. On the right that was roughly right, which is why it
    // survived being tuned by hand; on the LEFT it put the ball 0.28 m away, on the back of the hand, and
    // wrist rotation swung it inward across the forearm. Seen exactly that way in the headset.
    //
    // Live tuning if it still wants nudging: VRBallSetPalmOffset(right, x, y, z).
    this.m_vrBallPalmOffX = -0.109;
    this.m_vrBallPalmOffY = -0.022;
    this.m_vrBallPalmOffZ = 0.116;
    this.m_vrBallPalmOffLX = 0.084;
    this.m_vrBallPalmOffLY = 0.022;
    this.m_vrBallPalmOffLZ = -0.139;
  }
  let offLocal = right
    ? Vector4(this.m_vrBallPalmOffX, this.m_vrBallPalmOffY, this.m_vrBallPalmOffZ, 0.0)
    : Vector4(this.m_vrBallPalmOffLX, this.m_vrBallPalmOffLY, this.m_vrBallPalmOffLZ, 0.0);
  let off = Quaternion.Transform(this.VRBallHandWorldRot(right), offLocal);
  return Vector4(base.X + w.X + off.X, base.Y + w.Y + off.Y, base.Z + w.Z + off.Z, 1.0);
}

// ---- automated throw test ---------------------------------------------------------------------
// Spawns a ball, grabs it with a synthetic hand, moves that hand at (vx, vy, vz) for `hold`
// seconds, releases, and records where the ball goes. No headset needed, and every run is
// identical, so a change can be judged by numbers instead of by feel.
//
//   Game.GetPlayer():VRBallTestThrow(0.0, 6.0, 3.0, 0.4)   -- 6 m/s forward, 3 m/s up, 0.4 s swing
//   Game.GetPlayer():VRBallTestResult()
//
// The hand starts at chest height in front of the player and moves in WORLD axes.
@addMethod(PlayerPuppet) public func VRBallTestThrow(vx: Float, vy: Float, vz: Float, hold: Float) -> Void {
  let base = this.GetWorldPosition();
  let fwd = this.GetWorldForward();
  let start = Vector4(base.X + fwd.X * 0.4, base.Y + fwd.Y * 0.4, base.Z + 1.3, 1.0);

  this.VRBallDespawn();
  this.m_vrBallFake = true;
  this.m_vrBallFakePos = start;
  this.m_vrBallFakeVel = Vector4(vx, vy, vz, 0.0);
  this.m_vrBallFakeT = 0.0;
  this.m_vrBallFakeHold = hold;
  this.m_vrBallFakePhase = 1;
  this.m_vrBallFakeFlight = 0.0;
  this.m_vrBallFakeRange = 0.0;
  this.m_vrBallFakeApex = -9999.0;
  this.m_vrBallFakeGap = 0.0;
  this.m_vrBallCorrect = true;

  // Spawned clear of the player and steered in, exactly like a grab from a distance.
  this.VRBallSpawnAt(1.4, 1.3);
  this.m_vrBallHeld = 1;
}

// Steers the synthetic hand mid-run. Without it every change of hand motion means respawning the
// ball a metre away, which starts the controller saturated and hides how it behaves near the hand.
@addMethod(PlayerPuppet) public func VRBallTestHandVel(vx: Float, vy: Float, vz: Float) -> Void {
  this.m_vrBallFakeVel = Vector4(vx, vy, vz, 0.0);
  this.m_vrBallFakeT = 0.0;
}

@addMethod(PlayerPuppet) public func VRBallTestStop() -> Void {
  this.m_vrBallFake = false;
  this.m_vrBallFakePhase = 0;
}

@addMethod(PlayerPuppet) public func VRBallTestResult() -> String {
  let body = this.VRBallBody();
  let bv = IsDefined(body) ? VRBodyGetVel(body) : Vector4(0.0, 0.0, 0.0, 0.0);
  return s"bodyVel=(\(bv.X), \(bv.Y), \(bv.Z)) valid=\(bv.W) phase=\(this.m_vrBallFakePhase) landRange=\(this.m_vrBallFakeLandRange) landT=\(this.m_vrBallFakeLandT) handV=(\(this.m_vrBallFakeVel.X), \(this.m_vrBallFakeVel.Y), \(this.m_vrBallFakeVel.Z)) worstCarryGap=\(this.m_vrBallFakeGap) | flight=\(this.m_vrBallFakeFlight)s range=\(this.m_vrBallFakeRange)m apexRise=\(this.m_vrBallFakeApex) lastThrow=\(this.m_vrBallLastThrow)";
}

// Advances the synthetic hand and records the flight. Called from the tick.
@addMethod(PlayerPuppet) private func VRBallFakeTick(dt: Float, ballPos: Vector4) -> Void {
  if !this.m_vrBallFake { return; }
  if this.m_vrBallFakePhase == 1 {
    this.m_vrBallFakePos = Vector4(this.m_vrBallFakePos.X + this.m_vrBallFakeVel.X * dt,
                                       this.m_vrBallFakePos.Y + this.m_vrBallFakeVel.Y * dt,
                                       this.m_vrBallFakePos.Z + this.m_vrBallFakeVel.Z * dt, 1.0);
    // How far the ball lags the hand -- this is what "the ball drifts instead of following" is.
    let gap = Vector4.Distance(ballPos, this.m_vrBallFakePos);
    if gap > this.m_vrBallFakeGap { this.m_vrBallFakeGap = gap; }
    this.m_vrBallFakeT += dt;
    if this.m_vrBallFakeT >= this.m_vrBallFakeHold {
      this.m_vrBallFakePhase = 2;
      this.m_vrBallFakeRelPos = ballPos;
      this.m_vrBallFakeRelContacts = this.m_vrBallContacts;
      this.m_vrBallFakeLandRange = -1.0;
    }
    return;
  }
  if this.m_vrBallFakePhase == 2 {
    this.m_vrBallFakeFlight += dt;
    let dx = ballPos.X - this.m_vrBallFakeRelPos.X;
    let dy = ballPos.Y - this.m_vrBallFakeRelPos.Y;
    this.m_vrBallFakeRange = SqrtF(dx * dx + dy * dy);
    let rise = ballPos.Z - this.m_vrBallFakeRelPos.Z;
    if rise > this.m_vrBallFakeApex { this.m_vrBallFakeApex = rise; }
    // Flight distance is only meaningful up to the FIRST bounce; past that the number is
    // mostly the ball rolling away, which is how a motionless hand scored 4.57 m.
    if this.m_vrBallFakeLandRange < 0.0 && this.m_vrBallContacts > this.m_vrBallFakeRelContacts {
      this.m_vrBallFakeLandRange = this.m_vrBallFakeRange;
      this.m_vrBallFakeLandT = this.m_vrBallFakeFlight;
    }
    if this.m_vrBallFakeFlight > 4.0 { this.m_vrBallFakePhase = 3; }
  }
}

// Live tuning, per hand -- `right` picks which one. It used to set one offset for both, which is how the
// left hand ended up 0.28 m out: tuning by eye on the right hand and trusting it to mirror.
@addMethod(PlayerPuppet) public func VRBallSetPalmOffset(right: Bool, x: Float, y: Float, z: Float) -> Void {
  this.m_vrBallPalmOffInit = true;
  if right {
    this.m_vrBallPalmOffX = x;
    this.m_vrBallPalmOffY = y;
    this.m_vrBallPalmOffZ = z;
  } else {
    this.m_vrBallPalmOffLX = x;
    this.m_vrBallPalmOffLY = y;
    this.m_vrBallPalmOffLZ = z;
  }
}

@addMethod(PlayerPuppet) private func VRBallUpdatePalms(dt: Float) -> Void {
  let r = this.VRBallPalmWorld(true);
  let l = this.VRBallPalmWorld(false);
  if this.m_vrBallPalmHasPrev && dt > 0.0 {
    let inv = 1.0 / dt;
    if r.W > 0.5 && this.m_vrBallPalmR.W > 0.5 {
      let step = Vector4.Distance(r, this.m_vrBallPalmR);
      // A step larger than a hand can physically make in one frame is a tracking jump, not motion.
      if step > VRBallConst.MaxPalmStep() {
        this.m_vrBallPalmVelR = Vector4(0.0, 0.0, 0.0, 0.0);
      } else {
        this.m_vrBallPalmVelR = Vector4((r.X - this.m_vrBallPalmR.X) * inv,
                                            (r.Y - this.m_vrBallPalmR.Y) * inv,
                                            (r.Z - this.m_vrBallPalmR.Z) * inv, 0.0);
      }
    }
    if l.W > 0.5 && this.m_vrBallPalmL.W > 0.5 {
      let stepL = Vector4.Distance(l, this.m_vrBallPalmL);
      if stepL > VRBallConst.MaxPalmStep() {
        this.m_vrBallPalmVelL = Vector4(0.0, 0.0, 0.0, 0.0);
      } else {
        this.m_vrBallPalmVelL = Vector4((l.X - this.m_vrBallPalmL.X) * inv,
                                            (l.Y - this.m_vrBallPalmL.Y) * inv,
                                            (l.Z - this.m_vrBallPalmL.Z) * inv, 0.0);
      }
    }
  }
  this.m_vrBallPalmR = r;
  this.m_vrBallPalmL = l;
  this.m_vrBallPalmHasPrev = true;
}

// NOT CALLED any more -- kept because the reasoning is worth having if throws ever need it again.
//
// Release velocity: the PEAK of the window, smoothed with its two neighbours.
//
// Taken from HIGGS (Skyrim VR, adamhynek), src/hand.cpp GetMaxVelocity: find the fastest sample,
// and unless it sits at either end of the window return the average of the three samples centred on
// it. The raw peak is one frame of tracking, so a single bad sample becomes the whole throw; the
// three-sample average around the peak keeps the peak's magnitude while dropping that sensitivity.
// Averaging the WHOLE window would not do -- it drags the peak of a snap throw back down into the
// deceleration that follows it, which is the reason the window exists at all.
@addMethod(PlayerPuppet) private func VRBallReleaseVelocity() -> Vector4 {
  let h: array<Vector4> = [this.m_vrBallHist0, this.m_vrBallHist1, this.m_vrBallHist2,
                           this.m_vrBallHist3, this.m_vrBallHist4, this.m_vrBallHist5];
  let peak = 0;
  let peakLen = -1.0;
  let i = 0;
  while i < ArraySize(h) {
    let len = Vector4.Length(h[i]);
    if len > peakLen { peakLen = len; peak = i; }
    i += 1;
  }

  let out = h[peak];
  if peak > 0 && peak < ArraySize(h) - 1 {
    let a = h[peak - 1];
    let b = h[peak + 1];
    out = Vector4((a.X + out.X + b.X) / 3.0, (a.Y + out.Y + b.Y) / 3.0, (a.Z + out.Z + b.Z) / 3.0, 0.0);
  }
  // The carry command holds the ball up against gravity; that part is not throw.
  return Vector4(out.X, out.Y, out.Z, 0.0);
}

// Grab / carry / release. Nothing here needs engine collision: the grab is an explicit test
// against the palm sphere, and the throw hands the ball the palm's own measured velocity.
@addMethod(PlayerPuppet) private func VRBallGripTick(gripR: Bool, gripL: Bool, ent: ref<Entity>, ballPos: Vector4, dt: Float) -> Void {
  let body = this.VRBallBody();
  if !IsDefined(body) { return; }

  if this.m_vrBallHeld != 0 {
    let stillHeld = this.m_vrBallHeld == 1 ? gripR : gripL;
    let palm = this.m_vrBallHeld == 1 ? this.m_vrBallPalmR : this.m_vrBallPalmL;
    if stillHeld && palm.W > 0.5 {
      // A carried ball stays a REAL DYNAMIC BODY and is STEERED to the hand. It is never made
      // kinematic and never teleported. Both of those were mistakes, and between them they caused
      // every symptom this module has had:
      //
      //   Kinematic means infinite mass. The character controller cannot push a kinematic actor,
      //   so walking into the carried ball shoved the PLAYER instead -- the ball pushed the player away.
      //   As a 0.624 kg dynamic body the ball simply yields, and the steering brings it back.
      //
      //   Teleporting places a collider wherever we say, including inside another one. A solver is
      //   entitled to resolve that overlap as violently as it likes, and this one does: MEASURED at
      //   exactly penetration/dt, up to 7.5 m/s in a single frame. Steering cannot create that
      //   state -- the solver stops the ball at surfaces, so a ball pressed against a wall stays
      //   against the wall instead of being buried in it.
      //
      // Velocity command = the hand's own velocity (feed-forward) plus a proportional pull toward
      // the palm. The feed-forward is what makes steady tracking lag-free: with it the correction
      // term only has to cover acceleration, not the whole motion.
      body.SetIsKinematic(false);
      // Invisible to scene QUERIES while carried, but still fully simulated.
      //
      // MEASURED 2026-08-02: this is the one switch that separates the ball from the player. A
      // PhysX character controller moves by sweeping, not by contacts, which is why clearing all
      // 13 bits of the simulation filter one at a time changed nothing (gap stayed 0.1737 for
      // every one of them) while this single call collapses it to 0.0399. It is what made the
      // ball block the player from walking forward and judder against their body.
      //
      // World collision is untouched: with queries off a thrown ball still bounces and settles at
      // z + 0.119, exactly its own radius above the ground.
      body.SetIsQueryable(false);

      let cur = ent.GetWorldPosition();
      let handV = this.m_vrBallHeld == 1 ? this.m_vrBallPalmVelR : this.m_vrBallPalmVelL;
      // Where the hand wants the ball to be going.
      let want = Vector4(handV.X + (palm.X - cur.X) * VRBallConst.CarryGain(),
                             handV.Y + (palm.Y - cur.Y) * VRBallConst.CarryGain(),
                             handV.Z + (palm.Z - cur.Z) * VRBallConst.CarryGain() + 9.81 * dt, 0.0);
      // Both limits below scale with how far the ball has fallen behind.
      //
      // Held within a hand's reach of the palm they stay as they were, and they have to: the bounded pull is
      // what keeps a carried ball from being driven into the player's own body, into a wall, or inside the
      // hand's own collider. But after a dash the hand is metres away in a single frame, and a 9 m/s cap
      // with 80 m/s^2 behind it turns the recovery into a slow creep -- seen exactly that way.
      //
      // Nothing is held at that distance, so there is no contact left to protect: the factor is 1 while the
      // miss is under CarryRecoverDist and grows in proportion beyond it, so a 2 m gap gets ten times the
      // speed and ten times the acceleration and closes in a couple of frames. Capped so a tracking glitch
      // that reports the palm on the other side of the map cannot turn into a bullet.
      let miss = SqrtF((palm.X - cur.X) * (palm.X - cur.X)
                     + (palm.Y - cur.Y) * (palm.Y - cur.Y)
                     + (palm.Z - cur.Z) * (palm.Z - cur.Z));
      let farK = miss > VRBallConst.CarryRecoverDist() ? miss / VRBallConst.CarryRecoverDist() : 1.0;
      if farK > 12.0 { farK = 12.0; }

      let sp = Vector4.Length(want);
      let maxSp = VRBallConst.CarryMaxSpeed() * farK;
      if sp > maxSp {
        let k = maxSp / sp;
        want = Vector4(want.X * k, want.Y * k, want.Z * k, 0.0);
      }

      // The hand PULLS with a bounded force; it does not dictate the velocity.
      //
      // This is the difference between an object you are holding and an object you are teleporting,
      // and it is what fixes the ball being flung away on release. Writing the velocity outright
      // beats any contact, so the ball was driven INSIDE the hand's own collider and sat there
      // overlapping it; the moment the driving stopped, the solver resolved that overlap at
      // penetration/dt -- MEASURED at 7.5 m/s. Adding a bounded impulse instead leaves the
      // solver's contact resolution intact, so the ball comes to rest AGAINST the hand rather than
      // inside it, and there is no overlap left to release.
      //
      // The same bound is why a held ball can no longer be pushed through a wall or through the
      // player's own chest: the contact zeroes the velocity each step and the hand can only add
      // CarryAccel * dt back.
      let v = VRBodyGetVel(body);
      if v.W > 0.5 {
        let dvx = want.X - v.X;
        let dvy = want.Y - v.Y;
        let dvz = want.Z - v.Z;
        let dvLen = SqrtF(dvx * dvx + dvy * dvy + dvz * dvz);
        let maxDv = VRBallConst.CarryAccel() * farK * dt;
        if dvLen > maxDv && dvLen > 0.0001 {
          let k = maxDv / dvLen;
          dvx *= k; dvy *= k; dvz *= k;
        }
        let m = VRBallConst.Mass();
        body.AddLinearImpulse(Vector4(dvx * m, dvy * m, dvz * m, 0.0), true);
      } else {
        // No velocity readback: fall back to writing it, which is worse but better than dropping
        // the ball on the floor.
        VRBodySetVel(body, want);
      }
      // Six frames is 0.18 s at 33 fps -- the same window the toolkits use, and about as long as
      // the gap between the peak of a throw and the finger actually letting go.
      let carried = VRBodyGetVel(body);
      this.m_vrBallHist5 = this.m_vrBallHist4;
      this.m_vrBallHist4 = this.m_vrBallHist3;
      this.m_vrBallHist3 = this.m_vrBallHist2;
      this.m_vrBallHist2 = this.m_vrBallHist1;
      this.m_vrBallHist1 = this.m_vrBallHist0;
      this.m_vrBallHist0 = carried;

      // Orientation is steered the same way, by angular velocity rather than by writing a
      // transform. q_err = q_target * conj(q_current); for a small error its vector part is half
      // the rotation axis times the angle, so 2*Kp*(i,j,k) is the angular velocity that closes it.
      let qc = ent.GetWorldOrientation();
      let qt = this.VRBallHandWorldRot(this.m_vrBallHeld == 1);
      let qInv: Quaternion;
      qInv.i = -qc.i; qInv.j = -qc.j; qInv.k = -qc.k; qInv.r = qc.r;
      let qe = VRBallMath.QMul(qt, qInv);
      let sgn = qe.r < 0.0 ? -1.0 : 1.0;      // take the short way round
      VRBodySetAngVel(body, Vector4(2.0 * VRBallConst.CarrySpin() * qe.i * sgn,
                                        2.0 * VRBallConst.CarrySpin() * qe.j * sgn,
                                        2.0 * VRBallConst.CarrySpin() * qe.k * sgn, 0.0));
      return;
    }
    // Released: stop steering. Nothing is written, computed or estimated.
    //
    // The ball has been a real dynamic body the whole time, tracking the hand under a velocity
    // controller, so at this instant its velocity ALREADY IS the throw -- measured, by the engine,
    // not inferred by this script. Every previous version of this path guessed at it from palm
    // positions and then argued with the result: an impulse of m*v that doubled the throw, a
    // measure-and-cancel pass that flattened it, a three-frame median to survive tracking
    // glitches. None of that has anything left to do.
    // NOTHING is written here, and that is the point -- see the paragraph above. The HIGGS-style
    // smoothed-peak version (VRBallReleaseVelocity, still below, no longer called) replaced this and was
    // reverted: it takes the fastest sample of a six-frame window instead of the velocity the
    // ball actually has, so a throw came out of a history buffer rather than out of the simulation.
    let v = VRBodyGetVel(body);
    this.m_vrBallLastThrow = Vector4.Length(v);
    this.m_vrBallHeld = 0;
    this.m_vrBallHasPrev = false;   // the carried trajectory is not a physical one
    this.m_vrBallPendOn = false;
    return;
  }

  // Free ball: a rising grip edge grabs it. GrabReach is generous on purpose -- a ball lying on
  // the ground should come to hand without crouching for it.
  let dR = this.m_vrBallPalmR.W > 0.5 ? Vector4.Distance(this.m_vrBallPalmR, ballPos) : 9999.0;
  let dL = this.m_vrBallPalmL.W > 0.5 ? Vector4.Distance(this.m_vrBallPalmL, ballPos) : 9999.0;
  let rGrab = gripR && !this.m_vrBallGripPrevR && dR < VRBallConst.GrabReach();
  let lGrab = gripL && !this.m_vrBallGripPrevL && dL < VRBallConst.GrabReach();
  if rGrab || lGrab {
    this.m_vrBallHeld = (rGrab && dR <= dL) ? 1 : (lGrab ? 2 : 1);
    this.m_vrBallPendOn = false;
    // Stop colliding with characters while the hand is driving it -- see SimMask1. Disabling the
    // player's hit collider outright also worked, but that would stop bullets registering on the
    // player for as long as they held a ball; this touches only the ball.
    // Nothing about the ball changes on a grab. It keeps its mass, its collision and its filter;
    // the only difference is that a controller now steers it. That is the whole point: an object
    // that stops being a physics object while you hold it is what produced the shoving and the
    // ejections. (For the record, the mesh component's ToggleCollision is a no-op anyway --
    // measured: carrying with it on and off ejected a released ball at 7.35 and 7.39 m/s.)
  }
}

// Dribble. With no collision on the avatar, a slap has to be resolved by hand: treat the palm as
// a sphere of effectively infinite mass and reflect the ball off it. In the palm's frame the ball
// arrives at (v_ball - v_palm) and leaves at -e times that, so in world space
//     v_ball' = (1+e)*v_palm_n - e*v_ball_n     along the palm->ball normal.
// Only fires when the palm is actually closing on the ball, so resting a hand on it does nothing.
@addMethod(PlayerPuppet) private func VRBallPalmStrike(palm: Vector4, palmVel: Vector4, ballPos: Vector4, ballVel: Vector4, body: ref<PhysicalBodyInterface>, dt: Float) -> Bool {
  if palm.W < 0.5 { return false; }
  let reach = VRBallConst.Radius() + VRBallConst.PalmRadius();

  // SWEPT, not sampled. Testing the distance once per frame is the same discrete test the engine does,
  // and it fails the same way: MEASURED, a ball crossing the forearm capsule at 8 m/s covers 0.267 m
  // between frames and never lands inside it -- through 3 shots of 3, at every radius short of a torso,
  // with useCCD on or off. A dribble is exactly that speed range, which is why the ball went through
  // the hands. So the test here is over the whole frame: where did the ball pass CLOSEST to the palm
  // while both were moving, not where did it happen to be when the frame was sampled.
  //
  // Both travel in a straight line over one frame, so in the palm's frame of reference the ball moves
  // from d0 to d0 + dRel, and the closest approach is a minimum of a quadratic -- exact, no stepping.
  let d0 = Vector4(ballPos.X - palm.X, ballPos.Y - palm.Y, ballPos.Z - palm.Z, 0.0);
  let rx = (ballVel.X - palmVel.X) * dt;
  let ry = (ballVel.Y - palmVel.Y) * dt;
  let rz = (ballVel.Z - palmVel.Z) * dt;
  // Rewind both to the start of the frame: the sample is the END of the step the solver just took.
  let sx = d0.X - rx;
  let sy = d0.Y - ry;
  let sz = d0.Z - rz;

  let rr = rx * rx + ry * ry + rz * rz;
  let t = 0.0;
  if rr > 0.000001 {
    t = -(sx * rx + sy * ry + sz * rz) / rr;
    if t < 0.0 { t = 0.0; }
    if t > 1.0 { t = 1.0; }
  }
  let cx = sx + rx * t;
  let cy = sy + ry * t;
  let cz = sz + rz * t;
  let dist = SqrtF(cx * cx + cy * cy + cz * cz);
  if dist > reach || dist < 0.001 { return false; }

  // The normal at closest approach, which is the direction the palm actually met the ball in -- using
  // the end-of-frame offset instead would tilt the bounce by however far the ball travelled past.
  let n = Vector4(cx / dist, cy / dist, cz / dist, 0.0);
  let vP = Vector4.Dot(palmVel, n);
  let vB = Vector4.Dot(ballVel, n);
  if vP <= vB { return false; }                     // not closing: no strike

  let e = VRBallConst.PalmRestitution();
  let need = (1.0 + e) * vP - e * vB - vB;
  if need <= 0.0 { return false; }
  let j = VRBallConst.Mass() * need;
  body.AddLinearImpulse(Vector4(n.X * j, n.Y * j, n.Z * j, 0.0), true);
  return true;
}

// The body used to be modelled here, as capsules on the solved skeleton. It is not any more:
// vrport_player_hitbox.archive gives the PLAYER ENTITY thirteen bone-bound capsule colliders --
// torso, chest, head, upper arms, forearms, hands, thighs, shins -- so the engine resolves it, for
// every dynamic object in the game rather than for this one ball. VRBallBoneReport stays because
// the published bones are still the diagnostic for whether the skeleton is reaching scripts.

// Reports every published bone, so "the capsules do nothing" can be told apart from "the bones
// never arrived" -- which is exactly what went wrong the first time this was tried.
@addMethod(PlayerPuppet) public func VRBallBoneReport() -> String {
  let out = "";
  let i = 0;
  while i < 11 {
    let m = VRBodyBonePos(i);
    out += s"\(i):\(m.W > 0.5 ? "ok" : "MISSING")(\(m.X), \(m.Y), \(m.Z)) ";
    i += 1;
  }
  return out;
}

// ---- the bounce model -------------------------------------------------------------------------
// Coefficient of restitution as a function of impact speed along the normal. Rubber loses more
// energy the harder it is hit; the linear fit is anchored on the NBA drop rule (see the header).
@addMethod(PlayerPuppet) public func VRBallRestitution(vn: Float) -> Float {
  let e = 0.88 - 0.0125 * vn;
  if e > 0.86 { e = 0.86; }
  if e < 0.72 { e = 0.72; }
  return e;
}

// ---- the frame-accurate probe -------------------------------------------------------------------
// Every measurement of a bounce today came out noisy or plain wrong for one reason: an impact lasts a
// single frame (33 ms) while the outside sampler round-trips in ~60 ms, so it lands either side of the
// event and reports whatever it happens to catch -- a later floor bounce, a terminal fall, a ball that
// rolled off. Recording from inside the frame loop removes that whole class of error.
//
// The signal is the change in velocity across one frame with gravity taken out: free fall accounts for
// exactly 9.81*dt, so anything beyond that came from a contact. The largest such jump in a run IS the
// impact, and the velocities either side of it give impact and rebound speed exactly.
@addField(PlayerPuppet) let m_vrProbeOn: Bool;
@addField(PlayerPuppet) let m_vrProbeHasPrev: Bool;
@addField(PlayerPuppet) let m_vrProbeVPrev: Vector4;
@addField(PlayerPuppet) let m_vrProbeJump: Float;
@addField(PlayerPuppet) let m_vrProbeBefore: Vector4;
@addField(PlayerPuppet) let m_vrProbeAfter: Vector4;
@addField(PlayerPuppet) let m_vrProbePeak: Float;
@addField(PlayerPuppet) let m_vrProbeFrames: Int32;
@addField(PlayerPuppet) let m_vrProbeContacts: Int32;

@addMethod(PlayerPuppet) public func VRBallProbeStart() -> Void {
  this.m_vrProbeOn = true;
  this.m_vrProbeHasPrev = false;
  this.m_vrProbeJump = 0.0;
  this.m_vrProbePeak = 0.0;
  this.m_vrProbeFrames = 0;
  this.m_vrProbeContacts = 0;
  this.m_vrProbeBefore = Vector4(0.0, 0.0, 0.0, 0.0);
  this.m_vrProbeAfter = Vector4(0.0, 0.0, 0.0, 0.0);
}

@addMethod(PlayerPuppet) public func VRBallProbeStop() -> Void {
  this.m_vrProbeOn = false;
}

@addMethod(PlayerPuppet) private func VRBallProbeTick(dt: Float) -> Void {
  if !this.m_vrProbeOn || dt <= 0.0 { return; }
  let body = this.VRBallBody();
  if !IsDefined(body) { return; }
  let v = VRBodyGetVel(body);
  if v.W < 0.5 { return; }

  this.m_vrProbeFrames += 1;
  let sp = Vector4.Length(v);
  if sp > this.m_vrProbePeak { this.m_vrProbePeak = sp; }

  if this.m_vrProbeHasPrev {
    let dvx = v.X - this.m_vrProbeVPrev.X;
    let dvy = v.Y - this.m_vrProbeVPrev.Y;
    let dvz = v.Z - this.m_vrProbeVPrev.Z + 9.81 * dt;   // take out the frame's free fall
    let jump = SqrtF(dvx * dvx + dvy * dvy + dvz * dvz);
    if jump > 0.20 { this.m_vrProbeContacts += 1; }
    if jump > this.m_vrProbeJump {
      this.m_vrProbeJump = jump;
      this.m_vrProbeBefore = this.m_vrProbeVPrev;
      this.m_vrProbeAfter = v;
    }
  }
  this.m_vrProbeVPrev = v;
  this.m_vrProbeHasPrev = true;
}

// Impact and rebound along the contact normal, taken as the direction of the jump -- for a bounce that
// IS the surface normal, and restitution is defined against it. Reporting speeds along the approach
// axis instead would count sideways deflection as loss.
@addMethod(PlayerPuppet) public func VRBallProbeReport() -> String {
  let a = this.m_vrProbeBefore;
  let b = this.m_vrProbeAfter;
  let j = this.m_vrProbeJump;
  let vIn = 0.0;
  let vOut = 0.0;
  if j > 0.0001 {
    let nx = (b.X - a.X) / j;
    let ny = (b.Y - a.Y) / j;
    let nz = (b.Z - a.Z) / j;
    vIn = -(a.X * nx + a.Y * ny + a.Z * nz);
    vOut = b.X * nx + b.Y * ny + b.Z * nz;
  }
  let e = vIn > 0.05 ? vOut / vIn : 0.0;
  return s"frames=\(this.m_vrProbeFrames) contacts=\(this.m_vrProbeContacts) peak=\(this.m_vrProbePeak) jump=\(j) impact=\(vIn) rebound=\(vOut) e=\(e)";
}

// Per-frame driver. CET feeds the real frame delta.
@addMethod(PlayerPuppet) public func VRBallTick(dt: Float, gripR: Bool, gripL: Bool) -> Void {
  if !this.m_vrBallHas || dt <= 0.0 { return; }
  let ent = this.VRBallEntity();
  if !IsDefined(ent) { return; }


  let p0 = ent.GetWorldPosition();
  // A ball the population system has just entered reads the world origin until its transform lands, a tick or
  // two later. Taken for real, that origin is "lost" (the recall below then spawns another, which reads the origin
  // in turn: a new ball every tick) and a bogus velocity. Nothing of it counts until it is where it was put.
  if !this.m_vrBallPlaced {
    if Vector4.Distance(p0, this.GetWorldPosition()) > VRBallConst.LostDistance() { return; }
    this.m_vrBallPlaced = true;
  }
  this.VRBallFakeTick(dt, p0);
  // The synthetic hand owns the right grip while a test runs.
  let gR = this.m_vrBallFake ? (this.m_vrBallFakePhase == 1) : gripR;
  let gL = this.m_vrBallFake ? false : gripL;

  this.VRBallUpdatePalms(dt);
  this.VRBallProbeTick(dt);

  // Invisible to scene QUERIES for its whole life, held or free.
  //
  // A PhysX character controller moves by sweeping queries rather than by contacts, so a ball the
  // controller can see is an obstacle it has to get past -- and being 0.24 m across, under the step
  // height, what it does is CLIMB it. The player walks up and over the ball instead of into it.
  // Queries off and the controller never learns the ball is there.
  //
  // SIMULATION is deliberately untouched, and that is the whole point of using queries for this: the
  // sixteen bone capsules and the world still collide with the ball as before, so a shin can still kick
  // it and it still bounces off the floor. Only the movement system stops seeing it.
  let qb = this.VRBallBody();
  if IsDefined(qb) { qb.SetIsQueryable(false); }


  let p = ent.GetWorldPosition();


  // Recall a lost ball. One got thrown by a tracking glitch and was still falling past z = -7070
  // when we looked; without this it is gone for the rest of the session.
  if this.m_vrBallHeld == 0 {
    let away = Vector4.Distance(p, this.GetWorldPosition());
    if away > VRBallConst.LostDistance() {
      this.VRBallSpawnAt(1.5, 1.2);
      return;
    }
  }

  // Held: the transform is driven, not simulated, so none of the bounce bookkeeping applies.
  if this.m_vrBallHeld != 0 {
    this.VRBallGripTick(gR, gL, ent, p, dt);
    this.m_vrBallGripPrevR = gR;
    this.m_vrBallGripPrevL = gL;
    this.m_vrBallPrevPos = p;
    this.m_vrBallHasPrev = false;
    return;
  }

  if !this.m_vrBallHasPrev {
    this.m_vrBallPrevPos = p;
    this.m_vrBallPrevVel = Vector4(0.0, 0.0, 0.0, 0.0);
    this.m_vrBallHasPrev = true;
    return;
  }

  let inv = 1.0 / dt;
  let v = Vector4((p.X - this.m_vrBallPrevPos.X) * inv,
                      (p.Y - this.m_vrBallPrevPos.Y) * inv,
                      (p.Z - this.m_vrBallPrevPos.Z) * inv, 0.0);
  let dv = Vector4(v.X - this.m_vrBallPrevVel.X,
                       v.Y - this.m_vrBallPrevVel.Y,
                       v.Z - this.m_vrBallPrevVel.Z, 0.0);

  // Apply a pending correction now that the ball has separated and v is a clean reading again.
  // Energy bookkeeping along the contact normal: the ball must still reach the apex that
  // e_target * v_in would have given it from the contact point.
  if this.m_vrBallPendOn {
    let n = this.m_vrBallPendN;
    let s = (p.X - this.m_vrBallPendP.X) * n.X
          + (p.Y - this.m_vrBallPendP.Y) * n.Y
          + (p.Z - this.m_vrBallPendP.Z) * n.Z;      // height gained along the normal
    let gN = 9.81 * n.Z;                              // gravity's component along the normal
    let vN = Vector4.Dot(v, n);
    let vTarget = this.VRBallRestitution(this.m_vrBallPendVIn) * this.m_vrBallPendVIn;
    let budget = vTarget * vTarget * 0.5 - gN * s;    // kinetic energy per kg still required
    if budget > 0.0 {
      let vWant = SqrtF(2.0 * budget);
      let need = vWant - vN;
      if need > 0.0 {
        let body = this.VRBallBody();
        if IsDefined(body) {
          let j = VRBallConst.Mass() * need;
          body.AddLinearImpulse(Vector4(n.X * j, n.Y * j, n.Z * j, 0.0), true);
                }
      }
    }
    this.m_vrBallPendOn = false;
  }

  // A contact is the only thing that changes velocity faster than gravity can (g*dt ~ 0.3 m/s
  // at 33 Hz). The velocity step IS the contact impulse, so its direction is the contact normal.
  if Vector4.Length(dv) > VRBallConst.ContactSpeedThreshold() {
    let n = Vector4.Normalize(dv);
    let vInN = -Vector4.Dot(this.m_vrBallPrevVel, n);   // closing speed, > 0 approaching
    let vOutN = Vector4.Dot(v, n);                      // separating speed after the engine bounce
    if vInN > 0.0 && vOutN >= 0.0 {
      this.m_vrBallLastImpact = vInN;
      this.VRBallOnContact(vInN, vOutN, n, p, dt);
    }
  }

  // NO script palm strike any more. The engine does hands properly now.
  //
  // VRBallPalmStrike is kept below for reference but is no longer called. It was the only arm collision
  // that worked while the sixteen bone capsules were inert, and it worked by slapping the ball whenever it
  // came within Radius + PalmRadius = 0.209 m of the PALM POINT -- and that point sits 0.153 m in FRONT of
  // the wrist, because it is the place a held ball's centre goes, not the hand. So the strike zone reached
  // 0.36 m from the wrist, more than twice the length of the hand, and it swung through an arc as the wrist
  // rotated. Reported from the headset as exactly that: "turn the left wrist inward and the collision goes
  // much further out than the forearm actually is".
  //
  // What replaced it is measured: the capsules now stop a ball fired at either hand, either forearm, the
  // torso and the legs, 4 shots of 4 each, and they sit on their bones to within 0.0001 m with their axes
  // 0.0 degrees off the bone. A 0.209 m sphere hanging in front of the palm can only make that worse.
  //
  // The GRAB still uses the palm point, and should: that offset is correct for where a held ball goes.
  this.VRBallGripTick(gR, gL, ent, p, dt);
  this.m_vrBallGripPrevR = gR;
  this.m_vrBallGripPrevL = gL;

  this.m_vrBallPrevPos = p;
  this.m_vrBallPrevVel = v;
  this.VRBallTestWatch();
}

@addMethod(PlayerPuppet) private func VRBallOnContact(vInN: Float, vOutN: Float, n: Vector4, p: Vector4, dt: Float) -> Void {
  // Recorded before any correction is applied, so these always describe the ENGINE's own bounce.
  this.m_vrBallEIn = vInN;
  this.m_vrBallEOut = vOutN;
  this.m_vrBallEEngine = vOutN / vInN;
  this.m_vrBallContacts += 1;

  if this.m_vrBallTestPhase == 1 {
    this.m_vrBallTestHitZ = p.Z;
    this.m_vrBallTestApexZ = p.Z;
    this.m_vrBallTestPhase = 2;
    FTLog(s"[VRBall] contact: v_in=\(vInN) m/s  v_out(engine)=\(vOutN) m/s  -> e_engine=\(vOutN / vInN)");
  }
  if !this.m_vrBallCorrect { return; }

  // vInN comes from the frame BEFORE the contact, so it is a clean frame average -- but the true
  // speed at the moment of impact is half a frame of gravity higher.
  // vOutN is deliberately NOT used: the frame containing the contact averages descent and rebound
  // together, so its value is meaningless (it can even come out negative).
  this.m_vrBallPendVIn = vInN + 4.905 * dt;
  this.m_vrBallPendN = n;
  this.m_vrBallPendP = p;
  this.m_vrBallPendOn = true;
}

// ---- measurement ------------------------------------------------------------------------------
// Drops the ball from `height` metres and reports the effective restitution from the rebound.
// Run it once with correction OFF to learn what the engine really gives (that number belongs in
// VRBallConst.EngineRestitution), then ON to confirm the model lands on the NBA range.
//   Game.GetPlayer():VRBallSetCorrection(false)
//   Game.GetPlayer():VRBallDropTest(1.8)
@addMethod(PlayerPuppet) public func VRBallDropTest(height: Float) -> Void {
  this.VRBallSpawnAt(1.5, height);
  this.m_vrBallTestPhase = 1;
  this.m_vrBallTestStartZ = this.GetWorldPosition().Z + height;
  FTLog(s"[VRBall] drop test from \(height) m, correction=\(this.m_vrBallCorrect)");
}

@addMethod(PlayerPuppet) public func VRBallSetCorrection(on: Bool) -> Void {
  this.m_vrBallCorrect = on;
  FTLog(s"[VRBall] bounce correction = \(on)");
}

// Called from the tick while the test is running; watches for the apex after the first hit.
@addMethod(PlayerPuppet) public func VRBallTestWatch() -> Void {
  if this.m_vrBallTestPhase != 2 { return; }
  let ent = this.VRBallEntity();
  if !IsDefined(ent) { return; }
  let z = ent.GetWorldPosition().Z;
  if z > this.m_vrBallTestApexZ {
    this.m_vrBallTestApexZ = z;
    return;
  }
  // Falling again: the apex is settled.
  let hIn = this.m_vrBallTestStartZ - this.m_vrBallTestHitZ;
  let hOut = this.m_vrBallTestApexZ - this.m_vrBallTestHitZ;
  if hIn > 0.05 {
    let e = SqrtF(hOut / hIn);
    this.m_vrBallTestDrop = hIn;
    this.m_vrBallTestRebound = hOut;
    this.m_vrBallTestE = e;
    FTLog(s"[VRBall] RESULT drop=\(hIn) m  rebound=\(hOut) m  e_effective=\(e)  (NBA wants 0.82..0.88 from 1.8 m)");
  }
  this.m_vrBallTestPhase = 0;
}

@addMethod(PlayerPuppet) public func VRBallHas() -> Bool { return this.m_vrBallHas; }
@addMethod(PlayerPuppet) public func VRBallLastImpact() -> Float { return this.m_vrBallLastImpact; }

// Readouts for live_eval over the CETBridge -- the only way to get numbers out, since FTLog
// never reaches a file. One string keeps it to a single bridge round-trip.
@addMethod(PlayerPuppet) public func VRBallContacts() -> Int32 { return this.m_vrBallContacts; }
@addMethod(PlayerPuppet) public func VRBallEIn() -> Float { return this.m_vrBallEIn; }
@addMethod(PlayerPuppet) public func VRBallEOut() -> Float { return this.m_vrBallEOut; }
@addMethod(PlayerPuppet) public func VRBallEEngine() -> Float { return this.m_vrBallEEngine; }
@addMethod(PlayerPuppet) public func VRBallTestE() -> Float { return this.m_vrBallTestE; }
@addMethod(PlayerPuppet) public func VRBallTestDrop() -> Float { return this.m_vrBallTestDrop; }
@addMethod(PlayerPuppet) public func VRBallTestRebound() -> Float { return this.m_vrBallTestRebound; }
@addMethod(PlayerPuppet) public func VRBallCorrectionOn() -> Bool { return this.m_vrBallCorrect; }

@addMethod(PlayerPuppet) public func VRBallReport() -> String {
  return s"contacts=\(this.m_vrBallContacts) vIn=\(this.m_vrBallEIn) vOut=\(this.m_vrBallEOut) e_engine=\(this.m_vrBallEEngine) | drop=\(this.m_vrBallTestDrop) rebound=\(this.m_vrBallTestRebound) e_test=\(this.m_vrBallTestE) | correction=\(this.m_vrBallCorrect) | held=\(this.m_vrBallHeld) lastThrow=\(this.m_vrBallLastThrow)";
}

// Hand diagnostics. boneValid=0 means the plugin is not publishing the palm bone -- either an old
// CyberpunkVR_Hands.dll or the skeleton has not been solved yet.
@addMethod(PlayerPuppet) public func VRBallHandReport() -> String {
  let r = this.m_vrBallPalmR;
  let l = this.m_vrBallPalmL;
  let ent = this.VRBallEntity();
  let d = IsDefined(ent) ? Vector4.Distance(r, ent.GetWorldPosition()) : -1.0;
  let pm = VRPalmModelPos(1);
  let cm = VRCamModelPos();
  return s"boneValid=\(pm.W) palmModelR=(\(pm.X), \(pm.Y), \(pm.Z)) camModel=(\(cm.X), \(cm.Y), \(cm.Z)) palmR=(\(r.X), \(r.Y), \(r.Z)) palmL=(\(l.X), \(l.Y), \(l.Z)) speedR=\(Vector4.Length(this.m_vrBallPalmVelR)) distR_ball=\(d) held=\(this.m_vrBallHeld) lastThrow=\(this.m_vrBallLastThrow)";
}

@addMethod(PlayerPuppet) public func VRBallHeld() -> Int32 { return this.m_vrBallHeld; }
@addMethod(PlayerPuppet) public func VRBallSetHandScale(s: Float) -> Void { this.m_vrBallHandScale = s; }

// Console helper: where is the ball and is it actually moving?
@addMethod(PlayerPuppet) public func VRBallStatus() -> Void {
  let ent = this.VRBallEntity();
  if !IsDefined(ent) {
    FTLog("[VRBall] no ball");
    return;
  }
  let p = ent.GetWorldPosition();
  let body = this.VRBallBody();
  let kin = IsDefined(body) ? body.IsKinematic() : true;
  let sim = IsDefined(body) ? body.IsSimulated() : false;
  FTLog(s"[VRBall] pos=(\(p.X), \(p.Y), \(p.Z)) body=\(IsDefined(body)) kinematic=\(kin) simulated=\(sim) vel=(\(this.m_vrBallPrevVel.X), \(this.m_vrBallPrevVel.Y), \(this.m_vrBallPrevVel.Z))");
}
