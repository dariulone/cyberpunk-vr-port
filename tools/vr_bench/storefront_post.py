"""Storefront post fixture. Sampling is read-only; all motion is generated before timing."""
import ctypes
import math
from pathlib import Path
import statistics
import struct
import subprocess
import time

POST = (-1437.15, 1259.185, 24.80)
FOOT = (-1436.76, 1259.75, 23.09)
# Observed solved wrist on kind-4 static contact in post-ab01. Used only for
# commanding the reach. The separate POST landmark and native gates judge it.
CONTACT_PATCH = (-1437.114, 1259.322, 24.68)
FORWARD = (-.56, -.83)
LEFT = (-.27, -.38, -.23)
RIGHT = (.13, -.28, -.27)
STEP = .10
AXIS_LENGTH = math.dist(POST[:2], FOOT[:2])
PRESS_AXIS = tuple((p-f)/AXIS_LENGTH for p,f in zip(POST[:2], FOOT[:2]))
NS = 'cvr::physhands::'
BODY = 's_currentBodyFrame'
STEPS = NS + 'steps'
LISTS = NS + 'nearLists'
SYMBOLS = (BODY, STEPS, LISTS, NS + 'nearFront', NS + 'scene', 'g_hasWeaponEquipped')
TYPES = tuple('symbol:' + name for name in (BODY, STEPS, STEPS+'/data', STEPS+'/data/motor',
              STEPS+'/data/motor/touch', LISTS, LISTS+'/s', LISTS+'/s/bounds'))


def heading_error(quat, direction):
    yaw = 2 * math.atan2(quat[2], quat[3])
    return abs(math.degrees(math.remainder(yaw - math.atan2(-direction[0], direction[1]), math.tau)))


def forward(quat):
    yaw = 2*math.atan2(quat[2],quat[3])
    return (-math.sin(yaw), math.cos(yaw))


def generate(exe, base, cycles, station, wrists, patch=CONTACT_PATCH, target=CONTACT_PATCH,
             profile=(.055,0.,0.,0.), neutral_world=None):
    values = [*POST, *station, *RIGHT, STEP, *(x for p in wrists for x in p),
              *(neutral_world if neutral_world is not None else wrists[0]), *patch, *target, *profile]
    result = subprocess.run([str(exe), str(cycles), 'settled-dwell'], input=' '.join(map(str, values)),
                            capture_output=True, text=True, timeout=10)
    if result.returncode:
        raise RuntimeError('Post trace generator: ' + result.stderr.strip())
    frames = []
    for line in result.stdout.splitlines():
        t, yaw, x, y, z, qx, qw, phase = line.split()
        frames.append(dict(t=float(t), head=dict(base, yaw=base['yaw']+float(yaw)),
                           left=LEFT, right=list(map(float, (x,y,z))), rightQuat=(float(qx),0.,0.,float(qw)), phase=phase))
    if len(frames) != cycles*40:
        raise ValueError('Incomplete post trace')
    return frames


