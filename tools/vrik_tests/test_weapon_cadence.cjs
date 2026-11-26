// Exercise the production REDscript decisions with a controllable clock.
// test_player_weapon_balance.py checks their callers in the compiled retail PSM.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname,
    '../../mods/redscript/CyberpunkVRPort_NoAnims/vrport_weapon_trigger.reds'), 'utf8');

const game = {time: 0, vr: true, finger: 0n, dual: false, leftDown: false, rightDown: false};
const env = vm.createContext({
    VRWeaponTriggerEnabled: () => game.vr,
    VRCurrentFingerAction: () => game.finger,
    IsDefined: obj => obj != null,
    asPlayer: obj => obj?.isPlayer ? obj : null,
    GameInstance: {GetEngineTime: g => g.time},
    EngineTime: {ToDouble: t => t},
    gamedataTriggerMode: {SemiAuto: 'SemiAuto', FullAuto: 'FullAuto'},
    VRDualWieldTriggers: {Active: () => game.dual, Left: () => game.leftDown, Right: () => game.rightDown},
    GetAllBlackboardDefs: () => ({PlayerStateMachine: {
        QuestForceShoot: 'quest', IsDualWielding: 'dual', MountedToVehicle: 'vehicle'
    }}),
    StatusEffectSystem: {ObjectHasStatusEffect: () => false}
});

