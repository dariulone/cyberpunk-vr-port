// CyberpunkVRPort — VR motion melee. Treat the weapon's blade as a real collider: a segment from
// the grip to the tip, with a radius "hit zone" around it. Every frame the CET weapon mod passes the
// current blade segment; we find NPCs whose body is within that radius of the segment and fire the
// game's OWN native melee attack precisely on the touched enemy, so collision/damage/reaction/stamina
// behave like the flat game. Driven by the real VR swing, not the camera/animation.
//
// Called from CET each frame while swinging a melee weapon:
//   Game.GetPlayer():VRMeleeBladeHit(weapon, bladeStart, bladeEnd, hitRadius, strong)

module CyberpunkVRPort.Melee

// Distance from point p to segment [a,b] (component-wise; no Vector4-operator assumptions).
func VRMeleeDistPointSeg(p: Vector4, a: Vector4, b: Vector4) -> Float {
  let abx = b.X - a.X;  let aby = b.Y - a.Y;  let abz = b.Z - a.Z;
  let apx = p.X - a.X;  let apy = p.Y - a.Y;  let apz = p.Z - a.Z;
  let len2 = abx*abx + aby*aby + abz*abz;
  let t = 0.0;
  if len2 > 0.0001 { t = (apx*abx + apy*aby + apz*abz) / len2; };
  if t < 0.0 { t = 0.0; };
  if t > 1.0 { t = 1.0; };
  let cx = a.X + abx*t;  let cy = a.Y + aby*t;  let cz = a.Z + abz*t;
  let dx = p.X - cx;  let dy = p.Y - cy;  let dz = p.Z - cz;
  return SqrtF(dx*dx + dy*dy + dz*dz);
}

// Min distance from the enemy body axis (sampled at 5 heights from feet to head) to the blade segment.
// More samples = the blade isn't given a "free pass" through the gaps between samples on a tall body,
// which matters when the hit radius is small (so the detection is precise: "blade actually crossed the
// body silhouette", not "blade waved nearby").
func VRMeleeEnemyDist(ent: wref<GameObject>, a: Vector4, b: Vector4) -> Float {
  let base = ent.GetWorldPosition();
  let d0 = VRMeleeDistPointSeg(new Vector4(base.X, base.Y, base.Z + 0.25, 1.0), a, b);  // shin
  let d1 = VRMeleeDistPointSeg(new Vector4(base.X, base.Y, base.Z + 0.7,  1.0), a, b);  // hip
  let d2 = VRMeleeDistPointSeg(new Vector4(base.X, base.Y, base.Z + 1.05, 1.0), a, b);  // chest
  let d3 = VRMeleeDistPointSeg(new Vector4(base.X, base.Y, base.Z + 1.4,  1.0), a, b);  // neck
  let d4 = VRMeleeDistPointSeg(new Vector4(base.X, base.Y, base.Z + 1.65, 1.0), a, b);  // head
  return MinF(MinF(d0, d1), MinF(d2, MinF(d3, d4)));
}

// Find the weapon's melee Attack_GameEffect robustly (GetCurrentAttack mutates during attacks, so
// fall back to scanning GetAttacks()).
@addMethod(WeaponObject)
public func VRGetMeleeAttack() -> ref<Attack_GameEffect> {
  let a = this.GetCurrentAttack() as Attack_GameEffect;
  if IsDefined(a) { return a; };
  let attacks = this.GetAttacks();
  let i = 0;
  while i < ArraySize(attacks) {
    a = attacks[i] as Attack_GameEffect;
    if IsDefined(a) { return a; };
    i += 1;
  }
  return null;
}

// The weapon's melee reach, straight from the game's attack record (TweakDB Range() — the same value
// the flat game feeds the melee game-effect). No hardcoded blade length.
@addMethod(WeaponObject)
public func VRMeleeReach() -> Float {
  let a = this.VRGetMeleeAttack();
  if !IsDefined(a) { return 1.5; };
  let r = a.GetRecord().Range();
  if r < 0.3 { return 1.5; };
  return r;
}

