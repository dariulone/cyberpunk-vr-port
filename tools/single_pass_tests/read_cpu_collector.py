"""Verify a private native CPU collector's initialization and destruction."""
import argparse
import ctypes as c
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT/'tools/swimming_tests'), str(ROOT/'tools/roomscale_tests')]


def decode(data,query_data=None,fine_data=None,scene_data=None,scene_control=None):
    r = struct.unpack('<8I', data)
    if r[0]!=r[1] or not r[2] or r[2]!=r[3] or r[4:7]!=(1,1,1) or r[7]:
        raise ValueError('Native collector clone, destructor or input preservation check failed')
    result=dict(collectorLimit=r[0], metadataBits=r[2], ownedPointers=True, destructorCompleted=True,
                inputsUnchangedAtLaunch=True, queryJobsStarted=query_data is not None, meshListsPrepared=False)
    if query_data is not None:
        q=struct.unpack('<8I',query_data)
        if q[0]!=1 or q[1]!=1 or q[2] or q[3]!=1 or not q[4] or q[4]!=q[5] or q[6:]!=(1,1):
            raise ValueError('Private query completion, independence or frustum equality failed')
        result['query']=dict(differentBytes=q[2], independent=True, nativeTypeRva=hex(q[4]), groupsComplete=True, ownerReleased=True)
    if fine_data is not None:
        f=struct.unpack('<12I',fine_data)
        if query_data is None or not f[0] or not f[1] or f[2]!=f[3] or any(f[4:7]) or f[7]>4*1024*1024 or f[8]!=f[9] or f[10]!=f[11]:
            raise ValueError('Private fine visibility differs from native appended mesh IDs or capture was incomplete')
        result['fineVisibility']=dict(ranges=f[0],candidates=f[1],nativeIds=f[2],privateIds=f[3],
            differentRanges=f[4],bytesOwned=f[7],occlusionActive=bool(f[8] and f[10]),
            note='Captured fine-test ranges only; not a complete independently prepared scene list')
    if scene_data is not None:
        s=struct.unpack('<12I',scene_data)
        c=struct.unpack('<8I',scene_control) if scene_control is not None else None
        if fine_data is None or c is None or s[0]!=1 or s[1] or not s[2] or s[2]!=s[3] or not s[4] or s[5]!=c[0] or s[6]!=s[7] or s[8] or s[9]!=1 or s[11]!=1:
            raise ValueError('Independent scene-pool traversal failed or differs from complete native fine output')
        if c[0]+c[1]!=f[0] or s[7]+c[2]!=f[2] or c[3]!=c[0] or c[4]!=s[7] or c[5] or c[6] or c[7]!=1:
            raise ValueError('Scene-pool provenance or original-query replay does not match native results')
        result['scenePool']=dict(buckets=s[3],candidates=s[4],visibleBuckets=s[5],privateIds=s[6],
            nativeIds=s[7],differentIds=s[8],bytesOwned=s[10],nativeJobsComplete=True,
            outsidePoolRanges=c[1],outsidePoolIds=c[2],sourceQueryReplayMatches=True,
            note='Complete candidate pool, not other scene-system gather callbacks or material/draw preparation')
    return result


def decode_peer(base,result,query,fine,scene,control):
    r=struct.unpack('<16I',result)
    if any(r[i]!=1 for i in (0,1,2,3,4,5,10,11,12)) or not r[6] or not r[7] or r[8] or r[9] or r[13]:
        raise ValueError('Peer visibility was not ready early, differs from native MAIN, or lacks present-interval/lifetime proof')
    if r[14]>min(r[6],r[7]) or 2*r[14]+r[15]!=r[6]+r[7]:
        raise ValueError('Invalid two-eye overlap counts')
    main=decode(base,query,fine,scene,control)
    if r[7]!=main['scenePool']['nativeIds']:
        raise ValueError('Early MAIN visibility does not cover the complete native pool output')
    return dict(readyAtVrcamCull=True,readyBeforeMainQuery=True,samePresentInterval=True,sameCandidatePool=True,
        earlyVrcamIds=r[6],earlyMainIds=r[7],bothEyeOccurrences=r[14],singleEyeOccurrences=r[15],
        main=main,note='Early visibility only; native material preparation and rendering are not replaced')


