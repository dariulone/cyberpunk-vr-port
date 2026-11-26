// Rendered marks use fragments of the owner's private skinned mesh. The
// physics contact identifies the slot. Finite blade rays select the triangles.
native func VRAttachToSlot(entity: ref<IScriptable>, parent: ref<IScriptable>, slot: CName) -> Bool;
public native func VRBladeQueueMeshSweep(target: ref<Entity>, bone: CName, from: Vector4, to: Vector4, normal: Vector4, depth: Float,
  serial: Uint32, oldStart: Vector4, oldEnd: Vector4, start: Vector4, end: Vector4) -> Uint32;
public native func VRBladeQueueSurfaceMark(target: ref<Entity>, bone: CName, from: Vector4, to: Vector4, normal: Vector4, depth: Float,
  serial: Uint32, oldStart: Vector4, oldEnd: Vector4, start: Vector4, end: Vector4, kind: Int32) -> Uint32;
public native func VRBladePaintPoint(target: ref<Entity>, bone: CName, serial: Uint32) -> array<Vector4>;

@addField(NPCPuppet) private let vrBladeMarkReason: CName;
@addField(NPCPuppet) private let vrBladeMarkShape: CName;
@addField(VrBladeReceipt) public let vrMeshRequests: Uint32;
@addField(VrBladeReceipt) private let vrTrailPoint: Vector4;
@addField(VrBladeReceipt) private let vrTrailNormal: Vector4;
@addField(VrBladeReceipt) private let vrTrailDeadline: Float;
@addField(VrBladeReceipt) private let vrTrailSegments: Int32;
@addField(VrBladeReceipt) private let vrTrailReady: Bool;
@addField(VrBladeReceipt) private let vrTrailOnMesh: Bool;

@if(ModuleExists("Codeware"))
private func VRBladePaintLocal(r: ref<VrBladeReceipt>) -> array<Vector4> {
  let local: array<Vector4>;
  let slots=r.slots.slots;
  for slot in slots {
    if Equals(slot.slotName,r.slot) {
      let sample=VRBladePaintPoint(r.target,slot.boneName,r.serial);
      if ArraySize(sample)==2 {
        let inverse=Quaternion.Conjugate(slot.relativeRotation);
        let p=Quaternion.Transform(inverse,sample[0]-Vector4(slot.relativePosition.X,slot.relativePosition.Y,slot.relativePosition.Z,0));
        let n=Quaternion.Transform(inverse,sample[1]);
        p.W=sample[0].W; n.W=sample[1].W;
        ArrayPush(local,p);ArrayPush(local,n);
      }
      break;
    }
  }
  return local;
}
@if(!ModuleExists("Codeware"))
private func VRBladePaintLocal(r: ref<VrBladeReceipt>) -> array<Vector4> { let empty: array<Vector4>; return empty; }

@addMethod(WeaponObject)
public func VRBladeRenderedMarkPosition() -> Vector4 {
  let r=this.vrBladeReceipt;
  let frame: WorldTransform;
  if IsDefined(r) && IsDefined(r.slots) && r.slots.GetSlotTransform(r.slot,frame) {
    let sample=VRBladePaintLocal(r);
    if ArraySize(sample)==2 {
      let p=sample[0];p.W=1.0;
      let world=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,p));world.W=1.0;
      return world;
    }
  }
  return Vector4(0,0,0,0);
}

@addMethod(NPCPuppet)
public func VRBladeMarkDiagnostic() -> String {
  return "last=" + NameToString(this.vrBladeMarkReason) + " shape=" + NameToString(this.vrBladeMarkShape);
}

