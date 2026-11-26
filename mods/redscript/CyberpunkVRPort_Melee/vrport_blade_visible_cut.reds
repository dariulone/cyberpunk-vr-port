// All endpoints in a sample are observations of the visible weapon. Store
// them in the animated frames at observation time, before either actor moves.
public native func VRBladeSlotEdgeSample(side: Int32, serial: Uint32) -> array<Vector4>;
public native func VRBladeSameGeometry(first: ref<ISerializable>, second: ref<ISerializable>) -> Bool;
public native func VRBladeSameHitData(first: ref<HitShapeUserData>, second: ref<HitShapeUserData>) -> Bool;
public native func VRBladeCollectOccupied(component: ref<HitRepresentationComponent>, from: Vector4, to: Vector4, edge: Vector4,
  pinSlot: CName, pinFrame: WorldTransform, sink: ref<IScriptable>) -> Vector4;
public native func VRBladeMatchesOccupied(component: ref<HitRepresentationComponent>, hit: HitShapeData, slot: CName,
  geometry: ref<ISerializable>, data: ref<HitShapeUserData>, material: CName, center: Vector4, axis: Vector4) -> Bool;

public class VrBladeObservedShape extends IScriptable {
  public let name: CName;
  public let slot: CName;
  public let geometry: ref<ISerializable>;
  public let data: ref<HitShapeUserData>;
  public let material: CName;
  public let capsule: array<Vector4>;
  public let a: Vector4;
  public let b: Vector4;
  public let fraction: Float;
  public let interval: Vector4;
  public let frame: WorldTransform;
}

public class VrBladeVisiblePose extends IScriptable {
  public let serial: Uint32;
  public let stroke: Uint32;
  public let at: Float;
  public let a: Vector4;
  public let b: Vector4;
  public let goal: Vector4;
  public let advance: Float;
  public let railOffset: Vector4;
  public let railAxis: Vector4;
  public let worldA: Vector4;
  public let worldB: Vector4;
  public let rotation: Quaternion;
  public let shapes: array<ref<VrBladeObservedShape>>;
  public let counts: Vector4;
  public let preferredFraction: Float;

  public func VRBladeKeepOccupied(name: CName, slot: CName, geometry: ref<ISerializable>, data: ref<HitShapeUserData>,
    material: CName, center: Vector4, axis: Vector4, frame: WorldTransform, a: Vector4, b: Vector4, interval: Vector4) -> Bool {
    let reach=Vector4.Distance(a,b);
    if !(reach>=0.04 && interval.W>0.5 && interval.Y>0.001) { return false; };
    let obs=new VrBladeObservedShape();
    obs.name=name;obs.slot=slot;obs.geometry=geometry;obs.data=data;obs.material=material;
    obs.capsule=[center,axis];obs.frame=frame;obs.a=a;obs.b=b;obs.interval=interval;
    obs.fraction=AbsF(this.preferredFraction-interval.X)*reach<interval.Y*0.5 ? this.preferredFraction : interval.X;
    if IsDefined(VrBladeVisibleCut.Prior(this,obs)) { return false; };
    ArrayPush(this.shapes,obs);
    return true;
  }
}