def main():
    from live_read import Reader
    from live_symbols import resolve
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--query', action='store_true', help='Also run native asynchronous CPU query/occlusion preparation')
    p.add_argument('--fine', action='store_true', help='Also compare native appended IDs on owned fine-culling ranges')
    p.add_argument('--scene', action='store_true', help='Also traverse the full owned native scene candidate pool')
    p.add_argument('--peer', action='store_true', help='Prepare MAIN visibility early and compare both eyes against later native results')
    a=p.parse_args()
    a.scene=a.scene or a.peer
    a.fine=a.fine or a.scene
    a.query=a.query or a.fine
    game=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols=resolve(a.pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_CpuCollector*','CyberpunkVR_BoundsTrace*','CyberpunkVR_ParticleTrace*','g_menuModeValue'])
    reader=Reader(a.pid,game/'bin/x64/Cyberpunk2077.exe',0,0)
    k=reader.k
    k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
    k.WaitForSingleObject.argtypes=[c.c_void_p,c.c_uint32]
    address=lambda name:int(symbols['CyberpunkVR_CpuCollector'+name],16)
    read=lambda name:struct.unpack('<I',reader.read(address(name),4))[0]
    handle=None;armed=False
    report=dict(pid=a.pid,dll=symbols['sha256'],passed=False)
    def write(name,value):
        data,done=c.c_uint32(value),c.c_size_t()
        if not k.WriteProcessMemory(handle,address(name),c.byref(data),4,c.byref(done)) or done.value!=4:
            raise c.WinError(c.get_last_error())
    try:
        if read('Compatible')!=1 or read('Request') or read('State') in (1,4) or struct.unpack('<I',reader.read(int(symbols['g_menuModeValue'],16),4))[0]:
            raise RuntimeError('Native collector probe unavailable, busy or not in gameplay')
        handle=k.OpenProcess(0x101028,False,a.pid)
        if not handle:
            raise c.WinError(c.get_last_error())
        if a.query and read('QueryCompatible')!=1:
            raise RuntimeError('Native CPU query hook unavailable')
        if a.fine and read('FineCompatible')!=1:
            raise RuntimeError('Native fine-culling layout unavailable')
        if a.scene and read('SceneCompatible')!=1:
            raise RuntimeError('Native scene-pool layout unavailable')
        if a.peer and read('PeerCompatible')!=1:
            raise RuntimeError('Native peer-camera/query layout unavailable')
        armed=True;write('State',0);write('Request',5 if a.peer else 4 if a.scene else 3 if a.fine else 2 if a.query else 1);deadline=time.monotonic()+8
        while read('Request') or read('State') not in (2,3):
            if time.monotonic()>deadline:
                raise TimeoutError('Private native collector lifecycle did not complete')
            time.sleep(.01)
        report['state']=read('State')
        data=reader.read(address('Result'),32)
        report['raw']=list(struct.unpack('<8I',data))
        query_data=reader.read(address('QueryResult'),32) if a.query else None
        if query_data is not None:
            report['queryRaw']=list(struct.unpack('<8I',query_data))
        fine_data=reader.read(address('FineResult'),48) if a.fine else None
        if fine_data is not None:
            report['fineRaw']=list(struct.unpack('<12I',fine_data))
        scene_data=reader.read(address('SceneResult'),48) if a.scene else None
        if scene_data is not None:
            report['sceneRaw']=list(struct.unpack('<12I',scene_data))
        scene_control=reader.read(address('SceneControl'),32) if a.scene else None
        if scene_control is not None:
            report['sceneControl']=list(struct.unpack('<8I',scene_control))
        peer_data=[]
        if a.peer:
            for name,count in (('PeerResult',16),('PeerQuery',8),('PeerFine',12),('PeerScene',12),('PeerControl',8)):
                raw=reader.read(address(name),count*4);peer_data.append(raw)
                report[name]=list(struct.unpack(f'<{count}I',raw))
            report['PeerPlanes']=list(struct.unpack('<64f',reader.read(address('PeerPlanes'),256)))
            report['PeerInputDiff']=list(struct.unpack('<10I',reader.read(address('PeerInputDiff'),40)))
            count=read('PeerChangesCount')
            if count>64:
                raise ValueError('Invalid changed-candidate record count')
            changes=reader.read(address('PeerChanges'),count*64) if count else b''
            report['PeerChanges']=[dict(id=hex(r[0]),typeRva=hex(r[1]),prepareRva=hex(r[2]),before=r[3:9],after=r[9:15])
                                   for r in struct.iter_unpack('<QII12f',changes)]
            from bounds_trace import RECORD,decode as decode_bounds
            trace_read=lambda name:struct.unpack('<I',reader.read(int(symbols['CyberpunkVR_BoundsTrace'+name],16),4))[0]
            count=trace_read('Count')
            if count>4096:
                raise ValueError('Invalid bounds trace count')
            raw=reader.read(int(symbols['CyberpunkVR_BoundsTraceRecords'],16),count*RECORD.size) if count else b''
            try:
                if trace_read('Format')!=2:
                    raise ValueError('Unsupported bounds trace format')
                trace=decode_bounds(raw,trace_read('Lost'),[r['id'] for r in report['PeerChanges']])
                path=a.out.with_suffix('.bounds.json');path.write_text(json.dumps(trace,indent=2)+'\n')
                report['boundsTrace']=dict(trace['summary'],path=str(path))
            except ValueError as error:
                report['boundsTrace']=dict(error=str(error),count=count,lost=trace_read('Lost'))
            from particle_trace import RECORD as PARTICLE_RECORD,decode as decode_particles
            particle_read=lambda name:struct.unpack('<I',reader.read(int(symbols['CyberpunkVR_ParticleTrace'+name],16),4))[0]
            count=particle_read('Count')
            if count>4096:raise ValueError('Invalid particle trace count')
            raw=reader.read(int(symbols['CyberpunkVR_ParticleTraceRecords'],16),count*PARTICLE_RECORD.size) if count else b''
            try:
                trace=decode_particles(raw,particle_read('Lost'))
                path=a.out.with_suffix('.particles.json');path.write_text(json.dumps(trace,indent=2)+'\n')
                report['particleTrace']=dict(trace['summary'],path=str(path))
            except ValueError as error:
                report['particleTrace']=dict(error=str(error),count=count,lost=particle_read('Lost'))
        if report['state']!=2:
            raise RuntimeError('Private native collector failed')
        report.update(decode(data,query_data,fine_data,scene_data,scene_control))
        if a.peer:
            report['earlyPeer']=decode_peer(data,*peer_data)
            if report['earlyPeer']['earlyVrcamIds']!=report['scenePool']['nativeIds']:
                raise ValueError('Early VRCAM visibility does not cover its complete native pool output')
        report['passed']=True
    except Exception as error:
        report['error']=str(error)
    finally:
        try:
            if armed:
                errors=[]
                for name in ('Request','State'):
                    try:
                        write(name,0)
                    except OSError as error:
                        if k.WaitForSingleObject(handle,0)!=0:
                            errors.append(str(error))
                if errors:
                    report['passed']=False;report['cleanupError']=errors
        finally:
            if handle:k.CloseHandle(handle)
            reader.close();a.out.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    return 0 if report['passed'] else 1


if __name__=='__main__':
    raise SystemExit(main())
