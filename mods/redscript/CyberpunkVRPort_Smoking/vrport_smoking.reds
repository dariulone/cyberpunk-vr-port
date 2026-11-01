// CyberpunkVRPort — VR smoking (cigarette + lighter). No animations: the player does the motion in
// VR. This module spawns the vanilla prop item-entities into the hands, drives their FX (ember /
// smoke / lighter flame), plays the smoking Wwise events and runs the hands-free auto-puff. Pure
// reuse of base-game assets — no new meshes/FX, no TweakXL/ArchiveXL.
//
// Both are real vanilla prop items (canDrop/savable = false, hidden from UI, placementSlots already
// include WeaponLeft/Right):
//   Items.cigarette_i_stick  -> int_recreation_001__cigarette_i_stick.ent
//                               (EffectSpawner: ember, smoke, smoke_subtle, ember_flicked, ash_dump)
//   Items.apparel_lighter_a  -> int_apparel_001__lighter_a.ent   (EffectSpawner: ignition, ignition_loop)
//
// How to drive it: like Holster/Melee, a CET module (or the plugin) calls these PlayerPuppet helpers,
// e.g. Game.GetPlayer():VRSmokeToggle(). You can also test each step from the CET console directly.
//
// Wwise events: cmn_generic_male_{inhale|exhale}_smoking (female variants exist; kept male for v1).
//
// NOT done here (need the plugin / in-VR iteration, see notes at bottom):
//   * burn-down: the cigarette is a SKINNED mesh (no component visualScale) — shortening needs
//     chunkMask segment-hiding or a rig bone scale. VRSmokeGetLength() exposes 1..0 for the plugin.
//   * "stuck to mouth" as a true head-anchored transform. v1 keeps it in the hand and just auto-puffs.

// NOTE: intentionally NO `module` statement. The plugin registers SetVRSmokeFingers/L as PLAIN
// global natives; a `module` would make redscript expect them under a module-qualified name
// (CyberpunkVRPort.Smoking.SetVRSmokeFingers) and fail script validation. Global scope (like
// vrport_worldmap.reds) matches the plugin's registration. @addMethod/@addField are unaffected.

// Plugin natives (CyberpunkVR_Hands.dll): apply the captured finger + slot grip pose baked in
// bin\x64\CyberpunkVR_SmokeGrip_right.ini (cigarette, RH) and CyberpunkVR_LighterGrip_Left.ini
// (lighter, LH). 1 = curl the hand around the prop, 0 = release. VRIK still drives the wrist to
// the controller; only the fingers (+ the WeaponRight/Left slot transform) come from the pose.
native func SetVRSmokeFingers(active: Int32) -> Int32;   // right hand (cigarette)
native func SetVRSmokeFingersL(active: Int32) -> Int32;  // left hand (lighter)
// Left hand holds the CIGARETTE (1) vs the lighter (0): picks the cig-left grip pose
// (CyberpunkVR_SmokeGrip_Left.ini) and routes the left-hand capture/dump to that file.
native func SetVRSmokeLeftCig(on: Int32) -> Int32;
// Left-handed: the RIGHT hand holds the lighter (1) -- the left hand's lighter pose mirrored onto it, its thumb on the
// right trigger -- vs the cigarette (0).
native func SetVRSmokeRightLighter(on: Int32) -> Int32;
// Model-space cig->mouth distance from the VRIK solve (head bone + view offset vs cig slot). Tracks
// the real HMD + controller; the FPP camera can't (its world pos ignores HMD positional/lean tracking).
native func VRSmokeMouthDist() -> Float;
// LEFT hand -> mouth distance (mirror of VRSmokeMouthDist for the left controller). Lets the LEFT
// grip toggle the cig when the LEFT hand is the one raised to the mouth (right hand down).
native func VRSmokeMouthDistL() -> Float;
// Hands-free mouth anchor: 1 = pin the cig to the head (stays at the lips when the hand drops), 0 =
// back to the hand grip. Offset tunes the anchored cig in VR (pos metres, rot degrees, model space).
native func SetVRSmokeMouthAnchor(on: Int32) -> Int32;
// Pin an ARBITRARY non-weapon bone to the HMD mouth (for a prop on a non-weapon slot; both hands free).
// sel: 0=off (WeaponRight path), 1=Neck1, 2=Head, 3=Neck.
native func SetVRSmokeAnchorBone(sel: Int32) -> Int32;
native func SetVRSmokeMouthOffset(x: Float, y: Float, z: Float, pitch: Float, yaw: Float, roll: Float) -> Int32;
// Burn-down: Y (long-axis) scale of the cig, 1.0 = full. Experiment (attachment inheriting bone scale).
native func SetVRSmokeCigScaleY(y: Float) -> Int32;
// Burn-down EXPERIMENT: show only the low `count` mesh chunks of the cig (<=0 = all). Returns the
// number of mesh components touched. Test from the console to learn if chunks shorten it.
native func SetVRSmokeCigChunks(cig: ref<GameObject>, count: Int32) -> Int32;
// Burn-down: set visualScale.Y (length) on the cig's entMeshComponent (static-mesh override). Returns
// mesh components touched (0 = still skinned / override archive not loaded).
native func SetVRSmokeCigVisualScale(cig: ref<GameObject>, y: Float) -> Int32;
// Read the cig's authored full-length visualScale.Y (from the .ent) so we shrink relative to it.
native func GetVRSmokeCigVisualScaleY(cig: ref<GameObject>) -> Float;
// World pose for the exhale smoke (view pose * the smoke's own offset). Pos.W=1 => valid.
native func VRSmokeMouthWorldPos() -> Vector4;
native func VRSmokeMouthWorldRot() -> Quaternion;
// Live-tune the exhale smoke offset (separate from the cig anchor): x=right,y=fwd,z=up (m); angles deg.
native func SetVRSmokeSmokeOffset(x: Float, y: Float, z: Float, pitch: Float, yaw: Float, roll: Float) -> Int32;