@addMethod(PlayerPuppet)
public func VRMeleeBladeHit(weapon: wref<WeaponObject>, bladeStart: Vector4, bladeDir: Vector4, hitRadius: Float, strong: Bool) -> Int32 {
  if !IsDefined(weapon) { return 0; };
  // Holster guard: while a melee holster was just requested, reaching the hand to the holster zone is
  // motion that would trip the swing threshold and fire an attack — which leaves melee in an attack/busy
  // state and makes the UnequipWeapon get dropped. Suppress swings for the holster window so the unequip
  // lands. (This is why script-unequip "didn't work" for melee but the keyboard B does: B isn't a swing.)
  if this.VRHolsterIntentActive(1.0) { return 0; };
  let meleeAttack = weapon.VRGetMeleeAttack();
  if !IsDefined(meleeAttack) { return 0; };

  // blade segment = grip + direction * blade length. The game's attack Range() includes a lunge
  // (2-3 m) which makes the segment long enough to clip NEIGHBOURING NPCs ("I cut one, another gets
  // hit"); cap to ~physical blade length so only what the blade actually touches is hit.
  let reach = weapon.VRMeleeReach();
  if reach > 1.25 { reach = 1.25; };
  let dir = Vector4.Normalize(bladeDir);
  let bladeEnd = new Vector4(bladeStart.X + dir.X*reach, bladeStart.Y + dir.Y*reach, bladeStart.Z + dir.Z*reach, 1.0);

  // NPCs near the blade
  let q: TargetSearchQuery;
  q.testedSet = TargetingSet.Complete;
  q.searchFilter = TSF_NPC();
  q.maxDistance = Vector4.Distance(bladeStart, bladeEnd) + hitRadius + 2.0;
  q.filterObjectByDistance = true;
  q.ignoreInstigator = true;
  let parts: array<TS_TargetPartInfo>;
  GameInstance.GetTargetingSystem(this.GetGame()).GetTargetParts(this, q, parts);

  // closest enemy whose body is within hitRadius of the blade segment
  let bestEnemy: wref<GameObject>;
  let bestPos: Vector4;
  let bestDist = 99999.0;
  let i = 0;
  while i < ArraySize(parts) {
    let comp = TS_TargetPartInfo.GetComponent(parts[i]);
    if IsDefined(comp) {
      let ent = comp.GetEntity() as GameObject;
      if IsDefined(ent) && !ent.IsDead() {
        let d = VRMeleeEnemyDist(ent, bladeStart, bladeEnd);
        if d <= hitRadius && d < bestDist {
          bestDist = d;
          bestEnemy = ent;
          let bp = ent.GetWorldPosition();
          bestPos = new Vector4(bp.X, bp.Y, bp.Z + 1.0, 1.0);
        }
      }
    }
    i += 1;
  }
  // BLADE-CONTACT DETECTION + per-NPC cooldown.
  //   * CET gates THIS call by a "swing speed" threshold on the weapon vs player — so a stationary
  //     controller (blade resting in an NPC body, body micro-motion only) is NEVER a hit. It also means
  //     a real swing (>1.5 m/s) is required to register, even if the blade is on contact.
  //   * This function fires a hit IF the per-NPC cooldown (0.2s on the same NPC) has elapsed.
  //   * Fast slash through NPC1 -> NPC2 -> NPC3 -> 3 hits (each is a different ID, no cooldown applies).
  //   * Repeated slashes on the same NPC -> 1 hit every 0.2s (5 hits/sec max on one target).
  if !IsDefined(bestEnemy) { return 0; }
  let curId = bestEnemy.GetEntityID();
  let now = EngineTime.ToFloat(GameInstance.GetSimTime(this.GetGame()));
  let sameNpc = EntityID.IsDefined(this.m_vrMeleeLastTouched) && this.m_vrMeleeLastTouched == curId;
  if sameNpc && (now - this.m_vrMeleeLastHitTime) < 0.2 {
    return 0;
  }
  this.m_vrMeleeLastTouched = curId;
  this.m_vrMeleeLastHitTime = now;

  // Damage = the weapon's own per-hit value, so this works for ANY melee weapon (katana, knife, fists,
  // hammer). x2 on a strong (held-trigger) swing.
  let stats = GameInstance.GetStatsSystem(this.GetGame());
  let dmg = stats.GetStatValue(Cast<StatsObjectID>(weapon.GetEntityID()), gamedataStatType.EffectiveDamagePerHit);
  if dmg <= 0.0 { dmg = stats.GetStatValue(Cast<StatsObjectID>(weapon.GetEntityID()), gamedataStatType.DamagePerHit); };
  if dmg <= 0.0 { dmg = 50.0; };
  if strong { dmg *= 2.0; };

  // Build a real hit on the touched enemy and queue it through the native DamageSystem. The native
  // compute zeroes the attackComputed of a script hit (a plain melee record carries no damage value out
  // of combat state — diag proved this: we filled 8429, it came back 0). So we re-inject `dmg` in the
  // CalculateDamageVariants wrap below — that runs AFTER the native compute but BEFORE the pipeline flags
  // DealNoDamage — and the native stages (armor / crit / damage numbers / hit markers / ApplyDamage) then
  // run on our value. THIS is what makes the damage fully native (real numbers, markers, armor, crit) on
  // blade contact, for the exact NPC the blade touched.
  let hitEvent = new gameHitEvent();
  hitEvent.attackData = new AttackData();
  let ctx: AttackInitContext;
  ctx.record = meleeAttack.GetRecord();
  ctx.instigator = this;
  ctx.source = this;
  ctx.weapon = weapon;
  hitEvent.attackData.SetAttackDefinition(IAttack.Create(ctx));
  hitEvent.attackData.SetInstigator(this);
  hitEvent.attackData.SetSource(this);
  hitEvent.attackData.SetWeapon(weapon);
  // PreAttack pulls attackType (and hit-reaction severity) from the attack record. Without it
  // attackType stays at default and our melee gate in the DamageSystem wrap would reject our own hit.
  hitEvent.attackData.PreAttack();
  hitEvent.attackComputed = new gameAttackComputed();
  hitEvent.target = bestEnemy;
  hitEvent.hitPosition = bestPos;
  hitEvent.hitDirection = dir;

  // tag the hit as ours (the pipeline wrap re-injects dmg for it). COUNTER (not bool) so multiple in-flight
  // hits — e.g. fast slash through several NPCs in the same frame — all get the damage; the wrap
  // decrements per processed hit. The dmg value itself is stored on the player too (per-weapon, stable).
  this.m_vrMeleeDmg = dmg;
  this.m_vrMeleePending += 1;

  GameInstance.GetDamageSystem(this.GetGame()).QueueHitEvent(hitEvent, bestEnemy);
  return 1;
}

