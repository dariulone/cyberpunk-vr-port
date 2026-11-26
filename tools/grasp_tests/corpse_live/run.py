"""Attach-only corpse regression. C++ owns all pose/trajectory/calibration math.

The root starts Cyberpunk independently. This program never launches, kills,
restarts, or requests a manual save. A verified native save lock precedes mutations.
"""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import threading
import time
import traceback
import uuid

ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/'tools/sim_harness'))
from simxr import Sim, SIM_DIR, _send, unfocus_game

def lua_literal(value):
    if isinstance(value,(list,tuple)): return '{'+','.join(lua_literal(v) for v in value)+'}'
    if value is None: return 'nil'
    if isinstance(value,bool): return 'true' if value else 'false'
    return json.dumps(value,ensure_ascii=False)

class Bridge:
    def __init__(self,folder): self.folder=folder;self.lock=threading.Lock()
    def call(self,code,kind='eval',timeout=15):
        with self.lock:
            command=self.folder/'command.json';response=self.folder/'response.json'
            if command.exists(): raise RuntimeError('Bridge request already pending; exclusive runtime lease required')
            for _ in range(60):
                try: response.unlink(missing_ok=True);break
                except PermissionError: time.sleep(.025)
            else: raise RuntimeError('Bridge response remains locked')
            key=uuid.uuid4().hex;request={'id':key,'type':kind,'expr' if kind=='eval' else 'code':code}
            staged=self.folder/('corpse-'+key+'.tmp');staged.write_text(json.dumps(request),encoding='utf-8')
            for _ in range(60):
                try: staged.replace(command);break
                except PermissionError: time.sleep(.025)
            else: raise RuntimeError('Bridge command path remains locked')
            end=time.monotonic()+timeout
            while time.monotonic()<end:
                try:
                    answer=json.loads(response.read_text(encoding='utf-8'))
                    if answer.get('id')==key:
                        if not answer.get('ok'): raise RuntimeError(answer)
                        result=answer.get('result')
                        try: return json.loads(result)
                        except (TypeError,ValueError): return result
                except (FileNotFoundError,PermissionError,json.JSONDecodeError): pass
                time.sleep(.025)
            # Never delete a potentially still-executing request or terminate its game.
            raise TimeoutError('Bridge observation timed out; game left running, pending request retained')