@addField(PlayerPuppet) let m_vrSmokeHas: Bool;
@addField(PlayerPuppet) let m_vrSmokeLit: Bool;
@addField(PlayerPuppet) let m_vrSmokeMouth: Bool;
@addField(PlayerPuppet) let m_vrSmokeLen: Float;        // 1.0 full .. 0.0 spent
@addField(PlayerPuppet) let m_vrSmokePuffToken: Int32;  // guards stale auto-puff callbacks
// Gesture state, driven per-frame by VRSmokeTick (CET feeds the VR trigger/grip).
@addField(PlayerPuppet) let m_vrSmokeIgniting: Bool;
@addField(PlayerPuppet) let m_vrSmokeWasMouth: Bool;
@addField(PlayerPuppet) let m_vrSmokeGripPrev: Bool;    // right grip previous frame (edge detect)
@addField(PlayerPuppet) let m_vrSmokeGripPrevL: Bool;   // left grip previous frame (edge detect)
@addField(PlayerPuppet) let m_vrSmokeLightProgress: Float; // seconds the flame has held on the cig tip
@addField(PlayerPuppet) let m_vrSmokeCigBase: Float;       // authored full-length visualScale.Y (from .ent)
@addField(PlayerPuppet) let m_vrSmokeCigBaseKnown: Bool;
@addField(PlayerPuppet) let m_vrSmokeLightCd: Float;
@addField(PlayerPuppet) let m_vrSmokeStickCd: Float;
@addField(PlayerPuppet) let m_vrSmokeLastLight: Float;  // last cig-tip -> lighter distance (for the log)
@addField(PlayerPuppet) let m_vrSmokeLastMouth: Float;  // last cig -> mouth distance (plugin, model space)
@addField(PlayerPuppet) let m_vrSmokeLastMouthL: Float; // last LEFT hand -> mouth distance (plugin)
@addField(PlayerPuppet) let m_vrSmokeCigId: ItemID;     // stored so we can re-slot the SAME cig entity
@addField(PlayerPuppet) let m_vrSmokeMouthSlot: Int32;  // TEST slot: 0=Head, 1..4=HeadFabricEnhancer1..4
@addField(PlayerPuppet) let m_vrSmokeLeftHanded: Bool; // lit left-handed: cig in the left hand, lighter in the right
@addField(PlayerPuppet) let m_vrSmokeCigSlot: Int32;   // where the cig lives: 0=WeaponRight(RH),1=Splinter(mouth),2=WeaponLeft(LH)

@addMethod(PlayerPuppet) private func VRSmokeTS() -> ref<TransactionSystem> {
  return GameInstance.GetTransactionSystem(this.GetGame());
}
// The cig is in the slot it was put in: left-handed the other hand's slot holds the lighter, so the slots are not
// searched.
@addMethod(PlayerPuppet) public func VRSmokeGetCig() -> ref<GameObject> {
  return this.VRSmokeTS().GetItemInSlot(this, this.VRSmokeCigSlotID(this.m_vrSmokeCigSlot));
}
// Slot the cig currently occupies (0=WeaponRight, 1=Splinter/mouth, 2=WeaponLeft).
@addMethod(PlayerPuppet) private func VRSmokeCigSlotID(s: Int32) -> TweakDBID {
  if s == 1 { return t"AttachmentSlots.Splinter"; }
  if s == 2 { return t"AttachmentSlots.WeaponLeft"; }
  return t"AttachmentSlots.WeaponRight";
}
@addMethod(PlayerPuppet) private func VRSmokeLighterSlot() -> TweakDBID {
  return this.m_vrSmokeLeftHanded ? t"AttachmentSlots.WeaponRight" : t"AttachmentSlots.WeaponLeft";
}
@addMethod(PlayerPuppet) private func VRSmokeLighter() -> ref<GameObject> {
  return this.VRSmokeTS().GetItemInSlot(this, this.VRSmokeLighterSlot());
}
// The lighter left the hand holding it: that hand lets go.
@addMethod(PlayerPuppet) private func VRSmokeReleaseLighterHand() -> Void {
  if this.m_vrSmokeLeftHanded { SetVRSmokeFingers(0); SetVRSmokeRightLighter(0); } else { SetVRSmokeFingersL(0); }
}