function method(cls, name, native) {
    const decl = new RegExp('@(?:add|wrap)Method\\(' + cls + '\\)\\s+[^@{]+?func ' + name +
        '\\(([^)]*)\\) -> [\\w<>]+ \\{').exec(source);
    assert.ok(decl, cls + '.' + name);
    const args = decl[1].split(',').filter(Boolean).map(a => a.split(':')[0].trim().split(' ').at(-1));
    const start = decl.index + decl[0].length;
    let end = start, depth = 1;
    while (depth && end < source.length) {
        const c = source[end++];
        depth += (c === '{') - (c === '}');
    }
    assert.equal(depth, 0);
    const body = source.slice(start, end - 1).replace(/\/\/[^\n]*/g, '')
        .replace(/let (\w+): [^;]+;/g, 'let $1;')
        .replace(/scriptInterface\.executionOwner as PlayerPuppet/g, 'asPlayer(scriptInterface.executionOwner)')
        .replace(/Cast<(?:Int32|Uint32)>/g, 'Number')
        .replace(/\b[nt]"/g, '"').replace(/\b0ul\b/g, '0n')
        .replace(/(\d)[ud]\b/g, '$1').replace(/\bif ([^{]+)\{/g, 'if ($1) {');
    return vm.runInContext('(wrappedMethod => function(' + args + '){' + body + '})', env)(native);
}

function gun() {
    const w = {vrLastFingerAction: 0n};
    for (const name of ['VRHasFreshFingerAction', 'VRConsumeFingerAction', 'VRDiscardEarlyPress', 'VRRejectedPress']) {
        w[name] = method('WeaponObject', name);
    }
    return w;
}

function hand(player, left) {
    const state = {
        weapon: gun(), forced: false, selected: true, mode: 'SemiAuto',
        value: 0, count: 0, age: 0, flags: {}, lastCount: {valid: false},
        GetBoolParameter() { return Boolean(this.chargedAuto); },
        GetTemporaryBoolParameter() { return {value: this.forced}; },
        GetPermanentIntParameter() { return this.lastCount; }
    };
    const si = {
        executionOwner: player, GetGame: () => game,
        IsTriggerModeActive: mode => mode === state.mode,
        GetActionPressCount: () => state.count, GetActionValue: () => state.value,
        GetActionStateTime: () => state.age, IsActionJustPressed: () => state.age === 0,
        localBlackboard: {GetBool: key => Boolean(state.flags[key])}
    };
    const d = {
        IsLeftWeaponLogic: () => left, GetWeaponObject: () => state.weapon,
        IsInVisionModeActiveState: () => false, IsInValentinosFrenzy: () => false,
        GetQuestForceShootName: () => 'quest', GetLastChargePressCount: () => 'count',
        GetIsChargedFullAutoName: () => 'charged',
        ShouldShoot: () => state.selected
    };
    for (const name of ['VRCadencePlayer', 'VRShotIntervalReady', 'VRFingerSemiAuto', 'VRDualHandReleased', 'VRCanBeginCharge']) {
        d[name] = method('WeaponTransition', name);
    }
    let nativeReady = true;
    const seq = method('WeaponTransition', 'CanPerformNextShotInSequence', () => nativeReady);
    const action = method('WeaponTransition', 'IsSemiAutoAction', () => nativeReady);
    const receipt = method('ShootEvents', 'OnEnter', () => {
        state.lastCount = {valid: true, value: state.count};
    });
    return {
        state,
        shot: () => receipt.call(d, state, si),
        semi: () => action.call(d, state.weapon, state, si),
        sequence(allowed = true) {
            nativeReady = allowed;
            return seq.call(d, state.weapon, state, si);
        },
        gate(cls, name, allowed = true) {
            return method(cls, name, () => allowed).call(d, state, si);
        }
    };
}

const player = {isPlayer: true, PlayerLastUsedKBM: () => false, vrNextLeftShotTime: 0, vrNextRightShotTime: 0};
const right = hand(player, false);
const left = hand(player, true);
const gates = [
    ['BurstDecisions', 'ToShoot'],
    ['ChargeDecisions', 'EnterCondition'],
    ['CycleTriggerModeDecisions', 'ToCharge']
];

// Querying the native timer sets no deadline. Each hand has a free first round.
right.state.mode = 'FullAuto';
assert.equal(right.sequence(false), false);
right.state.mode = 'SemiAuto';
assert.equal(right.sequence(), true);
right.shot();
assert.equal(left.sequence(), true);
left.shot();
for (const h of [left, right]) {
    h.state.weapon = gun(); // same slot, different item
    for (const mode of ['SemiAuto', 'FullAuto', 'Burst', 'Charge']) {
        h.state.mode = mode;
        game.time = .099999;
        assert.equal(h.sequence(), false, mode);
        for (const [cls, name] of gates) assert.equal(h.gate(cls, name), false, cls);
        game.time = .1;
        h.state.count++;
        h.state.value = 1;
        assert.equal(h.sequence(), true, mode);
        for (const [cls, name] of gates) {
            assert.equal(h.gate(cls, name), true, cls);
            assert.equal(h.gate(cls, name, false), false, 'native rejection in ' + cls);
        }
        assert.equal(h.sequence(false), mode === 'SemiAuto', mode + ' with native cycle closed');
    }
}

// Each accepted burst round starts a new window. Repeated probes and slow
// authored burst timers cannot reserve a shot or move either hand's deadline.
game.time = .35;
assert.equal(right.gate(...gates[0], false), false);
game.time = .4;
right.shot();
game.time = .499;
assert.equal(right.gate(...gates[0]), false);
assert.equal(left.sequence(), true);
game.time = .5;
assert.equal(right.gate(...gates[0]), true);

// The XR token precedes its XInput count. Discard the too-early edge, including
// its delayed native echo after release. A fresh XR edge still wins with the
// same native count. Use Liberty's 275 ms gamepad cycle throughout the take.
for (const h of [right, left]) {
    h.state.mode = 'SemiAuto';
    game.time = 1;
    game.finger = 11n; h.state.count = 7; h.state.value = 1;
    h.shot();
    game.time = 1.05;
    game.finger = 12n; h.state.value = h === left ? 1 : 0; // left still sees the previous count held
    const libertyCycleReady = () => game.time > 1 + .275;
    assert.equal(h.sequence(libertyCycleReady()), false);
    assert.equal(h.sequence(libertyCycleReady()), false, 'polling the same edge must preserve its echo veto');
    game.time = 1.1;
    assert.equal(h.sequence(libertyCycleReady()), true);
    assert.equal(h.semi(), false, 'holding a discarded pull through the deadline');
    game.finger = 0n; h.state.count = 8; h.state.value = 1;
    assert.equal(h.semi(), false, 'late native echo after release');
    game.finger = 13n;
    assert.equal(libertyCycleReady(), false);
    assert.equal(h.sequence(false), true, 'fresh Liberty pull at 100 ms, native cycle is 275 ms');
    assert.equal(h.semi(), true);
    h.shot();
    game.time = 1.199999;
    game.finger = 14n; h.state.count = 9;
    assert.equal(h.sequence(false), false);
    game.time = 1.5;
    assert.equal(h.semi(), false, 'rejected before the new accepted-shot deadline');
    game.finger = 0n;
    assert.equal(h.semi(), false);
    h.state.count = 10;
    assert.equal(h.semi(), true, 'next independent native press');
    h.state.value = 0;
    assert.equal(h.semi(), false);
}

// A too-early charge tap and release must not enter charge and immediately
// discharge when the timer expires, even with a late native input callback.
right.state.mode = 'Charge';
game.time = 1.6;
right.shot();
game.time = 1.65;
game.finger = 15n;
assert.equal(right.gate(...gates[1]), false);
game.time = 1.71;
game.finger = 0n; right.state.count++; right.state.value = 1;
assert.equal(right.gate(...gates[1]), false);
assert.equal(right.gate(...gates[2]), false);
game.finger = 16n;
assert.equal(right.gate(...gates[1]), true);

game.finger = 0n;
game.dual = true;
game.rightDown = true;
for (const h of [right, left]) {
    h.state.mode = 'FullAuto';
    assert.equal(h.gate('WeaponTransition', 'VRReleasedFullAuto', false), h === left);
    h.state.chargedAuto = true;
    assert.equal(h.gate('WeaponTransition', 'VRReleasedFullAuto', false), false);
    h.state.chargedAuto = false;
}
left.state.mode = 'SemiAuto';
left.state.value = 1; left.state.count++;
assert.equal(left.semi(), false);
game.leftDown = true;
assert.equal(left.semi(), true);
game.dual = false;

game.time = 2;
right.shot();
game.time = 2.01;
assert.equal(right.sequence(), false);
game.vr = false;
assert.equal(right.sequence(), true);
right.state.mode = 'SemiAuto';
assert.equal(right.sequence(false), false, 'native semi-auto retains the gamepad cycle');
game.vr = true;
for (const flag of ['forced', 'blackboard']) {
    right.state.forced = flag === 'forced';
    right.state.flags.quest = flag === 'blackboard';
    assert.equal(right.sequence(), true);
    assert.equal(right.sequence(false), false);
}
const npc = hand({isPlayer: false}, false);
npc.shot();
assert.equal(npc.sequence(), true);
console.log('PASS Liberty fresh pulls at 100 ms with a 275 ms native cycle, independent hands, native mode timing and late echoes.');