@if(ModuleExists("Codeware"))
private func VRBladeCutSurface(r: ref<VrBladeReceipt>, start: Vector4, end: Vector4, normal: Vector4, depth: Float,
  bladeA: Vector4, bladeB: Vector4, kind: Int32, opt shellDepth: Float) -> Uint32 {
  let slots = r.slots.slots;
  let index = 0;
  while index < ArraySize(slots) {
    let slot = slots[index];
    if Equals(slot.slotName,r.slot) {
      let turn=slot.relativeRotation;
      let offset=Vector4(slot.relativePosition.X,slot.relativePosition.Y,slot.relativePosition.Z,0.0);
      let boneNormal=Quaternion.Transform(turn,normal);
      boneNormal.W=shellDepth;
      return VRBladeQueueSurfaceMark(r.target,slot.boneName,Quaternion.Transform(turn,start)+offset,
        Quaternion.Transform(turn,end)+offset,boneNormal,depth,r.serial,
        Quaternion.Transform(turn,r.meshStart)+offset,Quaternion.Transform(turn,r.meshEnd)+offset,
        Quaternion.Transform(turn,bladeA)+offset,Quaternion.Transform(turn,bladeB)+offset,kind);
    }
    index += 1;
  }
  return 0u;
}
@if(!ModuleExists("Codeware"))
private func VRBladeCutSurface(r: ref<VrBladeReceipt>, start: Vector4, end: Vector4, normal: Vector4, depth: Float,
  bladeA: Vector4, bladeB: Vector4, kind: Int32, opt shellDepth: Float) -> Uint32 { return 0u; }

// The confirmed shape and the entry are in the same animated slot. The
// entry-to-exit chord can be almost tangent to the shell on a grazing hit.
private func VRBladeCapsuleNormal(r: ref<VrBladeReceipt>) -> Vector4 {
  if ArraySize(r.capsule)!=2 { return Vector4(0,0,0,0); }
  let axis=r.capsule[1];
  let halfHeight=axis.W;
  axis.W=0.0;
  let radial=r.partPoint-r.capsule[0];
  radial.W=0.0;
  if halfHeight>0.0 {
    if Vector4.Length(axis)<0.001 { return Vector4(0,0,0,0); }
    axis=Vector4.Normalize(axis);
    radial-=axis*ClampF(Vector4.Dot(radial,axis),-halfHeight,halfHeight);
  }
  let distance=Vector4.Length(radial);
  if distance<0.001 { return Vector4(0,0,0,0); }
  radial=radial*(1.0/distance);
  radial.W=distance;
  return radial;
}

public func VRBladeAddSurfaceMark(target: ref<NPCPuppet>, shape: HitShapeData, motion: Vector4, r: ref<VrBladeReceipt>, opt impact: ref<VrBladeImpact>) -> Bool {
  let data=shape.userData as HitShapeUserDataBase;
  let frame: WorldTransform;
  let normal=shape.result.hitPositionEnter-shape.result.hitPositionExit;
  let tangent: Vector4;
  if !IsDefined(target) || !target.IsAttached() || !IsDefined(data) || NotEquals(data.GetShapeType(),EHitShapeType.Flesh)
    || !IsDefined(r.slots) || !r.slots.GetSlotTransform(r.slot,frame) { return false; }
  normal.W=0.0;
  normal=Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(frame)),normal);
  let capsuleNormal=VRBladeCapsuleNormal(r);
  let shellDepth=capsuleNormal.W;
  if shellDepth>0.0 {
    normal=capsuleNormal; normal.W=0.0;
  } else {
    if Vector4.Length(normal)<0.001 || Vector4.Length(normal)>1.25 { normal=r.partPoint; normal.W=0.0; }
    if Vector4.Length(normal)<0.001 { normal=Vector4(0,1,0,0); }
    normal=Vector4.Normalize(normal);
  }
  motion.W=0.0; tangent=Quaternion.Transform(Quaternion.Conjugate(WorldTransform.GetOrientation(frame)),motion);
  tangent-=normal*Vector4.Dot(tangent,normal);
  if Vector4.Length(tangent)<0.001 {
    tangent=AbsF(normal.Z)<0.8 ? Vector4(-normal.Y,normal.X,0,0) : Vector4(0,-normal.Z,normal.Y,0);
  }
  tangent=Vector4.Normalize(tangent);
  let endA=r.meshStart;let endB=r.meshEnd;
  if IsDefined(impact) {
    r.meshStart=WorldTransform.TransformInvPoint(frame,impact.previousA);
    r.meshEnd=WorldTransform.TransformInvPoint(frame,impact.previousB);
    endA=WorldTransform.TransformInvPoint(frame,impact.endA);endB=WorldTransform.TransformInvPoint(frame,impact.endB);
  }
  r.vrTrailPoint=VrCyberwareContactMath.Closest(r.partPoint,endA,endB);
  r.vrTrailNormal=normal; r.vrTrailReady=true;
  r.vrMeshRequests=VRBladeCutSurface(r,r.partPoint-tangent*0.015,r.partPoint+tangent*0.015,normal,0.015,
    endA,endB,r.puncture ? 2 : 1,shellDepth);
  r.meshStart=endA; r.meshEnd=endB;
  target.vrBladeMarkShape=shape.hitShapeName;
  target.vrBladeMarkReason=r.vrMeshRequests==0u ? n"NoMesh" : (r.puncture ? n"PunctureRequested" : n"SlashRequested");
  return r.vrMeshRequests>0u;
}