// ---- spawn bind: lighter in LH, cigarette in RH (unlit); left-handed the other way round ----
@addMethod(PlayerPuppet) public func VRSmokeToggle() -> Void {
  if this.m_vrSmokeHas { this.VRSmokeExtinguish(); } else { this.VRSmokeSpawn(); }
}
@addMethod(PlayerPuppet) public func VRSmokeSpawn() -> Void {
  let ts = this.VRSmokeTS();
  let cig = ItemID.FromTDBID(t"Items.cigarette_i_stick");
  this.m_vrSmokeCigId = cig;
  this.m_vrSmokeLeftHanded = VRLeftHanded();
  this.m_vrSmokeCigSlot = this.m_vrSmokeLeftHanded ? 2 : 0;   // starts in the hand that does not hold the lighter
  ts.GiveItem(this, cig, 1);
  ts.AddItemToSlot(this, this.VRSmokeCigSlotID(this.m_vrSmokeCigSlot), cig, true);
  let lt = ItemID.FromTDBID(t"Items.apparel_lighter_a");
  ts.GiveItem(this, lt, 1);
  ts.AddItemToSlot(this, this.VRSmokeLighterSlot(), lt, true);
  this.m_vrSmokeHas = true;
  this.m_vrSmokeLit = false;
  this.m_vrSmokeMouth = false;
  this.m_vrSmokeLen = 1.0;
  // Default full length = 1.3 (authored). FORCE it on spawn (known=true) instead of re-learning via
  // GetVRSmokeCigVisualScaleY: on a re-spawn the cig mesh can still carry the previous burned-down
  // (short "butt") visualScale override, which the learner would wrongly latch as the base. The tick
  // then applies base*len = 1.3 each frame, resetting the mesh to full length.
  this.m_vrSmokeCigBase = 1.3;
  this.m_vrSmokeCigBaseKnown = true;
  // each hand's grip around what it holds: the cig pose in its hand, the lighter pose in the other
  SetVRSmokeRightLighter(this.m_vrSmokeLeftHanded ? 1 : 0);
  SetVRSmokeLeftCig(this.m_vrSmokeLeftHanded ? 1 : 0);
  SetVRSmokeFingers(1);
  SetVRSmokeFingersL(1);
}