class Probe:
    def __init__(self,args):
        self.args=args;self.out=args.out.resolve();self.out.mkdir(parents=True,exist_ok=False)
        self.game=args.game.resolve();self.plugin=self.game/'red4ext/plugins/cyberpunkvrport'
        self.bridge=Bridge(self.plugin/'lua/CVRBridge');self.sim=None;self.seq=0
        self.samples=[];self.phases=[];self.acquisitions=[];self.mass_events=[];self.errors=[];self.recording=False;self.begun=False;self.loaded=False
        self.lock_requested=False;self.lock_verified=False;self.lock_reason=None;self.mutated=False
        self.settings={};self.hand_poses=None;self.expected_proxy=None
    def cpp(self,command,data):
        self.seq+=1;source=self.out/('math-%04d-%s.json'%(self.seq,command))
        source.write_text(json.dumps(data),encoding='utf-8')
        result=subprocess.run([str(self.args.generator),command,str(source)],capture_output=True,text=True)
        if result.returncode: raise RuntimeError('C++ '+command+': '+result.stderr.strip())
        value=json.loads(result.stdout)
        source.with_suffix('.result.json').write_text(json.dumps(value),encoding='utf-8')
        return value
    def call(self,expr): return self.bridge.call(expr)
    def exec(self,code): return self.bridge.call(code,'exec')
    def sample(self,full=False): return self.call('CorpseAudit.sample('+('true' if full else 'false')+')')
    def settings_begin(self):
        paths=[self.plugin/'config/physhands.ini',self.plugin/'config/vrport.ini',
            self.plugin/'lua/CyberpunkVRPort_Holster/holster_places.json',SIM_DIR/'settings.json']
        folder=self.out/'settings';folder.mkdir()
        for i,path in enumerate(paths):
            data=path.read_bytes() if path.exists() else None;self.settings[path]=data
            if data is not None: (folder/('%d-%s'%(i,path.name))).write_bytes(data)
    def settings_apply(self):
        if self.args.trace:
            path=self.plugin/'config/physhands.ini';data=self.settings[path]
            if data is None: raise RuntimeError('physics trace config missing')
            if not re.search(rb'(?m)^trace=\d+',data): raise RuntimeError('no trace setting in physics config')
            path.write_bytes(re.sub(rb'(?m)^trace=\d+',b'trace=1',data))
    def take_save_lock(self):
        self.lock_reason='CPVR_CorpseAudit_'+uuid.uuid4().hex
        self.lock_requested=True
        (self.out/'save-lock-token.json').write_text(json.dumps({'reason':self.lock_reason,'pid':self.args.pid},indent=2),encoding='utf-8')
        (self.out/'save-lock.json').write_text(json.dumps({'reason':self.lock_reason,'status':'requested','pid':self.args.pid},indent=2),encoding='utf-8')
        self.call('CorpseAudit.lockSaving('+lua_literal(self.lock_reason)+')')
        end=time.monotonic()+10
        while time.monotonic()<end:
            state=self.call('CorpseAudit.saveLockState()')
            (self.out/'save-lock.json').write_text(json.dumps(state,indent=2),encoding='utf-8')
            if state['protected']:
                state=self.call('CorpseAudit.confirmSaveLock()')
                self.lock_verified=state['protected'] and state['verified']
                (self.out/'save-lock.json').write_text(json.dumps(state,indent=2),encoding='utf-8')
                if self.lock_verified: return
            time.sleep(.1)
        raise RuntimeError('unique save lock not verified; no gameplay mutations permitted')
    def read_head(self):
        path=SIM_DIR/'runtime_status.json'
        if time.time()-path.stat().st_mtime>5: raise RuntimeError('simulator status is stale; no head-pose snapshot')
        state=json.loads(path.read_text(encoding='utf-8'))['head_tracking'];p=state['position']
        return dict(x=p['x'],y=p['y'],z=p['z'],yaw=state['yaw'],pitch=state['pitch'],roll=state['roll'])
    def begin(self):
        if not self.args.generator.is_file(): raise RuntimeError('build corpse_probe first')
        if not self.call('CorpseAudit == nil'): raise RuntimeError('previous CorpseAudit still exists; restore it before another run')
        if self.call('Game.GetPlayer() == nil'): raise RuntimeError('no spawned player')
        if self.call('Game.GetBlackboardSystem():Get(Game.GetAllBlackboardDefs().UI_System):GetBool(Game.GetAllBlackboardDefs().UI_System.IsInMenu)'):
            raise RuntimeError('close native menus before running regression')
        if self.call('type(VRPhysHeldBody) ~= "function"'): raise RuntimeError('deployed DLL lacks completed-hold diagnostic')
        if self.call('type(VRBodySolverMass) ~= "function"'): raise RuntimeError('deployed DLL lacks uncached physical mass diagnostic')
        self.sim=Sim(self.args.pid,focus_away=False)
        if self.sim.status().get('recording'): raise RuntimeError('another simulator recording is active')
        self.original_head=self.read_head()
        (self.out/'simulator-original.json').write_text(json.dumps({'pid':self.args.pid,'head':self.original_head,
            'controllers':[v.hex() for v in self.sim.saved],
            'body':[v.hex() for v in self.sim.saved_body] if self.sim.body_state else None},indent=2),encoding='utf-8')
        self.settings_begin()
        self.exec((Path(__file__).with_name('bridge.lua')).read_text(encoding='utf-8'))
        self.loaded=True
        self.original=self.call('CorpseAudit.begin()');self.begun=True
        (self.out/'original.json').write_text(json.dumps(self.original,indent=2),encoding='utf-8')
        self.take_save_lock()
        self.mutated=True
        self.settings_apply()
        unfocus_game()
        self.exec('CorpseAudit.prepare()')
        self.sim.grip('left',0);self.sim.grip('right',0);self.sim.stick('left',0,0);self.sim.stick('right',0,0)
        self.sim.hands_follow_head(False);self.sim.body_yaw(0)
        initial=self.cpp('initial',dict(self.original,spawnDistance=self.args.spawn_distance));self.hand_poses=initial['hands']
        _send('head_pose_command.json',initial['head'])
        for side in (0,1): self.put(side,self.hand_poses[side])
        time.sleep(1)
        self.exec('CorpseAudit.spawn('+lua_literal(initial['spawn'])+','+lua_literal(self.args.record)+')')
        for _ in range(40):
            if self.call('CorpseAudit.kill()'): break
            time.sleep(.25)
        else: raise RuntimeError('NPC did not spawn; game is still running')
        time.sleep(4)
        sample=self.sample(True)
        if not sample['dead'] or len(sample['slots'])<10: raise RuntimeError('NPC death/slot data not established')
        self.shot('dead-ragdoll')
    def put(self,side,pose):
        self.sim.hand('left' if side==0 else 'right',pos=pose['p'],quat=pose['q'],mode='body')
        self.hand_poses[side]=pose
    def path(self,mode,seconds=1.5,**extra):
        return self.cpp('motion',dict(mode=mode,seconds=seconds,hands=self.hand_poses,**extra))
    def play(self,path):
        start=time.perf_counter()
        for frame in path['frames']:
            delay=start+frame['t']-time.perf_counter()
            if delay>0: time.sleep(delay)
            for side,pose in enumerate(frame['hands']): self.put(side,pose)
            if 'head' in frame: _send('head_pose_command.json',frame['head'])
            for which,value in frame.get('sticks',{}).items(): self.sim.stick(which,*value)
        self.sim.stick('left',0,0);self.sim.stick('right',0,0)
    def shot(self,name):
        files=self.sim.shot(self.out/name,eyes='both',eye_size=1600)
        print(json.dumps({'shot':name,'files':files}),flush=True)
    def diagnostic(self,name):
        diag=self.plugin/'log/vrik_diag.txt';start=diag.stat().st_size if diag.exists() else 0
        self.exec('LogVRDiag(0,0,0,0,0,0,1)')
        with diag.open('rb') as source: source.seek(start);text=source.read().decode('utf-8','replace')
        (self.out/(name+'-vrik-diag.txt')).write_text(text,encoding='utf-8')
        (self.out/(name+'-state.json')).write_text(json.dumps(self.sample(True),indent=2),encoding='utf-8')
        return text
    def phase(self,name,mode='hold',seconds=3,hands=(1,),clip=False,**options):
        criteria={k:options.pop(k) for k in tuple(options) if k in {'relation','minimumTravel','minimumCorpseTravel','minimumRotation','minimumReachSeconds','released','maximumSlip95'}}
        phase=dict(name=name,hands=list(hands),minimumSeconds=max(0,seconds-.4),**criteria)
        self.phases.append(phase);errors=[];stop=threading.Event()
        def sampling():
            while not stop.is_set():
                try:
                    row=self.sample();row.update(phase=name,t=time.perf_counter());self.samples.append(row)
                except Exception as error: errors.append(str(error));return
                stop.wait(.1)
        if clip:
            self.sim.record_start(str(self.out/(name+'.mp4')),eyes='both',eye_size=1440,fps=30,max_seconds=seconds+5)
            self.recording=True
        thread=threading.Thread(target=sampling,daemon=True);thread.start()
        try: self.play(self.path(mode,seconds,**options))
        finally:
            stop.set();thread.join(timeout=17)
            if thread.is_alive(): raise RuntimeError('sampling request still pending; game left running')
            if self.recording:
                self.sim.record_stop();self.recording=False
            self.persist()
        if errors: raise RuntimeError(errors[0])
        if not self.check_masses(name): self.errors.append({'phase':name,'error':'raw solver/carry mass check failed'})
        report=self.cpp('analyze',{'phases':[phase],'samples':self.samples})
        print(json.dumps(report),flush=True)
        if not report['pass']: self.errors.append({'phase':name,'evidence':report})
    def persist(self):
        (self.out/'samples.json').write_text(json.dumps(self.samples),encoding='utf-8')
        (self.out/'phases.json').write_text(json.dumps(self.phases,indent=2),encoding='utf-8')
        (self.out/'acquisitions.json').write_text(json.dumps(self.acquisitions,indent=2),encoding='utf-8')
        (self.out/'mass-events.json').write_text(json.dumps(self.mass_events,indent=2),encoding='utf-8')
    def check_masses(self,label):
        values=self.call('CorpseAudit.masses()')
        report=self.cpp('mass-check',{'masses':values})
        self.mass_events.append({'label':label,'t':time.perf_counter(),'masses':values,'report':report})
        self.persist()
        print(json.dumps({'massCheck':label,'report':report}),flush=True)
        return report['pass']
    def face(self,wanted):
        rate=None
        for _ in range(12):
            before=self.sample()['player']['forward'];data={'wanted':wanted,'actual':before}
            if rate is not None: data['rate']=rate
            result=self.cpp('face',data)
            if result['done']: return
            self.sim.stick('right',result['stick'],0);time.sleep(result['seconds']);self.sim.stick('right',0,0);time.sleep(.3)
            data={'before':before,'after':self.sample()['player']['forward'],'seconds':result['seconds'],'stick':result['stick']}
            if rate is not None: data['previous']=rate
            rate=self.cpp('turn-rate',data)['rate']
        raise RuntimeError('player did not face target after measured stick feedback')
    def calibrate(self,side,plan,target,orientation=False):
        previous=None
        def settled():
            before=self.sample()['hands'][side]['frame']
            for _ in range(12):
                time.sleep(.12);after=self.sample()['hands'][side]['frame']
                if len(before)!=9 or len(after)!=9: raise RuntimeError('actual hand frame unavailable during calibration')
                if self.cpp('stability',{'before':before,'after':after,'orientation':orientation})['stable']: return after
                before=after
            raise RuntimeError('hand frame did not settle for calibration')
        for iteration in range(8):
            probe=self.cpp('orientation-probes' if orientation else 'position-probes',{'pose':self.hand_poses[side]})
            self.put(side,probe['poses'][0]);base=settled();frames=[base]
            for pose in probe['poses'][1:]:
                self.put(side,pose);frames.append(settled())
                self.put(side,probe['poses'][0]);repeated=settled()
                if not self.cpp('stability',{'before':base,'after':repeated,'orientation':orientation})['stable']:
                    raise RuntimeError('bracketed baseline changed; Jacobian would mix pose dynamics with control response')
            inp=dict(pose=probe['poses'][0],frames=frames,step=probe['step'])
            inp.update(normal=plan['normal'],along=plan['along']) if orientation else inp.update(target=target,rawPalm=True)
            result=self.cpp('orientation-solve' if orientation else 'position-solve',inp)
            print(json.dumps({'calibration':('orientation' if orientation else 'raw-palm-position'),'side':side,'iteration':iteration,'error':result['error']}),flush=True)
            if result['converged']: return
            if previous is not None and iteration>=3 and result['error']>=previous and not orientation:
                raise RuntimeError('raw palm calibration is not converging; stop instead of pushing controller further')
            previous=result['error'];self.play(self.path('to',.3,side=side,target=result['pose']))
        raise RuntimeError('hand calibration did not converge')
    def acquire(self,side,plan,expected_segment=None,relation=None,strict_segment=False):
        which='left' if side==0 else 'right';self.sim.grip(which,0)
        self.calibrate(side,plan,plan['safe'],True)
        self.calibrate(side,plan,plan['safe'])
        # The palm can become physically constrained before the geometric goal;
        # a confirmed nearby grasp candidate is the acquisition gate in that case.
        for attempt in range(8):
            if self.call('CorpseAudit.near('+str(side)+')'):
                state=self.sample(True)
                request={'palm':state['hands'][side]['frame'][8][:3],'slots':state['slots']}
                if 'mid' in plan: request['mid']=plan['mid']
                probes=self.cpp('mass-probes',request)
                # Lua table keys are fixed; all ray geometry comes from C++.
                literal='{points='+lua_literal(probes['points'])+',rays='+lua_literal(probes['rays'])+'}'
                masses=self.call('CorpseAudit.captureMasses('+literal+')')
                (self.out/('mass-baseline-'+str(self.seq)+'.json')).write_text(json.dumps(masses,indent=2),encoding='utf-8')
                self.diagnostic('pre-grip-'+str(len(self.acquisitions)))
                self.sim.grip(which,1)
                for _ in range(15):
                    time.sleep(.1);sample=self.sample();hand=sample['hands'][side]
                    if hand['native']['active'] and hand['grasp'].get('fast'):
                        if not sample['dead'] or hand.get('nativeKind')!=12: raise RuntimeError('grip acquired a non-ragdoll body')
                        if not hand.get('baselineMass'): raise RuntimeError('actual held body lacks an independent pre-hold mass baseline')
                        allowed=(expected_segment,)
                        if not strict_segment:
                            if expected_segment in (12,13): allowed=(12,13)
                            elif expected_segment in (8,9): allowed=(8,9)
                        actual_segment=hand['grasp']['fast'].get('segment')
                        if expected_segment is not None and actual_segment not in allowed:
                            raise RuntimeError('grip acquired a different anatomical segment')
                        if self.expected_proxy is not None and hand['native']['proxy']!=self.expected_proxy:
                            raise RuntimeError('grip acquired a different ragdoll')
                        self.expected_proxy=hand['native']['proxy']
                        if relation:
                            other=sample['hands'][1-side]['native'];same=other['proxy']==hand['native']['proxy'] and other['body']==hand['native']['body']
                            if (relation=='same')!=same: self.sim.grip(which,0);time.sleep(.5);return False
                        evidence={'side':side,'plannedSegment':expected_segment,'actualSegment':actual_segment,
                            'native':hand['native'],'grasp':hand['grasp'],'baselineMass':hand.get('baselineMass'),
                            'solverMass':hand.get('solverMass'),'reportedCarryMass':hand.get('reportedCarryMass'),'t':time.perf_counter()}
                        self.acquisitions.append(evidence);self.persist();print(json.dumps({'acquired':evidence}),flush=True)
                        return True
                self.sim.grip(which,0);time.sleep(.3)
            # Generated 2cm descent; all floating-point trajectory math is C++.
            self.play(self.path('move',.25,side=side,delta=[0,-.02,0]))
        raise RuntimeError('no actual native ragdoll hold after contact approach')
    def drop(self):
        self.sim.grip('left',0);self.sim.grip('right',0);self.sim.stick('left',0,0);self.sim.stick('right',0,0);time.sleep(.7)
        state=self.sample()
        if any(h['native']['active'] or h['grasp']['held'] for h in state['hands']): raise RuntimeError('drop did not release actual hold')
        if not self.check_masses('drop'): raise RuntimeError('uncached physical mass not restored after release')
    def position_for(self,part,pair=False):
        self.drop();self.expected_proxy=None
        initial=self.cpp('initial',{'player':self.sample()['player']})
        for side in (0,1): self.put(side,initial['hands'][side])
        _send('head_pose_command.json',self.cpp('head',{'height':self.args.grip_head_height}));time.sleep(.8)
        measurement=self.cpp('posture',{'diagnostic':self.diagnostic('stance-'+str(self.seq))})
        print(json.dumps({'posture':measurement}),flush=True)
        plan=self.cpp('limb',{'sample':self.sample(True),'part':part,'pairCentre':pair})
        candidates=self.cpp('stand-candidates',{'plan':plan,'floor':self.original['player']['position'][2],'measurement':measurement})
        rays=[[[ray['a'],ray['b']] for ray in candidate['rays']] for candidate in candidates['candidates']]
        candidates['hits']=self.call('CorpseAudit.standingGround('+lua_literal(rays)+')')
        stand=self.cpp('choose-stand',candidates)
        print(json.dumps({'stance':{k:v for k,v in stand.items() if k!='rays'}}),flush=True)
        self.exec('CorpseAudit.teleport('+lua_literal(stand['stand'])+','+lua_literal(stand['yaw'])+')')
        time.sleep(.8);self.face(stand['forward'])
        _send('head_pose_command.json',self.cpp('head',{'height':self.args.grip_head_height}));time.sleep(.8)
        initial=self.cpp('initial',{'player':self.sample()['player']})
        for side in (0,1): self.put(side,initial['hands'][side])
        time.sleep(.4)
        return self.cpp('limb',{'sample':self.sample(True),'part':part,'pairCentre':pair})
    def one_hand(self,part):
        plan=self.position_for(part);self.acquire(1,plan,plan['segment']);self.shot(part+'-held')
        self.phase(part+'-rest',seconds=2)
        self.phase(part+'-lift','move',seconds=2,side=1,delta=[0,.3,0],clip=True)
        self.phase(part+'-sideways','move',seconds=2,side=1,delta=[-.25,0,0])
        self.phase(part+'-wrist','twist',seconds=2,side=1,angleDegrees=45)
        self.phase(part+'-stand','stand',seconds=2,**{'from':self.args.grip_head_height,'to':1.7})
        self.phase(part+'-walk','walk',seconds=5,minimumTravel=.15,minimumCorpseTravel=.08,clip=True)
        self.phase(part+'-turn','turn',seconds=4)
        constrained=self.args.reach_mode=='floor'
        name=part+('-floor-block' if constrained else '-reach')
        self.phase(name+'-move','move',seconds=2 if constrained else 1,side=1,delta=[0,-1.8,0] if constrained else [.95,0,0])
        self.phase(name+'-hold',seconds=4.5,minimumReachSeconds=3.0,clip=constrained)
        self.shot(part+'-outside-reach');self.drop();self.phase(part+'-drop',seconds=1,hands=(),released=True)
        plan=self.position_for(part);self.acquire(1,plan,plan['segment']);self.phase(part+'-regrab',seconds=2);self.shot(part+'-regrab');self.drop()
    def paired(self,same):
        part=self.args.pair_part if same else 'forearm';plan=self.position_for(part,pair=not same);self.acquire(1,plan,plan['segment'],strict_segment=not same)
        if same:
            self.phase('same-part-setup-lift','move',seconds=2,hands=(1,),side=1,delta=[0,.30,0])
            accepted=False
            for offset in (.10,-.10,.06,-.06):
                sample=self.sample(True)
                plan=self.cpp('second-grip',{'sample':sample,'offset':offset})
                surface=self.call('CorpseAudit.heldSurface(1,'+lua_literal(plan['query'])+')')
                plan=self.cpp('second-grip',{'sample':sample,'offset':offset,'surface':surface,'query':plan['query']})
                if self.acquire(0,plan,relation='same'): accepted=True;break
            if not accepted: raise RuntimeError('two hands never acquired the same actual PhysX body')
        else:
            plan=self.cpp('limb',{'sample':self.sample(True),'part':'forearm','limbSide':'left'})
            if not self.acquire(0,plan,plan['segment'],relation='different',strict_segment=True): raise RuntimeError('different limbs resolved to one native body')
        name='same-part' if same else 'different-limbs';self.shot(name+'-held')
        self.phase(name+'-rest',seconds=2,hands=(0,1),relation='same' if same else 'different')
        if same: self.phase(name+'-rotate','pair-turn',seconds=3,hands=(0,1),relation='same',minimumRotation=10,angleDegrees=35,clip=True)
        else:
            self.phase(name+'-lift','pair-move',seconds=3,hands=(0,1),relation='different',delta=[0,.25,0],clip=True)
            self.phase(name+'-walk','walk',seconds=5,hands=(0,1),relation='different',minimumTravel=.15,minimumCorpseTravel=.08,clip=True)
        self.sim.grip('left',0);time.sleep(.5);self.phase(name+'-one-left',seconds=2,hands=(1,));self.shot(name+'-one-left');self.drop()
    def cleanup(self):
        errors=[];restoration_verified=False;unlock_requested=False;unlock_verified=False;root_retained=False
        if self.sim and self.mutated:
            try:
                self.sim.grip('left',0);self.sim.grip('right',0);self.sim.stick('left',0,0);self.sim.stick('right',0,0)
                if self.recording: self.sim.record_stop();self.recording=False
                time.sleep(.7)
            except Exception as e: errors.append('release input: '+str(e))
        if self.begun and self.mutated:
            try:
                if not self.check_masses('cleanup-before-delete'): errors.append('raw physical mass not restored before entity cleanup')
            except Exception as e: errors.append('raw mass restore verification: '+str(e))
        if self.sim and self.mutated:
            try:
                self.sim.restore()
                if hasattr(self,'original_head'): _send('head_pose_command.json',self.original_head)
            except Exception as e: errors.append('simulator restore: '+str(e))
        if self.begun and self.mutated:
            try:
                time.sleep(.4);self.exec('CorpseAudit.restore()');time.sleep(.8)
                self.face(self.original['player']['forward'])
                self.exec('CorpseAudit.restorePosition()');time.sleep(.5)
            except Exception as e: errors.append('game restore: '+str(e))
        if self.mutated:
            for path,data in self.settings.items():
                try:
                    if data is None: path.unlink(missing_ok=True)
                    elif not path.exists() or path.read_bytes()!=data: path.write_bytes(data)
                except Exception as e: errors.append('settings restore: '+str(e))
        if self.begun and self.lock_verified:
            try:
                time.sleep(.8);restored=self.call('CorpseAudit.restored()')
                restored=self.cpp('restore-check',restored)
                (self.out/'restored.json').write_text(json.dumps(restored,indent=2),encoding='utf-8')
                if not restored['pass']: errors.append('restored state verification failed')
                if not errors:
                    restoration_verified=True;unlock_requested=True
                    self.exec('CorpseAudit.releaseSaveLock(true)')
                    end=time.monotonic()+10
                    while time.monotonic()<end:
                        lock_state=self.call('CorpseAudit.saveLockState()')
                        (self.out/'save-lock-release.json').write_text(json.dumps(lock_state,indent=2),encoding='utf-8')
                        if lock_state['membershipKnown'] and not lock_state['owned']:
                            unlock_verified=True;self.exec('CorpseAudit=nil');break
                        if not lock_state['membershipKnown'] and lock_state['rootVerified'] and lock_state['protected']:
                            # This host exposes neither the private reason list nor
                            # dispatch ACKs. REMOVE was queued only for our token;
                            # root's causally-proven lock remains until ExitGame.
                            root_retained=True;self.exec('CorpseAudit=nil');break
                        time.sleep(.1)
                    else: errors.append('own save-lock removal not confirmed; retained probe for repair')
            except Exception as e: errors.append('restore verification: '+str(e))
        elif self.loaded and not self.lock_requested:
            try: self.exec('CorpseAudit=nil')
            except Exception as e: errors.append('unused probe cleanup: '+str(e))
        if self.sim:
            close=ctypes.WinDLL('kernel32',use_last_error=True).CloseHandle;close.argtypes=[wintypes.HANDLE];close(self.sim.h)
        (self.out/'cleanup.json').write_text(json.dumps({'errors':errors,'gameStopped':False,'saveLockReason':self.lock_reason,
            'mutationsPerformed':self.mutated,'lockVerified':self.lock_verified,'restorationVerified':restoration_verified,
            'unlockRequested':unlock_requested,'unlockVerified':unlock_verified,
            'rootProtectionRetained':root_retained,
            'saveLockRetainedOrUnconfirmed':self.lock_requested and not unlock_verified},indent=2),encoding='utf-8')
        return errors
    def run(self):
        try:
            self.begin()
            cases=self.args.case or ['shin','forearm','same-part','different-limbs']
            for case in cases:
                print('CASE '+case,flush=True)
                if case in ('shin','forearm'): self.one_hand(case)
                else: self.paired(case=='same-part')
        except Exception as error:
            self.errors.append({'error':str(error),'traceback':traceback.format_exc()});print(traceback.format_exc(),flush=True)
            if self.begun and self.mutated:
                try:
                    diag=self.plugin/'log/vrik_diag.txt';start=diag.stat().st_size if diag.exists() else 0
                    self.exec('LogVRDiag(0,0,0,0,0,0,1)')
                    with diag.open('rb') as source: source.seek(start);text=source.read().decode('utf-8','replace')
                    (self.out/'failure-vrik-diag.txt').write_text(text,encoding='utf-8')
                    (self.out/'failure-state.json').write_text(json.dumps(self.sample(True),indent=2),encoding='utf-8')
                    self.shot('failure-pose')
                except Exception as capture_error: self.errors.append({'failureCapture':str(capture_error)})
        finally:
            self.persist();self.errors.extend(self.cleanup())
            try:
                if self.phases:
                    report=self.cpp('analyze',{'samples':self.samples,'phases':self.phases});report['harnessErrors']=self.errors
                    report['pass']=report['pass'] and not self.errors
                else: report={'pass':False,'harnessErrors':self.errors,'reason':'no verified phases'}
            except Exception as error:
                report={'pass':False,'harnessErrors':self.errors,'analysisError':str(error)}
            (self.out/'report.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(json.dumps({'pass':report['pass'],'report':str(self.out/'report.json'),'gameLeftRunning':True}),flush=True)
        return report['pass']

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid',required=True,type=int)
    parser.add_argument('--out',required=True,type=Path)
    parser.add_argument('--game',type=Path,default=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'))
    parser.add_argument('--generator',type=Path,default=ROOT/'build/corpse-live/Release/corpse_probe.exe')
    parser.add_argument('--record',default='Character.LowlifeMale')
    parser.add_argument('--spawn-distance',type=float,default=1.1,help='signed distance from initial player along forward; C++ generates spawn position')
    parser.add_argument('--grip-head-height',type=float,default=.85,help='HMD height for leaning down to the actual floor ragdoll')
    parser.add_argument('--reach-mode',choices=['lateral','floor'],default='lateral',help='measured >45cm separation regression; floor is an explicit synthetic contact stress, not natural posture')
    parser.add_argument('--trace',action='store_true')
    parser.add_argument('--case',action='append',choices=['shin','forearm','same-part','different-limbs'])
    parser.add_argument('--pair-part',choices=['trunk','shin'],default='trunk',help='explicit same-body handover fixture part')
    args=parser.parse_args()
    if not re.fullmatch(r'Character\.[A-Za-z0-9_]+',args.record): parser.error('invalid test NPC record')
    return 0 if Probe(args).run() else 1
if __name__=='__main__': sys.exit(main())