@addMethod(WeaponObject)
private func VRBladeRecordEdgePose(r: ref<VrBladeReceipt>, pin: ref<VrBladeSlotAttachment>, slotFrame: WorldTransform,
  a: Vector4, b: Vector4, edge: Vector4, goal: Vector4, side: Int32) -> Void {
  let path=VRBladeSlotEdgeSample(side,r.jointSerial);
  if ArraySize(path)!=2 || path[0].W<1.0 || !IsDefined(pin.slots) {
    ArrayClear(pin.edgeFrames);return;
  };
  let pose=new VrBladeVisiblePose();
  pose.stroke=Cast<Uint32>(path[0].W);
  pose.serial=r.serial;pose.at=VRMeleeRealTime();
  if pose.stroke==r.lastEdgeStroke { ArrayClear(pin.edgeFrames);return; };
  pose.railOffset=path[0];pose.railOffset.W=0.0;
  pose.railAxis=path[1];pose.railAxis.W=0.0;
  pose.advance=Vector4.Dot(pose.railOffset,pose.railAxis);
  pose.a=WorldTransform.TransformInvPoint(slotFrame,a);
  pose.b=WorldTransform.TransformInvPoint(slotFrame,b);
  pose.worldA=a;pose.worldA.W=1.0;pose.worldB=b;pose.worldB.W=1.0;
  pose.rotation=WorldTransform.GetOrientation(slotFrame);
  goal.W=1.0;
  pose.goal=WorldTransform.TransformInvPoint(slotFrame,goal);
  let n=ArraySize(pin.edgeFrames);
  if n>0 {
    let last=pin.edgeFrames[n-1];
    if last.serial!=r.serial || last.stroke!=pose.stroke || last.at>pose.at || pose.at-last.at>0.10 || pose.advance<last.advance {
      ArrayClear(pin.edgeFrames);
    } else {
      if last.at==pose.at { return; };
    };
  };
  pose.preferredFraction=pin.fraction;
  pose.counts=VRBladeCollectOccupied(pin.slots,a,b,edge,pin.slot,slotFrame,pose);
  while ArraySize(pin.edgeFrames)>0 && (pose.at-pin.edgeFrames[0].at>0.15 || ArraySize(pin.edgeFrames)>=24) {
    ArrayErase(pin.edgeFrames,0);
  };
  ArrayPush(pin.edgeFrames,pose);
}

public abstract class VrBladeVisibleCut {
  public static func UseCut(impact: ref<VrBladeImpact>, shape: ref<VrBladeCutShape>) -> Void {
    impact.cutShape=shape;
    impact.contactMetrics.X=shape.window.W;
    let frame=shape.observedFrame;
    impact.sweepFrom=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.from));
    impact.sweepTo=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.to));
    impact.sweepFrom.W=1.0;impact.sweepTo.W=1.0;
    impact.previousA=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.oldA));
    impact.previousB=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.oldB));
    impact.endA=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.endA));
    impact.endB=WorldPosition.ToVector4(WorldTransform.TransformPoint(frame,shape.endB));
  }
  public static func BoxHalfExtent() -> Float { return 0.035; }

  // The stock effect sweeps a cube. Require the entire starting cube to
  // clear the observed capsule, even at an oblique angle to its spine.
  public static func ExteriorEnd(obs: ref<VrBladeObservedShape>) -> Int32 {
    let center=obs.capsule[0];
    let axis=obs.capsule[1];
    let clearance=center.W+SqrtF(3.0)*VrBladeVisibleCut.BoxHalfExtent();
    let height=axis.W;
    center.W=1.0;axis.W=0.0;
    let half=Vector4.Normalize(axis)*height;
    let lo=center-half;
    let hi=center+half;
    let clearA=Vector4.Distance(obs.a,VrCyberwareContactMath.Closest(obs.a,lo,hi))>clearance;
    let clearB=Vector4.Distance(obs.b,VrCyberwareContactMath.Closest(obs.b,lo,hi))>clearance;
    if clearA { return clearB ? 3 : 1; };
    if clearB { return -1; };
    return 0;
  }
  public static func Station(a: Vector4, b: Vector4, f: Float) -> Vector4 {
    return a+(b-a)*f;
  }
  public static func Prior(pose: ref<VrBladeVisiblePose>, current: ref<VrBladeObservedShape>) -> ref<VrBladeObservedShape> {
    for obs in pose.shapes {
      if Equals(obs.name,current.name) && Equals(obs.slot,current.slot)
        && Equals(obs.material,current.material) && VRBladeSameHitData(obs.data,current.data)
        && Vector4.Distance(obs.capsule[0],current.capsule[0])==0.0 && Vector4.Distance(obs.capsule[1],current.capsule[1])==0.0
        && obs.capsule[0].W==current.capsule[0].W && obs.capsule[1].W==current.capsule[1].W { return obs; };
    };
    return null;
  }
  // X is signed solver travel over this pair of samples. Y and Z are signed
  // visible travel at exactly the same shaft fraction, in the pin and hit
  // shape frames respectively. W is their shared, direction-checked length.
  public static func Window(old: ref<VrBladeVisiblePose>, now: ref<VrBladeVisiblePose>, prior: ref<VrBladeObservedShape>,
    current: ref<VrBladeObservedShape>, pinToShape: Quaternion, intent: Float) -> Vector4 {
    let f=current.fraction;
    let physical=now.railOffset-old.railOffset;physical.W=0.0;
    let mapped=now.railAxis;
    let measured=Vector4.Dot(physical,mapped);
    let goal=now.goal-old.goal;goal.W=0.0;
    let pinMotion=VrBladeVisibleCut.Station(now.a,now.b,f)-VrBladeVisibleCut.Station(old.a,old.b,f);pinMotion.W=0.0;
    let motion=VrBladeVisibleCut.Station(current.a,current.b,f)-VrBladeVisibleCut.Station(prior.a,prior.b,f);motion.W=0.0;
    let distance=Vector4.Length(goal);
    if !(f>=0.0 && f<=1.0 && measured>=0.008 && measured<=0.35 && distance>=0.008 && distance<=0.35) { return Vector4(measured,0,0,0); };
    let goalTravel=Vector4.Dot(goal,mapped);
    let atPin=Vector4.Dot(pinMotion,mapped);
    let atShape=Vector4.Dot(motion,Quaternion.Transform(pinToShape,mapped));
    let pinLength=Vector4.Length(pinMotion);let length=Vector4.Length(motion);
    if !(measured>=0.96*Vector4.Length(physical) && Vector4.Length(physical)<=0.35 && goalTravel>=0.96*distance && pinLength<=0.35 && length<=0.35
      && atPin>=0.96*pinLength && atShape>=0.96*length) { return Vector4(measured,atPin,atShape,0); };
    let credit=MinF(intent,MinF(measured,MinF(goalTravel,MinF(atPin,atShape))));
    return Vector4(measured,atPin,atShape,credit>=0.008 ? credit : 0.0);
  }
}