// ---- TEST: swap the cig onto a HEAD slot on the fly (frees WeaponRight for a weapon) ----
// From the CET console, with a cig spawned in hand:
//   Game.GetPlayer():VRSmokeTestSlot(0)      -> AttachmentSlots.Head
//   Game.GetPlayer():VRSmokeTestSlot(1..4)   -> AttachmentSlots.HeadFabricEnhancer1..4
//   Game.GetPlayer():VRSmokeTestHand()       -> back into the right hand
// Returns TRUE/FALSE (did it attach). ignoreRestrictions=true bypasses the cig's placementSlots.
// CAVEAT: the head bone is idle in FPP, so if it renders it will sit at the MODEL head, not yet the
// real HMD. This only answers "does the prop attach + render in that slot"; HMD-tracking is the next
// (plugin) step once we know a slot that renders.
@addMethod(PlayerPuppet) public func VRSmokeMouthSlotID(idx: Int32) -> TweakDBID {
  // vanilla head slots (tested: prop does NOT render -- garment-composited, no bone binding)
  if idx == 1 { return t"AttachmentSlots.HeadFabricEnhancer1"; }
  if idx == 2 { return t"AttachmentSlots.HeadFabricEnhancer2"; }
  if idx == 3 { return t"AttachmentSlots.HeadFabricEnhancer3"; }
  if idx == 4 { return t"AttachmentSlots.HeadFabricEnhancer4"; }
  // EquipmentEx OutfitSlots -- REAL bone-mapped slots that render props. HandProp* are dedicated
  // prop slots (best bet, and visible in-hand in FPP); Mask/Glasses/Head/Neckwear ride the head/neck.
  if idx == 5 { return t"OutfitSlots.HandPropRight"; }
  if idx == 6 { return t"OutfitSlots.HandPropLeft"; }
  if idx == 7 { return t"OutfitSlots.Mask"; }
  if idx == 8 { return t"OutfitSlots.Glasses"; }
  if idx == 9 { return t"OutfitSlots.Head"; }
  if idx == 10 { return t"OutfitSlots.Neckwear"; }
  // HELD-ITEM slots (spawn+attach a real entity like the weapon slots do -> should render the prop),
  // but separate from WeaponRight/WeaponLeft so no collision with pistol or lighter.
  if idx == 11 { return t"AttachmentSlots.Consumable"; }
  if idx == 12 { return t"AttachmentSlots.Inspect"; }
  // entSlotComponent slot NAMES bound to non-weapon bones (from the player .ent dump). Testing whether
  // AddItemToSlot resolves them by name + renders the prop there -- 13=TppHead is the goal (Head bone).
  if idx == 13 { return t"AttachmentSlots.TppHead"; }       // -> Head bone (TARGET)
  if idx == 14 { return t"AttachmentSlots.CarryingRight"; }  // -> WeaponRight bone (held-entity probe)
  if idx == 15 { return t"AttachmentSlots.PersonalLink"; }   // -> Spine3 (renders per gameAttachmentSlots)
  if idx == 16 { return t"AttachmentSlots.bothHands"; }      // -> WeaponLeft bone
  if idx == 17 { return t"AttachmentSlots.MonoDiscTarget"; } // -> LeftShoulder
  if idx == 18 { return t"AttachmentSlots.Splinter"; }       // -> (None)
  return t"AttachmentSlots.Head";
}
// MOVE the SAME spawned cig between slots (no re-give -- re-giving a single-instance item breaks it).
// Spawn a cig first with Toggle, then call these. ignoreRestrictions=true bypasses placementSlots.
@addMethod(PlayerPuppet) public func VRSmokeTestSlot(idx: Int32) -> Bool {
  if !this.m_vrSmokeHas { FTLog("[Smoke] TestSlot: spawn a cig first (Toggle)"); return false; }
  let ts = this.VRSmokeTS();
  ts.RemoveItemFromSlot(this, t"AttachmentSlots.WeaponRight", false);
  ts.RemoveItemFromSlot(this, this.VRSmokeMouthSlotID(this.m_vrSmokeMouthSlot), false);
  this.m_vrSmokeMouthSlot = idx;
  let ok = ts.AddItemToSlot(this, this.VRSmokeMouthSlotID(idx), this.m_vrSmokeCigId, true, null, ERenderingPlane.RPl_Scene, false, true);
  SetVRSmokeFingers(0);
  SetVRSmokeMouthAnchor(0);
  if ok { FTLog("[Smoke] TestSlot attach=TRUE"); } else { FTLog("[Smoke] TestSlot attach=FALSE"); }
  return ok;
}
@addMethod(PlayerPuppet) public func VRSmokeTestHand() -> Bool {
  if !this.m_vrSmokeHas { FTLog("[Smoke] TestHand: spawn a cig first (Toggle)"); return false; }
  let ts = this.VRSmokeTS();
  ts.RemoveItemFromSlot(this, this.VRSmokeMouthSlotID(this.m_vrSmokeMouthSlot), false);
  let ok = ts.AddItemToSlot(this, t"AttachmentSlots.WeaponRight", this.m_vrSmokeCigId, true);
  SetVRSmokeFingers(1);
  if ok { FTLog("[Smoke] TestHand attach=TRUE"); } else { FTLog("[Smoke] TestHand attach=FALSE"); }
  return ok;
}
// TEST: pin the Splinter-slot cig (its bone is set in the .ent -> Neck1/Head) to the HMD mouth via
// the plugin. sel MUST match the .ent bone: 1=Neck1, 2=Head, 3=Neck. Both hands stay free.
//   Game.GetPlayer():VRSmokeToggle()            -- spawn
//   Game.GetPlayer():VRSmokeTestMouthPin(1)     -- cig -> Splinter(Neck1), pin Neck1 to the mouth
//   Game.SetVRSmokeMouthOffset(x,y,z,p,yw,r)    -- fine-tune the lips position (m / deg)
//   Game.GetPlayer():VRSmokeTestUnpin()         -- release + back to hand
@addMethod(PlayerPuppet) public func VRSmokeTestMouthPin(sel: Int32) -> Void {
  this.VRSmokeTestSlot(18);
  SetVRSmokeAnchorBone(sel);
  SetVRSmokeMouthAnchor(1);
  FTLog("[Smoke] TestMouthPin: cig on Splinter, plugin pinning the chosen bone to the mouth");
}
@addMethod(PlayerPuppet) public func VRSmokeTestUnpin() -> Void {
  SetVRSmokeMouthAnchor(0);
  SetVRSmokeAnchorBone(0);
  this.VRSmokeTestHand();
}
// TEST: cig -> Splinter slot (bound to RightHandEnd in the .ent), pinned to the mouth by the PROVEN
// hand-chain WeaponRight-anchor (which now targets RightHandEnd). Leaf bone => body/head untouched,
// and WeaponRight stays free for a pistol. Usage:
//   Game.GetPlayer():VRSmokeToggle()
//   Game.GetPlayer():VRSmokeTestSplinterPin()
//   Game.SetVRSmokeMouthOffset(-0.012,-0.008,-0.015, 0,0,0)  -- tune (bone-local, like the hand grip)
@addMethod(PlayerPuppet) public func VRSmokeTestSplinterPin() -> Void {
  this.VRSmokeTestSlot(18);
  SetVRSmokeAnchorBone(0);
  SetVRSmokeMouthAnchor(1);
  FTLog("[Smoke] SplinterPin: cig on Splinter(RightHandEnd), hand-chain anchor -> mouth");
}
// Put the cig on Consumable, then force the EXISTING WeaponRight-bone mouth pin ON. If the cig snaps
// to the mouth -> Consumable shares the WeaponRight bone (bad: a pistol would follow). If it stays in
// the hand -> Consumable is a different bone (good: I add plugin pinning for that bone).
@addMethod(PlayerPuppet) public func VRSmokeTestConsumablePin() -> Void {
  this.VRSmokeTestSlot(11);
  SetVRSmokeMouthAnchor(1);
  FTLog("[Smoke] Consumable + WeaponRight-pin ON: does the cig go to the mouth or stay in hand?");
}

