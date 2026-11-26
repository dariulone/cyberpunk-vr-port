"""Play timed OpenXR controller paths against the installed game's physical melee."""
import argparse
from contextlib import contextmanager
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
import uuid

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path[:0] = [str(ROOT/'tools/sim_harness'),str(ROOT/'tools/quickboot_tests')]
from simxr import Sim
from launch_to_save import DEFAULT_GAME, focus_window, process_ids, process_path, windows_for


class Fixture:
    def __init__(self, path):
        self.path = path

    def call(self, action, **fields):
        key = uuid.uuid4().hex
        stage = self.path / f"{key}.tmp"
        stage.write_text(json.dumps(dict(action=action,id=key,**fields)),encoding="utf-8")
        try:
            for attempt in range(80):
                try:
                    stage.replace(self.path/"request.json")
                    break
                except PermissionError:
                    if attempt==79: raise
                    time.sleep(.01)
        finally:
            stage.unlink(missing_ok=True)
        end = time.monotonic()+12
        while time.monotonic()<end:
            try:
                reply = json.loads((self.path/"response.json").read_text(encoding="utf-8"))
                if reply['id']==key:
                    if not reply['ok']: raise RuntimeError(f"{action}: {reply.get('error')}")
                    return reply.get('result')
            except (FileNotFoundError,PermissionError,json.JSONDecodeError):
                pass
            time.sleep(.012)
        raise TimeoutError(f"No fixture reply for {action}")


@contextmanager
def session(args, folder):
    if args.pid:
        if process_path(args.pid)!=args.game/"bin/x64/Cyberpunk2077.exe":
            raise RuntimeError("The specified PID is not the selected game")
        yield args.pid
        return
    if not args.manifest or not args.manifest.is_file():
        raise ValueError("--manifest must name the simulator runtime JSON")
    if process_ids("Cyberpunk2077.exe") or folder.exists():
        raise RuntimeError("An existing game or test module is present")
    pid = None
    folder.mkdir()
    try:
        shutil.copyfile(HERE/"fixture.lua", folder/"init.lua")
        shutil.copyfile(HERE/"slide_feedback_probe.lua", folder/"slide_feedback_probe.lua")
        command = [sys.executable, "-B", str(ROOT/"tools/quickboot_tests/launch_to_save.py"),
                   "--game-root", str(args.game), "--save", args.save]
        env = {**os.environ,"XR_RUNTIME_JSON":str(args.manifest.resolve())}
        with subprocess.Popen(command, env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,
                              text=True,encoding="utf-8",errors="replace") as child:
            for line in child.stdout:
                print(line,end="",flush=True)
                if line.startswith("Launched PID "): pid=int(line.split()[2])
            if child.wait() or not pid: raise RuntimeError("QuickBoot did not enter gameplay")
        yield pid
    finally:
        if pid and process_path(pid)==args.game/"bin/x64/Cyberpunk2077.exe":
            subprocess.run(["taskkill","/F","/PID",str(pid),"/FI","IMAGENAME eq Cyberpunk2077.exe"],check=True)
            wait_for(lambda:pid not in process_ids("Cyberpunk2077.exe"),"Test process has not exited")
        for filename in ("init.lua","slide_feedback_probe.lua","request.json","response.json"): (folder/filename).unlink(missing_ok=True)
        # Keep the host's per-module logs when present.
        for file in folder.iterdir():
            if file.suffix=='.log': shutil.move(str(file),str(args.out/file.name))
        folder.rmdir()


def wait_for(check, message):
    end=time.monotonic()+6
    while time.monotonic()<end:
        if check(): return
        time.sleep(.06)
    raise RuntimeError(message)


def play(sim, side, frames, reverse=False):
    # Sampling uses absolute deadlines; no synchronous reads from the game
    # during a gesture. Keep actual send times alongside the requested path.
    start=time.perf_counter()
    sent=[]
    for i, frame in enumerate(frames):
        when=frame[0]
        pose=frames[-1-i] if reverse else frame
        remaining=start+when-time.perf_counter()
        if remaining>0: time.sleep(remaining)
        sim.hand(side,pos=pose[1:4],quat=pose[4:8],mode='body')
        sent.append(time.perf_counter()-start)
    return sent


