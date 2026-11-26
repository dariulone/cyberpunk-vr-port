// Offline decision replay. Optionally pass the JSON from live_finger_cadence.py.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname,
    '../../mods/redscript/CyberpunkVRPort_NoAnims/vrport_full_auto_release.reds'), 'utf8');
const env = vm.createContext({
    vr: true, dual: false, left: false, finger: 2n, native: true,
    slots: {154: 0, 160: 1}, gamedataTriggerMode: {FullAuto: 'FullAuto'},
    IsDefined: obj => obj != null,
    GetAllBlackboardDefs: () => ({PlayerStateMachine: {QuestForceShoot: 'quest', MountedToVehicle: 'vehicle'}}),
    StatusEffectSystem: {ObjectHasStatusEffect: () => false}
});
Object.assign(env, {
    VRWeaponTriggerEnabled: () => env.vr, VRDualWieldTriggers: {Active: () => env.dual},
    VRLeftHanded: () => env.left, VRCurrentFingerAction: () => env.finger,
    GetVRSharedSlot: slot => env.slots[slot], wrappedMethod: () => env.native
});

function method(name, args) {
    const start = source.indexOf('func ' + name + '(');
    assert.ok(start >= 0, name);
    const first = source.indexOf('{', start);
    let end = first + 1, depth = 1;
    while (end < source.length && depth) {
        const c = source[end++];
        depth += (c === '{') - (c === '}');
    }
    assert.equal(depth, 0);
    const body = source.slice(first+1,end-1).replace(/let (\w+): [^;]+;/g,'let $1;')
        .replace(/ as PlayerPuppet\b/g,'').replace(/\b[nt]"/g,'"').replace(/\b0ul\b/g,'0n')
        .replace(/\bif ([^{]+)\{/g,'if ($1){');
    return vm.runInContext('(function('+args+'){'+body+'})',env);
}

const weapon = {vrAutoFingerSlot:0, vrAutoPressCount:0, vrAutoReleased:false,
    VRAutoReleaseBlocks:method('VRAutoReleaseBlocks','slot,count,travel,fresh')};
const ctx = {charged:false,forced:false,GetBoolParameter(){return this.charged;},
    GetTemporaryBoolParameter(){return {value:this.forced};}};
const si = {mode:'FullAuto',count:77,executionOwner:{},localBlackboard:{GetBool:()=>false},
    IsTriggerModeActive(mode){return this.mode===mode;},GetActionPressCount(){return this.count;}};
const decision = {GetWeaponObject:()=>weapon, IsInValentinosFrenzy:()=>false,
    IsInVisionModeActiveState:()=>false,GetQuestForceShootName:()=>'quest',GetIsChargedFullAutoName:()=>'charged',
    VRReleasedFullAuto:method('VRReleasedFullAuto','stateContext,scriptInterface'),
    entry:method('IsFullAutoAction','weaponObject,stateContext,scriptInterface'),
    shoot:method('ToShoot','stateContext,scriptInterface'), ready:method('ToReady','stateContext,scriptInterface')};
function check(blocked) {
    // A stale native hold keeps the entry and shoot decisions true and ready false.
    env.native=true;
    assert.equal(decision.entry(weapon,ctx,si),!blocked);
    assert.equal(decision.shoot(ctx,si),!blocked);
    env.native=false;
    assert.equal(decision.ready(ctx,si),blocked);
    assert.equal(decision.shoot(ctx,si),false); // native delay/ammo rejection also wins
}
check(false);
env.finger=0n;
env.slots[160]=.21; check(false); // hold survives token expiry and intermediate travel
env.slots[160]=.20; check(true);
env.slots[160]=.4; check(true); // rebound below break cannot reopen the same native press
for (const mode of ['Charge','Burst','SemiAuto']) {si.mode=mode; check(false);}
si.mode='FullAuto';
ctx.charged=true; check(false); ctx.charged=false;
ctx.forced=true; check(false); ctx.forced=false;
env.dual=true; check(false); env.dual=false;
env.vr=false; check(false); env.vr=true;
si.count++; env.slots[160]=0; check(false); // unrelated native press in VR
env.left=true; env.finger=3n; env.slots[154]=.85; check(false);
env.finger=0n; env.slots[154]=.20; check(true);
env.finger=5n; env.slots[154]=.85; check(false); // new XR pull despite unchanged native count
env.left=false;

if (process.argv[2]) {
    const series=JSON.parse(fs.readFileSync(process.argv[2],'utf8')).series;
    const take=series.find(t=>t.mode==='FullAuto' && !t.passed);
    assert.ok(take,'expected the captured full-auto release failure');
    const [press,release]=take.input.edges;
    assert.ok(press.down && !release.down && take.input.edges.length===2);
    const receipts=take.actions.filter(e=>e.event==='receipt' && e.t<=take.prevTime);
    assert.equal(receipts.length,take.original-take.remaining);
    let rejected=0;
    for (const r of receipts) {
        assert.equal(r.value,1);
        si.count=r.count;
        env.finger=BigInt(r.token.replace('ULL',''));
        // A receipt within the release sampling interval may precede reset.
        const released=r.t>release.hi;
        env.slots[160]=released?0:1;
        check(released);
        if (released) ++rejected;
    }
    assert.equal(rejected,3);
    console.log('Replayed Ajax: all three post-release rounds vetoed, earlier receipts permitted.');
}
console.log('PASS full-auto entry, shot and ready decisions, mode/ownership and re-arm checks.');