// ---- LEFT TRIGGER: lighter flame on/off ----
// "ignition" is the flint spark (the click/flash you press for); "ignition_loop" is the sustained
// flame. Fire the spark each press so you see + hear the strike, then hold the flame.
@addMethod(PlayerPuppet) public func VRSmokeIgnite() -> Void {
  let lt = this.VRSmokeLighter();
  if !IsDefined(lt) { return; }
  GameObjectEffectHelper.StartEffectEvent(lt, n"ignition");        // spark / flint strike
  GameObjectEffectHelper.StartEffectEvent(lt, n"ignition_loop", true); // sustained flame
  GameObject.PlaySound(lt, n"cmn_generic_item_lighter");           // flint/whoosh (generic lighter SFX)
}
@addMethod(PlayerPuppet) public func VRSmokeDouse() -> Void {
  let lt = this.VRSmokeLighter();
  if IsDefined(lt) {
    GameObjectEffectHelper.StopEffectEvent(lt, n"ignition_loop");
    GameObjectEffectHelper.StopEffectEvent(lt, n"ignition");
  }
}

// ---- flame tip near cigarette tip: light it, lighter disappears ----
@addMethod(PlayerPuppet) public func VRSmokeLight() -> Void {
  if !this.m_vrSmokeHas || this.m_vrSmokeLit { return; }
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  this.m_vrSmokeLit = true;
  GameObjectEffectHelper.StartEffectEvent(cig, n"ember");                // initial catch flash
  GameObjectEffectHelper.StartEffectEvent(cig, n"ember_passive", true);  // CONTINUOUS tip smolder glow
  GameObjectEffectHelper.StartEffectEvent(cig, n"smoke_trail", true);    // continuous smoke trail from the cig
  GameObject.PlaySound(cig, n"cmn_generic_male_inhale_smoking");
  this.VRSmokeDouse();
  this.VRSmokeTS().RemoveItemFromSlot(this, this.VRSmokeLighterSlot(), true); // lighter gone
  this.VRSmokeReleaseLighterHand();
  if this.m_vrSmokeMouth { this.VRSmokeSchedulePuff(); } // lit while in the mouth -> start auto-puff
}

// ---- brought to mouth (proximity) ----
@addMethod(PlayerPuppet) public func VRSmokeInhale() -> Void {
  if !this.m_vrSmokeLit { return; }
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  GameObjectEffectHelper.StartEffectEvent(cig, n"ember");
  GameObject.PlaySound(cig, n"cmn_generic_male_inhale_smoking");
}
@addMethod(PlayerPuppet) public func VRSmokeExhale() -> Void {
  if !this.m_vrSmokeLit { return; }
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  this.VRSmokeMouthPuff(); // spawn the exhale smoke at the mouth (world)
  GameObject.PlaySound(cig, n"cmn_generic_male_exhale_smoking");
  // Length is burned continuously in the tick now (smooth), not in discrete puff steps.
}
@addMethod(PlayerPuppet) public func VRSmokeFlickAsh() -> Void {
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  GameObjectEffectHelper.StartEffectEvent(cig, n"ember_flicked");
  GameObjectEffectHelper.StartEffectEvent(cig, n"ash_dump");
}
// Exhale smoke spawned at the MOUTH (world), independent of where the cig is. The FxResource comes
// from a ResourceLibraryComponent named "ResourceLibrary" on the cig (key "mouth_smoke" ->
// d_cigarette_smoke_exhaust_fpp). Position = FPP camera (head) a touch lower = lips; dir = facing.
@addMethod(PlayerPuppet) public func VRSmokeMouthPuff() -> Void {
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  let lib = cig.FindComponentByName(n"ResourceLibrary") as ResourceLibraryComponent;
  if !IsDefined(lib) { return; }
  let fx = lib.GetResource(n"mouth_smoke");
  if !FxResource.IsValid(fx) { return; }
  // Real HMD-tracked mouth pose from the plugin (view pose * the smoke's own tunable offset).
  let mp = VRSmokeMouthWorldPos();
  if mp.W < 0.5 { return; }
  let xf: WorldTransform;
  WorldTransform.SetPosition(xf, Vector4(mp.X, mp.Y, mp.Z, 1.0));
  WorldTransform.SetOrientation(xf, VRSmokeMouthWorldRot());
  GameInstance.GetFxSystem(this.GetGame()).SpawnEffect(fx, xf);
}

