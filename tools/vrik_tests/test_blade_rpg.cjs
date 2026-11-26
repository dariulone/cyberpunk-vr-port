// node tools/vrik_tests/test_blade_rpg.cjs
// Offline script-policy replay, not an engine/VR test. Execute the production
// method bodies against small game-object doubles; unknown API calls throw.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const dir = path.resolve(__dirname, '../../mods/redscript/CyberpunkVRPort_Melee');
const reds = fs.readFileSync(path.join(dir, 'vrport_blade_rpg.reds'), 'utf8');
const visibleCut = fs.readFileSync(path.join(dir, 'vrport_blade_visible_cut.reds'), 'utf8');
const enhanced = fs.readFileSync(path.join(dir, 'vrport_melee_enhanced.reds'), 'utf8');
const contact = fs.readFileSync(path.join(dir, 'vrport_blade_contact.reds'), 'utf8');
function body(src, name) {
  const start = src.indexOf('func ' + name + '(');
  assert.notEqual(start, -1, name);
  const open = src.indexOf('{', start); let depth = 1, i = open + 1;
  for (; i < src.length && depth !== 0; i++) depth += (src[i] === '{') - (src[i] === '}');
  assert.equal(depth, 0);
  return src.slice(open + 1, i - 1);
}
const ctx = vm.createContext({ MaxF: Math.max, MinF: Math.min,
  ClampF: (v, a, b) => Math.max(a, Math.min(b, v)), Equals: (a,b) => a===b,
  NotEquals: (a,b) => a!==b, IsDefined: x => x != null, EnumInt: x=>x,
  GetAllBlackboardDefs: () => ({PlayerStateMachine:{Takedown:'Takedown'}}), VrBladeRPG: {} });
