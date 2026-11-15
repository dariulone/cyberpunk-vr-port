// PEOPLE IN FRONT OF THE HUD (include/Runtimes/HudClearance.hpp, mods/cet/CyberpunkVRPort_Stereo/modules/hud_clearance.lua).
//
// The clearance rays are static-only, so they skip V's own body -- and with it everybody else. People are
// found here instead, the way the melee module finds them: the targeting system's NPC query around the
// player, V excluded. Each one is a standing body, a vertical column of radius RADIUS sampled at five
// heights. A sample inside the HUD's area (|side| <= tanH * depth, |vertical| <= tanV * depth, widened by
// the radius) that is nearer than anything so far gives depth - RADIUS: the front of that body, along the
// camera's forward axis. 99 = nobody in the way.
@addMethod(PlayerPuppet)
public func VRHudPeopleDepth(origin: Vector4, forward: Vector4, right: Vector4, up: Vector4, tanH: Float, tanV: Float, maxDist: Float) -> Float {
  let radius: Float = 0.30;
  let best: Float = 99.0;
  let q: TargetSearchQuery;
  q.testedSet = TargetingSet.Complete;
  q.searchFilter = TSF_NPC();
  q.maxDistance = maxDist;
  q.filterObjectByDistance = true;
  q.ignoreInstigator = true;
  let parts: array<TS_TargetPartInfo>;
  GameInstance.GetTargetingSystem(this.GetGame()).GetTargetParts(this, q, parts);
  let seen: array<EntityID>;
  let i: Int32 = 0;
  while i < ArraySize(parts) {
    let comp = TS_TargetPartInfo.GetComponent(parts[i]);
    if IsDefined(comp) {
      let ent = comp.GetEntity() as GameObject;
      if IsDefined(ent) && !ArrayContains(seen, ent.GetEntityID()) {
        ArrayPush(seen, ent.GetEntityID());
        let base: Vector4 = ent.GetWorldPosition();
        let k: Int32 = 0;
        while k < 5 {
          let p: Vector4 = Vector4(base.X, base.Y, base.Z + 0.20 + 0.35 * Cast<Float>(k), 1.00);
          let d: Vector4 = Vector4(p.X - origin.X, p.Y - origin.Y, p.Z - origin.Z, 0.00);
          let depth: Float = Vector4.Dot(d, forward);
          if depth > 0.05 {
            let side: Float = AbsF(Vector4.Dot(d, right)) - radius;
            let vertical: Float = AbsF(Vector4.Dot(d, up)) - radius;
            if side <= tanH * depth && vertical <= tanV * depth {
              let front: Float = depth - radius;
              if front < best {
                best = front;
              };
            };
          };
          k += 1;
        };
      };
    };
    i += 1;
  };
  return best;
}