def assess(case, before, frames, blade, air=False):
    if not frames: return {"tracked":False}
    events=([r for r in frames if r['serial']>before['serial'] and r['same']] if blade
            else [r for r in frames if r['hits']>before['hits'] and r.get('hp',before.get('hp'))!=before.get('hp')])
    checks={"tracked":len({r.get('epoch') for r in frames})>=4 and all(not r.get('error') for r in frames)}
    if air:
        checks["no_air_hit"]=all(r['serial']==before['serial'] and r['hits']==before['hits'] for r in frames)
        if case in ('swing','reverse'):
            checks['one_air_swing']=frames[-1].get('starts',0)-before.get('starts',0)==1
            checks['one_air_whoosh']=frames[-1]['whooshes']-before['whooshes']==1
        return checks
    checks.update(start_clear=before['shapes']>0 and before['shapeDepth']==0,
                  shape_crossing=any(r['shapeDepth']>0 for r in frames),target_hit=bool(events),
                  damage=any(r.get('hp',before['hp'])<before['hp'] for r in events))
    if blade:
        expected_kind=2 if case=='thrust' else 1
        checks['mesh_mark']=any(r.get("mark") and r.get('inkKind')==expected_kind and r.get('inkTriangles',0)>0 for r in events)
    if case=="thrust" and blade:
        # Require observations of the native constraint during the stationary
        # hold after the controller has finished its forward travel.
        bound=[r for r in events if r.get('joint') and r.get('puncture') and r['depth']>0]
        checks['lodged_hold']=bool(bound and bound[-1]['at']-bound[0]['at']>=.30 and frames[-1].get('joint'))
    elif blade:
        checks['slash']=any(not r.get('puncture') for r in events)
    # An attack may apply a damage-over-time status. Count impact receipts,
    # not HP deltas, to detect repeated static hits during the settled hold.
    still=[r for r in frames if r['at']>=frames[-1]['at']-.22]
    checks['no_static_repeat']=bool(events and len(still)>=3 and all(r['hits']==still[0]['hits'] for r in still))
    return checks


def assess_return(held, rows):
    old=held['serial']
    clear=next((i for i,r in enumerate(rows) if r['same'] and r['serial']==old and r['released']
                and not r['entryBlocked'] and not r.get('joint') and r['shapes']==held['shapes']>0
                and r['shapeDepth']==0),None)
    after=rows[clear+1:] if clear is not None else []
    return dict(return_from_joint=bool(held['same'] and held.get('joint') and held['puncture']),
                released_before_return=clear is not None,
                eligible_on_release=clear is not None and rows[clear]['eligible'],
                return_overlap=any(r['shapeDepth']>0 for r in after),
                independent_puncture=any(r['same'] and r['serial']==old+1 and r['hits']==held['hits']+1
                    and r['puncture'] and not r['follow'] and r.get('joint') and r['jointSerial']!=held['jointSerial']
                    and r['shapeDepth']>0 and r['hp']<rows[clear]['hp'] for r in after),
                single_return_hit=bool(after and after[-1]['hits']==held['hits']+1 and after[-1]['serial']==old+1))


