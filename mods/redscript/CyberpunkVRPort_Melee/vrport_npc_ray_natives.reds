// CyberpunkVRPort -- global native declaration for the NPC hit-representation surface query.
//
// This file intentionally has no module declaration. The plugin registers a plain global
// QueryVRNpcHitSurface; declaring it inside CyberpunkVRPort.Melee would make redscript look for a
// module-qualified native that does not exist.

native func QueryVRNpcHitSurface(entity: ref<GameObject>, from: Vector4, to: Vector4) -> Vector4;

// Find the closest NPC body surface on the current muzzle ray. Targeting only supplies nearby
// candidates; the native query intersects each unique entity's live animated HitRepresentation.
@addMethod(PlayerPuppet)
public func VRFindNpcBarrelRayHit(rayStart: Vector4, rayEnd: Vector4) -> Vector4 {
  let q: TargetSearchQuery;
  q.testedSet = TargetingSet.Complete;
  q.searchFilter = TSF_NPC();
  q.maxDistance = 60.0;
  q.filterObjectByDistance = true;
  q.ignoreInstigator = true;
  let parts: array<TS_TargetPartInfo>;
  GameInstance.GetTargetingSystem(this.GetGame()).GetTargetParts(this, q, parts);

  let seen: array<EntityID>;
  let best = new Vector4(0.0, 0.0, 0.0, 0.0);
  let bestDistanceSq = 999999999.0;
  let i = 0;
  while i < ArraySize(parts) {
    let comp = TS_TargetPartInfo.GetComponent(parts[i]);
    if IsDefined(comp) {
      let ent = comp.GetEntity() as GameObject;
      if IsDefined(ent) {
        let entityId = ent.GetEntityID();
        let duplicate = false;
        let j = 0;
        while j < ArraySize(seen) {
          if seen[j] == entityId { duplicate = true; };
          j += 1;
        };
        if !duplicate {
          ArrayPush(seen, entityId);
          let hit = QueryVRNpcHitSurface(ent, rayStart, rayEnd);
          if hit.W > 0.5 {
            let dx = hit.X - rayStart.X;
            let dy = hit.Y - rayStart.Y;
            let dz = hit.Z - rayStart.Z;
            let distanceSq = dx*dx + dy*dy + dz*dz;
            if distanceSq < bestDistanceSq {
              bestDistanceSq = distanceSq;
              best = hit;
            };
          };
        };
      };
    };
    i += 1;
  };
  if best.W < 0.5 {
    if ArraySize(parts) == 0 {
      best.W = -1.0; // TargetingSystem returned no NPC target parts.
    } else {
      best.W = -3.0; // Unique NPC candidates existed, but no body surface intersected.
    };
  };
  return best;
}
