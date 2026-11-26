// Cosmetic rig marker for the VR interaction, not another combat hitbox. In
// the stock man_base / woman_base A-pose, Neck1 is about three quarters of
// the way along the Spine3 -> Head segment. Interpolating those two live attachment
// slots also follows ordinary pose/scale changes without mutating any NPC asset.
@addMethod(NPCPuppet)
public func VRBladeServicePort() -> Vector4 {
  let slots = this.FindComponentByName(n"Item_Attachment_Slot") as SlotComponent;
  let shapes = this.FindComponentByName(n"HitRepresentation") as SlotComponent;
  let lower: WorldTransform;
  let upper: WorldTransform;
  let a: Vector4;
  let b: Vector4;
  let marker: Vector4;
  let span: Float;
  if IsDefined(shapes) && shapes.GetSlotTransform(n"Neck",upper) {
    marker = WorldPosition.ToVector4(WorldTransform.GetWorldPosition(upper)); marker.W=1.0; return marker;
  };
  if this.vrBladeNeckConfigured && IsDefined(shapes) && shapes.GetSlotTransform(n"Head",upper) {
    marker=WorldPosition.ToVector4(WorldTransform.TransformPoint(upper,Vector4(-0.070,0.0135,0.0,1.0)));
    marker.W=1.0; return marker;
  };
  if !IsDefined(slots) || !slots.GetSlotTransform(n"Chest",lower) || !slots.GetSlotTransform(n"Head",upper) { return Vector4(0,0,0,0); };
  a=WorldPosition.ToVector4(WorldTransform.GetWorldPosition(lower));
  b=WorldPosition.ToVector4(WorldTransform.GetWorldPosition(upper));
  a.W=1.00; b.W=1.00; span=Vector4.Distance(a,b);
  if !(span > 0.14 && span < 0.50) { return Vector4(0,0,0,0); };
  marker = a + (b-a)*0.75; marker.W=1.00; return marker;
}

@addField(NPCPuppet) public let vrBladeNeckConfigured: Bool;