@addMethod(WeaponObject)
private func VRBladeClipStations(impact: ref<VrBladeImpact>, stroke: Uint32, intent: Float) -> Bool {
  let pin=this.vrBladeReceipt.attachment;
  if !IsDefined(pin) || !IsDefined(pin.slots) || pin.slots!=this.vrBladeReceipt.slots { return this.VRBladeFollowResult(n"NoVisiblePin"); };
  let count=ArraySize(pin.edgeFrames);
  if count==0 { return this.VRBladeFollowResult(n"NoVisibleWindow"); };
  let now=pin.edgeFrames[count-1];
  this.vrBladeFollowCounts=now.counts;
  let age=VRMeleeRealTime()-now.at;
  if now.serial!=impact.parentSerial || now.stroke!=stroke || age<0.0 || age>.05 { return this.VRBladeFollowResult(n"StaleVisibleWindow"); };
  this.vrBladeFollowGeometry.W=Cast<Float>(ArraySize(now.shapes));
  if ArraySize(now.shapes)==0 {
    if now.counts.W<0.0 { return this.VRBladeFollowResult(n"ObservationUnavailable"); };
    if now.counts.X>0.0 {
      return this.VRBladeFollowResult(now.counts.Y>0.0 ? n"NoRetainedShape" : n"NoResolvedShape");
    };
    if now.counts.W>0.0 { return this.VRBladeFollowResult(n"UnresolvedSlots"); };
    return this.VRBladeFollowResult(n"VisibleBladeClear");
  };
  let latestA=impact.a;latestA.W=1.0;
  let latestB=impact.b;latestB.W=1.0;
  if Vector4.Distance(latestA,now.worldA)>0.001 || Vector4.Distance(latestB,now.worldB)>0.001 { return this.VRBladeFollowResult(n"StaleVisibleWindow"); };
  let best=0.0;
  let gate=n"NoVisibleWindow";
  for current in now.shapes {
      let frame=current.frame;
      let exterior=VrBladeVisibleCut.ExteriorEnd(current);
      let chord: ref<VrBladeCutShape>;
      chord=null;
      let pinToShape=Quaternion.Conjugate(WorldTransform.GetOrientation(frame))*now.rotation;
      let index=count-2;
      if !VrBladeRPG.ValidZoneData(current.data as HitShapeUserDataBase) { index=-1; };
      while index>=0 {
        let old=pin.edgeFrames[index];
        let prior=VrBladeVisibleCut.Prior(old,current);
        if !IsDefined(prior) { break; }; // never bridge missing shape occupancy
        let window=VrBladeVisibleCut.Window(old,now,prior,current,pinToShape,intent);
        if best==0.0 && window.X>=this.vrBladeFollowWindow.X { this.vrBladeFollowWindow=window; };
        if window.W>0.0 {
          gate=n"NoClippedInterval";
          let end=VrBladeVisibleCut.Station(current.a,current.b,current.fraction);
          let was=VrBladeVisibleCut.Station(prior.a,prior.b,current.fraction);
          let share=window.W/Vector4.Distance(was,end);
          let start=end+(was-end)*share;
          let shaft=Vector4.Normalize(current.b-current.a);
          let interval=VRBladeCapsuleInterval(start,end,Vector4(0,0,0,0),shaft,current.capsule[0],current.capsule[1],this.vrBladeReceipt.material);
          if interval.W>0.5 && interval.Y>0.001 && (!IsDefined(chord) || interval.Y>chord.window.W) {
            if exterior==0 { gate=n"NoExteriorStart";break; };
            let half=interval.Y/(2.0*window.W);
            let shape=new VrBladeCutShape();
            shape.name=current.name;shape.slot=current.slot;shape.capsule=current.capsule;
            shape.observation=current;
            shape.reverseQuery=exterior<0;
            shape.eitherEnd=exterior==3;
            shape.window=Vector4(window.X,window.Y,window.Z,interval.Y);
            shape.observedFrame=frame;
            let shaftHalf=current.interval.Y/(2.0*Vector4.Distance(current.a,current.b));
            shape.shaftFrom=VrBladeVisibleCut.Station(current.a,current.b,ClampF(current.interval.X-shaftHalf,0.0,1.0));
            shape.shaftTo=VrBladeVisibleCut.Station(current.a,current.b,ClampF(current.interval.X+shaftHalf,0.0,1.0));
            let first=ClampF(interval.X-half,0.0,1.0);let last=ClampF(interval.X+half,0.0,1.0);
            shape.from=VrBladeVisibleCut.Station(start,end,first);shape.to=VrBladeVisibleCut.Station(start,end,last);
            shape.oldA=VrBladeVisibleCut.Station(current.a,prior.a,share*(1.0-first));
            shape.oldB=VrBladeVisibleCut.Station(current.b,prior.b,share*(1.0-first));
            shape.endA=VrBladeVisibleCut.Station(current.a,prior.a,share*(1.0-last));
            shape.endB=VrBladeVisibleCut.Station(current.b,prior.b,share*(1.0-last));
            chord=shape;
          };
        } else { if Equals(gate,n"NoVisibleWindow") { gate=n"StationMotion"; }; };
        index-=1;
      };
      if IsDefined(chord) {
        ArrayPush(impact.cutCandidates,chord);
        if chord.window.W>best { best=chord.window.W;impact.cutShape=chord;this.vrBladeFollowWindow=chord.window; };
      };
  };
  if best<=0.0 { return this.VRBladeFollowResult(gate); };
  impact.a=now.worldA;impact.b=now.worldB;
  impact.edgeStroke=stroke;
  impact.contactMetrics=Vector4(best,0,this.vrBladeFollowGeometry.Y,0);
  VrBladeVisibleCut.UseCut(impact,impact.cutShape);
  return true;
}