// ---- GRIP at mouth: enable/disable hands-free auto-puff (v1 keeps cig in hand) ----
@addMethod(PlayerPuppet) public func VRSmokeStickToggle(destHand: Int32) -> Void {
  let ts = this.VRSmokeTS();
  if this.m_vrSmokeMouth {
    // OUT of the mouth -> into the hand whose grip fired. destHand: 0 = RIGHT (WeaponRight),
    // 2 = LEFT (WeaponLeft). If THAT hand already holds a weapon, leave the cig in the mouth.
    if destHand == 2 {
      if IsDefined(ts.GetItemInSlot(this, t"AttachmentSlots.WeaponLeft")) { return; }   // left hand busy -> stay
      ts.RemoveItemFromSlot(this, t"AttachmentSlots.Splinter", false);
      SetVRSmokeLeftCig(1);   // the left hand now holds the CIG -> apply the cig-left grip pose
      ts.AddItemToSlot(this, t"AttachmentSlots.WeaponLeft", this.m_vrSmokeCigId, true);
      SetVRSmokeFingersL(1);
      this.m_vrSmokeCigSlot = 2;
    } else {
      if IsDefined(ts.GetItemInSlot(this, t"AttachmentSlots.WeaponRight")) { return; }  // right hand busy -> stay
      ts.RemoveItemFromSlot(this, t"AttachmentSlots.Splinter", false);
      ts.AddItemToSlot(this, t"AttachmentSlots.WeaponRight", this.m_vrSmokeCigId, true);
      SetVRSmokeFingers(1);
      this.m_vrSmokeCigSlot = 0;
    }
    SetVRSmokeMouthAnchor(0);
    this.m_vrSmokeMouth = false;
    this.m_vrSmokePuffToken += 1;
  } else {
    // INTO the mouth: move the cig off its current hand onto Splinter (WeaponRight1 leaf) -> frees BOTH
    // hands + both weapon slots. Can be put in unlit (to light it there).
    if !this.m_vrSmokeHas { return; }
    ts.RemoveItemFromSlot(this, this.VRSmokeCigSlotID(this.m_vrSmokeCigSlot), false);
    ts.AddItemToSlot(this, t"AttachmentSlots.Splinter", this.m_vrSmokeCigId, true, null, ERenderingPlane.RPl_Scene, false, true);
    if this.m_vrSmokeCigSlot == 2 { SetVRSmokeFingersL(0); SetVRSmokeLeftCig(0); } else { SetVRSmokeFingers(0); }
    this.m_vrSmokeCigSlot = 1;
    SetVRSmokeMouthAnchor(1);   // pin WeaponRight1 to the lips (hands-free)
    this.m_vrSmokeMouth = true;
    if this.m_vrSmokeLit {
      this.VRSmokeInhale();
      this.VRSmokeScheduleExhale(this.m_vrSmokePuffToken);
      this.VRSmokeSchedulePuff();
    }
  }
}
@addMethod(PlayerPuppet) public func VRSmokeHasCig() -> Bool { return this.m_vrSmokeHas; }
@addMethod(PlayerPuppet) public func VRSmokeIsLit() -> Bool { return this.m_vrSmokeLit; }
@addMethod(PlayerPuppet) public func VRSmokeSchedulePuff() -> Void {
  let cb = new VRSmokePuffCallback();
  cb.player = this;
  cb.token = this.m_vrSmokePuffToken;
  GameInstance.GetDelaySystem(this.GetGame()).DelayCallback(cb, 5.0); // period (s)
}
@addMethod(PlayerPuppet) public func VRSmokeAutoPuff(token: Int32) -> Void {
  if !this.m_vrSmokeMouth || !this.m_vrSmokeLit || token != this.m_vrSmokePuffToken { return; }
  this.VRSmokeInhale();                 // draw at the mouth (ember + inhale sound)
  this.VRSmokeScheduleExhale(token);    // then blow the smoke out (see below)
  if this.m_vrSmokeLit && this.m_vrSmokeMouth { this.VRSmokeSchedulePuff(); }
}
// Hands-free exhale: ~1.5 s after the draw, emit the smoke. The cig is anchored at the mouth while
// in-mouth, so its "smoke" effect billows from the mouth (the smoke-from-mouth the hand path can't do).
@addMethod(PlayerPuppet) public func VRSmokeScheduleExhale(token: Int32) -> Void {
  let cb = new VRSmokeExhaleCallback();
  cb.player = this;
  cb.token = token;
  GameInstance.GetDelaySystem(this.GetGame()).DelayCallback(cb, 1.5);
}
@addMethod(PlayerPuppet) public func VRSmokeAutoExhale(token: Int32) -> Void {
  if !this.m_vrSmokeMouth || !this.m_vrSmokeLit || token != this.m_vrSmokePuffToken { return; }
  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  this.VRSmokeMouthPuff(); // spawn the exhale smoke at the mouth (world)
  GameObject.PlaySound(cig, n"cmn_generic_male_exhale_smoking");
}

// ---- burn-down bookkeeping (rendering of the shrink is the plugin's job) ----
@addMethod(PlayerPuppet) private func VRSmokeConsume(step: Float) -> Void {
  this.m_vrSmokeLen -= step;
  let cig = this.VRSmokeGetCig();
  if IsDefined(cig) { SetVRSmokeCigVisualScale(cig, this.m_vrSmokeLen); } // shrink length as it burns
  if this.m_vrSmokeLen <= 0.12 { this.VRSmokeExtinguish(); }
}
@addMethod(PlayerPuppet) public func VRSmokeGetLength() -> Float { return this.m_vrSmokeLen; }
@addMethod(PlayerPuppet) public func VRSmokeIsMouth() -> Bool { return this.m_vrSmokeMouth; }