// VR-melee state on the player:
//   m_vrMeleeDmg          - raw damage to re-inject for the next N pipeline runs (per-weapon, stable)
//   m_vrMeleePending      - COUNT of in-flight hits we queued and haven't seen at PreProcess yet
//   m_vrMeleeLastTouched  - last NPC the blade was touching (per-NPC cooldown key)
//   m_vrMeleeLastHitTime  - sim time of the last hit on m_vrMeleeLastTouched (drives the 0.2s cooldown)
@addField(PlayerPuppet) let m_vrMeleeDmg: Float;
@addField(PlayerPuppet) let m_vrMeleePending: Int32;
@addField(PlayerPuppet) let m_vrMeleeLastTouched: EntityID;
@addField(PlayerPuppet) let m_vrMeleeLastHitTime: Float;

// Re-inject our damage at the FIRST point the damage pipeline touches attackComputed
// (CalculateDamageVariants, inside PreProcess) — after the native compute zeroed it, but BEFORE
// ModifyHitData would flag DealNoDamage for a 0-damage hit. Then armor / crit / damage numbers / hit
// markers / ApplyDamage all run NATIVELY on our value. Identified by the player's pending flag (set in
// VRMeleeBladeHit just before QueueHitEvent), so it stays weapon-agnostic and uses no hit-flag hacks.
@wrapMethod(DamageSystem)
private func CalculateDamageVariants(hitEvent: ref<gameHitEvent>) -> Void {
  if IsDefined(hitEvent.attackData) && IsDefined(hitEvent.attackComputed) {
    // gate: player is the instigator, the attack is a MELEE attack (so a gun/grenade/quickhack hit
    // doesn't get our melee damage), and we still have queued VR-melee hits to claim.
    let player = hitEvent.attackData.GetInstigator() as PlayerPuppet;
    if IsDefined(player) && player.m_vrMeleePending > 0
       && AttackData.IsMelee(hitEvent.attackData.GetAttackType()) {
      hitEvent.attackComputed.SetAttackValue(player.m_vrMeleeDmg, gamedataDamageType.Physical);
      player.m_vrMeleePending -= 1;
    }
  }
  wrappedMethod(hitEvent);
}