def trial(args,fixture,sim,kind):
    wait_for(lambda:fixture.call('restoreStart'),"Player did not return to the trial origin")
    angles=[str(args.yaw),str(args.roll)]
    rotation=list(map(float,subprocess.check_output([str(args.generator),'neutral',*angles],text=True).split()))
    side=fixture.call('hand')
    sim.hand(side,pos=(.15 if side=='right' else -.15,-.50,-.24),quat=rotation,mode='body')
    time.sleep(.8)
    cal=fixture.call("calibrate")
    blade=cal['kind']>0
    press=kind in ('edge-press','flat-press')
    quick=kind=='quick-reentry'
    stealth=kind in ('stealth-back','front-thrust')
    animate=kind in ('animated-hold','moving-release')
    if (press or quick or stealth or animate) and not blade: raise ValueError("This trial requires a bladed item")
    path_kind='thrust' if press or quick or stealth or animate else kind
    def path(motion):
        cmd=[str(args.generator),motion,*map(str,cal['blade']+cal['edge']),*angles]
        frames=[list(map(float,line.split())) for line in subprocess.check_output(cmd,text=True).splitlines()]
        if side=='left':
            for frame in frames:frame[1]-=.30
        return frames
    frames=path(path_kind)
    return_path=path('quick-return') if quick else None
    sim.hand(side,pos=frames[0][1:4],quat=frames[0][4:8],mode='body')
    time.sleep(.75)
    report={'case':kind,'calibration':cal,'path':frames,'targetSpeed':args.target_speed,
            'yawDegrees':args.yaw,'rollDegrees':args.roll,'releaseAIOnStroke':args.release_ai,
            'character':args.target_record}
    recording=False
    try:
        before=fixture.call('begin')
        report['airSent']=play(sim,side,frames)
        if quick: report['airReturnSent']=play(sim,side,return_path)
        time.sleep(.35)
        air=fixture.call('finish')
        report.update(air=air,airChecks=assess(kind,before,air,blade,True))
        sim.hand(side,pos=frames[0][1:4],quat=frames[0][4:8],mode='body')
        time.sleep(.7)
        length=cal['reach']+(.15 if path_kind=='thrust' else -.16)
        point=[cal['world'][i]+length*cal['direction'][i] for i in range(2)]+[cal['floor']+.02]
        facing={'stealth-back':'away','front-thrust':'toward','animated-hold':'toward','moving-release':'toward'}.get(kind,args.facing)
        fixture.call('spawn',point=point,record=args.target_record,facing=facing)
        wait_for(lambda:fixture.call('ready'),"Target did not attach")
        fixture.call('prepare',targetSpeed=args.target_speed)
        wait_for(lambda:fixture.call('shapes'),"No resolved native hit capsules on fixture target")
        time.sleep(.6)
        idle_before=fixture.call('begin')
        time.sleep(.25)
        idle_rows=fixture.call('finish')
        if args.video:
            video=args.out/(kind+'.mp4')
            if sim.status().get('recording'):raise RuntimeError('Simulator is already recording')
            try:
                sim.record_start(str(video),eyes='left',eye_size=1080,fps=60,codec='h264',mbps=16,max_seconds=20)
            except TimeoutError:
                status=sim.status()
                if not status.get('recording') or Path(status['video'])!=video:raise
            recording=True
        before=fixture.call('begin',releaseAI=args.release_ai)
        report.update(before=before,idle=idle_rows)
        report['sent']=play(sim,side,frames)
        time.sleep(.75)
        rows=fixture.call('finish')
        report.update(contact=rows,checks=assess(path_kind,before,rows,blade))
        if stealth:
            hit=next((r for r in rows if r.get('same') and r['serial']>before['serial']),{})
            checks=report['checks']
            # Stealth trials can defeat the target on the first hit.
            checks.pop('lodged_hold',None)
            checks['initial_facing']=before['rearPosition']==(kind=='stealth-back')
            checks['direction']=bool(hit) and hit['rear']==(kind=='stealth-back')
            checks['stealth_rule']=bool(hit) and hit['stealth']==(kind=='stealth-back')
            if kind=='stealth-back':
                checks['undetected_at_start']=before['detection']==0
                checks['body_zone']=hit.get('hitShape') not in (None,'Head','Neck') and not hit.get('portMatched')
                budget=before['hp']*(.80 if before['elite'] else 1.00)
                checks['health_budget']=bool(hit) and abs(hit.get('physicalAfter',0)-budget)<1
                checks['health_loss']=bool(hit) and abs((before['hp']-hit['hp'])-hit['physicalAfter'])<1
        report['checks']['idle']=bool(idle_rows and all(r['serial']==idle_before['serial']
            and r['hp']==idle_before['hp'] for r in idle_rows))
        if press:
            continued=path(kind)
            locked=fixture.call('begin')
            report['pressSent']=play(sim,side,continued)
            time.sleep(.55)
            after=fixture.call('finish')
            report.update(pressBefore=locked,press=after,pressPath=continued)
            continuation=[r for r in after if r['serial']>locked['serial'] and r['follow'] and r['same']]
            report['checks']['press_from_joint']=bool(locked.get('joint'))
            if kind=='edge-press':
                report['checks']['edge_only']=any(r.get('jointSerial')==locked.get('jointSerial')
                    and r['hp']<locked['hp'] for r in continuation)
                report['checks']['follow_mark']=any(r.get('mark') and r.get('inkKind') in (1,3)
                    and r.get('inkTriangles',0)>0 for r in continuation)
            else:
                report['checks']['edge_only']=not continuation
        if args.screenshots:
            try:
                report['screenshots']=sim.shot(args.out/(kind+"-hold"),eyes='both',eye_size=960,timeout=4)
            except TimeoutError:
                files=[args.out/f"{kind}-hold_{eye}.jpg" for eye in ("L","R")]
                if not all(p.is_file() and p.stat().st_size for p in files):raise
                report['screenshots']=[str(p) for p in files]
        if quick:
            held=fixture.call('begin')
            report['returnSent']=play(sim,side,return_path)
            time.sleep(.60)
            after=fixture.call('finish')
            report.update(returnBefore=held,returnPath=return_path,returnFrames=after)
            report['checks'].update(assess_return(held,after))
        if animate:
            held=fixture.call('sample')
            fixture.call('begin',animate=kind=='animated-hold',moveTarget=kind=='moving-release')
            time.sleep(1.3)
            after=fixture.call('finish')
            report.update(animationBefore=held,animation=after)
            if held.get('joint') and held.get('animatedAnchor') and held.get('wrist'):
                retained=[r for r in after if r.get('joint') and r.get('wrist')]
                travel=max((math.dist(r['animatedAnchor'],held['animatedAnchor']) for r in after if r.get('animatedAnchor')),default=0)
                wrist=max((math.dist(r['wrist'],held['wrist']) for r in retained),default=0)
                restGap=math.dist(held['wrist'],held['rawWorld'])
                extra=max((math.dist(r['wrist'],r['rawWorld'])-restGap for r in after if r.get('wrist') and r.get('rawWorld')),default=0)
                report['movementMeters']={'anchor':travel,'retainedWrist':wrist,'extraControllerGap':extra}
                report['checks']['animation_moved']=travel>.015
                report['checks']['stationary_controller']=all(math.dist(r['hand'],held['hand'])<.001 for r in after)
                # The live world-space samples include the controller's sub-millimetre tracking tolerance.
                report['checks']['wrist_bounded']=wrist<=.121 and extra<=.121
                report['checks']['same_contact']=all(r['serial']==held['serial'] for r in after)
                if kind=='moving-release':
                    report['checks']['escaped']=travel>.48 and after[-1]['released'] and not after[-1].get('joint')
            else: report['checks']['animation_from_hold']=False
        if kind=='thrust' and blade:
            fixture.call('begin')
            play(sim,side,frames,reverse=True)
            time.sleep(.55)
            withdraw=fixture.call('finish')
            extension=None
            if withdraw and (withdraw[-1].get('joint') or withdraw[-1]['shapeDepth']>0):
                extension=path('withdraw')
                fixture.call('begin')
                report['withdrawExtensionSent']=play(sim,side,extension)
                time.sleep(.3)
                extra=fixture.call('finish')
                # Each observation starts a new relative clock.
                report['withdrawExtension']=extra
            last=(report.get('withdrawExtension') or withdraw)[-1]
            report['withdraw']=withdraw
            report['checks']['released']=bool(not last.get('joint') and last['shapes']>0 and last['shapeDepth']==0)
            start=fixture.call('begin')
            if extension: play(sim,side,extension,reverse=True)
            play(sim,side,frames)
            time.sleep(.60)
            again=fixture.call('finish')
            report['reentry']=again
            report['checks']['reentry']=bool(report['checks']['released'] and any(r['same']
                and r['serial']>start['serial'] and r.get('joint') for r in again))
        print(kind,report['airChecks'],report['checks'],flush=True)
        return all(report['airChecks'].values()) and all(report['checks'].values())
    finally:
        try:
            if recording:
                status=sim.status()
                if status.get('recording'):status=sim.record_stop()
                if Path(status.get('video',''))!=video or not video.stat().st_size:
                    raise RuntimeError('Missing simulator recording')
                report['video']=status
        finally:
            (args.out/(kind+".json")).write_text(json.dumps(report,indent=2),encoding='utf-8')
            fixture.call('remove')
            wait_for(lambda:not fixture.call('ready'),"Trial target did not detach")


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--game',type=Path,default=DEFAULT_GAME)
    p.add_argument('--manifest',type=Path)
    p.add_argument('--pid',type=int,help="Attach to an existing disposable simulator session with the fixture installed")
    p.add_argument('--save',default="AutoSave-7")
    p.add_argument('--item',default="Items.Preset_Katana_Takemura")
    p.add_argument('--target-record',default="Character.maelstrom_grunt1_melee1_knife_ma",
                   help="Character record for the disposable target")
    p.add_argument('--generator',type=Path,default=ROOT/'build/physical-melee/trajectory/Release/melee_trajectory.exe')
    p.add_argument('--out',type=Path,required=True)
    p.add_argument('--slide-controls',action='store_true',help="Run relative-motion haptic controls in the game script VM")
    p.add_argument('--screenshots',action='store_true',help="Capture both eyes after each contact gesture")
    p.add_argument('--video',action='store_true',help="Record the left eye at 1080p and 60 fps during each contact trial")
    p.add_argument('--target-speed',type=float,default=.005,help="NPC time scale. Use 1 for ordinary AI and animation")
    p.add_argument('--facing',choices=('away','toward'),help="Set the spawned NPC's initial facing relative to the player")
    p.add_argument('--release-ai',action='store_true',help="Restore ordinary AI speed as the contact stroke starts")
    p.add_argument('--yaw',type=float,default=0.,help="Turn the gesture about the tracking-space vertical axis, in degrees")
    p.add_argument('--roll',type=float,default=0.,help="Rotate the cutting plane about the tracking-space forward axis, in degrees")
    p.add_argument('--cases',nargs='+',choices=('swing','reverse','arc','thrust','edge-press','flat-press','quick-reentry','stealth-back','front-thrust','animated-hold','moving-release'),
                   default=['swing','reverse','arc','thrust'])
    a=p.parse_args();a.game=a.game.resolve();a.out=a.out.resolve()
    if not 0<a.target_speed<=1: p.error('--target-speed must be greater than zero and at most one')
    if not a.generator.is_file():p.error('Build melee_trajectory first')
    a.out.mkdir(parents=True,exist_ok=False)
    folder=a.game/'red4ext/plugins/cyberpunkvrport/lua/CPVR_PhysicalMeleeTest'
    with session(a,folder) as pid:
        sim=Sim(pid,focus_away=False)
        preview=next(w for w in windows_for(pid) if w['visible'] and w['title'].startswith('OpenXR Simulator'))
        focus_window(preview['hwnd'])
        sim.hands_follow_head(True);sim.head(0,-7)
        f=Fixture(folder)
        if a.slide_controls:
            (a.out/'slide-controls.json').write_text(json.dumps(f.call('slideControls'),indent=2))
        f.call('lock',tag='cpvr_melee_'+uuid.uuid4().hex)
        wait_for(lambda:f.call('locked'),"Save lock unavailable")
        f.call('equip',item=a.item);time.sleep(1);f.call('draw');time.sleep(1.5)
        summary={kind:trial(a,f,sim,kind) for kind in a.cases}
        (a.out/'summary.json').write_text(json.dumps(dict(pid=pid,item=a.item,passed=summary),indent=2))
        return 0 if all(summary.values()) else 1


if __name__=="__main__": sys.exit(main())