@addMethod(PlayerPuppet) public func VRSmokeExtinguish() -> Void {
  this.m_vrSmokePuffToken += 1;
  let cig = this.VRSmokeGetCig();
  if IsDefined(cig) {
    GameObjectEffectHelper.StopEffectEvent(cig, n"smoke_trail");
    GameObjectEffectHelper.StopEffectEvent(cig, n"ember_passive");
  }
  this.VRSmokeTS().RemoveItemFromSlot(this, t"AttachmentSlots.WeaponRight", true);
  this.VRSmokeTS().RemoveItemFromSlot(this, t"AttachmentSlots.WeaponLeft", true);
  this.VRSmokeTS().RemoveItemFromSlot(this, t"AttachmentSlots.Splinter", true);   // in the mouth (WeaponRight1)
  this.VRSmokeTS().RemoveItemFromSlot(this, this.VRSmokeMouthSlotID(this.m_vrSmokeMouthSlot), true);
  this.m_vrSmokeCigSlot = 0;
  this.m_vrSmokeHas = false;
  this.m_vrSmokeLit = false;
  this.m_vrSmokeMouth = false;
  this.m_vrSmokeLen = 1.0;
  this.m_vrSmokeCigBaseKnown = false;
  SetVRSmokeMouthAnchor(0);
  SetVRSmokeFingers(0);
  SetVRSmokeFingersL(0);
  SetVRSmokeLeftCig(0);
  SetVRSmokeRightLighter(0);
}

// World tip = origin + forward*len. The cig's long axis runs along its forward; if the prop's tip
// turns out to lie along another axis in VR, swap GetWorldForward for GetWorldUp/GetWorldRight (and
// tune len) -- the log prints the resulting distances so it can be dialed in.
@addMethod(PlayerPuppet) private func VRSmokeTipWorld(obj: ref<GameObject>, len: Float) -> Vector4 {
  return obj.GetWorldPosition() + obj.GetWorldForward() * len;
}

@addMethod(PlayerPuppet) public func VRSmokeLastLightDist() -> Float { return this.m_vrSmokeLastLight; }
@addMethod(PlayerPuppet) public func VRSmokeLastMouthDist() -> Float { return this.m_vrSmokeLastMouth; }
@addMethod(PlayerPuppet) public func VRSmokeLastMouthDistL() -> Float { return this.m_vrSmokeLastMouthL; }