class Post:
    def __init__(self, game, layout):
        self.game = game
        self.types = {k[7:]:layout[k] for k in TYPES}
        self.ticks = ctypes.windll.kernel32.GetTickCount64
        self.ticks.restype = ctypes.c_ulonglong
        self.scene = game.u64(NS+'scene')
        self.owner = None
        self.yaw = None
        self.collider = None
        self.probe_foot = None
        self.timed_foot = None
        self.restage_wait = 0.

    def offset(self, name, field):
        return self.types[name]['fields'][field]['offset']

    def body(self):
        data = self.game.read(self.game.addr(BODY), self.types[BODY]['size'])
        def get(fmt, name):
            return struct.unpack_from(fmt, data, self.offset(BODY, name))
        return dict(foot=get('<3f', 'position'), quat=get('<4f', 'rotation'),
                    owner=get('<Q', 'owner')[0], ageMs=self.ticks()-get('<Q', 'stamp')[0])

    def placement(self, body):
        if not self.scene or self.game.u64(NS+'scene') != self.scene or not body['owner']:
            return 'game world unavailable or changed'
        if self.game.read(self.game.addr('g_hasWeaponEquipped'), 1) != b'\0':
            return 'post case requires empty hands'
        if not 0 <= body['ageMs'] <= 250 or (self.owner is not None and body['owner'] != self.owner):
            return 'player transform stale or player changed'
        if (not all(math.isfinite(v) for v in (*body['foot'], *body['quat'])) or
                abs(math.hypot(*body['quat'])-1) > .01 or
                math.dist(body['foot'], FOOT) > (.07 if self.owner is None else .20) or
                heading_error(body['quat'], FORWARD) > 10):
            return f'place the player at {FOOT} facing 146 degrees'
        yaw = 2*math.atan2(body['quat'][2], body['quat'][3])
        if self.yaw is not None and abs(math.remainder(yaw-self.yaw, math.tau)) > math.radians(3):
            return 'player heading changed more than 3 degrees'
        if self.probe_foot and math.dist(body['foot'],self.probe_foot)>.10:
            return 'player moved over 10 cm within a post probe'
        return None

    def step(self):
        g, name = self.game, STEPS+'/data'
        for _ in range(8):
            seq = g.u32(STEPS, self.offset(STEPS, 'seq'))
            if not seq or seq & 1:
                continue
            data = g.read(g.addr(STEPS)+self.offset(STEPS,'data'), self.types[name]['size'])
            if seq == g.u32(STEPS, self.offset(STEPS,'seq')):
                break
        else:
            return None
        def get(fmt, field, skip=0):
            return struct.unpack_from(fmt, data, self.offset(name, field)+skip)
        origin = get('<3f','origin')
        def world(field, skip):
            return [a+b for a,b in zip(get('<3f',field,skip), origin)]
        # Right hand = entry 1. pose/sent use xyzw + xyz; sentTrack is an xyz array.
        wrist, target, tracked = world('pose', 44), world('sent', 44), world('sentTrack', 12)
        motor = name+'/motor'
        touch = self.offset(name,'motor') + self.types[motor]['size'] + self.offset(motor,'touch')
        def contact(fmt, field):
            return struct.unpack_from(fmt, data, touch+self.offset(motor+'/touch',field))[0]
        at = get('<Q','at')[0]
        inward = sum((target[k]-wrist[k])*PRESS_AXIS[k] for k in range(2))
        return dict(serial=get('<Q','serial')[0], at=at, ageMs=self.ticks()-at, origin=origin,
                    rightValid=get('<?','valid',1)[0], onWorld=get('<?','onWorld',1)[0],
                    kind=contact('<i','kind'), depthM=contact('<f','depth'), index=contact('<i','index'),
                    wrist=wrist, target=target, tracked=tracked, refusedM=math.dist(wrist,target),
                    inwardRefusedM=inward)

    def near(self, step, foot):
        """Read the published list header and, for static contacts, its measured collider bounds."""
        g = self.game
        front = g.i32(NS+'nearFront')
        if front not in (0,1):
            return dict(fresh=False)
        addr = g.addr(LISTS) + front*self.types[LISTS]['size']
        start = self.offset(LISTS,'n')
        header = g.read(addr+start, self.types[LISTS]['size']-start)
        def get(fmt, name):
            return struct.unpack_from(fmt, header, self.offset(LISTS,name)-start)
        n, at = get('<i','n')[0], get('<Q','at')[0]
        origin = get('<3f','origin')
        center = [a+b for a,b in zip(origin,get('<3f','center'))]
        age = self.ticks()-at
        count_limit = (start-self.offset(LISTS,'s')) // self.types[LISTS+'/s']['size']
        fresh = (0 < n <= count_limit and 0 <= age <= 5000 and step is not None and
                 origin == step['origin'] and math.dist(center, (*foot[:2], foot[2]+1.1)) <= .5)
        evidence = dict(fresh=fresh, ageMs=age, count=n, stamp=at, center=center)
        if (fresh and at < step['at'] and step['onWorld'] and step['kind'] in (1,4)
                and 0 <= step['index'] < n):
            shape = LISTS+'/s'
            ptr = addr + self.offset(LISTS,'s') + step['index'] * self.types[shape]['size']
            bounds = g.read(ptr+self.offset(shape,'bounds'), self.types[shape+'/bounds']['size'])
            def bound(field):
                v = struct.unpack_from('<3f', bounds, self.offset(shape+'/bounds',field))
                return [a+b for a,b in zip(origin,v)]
            lo, hi = bound('lo'), bound('hi')
            evidence['bounds'] = [lo, hi]
            evidence['postBoundsGapM'] = math.dist(POST, [max(a,min(b,p)) for a,b,p in zip(lo,hi,POST)])
            if self.collider:
                evidence['sameColliderBounds'] = all(math.dist(a,b)<.02 for a,b in zip(self.collider,(lo,hi)))
        if g.i32(NS+'nearFront') != front or g.read(addr+self.offset(LISTS,'at'),8) != struct.pack('<Q',at):
            return dict(fresh=False, publicationChanged=True)
        return evidence

    def sample(self, enabled):
        body = self.body()
        step = self.step()
        problem = self.placement(body)
        if not step or not step['serial'] or not 0 <= step['ageMs'] <= 250:
            problem = problem or 'physics fetch is stale'
        near = self.near(step, body['foot'])
        if near.get('publicationChanged'):
            near = self.near(step,body['foot'])
        if enabled and (not step or not step['rightValid'] or not near['fresh']):
            problem = problem or 'right hand or local static-shape list is not ready'
        return dict(body=body, step=step, near=near, problem=problem)

    def ready(self, enabled):
        end = time.monotonic()+12
        previous = None
        while time.monotonic() < end:
            s = self.sample(enabled)
            if not s['problem']:
                serial = s['step']['serial']
                if previous is not None and serial > previous:
                    return s
                previous = serial
            time.sleep(.04)
        raise RuntimeError(s['problem'] or 'physics fetch serial did not advance')

    def retract(self, sim, base, send_pose, enabled):
        self.probe_foot = None
        send_pose(sim,dict(head=base,left=LEFT,right=RIGHT))
        time.sleep(.25)
        end = time.monotonic()+4
        previous = []
        while time.monotonic() < end:
            s = self.sample(enabled)
            st = s['step']
            if (not s['problem'] and (not enabled or not st['onWorld'])
                    and (not previous or st['serial']>previous[-1]['step']['serial'])):
                previous.append(s)
                previous = previous[-4:]
                if len(previous)==4 and all(math.dist(x['body']['foot'],s['body']['foot'])<.007 for x in previous):
                    self.probe_foot = s['body']['foot']
                    return s
            else:
                previous.clear()
            time.sleep(.07)
        raise RuntimeError(s['problem'] or 'controller did not release and feet settle after retraction')

    def calibrate(self, sim, base, cycles, exe, send_pose, world_count, record, attempts):
        s = self.retract(sim,base,send_pose,True)
        body = s['body']
        self.owner = body['owner']
        self.yaw = 2*math.atan2(body['quat'][2], body['quat'][3])
        sim.hand('left', pos=LEFT, quat=(0,0,0,1), mode='body')
        wrists = []
        for axis in (None, 0, 1, 2, None):
            xyz = list(RIGHT)
            if axis is not None:
                xyz[axis] += STEP
            sim.hand('right', pos=xyz, quat=(0,0,0,1), mode='body')
            time.sleep(.35)
            samples = []
            for _ in range(3):
                samples.append(self.ready(True)['step']['tracked'])
            if max(math.dist(p,samples[0]) for p in samples) > .025:
                raise RuntimeError('Native wrist unstable during calibration')
            wrists.append([statistics.median(col) for col in zip(*samples)])
        if math.dist(wrists[0],wrists[-1]) > .03:
            raise RuntimeError('Neutral wrist calibration failed to repeat within 3 cm')
        info = dict(post=POST, seedPatch=CONTACT_PATCH, foot=body['foot'], headingRad=self.yaw, neutral=RIGHT,
                    axisStep=STEP, measuredWrists=wrists, generator=str(Path(exe)), attempts=[])

        def attempt(frames, label, start_pose):
            samples, counts, delivery = [], [world_count()], []
            start = time.perf_counter()
            for frame in frames:
                time.sleep(max(0,start+frame['t']-time.perf_counter()))
                send_pose(sim,frame)
                delivery.append(time.perf_counter()-start)
                samples.append(self.sample(True))
                counts.append(world_count())
            contacts = sum(b-a for a,b in zip(counts,counts[1:]) if b>=a)
            cycles_loaded, issues = assess(frames,samples,True,dict(worldStepsRight=contacts))
            if any(t-f['t']>=.1 for t,f in zip(delivery,frames)):
                issues.append('calibration pose deadline missed')
            info['attempts'].append(dict(label=label, frames=frames, samples=samples, worldSteps=contacts,
                                         start=start_pose, deliverySeconds=delivery, cycles=cycles_loaded, issues=issues))
            record(info)
            return samples, issues

        # The first path is a locator. A transient is sufficient to identify
        # a pad, but cannot qualify the measured workload.
        start_pose = self.retract(sim,base,send_pose,True)
        locator = generate(exe,base,1,start_pose['body']['foot'],wrists[:4],neutral_world=start_pose['step']['tracked'])
        samples, _ = attempt(locator,'locate',start_pose)
        hits = [s for s in samples if static_post_contact(s)]
        if not hits:
            raise RuntimeError('No native static contact near the post during bind; inspect postCalibration.attempts')
        seed = max(hits,key=lambda s:s['step']['inwardRefusedM'])
        self.collider = seed['near']['bounds']
        patch, target = seed['step']['wrist'], seed['step']['target']
        info.update(bind=seed, criterionVersion='steady-dwell-1')
        # Fixed, bounded alternatives. Try the original wrist orientation,
        # then turn the pad and sample adjacent points on the same collider.
        profiles = ((.045,0.,-.02,0.), (.075,0.,-.025,-35.), (.075,0.,-.025,35.),
                    (.080,.07,-.04,-35.), (.080,-.07,-.04,35.),
                    (.095,-.045,-.065,-55.), (.095,.045,-.065,55.))
        best_steady = 0.
        for profile in profiles[:attempts]:
            start_pose = self.retract(sim,base,send_pose,True)
            station, neutral = start_pose['body']['foot'], start_pose['step']['tracked']
            check = generate(exe,base,1,station,wrists[:4],patch,target,profile,neutral)
            first, issues = attempt(check,profile,start_pose)
            info['attempts'][-1].update(patch=patch, motorTarget=target)
            if issues:
                # Learn from the settled part only. Rebind the next candidate
                # to an observed pad/target on the same geometry, within the
                # original patch's neighborhood. Never move the landmark or gate.
                steady = [s for s in first[16:24] if static_post_contact(s)
                          and math.dist(s['step']['wrist'],seed['step']['wrist']) < .12]
                if len(steady)>=3:
                    steady.sort(key=lambda s:s['step']['inwardRefusedM'])
                    measured = steady[len(steady)//2]['step']
                    if measured['inwardRefusedM'] > best_steady+.001:
                        best_steady = measured['inwardRefusedM']
                        patch, target = measured['wrist'], measured['target']
                continue
            verify_start = self.retract(sim,base,send_pose,True)
            if math.dist(verify_start['body']['foot'],station)>.02:
                info['attempts'][-1]['issues'].append('feet shifted over 2 cm before verification')
                record(info)
                continue
            _, issues = attempt(check,'verify same profile',verify_start)
            if not issues:
                settled = self.retract(sim,base,send_pose,True)
                if math.dist(settled['body']['foot'],station)>.02:
                    info['attempts'][-1]['issues'].append('feet shifted over 2 cm after verification')
                    record(info)
                    continue
                self.timed_foot = station
                info.update(profile=profile, qualifiedPatch=patch, qualifiedTarget=target,
                            timedFoot=station, settled=settled, qualified=True)
                record(info)
                return generate(exe,base,cycles,station,wrists[:4],patch,target,profile,neutral), info
        self.retract(sim,base,send_pose,True)
        raise RuntimeError('No dwell passed twice on the bound collider; see short calibration attempts')

    def timed_start(self, sim, base, send_pose, enabled):
        body = self.retract(sim,base,send_pose,enabled)['body']
        if math.dist(body['foot'],self.timed_foot)<=.02:
            return body
        message = f'Restage at qualified foot {self.timed_foot}, yaw {math.degrees(self.yaw):.1f}; measured {body["foot"]}'
        if self.restage_wait:
            print(message, f'(waiting up to {self.restage_wait:g} s before timing)', flush=True)
            end = time.monotonic()+self.restage_wait
            while time.monotonic()<end:
                time.sleep(.2)
                body=self.body()
                if not self.placement(body) and math.dist(body['foot'],self.timed_foot)<=.02:
                    body=self.retract(sim,base,send_pose,enabled)['body']
                    if math.dist(body['foot'],self.timed_foot)<=.02:
                        return body
        raise RuntimeError(message)


def static_post_contact(s):
    if s['problem'] or not s['step'] or not s['near']:
        return False
    st, near = s['step'], s['near']
    return (st['onWorld'] and st['kind'] in (1,4) and st['depthM']>0 and math.dist(st['wrist'],POST)<=.40
            and near.get('postBoundsGapM',math.inf)<=.15 and near.get('sameColliderBounds',True))


def assess(frames, samples, enabled, counters):
    """Each commanded reach must load the right wrist against the post area and then release it."""
    issues = {s['problem'] for s in samples if s['problem']}
    cycles = []
    last_serial = None
    consecutive = 0
    for i,(frame,s) in enumerate(zip(frames,samples,strict=True)):
        if i % 40 == 0:
            cycles.append(dict(loaded=0, steadyRun=0, released=0, staticPressSamples=0,
                               maxDepthM=0., maxInwardRefusedM=0., maxPressClampM=0.))
        cycle = cycles[-1]
        st = s['step']
        qualified = (enabled and frame['phase']=='press' and i%40>=16 and static_post_contact(s)
                     and st['inwardRefusedM']>=.005)
        consecutive = consecutive+1 if qualified else 0
        cycle['steadyRun'] = max(cycle['steadyRun'],consecutive)
        if not st:
            continue
        if last_serial is not None and st['serial'] <= last_serial:
            issues.add('physics fetch serial stalled or reset')
        last_serial = st['serial']
        if not enabled or s['problem']:
            continue
        if frame['phase']=='press':
            clamp = math.dist(st['tracked'],st['target'])
            cycle['maxPressClampM'] = max(cycle['maxPressClampM'],clamp)
        if frame['phase']=='press' and static_post_contact(s):
            cycle['staticPressSamples'] += 1
            cycle['maxDepthM'] = max(cycle['maxDepthM'],st['depthM'])
            cycle['maxInwardRefusedM'] = max(cycle['maxInwardRefusedM'],st['inwardRefusedM'])
            if st['inwardRefusedM']>=.005:
                cycle['loaded'] += 1
        if i%40 >= 32 and not st['onWorld']:
            cycle['released'] += 1
    if enabled and any(c['steadyRun']<3 or c['released']<2 for c in cycles):
        issues.add('post press/release unconfirmed in one or more cycles')
    if enabled and counters['worldStepsRight'] < 3*len(cycles):
        issues.add('insufficient native right-hand world-contact steps')
    if not enabled and counters['worldStepsRight']:
        issues.add('right-hand world contacts continued with physics off')
    return cycles, sorted(issues)
