// node --test tools/vrik_tests/test_npc_melee_reaction.cjs
// Replay the production REDscript methods with doubles for engine-owned APIs.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const vm = require('node:vm');

const scripts = path.resolve(__dirname, '../../mods/redscript');
const read = name => fs.readFileSync(path.join(scripts, name), 'utf8');
const policy = read('CyberpunkVRPort_Melee/vrport_npc_melee_reaction.reds');
const stroke = read('CyberpunkVRPort_Melee/vrport_contact_stroke.reds');
const rpg = read('CyberpunkVRPort_Melee/vrport_blade_rpg.reds');
const combat = read('CyberpunkVRPort_Cyberware/vrport_cyberware_combat.reds');

function method(context, source, name, params) {
  const start = source.indexOf('func ' + name + '(');
  assert.notEqual(start, -1, name);
  const open = source.indexOf('{', start);
  let end = open + 1, depth = 1;
  while (depth && end < source.length) {
    depth += Number(source[end] === '{') - Number(source[end] === '}');
    end++;
  }
  assert.equal(depth, 0);
  const code = source.slice(open + 1, end - 1).replace(/\/\/[^\n]*/g, '')
    .replace(/let (\w+): ref<\w+>;/g, 'let $1;')
    .replace(/data\.GetInstigator\(\) as PlayerPuppet/g, 'data.GetInstigator()?.player')
    .replace(/ as (Attack_GameEffect|Attack_Melee_Record)/g, '')
    .replace(/\bn"/g, '"').replace(/\b(\d+)u\b/g, '$1').replace(/\bif ([^{}]+)\{/g, 'if ($1) {');
  return vm.runInContext(`(function(${params}) {${code}})`, context, { filename: name });
}

const enums = new Proxy({}, {get: (_, name) => name});
const base = () => vm.createContext({
  IsDefined: x => x != null, Equals: Object.is, NotEquals: (a, b) => a !== b, Min: Math.min,
  hitFlag: enums, animHitReactionType: enums, EAIHitIntensity: enums, AICommandState: enums,
  gamedataAttackSubtype: enums, AttackData: {
    IsMelee: type => ['Melee', 'StrongMelee'].includes(type), IsLightMelee: type => type === 'Melee',
  },
});

function reaction(o = {}) {
  const ctx = base();
  ctx.VrBladeRPG = {Rejected: method(ctx, rpg, 'Rejected', 'data')};
  ctx.VrPhysicalMeleeReaction = {SeverityCap: method(ctx, policy, 'SeverityCap', 'hit,npc')};
  const npc = {
    GetEntityID: () => 9, IsActive: () => !o.inactive, IsAboutToBeDefeated: () => o.dying,
    IsIncapacitated: () => o.incapacitated, IsRagdolling: () => o.ragdoll,
  };
  const record = {AttackSubtype: () => ({Type: () => o.subtype ?? 'ComboAttack'})};
  const attack = {GetRecord: () => record, vrMeleeContactTarget: o.contactTarget ?? 9, vrPhysicalLightChain: !o.heavy};
  const ticket = {confirmed: !o.unconfirmed, attack: o.otherAttack ? {} : attack, target: o.ticketTarget ?? 9, weapon: 7};
  let maximum = o.maximum ?? -1;
  const writes = [], stock = [];
  const data = {
    GetAttackDefinition: () => attack, GetWeapon: () => ({GetItemID: () => o.weapon ?? 7}),
    GetInstigator: () => ({player: o.otherInstigator ? null : {}}), GetAttackType: () => o.type ?? 'Melee',
    WasBlocked: () => o.block, WasDeflectedAny: () => o.parry, HasFlag: f => f === o.flag,
    GetHitReactionSeverityMax: () => maximum, SetHitReactionSeverityMax: x => { writes.push(x); maximum = x; },
  };
  const hit = {target: o.otherReceiver ? {} : npc, attackData: data, projectionPipeline: !!o.projected,
    vrMeleeEnhancedTicket: o.noTicket ? null : ticket, vrSeatedBladeReaction: !!o.seat};
  ctx.wrappedMethod = (impulse, event) => {
    assert.equal(event, hit); assert.equal(impulse, 8);
    stock.push(maximum);
    return o.stockResult ?? 'Impact';
  };
  const component = {m_ownerNPC: npc, m_isAlive: !o.dead, m_hitIntensity: 'Heavy'};
  const select = method(ctx, policy, 'GetReactionType', 'guardBreakImpulse,newHitEvent');
  const result = select.call(component, 8, hit);
  assert.equal(result, o.stockResult ?? 'Impact');
  assert.equal(maximum, o.maximum ?? -1);
  assert.equal(stock.length, 1);
  return {cap: stock[0], writes, intensity: component.m_hitIntensity};
}

test('only the confirmed physical combo lowers the stock selector maximum', () => {
  assert.deepEqual(reaction(), {cap: 1, writes: [1, -1], intensity: 'Light'});
  assert.deepEqual(reaction({maximum: 0, stockResult: 'Twitch'}), {cap: 0, writes: [0, 0], intensity: 'Light'});
  for (const control of [
    {unconfirmed: true}, {noTicket: true}, {projected: true}, {otherAttack: true}, {contactTarget: 8},
    {ticketTarget: 8}, {weapon: 8}, {otherReceiver: true}, {otherInstigator: true}, {dead: true},
    {dying: true}, {ragdoll: true}, {incapacitated: true}, {inactive: true}, {block: true}, {parry: true},
    {heavy: true}, {type: 'StrongMelee'}, {subtype: 'FinalAttack'}, {subtype: 'DeflectAttack'},
    ...['Ranged', 'Explosion', 'Effect', 'Thrown', 'QuickMelee'].map(type => ({type, seat: true})),
    ...['ForceKnockdown', 'HighSpeedMelee', 'StunApplied', 'WasDodged', 'DealNoDamage'].map(flag => ({flag})),
  ]) {
    assert.deepEqual(reaction(control), {cap: -1, writes: [], intensity: 'Heavy'}, JSON.stringify(control));
  }
});

test('the seat is classified on its own queued hit, including a strong entry and follow-through', () => {
  for (const extra of [{}, {subtype: 'FinalAttack'}, {type: 'StrongMelee', heavy: true}]) {
    assert.deepEqual(reaction({seat: true, maximum: 3, stockResult: 'Twitch', ...extra}),
      {cap: 0, writes: [0, 3], intensity: 'Light'});
  }
  assert.match(combat, /attack\.vrPhysicalLightChain = swing\.chain;[\s\S]+?attack\.StartAttack\(\)/);
  assert.match(rpg, /hit\.vrSeatedBladeReaction = r\.awaitingSeat \|\| \(r\.follow && r\.lodged\);/);
  assert.match(rpg, /if !r\.follow[^{]+\{\s*weapon\.VREndBladeMovementHold\(weapon\.vrBladeReceipt\);/);
});

function contact() {
  const ctx = base();
  const pending = new Map(), sent = [], ended = [];
  let now = 20, nextID = 0;
  const ai = {
    GetEnqueuedOrExecutingCommand: () => ai.foreign, SendCommand(command) {
      if (ai.reject) return false;
      command.state = 'Enqueued'; sent.push(command); return true;
    },
    CancelCommand: cmd => ended.push(['cancel', cmd]), StopExecutingCommand: cmd => ended.push(['stop', cmd]),
  };
  const delays = {
    DelayCallback(cb, seconds) { assert.ok(seconds > 0); pending.set(++nextID, cb); return nextID; },
    CancelCallback: id => pending.delete(id),
  };
  const call = method(ctx, policy, 'Call', '');
  Object.assign(ctx, {
    VrBladeMovementHold: class { Call() { call.call(this); } }, AIHoldPositionCommand: class {},
    VRMeleeRealTime: () => now, GameInstance: {GetDelaySystem: () => delays},
    VrBladeRPG: {SpecialTarget: (_, target) => !target.protected},
  });
  const target = {
    IsAttached: () => true, IsActive: () => true, IsRagdolling: () => false, IsIncapacitated: () => false,
    GetGame: () => 1, GetAIControllerComponent: () => ai, VRClearBladeMovementHold: method(ctx, policy, 'VRClearBladeMovementHold', ''),
  };
  const weapon = {
    vrBladeReceipt: {owner: {VRBladeAllowed: () => true}, target, serial: 1, jointSerial: 11, puncture: true},
    VRBeginBladeMovementHold: method(ctx, policy, 'VRBeginBladeMovementHold', 'r'),
    VREndBladeMovementHold: method(ctx, policy, 'VREndBladeMovementHold', 'r'),
    lodge: method(ctx, stroke, 'VRBladeLodgeContact', 'serial'), release: method(ctx, stroke, 'VRBladeReleaseContact', 'serial'),
  };
  weapon.VRBladeReleaseContact = weapon.release;
  function fire(elapsed) {
    now += elapsed;
    assert.equal(pending.size, 1);
    const [id, cb] = pending.entries().next().value;
    pending.delete(id); cb.Call();
  }
  return {ctx, ai, weapon, target, pending, sent, ended, fire};
}

test('one hold across repeated seats and receipt rollover, only its release can cancel it', () => {
  const {weapon, target, pending, sent, ended, fire} = contact();
  weapon.lodge(9); assert.equal(sent.length, 0);
  weapon.lodge(1); weapon.lodge(1);
  const first = weapon.vrBladeReceipt;
  const handle = target.vrBladeMovementHold;
  weapon.vrBladeReceipt = {...first, serial: 2, puncture: false};
  weapon.lodge(2); weapon.release(1); fire(.10);
  weapon.VREndBladeMovementHold({...first, jointSerial: 10});
  weapon.VREndBladeMovementHold.call({}, first);
  assert.equal(target.vrBladeMovementHold, handle); assert.equal(sent.length, 1);
  assert.equal(pending.size, 1); assert.equal(ended.length, 0);
  sent[0].state = 'Executing';
  weapon.release(2); weapon.release(2); weapon.lodge(2);
  assert.equal(sent.length, 1); assert.deepEqual(ended, [['stop', sent[0]]]);
  assert.equal(pending.size, 0); assert.equal(target.vrBladeMovementHold, null);

  weapon.vrBladeReceipt = {...first, serial: 3, jointSerial: 12, lodged: false};
  weapon.lodge(3); handle.Call(); weapon.VREndBladeMovementHold(first);
  assert.equal(sent.length, 2); assert.equal(target.vrBladeMovementHold.command, sent[1]);
});

test('a lost owner, seat or heartbeat releases the exact outstanding command', () => {
  for (const invalidate of [
    w => {w.vrBladeReceipt = null;}, w => {w.vrBladeReceipt.owner = null;},
    w => {w.vrBladeReceipt.owner.VRBladeAllowed = () => false;},
    w => {w.vrBladeReceipt.jointSerial++;}, w => {w.vrBladeReceipt.released = true;},
    w => {w.vrBladeReceipt.target.IsRagdolling = () => true;}, () => {},
  ]) {
    const {weapon, pending, sent, ended, fire} = contact();
    weapon.lodge(1); invalidate(weapon); fire(.55);
    assert.deepEqual(ended, [['cancel', sent[0]]]); assert.equal(pending.size, 0);
  }
});

test('a valid seat after a tracking stall reacquires once and still releases', () => {
  const {weapon, target, sent, ended, fire} = contact();
  weapon.lodge(1); fire(.55);
  assert.equal(target.vrBladeMovementHold, null);
  weapon.vrBladeReceipt = {...weapon.vrBladeReceipt, serial: 2, puncture: false};
  weapon.lodge(2); weapon.lodge(2);
  assert.equal(sent.length, 2); assert.equal(target.vrBladeMovementHold.command, sent[1]);
  weapon.release(1); assert.equal(ended.length, 1);
  weapon.release(2); assert.deepEqual(ended, [['cancel', sent[0]], ['cancel', sent[1]]]);
});

test('target restrictions, existing orders and command rejection acquire nothing', () => {
  for (const reason of ['protected', 'foreign', 'rejected', 'pass-through', 'ordinary slash']) {
    const {ai, weapon, target, sent, ended, pending} = contact();
    target.protected = reason === 'protected'; ai.foreign = reason === 'foreign' ? {} : null;
    ai.reject = reason === 'rejected'; weapon.vrBladeReceipt.released = reason === 'pass-through';
    weapon.vrBladeReceipt.puncture = reason !== 'ordinary slash';
    weapon.lodge(1); weapon.release(1);
    assert.deepEqual([sent.length, ended.length, pending.size], [0, 0, 0], reason);
  }
});

test('incapacitation, ragdoll and either detach clear before forwarding the game event', () => {
  for (const [type, name] of [['NPCPuppet', 'OnIncapacitated'], ['NPCPuppet', 'OnRagdollEnabledEvent'],
    ['NPCPuppet', 'OnDetach'], ['WeaponObject', 'OnDetach']]) {
    const {ctx, weapon, target, sent, ended, pending} = contact();
    weapon.lodge(1);
    let forwards = 0;
    ctx.wrappedMethod = () => { forwards++; assert.equal(target.vrBladeMovementHold, null); return true; };
    const source = policy.slice(policy.indexOf('@wrapMethod(' + type + ')', policy.indexOf('func VRClearBladeMovementHold(')));
    method(ctx, source, name, 'evt').call(type === 'WeaponObject' ? weapon : target, {});
    if (name === 'OnDetach') assert.equal(weapon.vrBladeReceipt.released, true);
    assert.deepEqual(ended, [['cancel', sent[0]]]); assert.equal(pending.size, 0); assert.equal(forwards, 1);
  }
});