// Per-frame gesture driver. CET feeds both VR triggers (0..1; the lighter's hand strikes it) and BOTH grips (bool). Proximity is
// computed from the REAL world positions of the cig tip (cigar_tip slot), the lighter, and the head
// (FPP camera) -- not the coarse controller distance -- so lighting/puffing fire when the props meet.
@addMethod(PlayerPuppet) public func VRSmokeTick(leftTrigger: Float, rightTrigger: Float, gripR: Bool, gripL: Bool, dt: Float) -> Void {
  if !this.m_vrSmokeHas {
    this.m_vrSmokeIgniting = false;
    this.m_vrSmokeWasMouth = false;
    return;
  }
  this.m_vrSmokeLightCd -= dt;
  this.m_vrSmokeStickCd -= dt;

  let cig = this.VRSmokeGetCig();
  if !IsDefined(cig) { return; }
  // Burn-down length. Learn the authored full length (visualScale.Y from the .ent, e.g. 1.3) once,
  // then apply base * burnLength -- so the full size lives in WolvenKit, not hardcoded here.
  if !this.m_vrSmokeCigBaseKnown {
    let b = GetVRSmokeCigVisualScaleY(cig);
    if b > 0.01 { this.m_vrSmokeCigBase = b; this.m_vrSmokeCigBaseKnown = true; }
  }
  if this.m_vrSmokeCigBaseKnown {
    SetVRSmokeCigVisualScale(cig, this.m_vrSmokeCigBase * this.m_vrSmokeLen);
  }
  let cigTip = this.VRSmokeTipWorld(cig, 0.065);

  // Mouth proximity from the plugin: |controller - HMD| (HMD-local metres), per hand. Measured in VR:
  // hand at the lips ~0.17, chest ~0.4, arm down ~0.6. So "at mouth" enters at 0.22. Each grip is
  // gated on its OWN hand's distance, so raising the LEFT hand triggers the LEFT grip and vice versa.
  this.m_vrSmokeLastMouth  = VRSmokeMouthDist();
  this.m_vrSmokeLastMouthL = VRSmokeMouthDistL();

  this.m_vrSmokeLastLight = 999.0;
  let lighter = this.VRSmokeLighter();
  if IsDefined(lighter) {
    this.m_vrSmokeLastLight = Vector4.Distance(cigTip, lighter.GetWorldPosition());
  }

  // Grip near the mouth. Each grip only fires when ITS OWN hand is at the lips (rising edge). Which
  // hand fires decides the destination when taking the cig OUT of the mouth: right grip -> RIGHT hand,
  // left grip -> LEFT hand. Putting the cig IN fires from whichever hand holds it (that hand is raised).
  let fireR = gripR && !this.m_vrSmokeGripPrev  && this.m_vrSmokeLastMouth  < 0.22;
  let fireL = gripL && !this.m_vrSmokeGripPrevL && this.m_vrSmokeLastMouthL < 0.22;
  if (fireR || fireL) && this.m_vrSmokeStickCd <= 0.0 {
    if this.m_vrSmokeMouth {
      let dest = 2;              // LEFT grip fired -> left hand
      if fireR { dest = 0; }     // RIGHT grip fired (wins if both the same frame) -> right hand
      this.VRSmokeStickToggle(dest);
    } else {
      this.VRSmokeStickToggle(0);   // INTO the mouth (destination arg unused for this direction)
    }
    this.m_vrSmokeStickCd = 0.5;
  }
  this.m_vrSmokeGripPrev  = gripR;
  this.m_vrSmokeGripPrevL = gripL;

  if !this.m_vrSmokeLit {
    let lt = this.m_vrSmokeLeftHanded ? rightTrigger : leftTrigger;
    if lt >= 0.95 && !this.m_vrSmokeIgniting {
      this.VRSmokeIgnite(); this.m_vrSmokeIgniting = true;
    } else {
      if lt < 0.85 && this.m_vrSmokeIgniting {
        this.VRSmokeDouse(); this.m_vrSmokeIgniting = false;
      }
    }
    // Gradual light: the flame has to heat the cig tip for ~1.5 s before it catches. Pulling the
    // flame off the tip resets the progress, so you have to hold it there like a real light.
    if this.m_vrSmokeIgniting && this.m_vrSmokeLastLight < 0.08 {
      this.m_vrSmokeLightProgress += dt;
      if this.m_vrSmokeLightProgress >= 1.5 && this.m_vrSmokeLightCd <= 0.0 {
        this.VRSmokeLight();
        this.m_vrSmokeIgniting = false;
        this.m_vrSmokeLightProgress = 0.0;
        this.m_vrSmokeLightCd = 0.5;
      }
    } else {
      this.m_vrSmokeLightProgress = 0.0;
    }
  } else {
    this.m_vrSmokeIgniting = false;
    // Hysteresis: once at the mouth, hold until 0.30; to (re)enter, must drop below 0.22.
    let atMouth = this.m_vrSmokeLastMouth < (this.m_vrSmokeWasMouth ? 0.30 : 0.22);
    if atMouth && !this.m_vrSmokeWasMouth {
      this.VRSmokeInhale();
    } else {
      if !atMouth && this.m_vrSmokeWasMouth {
        this.VRSmokeExhale();
      }
    }
    this.m_vrSmokeWasMouth = atMouth;
    // Continuous smolder: ~2 min passive (len 1.0 -> 0.4 at 0.005/s), ~3x faster while drawing at the
    // mouth. Smooth because the length feeds visualScale every tick. Stops/extinguishes at len 0.4 so
    // a ~40% stub remains (doesn't burn down to nothing).
    let burn = atMouth ? 0.015 : 0.005;
    this.m_vrSmokeLen -= burn * dt;
    if this.m_vrSmokeLen <= 0.4 { this.VRSmokeExtinguish(); }
  }
}

// A weapon, a gadget or a consumable taken into a hand, or the weapons put away, ends the smoke: the cigarette
// and the lighter sit in both hands' weapon slots and hold both grips, and a weapon drawn over them left the
// two props and the poses behind. Every way the game draws, switches or holsters comes through here -- the Y
// tap and the cycling inputs (the state machine's SendEquipmentSystemWeaponManipulationRequest), DrawItem (the
// wheels), UnequipAll -- so the end of a left-hand action (UnequipConsumable, UnequipGadget,
// UnequipLeftHandCyberware) is the only request that leaves the cigarette in.
@addMethod(PlayerPuppet) public func VRSmokeEndedBy(action: EquipmentManipulationAction) -> Bool {
  return NotEquals(action, EquipmentManipulationAction.Undefined)
    && NotEquals(action, EquipmentManipulationAction.UnequipConsumable)
    && NotEquals(action, EquipmentManipulationAction.UnequipGadget)
    && NotEquals(action, EquipmentManipulationAction.UnequipLeftHandCyberware);
}

@wrapMethod(EquipmentSystemPlayerData)
public final func OnEquipmentSystemWeaponManipulationRequest(request: ref<EquipmentSystemWeaponManipulationRequest>) -> Void {
  let player = this.m_owner as PlayerPuppet;
  if IsDefined(player) && player.VRSmokeHasCig() && player.VRSmokeEndedBy(request.requestType) {
    player.VRSmokeExtinguish();
  }
  wrappedMethod(request);
}

public class VRSmokePuffCallback extends DelayCallback {
  public let player: wref<PlayerPuppet>;
  public let token: Int32;
  public func Call() -> Void {
    if IsDefined(this.player) { this.player.VRSmokeAutoPuff(this.token); }
  }
}

public class VRSmokeExhaleCallback extends DelayCallback {
  public let player: wref<PlayerPuppet>;
  public let token: Int32;
  public func Call() -> Void {
    if IsDefined(this.player) { this.player.VRSmokeAutoExhale(this.token); }
  }
}