public func VRBladeContinueTrail(current: ref<VrBladeReceipt>, previous: ref<VrBladeReceipt>) -> Void {
  if !IsDefined(previous) || current.target!=previous.target || NotEquals(current.slot,previous.slot)
    || NotEquals(current.shapeName,previous.shapeName) { return; }
  current.vrTrailPoint=previous.vrTrailPoint; current.vrTrailNormal=previous.vrTrailNormal;
  current.vrTrailDeadline=previous.vrTrailDeadline; current.vrTrailSegments=previous.vrTrailSegments;
  current.vrTrailReady=previous.vrTrailReady; current.meshStart=previous.meshStart; current.meshEnd=previous.meshEnd;
  current.vrTrailOnMesh=previous.vrTrailOnMesh;
}

@addMethod(WeaponObject)
public func VRBladeTrailSegments() -> Int32 {
  return IsDefined(this.vrBladeReceipt) ? this.vrBladeReceipt.vrTrailSegments : 0;
}
@addMethod(WeaponObject)
public func VRBladeTraceSurface(serial: Uint32, a: Vector4, b: Vector4) -> Bool {
  let r=this.vrBladeReceipt;
  let frame: WorldTransform;
  if !IsDefined(r) || r.serial!=serial || !r.vrTrailReady || !IsDefined(r.owner) || !IsDefined(r.target) || !r.target.IsAttached()
    || !r.owner.VRBladeAllowed(this) || !IsDefined(r.slots) || !r.slots.GetSlotTransform(r.slot,frame) { return false; }
  let now=EngineTime.ToFloat(GameInstance.GetSimTime(r.owner.GetGame()));
  if now<r.time || now<r.vrTrailDeadline { return false; }
  let localA=WorldTransform.TransformInvPoint(frame,a);
  let localB=WorldTransform.TransformInvPoint(frame,b);
  if !r.vrTrailOnMesh && r.vrTrailSegments==0 {
    let sample=VRBladePaintLocal(r);
    if ArraySize(sample)==2 {
      r.vrTrailPoint=sample[0]; r.vrTrailPoint.W=1.0;
      r.vrTrailNormal=sample[1]; r.vrTrailNormal.W=0.0;
      r.vrTrailOnMesh=true;
    }
  }
  let point=VrCyberwareContactMath.Closest(r.vrTrailPoint,localA,localB);
  let delta=point-r.vrTrailPoint; delta.W=0.0;
  let normal=r.vrTrailNormal;
  let tangent=delta-normal*Vector4.Dot(normal,delta);
  if Vector4.Length(tangent)<0.018 { return false; }
  r.vrTrailDeadline=now+0.025;
  if Vector4.Length(delta)>0.15 {
    r.vrTrailPoint=point; r.meshStart=localA; r.meshEnd=localB; return false;
  }
  let worldMotion=Quaternion.Transform(WorldTransform.GetOrientation(frame),delta);
  r.vrMeshRequests=VRBladeCutSurface(r,r.vrTrailPoint,point,normal,ClampF(this.VRBladeMeasure(serial,a,b,worldMotion).X,0.012,0.15),localA,localB,3);
  r.vrTrailPoint=point;r.meshStart=localA;r.meshEnd=localB;
  if r.vrMeshRequests==0u { return false; }
  r.vrTrailSegments+=1;
  return true;
}