for (const name of ['gamedataNPCRarity','gamePSMTakedown','EAIAttitude','gamedataNPCType','gameGodModeType','gamedataStatType','gameEPowerDifferential',
  'gamedataDamageType','gamedataStatPoolType','gameuiHitType','hitFlag','EHitShapeType','gamedataNPCHighLevelState']) {
  ctx[name] = new Proxy({}, {get: (_, k) => k});
}
function method(name, params, src=reds) {
  // No reimplementation of REDscript branches/constants: remove only type
  // syntax and add JS condition parentheses. Failure to parse fails the test.
  const js = body(src,name).replace(/\/\/[^\n]*/g, '').replace(/\blet (\w+): (?:Float|Vector4|WorldTransform|HitShapeData|ref<\w+>|array<HitShapeData>);/g,'let $1;')
    .replace(/\blet (\w+): Int32;/g,'let $1=0;').replace(/\blet (\w+): Int32 =/g,'let $1 =')
    .replace(/\s+as (?:HitShapeUserDataBase|HitRepresentationComponent|HitData_Base|Attack_GameEffect|PlayerPuppet|NPCPuppet|SlotComponent)/g,'').replace(/Cast<StatsObjectID>/g,'').replace(/\b[nt]"/g,'"')
    .replace(/for (\w+) in ([\w.]+) \{/g,'for (const $1 of $2) {')
    .replace(/Cast<(?:Uint32|Float)>/g,'Number').replace(/\bwhile ([^{}]+)\{/g,'while ($1) {')
    .replace(/\bif ([^{}]+)\{/g, 'if ($1) {').replace(/\b(\d+)u\b/g,'$1');
  return vm.runInContext(`(function(${params}) {${js}})`, ctx);
}
for (const [fn, params] of Object.entries({StealthRegion:'name',Budget:'current,maximum,floor,elite',FollowFactor:'strength',Rejected:'data',WoundTarget:'player,npc',SpecialTarget:'player,npc',ApplyRule:'hit,impact,player,npc,weapon'}))
  ctx.VrBladeRPG[fn] = method(fn,params);
ctx.NameToString = name => name ?? '';
ctx.StrContains = (name, part) => name.includes(part);
ctx.VrBladeRPG.MaterialId = method('MaterialId', 'material');
for(const [name, expected] of [['character_flesh_head.physmat',0],['cyber_limb',1],['steel',2],['metal_armor',3],['',4]])
  assert.equal(ctx.VrBladeRPG.MaterialId(name),expected);
const defaults = {current:100,maximum:100,floor:0,raw:10,other:0,grade:'Normal',det:0,high:'Relaxed',hostile:'AIA_Hostile',body:3,flags:['StealthHit'],intent:true,kind:1};
function replay(options={}) {
  const s={...defaults,...options}; let hp=s.current, physical=s.raw, applied=0;
  const stats={GetStatValue:(_,k)=>k==='Strength'?s.body:(s.stats?.[k]??0),GetStatBoolValue:(_,k)=>!!s.stats?.[k]};
  const pools={GetStatPoolValue:(_,k)=>k==='Health'?hp:(s.pools?.[k]??0), GetStatPoolMaxPointValue:()=>s.maximum,GetStatPoolValueCustomLimit:()=>s.floor};
  const game={GetStatsSystem:()=>stats, GetStatPoolsSystem:()=>pools,GetGodModeSystem:()=>({HasGodMode:(_,k)=>k===s.god}),
    GetWorkspotSystem:()=>({IsActorInWorkspot:()=>!!s.workspot}),GetStatusEffectSystem:()=>({HasStatusEffect:()=>!!s.noGrapple})};
  Object.assign(ctx,{GameInstance:game,GameObject:{GetAttitudeTowards:()=>s.hostile}, NPCManager:{HasTag:(_,k)=>k===s.tag},
    AttackData:{IsPlayerInCombat:()=>!!s.combat},RPGManager:{CalculatePowerDifferential:()=>s.difficulty??'NORMAL'}});
  const npc={GetEntityID:()=>2,GetRecordID:()=>3,GetNPCRarity:()=>s.grade,IsElite:()=>s.grade==='Elite',GetNPCType:()=>s.type??'Human',
    IsActive:()=>true,GetPuppetStateBlackboard:()=>({}),GetHighLevelStateFromBlackboard:()=>s.high,GetDetectionPercentage:()=>s.det};
  for(const name of ['IsDead','IsIncapacitated','IsAboutToDieOrDefeated','IsRagdolling','IsBoss','IsMassive','IsQuest','IsVendor','IsCrowd','IsCharacterCivilian','IsResistantToTakedown']) npc[name]=()=>s.npc===name;
  ctx.ScriptedPuppet={IsDefeated:()=>false};
  const player={GetGame:()=>game, GetEntityID:()=>1, VRBladeAllowed:()=>s.held!==false, GetPlayerStateMachineBlackboard:()=>({GetInt:()=>s.takedown??'Default'}),
    GetIsInFastFinisher:()=>false,GetIsInWorkspotFinisher:()=>false};
  ctx.VrBladeRPG.PortContact=(_,shape)=>ctx.VrBladeRPG.StealthRegion(shape.hitShapeName);
  ctx.HitShapeUserDataBase={IsHitReactionZoneTorso:d=>d.zone==='Torso',IsHitReactionZoneHead:d=>d.zone==='Head'};
  const hit={attackData:{GetMinimumHealthPercent:()=>s.minPercent??0, HasFlag:f=>s.flags.includes(f),WasBlocked:()=>!!s.blocked,
    WasDeflectedAny:()=>!!s.deflected,GetHitType:()=>s.hitType??'Hit'},
    attackComputed:{GetAttackValue:()=>physical,GetTotalAttackValue:()=>physical+s.other,SetAttackValue:v=>{physical=v;applied++;}},
    hitRepresentationResult:{hitShapes:[{hitShapeName:s.slot??'Neck',userData:{zone:s.zone,GetShapeType:()=>s.material??'Flesh',m_hitShapeDamageMod:s.shapeMod??1}}]}};
  const impact={thrust:!!s.thrust,stealthIntent:s.intent,rearAtStart:s.behind!==false,parentSerial:s.parent??0,stealthApplied:false,noDismember:s.short??false};
  ctx.VrBladeRPG.ApplyRule(hit,impact,player,npc,{VRBladeKind:()=>s.kind});
  // The fake stat pool can report an actual before/after delta. Its drain is
  // deliberately not a substitute for Cyberpunk's native pipeline/finisher grace.
  hp=Math.max(s.floor,hp-physical);
  return {delta:s.current-hp, budget:physical, applied,stealth:impact.stealthApplied,available:impact.availableHealth,strengthReaction:impact.strengthReaction,bleed:!!impact.bleedOnHit};
}
const close=(a,b)=>assert.ok(Math.abs(a-b)<1e-6,`${a} != ${b}`);
let cases=0;
for(const zone of ['Torso','Head']) {
  const hit={slot:'BodySlot',zone,thrust:true};
  for(const [grade,delta] of [['Normal',100],['Elite',80]]) {
    const result=replay({...hit,grade});assert.ok(result.stealth);close(result.delta,delta);cases++;
  }
  for(const extra of [{behind:false},{thrust:false},{zone:'Limb'},{combat:true},{det:1},{flags:[]}]) {
    assert.equal(replay({...hit,...extra}).stealth,false);cases++;
  }
}
for(const [opts, delta] of [[{},100],[{kind:2},100],[{grade:'Elite'},80],[{grade:'Elite',current:50},50],
  [{grade:'Elite',maximum:250,current:235},200],[{grade:'Elite',floor:35},65],[{grade:'Elite',floor:.1},80],[{floor:.1},99.9]]) {
  const r=replay(opts); close(r.delta,delta); close(r.budget,delta); assert.ok(r.stealth); cases++;
}
for(const opts of [{slot:'Head'},{slot:'Belly'},{slot:'None'},{behind:false},{intent:false},{kind:-1},{det:1},{high:'Alerted'},{combat:true},{flags:[]},{blocked:true},{deflected:true},
  {grade:'Boss'},{grade:'MaxTac'},{npc:'IsQuest'},{npc:'IsCharacterCivilian'},{npc:'IsResistantToTakedown'}, {hostile:'AIA_Neutral'},{hostile:'AIA_Friendly'},
  {material:'Armor'},{stats:{Armor:10}},{stats:{PhysicalResistance:5}},{stats:{MeleeResistance:2}},{pools:{Overshield:15}},
  {pools:{CPO_Armor:1}},{other:1},{takedown:'Grapple'},{minPercent:15},{stats:{IsInvulnerable:1}},{god:'Immortal'},
  {workspot:true},{tag:'ImmuneToTakedown'},{high:'Combat'},{held:false},...['Nonlethal','WasMitigated','DealNoDamage','CannotModifyDamage','DoNotTriggerFinisher'].map(f=>({flags:['StealthHit',f]}))]) {
  const r=replay(opts); assert.equal(r.applied,0,JSON.stringify(opts)); assert.equal(r.stealth,false); cases++;
}
for(const [opts,mul] of [[{thrust:true,intent:false},1.5],[{thrust:true,intent:false,hostile:'AIA_Neutral'},1.5],
  [{thrust:true,intent:false,blocked:true},1],[{thrust:false,intent:false},1],[{thrust:true,intent:false,god:'Immortal'},1]]) {
  const r=replay(opts);close(r.delta,10*mul);assert.equal(r.bleed,mul>1);cases++;
}
assert.equal(replay({thrust:true,grade:'Elite'}).bleed,false);cases++;
for(const [body,mult] of [[0,1.75],[3,1.75],[11.5,1.875],[20,2],[100,2]]) {
  const r=replay({parent:17,body,flags:[]}); close(r.delta,10*mult); assert.equal(r.stealth,false); cases++;
}
for(const [value,short,on] of [[11.99,false,false],[12,false,true],[20,true,false],[12,true,false]]) {
  assert.equal(replay({parent:3,body:value,short,flags:[]}).strengthReaction,on);cases++;
}
// Execute the dispatcher too: a synthetic positive ledger must not be sufficient
// without the same native attack, weapon, NPC, shape, and actual owner.
const send = method('VRBladePublishHit','hit,healthLoss');
const receiptCases = [{},{loss:0},{loss:-1},{available:0},{projected:true},{client:true},{switched:true},{foreign:true},{wrongItem:true},
  {wrongTarget:true},{wrongInstigator:true},{cancel:true},{claimed:false},{invalidShape:true},{empty:true},{blocked:true},{parent:9},{parent:1},{parent:1,stroke:16},{mute:true},{through:true}];
for (const s of receiptCases) {
  const p={GetGame:()=>0,vrBladeNextJointSerial:87}, npc={GetEntityID:()=>71,GetWorldTransform:()=>0,FindComponentByName:()=>null};
  const originalEntry={claimed:true};
  const w={GetItemID:()=>55,vrBladeReceipt:{serial:1,jointSerial:83,entry:originalEntry,seatSlot:'Shoulder',seatPoint:{X:8},lodged:true},VRBladeConfirmedSerial(){return this.vrBladeReceipt.serial;}};
  const impact={claimed:s.claimed!==false,cancelled:!!s.cancel,published:false,target:71,availableHealth:s.available??9,parentSerial:s.parent??0,edgeStroke:s.stroke??0,overlapPoint:{X:5,W:1}};
  if(s.through) Object.assign(impact,{thrust:true,entry:{claimed:true,endedClear:true}});
  const atk={vrBladeImpact:impact}, flags=s.mute?['DisableVFX']:[], data={GetWeapon:()=>w,GetAttackDefinition:()=>atk,GetInstigator:()=>s.wrongInstigator?{}:p,
    WasBlocked:()=>!!s.blocked,WasDeflectedAny:()=>false,HasFlag:f=>flags.includes(f)};
  const hit={target:npc,attackData:data,projectionPipeline:!!s.projected,vrMeleeEnhancedTicket:{weapon:s.wrongItem?12:55,attack:s.foreign?{}:atk,target:s.wrongTarget?12:71},
    hitRepresentationResult:{hitShapes:s.empty?[]:[{userData:{ok:!s.invalidShape},result:{hitPositionEnter:{W:0}}}]}};
  let marks=0,continued=0;
  Object.assign(ctx,{ArraySize:x=>x.length,VRLeftHandedWeapon:{HeldSlot:()=>1},GameInstance:{GetSimTime:()=>8,GetRuntimeInfo:()=>({IsClient:()=>!!s.client}),
    GetTransactionSystem:()=>({GetItemInSlot:()=>s.switched?{}:w})},EngineTime:{ToFloat:x=>x},WorldTransform:{TransformInvPoint:(_,x)=>({...x})},
    HitShapeUserDataBase:{GetHitReactionZone:()=>3},VrBladeReceipt:class{},VRBladeAddSurfaceMark:()=>{marks++;},VRBladeContinueTrail:()=>{continued++;}});
  ctx.VrBladeRPG.ValidZone=shape=>shape.userData.ok;
  const ok=Object.keys(s).length===0 || s.parent===1 || !!s.mute || !!s.through;
  assert.equal(send.call(p,hit,s.loss??20),ok,JSON.stringify(s));
  assert.equal(w.vrBladeReceipt.serial,ok?2:1); assert.equal(marks,ok&&!s.mute&&(!s.parent||s.stroke)?1:0);
  if(ok) assert.equal(w.vrBladeReceipt.entry,s.parent?originalEntry:impact.entry);
  assert.equal(continued,ok&&!s.mute&&s.parent&&!s.stroke?1:0);
  if(s.stroke) {
    assert.equal(w.vrBladeReceipt.lastEdgeStroke,16);assert.equal(w.vrBladeReceipt.seatSlot,'Shoulder');
    assert.equal(w.vrBladeReceipt.seatPoint.X,8);assert.equal(w.vrBladeReceipt.point.X,5);
    assert.equal(w.vrBladeReceipt.lodged,true);assert.ok(!w.vrBladeReceipt.puncture);
  }
  assert.equal(w.vrBladeReceipt.jointSerial,ok&&!s.parent?88:83);
  if(s.through) {
    assert.equal(w.vrBladeReceipt.puncture,true);
    assert.equal(w.vrBladeReceipt.released,true);assert.ok(!w.vrBladeReceipt.awaitingSeat);
  }
  if(ok){assert.equal(w.vrBladeReceipt.healthLoss,9);assert.equal(w.vrBladeReceipt.follow,s.parent===1);
    assert.equal(send.call(p,hit,20),false);assert.equal(w.vrBladeReceipt.serial,2);}
  cases++;
}
// The body is never forced into ragdoll. Only a matching, unspent receipt may
// affect the already simulated actor, and the rate cap survives a long frame.
const load = method('VRBladeLoadRagdoll','serial,wristReaction');
{
  let rag=true, time=5, allowed=true, deliveries=[];
  const target={IsRagdolling:()=>rag,QueueEvent:e=>deliveries.push(e)};
  const w={vrBladeReceipt:{serial:3,target,owner:{GetGame:()=>0,VRBladeAllowed:()=>allowed},slot:'Contact',time:5,loadTime:0},
    VRBladeAnchorWorld:()=>({W:1})};
  Object.assign(ctx,{IsNameValid:s=>!!s,Vector4:{Length:d=>Math.abs(+d)},CreateRagdollApplyImpulseEvent:(p,v,r)=>({p,v,r}),GameInstance:{GetSimTime:()=>time}});
  const vec=(x)=>({W:0,valueOf:()=>x});
  assert.equal(load.call(w,3,vec(.04)),false);time+=.06;rag=false;assert.equal(load.call(w,3,vec(.04)),false);
  rag=true;assert.equal(load.call(w,2,vec(.04)),false);assert.equal(load.call(w,3,vec(.2)),false);
  assert.equal(load.call(w,3,vec(.04)),true);assert.ok(deliveries[0].v<0);assert.equal(load.call(w,3,vec(.04)),false);
  time+=2;assert.equal(load.call(w,3,vec(.04)),true);close(deliveries[1].v,-.04*160*.08);
  allowed=false;time+=2;assert.equal(load.call(w,3,vec(.04)),false);allowed=true;w.vrBladeReceipt.spent=true;
  assert.equal(load.call(w,3,vec(.04)),false);assert.equal(deliveries.length,2);cases+=9;
}
// Exercise the actual marker/shape intersection using 1D segments embedded in world space.
{
  const evaluate=method('PortContact','impact,shape');const point=(x,W=1)=>({X:x,W});
  Object.assign(ctx,{IsNameValid:n=>!!n&&n!=='None',Vector4:{Distance:(a,b)=>Math.abs(a.X-b.X)},
    VrCyberwareContactMath:{Closest:(p,a,b)=>point(Math.min(Math.max(p.X,Math.min(a.X,b.X)),Math.max(a.X,b.X)))}});
  function verify(name,w,weaponA,weaponB,enter,exit){return evaluate(
    {a:point(weaponA),b:point(weaponB),portAtStart:point(0,w)},{hitShapeName:name,result:{hitPositionEnter:point(enter),hitPositionExit:point(exit)}});}
  assert.equal(verify('Head',1,-.1,.1,-.1,.1),true);
  assert.equal(verify('Belly',1,-.1,.1,-.1,.1),false);cases++;
  for(const spec of [['Head',0,-.1,.1,-.1,.1],['Head',1,.08,.3,-.1,.1],['Head',1,-.2,.35,.14,.3],
      ['None',1,-.1,.1,-.1,.1],['Chest',1,-.30,.1,-2,.1]])assert.equal(verify(...spec),false);
  // Test the horizontal rear-side rule too; vertical separation is not facing.
  const rearSource=body(reds,'Behind').replace('player.GetWorldPosition() - npc.GetWorldPosition()', 'delta');
  const rear=vm.runInContext(`(function(delta,npc) {${rearSource}})`,ctx);
  const facing={GetWorldForward:()=>({X:0,Y:1,Z:0})};
  assert.equal(rear({X:.1,Y:-1,Z:3},facing),true);assert.equal(rear({X:0,Y:1,Z:-5},facing),false);
  assert.equal(rear({X:-1,Y:0,Z:0},facing),false);cases+=9;
}
// Replay the stock effect callback with a displaced animated slot. The native
// shaft entry can be much farther from the clipped station than the box width.
{
  const gap=method('VRBladeCutGap','shape,hit');
  const useCut=method('UseCut','impact,shape',visibleCut);
  const pipeline=method('ProcessPipeline','hitEvent,cache',contact);
  const matches=method('FollowMatches','hit,impact,shape');
  const v=(X,Y=0,Z=0,W=1)=>({X,Y,Z,W});
  const distance=(a,b)=>Math.abs(a.X-b.X);
  const capsule=[v(.1,0,0,.12),v(1,0,0,.1)];
  for(const state of ['current frame','request frame','no callback','no shapes','past shaft','wrong region','changed capsule','replaced slots',
    'invalid zone','protection','old stroke','old parent','withdrawn','foreign owner','cancelled','expired','new pending',
    'only secondary reported','opposite query','protected secondary','occupied only','stock order','both ends clear']) {
    let calls=0;
    const slots={GetSlotTransform:s=>s==='Chest'};
    const target={GetEntityID:()=>17,FindComponentByName:()=>state==='replaced slots'?{}:slots};
    const owner={VRBladeAllowed:()=>true,GetGame:()=>0};
    const observation={geometry:{},data:{},material:'character_flesh'};
    const physical={name:'LeftTorso',slot:'Chest',from:v(.190),to:v(.204578),shaftFrom:v(.095),shaftTo:v(.21),capsule,observedFrame:.2,observation,
      oldA:v(.04),oldB:v(.66),endA:v(.04),endB:v(.66),window:{W:.014578},reverseQuery:false};
    const secondary={...physical,name:'Belly',to:v(.198),window:{W:.008},
      reverseQuery:['opposite query','both ends clear'].includes(state),eitherEnd:state==='both ends clear'};
    const impact={target:17,a:v(0),b:v(1),events:0,created:1,parentSerial:4,edgeStroke:10,cutShape:physical,
      cutCandidates:state==='occupied only'?[physical]:[physical,secondary],
      sweepFrom:v(.390),sweepTo:v(.404578),contactMetrics:{X:.014578,Z:1},cancelled:state==='cancelled'};
    const r={owner,target,serial:4,slots,lodged:true,pending:state==='new pending'?{}:impact,jointSerial:7,released:state==='withdrawn'};
    const w={vrBladeReceipt:r,VRBladeCutGap:gap,VRBladeConfirmedSerial:()=>state==='old parent'?5:4,VRBladeConfirmedAge:()=>.4};
    const def={vrMeleeContactTarget:17,vrBladeImpact:impact};
    const data={GetAttackType:()=>'Melee',GetAttackDefinition:()=>def,GetWeapon:()=>w,GetInstigator:()=>owner};
    const useSecondary=['only secondary reported','opposite query','protected secondary','occupied only','stock order','both ends clear'].includes(state);
    const reported={hitShapeName:state==='wrong region'?'Head':useSecondary?secondary.name:physical.name,physicsMaterial:'character_flesh',
      userData:{ok:state!=='invalid zone'},result:{hitPositionEnter:v(state==='past shaft'?2:state==='request frame'?.30:.60)}};
    const shapes=state==='no shapes'?[]:['protection','protected secondary'].includes(state)?[{...reported,userData:{layer:true}},reported]:[reported];
    if(state==='stock order') shapes.push({...reported,hitShapeName:physical.name});
    const hit={target:state==='foreign owner'?{GetEntityID:()=>18}:target,attackData:data,hitRepresentationResult:{hitShapes:shapes}};
    Object.assign(ctx,{VrBladeVisibleCut:{UseCut:useCut},AbsF:Math.abs,VRLeftHandedWeapon:{InHand:()=>false},EntityID:{IsDefined:Boolean},
      VRBladeEdgeStrokeLive:(_,key,stroke)=>state!=='old stroke'&&key===7&&stroke===10,
      VRBladeMatchesOccupied:(comp,h,slot,geometry,data,mat,center,axis)=>comp===slots&&h.userData===reported.userData&&slot==='Chest'
        && geometry===observation.geometry && data===observation.data && mat===observation.material
        && center===capsule[0] && axis===capsule[1] && state!=='changed capsule',
      DamageSystemHelper:{IsProtectionLayer:s=>s.userData.layer},HitShape_Type:{ProtectionLayer:'ProtectionLayer'},
      AttackData:{IsMelee:()=>true},Vector4:Object.assign(v,{Distance:distance}),WorldPosition:{ToVector4:x=>x},
      WorldTransform:{TransformInvPoint:(f,x)=>v(x.X-(f??.5)),TransformPoint:(_,x)=>v(x.X+.5)},
      ArrayClear:a=>{a.length=0;},ArrayPush:(a,s)=>a.push(s),GameInstance:{GetSimTime:()=>state==='expired'?2:1.01},
      VrCyberwareContactMath:{Closest:(x,a,b)=>v(Math.min(b.X,Math.max(x.X,a.X)))},
      wrappedMethod:e=>{++calls;assert.equal(e.hitRepresentationResult.hitShapes[0],reported);}});
    Object.assign(ctx.VrBladeRPG,{FollowMatches:matches,ValidZone:s=>s.userData.ok,SpecialTarget:()=>true});
    if(state!=='no callback') {pipeline(hit,{});pipeline(hit,{});}
    const accepted=['current frame','request frame','only secondary reported','stock order','both ends clear'].includes(state);
    assert.equal(calls,Number(accepted),state);
    assert.equal(Boolean(impact.claimed),accepted,state);assert.equal(Boolean(r.spent),accepted,state);
    if(accepted) {
      const expected=useSecondary?secondary:physical;
      close(impact.overlapPoint.X,.5+expected.to.X);close(impact.cutPoint.X,expected.to.X);
      assert.equal(impact.cutShape,expected);close(impact.contactMetrics.X,expected.window.W);assert.equal(impact.cutSlot,'Chest');
    }
    if(['opposite query','occupied only'].includes(state)) assert.equal(impact.cutProof.Y,0,state);
    if(state==='changed capsule') {assert.ok(impact.cutProof.Y>0);assert.equal(impact.cutProof.Z,0);}
    if(state==='protected secondary') assert.equal(impact.cutProof.W,1);
    cases++;
  }
}
{
  const measure=method('VRBladeSeatInterval','serial,a,b,edge');
  const owner={VRBladeAllowed:()=>true};
  const component={}, target={IsAttached:()=>true,FindComponentByName:()=>component};
  const receipt={serial:42,puncture:true,owner,target};
  let named=false, bodyDepth=.209;
  const vector=(X,Y,Z,W)=>({X,Y,Z,W});
  Object.assign(ctx,{Vector4:Object.assign(vector,{Distance:()=>.75}),
    VRBladeSweptContact:(shapes,a,b,c,d)=>{
      assert.equal(shapes,component);assert.equal(a,c);assert.equal(b,d);
      return vector(0,.78,bodyDepth,bodyDepth>0?1:0);
    }});
  const w={vrBladeReceipt:receipt,VRMeleeReach:()=>.75,
    VRBladeReceiptInterval:()=>named?vector(.55,.15,0,1):vector(0,0,0,0)};
  const sample=serial=>measure.call(w,serial,0,.75,'edge');
  assert.equal(sample(42).Y,.209);
  named=true;assert.equal(sample(42).X,.55);
  named=false;bodyDepth=0;assert.equal(sample(42).W,0);
  bodyDepth=.2;receipt.puncture=false;assert.equal(sample(42).W,0);
  receipt.puncture=true;assert.equal(sample(41).W,0);
  receipt.released=true;assert.equal(sample(42).W,0);cases+=6;
}
{
  const reported=method('VRBladePartNotified','',contact);
  const r={impact:{partNotified:true}};
  const w={vrBladeReceipt:r,vrLastBladeProbe:{partNotified:false}};
  assert.equal(reported.call(w),true);
  r.impact.partNotified=false;w.vrLastBladeProbe.partNotified=true;
  assert.equal(reported.call(w),false);
  cases+=2;
}
{
  const matches=method('SameShapeData','r,shape');
  const geometry=[{X:0,W:.12},{X:1,W:.20}];
  const r={shapeData:{},material:0,slots:{},capsule:geometry};
  const shape={shapeName:'Chest',hitShapeName:'Chest',physicsMaterial:'flesh',userData:{}};
  let measured=geometry.map(v=>({...v}));
  Object.assign(ctx,{AbsF:Math.abs,Vector4:{Distance:(a,b)=>Math.abs(a.X-b.X)},VRBladeReadCapsule:()=>measured});
  assert.equal(matches(r,shape),true);
  measured[1].W+=.1;assert.equal(matches(r,shape),false);
  measured=[];assert.equal(matches(r,shape),false);
  measured=geometry;shape.physicsMaterial='metal';assert.equal(matches(r,shape),false);cases+=4;
}
{
  const strokes=fs.readFileSync(path.join(dir,'vrport_contact_stroke.reds'),'utf8');
  const blocked=method('VRBladeEntryBlocked','',strokes);
  let joint=false,now=20;
  const receipt={serial:12,target:{IsAttached:()=>true},owner:{GetGame:()=>0},awaitingSeat:true,lodgedAt:20};
  const gate={vrBladeReceipt:receipt};
  Object.assign(ctx,{VRMeleeRealTime:()=>now,VRBladeJointPoint:()=>({W:+joint}),VRLeftHandedWeapon:{InHand:()=>false},
    GameInstance:{GetSimTime:()=>now}});
  assert.equal(blocked.call(gate),true);now+=.3;assert.equal(blocked.call(gate),false);
  receipt.lodged=true;joint=true;assert.equal(blocked.call(gate),true);
  joint=false;receipt.pending={created:now};assert.equal(blocked.call(gate),true);
  receipt.released=true;assert.equal(blocked.call(gate),false);cases+=5;
  const release=method('VRBladeReleaseContact','serial',strokes);
  const target={}, pending={cancelled:false}, entry={claimed:true};
  const w={vrBladeReceipt:{serial:12,pending,target,entry},vrBladeTouch:target,vrBladeAttempt:{entry,claimed:false}};
  release.call(w,11);assert.equal(pending.cancelled,false);
  w.vrBladeTouch=w.vrBladeReceipt.target=null; // both weak handles expired on removal
  release.call(w,12);assert.equal(w.vrBladeReceipt.released,true);assert.equal(pending.cancelled,true);
  assert.equal(w.vrBladeAttempt,null);assert.equal(entry.expired,true);
  const newer={entry:{},published:true};
  w.vrBladeAttempt=newer;w.vrBladeTouch=target;
  release.call(w,12);assert.equal(w.vrBladeAttempt,newer);assert.equal(w.vrBladeTouch,target);
  assert.equal(w.vrBladeReceipt.pending,null);cases+=4;
}
assert.match(body(enhanced,'SendDamageEvents'),/affectedStatPool, gamedataStatPoolType\.Health/);
assert.match(body(enhanced,'VRMeleeConfirmEnhancedHit'),/damage > 0\.00[\s\S]*VRBladePublishHit/);
assert.doesNotMatch(body(contact,'ProcessPipeline'),/VRMeleeChainLanded|VRBladePublishHit|VRBladeAddSurfaceMark/);
const publish=body(reds,'VRBladePublishHit');
for(const token of ['healthLoss > 0.00','hit.projectionPipeline','blade != ticket.attack','weapon.GetItemID() != ticket.weapon','npc.GetEntityID()','impact.published','ValidZone(shape)','impact.availableHealth > 0.00']) assert.ok(publish.includes(token),token);
assert.ok(publish.indexOf('weapon.vrBladeReceipt = r') < publish.indexOf('VRBladeAddSurfaceMark('));
const follow=body(reds,'VRBladeTryFollowThrough');
assert.ok(follow.indexOf('r.pending = impact') < follow.indexOf('this.VRCyberwareOrdinaryContact('));
assert.ok(follow.includes('r.pending.cancelled = true') && !follow.includes('r.spent = true'));
assert.match(body(contact,'ProcessPipeline'),/if impact.parentSerial != 0u \{[\s\S]*!VrBladeRPG.FollowMatches[\s\S]*vrBladeReceipt.spent = true/);
assert.match(body(contact,'ProcessPipeline'),/gap < nearest && \(impact.parentSerial == 0u \|\| \(VrBladeRPG.ValidZone\(shapes\[i\]\) && VrBladeRPG.FollowMatches\(hitEvent, impact, shapes\[i\]\)\)\)/);
assert.match(body(reds,'FollowMatches'),/Equals\(weapon.vrBladeReceipt.shapeName,shape.hitShapeName\)/);
assert.doesNotMatch(reds,/\.Kill\(|QueueHitEvent\(|RequestSettingStatPool|RequestChangingStatPool|\.AddFlag\(/);
// Execute the request through native-path capture, capsule clipping and effect
// dispatch. JS has no Vector4 operators, so bind those expressions to 3D math.
{
  const v=(X,Y,Z,W=0)=>({X,Y,Z,W});
  const plus=(a,b)=>v(a.X+b.X,a.Y+b.Y,a.Z+b.Z,a.W+b.W);
  const minus=(a,b)=>v(a.X-b.X,a.Y-b.Y,a.Z-b.Z,a.W-b.W);
  const times=(a,k)=>v(a.X*k,a.Y*k,a.Z*k,a.W*k);
  const dot=(a,b)=>a.X*b.X+a.Y*b.Y+a.Z*b.Z;
  const len=a=>Math.hypot(a.X,a.Y,a.Z);
  Object.assign(ctx,{plus,minus,times,Vector4:Object.assign(v,{Dot:dot,Length:len,
    Distance:(a,b)=>len(minus(a,b)),Normalize:a=>times(a,1/len(a))}),
    VrCyberwareContactMath:{Closest:(p,a,b)=>{
      const ab=minus(b,a),squared=dot(ab,ab);
      const k=squared>1e-7?dot(minus(p,a),ab)/squared:0;
      return plus(a,times(ab,Math.max(0,Math.min(1,k))));
    }},
    VrBladeCutShape:class{},VrBladeImpact:class{constructor(){this.cutCandidates=[];}},
    ArrayPush:(a,b)=>a.push(b),ArraySize:a=>a.length,VRLeftHandedWeapon:{InHand:()=>false},
    Quaternion:{Conjugate:x=>x,Transform:(_,a)=>a},WorldTransform:{TransformInvPoint:(_,a)=>a,GetOrientation:()=>0},
    GameInstance:{GetSimTime:()=>1},EngineTime:{ToFloat:t=>t}});
  function translated(file, name, params, operations={}) {
    let code=body(file,name);
    for(const [expression,value] of Object.entries(operations)) {
      assert.ok(code.includes(expression),expression);code=code.replaceAll(expression,value);
    }
    return method(name,params,`func ${name}(){${code}}`);
  }
  {
    // Both directions of this finite crossing are covered by blade_contact_state_tests.
    // Supply the geometry observations here and execute the actual entry/claim policy.
    const sweep=translated(contact,'VRBladeSweep','weapon,oldA,oldB,a,b,passive,beginOnContact,trackedMotion,strong,speed',{
      'let trace: TraceResult;':'let trace={normal:Vector4(1,0,0,0)};',
      'oldA+(oldB-oldA)*0.10':'plus(oldA,times(minus(oldB,oldA),.10))',
      'a+(b-a)*0.10':'plus(a,times(minus(b,a),.10))',
      'prevStart+(curStart-prevStart)*overlap.X':'plus(prevStart,times(minus(curStart,prevStart),overlap.X))',
      'oldB+(b-oldB)*overlap.X':'plus(oldB,times(minus(b,oldB),overlap.X))',
      'p+(q-p)*overlap.Y':'plus(p,times(minus(q,p),overlap.Y))',
      'prevStart+(oldB-prevStart)*overlap.Y':'plus(prevStart,times(minus(oldB,prevStart),overlap.Y))',
      'curStart+(b-curStart)*overlap.Y':'plus(curStart,times(minus(b,curStart),overlap.Y))',
      'q-p':'minus(q,p)','q - p':'minus(q,p)','b-oldB':'minus(b,oldB)','a-oldA':'minus(a,oldA)',
      'point + direction * 0.12':'plus(point,times(direction,.12))'
    });
    const shapes={};
    let npc={GetEntityID:()=>71,IsAttached:()=>true,IsActive:()=>true,IsDead:()=>false,FindComponentByName:()=>shapes};
    Object.assign(ctx,{VrBladeEntry:class{},TraceResult:{GetHitEntity:()=>npc},ScriptedPuppet:{IsDefeated:()=>false},
      GameObject:{IsFriendlyTowardsPlayer:()=>false},GetVRSharedSlot:()=>0,
      VRBladeSweptContact:(component,oldA,oldB,a,b)=>{
        assert.equal(component,shapes);
        const crosses=Math.min(oldA.X,a.X)<.06 && Math.max(oldA.X,a.X)>-.06;
        return crosses?v(.45,.5,.1,1):v(0,0,0,0);
      }});
    ctx.VRBladeEntrySweep=(component,oldA,oldB,a,b)=>{
      const hit=ctx.VRBladeSweptContact(component,oldA,oldB,a,b);
      if(hit.W && Math.abs(a.X)>.06) hit.W=2;
      return hit;
    };
    ctx.GameInstance.GetSpatialQueriesSystem=()=>({});
    ctx.VrCyberwareContactMath.SweptTrace=()=>true; // enclosing proxy also covers clear poses
    let blocked=false;
    let serial=0,immediate=true;
    const w={VRBladeEntryBlocked:()=>blocked,VRBladeConfirmedSerial:()=>serial,
      VRBladeNoDismember:()=>false,GetWorldForward:()=>v(1,0,0)};
    const impacts=[];
    const player={GetGame:()=>0,VRBladeAllowed:()=>true,VRCyberwareBeginSwing:()=>true,
      VRCyberwareOrdinaryContact:(_,target,from,to,passive,impact)=>{
        assert.equal(target,npc);impacts.push(impact);impact.claimed=impact.entry.claimed=immediate;return 1;
      }};
    const move=(x,y,passive=false)=>sweep.call(player,w,v(x,0,-.4,1),v(x,0,.35,1),
      v(y,0,-.4,1),v(y,0,.35,1),passive,true,v(y-x,0,0),false,4);
    assert.equal(move(-.48,.52),1);
    assert.equal(move(.52,-.48),1,'clear turning point swallowed immediate reverse crossing');
    assert.notEqual(impacts[0].entry,impacts[1].entry,'two separate contacts shared one callback claim');
    assert.equal(move(-.48,0),1);
    assert.equal(move(0,.01),0,'continuous overlap hit twice');
    assert.equal(move(.01,.52),0,'withdrawal became a fresh entry');
    assert.equal(move(.52,.48),0,'proxy alone accepted as a new contact');
    blocked=true;assert.equal(move(.48,-.48),0,'live lodge admitted an ordinary sweep');
    blocked=false;assert.equal(move(-.48,.52,true),0,'passive motion attacked');
    cases+=8;
    const canClaim=method('VRBladeCanClaimEntry','impact,player',contact).bind(w);
    immediate=false;
    assert.equal(move(.52,-.48),1);const first=impacts.at(-1);
    assert.equal(move(-.48,.52),1);const second=impacts.at(-1);
    assert.notEqual(first.entry,second.entry);
    assert.equal(canClaim(first,player),true,'late first crossing lost to pending second crossing');
    first.entry.claimed=true;serial++;
    assert.equal(canClaim(second,player),true);
    second.entry.claimed=true;
    assert.equal(canClaim(first,player),false,'old callback overtook newer claim');
    first.entry.claimed=second.entry.claimed=false;serial--;
    ctx.GameInstance.GetSimTime=()=>1.13;
    assert.equal(canClaim(first,player),false,'expired crossing stayed claimable');
    ctx.GameInstance.GetSimTime=()=>1;
    assert.equal(move(.52,0),1);const unfinished=impacts.at(-1);
    assert.equal(move(0,.01),1);assert.equal(impacts.at(-1).entry,unfinished.entry,'continuous probes need one claim');
    cases+=9;
    // Callback A wins while sibling B is still the weapon's latest probe.
    // Removing their target invalidates the weak touch before the next CET update.
    const sibling=impacts.at(-1);
    const removed=npc;
    npc={...removed,GetEntityID:()=>72};
    assert.equal(move(-.48,.01),0,'a live unclaimed crossing lost its reservation to another target');
    unfinished.claimed=unfinished.published=unfinished.entry.claimed=true;
    assert.ok(!sibling.claimed);
    w.vrBladeTouch=null;
    ctx.GameInstance.GetSimTime=()=>2;
    immediate=true;
    assert.equal(move(-.48,.01),1,'claimed sibling on a removed target blocked the next target at stage 2');
    assert.equal(w.vrBladeSweepSample.W,3);
    assert.equal(impacts.at(-1).target,72);
    assert.notEqual(impacts.at(-1).entry,sibling.entry);
    assert.equal(canClaim(sibling,player),false);
    assert.equal(move(.01,.02),0,'continuous contact hit the replacement twice');
    // A detached proxy may remain addressable until its last handle drops.
    npc.IsAttached=()=>false;
    npc={...removed,GetEntityID:()=>73};
    assert.equal(move(-.48,0),1);
    assert.equal(impacts.at(-1).target,73);
    ctx.GameInstance.GetSimTime=()=>1;
    cases+=9;
  }
  const capture=translated(reds,'VRBladeCaptureCut','impact,motion',{'end-start':'minus(end,start)'});
  const request=method('VRBladeTryFollowThrough','weapon,serial,a,b,motion,oldA,oldB');
  const record=method('VRBladeFollowResult','gate');
  const station=translated(visibleCut,'Station','a,b,f',{'a+(b-a)*f':'plus(a,times(minus(b,a),f))'});
  const exterior=translated(visibleCut,'ExteriorEnd','obs',{
    'let center=obs.capsule[0]':'let center={...obs.capsule[0]}','let axis=obs.capsule[1]':'let axis={...obs.capsule[1]}',
    'Vector4.Normalize(axis)*height':'times(Vector4.Normalize(axis),height)','center-half':'minus(center,half)','center+half':'plus(center,half)'
  });
  const prior=method('Prior','pose,current',visibleCut);
  const window=translated(visibleCut,'Window','old,now,prior,current,pinToShape,intent',{
    'now.railOffset-old.railOffset':'minus(now.railOffset,old.railOffset)',
    'now.goal-old.goal':'minus(now.goal,old.goal)',
    'VrBladeVisibleCut.Station(now.a,now.b,f)-VrBladeVisibleCut.Station(old.a,old.b,f)':
      'minus(VrBladeVisibleCut.Station(now.a,now.b,f),VrBladeVisibleCut.Station(old.a,old.b,f))',
    'VrBladeVisibleCut.Station(current.a,current.b,f)-VrBladeVisibleCut.Station(prior.a,prior.b,f)':
      'minus(VrBladeVisibleCut.Station(current.a,current.b,f),VrBladeVisibleCut.Station(prior.a,prior.b,f))'
  });
  const observe=method('VRBladeRecordEdgePose','r,pin,slotFrame,a,b,edge,goal,side',visibleCut);
  const keep=method('VRBladeKeepOccupied','name,slot,geometry,data,material,center,axis,frame,a,b,interval',visibleCut);
  const clip=translated(visibleCut,'VRBladeClipStations','impact,stroke,intent',{
    'Quaternion.Conjugate(WorldTransform.GetOrientation(frame))*now.rotation':'now.rotation-WorldTransform.GetOrientation(frame)',
    'end+(was-end)*share':'plus(end,times(minus(was,end),share))',
    'current.b-current.a':'minus(current.b,current.a)'
  });
  const rotate=(angle,p)=>v(Math.cos(angle)*p.X-Math.sin(angle)*p.Y,Math.sin(angle)*p.X+Math.cos(angle)*p.Y,p.Z,p.W);
  const toWorld=(frame,p)=>Object.assign(plus(frame.pos,rotate(frame.angle,p)),{W:1});
  const toLocal=(frame,p)=>Object.assign(rotate(-frame.angle,minus(p,frame.pos)),{W:1});
  Object.assign(ctx,{SqrtF:Math.sqrt,VrBladeVisibleCut:{Station:station,Prior:prior,Window:window,ExteriorEnd:exterior,
    UseCut:method('UseCut','impact,shape',visibleCut),BoxHalfExtent:method('BoxHalfExtent','',visibleCut)},VrBladeVisiblePose:class{constructor(){this.shapes=[];}},
    VrBladeObservedShape:class{},ArrayClear:a=>{a.length=0;},ArrayErase:(a,i)=>a.splice(i,1),
    Quaternion:{Transform:rotate,Conjugate:a=>-a},WorldTransform:{TransformInvPoint:toLocal,TransformPoint:toWorld,GetOrientation:f=>f.angle},
    WorldPosition:{ToVector4:p=>p}});
  const combat=fs.readFileSync(path.join(dir,'../CyberpunkVRPort_Cyberware/vrport_cyberware_combat.reds'),'utf8');
  const attack=translated(combat,'VRCyberwareAttack','swing,contact,target,side,targetID,radius,impact',{
    'let context: AttackInitContext;':'let context={};','target - contact':'minus(target,contact)','target-start':'minus(target,start)'
  });
  const fields=new Proxy({}, {get:(_,key)=>key});
  Object.assign(ctx,{GetAllBlackboardDefs:()=>({EffectSharedData:fields}),EffectData:new Proxy({}, {
    get:()=>((values,key,value)=>{values[key]=value;})}),ToVariant:v=>v,VrCyberwareAttackTicket:class{},
    WeaponObject:{GetWeaponChargeNormalized:()=>0},ArrayRemove:(a,value)=>a.splice(a.indexOf(value),1)});
  ctx.Quaternion.BuildFromDirectionVector=dir=>dir;
  // A capsule with zero spine length is an authored sphere. A centerline
  // that begins outside it can still have an overlapping starting box.
  const sphere={capsule:[v(0,0,0,.10),v(0,0,1,0)]};
  for(const [base,tip,end] of [[.17,0,1],[.15,-.20,-1],[.15,-.15,0],[.08,-.09,0]]) {
    assert.equal(exterior({...sphere,a:v(base,0,0),b:v(tip,0,0)}),end);
    cases++;
  }
  for(const [opts,outcome] of [
    [{gap:.0547,slip:.0197},'Dispatched'],[{gap:.0694,slip:.0947},'Dispatched'],
    [{gap:.062148,slip:.017742,visible:.017993,clippedDepth:.014578},'Dispatched'],
    [{tipFirst:true},'Dispatched'],[{bothInside:true},'NoExteriorStart'],
    [{oldAdvance:.265,slip:.45-.265,visible:.095,rotate:true},'Dispatched'],
    [{slip:.45},'StationMotion'],[{oldAdvance:.65,slip:-.205},'NoVisibleWindow'],
    [{noJoint:true},'NoSolverPath'],[{spentStroke:true},'UsedStroke'],[{slip:.002},'SolverDistance'],
    [{slip:-.223},'SolverDirection'],[{railRejected:true},'NoSolverPath'],[{flat:true},'ControllerDirection'],
    [{visible:0,rotate:true},'StationMotion'],[{sideways:true},'StationMotion'],[{candidateMoves:true},'StationMotion'],
    [{visibleClear:true},'VisibleBladeClear'],[{unresolved:true},'NoResolvedShape'],[{tiny:true},'NoRetainedShape'],
    [{unavailable:true},'ObservationUnavailable'],[{duplicate:true},'Dispatched'],[{replaced:true},'Dispatched'],[{clone:true},'Dispatched'],
    [{neighbors:true},'Dispatched'],[{neighbors:true,protectedNeighbor:true},'Dispatched'],
    [{changedData:true},'NoVisibleWindow'],[{changedGeometry:true},'NoVisibleWindow'],[{clippedDepth:0},'NoClippedInterval'],
    [{fraction:.1},'Dispatched'],[{pivot:true,slip:.065},'Dispatched'],[{freshShape:true},'NoVisibleWindow'],[{stalePose:true},'StaleVisibleWindow'],
    [{targetRules:false},'TargetRules'],[{stale:true},'ReceiptChanged'],[{cannotStart:true},'EffectStart']
  ]) {
    const gap=opts.gap??.09, slip=opts.slip??.02, visible=opts.visible??slip;
    const startAdvance=opts.oldAdvance??0, advance=startAdvance+slip;
    let tick=0, time=.8, dispatches=0;
    const edge=v(1,0,0);
    const centerZ=opts.tipFirst?.04:opts.bothInside?.375:.56;
    const capsule=[v(0,0,centerZ,opts.bothInside?.40:.1),v(0,0,1,.05)];
    const measuredFraction=opts.tipFirst?.10:.76;
    const frames=[{pos:v(-.8,.4,-.1),angle:opts.rotate?Math.PI/3:0},{pos:v(0,0,0),angle:0}];
    const candidate=()=>opts.candidateMoves && tick===0 ? {...frames[0],pos:plus(frames[0].pos,v(-visible,0,0))}:frames[tick];
    const baseAt=i=>toWorld(frames[i],v(i===0&&!opts.sideways&&!opts.pivot?-visible:0,i===0&&opts.sideways?-visible:0,0,1));
    const tipAt=i=>i===0&&opts.pivot ? plus(baseAt(i),v(-.75*Math.sin(.11),0,.75*Math.cos(.11))) : plus(baseAt(i),v(0,0,.75,0));
    const slots={},geometry={},data={},protectedData={ok:false};
    const player={GetGame:()=>0,VRBladeAllowed:()=>true,VRHolsterIntentActive:()=>false,
      VRCyberwareOrdinaryContact:(_,target,a,b,passive,impact)=>{
        ++dispatches;assert.equal(impact.edgeStroke,1);
        assert.equal(impact.cutShape.name,'LeftTorso');
        close(impact.sweepFrom.Y,0);close(impact.sweepTo.Y,0); // visible shaft, not the physical Y=gap
        const shape=impact.cutShape;
        if(!opts.replaced) assert.equal(shape.observation.geometry,geometry);
        if(!opts.clone) assert.equal(shape.observation.data,data);
        assert.equal(impact.cutCandidates.length,opts.neighbors&&!opts.protectedNeighbor?2:1);
        if(opts.neighbors&&!opts.protectedNeighbor) {
          const other=impact.cutCandidates.find(c=>c.name==='Belly');
          assert.ok(other.window.W>0 && other.window.W<shape.window.W);
        }
        assert.equal(shape.capsule[0],capsule[0]);assert.equal(shape.observation.frame,shape.observedFrame);
        close(len(minus(station(shape.oldA,shape.oldB,measuredFraction),impact.sweepFrom)),0);
        close(len(minus(station(shape.endA,shape.endB,measuredFraction),impact.sweepTo)),0);
        assert.ok(impact.sweepTo.X>impact.sweepFrom.X);
        assert.ok(impact.sweepTo.X-impact.sweepFrom.X<=Math.min(slip,visible,.08)+1.e-6);
        assert.ok(impact.contactMetrics.X>0);
        const values={},tickets=[];
        const attackOwner={GetGame:()=>0,vrCyberwareTickets:tickets,VRMeleeRegisterAttack:()=>({}),VRMeleeCancelAttack:()=>{}};
        const swing={record:{ImpactFxSlot:()=>''},weapon:{GetFxPackage:()=>'',GetItemID:()=>12},number:1};
        let starts=0;
        const definition={PrepareAttack:()=>({GetSharedData:()=>values}),StartAttack:()=>{
          ++starts;assert.equal(tickets[0].attack,definition);
          assert.equal(definition.vrMeleeContactTarget,17);assert.equal(definition.vrBladeImpact,impact);return !opts.cannotStart;
        }};
        ctx.IAttack={Create:context=>{assert.equal(context.record,swing.record);return definition;}};
        const started=attack.call(attackOwner,swing,a,b,1,17,.175,impact);
        assert.equal(starts,1);assert.equal(tickets.length,Number(started));assert.equal(values.attack,definition);
        close(values.range,len(minus(b,a)));close(values.box.X,.070);
        close(len(minus(values.position,opts.tipFirst?b:a)),0);close(values.forward.Z,opts.tipFirst?-1:1);
        assert.ok(values.range>impact.contactMetrics.X);return Number(started);
      }};
    const w={vrBladeFollowAttempt:7,VRBladeFollowResult:record,VRBladeCaptureCut:capture,VRBladeClipStations:clip,VRBladeConfirmedSerial:()=>4,VRBladeConfirmedAge:()=>.9,
      VRMeleeReach:()=>.75,GetWorldPosition:()=>baseAt(1),VRBladeNoDismember:()=>false,VRBladeNoteProbe:()=>{}};
    const pin={slots,slot:'RightShoulder',fraction:opts.fraction??measuredFraction,edgeFrames:[]};
    const r=w.vrBladeReceipt={owner:player,target:{GetEntityID:()=>17,IsRagdolling:()=>false},lodged:true,puncture:true,
      slots,attachment:pin,serial:4,jointSerial:4,lastEdgeStroke:opts.spentStroke?1:0,seatEdge:edge,material:0};
    ctx.VrBladeRPG.SpecialTarget=()=>opts.targetRules!==false;
    ctx.VrBladeRPG.ValidZoneData=d=>d.ok!==false;
    Object.assign(ctx,{VRMeleeRealTime:()=>time,
      VRBladeSameGeometry:(first,second)=>first!=null && first===second,
      VRBladeSameHitData:(first,second)=>first===second || first.authored===data && second===data || second.authored===data && first===data,
      VRBladeSlotEdgeSample:()=>[v(tick?advance:startAdvance,0,0,1),v(1,0,0)],
      VRBladeEdgeSample:()=>opts.noJoint?[]:[v(-advance,gap,.57,1),v(0,gap,.57,1),v(1,0,0,opts.railRejected?2:5)],
      VRBladeCollectOccupied:(comp,a,b,edge,pinSlot,pinFrame,pose)=>{
        assert.equal(comp,slots);assert.equal(pinSlot,pin.slot);assert.equal(pinFrame,frames[tick]);
        close(len(minus(b,a)),.75);
        if(opts.unavailable) return v(0,0,0,-1);
        if(opts.unresolved) return v(1,0,0,0);
        if(opts.visibleClear || (!tick && opts.freshShape)) return v(0,0,0,0);
        const frame=candidate(),localA=toLocal(frame,a),localB=toLocal(frame,b);
        const center=tick&&opts.changedGeometry?{...capsule[0],W:capsule[0].W+.00001}:capsule[0];
        const sampledData=tick&&opts.changedData?{}:tick&&opts.clone?{authored:data}:data;
        keep.call(pose,'LeftTorso','Chest',tick&&opts.replaced?{}:geometry,sampledData,
          'character_flesh',center,capsule[1],frame,localA,localB,v(measuredFraction,opts.tiny?.0005:.12,0,1));
        if(opts.neighbors){
          keep.call(pose,'Belly','Chest',geometry,opts.protectedNeighbor?protectedData:data,'character_flesh',
            {...center,W:.09},capsule[1],frame,localA,localB,v(measuredFraction,.1,0,1));
        }
        if(opts.duplicate){
          // This second container shares the names and capsule but has a
          // different native shape and only appears in the latest pose.
          if(tick) keep.call(pose,'LeftTorso','Chest',{},data,'character_flesh',center,capsule[1],frame,localA,localB,v(.8,.12,0,1));
        }
        const raw=1+Number(!!opts.duplicate&&!!tick)+Number(!!opts.neighbors);
        return v(raw,raw,pose.shapes.length,0);
      },
      VRBladeCapsuleInterval:(a,b,motion,shaft,center)=>{
        const d=minus(b,a);
        if(Math.abs(d.Z)>.7) return v(measuredFraction,.12,0,1);
        close(a.Y,0);close(b.Y,0);assert.ok(b.X>a.X && b.X<=.0001 && a.X>=-Math.abs(visible)-.0001);
        return v(.5,opts.clippedDepth??len(d)*(center.W===.09?.6:.8),0,1);
      }
    });
    for(tick=0;tick<2;tick++) {
      time+=.022;
      const goal=toWorld(frames[tick],v(tick?advance:startAdvance,0,.57));
      observe.call(w,r,pin,frames[tick],baseAt(tick),tipAt(tick),edge,goal,1);
    }
    tick=1;
    if(opts.stalePose) time+=.1;
    assert.equal(request.call(player,w,opts.stale?3:4,baseAt(1),tipAt(1),opts.flat?v(0,.08,0):v(.08,0,0),baseAt(0),tipAt(0)),
      outcome==='Dispatched',JSON.stringify(opts));
    assert.equal(w.vrBladeFollowGate,outcome,JSON.stringify(opts));
    assert.equal(w.vrBladeFollowSerial,opts.stale?3:4);assert.equal(w.vrBladeFollowAttempt,8);
    if(opts.duplicate) assert.deepEqual(w.vrBladeFollowCounts,v(2,2,1,0));
    if(opts.unresolved) assert.deepEqual(w.vrBladeFollowCounts,v(1,0,0,0));
    if(opts.tiny) assert.deepEqual(w.vrBladeFollowCounts,v(1,1,0,0));
    assert.equal(dispatches,['Dispatched','EffectStart'].includes(outcome)?1:0);
    if(outcome==='Dispatched') close(w.vrBladeFollowGeometry.X,gap);
    cases++;
  }
}
// An actual occupied midpoint is stored in the animated slot and measured
// again against the observed weapon before the corrected wrist can be drawn.
{
  const bind=method('VRBladeBindSlot','r,component,a,b,edge');
  const follow=method('VRBladeFollowSlot','side,serial,a,b,edge,trackedGrip={W:0}');
  const v=(X,Y=0,Z=0,W=1)=>({X,Y,Z,W,valueOf(){return this.X;}});
  let at=0, depth=.12,live=false,missing=false;
  const slot=name=>({GetSlotTransform:s=>s===name&&!missing});
  let component=slot('Chest');
  const record={serial:4,jointSerial:301,lodged:true,slots:component,capsule:[],owner:{VRBladeAllowed:()=>true},
    target:{IsAttached:()=>true,FindComponentByName:()=>component},material:0};
  const w={vrBladeReceipt:record,VRMeleeReach:()=>.75,GetWorldPosition:()=>0,
    VRBladeReceiptInterval:()=>v(0,0,0,0),VRBladeBindSlot:bind,VRBladeRecordEdgePose:()=>{}};
  let publications=0,bindings=0;
  Object.assign(ctx,{VrBladeSlotAttachment:class{depth=0;edgeFrames=[];},Vector4:Object.assign(v,{Distance:(a,b)=>Math.abs(a-b)}),
    VRBladeCollectOccupied:(comp,a,b,edge,pinSlot,frame,pose)=>{
      if(depth>.001 && !missing) pose.shapes.push({name:'Torso',slot:'Chest',frame:at,interval:v(.6,depth,0,1),capsule:[v(1),v(2)]});
    },
    VRBladeCapsuleInterval:()=>v(.6,depth,0,depth>0?1:0),
    WorldPosition:{ToVector4:p=>p},Quaternion:{Transform:(_,p)=>p,Conjugate:q=>q},
    WorldTransform:{GetOrientation:()=>0,TransformInvPoint:(_,p)=>v(p-at),TransformPoint:(_,p)=>v(Number(p)+at)},
    VRBladeAnimatedAttachment:(side,key,animated,q,station,bind)=>{
      assert.equal(key,301);publications++;if(bind) {bindings++;close(station,animated.X);}
      return v(animated.X,0,0,live?3:1);
    }});
  const sample=n=>follow.call(w,1,n,0,.75,1);
  assert.equal(sample(4).W,1);assert.equal(record.attachment.slot,'Chest');close(record.attachment.localPoint.X,.45);
  at=.8;depth=0;live=true;
  close(sample(4).X,1.25);assert.equal(record.attachment.depth,0);assert.equal(bindings,1);
  missing=true;assert.equal(sample(4).W,0);missing=false;
  component=slot('Chest');assert.equal(sample(4).W,0);assert.equal(record.attachment,null);
  assert.equal(publications,2);depth=.1;assert.equal(sample(4).W,3);assert.equal(bindings,2);
  record.released=true;assert.equal(sample(4).W,0);assert.equal(publications,3);cases+=7;
}
console.log(`${cases} offline RPG replay cases and pipeline source contracts passed`);
