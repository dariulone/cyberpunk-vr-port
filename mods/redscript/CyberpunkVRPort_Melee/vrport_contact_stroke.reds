// A controller stroke can enter a target between samples or start in contact.
// Begin its combat episode only after the finite sweep finds an eligible NPC.
@addMethod(PlayerPuppet)
public func VRBladeContactStroke(w: wref<WeaponObject>, prevA: Vector4, prevB: Vector4, a: Vector4, b: Vector4, opt trackedMotion: Vector4) -> Int32 {
  if !IsDefined(w) || w.VRBladeKind() <= 0 || this.VRHolsterIntentActive(1.00) { return 0; };
  return this.VRBladeSweep(w,prevA,prevB,a,b,false,true,trackedMotion);
}

@addMethod(PlayerPuppet)
public func VRPhysicalMeleeWeapon() -> wref<WeaponObject> {
  let ts=GameInstance.GetTransactionSystem(this.GetGame());
  if !IsDefined(ts) { return null; };
  return ts.GetItemInSlot(this,VRLeftHandedWeapon.HeldSlot(this)) as WeaponObject;
}

@addMethod(WeaponObject)
public func VRBladeContactHeld() -> Bool { return IsDefined(this.vrBladeTouch); }

@addMethod(WeaponObject)
public func VRBladeEntryBlocked() -> Bool {
  let r=this.vrBladeReceipt;
  if !IsDefined(r) || r.released || !IsDefined(r.target) || !r.target.IsAttached() || !IsDefined(r.owner) { return false; };
  let elapsed=VRMeleeRealTime()-r.lodgedAt;
  if (r.awaitingSeat || r.lodged) && elapsed>=0.0 && elapsed<0.25 { return true; };
  if r.lodged && VRBladeJointPoint(VRLeftHandedWeapon.InHand(r.owner) ? 0 : 1,r.jointSerial).W>0.5 { return true; };
  return IsDefined(r.pending) && !r.pending.cancelled && EngineTime.ToFloat(GameInstance.GetSimTime(r.owner.GetGame()))-r.pending.created<0.20;
}

@addMethod(WeaponObject)
public func VRBladeLodgeContact(serial: Uint32) -> Void {
  let r=this.vrBladeReceipt;
  if IsDefined(r) && r.serial==serial && (r.puncture || r.lodged) && !r.released {
    if !r.lodged || this.vrResumeBladeHoldJoint == r.jointSerial {
      this.vrResumeBladeHoldJoint = 0u;
      this.VRBeginBladeMovementHold(r);
    };
    r.lodged=true; r.awaitingSeat=false; r.lodgedAt=VRMeleeRealTime();
  };
}

@addMethod(WeaponObject)
public func VRBladeReleaseContact(serial: Uint32) -> Void {
  let r=this.vrBladeReceipt;
  if !IsDefined(r) || r.serial!=serial { return; };
  if r.lodged { this.VREndBladeMovementHold(r); };
  r.released=true; r.lodged=false; r.awaitingSeat=false;
  if IsDefined(r.pending) { r.pending.cancelled=true; r.pending=null; };
  if IsDefined(r.entry) { r.entry.expired=true; };
  if IsDefined(r.entry) && IsDefined(this.vrBladeAttempt) && this.vrBladeAttempt.entry==r.entry {
    this.vrBladeTouch=null;
    this.vrBladeAttempt=null;
  };
}

@addMethod(WeaponObject)
public func VRPhysicalWeaponRecordReady() -> Bool {
  let record=this.GetWeaponRecord();
  return IsDefined(record) && IsDefined(record.ItemType());
}
