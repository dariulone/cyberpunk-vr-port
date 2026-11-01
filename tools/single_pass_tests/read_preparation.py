"""Capture owned preparation lists and diagnose paired-request eligibility."""
import argparse
from collections import Counter, defaultdict
import ctypes as c
import hashlib
import json
from pathlib import Path
import struct
import sys
import time
from graph_plan import decode_graph
from paired_camera import decode_cameras,compare_cameras
from paired_head_turn import HeadTurn
from early_depth_report import FIELDS as EARLY_FIELDS,MASK_FIELDS,NATIVE_FIELDS,OPTIMIZED_FIELDS,validate as validate_early,decode_gpu_times,validate_native_timing
from early_depth_report import draw_limit,MAX_SOURCES,MAX_INSTANCE_ROWS
from read_constants import decode_depth_comparison
from retained_geometry_report import MODES as RETAINED_MODES,decode as decode_retained,validate as validate_retained,validate_options as validate_retained_options
from paired_geometry_report import decode as decode_paired_geometry,validate as validate_paired_geometry,validate_options as validate_paired_geometry_options

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT / 'tools/swimming_tests'), str(ROOT / 'tools/roomscale_tests')]
KINDS = {1: 'stage_begin', 2: 'gather', 3: 'filter', 4: 'finalize', 5: 'stage_end',
         6: 'consume', 7: 'graph_before', 8: 'graph_after', 9: 'cull_begin', 10: 'cull_end',
         11: 'root_suppressed', 12: 'eye_copy_recorded', 13: 'draw_begin', 14: 'draw_end',
         15: 'submit_list', 16: 'reset_list', 17: 'root_completed',18:'materialize_begin',19:'materialize_end',
         20:'proxy_gather_begin',21:'proxy_gather_end'}
FIELDS = ('sequence', 'qpc', 'scope', 'context', 'view', 'storage', 'input', 'output',
          'frame', 'kind', 'side', 'thread', 'stage', 'mode', 'flags0', 'flags1', 'offset', 'count', 'status', 'reserved')
FRONTIER_ROWS = (('ranges','Range','Ranges',24,64),('slots','Slot','Slots',20,4096),('submits','Submit','Submits',16,4096))
INFIX_FIELDS = ('enabled','frame','attempts','boundary','mainIndex','peerIndex','batchCount','suffixCount',
                'batchId','consumer','graphEpoch','fingerprint','prefixExecuted','consumerSubmitted','suffixExecuted','error')
TAIL_FIELDS = ('enabled','frame','firstOrdinal','lastOrdinal','sourceCount','baselineCaptured','baselineBytes','stage','qualified','error')
HANDOFF_STATS_FIELDS = ('schema','sourceCount','targetCount','stateCount','begins','beginRejected','barriers','transitions',
                        'aliases','writes','exits','closeOk','closeFailed','resets','pendingValidated','submitRejected')
HANDOFF_SOURCE_FIELDS = ('frame','side','list','generation','context','registry','targetFirst','targetCount',
                         'stateFirst','stateCount','flags','exitReason','exitKind','closeHRESULT','queue','failure')
HANDOFF_TARGET_FIELDS = ('source','slot','resource','viewIdentity','nativeRegistry','nativeHandle','creationKind','heap',
                         'heapOffset','allocationBytes','width','height','format','viewFormat','resourceFlags','viewFlags',
                         'mips','layers','planes','stateFirst','stateCount','flags','failure','seedState')
HANDOFF_STATE_FIELDS = ('source','target','subresource','seed','current','frozen','flags','transitions')
HANDOFF_ARRAYS = (('sources','Sources',HANDOFF_SOURCE_FIELDS,2),('targets','Targets',HANDOFF_TARGET_FIELDS,8),
                  ('states','States',HANDOFF_STATE_FIELDS,10))
HANDOFF_SOURCE_LOCAL_FIELDS = ('flags','failure','closeHRESULT','foreignFailure','foreignList','foreignKind',
                               'foreignEvents','submittedObserved')
HANDOFF_OWNER_FIELDS = ('nativeRecordTag','ownerObservation')
HANDOFF_V2_ARRAYS = (('sourceLocal','SourceLocal',HANDOFF_SOURCE_LOCAL_FIELDS,2),('owners','Owners',HANDOFF_OWNER_FIELDS,8))


def arm_scene_access_controls(read,write,claim):
    from scene_access_report import arm_controls,CONTROLS
    def owned_write(name,value):
        # Claim before WriteProcessMemory: a reported failure can follow a
        # partial write, and the outer finally must still retry both controls.
        if name==CONTROLS[0] and value==1:claim()
        write(name,value)
    return arm_controls(read,owned_write)


def capture_scene_access_failure(report,read_export,read_memory):
    from scene_access_report import read_report,PREFIX
    try:
        report['sceneAccess']=read_report(read_export,read_memory)
        return
    except Exception as error:report['sceneAccessEvidenceError']=str(error)
    observation=dict(completedVerified=False,stable=False,sequenceBefore=None,sequenceAfter=None)
    report['sceneAccessFailureObservation']=observation
    try:observation['sequenceBefore'],=struct.unpack('<I',read_export(PREFIX+'Seq',4))
    except Exception as error:observation['sequenceBeforeError']=str(error)
    try:report['sceneAccessFailureStats']=list(struct.unpack('<16Q',read_export(PREFIX+'Stats',128)))
    except Exception as error:report['sceneAccessFailureStatsError']=str(error)
    try:observation['sequenceAfter'],=struct.unpack('<I',read_export(PREFIX+'Seq',4))
    except Exception as error:observation['sequenceAfterError']=str(error)
    before,after=observation['sequenceBefore'],observation['sequenceAfter']
    observation['stable']=before is not None and before==after and not before&1 and 'sceneAccessFailureStats' in report


def decode_target_handoff(sequence, stats, arrays):
    def words(values,count):
        if (not isinstance(values,(list,tuple)) or len(values)!=count or
                any(type(value) is not int or not 0<=value<2**64 for value in values)):
            raise ValueError('Target handoff telemetry array ABI differs')
        return list(values)
    if type(sequence) is not int or not 0<=sequence<2**32 or sequence&1:
        raise ValueError('Target handoff sequence ABI differs or publication is odd')
    stats=words(stats,16)
    if stats[0] not in (1,2) or any(value>limit for value,limit in zip(stats[1:4],(2,8,10))):
        raise ValueError('Target handoff schema or occupied row count differs')
    if not isinstance(arrays,dict):raise ValueError('Target handoff row arrays are missing')
    raw,rows={},{}
    for name,_,fields,limit in HANDOFF_ARRAYS+(HANDOFF_V2_ARRAYS if stats[0]==2 else ()):
        values=raw[name]=words(arrays.get(name),len(fields)*limit)
        rows[name]=[dict(zip(fields,values[index:index+len(fields)])) for index in range(0,len(values),len(fields))]
    sources,targets,states=(rows[name] for name in ('sources','targets','states'))
    populated=[index for index,row in enumerate(sources) if any(row.values())]
    if stats[1]!=len(populated):raise ValueError('Target handoff occupied source count differs')
    target_count=state_count=0
    for index in populated:
        source=sources[index]
        if (source['side']!=index or source['targetFirst']!=index*4 or source['stateFirst']!=index*5 or
                source['targetCount']>4 or source['stateCount']>5):
            raise ValueError('Target handoff source range exceeds its fixed side slots')
        target_count+=source['targetCount'];state_count+=source['stateCount']
        state_first=source['stateFirst'];state_end=state_first+source['stateCount'];covered=set()
        for offset in range(source['targetCount']):
            target_index=source['targetFirst']+offset;target=targets[target_index]
            first,count=target['stateFirst'],target['stateCount']
            if (target['source']!=index or target['slot']!=(0,1,2,4)[offset] or
                    not state_first<=first<=state_end or count>state_end-first):
                raise ValueError('Target handoff target reference or state extent differs')
            for subresource in range(count):
                state_index=first+subresource;state=states[state_index]
                if (state_index in covered or state['source']!=index or state['target']!=target_index or
                        state['subresource']!=subresource):
                    raise ValueError('Target handoff state belongs to another source or target')
                covered.add(state_index)
        if covered!=set(range(state_first,state_end)):
            raise ValueError('Target handoff source omitted state rows')
    if stats[2:4]!=[target_count,state_count]:
        raise ValueError('Target handoff occupied target or state count differs')
    return dict(sequence=sequence,classification='OBSERVATION',executionPermission=False,exportPermission=False,
                nativeExportPermission=False,residencyPermission=False,ownershipPermission=False,
                placedHeapOwnershipProved=False,prefixNoninterferenceProved=False,
                gpuCompletionProved=False,abiValidated=True,stats=dict(zip(HANDOFF_STATS_FIELDS,stats)),
                raw=dict(stats=stats,**raw),populatedSources=populated,**rows,
                limitation='Observed identities and local states do not qualify native output handoff. Owner observations are untyped numbers; no execution, export, residency or ownership permission is granted.')


def read_target_handoff(read_export, *, attempts=8):
    prefix='CyberpunkVR_TargetHandoff'
    def words(suffix,count):
        raw=read_export(prefix+suffix,count*8)
        if not isinstance(raw,bytes) or len(raw)!=count*8:
            raise ValueError('Target handoff exported array ABI differs')
        return list(struct.unpack(f'<{count}Q',raw))
    def sequence():
        raw=read_export(prefix+'Seq',4)
        if not isinstance(raw,bytes) or len(raw)!=4:
            raise ValueError('Target handoff exported sequence ABI differs')
        return struct.unpack('<I',raw)[0]
    for _ in range(attempts):
        before=sequence()
        if before&1:continue
        stats=words('Stats',16)
        arrays={name:words(suffix,len(fields)*limit)
                for name,suffix,fields,limit in HANDOFF_ARRAYS+(HANDOFF_V2_ARRAYS if stats[0]==2 else ())}
        if before==sequence():return decode_target_handoff(before,stats,arrays)
    raise ValueError('Target handoff publication remained odd or changed during reading')


def decode_prepared_tail(values):
    if (not isinstance(values,(list,tuple)) or len(values)!=20 or
            any(type(value) is not int or not 0<=value<2**64 for value in values)):
        raise ValueError('Prepared tail telemetry ABI differs')
    return dict(stats=list(values),eyes=[dict(zip(TAIL_FIELDS,values[eye*10:(eye+1)*10])) for eye in range(2)])


def validate_prepared_tail(values, source, staged):
    from source_cbv_report import _source_rows,_words
    from staged_sources_report import analyze as analyze_staged
    result=decode_prepared_tail(values)
    stats,rows=_source_rows(source)
    if not isinstance(staged,dict) or not isinstance(staged.get('rows'),(list,tuple)):
        raise ValueError('Prepared tail lacks complete staged source coverage')
    staged_rows=[_words(row,8,'staged tail source') for row in staged['rows']]
    complete=analyze_staged(staged.get('stats'),b''.join(struct.pack('<8Q',*row) for row in staged_rows),source,complete=True)
    selected=set()
    for eye,tail in enumerate(result['eyes']):
        original=[row for row in rows if row[1]==eye]
        begin=len(original)
        while begin and original[begin-1][4]==9:begin-=1
        suffix=original[begin:]
        if (not suffix or tail['enabled']!=1 or tail['frame']!=stats[1] or
                tail['firstOrdinal']!=suffix[0][0] or tail['lastOrdinal']!=suffix[-1][0] or
                tail['sourceCount']!=len(suffix) or tail['baselineCaptured']!=1 or
                not 0<tail['baselineBytes']<=64*1024*1024 or tail['stage']!=9 or tail['qualified']!=1 or tail['error']):
            raise ValueError('Prepared tail is not the exact qualified final stage-9 interval for both eyes')
        if any(row[2]!=0 or complete['rows'][row[0]][2:4]!=[1,0] for row in suffix):
            raise ValueError('Prepared tail excluded an indirect or unready final source command')
        tail['ordinals']=[row[0] for row in suffix]
        selected.update(tail['ordinals'])
    if not 0<len(selected)<=512:raise ValueError('Prepared tail exceeds current source capacity')
    # Native side1 is array eye0; prepared source rows carry array-eye indices.
    result.update(frame=stats[1],sourceCount=len(selected),sources=[[row[0],1-row[1]] for row in rows if row[0] in selected],
                  nativeSources=[[row[0],row[1]] for row in rows if row[0] in selected],
                  verified=True,fullSourceCoverageVerified=True)
    return result


def decode_prepared_infix(values):
    if (not isinstance(values,(list,tuple)) or len(values)!=16 or
            any(type(value) is not int or not 0<=value<2**64 for value in values)):
        raise ValueError('Prepared infix telemetry ABI differs')
    return dict(zip(INFIX_FIELDS,values),limitation='Diagnostic owned consumer insertion does not replace native output or suppress native lists.')


def validate_prepared_infix(values, frame, current_source, packets):
    from read_prepared_pair import CURRENT_SOURCE_FIELDS,validate_current_source
    result=decode_prepared_infix(values)
    if not isinstance(current_source,dict):raise ValueError('Prepared infix lacks current source completion')
    validate_current_source([current_source.get(name) for name in CURRENT_SOURCE_FIELDS],frame,packets)
    enabled,source_frame,attempts,boundary,main,peer,count,suffix,batch,consumer,epoch,fingerprint,prefix,submitted,executed,error=values
    if (enabled!=1 or source_frame!=frame or attempts!=1 or main==peer or
            not 0<boundary<count<=256 or boundary!=max(main,peer)+1 or
            main>=boundary or peer>=boundary or suffix!=count-boundary or
            not all((batch,consumer,epoch,fingerprint)) or
            prefix!=boundary or submitted!=1 or executed!=suffix or error):
        raise ValueError('Prepared infix did not complete between the exact source prefix and native suffix')
    return result


def decode_submission_frontier(sequence, installed, state, failure, stats, details, blocks):
    if (any(type(value) is not int or not 0<=value<2**32 for value in (sequence,installed,state,failure)) or
            sequence&1 or installed>3 or state>4 or failure>14):
        raise ValueError('Submission frontier scalar ABI or sequence differs')
    for values in (stats,details):
        if not isinstance(values,(list,tuple)) or len(values)!=16 or any(type(value) is not int or not 0<=value<2**64 for value in values):
            raise ValueError('Submission frontier counter ABI differs')
    if not isinstance(blocks,dict):raise ValueError('Submission frontier row blocks are missing')
    if stats[15]:raise ValueError('Submission frontier observation acquired permission')
    rows={}
    for name,_,_,words,limit in FRONTIER_ROWS:
        raw=blocks.get(name)
        if not isinstance(raw,bytes) or len(raw)%(words*8) or len(raw)>limit*words*8:
            raise ValueError('Submission frontier row ABI or capacity differs')
        rows[name]=[list(row) for row in struct.iter_unpack(f'<{words}Q',raw)]
    ranges,slots,submits=(rows[name] for name in ('ranges','slots','submits'))
    if (stats[1]+stats[2]!=len(ranges) or stats[0]<len(ranges) or stats[3]!=len(slots) or
            list(stats[4:7])!=[sum(bool(row[6]) for row in slots),sum(bool(row[12]) for row in slots),
                         sum(bool(row[6]) and not row[12] for row in slots)] or stats[7]+stats[8]!=stats[1]):
        raise ValueError('Submission frontier ranges, slots or counters are incoherent')
    cursor=0
    for row in ranges:
        if row[12]!=cursor or row[13]>256 or row[13]>len(slots)-cursor or row[15] not in (0,1) or row[16]>14 or row[17]>8:
            raise ValueError('Submission frontier range extent or status differs')
        cursor+=row[13]
    if cursor!=len(slots):raise ValueError('Submission frontier omitted slot rows')
    for index,row in enumerate(slots):
        if row[2]>=len(ranges) or row[13] not in (0,1) or row[13]!=bool(row[12]) or bool(row[11])!=bool(row[12]):
            raise ValueError('Submission frontier slot reference or token fields differ')
        parent=ranges[row[2]]
        if row[:2]!=parent[:2] or not parent[12]<=index<parent[12]+parent[13] or row[3]!=parent[6]+index-parent[12]:
            raise ValueError('Submission frontier slot belongs to another range')
    batches={}
    for row in submits:
        if (not row[1] or not row[3] or not 0<row[4]<=256 or row[5]>=row[4] or row[7]>6 or
                (row[9]==2**64-1)!=(row[7] in (0,6))):
            raise ValueError('Submission frontier batch bounds or match kind differs')
        batch=batches.setdefault((row[0],row[1]),[])
        if row[5]!=len(batch) or (batch and row[:5]!=batch[0][:5]):
            raise ValueError('Submission frontier batch rows are incoherent')
        batch.append(row)
        if row[9]!=2**64-1:
            if row[9]>=len(slots):raise ValueError('Submission frontier submit refers to a missing slot')
            slot=slots[row[9]]
            if [row[0],row[8],row[6],row[10],row[12],row[13],row[14],row[15]]!=[slot[0],slot[1],slot[7],slot[11],slot[6],slot[4],slot[15],slot[16]]:
                raise ValueError('Submission frontier submit belongs to another source slot')
    counts=[sum(row[7] in kinds for row in submits) for kinds in ((1,2),(0,),(3,4,6),(5,))]
    if stats[9]!=len(batches) or list(stats[10:14])!=counts or any(len(batch)!=batch[0][4] for batch in batches.values()):
        raise ValueError('Submission frontier submit counters or batch coverage differ')
    return dict(sequence=sequence,installed=installed,state=state,failure=failure,stats=list(stats),details=list(details),**rows)


def read_submission_frontier(read_export, read_memory, *, attempts=8):
    prefix='CyberpunkVR_SubmissionFrontier'
    read32=lambda suffix:struct.unpack('<I',read_export(prefix+suffix,4))[0]
    for _ in range(attempts):
        sequence=read32('Seq')
        if sequence&1:continue
        installed,state,failure=(read32(name) for name in ('Installed','State','Failure'))
        stats=list(struct.unpack('<16Q',read_export(prefix+'Stats',128)))
        details=list(struct.unpack('<16Q',read_export(prefix+'Details',128)))
        blocks={}
        for name,singular,plural,words,limit in FRONTIER_ROWS:
            count,stride=read32(singular+'Count'),read32(singular+'Bytes')
            if count>limit or stride!=words*8:raise ValueError('Submission frontier exported extent or stride differs')
            pointer,=struct.unpack('<Q',read_export(prefix+plural,8))
            size=count*stride
            if size and (not pointer or pointer>2**64-1-size):raise ValueError('Submission frontier row pointer exceeds its bounded range')
            blocks[name]=read_memory(pointer,size) if size else b''
            if len(blocks[name])!=size:raise ValueError('Submission frontier row read was incomplete')
        if sequence==read32('Seq'):
            return decode_submission_frontier(sequence,installed,state,failure,stats,details,blocks)
    raise ValueError('Submission frontier publication remained odd or changed during reading')


def wait_owned_execution(read32, read_words, epoch, pair, *, timeout=15, on_snapshot=lambda snapshot: None,
                         monotonic=time.monotonic, sleep=time.sleep):
    if type(epoch) is not int or not 0<epoch<=0xffffffff or type(pair) is not int or not 0<pair<1<<64:
        raise ValueError('Invalid expected owned execution epoch/pair')
    deadline=monotonic()+timeout
    last=None
    while monotonic()<deadline:
        seq=read32('CyberpunkVR_OwnedExecutionSeq')
        if seq&1:
            sleep(.01);continue
        stats=list(read_words('CyberpunkVR_OwnedExecutionStats',24))
        resources=list(read_words('CyberpunkVR_OwnedExecutionResources',12))
        compiler=list(read_words('CyberpunkVR_OwnedExecutionCompiler',16))
        retirement=list(read_words('CyberpunkVR_OwnedExecutionRetirement',8))
        admission=list(read_words('CyberpunkVR_OwnedExecutionAdmission',6))
        closing=read32('CyberpunkVR_OwnedExecutionClosing')
        if seq!=read32('CyberpunkVR_OwnedExecutionSeq'):
            sleep(.01);continue
        if len(stats)!=24 or len(resources)!=12 or len(compiler)!=16 or len(retirement)!=8 or len(admission)!=6:
            raise ValueError('Owned execution telemetry ABI mismatch')
        last=dict(stats=stats,resources=resources,compiler=compiler,retirement=retirement,admission=admission,sequence=seq,closing=closing)
        on_snapshot(last)
        if closing or (stats[2:4]==[epoch,pair] and stats[0] in (4,5,6)):
            return last
        sleep(.02)
    identity=None if last is None else dict(status=last['stats'][0],epoch=last['stats'][2],pair=last['stats'][3])
    raise TimeoutError(f'Owned execution did not finish for epoch {epoch}, pair {pair}: {identity}')


def wait_recorder_armed(read,alive,*,timeout=3,monotonic=time.monotonic,sleep=time.sleep):
    deadline=monotonic()+timeout
    while monotonic()<deadline:
        if not alive():raise RuntimeError('Game exited before retained recorder initialization')
        request,state=read('Request'),read('State')
        if state==4:raise RuntimeError('Retained recorder rejected initialization')
        if not request and state==1:return dict(request=0,state=1,requestConsumed=True)
        sleep(.01)
    raise TimeoutError('Retained recorder was not initialized before preparation capture')


def read_resource_replay_snapshot(read32, read_words, read_rows, *, timeout=2,
                                  monotonic=time.monotonic, sleep=time.sleep):
    deadline=monotonic()+timeout
    fields=(('stats','Stats',16),('proof','Proof',8),('registration','Registration',8),
            ('reset','Reset',8),('asyncScope','Async',8),('mismatch','Mismatch',16))
    while monotonic()<deadline:
        seq=read32('CyberpunkVR_OwnedExecutionSeq')
        if seq&1:sleep(.002);continue
        result={key:list(read_words('CyberpunkVR_OwnedResourceReplay'+name,count)) for key,name,count in fields}
        valid=all(len(result[key])==count for key,_,count in fields)
        count=result['proof'][3] if valid else 221
        row_bytes=read32('CyberpunkVR_OwnedResourceReplayRowBytes')
        raw=read_rows(count) if valid and row_bytes==32 and count<=220 else b''
        if seq!=read32('CyberpunkVR_OwnedExecutionSeq'):sleep(.002);continue
        if not valid or row_bytes!=32 or count>220 or len(raw)!=count*32:
            raise ValueError('Native resource replay row ABI/count changed')
        return dict(result,raw=raw,sequence=seq)
    raise TimeoutError('Native resource replay has no coherent export snapshot')


def decode(raw, packets, max_frames=4,all_stages=False):
    if len(raw) % 112 or len(packets) % 16:
        raise ValueError('Invalid preparation ABI')
    rows = [dict(zip(FIELDS, r)) for r in struct.iter_unpack('<8Q2Ii9I', raw)]
    previous = 0
    for i, row in enumerate(rows):
        if row['sequence'] != i+1 or row['kind'] not in KINDS or row['side'] not in (-1, 0, 1) or not 1 <= row['frame'] <= max_frames or row['qpc'] < previous:
            raise ValueError('Invalid preparation identity/order')
        if row['offset'] + row['count'] > len(packets)//16 or row['count'] > 32768 or row['status'] not in (0, 1, 2):
            raise ValueError('Invalid preparation data range/status')
        if row['kind'] == 15 and (not row['context'] or not row['input'] or not row['scope'] or
                                  not 0 < row['flags1'] <= 256 or row['flags0'] >= row['flags1']):
            raise ValueError('Invalid command submission identity')
        if row['kind'] in (13,14) and (not row['output'] or not row['view'] or not row['scope'] or
                                      row['side'] not in (0,1) or (row['stage'] != 3 and not all_stages)):
            raise ValueError('Invalid native depth-draw identity')
        if row['kind'] in (20,21) and (not row['scope'] or row['side'] not in (0,1)):
            raise ValueError('Invalid native proxy-gather identity')
        previous = row['qpc']
    return rows


def proxy_preparation(rows, packets):
    ends=defaultdict(list)
    for row in rows:
        if row['kind']==21:ends[row['scope']].append(row)
    eyes=defaultdict(lambda:[Counter(),Counter()])
    ranges=Counter();incomplete=[];typed=True
    for start in (r for r in rows if r['kind']==20):
        matches=ends.pop(start['scope'],[])
        fields=('frame','side','thread','context','view','storage','input','output','count','offset','status','flags0','reserved')
        if (len(matches)!=1 or start['status'] or matches[0]['sequence']<=start['sequence'] or
                any(matches[0][k]!=start[k] for k in fields) or not start['storage'] or start['output']!=start['storage']):
            incomplete.append(start['sequence']);continue
        raw=packets[start['offset']*16:(start['offset']+start['count'])*16]
        if start['reserved']==0:
            typed=False
            values=[(identifier,method,0,0) for identifier,method in struct.iter_unpack('<2Q',raw)]
            expected=start['count']
        elif start['reserved']==2:
            if start['count']!=start['flags0']*2:raise ValueError('Invalid proxy-gather stride/count')
            values=list(struct.iter_unpack('<4Q',raw));expected=start['flags0']
            if any(not table for _,_,table,_ in values):raise ValueError('Invalid proxy vtable')
        else:raise ValueError('Unknown proxy-gather schema')
        if len(values)!=expected or any(not identifier or not method for identifier,method,table,update in values):
            raise ValueError('Invalid owned proxy-gather inputs')
        eyes[start['frame']][start['side']].update(values)
        ranges[start['frame'],start['side']]+=1
    if ends:raise ValueError('Unmatched proxy-gather completion')
    frames=[]
    for frame,(main,peer) in sorted(eyes.items()):
        common=main&peer
        methods=sorted({(method,table,update) for _,method,table,update in main.keys()|peer.keys()})
        families=[]
        for method,table,update in methods:
            count=lambda values:sum(n for (_,m,t,u),n in values.items() if (m,t,u)==(method,table,update))
            families.append(dict(method=hex(method),vtable=hex(table),update=hex(update),main=count(main),peer=count(peer),common=count(common)))
        frames.append(dict(frame=frame,ranges=[ranges[frame,0],ranges[frame,1]],
            calls=[sum(main.values()),sum(peer.values())],common=sum(common.values()),families=families))
    return dict(frames=frames,incomplete=incomplete,typed=typed,
                limitation='Common ID/vtable/method occurrences are duplicate preparation candidates, not proof of camera-independent output or saved work.')


def restore_controls(write, exited, graph=False, schedule=False, camera_write=None):
    errors = []
    values=[('Request',0),('State',0),('PairRequest',0),('PairMergeLimit',1)]
    if graph:values.extend([('GraphRequest',0),('GraphState',0)])
    if schedule:values.append(('ScheduleMode',0))
    if camera_write:values.append(('CameraTrace',0))
    operations=[(write,name,value) for name,value in values]
    if camera_write:operations.extend((camera_write,name,0) for name in ('Request','State','NodeFilter'))
    for setter, name, value in operations:
        try:
            setter(name, value)
        except OSError as error:
            if exited():
                return 'process_exited'
            errors.append(error)
    if errors:
        raise errors[0]
    return 'restored'


def analyze(rows, packets, frequency):
    def content(row):
        return packets[row['offset']*16:(row['offset']+row['count'])*16]
    scopes = defaultdict(list)
    for row in rows:
        if row['scope'] and row['kind'] in (1, 2, 3, 4, 5):
            scopes[row['scope']].append(row)
    finished = []
    for scope, events in scopes.items():
        kinds = [r['kind'] for r in events]
        if kinds[0] != 1 or kinds[-1] != 5:
            continue  # Capture boundaries can intersect a native stage.
        begin, end = events[0], events[-1]
        if any(r['frame'] != begin['frame'] or r['thread'] != begin['thread'] or r['side'] != begin['side'] for r in events):
            continue
        finished.append(dict(scope=scope, frame=begin['frame'], side=begin['side'], stage=begin['stage'], mode=begin['mode'],
                             reserved=begin['reserved'], flags=[begin['flags0'], begin['flags1']], storage=hex(begin['storage']),
                             input=hex(begin['input']), output=hex(begin['output']),
                             counts={KINDS[r['kind']]: r['count'] for r in events if r['kind'] in (2, 3, 4, 5)},
                             kinds=kinds, begin=begin['sequence'], end=end['sequence'],
                             observedMilliseconds=(end['qpc']-begin['qpc'])*1000/frequency))
    paired = defaultdict(lambda: defaultdict(list))
    for row in rows:
        if row['kind'] in (2, 3, 4, 5) and row['side'] in (0, 1) and row['stage'] < 256 and not row['status']:
            # +0x14 is copied from the work context, not the pass definition;
            # retain it as evidence, but do not mistake it for a pass identity.
            key = (row['frame'], row['stage'], row['mode'], row['flags0'], row['flags1'], row['kind'])
            paired[key][row['side']].append(row)
    comparisons = []
    for key, eyes in paired.items():
        if len(eyes[0]) != 1 or len(eyes[1]) != 1:
            continue
        a, b = eyes[1][0], eyes[0][0]
        av, bv = content(a), content(b)
        aset, bset = Counter(struct.iter_unpack('<2Q', av)), Counter(struct.iter_unpack('<2Q', bv))
        comparisons.append(dict(frame=key[0], stage=key[1], mode=key[2], kind=KINDS[key[-1]], counts=[a['count'], b['count']],
                                sequences=[a['sequence'], b['sequence']], equal=av == bv,
                                commonPackets=sum((aset & bset).values()),
                                workContextWords=[a['reserved'], b['reserved']],
                                sameStorage=a['storage'] == b['storage'], sameInput=a['input'] == b['input']))
    consumes = []
    for row in rows:
        if row['kind'] != 6 or row['status']:
            continue
        matches = [r for r in rows if r['kind'] == 5 and not r['status'] and r['frame'] == row['frame'] and
                   r['side'] == row['side'] and r['stage'] == row['stage'] and r['mode'] == row['mode'] and
                   r['view'] == row['view'] and r['qpc'] <= row['qpc'] and content(r) == content(row)]
        consumes.append(dict(frame=row['frame'], side=row['side'], stage=row['stage'], mode=row['mode'], count=row['count'],
                             sequence=row['sequence'], matchingStageEnds=[r['sequence'] for r in matches]))
    order = [{**{k: r[k] for k in ('sequence', 'frame', 'side', 'stage', 'mode', 'count', 'flags0', 'flags1')},
              'kind': KINDS[r['kind']], 'view': hex(r['view']), 'storage': hex(r['storage']),
              'milliseconds': (r['qpc']-rows[0]['qpc'])*1000/frequency} for r in rows if r['kind'] in (1, 5, 6, 7, 8, 9, 10)]
    scheduling = []
    for frame in sorted({r['frame'] for r in rows}):
        selected = [r for r in rows if r['frame'] == frame]
        vrcam = [r for r in selected if r['kind'] == 6 and r['side'] == 1 and r['stage'] == 3]
        main = [r for r in selected if r['kind'] == 1 and r['side'] == 0 and r['stage'] == 3]
        if len(vrcam) == 1 and len(main) == 1:
            scheduling.append(dict(frame=frame, vrcamDepthConsumed=vrcam[0]['sequence'], mainDepthPreparationBegins=main[0]['sequence'],
                                   mainAvailableBeforeVrcam=main[0]['sequence'] < vrcam[0]['sequence'],
                                   sameTransientStorage=main[0]['storage'] == vrcam[0]['storage']))
    materialization=[]
    for start in (r for r in rows if r['kind']==18):
        end=next((r for r in rows if r['kind']==19 and r['scope']==start['scope']),None)
        if end and end['frame']==start['frame']:
            materialization.append(dict(frame=start['frame'],side=start['side'],view=hex(start['view']),
                storage=hex(start['storage']),inputStorage=hex(start['input']),query=hex(start['output']),
                workFlags=start['flags1']&3,workFlagsRaw=start['flags1'],viewIndex=start['mode']&255,
                viewWordRaw=start['mode'],candidates=start['flags0'],
                begin=start['sequence'],end=end['sequence']))
    return dict(kinds=dict(Counter(KINDS[r['kind']] for r in rows)), completedStages=finished, scheduling=scheduling,
                materialization=materialization,proxyPreparation=proxy_preparation(rows,packets),
                comparisons=comparisons, consumes=consumes, order=order, nativeWorkChanged=False,
                limitation='Observed intervals include copying overhead and are not performance measurements. Packet equality is not a mutable-resource ownership proof.')


def eye_copy_order(rows, frame, view):
    """Prove depth-to-copy list order, not pixel equivalence or GPU completion."""
    generations = Counter()
    identities, submissions, begins = {}, defaultdict(list), {}
    for row in rows:
        kind = row['kind']
        if kind == 16:
            generations[row['context']] += 1
        if kind in (12, 13, 14, 15):
            command_list = row['output'] if kind in (13, 14) else row['context']
            key = command_list, generations[command_list]
            identities[row['sequence']] = key
            if kind == 15:
                submissions[key].append(row)
            if kind == 13:
                begins[row['scope'], command_list] = row
    copies = [r for r in rows if r['kind'] == 12 and r['frame'] == frame]
    ends = [r for r in rows if r['kind'] == 14 and r['stage']==3 and r['frame'] == frame and r['side'] == 1 and r['view'] == view]
    checks = []
    for copy in copies:
        tagged = copy['side'] == 1 and copy['view'] == view and copy['flags0'] != 0
        ckey = identities[copy['sequence']]
        cs = submissions[ckey]
        for end in ends:
            key = identities[end['sequence']]
            begin = begins.get((end['scope'], key[0]))
            ds = submissions[key]
            reason, ordered = 'incomplete-recording-or-submission', False
            if (tagged and begin and identities[begin['sequence']] == key and end['flags0'] > 0
                    and len(ds) == len(cs) == 1 and ds[0]['sequence'] > end['sequence']
                    and cs[0]['sequence'] > copy['sequence']):
                if key == ckey:
                    ordered = end['sequence'] < copy['sequence']
                    reason = 'same-list' if ordered else 'copy-recorded-before-depth'
                elif ds[0]['input'] != cs[0]['input']:
                    reason = 'different-queues-require-fence-evidence'
                elif ds[0]['scope'] == cs[0]['scope']:
                    reason = 'same-batch-requires-resource-dependency-evidence'
                else:
                    ordered = ds[0]['sequence'] < cs[0]['sequence']
                    reason = 'separate-ordered-submissions' if ordered else 'copy-submitted-before-depth'
            checks.append(dict(copy=copy['sequence'], depthEnd=end['sequence'], ordered=ordered, reason=reason,
                               copyList=hex(ckey[0]), depthList=hex(key[0]),
                               copySubmissions=[s['sequence'] for s in cs], depthSubmissions=[s['sequence'] for s in ds]))
    return dict(passed=bool(copies and ends and checks) and all(c['ordered'] for c in checks),
                copies=len(copies), depthLists=len(ends), checks=checks,
                limitation='Native depth list ordering only; no GPU completion or final-image equivalence claim.')


def main(argv=None):
    from live_read import Reader
    from live_symbols import resolve
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--pid', type=int, required=True)
    p.add_argument('--out', type=Path, required=True)
    p.add_argument('--compact', action='store_true', help='Keep detailed arrays in the output file, not stdout')
    p.add_argument('--frames', type=int, choices=range(1, 9), default=2)
    p.add_argument('--check-pair-admission', '--pair-prepare', dest='pair_prepare', action='store_true',
                   help='Verify refusal of a paired graph without both output bindings (does not enable it)')
    p.add_argument('--main-root-pair', action='store_true', help='One-shot native paired-view selection in an output-owning MAIN root')
    p.add_argument('--merge-root', action='store_true', help='After a verified warmup, replace one standalone VRCAM root with paired MAIN')
    p.add_argument('--merge-frames', type=int, choices=range(1,8), default=1, help='Consecutive merged roots; keep one normal tail frame in the capture')
    p.add_argument('--graph-plan',action='store_true',help='Also own the native combined graph before its declaration jobs')
    p.add_argument('--schedule-preparation',choices=('check','apply'),help='Check or apply the bounded early VRCAM CPU schedule')
    p.add_argument('--paired-cameras',action='store_true',help='Compare MAIN-first peer camera preparation with the exact native VRCAM depth pass')
    p.add_argument('--camera-turn',type=Path,help='Step and restore the simulator pose during this camera capture')
    p.add_argument('--early-depth-gpu',type=draw_limit,help='Record this many paired depth draws, or all compatible draws, and compare native references')
    p.add_argument('--whole-depth-target',action='store_true',help='Route the entire native depth group and verify both native copy-back targets')
    p.add_argument('--optimized-depth',action='store_true',help='Execute the bounded native depth group without reference draws')
    p.add_argument('--reject-depth-commit',action='store_true',help='Exercise native fallback by withholding optimized admission')
    p.add_argument('--native-depth-timing',action='store_true',help='Time the same native group without replacing any draw or target')
    p.add_argument('--native-camera',action='store_true',help='Use the native heap and MAIN-prefix twin camera for the audited depth family')
    p.add_argument('--native-camera-stages',type=int,choices=(1,3),default=1)
    p.add_argument('--skinning-depth',action='store_true')
    p.add_argument('--alpha-depth-reference',action='store_true')
    p.add_argument('--alpha-depth-optimized',action='store_true')
    p.add_argument('--skinned-alpha-depth',action='store_true')
    p.add_argument('--material-depth',action='store_true',help='Read GPU b4 at alpha depth draws without merging them')
    p.add_argument('--material-hints',type=Path,help='Same-process prior GPU material report supplying descriptor index hints')
    p.add_argument('--pass-targets',action='store_true',help='Capture all native geometry pass boundaries and bound target identities')
    p.add_argument('--retained-geometry',choices=('reference','replace'),help='Compare or route native geometry through retained array layers')
    p.add_argument('--retained-stage',type=int,choices=(6,7),default=7)
    p.add_argument('--retained-source-stream',action='store_true',help='Own every original direct/indirect source occurrence and full graphics bindings')
    p.add_argument('--retained-indirect-snapshots',action='store_true',help='Copy actual indirect source arguments and compare the private-buffer reference')
    p.add_argument('--retained-source-cbvs',action='store_true',help='Verify current source CBV owners including native direct b5 creation')
    p.add_argument('--retained-source-tables',action='store_true',help='Own finite source descriptor tables; report bindless uses as deferred')
    p.add_argument('--retained-source-details',action='store_true',help='Observe actual source CBV heap, creation and native allocation/state metadata')
    p.add_argument('--retained-cbv-snapshots',action='store_true',help='Copy DEFAULT source CBV payloads at their original draw; UPLOAD stays deferred')
    p.add_argument('--retained-upload-snapshots','--retained-direct-upload',dest='retained_direct_upload',action='store_true',help='Copy qualified completed UPLOAD writers within the original queued epoch')
    p.add_argument('--retained-source-selectors',action='store_true',help='Observe exact original selector intent, compact descriptors and GPU byte matching')
    p.add_argument('--retained-ia-snapshots',action='store_true',help='Observe and copy admitted source IA views, preserving original coordinates')
    p.add_argument('--prepared-current',action='store_true',help='Arm the owned current-frame sequence before retained source capture')
    p.add_argument('--prepared-infix',action='store_true',help='Insert the diagnostic owned consumer between original native lists in the coupled source batch')
    p.add_argument('--prepared-tail',action='store_true',help='Compare the complete final stage-9 source tail against the native exit')
    p.add_argument('--allow-vrs-refinement',action='store_true',help='Accept the approved shared finer VRS map only with complete native-rate attribution proof')
    p.add_argument('--native-export',action='store_true',help='Arm owned native output copies for the approved current-frame final tail')
    p.add_argument('--full-t5',action='store_true',help='Compare complete original T5 views at every current-frame final-tail source')
    p.add_argument('--actual-membership',action='store_true',help='Check actual private B4/full-T5 selectors against owned descriptor bitmaps')
    p.add_argument('--borrowed-tables',action='store_true',help='Observe exact active descriptor ranges before and after the native prefix')
    p.add_argument('--fixed-assets',action='store_true',help='Classify full borrowed dynamic ranges with actual fixed-allocation bootstrap pins')
    p.add_argument('--paired-gbuffer',action='store_true',help='Compare one delayed paired native G-buffer draw with both original eyes')
    p.add_argument('--paired-gbuffer-ordinal',type=int,choices=range(64),default=0,help='Zero-based eligible MAIN quad selector for the paired reference')
    p.add_argument('--paired-gbuffer-family',type=int,choices=(40,70),default=40,help='Exact audited shader pair for the native reference')
    p.add_argument('--owned-graph-sort',action='store_true',help='Native-sort owned original and candidate graphs without redirecting jobs')
    p.add_argument('--whole-gbuffer-plan',action='store_true',help='Native-sort the complete discard frontier and verify CPU context-prefix readiness without execution')
    p.add_argument('--owned-reference',action='store_true',help='Execute one owned reference graph with unchanged native dependencies')
    p.add_argument('--owned-coupled',action='store_true',help='Experimentally execute one natively recompiled CPU/GPU coupled graph; preserve all native draws')
    p.add_argument('--scene-split',action='store_true',help='Split the peer discard SceneDriver into two native nodes without suppressing any draw')
    p.add_argument('--scene-split-eyes',choices=('one','both'),default='one',help='Select one peer eye or both eyes; both implies --scene-split')
    p.add_argument('--scene-access',action='store_true',help='Observe peer-tail/main-prefix operands without granting reordering permission')
    a = p.parse_args(argv)
    if a.scene_split_eyes=='both':a.scene_split=True
    from scene_access_report import validate_options as validate_scene_access_options
    try:validate_scene_access_options(a.scene_access,eyes=a.scene_split_eyes,retained_geometry=a.retained_geometry,pass_targets=a.pass_targets)
    except ValueError as error:p.error(str(error))
    from scene_split_report import validate_options as validate_scene_split_options
    try:validate_scene_split_options(a.scene_split,prepared_current=a.prepared_current,prepared_infix=a.prepared_infix,
        native_export=a.native_export,source_stream=a.retained_source_stream)
    except ValueError as error:p.error(str(error))
    if a.scene_split:a.owned_coupled=True
    if a.actual_membership and not (a.full_t5 and a.prepared_current and a.prepared_infix and a.prepared_tail):
        p.error('--actual-membership requires --full-t5, --prepared-current, --prepared-infix and --prepared-tail')
    if a.fixed_assets and not a.borrowed_tables:p.error('--fixed-assets requires --borrowed-tables')
    if a.borrowed_tables and not (a.full_t5 and a.prepared_current and a.prepared_infix and a.prepared_tail):
        p.error('--borrowed-tables requires --full-t5, --prepared-current, --prepared-infix and --prepared-tail')
    if a.full_t5 and not (a.prepared_current and a.prepared_infix and a.prepared_tail):
        p.error('--full-t5 requires --prepared-current, --prepared-infix and --prepared-tail')
    if a.native_export and not (a.prepared_current and a.prepared_infix and a.prepared_tail and a.allow_vrs_refinement):
        p.error('--native-export requires --prepared-current, --prepared-infix, --prepared-tail and --allow-vrs-refinement')
    if a.allow_vrs_refinement and not a.prepared_tail:p.error('--allow-vrs-refinement requires --prepared-tail')
    if a.prepared_tail and not a.prepared_infix:p.error('--prepared-tail requires --prepared-infix')
    if a.prepared_infix and not (a.prepared_current and a.owned_coupled):
        p.error('--prepared-infix requires --prepared-current and --owned-coupled')
    if a.owned_coupled:
        a.owned_reference=True
        if a.whole_gbuffer_plan or a.owned_graph_sort:
            p.error('--owned-coupled cannot apply a second legacy topology plan')
    if a.retained_source_stream and not (a.whole_gbuffer_plan or a.owned_coupled):p.error('--retained-source-stream requires an owned whole-pass schedule')
    if a.retained_source_stream and a.owned_coupled and a.retained_stage!=7:p.error('Coupled source capture requires the whole G-buffer discard pass')
    if a.retained_indirect_snapshots and (not a.retained_source_stream or a.retained_geometry!='reference'):
        p.error('--retained-indirect-snapshots requires the retained source reference stream')
    if a.retained_source_cbvs and not a.retained_source_stream:p.error('--retained-source-cbvs requires --retained-source-stream')
    if a.retained_source_tables and not a.retained_source_cbvs:p.error('--retained-source-tables requires --retained-source-cbvs')
    if a.retained_source_details and not a.retained_source_cbvs:p.error('--retained-source-details requires --retained-source-cbvs')
    if a.retained_cbv_snapshots and not (a.retained_source_tables and a.retained_source_details):p.error('--retained-cbv-snapshots requires --retained-source-tables and --retained-source-details')
    if a.retained_direct_upload and not a.retained_cbv_snapshots:p.error('--retained-direct-upload requires --retained-cbv-snapshots')
    if a.retained_source_selectors and not a.retained_cbv_snapshots:p.error('--retained-source-selectors requires --retained-cbv-snapshots')
    if a.retained_ia_snapshots and not a.retained_source_stream:p.error('--retained-ia-snapshots requires --retained-source-stream')
    if a.prepared_current and (a.retained_geometry!='reference' or a.retained_stage!=7 or not a.retained_source_selectors or not a.retained_ia_snapshots):
        p.error('--prepared-current requires full retained reference stage7, source selectors and owned IA snapshots')
    if a.whole_gbuffer_plan:
        a.owned_reference=True
        if a.owned_graph_sort:p.error('--whole-gbuffer-plan uses owned reference execution, not legacy diagnostic source tickets')
        if a.retained_geometry!='reference' or a.retained_stage!=7 or not a.pass_targets:
            p.error('--whole-gbuffer-plan requires retained reference stage7 and pass targets')
    if a.owned_reference and (a.owned_graph_sort or a.retained_geometry!='reference' or not a.pass_targets or a.schedule_preparation!='apply' or not a.merge_root):
        p.error('--owned-reference requires merged retained reference geometry, pass targets and schedule apply, without diagnostic owned sorting')
    if a.paired_gbuffer_ordinal and not a.paired_gbuffer:p.error('--paired-gbuffer-ordinal requires --paired-gbuffer')
    if a.paired_gbuffer_family!=40 and not a.paired_gbuffer:p.error('--paired-gbuffer-family requires --paired-gbuffer')
    if a.owned_graph_sort and (not a.graph_plan and not a.schedule_preparation or not a.pass_targets):p.error('--owned-graph-sort requires a complete paired pass/graph capture')
    from early_depth_report import validate_skinned_alpha_options
    try:validate_skinned_alpha_options(a.skinned_alpha_depth,a.skinning_depth,a.alpha_depth_reference,a.alpha_depth_optimized,native_timing=a.native_depth_timing)
    except ValueError as error:p.error(str(error))
    try:validate_paired_geometry_options(a.paired_gbuffer,pass_targets=a.pass_targets,
        paired_apply=a.merge_root and a.schedule_preparation=='apply',
        conflicts=(a.retained_geometry,a.early_depth_gpu,a.paired_cameras,a.camera_turn,a.merge_frames!=1))
    except ValueError as error:p.error(str(error))
    if a.schedule_preparation:a.graph_plan=True
    try:validate_retained_options(a.retained_geometry,a.retained_stage,pass_targets=a.pass_targets,
        paired_apply=a.merge_root and a.schedule_preparation=='apply',
        conflicts=(a.early_depth_gpu,a.whole_depth_target,a.optimized_depth,a.reject_depth_commit,a.native_depth_timing,
                   a.native_camera,a.native_camera_stages!=1,a.skinning_depth,a.alpha_depth_reference,a.alpha_depth_optimized,
                   a.skinned_alpha_depth,a.material_depth,a.material_hints,a.paired_cameras,a.camera_turn,a.merge_frames!=1))
    except ValueError as error:p.error(str(error))
    if a.paired_cameras and a.schedule_preparation!='apply':p.error('--paired-cameras requires --schedule-preparation apply')
    if a.camera_turn and not a.paired_cameras:p.error('--camera-turn requires --paired-cameras')
    if a.early_depth_gpu and a.schedule_preparation!='apply':p.error('--early-depth-gpu requires the early CPU schedule')
    if a.optimized_depth:a.whole_depth_target=True
    if a.native_depth_timing:
        if a.optimized_depth or a.whole_depth_target or not a.early_depth_gpu:p.error('Native timing requires an early-depth request and no replacement mode')
    if a.reject_depth_commit and not a.optimized_depth:p.error('--reject-depth-commit requires --optimized-depth')
    if a.whole_depth_target and not a.early_depth_gpu:p.error('--whole-depth-target requires --early-depth-gpu')
    if a.native_camera_stages==3:a.native_camera=True
    if a.native_camera and (not a.early_depth_gpu or a.native_depth_timing):p.error('Native camera requires a depth render comparison')
    if a.skinning_depth and not a.native_camera:p.error('Skinning depth requires native camera binding')
    if a.alpha_depth_reference and (not a.native_camera or not a.whole_depth_target or a.optimized_depth or a.native_depth_timing):
        p.error('Alpha reference requires whole-depth mode1 and native camera')
    if a.alpha_depth_optimized and (a.alpha_depth_reference or not a.native_camera or not a.optimized_depth or not a.material_hints):
        p.error('Optimized alpha requires native camera, optimized depth and current-process material hints')
    if a.material_depth and (not a.whole_depth_target or a.optimized_depth or a.native_depth_timing):p.error('Material capture requires whole-depth reference mode')
    if a.material_hints and not (a.material_depth or a.alpha_depth_optimized):p.error('Texture hints require material capture or optimized alpha')
    if a.pass_targets and (not (a.early_depth_gpu or a.retained_geometry or a.paired_gbuffer) or a.native_depth_timing):p.error('Pass targets require a geometry capture')
    if (a.frames>4 or a.merge_frames>3) and not a.paired_cameras:p.error('Extended trace requires --paired-cameras')
    if a.merge_frames!=1 and not a.merge_root:p.error('--merge-frames requires --merge-root')
    if a.merge_root and a.merge_frames>=a.frames:p.error('Merged roots require at least one normal tail frame')
    if a.graph_plan and not a.merge_root:p.error('--graph-plan requires --merge-root')
    if sum((a.main_root_pair,a.merge_root,a.pair_prepare))>1:p.error('Choose one pair test mode')
    a.main_root_pair=a.main_root_pair or a.merge_root
    a.pair_prepare=a.pair_prepare or a.main_root_pair
    game = Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077')
    symbols = resolve(a.pid, game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                      ['CyberpunkVR_Preparation*','CyberpunkVR_SinglePassTrace*','CyberpunkVR_EarlyDepth*','CyberpunkVR_NativeStereo*',
                       'CyberpunkVR_NativeStereoAllDraws','CyberpunkVR_GeometryPacket*','CyberpunkVR_GeometryOrigin*','CyberpunkVR_ConstantCaptureEnable',
                       'CyberpunkVR_DescriptorCaptureFailure','CyberpunkVR_DescriptorStoreStats','CyberpunkVR_DescriptorHeapSlots',
                       'CyberpunkVR_StereoFamilyAssetStatus','CyberpunkVR_RetainedGeometry*','CyberpunkVR_RetainedNativeTable*','CyberpunkVR_PairedGeometry*','CyberpunkVR_StereoGpuProbe*',
                       'CyberpunkVR_ConstantBankRequest','CyberpunkVR_GeometryFreezeEnable','CyberpunkVR_SinglePassTwinUpload', 'CyberpunkVR_CullReuseMode', 'g_menuModeValue',
                       'CyberpunkVR_TargetAccess*','CyberpunkVR_RetainedTargetLifetime','CyberpunkVR_PreparedNativeExport*','CyberpunkVR_RetainedFullT5*','CyberpunkVR_PreparedActualMembership*',
                       'CyberpunkVR_RetainedBorrowed*','CyberpunkVR_RetainedPageableUse*','CyberpunkVR_RetainedPageableSearch',
                       'CyberpunkVR_RetainedFixedAssets*','CyberpunkVR_RetainedFixedAsset*','CyberpunkVR_RetainedFixedSlot*','CyberpunkVR_RetainedFixedDescriptor*',
                       'CyberpunkVR_OwnedStage*','CyberpunkVR_OwnedScene*','CyberpunkVR_SceneAccess*','CyberpunkVR_SceneChildEnable',
                       'CyberpunkVR_OwnedExecution*','CyberpunkVR_OwnedResourceReplay*','CyberpunkVR_SubmissionFrontier*','CyberpunkVR_TargetHandoff*','CyberpunkVR_PreparedCurrent*','CyberpunkVR_PreparedInfixEnable','CyberpunkVR_RetainedInfix','CyberpunkVR_PreparedTailEnable','CyberpunkVR_PreparedVrsRefinementEnable','CyberpunkVR_RetainedTail','CyberpunkVR_PreparedPairRequest','CyberpunkVR_PreparedPairState','CyberpunkVR_PreparedPairStats','CyberpunkVR_PreparedPairResidency','CyberpunkVR_RetainedBaseline','CyberpunkVR_RetainedCurrent*','CyberpunkVR_RetainedGuarded*','CyberpunkVR_RetainedStaged*','CyberpunkVR_RetainedSourceStream*','CyberpunkVR_RetainedSourceBoundary*','CyberpunkVR_RetainedSourceQueueOrder','CyberpunkVR_RetainedIndirectBindings*',
                       'CyberpunkVR_RetainedIndirectSnapshots*','CyberpunkVR_RetainedSourceCbv*','CyberpunkVR_SourceCbvBindingEnable','CyberpunkVR_RetainedSourceTables*','CyberpunkVR_RetainedSourceDetails*','CyberpunkVR_SourceCbvSnapshots*','CyberpunkVR_RetainedSourceSelectors*','CyberpunkVR_RetainedSourceAssembly*','CyberpunkVR_RetainedSelectorCoverage*','CyberpunkVR_RetainedSelectorCacheEvent*','CyberpunkVR_RetainedT5*','CyberpunkVR_StructuredBufferSnapshotsEnable','CyberpunkVR_StructuredBufferFinalizeDiagnostic','CyberpunkVR_RetainedIa*','CyberpunkVR_NativeIaSnapshotsEnable','CyberpunkVR_RetainedInstance*','CyberpunkVR_NativeInstanceObserver*','CyberpunkVR_NativeInstanceGpuProbePhase','CyberpunkVR_RetainedPrivateGeometry*','CyberpunkVR_RetainedMaterial*'])
    reader = Reader(a.pid, game/'bin/x64/Cyberpunk2077.exe', 0, 0)
    k = reader.k
    k.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    k.WaitForSingleObject.argtypes = [c.c_void_p, c.c_uint32]
    handle = None
    armed = False
    scene_access_owned = False
    turn=None;turned=False
    report = dict(pid=a.pid, dll=symbols['sha256'], passed=False, frames=a.frames, pairedRequest=a.pair_prepare,
                  preparedInfixRequested=a.prepared_infix,preparedTailRequested=a.prepared_tail,allowVrsRefinement=a.allow_vrs_refinement,
                  nativeExportRequested=a.native_export,nativeExportAvailable='CyberpunkVR_PreparedNativeExport' in symbols,
                  fullT5Requested=a.full_t5,fullT5Available='CyberpunkVR_RetainedFullT5Stats' in symbols,
                  actualMembershipRequested=a.actual_membership,actualMembershipAvailable='CyberpunkVR_PreparedActualMembershipStats' in symbols,
                  borrowedTablesRequested=a.borrowed_tables,borrowedTablesAvailable='CyberpunkVR_RetainedBorrowedTablesStats' in symbols,
                  fixedAssetsRequested=a.fixed_assets,fixedAssetsAvailable='CyberpunkVR_RetainedFixedAssetsStats' in symbols,
                  sceneSplitRequested=a.scene_split,sceneSplitEyes=a.scene_split_eyes if a.scene_split else None,
                  sceneAccessRequested=a.scene_access)
    address = lambda name: int(symbols['CyberpunkVR_Preparation'+name], 16)
    read = lambda name: struct.unpack('<I', reader.read(address(name), 4))[0]
    def write(name, value, prefix='CyberpunkVR_Preparation'):
        data, done = c.c_uint32(value), c.c_size_t()
        if not k.WriteProcessMemory(handle, int(symbols[prefix+name],16), c.byref(data), 4, c.byref(done)) or done.value != 4:
            raise c.WinError(c.get_last_error())
    camera_read=lambda name:struct.unpack('<I',reader.read(int(symbols['CyberpunkVR_SinglePassTrace'+name],16),4))[0]
    camera_write=lambda name,value:write(name,value,'CyberpunkVR_SinglePassTrace')
    read_full=lambda name:struct.unpack('<I',reader.read(int(symbols[name],16),4))[0]
    early_read=lambda name:read_full('CyberpunkVR_EarlyDepth'+name)
    retained_read=lambda name:read_full('CyberpunkVR_RetainedGeometry'+name)
    paired_read=lambda name:read_full('CyberpunkVR_PairedGeometry'+name)
    def capture_scene_access():
        from scene_access_report import read_report
        report['sceneAccess']=read_report(lambda name,size:reader.read(int(symbols[name],16),size),reader.read)
    def claim_scene_access():
        nonlocal scene_access_owned
        scene_access_owned=True
    def capture_actual_membership(verify=False):
        if report['actualMembershipAvailable']:
            from actual_membership_report import read_report,validate_partial
            value=read_report(lambda name,size:reader.read(int(symbols[name],16),size))
            report['actualMembership']=value
            if verify:report['actualMembership']=validate_partial(value,requested=a.actual_membership,
                frame=report.get('sourceStream',{}).get('stats',[0,0])[1])
        elif a.actual_membership:raise ValueError('Actual membership telemetry is unavailable')
    def capture_fixed_assets(verify=False):
        if report['fixedAssetsAvailable']:
            from fixed_assets_report import read_report,validate
            value=read_report(lambda name,size:reader.read(int(symbols[name],16),size),reader.read,
                samples_available='CyberpunkVR_RetainedFixedDescriptorRows' in symbols)
            report['fixedAssets']=value
            if verify:report['fixedAssets']=validate(value,requested=a.fixed_assets,source=report.get('sourceStream'),
                tail=report.get('preparedTail'),borrowed=report.get('borrowedTables'))
        elif a.fixed_assets:raise ValueError('Fixed-assets telemetry is unavailable in this build')
    def capture_borrowed_tables(verify=False):
        if report['borrowedTablesAvailable']:
            from borrowed_tables_report import read_report,validate
            value=read_report(lambda name,size:reader.read(int(symbols[name],16),size),reader.read,
                pageable_available='CyberpunkVR_RetainedPageableUseStats' in symbols,
                search_available='CyberpunkVR_RetainedPageableSearch' in symbols)
            report['borrowedTables']=value
            if verify:report['borrowedTables']=validate(value,requested=a.borrowed_tables,
                source=report.get('sourceStream'),tail=report.get('preparedTail'))
        elif a.borrowed_tables:
            raise ValueError('Borrowed-table telemetry is unavailable in this build')
    def capture_full_t5(verify=False):
        if report['fullT5Available']:
            from full_t5_report import read_report,validate
            value=read_report(lambda name,size:reader.read(int(symbols[name],16),size),reader.read)
            report['fullT5']=value
            if verify:report['fullT5']=validate(value['stats'],value['rows'],requested=a.full_t5,
                source=report.get('sourceStream'),tail=report.get('preparedTail'))
        elif a.full_t5:
            raise ValueError('Full T5 telemetry is unavailable in this build')
    def capture_export():
        if report['nativeExportAvailable']:
            from native_export_report import decode
            values=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_PreparedNativeExport'],16),192)))
            report['nativeExportRaw']=values;report['nativeExport']=decode(values)
            if a.native_export and report['nativeExport']['error']:
                raise ValueError(f"Native export refused or failed: {report['nativeExport']['error']}")
        elif a.native_export:
            raise ValueError('Native export telemetry is unavailable in this build')
    def capture_tail():
        values=list(struct.unpack('<20Q',reader.read(int(symbols['CyberpunkVR_RetainedTail'],16),160)))
        report['preparedTail']=decode_prepared_tail(values)
        return values
    def capture_handoff():
        report['targetHandoff']=read_target_handoff(lambda name,size:reader.read(int(symbols[name],16),size))
        if 'CyberpunkVR_TargetAccessSeq' in symbols:
            from target_window_report import read_report
            report['targetWindow']=read_report(lambda name,size:reader.read(int(symbols[name],16),size))
        return report['targetHandoff']
    def capture_infix():
        values=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedInfix'],16),128)))
        report['preparedInfix']=decode_prepared_infix(values)
        return values
    def capture_frontier():
        value=read_submission_frontier(lambda name,size:reader.read(int(symbols[name],16),size),reader.read)
        if 'CyberpunkVR_SubmissionFrontierBoundaryFailure' in symbols:
            value['boundaryFailure']=read_full('CyberpunkVR_SubmissionFrontierBoundaryFailure')
        report['submissionFrontier']=value
        for name,_,_,words,_ in FRONTIER_ROWS:
            a.out.with_name(a.out.stem+'-submission-frontier-'+name+'.bin').write_bytes(
                b''.join(struct.pack(f'<{words}Q',*row) for row in value[name]))
    def capture_native_targets():
        from pass_target_order import TARGET_BYTES
        native_seq=read_full('CyberpunkVR_NativeStereoProbeSeq');native_count=read_full('CyberpunkVR_NativeStereoDrawCount')
        if native_seq&1 or native_count>4096:raise ValueError('Invalid native draw publication')
        native=reader.read(int(symbols['CyberpunkVR_NativeStereoDraws'],16),native_count*1096)
        targets=b''
        if a.pass_targets:
            if read_full('CyberpunkVR_NativeStereoTargetRecordBytes')!=TARGET_BYTES:raise ValueError('Native pass target ABI mismatch')
            targets=reader.read(int(symbols['CyberpunkVR_NativeStereoTargets'],16),native_count*TARGET_BYTES)
            a.out.with_name(a.out.stem+'-pass-targets.bin').write_bytes(targets)
        if native_seq!=read_full('CyberpunkVR_NativeStereoProbeSeq'):raise ValueError('Native draws changed')
        a.out.with_name(a.out.stem+'-early-draws.bin').write_bytes(native)
        return native,targets
    early_controls=('CyberpunkVR_EarlyDepthSkinnedAlpha','CyberpunkVR_EarlyDepthAlphaOptimized','CyberpunkVR_EarlyDepthAlphaReference','CyberpunkVR_EarlyDepthMaterialHintCount','CyberpunkVR_EarlyDepthMaterialCapture','CyberpunkVR_EarlyDepthSkinning','CyberpunkVR_EarlyDepthNativeCamera','CyberpunkVR_EarlyDepthRequest','CyberpunkVR_EarlyDepthState','CyberpunkVR_EarlyDepthWholeTarget','CyberpunkVR_EarlyDepthRejectCommit','CyberpunkVR_GeometryPacketRequest',
                    'CyberpunkVR_GeometryPacketState','CyberpunkVR_GeometryPacketResolve','CyberpunkVR_NativeStereoProbeRequest',
                    'CyberpunkVR_NativeStereoProbeState','CyberpunkVR_NativeStereoAllDraws')
    try:
        if any(struct.unpack('<I', reader.read(int(symbols[n], 16), 4))[0] for n in ('g_menuModeValue', 'CyberpunkVR_CullReuseMode')):
            raise RuntimeError('Preparation capture requires gameplay and unmodified native culling')
        if read('State') == 1 or read('Request') or read('PairRequest') or read('PairState') == 1:
            raise RuntimeError('Preparation capture already active')
        if a.graph_plan and (read('GraphState')==1 or read('GraphRequest')):raise RuntimeError('Graph capture already active')
        if read('OwnedGraphMode') or read('GraphJobsMode') or read('DefinitionMode'):raise RuntimeError('Native owned-graph sorting/job lease is already active')
        if read_full('CyberpunkVR_OwnedExecutionMode') or read_full('CyberpunkVR_OwnedExecutionRequest'):
            raise RuntimeError('Owned reference execution is already armed')
        if a.owned_reference and read_full('CyberpunkVR_OwnedExecutionClosing'):
            raise RuntimeError('Owned reference execution is permanently closing')
        if a.owned_graph_sort and read('DefinitionCompatible')!=1:raise RuntimeError('Native definition observer is unavailable')
        if a.schedule_preparation and (read('ScheduleMode') or read('ScheduleCompatible')!=1):raise RuntimeError('CPU schedule probe unavailable or already active')
        if a.paired_cameras and (read('CameraTrace') or camera_read('State')==1 or camera_read('Request') or camera_read('NodeFilter')):
            raise RuntimeError('Camera trace already active')
        if a.early_depth_gpu:
            if a.skinned_alpha_depth and (early_read('RunRequest') or early_read('RunState')==1 or read('RunRequest') or read('RunState')==1):
                raise RuntimeError('Skinned alpha requires no active continuous preparation or depth run')
            if read('AllStages'):raise RuntimeError('All-stage preparation capture is already active')
            if early_read('CameraStages')!=1:raise RuntimeError('Camera stage control is not at its default')
            if early_read('SourceCapacity')!=MAX_SOURCES or early_read('InstanceCapacity')!=MAX_INSTANCE_ROWS:
                raise ValueError('Early depth publication capacity ABI mismatch')
            report['earlyDepthPreflight']={n:read_full(n) for n in (*early_controls,'CyberpunkVR_ConstantCaptureEnable','CyberpunkVR_StereoFamilyAssetStatus')}
            if (any(report['earlyDepthPreflight'][n] for n in early_controls) or read_full('CyberpunkVR_ConstantCaptureEnable') or
                    read_full('CyberpunkVR_StereoFamilyAssetStatus')!=2):
                raise RuntimeError('Early depth requires idle observers and a verified family package')
        if a.retained_geometry:
            if a.fixed_assets and ('CyberpunkVR_RetainedFixedAssetsEnable' not in symbols or not report['fixedAssetsAvailable']):
                raise RuntimeError('Fixed-assets controls or telemetry are unavailable')
            if 'CyberpunkVR_RetainedFixedAssetsEnable' in symbols and read_full('CyberpunkVR_RetainedFixedAssetsEnable'):
                raise RuntimeError('Fixed-assets observation is already armed')
            report['retainedPreflight']={n:retained_read(n) for n in ('Request','State','Stage','Verified')}
            if a.actual_membership and ('CyberpunkVR_PreparedActualMembershipEnable' not in symbols or not report['actualMembershipAvailable']):
                raise RuntimeError('Actual membership controls or telemetry are unavailable')
            if 'CyberpunkVR_PreparedActualMembershipEnable' in symbols and read_full('CyberpunkVR_PreparedActualMembershipEnable'):
                raise RuntimeError('Actual membership is already armed')
            if a.borrowed_tables and ('CyberpunkVR_RetainedBorrowedTablesEnable' not in symbols or not report['borrowedTablesAvailable']):
                raise RuntimeError('Borrowed-table controls or telemetry are unavailable')
            if 'CyberpunkVR_RetainedBorrowedTablesEnable' in symbols and read_full('CyberpunkVR_RetainedBorrowedTablesEnable'):
                raise RuntimeError('Borrowed-table observation is already armed')
            if a.full_t5 and ('CyberpunkVR_RetainedFullT5Enable' not in symbols or not report['fullT5Available']):
                raise RuntimeError('Full T5 controls or telemetry are unavailable')
            if 'CyberpunkVR_RetainedFullT5Enable' in symbols and read_full('CyberpunkVR_RetainedFullT5Enable'):
                raise RuntimeError('Full T5 snapshots are already armed')
            if a.native_export and ('CyberpunkVR_PreparedNativeExportEnable' not in symbols or not report['nativeExportAvailable']):
                raise RuntimeError('Native export controls or telemetry are unavailable')
            if 'CyberpunkVR_PreparedNativeExportEnable' in symbols and read_full('CyberpunkVR_PreparedNativeExportEnable'):
                raise RuntimeError('Prepared native output export is already armed')
            if read_full('CyberpunkVR_PreparedCurrentEnable'):
                raise RuntimeError('Current-frame prepared comparison is already armed')
            if read_full('CyberpunkVR_PreparedInfixEnable'):
                raise RuntimeError('Prepared infix insertion is already armed')
            if read_full('CyberpunkVR_PreparedTailEnable'):
                raise RuntimeError('Prepared native tail comparison is already armed')
            if read_full('CyberpunkVR_PreparedVrsRefinementEnable'):
                raise RuntimeError('Prepared VRS refinement policy is already armed')
            if read_full('CyberpunkVR_SubmissionFrontierEnable'):
                raise RuntimeError('Submission frontier observation is already enabled')
            if a.prepared_current and (read_full('CyberpunkVR_PreparedPairRequest') or read_full('CyberpunkVR_PreparedPairState') in (1,2,3,4)):
                raise RuntimeError('Prepared comparison is already active')
            active=any(read_full(n) for n in (*early_controls,'CyberpunkVR_ConstantCaptureEnable','CyberpunkVR_ConstantBankRequest',
                                            'CyberpunkVR_GeometryFreezeEnable','CyberpunkVR_StereoGpuProbeRequest',
                                            'CyberpunkVR_StereoGpuProbeGroup','CyberpunkVR_SinglePassTwinUpload'))
            active|=read_full('CyberpunkVR_StereoGpuProbeState') in (1,2) or bool(early_read('RunRequest') or read('RunRequest'))
            active|=early_read('RunState')==1 or read('RunState')==1
            if active or read('AllStages') or retained_read('Request') or retained_read('State') or retained_read('Stage')!=7:
                raise RuntimeError('Retained geometry requires idle capture controls and native bindings')
            if retained_read('Verified') not in (0,1) or (a.retained_geometry=='replace' and retained_read('Verified')!=1):
                raise RuntimeError('Retained replacement requires a completed reference in this process')
            if a.retained_source_stream and read_full('CyberpunkVR_RetainedSourceStreamEnable'):
                raise RuntimeError('Source-stream capture is already armed')
            if a.retained_ia_snapshots and read_full('CyberpunkVR_NativeIaSnapshotsEnable'):
                raise RuntimeError('Native IA capture is already armed')
            if a.retained_ia_snapshots and (read_full('CyberpunkVR_NativeInstanceObserverEnable') or
                                          read_full('CyberpunkVR_NativeInstanceObserverInstalled')!=7):
                raise RuntimeError('Instance observer requires all three actual native hooks and an idle control')
            if read_full('CyberpunkVR_RetainedIndirectSnapshotsEnable'):
                raise RuntimeError('Indirect-snapshot capture is already armed')
            if read_full('CyberpunkVR_RetainedSourceCbvEnable') or read_full('CyberpunkVR_SourceCbvBindingEnable'):
                raise RuntimeError('Source CBV capture is already armed')
            if a.retained_source_tables and read_full('CyberpunkVR_RetainedSourceTablesEnable'):
                raise RuntimeError('Source table capture is already armed')
            if a.retained_source_details and read_full('CyberpunkVR_RetainedSourceDetailsEnable'):
                raise RuntimeError('Source detail capture is already armed')
            if a.retained_cbv_snapshots and any(read_full(name) for name in ('CyberpunkVR_RetainedSourceCbvSnapshotsEnable','CyberpunkVR_SourceCbvSnapshotsEnable','CyberpunkVR_SourceCbvSnapshotsUploadEnable')):
                raise RuntimeError('Source CBV snapshot capture is already armed')
        if a.paired_gbuffer:
            controls=(*early_controls,'CyberpunkVR_ConstantCaptureEnable','CyberpunkVR_ConstantBankRequest',
                      'CyberpunkVR_GeometryFreezeEnable','CyberpunkVR_SinglePassTwinUpload',
                      'CyberpunkVR_RetainedGeometryRequest','CyberpunkVR_RetainedGeometryState',
                      'CyberpunkVR_StereoGpuProbeRequest','CyberpunkVR_StereoGpuProbeGroup')
            if any(read_full(n) for n in controls) or paired_read('Request') or paired_read('State') or paired_read('Ordinal') or paired_read('Family') or read('AllStages'):
                raise RuntimeError('Paired G-buffer requires idle native capture controls')
            if read_full('CyberpunkVR_StereoFamilyAssetStatus')!=2:
                raise RuntimeError('Paired G-buffer requires verified exact native shader families')
            if (read_full('CyberpunkVR_StereoGpuProbeState') in (1,2) or early_read('RunRequest') or read('RunRequest') or
                    early_read('RunState')==1 or read('RunState')==1 or early_read('CameraStages')!=1):
                raise RuntimeError('Paired G-buffer cannot share active GPU/continuous camera scopes')
        handle = k.OpenProcess(0x101028, False, a.pid)
        if not handle:
            raise c.WinError(c.get_last_error())
        armed = True
        if a.camera_turn:turn=HeadTurn(a.camera_turn)
        if a.paired_cameras:
            if camera_read('RecordBytes')!=4616:raise ValueError('Camera trace ABI mismatch')
            camera_write('NodeFilter',0x23A938);write('CameraTrace',1)
        if a.owned_graph_sort:
            write('GraphJobsEpoch',(read('GraphJobsEpoch')+1)&0xffffffff or 1)
            write('DefinitionMode',1);write('DefinitionRequest',1)
            write('OwnedGraphMode',3 if a.whole_gbuffer_plan else 2);write('GraphJobsMode',1)
        if a.owned_reference:
            if a.scene_access:
                from scene_access_report import CONTROLS,BLOCKS,PREFIX
                required=(*CONTROLS,PREFIX+'Seq',PREFIX+'Stats',*(PREFIX+suffix for _,suffix,_,_ in BLOCKS))
                if any(name not in symbols for name in required):raise RuntimeError('Scene access exports are unavailable')
                report['sceneAccessStartup']=arm_scene_access_controls(read_full,lambda name,value:write(name,value,''),claim_scene_access)
            if a.scene_split and 'CyberpunkVR_OwnedSceneSplitEnable' not in symbols:
                raise RuntimeError('Native scene split control is unavailable')
            if a.scene_split_eyes=='both' and 'CyberpunkVR_OwnedSceneSplitSections' not in symbols:
                raise RuntimeError('Two-eye native scene split section export is unavailable')
            if 'CyberpunkVR_OwnedSceneSplitEnable' in symbols:
                report['sceneSplitStartup']=read_full('CyberpunkVR_OwnedSceneSplitEnable')
                if report['sceneSplitStartup']:raise RuntimeError('Native scene split control was not initially idle')
                if a.scene_split:write('CyberpunkVR_OwnedSceneSplitEnable',2 if a.scene_split_eyes=='both' else 1,'')
            write('GraphJobsEpoch',(read('GraphJobsEpoch')+1)&0xffffffff or 1)
            if a.whole_gbuffer_plan:write('OwnedGraphMode',3);write('CyberpunkVR_OwnedResourceReplayEnable',1,'')
            write('CyberpunkVR_OwnedExecutionMode',1,'')
            write('CyberpunkVR_OwnedExecutionRequest',2 if a.owned_coupled else 1,'')
        if a.graph_plan:
            write('GraphState',0);write('GraphRequest',1)
        if a.schedule_preparation:
            schedule_before=struct.unpack('<16I',reader.read(address('ScheduleStats'),64))
            write('ScheduleMode',0 if a.owned_coupled else 1 if a.schedule_preparation=='check' else 2)
        if a.early_depth_gpu:
            if a.pass_targets:write('AllStages',1)
            write('CyberpunkVR_GeometryPacketResolve',2,'');write('CyberpunkVR_NativeStereoAllDraws',1,'')
            write('WholeTarget',3 if a.native_depth_timing else 2 if a.optimized_depth else int(a.whole_depth_target),'CyberpunkVR_EarlyDepth')
            write('RejectCommit',int(a.reject_depth_commit),'CyberpunkVR_EarlyDepth')
            write('NativeCamera',int(a.native_camera),'CyberpunkVR_EarlyDepth')
            write('CameraStages',a.native_camera_stages,'CyberpunkVR_EarlyDepth')
            write('Skinning',int(a.skinning_depth),'CyberpunkVR_EarlyDepth')
            write('AlphaReference',int(a.alpha_depth_reference),'CyberpunkVR_EarlyDepth')
            write('AlphaOptimized',int(a.alpha_depth_optimized),'CyberpunkVR_EarlyDepth')
            write('SkinnedAlpha',int(a.skinned_alpha_depth),'CyberpunkVR_EarlyDepth')
            write('MaterialCapture',int(a.material_depth),'CyberpunkVR_EarlyDepth')
            if a.material_hints:
                from material_readback import hints
                previous=json.loads(a.material_hints.read_text(encoding='utf-8'))
                if previous['pid']!=a.pid or previous['dll']!=report['dll']:raise ValueError('Texture hint process/build differs')
                entries=hints(previous['earlyDepthGpu']['materials']);raw_hints=b''.join(struct.pack('<2I',*entry) for entry in entries)
                buffer=c.create_string_buffer(raw_hints);done=c.c_size_t()
                if not k.WriteProcessMemory(handle,int(symbols['CyberpunkVR_EarlyDepthMaterialHints'],16),buffer,len(raw_hints),c.byref(done)) or done.value!=len(raw_hints):
                    raise c.WinError(c.get_last_error())
                write('MaterialHintCount',len(entries),'CyberpunkVR_EarlyDepth')
            write('Request',a.early_depth_gpu,'CyberpunkVR_EarlyDepth')
        if a.retained_geometry:
            if a.fixed_assets:write('CyberpunkVR_RetainedFixedAssetsEnable',1,'')
            if a.actual_membership:write('CyberpunkVR_PreparedActualMembershipEnable',1,'')
            if a.borrowed_tables:write('CyberpunkVR_RetainedBorrowedTablesEnable',1,'')
            if a.full_t5:write('CyberpunkVR_RetainedFullT5Enable',1,'')
            if a.native_export:write('CyberpunkVR_PreparedNativeExportEnable',1,'')
            if a.allow_vrs_refinement:write('CyberpunkVR_PreparedVrsRefinementEnable',1,'')
            if a.prepared_tail:write('CyberpunkVR_PreparedTailEnable',1,'')
            if a.prepared_infix:write('CyberpunkVR_PreparedInfixEnable',1,'')
            if a.prepared_current:write('CyberpunkVR_PreparedCurrentEnable',1,'')
            if a.retained_source_stream:write('CyberpunkVR_RetainedSourceStreamEnable',1,'')
            if a.retained_indirect_snapshots:write('CyberpunkVR_RetainedIndirectSnapshotsEnable',1,'')
            if a.retained_source_cbvs:
                write('CyberpunkVR_SourceCbvBindingEnable',1,'');write('CyberpunkVR_RetainedSourceCbvEnable',1,'')
            if a.retained_source_tables:write('CyberpunkVR_RetainedSourceTablesEnable',1,'')
            if a.retained_source_details:write('CyberpunkVR_RetainedSourceDetailsEnable',1,'')
            if a.retained_cbv_snapshots:
                write('CyberpunkVR_SourceCbvSnapshotsEnable',1,'');write('CyberpunkVR_RetainedSourceCbvSnapshotsEnable',1,'')
                if a.retained_direct_upload:write('CyberpunkVR_SourceCbvSnapshotsUploadEnable',1,'')
                if a.retained_source_selectors:write('CyberpunkVR_StructuredBufferSnapshotsEnable',1,'')
            if a.retained_ia_snapshots:
                write('CyberpunkVR_NativeInstanceObserverResetRequest',1,'')
                write('CyberpunkVR_NativeInstanceObserverEnable',1,'')
                write('CyberpunkVR_NativeIaSnapshotsEnable',1,'')
            write('AllStages',1)
            write('CyberpunkVR_GeometryPacketResolve',2,'');write('CyberpunkVR_NativeStereoAllDraws',1,'')
            write('Stage',a.retained_stage,'CyberpunkVR_RetainedGeometry')
            write('Request',RETAINED_MODES[a.retained_geometry],'CyberpunkVR_RetainedGeometry')
            # Present arms packet/target capture with the preparation request.
            # Starting that one-frame capture here would precede the trace.
            report['retainedArm']=wait_recorder_armed(retained_read,lambda:k.WaitForSingleObject(handle,0)!=0)
        if a.paired_gbuffer:
            write('AllStages',1)
            write('CyberpunkVR_GeometryPacketResolve',2,'');write('CyberpunkVR_NativeStereoAllDraws',1,'')
            write('CyberpunkVR_NativeStereoProbeRequest',1,'');write('CyberpunkVR_GeometryPacketRequest',1,'')
            write('Ordinal',a.paired_gbuffer_ordinal,'CyberpunkVR_PairedGeometry')
            write('Family',70 if a.paired_gbuffer_family==70 else 0,'CyberpunkVR_PairedGeometry')
            write('Request',1,'CyberpunkVR_PairedGeometry')
        if a.merge_root:
            if read('PairVerified')!=1 or read('RootCompatible')!=1:raise RuntimeError('Merged root requires a verified warmup and compatible root hook')
            write('RootSkipped',0)
            write('PairMergeLimit',a.merge_frames)
        if a.pair_prepare:
            write('PairState', 0)
            write('PairRequest',3 if a.merge_root else 2 if a.main_root_pair else 1)
        write('Request', a.frames)
        def preparation_poll():
            nonlocal turned
            if turn and not turned and camera_read('State')==1 and camera_read('Count'):
                report['cameraTurn']=turn.apply(1);turned=True
            if a.pair_prepare and read('PairState') in (2,3,4):
                report['pairPreparationProgress'] = dict(state=read('PairState'), data=list(struct.unpack('<8Q', reader.read(address('PairData'),64))))
        def preparation_slow(sample):
            report['preparationSlowObservation']=sample
            if a.retained_geometry:
                try:
                    import importlib.util
                    spec=importlib.util.spec_from_file_location('live_dump',Path(__file__).resolve().parents[1]/'crash/live_dump.py')
                    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
                    report['preparationLiveDump']=module.capture(a.pid,a.out.with_name(a.out.stem+'-pending.dmp'))
                except Exception as error:
                    report['preparationLiveDumpError']=str(error)
        from preparation_wait import wait as wait_preparation
        report['preparationWait']=wait_preparation(read,lambda:k.WaitForSingleObject(handle,0)!=0,
                                                  preparation_poll,preparation_slow)
        seq, count, length = read('Seq'), read('Count'), read('PacketCount')
        if seq & 1 or count > 8192 or length > 524288:
            raise ValueError('Preparation publication incomplete')
        blocks = {}
        for name, amount in (('Records', count*112), ('Packets', length*16)):
            pointer = struct.unpack('<Q', reader.read(address(name), 8))[0]
            blocks[name] = reader.read(pointer, amount) if amount else b''
            path = a.out.with_name(a.out.stem+'-'+name.lower()+'.bin')
            path.write_bytes(blocks[name])
            report[name.lower()+'File'] = path.name
        report['failure'] = read('Failure')
        if a.early_depth_gpu:
            deadline=time.monotonic()+15
            while early_read('State') in (0,1,2) and time.monotonic()<deadline:time.sleep(.02)
            state=early_read('State');raw=reader.read(int(symbols['CyberpunkVR_EarlyDepthComparison'],16),88)
            report['earlyDepthGpu']=dict(state=state,result=struct.unpack('<i',reader.read(int(symbols['CyberpunkVR_EarlyDepthResult'],16),4))[0],
                stats=dict(zip(EARLY_FIELDS,struct.unpack('<16I',reader.read(int(symbols['CyberpunkVR_EarlyDepthStats'],16),64)))),
                comparison=None,nativeDrawsReplaced=0)
            report['earlyDepthGpu']['origin']=dict(mode=read_full('CyberpunkVR_GeometryOriginMode'),
                compatible=read_full('CyberpunkVR_GeometryOriginCompatible'),verified=read_full('CyberpunkVR_GeometryOriginVerified'),
                stats=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_GeometryOriginStats'],16),32))))
            report['earlyDepthGpu']['gpuInstances']=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthGpuInstances'],16),32)))
            report['earlyDepthGpu']['nativeCamera']=dict(enabled=a.native_camera,
                stats=list(struct.unpack('<3Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthNativeCameraStats'],16),24))))
            report['earlyDepthGpu']['nativeCamera'].update(stages=a.native_camera_stages,
                stageMasks=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthCameraStageMasks'],16),32))))
            report['earlyDepthGpu']['skinning']=dict(enabled=a.skinning_depth,
                stats=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthSkinningStats'],16),48))),
                proofs=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthShaderProofs'],16),32))),
                bindingChecks=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthBindingChecks'],16),32))))
            report['earlyDepthGpu']['materialBindingStatuses']=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthMaterialBindings'],16),96)))
            report['earlyDepthGpu']['alphaReference']=dict(enabled=a.alpha_depth_reference,
                stats=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthAlphaStats'],16),48))))
            report['earlyDepthGpu']['alphaOptimized']=dict(enabled=a.alpha_depth_optimized,
                recordingDetails=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRecordingDetails'],16),64))),
                proofs=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthAlphaProofs'],16),48))),
                recording=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthRecording'],16),48))))
            report['earlyDepthGpu']['skinnedAlpha']=dict(enabled=a.skinned_alpha_depth,
                stats=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthSkinnedAlphaStats'],16),48))))
            from material_readback import decode as decode_material,validate as validate_material,ROW_BYTES
            if early_read('MaterialRowBytes')!=ROW_BYTES:raise ValueError('Material capture ABI mismatch')
            material_count=early_read('MaterialCount');material_bytes=early_read('MaterialBytes')
            if material_count>128 or material_bytes>128*292:raise ValueError('Material capture overflow')
            material_blocks=[]
            for name,size in (('MaterialRows',material_count*ROW_BYTES),('MaterialBlob',material_bytes)):
                pointer=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_EarlyDepth'+name],16),8))[0]
                block=reader.read(pointer,size) if size else b'';material_blocks.append(block)
                a.out.with_name(a.out.stem+'-'+name.lower()+'.bin').write_bytes(block)
            report['earlyDepthGpu']['materials']=decode_material(*material_blocks,a.material_depth)
            from material_readback import attach_descriptors,validate_textures
            descriptor_pointer=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthMaterialDescriptors'],16),8))[0]
            material_descriptors=reader.read(descriptor_pointer,material_count*480) if material_count else b''
            a.out.with_name(a.out.stem+'-materialdescriptors.bin').write_bytes(material_descriptors)
            attach_descriptors(report['earlyDepthGpu']['materials'],material_descriptors)
            source_count=report['earlyDepthGpu']['stats']['recorded']
            if source_count>MAX_SOURCES:raise ValueError('Invalid early GPU source count')
            report['earlyDepthGpu']['sources']=list(struct.iter_unpack('<2Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthSources'],16),source_count*16)))
            report['earlyDepthGpu']['coverage']=list(struct.iter_unpack('<2I',reader.read(int(symbols['CyberpunkVR_EarlyDepthCoverage'],16),source_count*8)))
            report['earlyDepthGpu']['masks']=list(struct.iter_unpack('<2I',reader.read(int(symbols['CyberpunkVR_EarlyDepthMasks'],16),source_count*8)))
            report['earlyDepthGpu']['maskStats']=dict(zip(MASK_FIELDS,struct.unpack('<14I',reader.read(int(symbols['CyberpunkVR_EarlyDepthMaskStats'],16),56))))
            report['earlyDepthGpu']['extraCoverage']=list(struct.iter_unpack('<2I',reader.read(int(symbols['CyberpunkVR_EarlyDepthExtraCoverage'],16),source_count*8)))
            report['earlyDepthGpu']['failureData']=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthFailureData'],16),48)))
            report['earlyDepthGpu']['cameraBindingInfo']=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthCameraBindingInfo'],16),128)))
            report['earlyDepthGpu']['descriptorFailure']=read_full('CyberpunkVR_DescriptorCaptureFailure')
            report['earlyDepthGpu']['descriptorStoreStats']=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_DescriptorStoreStats'],16),48)))
            report['earlyDepthGpu']['liveHeapSlots']=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_DescriptorHeapSlots'],16),8))[0]
            report['earlyDepthGpu']['nativeTargets']=dict(zip(NATIVE_FIELDS,struct.unpack('<8I',reader.read(int(symbols['CyberpunkVR_EarlyDepthNativeCounts'],16),32))))
            report['earlyDepthGpu']['nativeDifferences']=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthNativeDifferences'],16),32)))
            if a.optimized_depth or a.native_depth_timing:report['earlyDepthGpu']['nativeDifferences']=None
            report['earlyDepthGpu']['nativeTiming']=a.native_depth_timing
            report['earlyDepthGpu']['targetSize']=list(struct.unpack_from('<2I',raw))
            times=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthGpuTimes'],16),48)))
            report['earlyDepthGpu']['gpuTimes']=decode_gpu_times(times) if state==3 and (a.whole_depth_target or a.native_depth_timing) else None
            report['earlyDepthGpu']['optimized']=dict(zip(OPTIMIZED_FIELDS,struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_EarlyDepthOptimized'],16),64))))
            if a.optimized_depth and state==3:
                o=report['earlyDepthGpu']['optimized'];n=report['earlyDepthGpu']['nativeTargets']
                executed=o['mainCommands']+o['peerUnconditional']+(o['peerOptimizedRuns'] if o['gpuDecision']==1 else o['peerFallbackBatches'])
                saved=n['mainDraws']+n['peerDraws']-executed
                report['earlyDepthGpu'].update(netDrawCommandReduction=saved,nativeDrawsReplaced=max(0,saved))
            if state==3 and not a.optimized_depth and not a.native_depth_timing:
                peer_minimum=16 if (report['earlyDepthGpu']['maskStats']['both'] or report['earlyDepthGpu']['maskStats']['peerOnly']) else 0
                report['earlyDepthGpu']['comparison']=decode_depth_comparison(raw,minimum_written=(peer_minimum,16))
            native_raw,target_raw=capture_native_targets()
            instance_count=early_read('InstanceCount');instance_bytes=early_read('InstanceBytes')
            if instance_count>MAX_INSTANCE_ROWS or instance_bytes>8*1024*1024:raise ValueError('Invalid early instance publication')
            for name,size in (('InstanceRows',instance_count*136),('InstanceBlob',instance_bytes)):
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_EarlyDepth'+name],16),8))
                a.out.with_name(a.out.stem+'-early-'+name.lower()+'.bin').write_bytes(reader.read(pointer,size) if size else b'')
            a.out.with_name(a.out.stem+'-early-depth.bin').write_bytes(raw)
        if a.retained_geometry:
            deadline=time.monotonic()+15
            while time.monotonic()<deadline and (retained_read('State') in (0,1,2) or read_full('CyberpunkVR_NativeStereoProbeState')==1):time.sleep(.02)
            if a.prepared_current:
                report['preparedCurrentSourceDiagnostic']=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedCurrent'],16),128)))
                if a.prepared_tail:
                    capture_handoff()
                    capture_tail()
                if a.prepared_infix:capture_infix()
                capture_frontier()
            if a.scene_split:
                # Native retained Arm installs/resets the observer after idle preflight.
                # Preserve this finished batch before later report decoding or frames.
                capture_frontier()
                write('CyberpunkVR_SubmissionFrontierEnable',0,'')
                report['sceneSplitFrontierCleanup']=read_full('CyberpunkVR_SubmissionFrontierEnable')
                if report['sceneSplitFrontierCleanup']:raise RuntimeError('Scene split frontier observer was not stopped')
            if a.scene_access:capture_scene_access()
            if 'CyberpunkVR_RetainedT5OverlayError' in symbols:
                report['t5OverlayError']=reader.read(int(symbols['CyberpunkVR_RetainedT5OverlayError'],16),256).split(b'\0',1)[0].decode('ascii',errors='replace')
            guarded_raw=staged_raw=None
            if a.retained_source_selectors:
                guarded_stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedGuardedStats'],16),128)))
                guarded_count=read_full('CyberpunkVR_RetainedGuardedCount')
                if guarded_count>2048 or read_full('CyberpunkVR_RetainedGuardedRowBytes')!=128:raise ValueError('Guarded table row ABI differs')
                guarded_pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedGuardedRows'],16),8))
                guarded_raw=reader.read(guarded_pointer,guarded_count*128) if guarded_count else b''
                report['guardedTablesDiagnostic']=dict(stats=guarded_stats,rows=list(struct.iter_unpack('<16Q',guarded_raw)))
                guarded_cache=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_RetainedGuardedCacheStats'],16),32)))
                report['guardedTablesDiagnostic']['cacheStats']=guarded_cache
                a.out.with_name(a.out.stem+'-guarded-tables.bin').write_bytes(guarded_raw)
                if a.retained_ia_snapshots:
                    staged_stats=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedStats'],16),96)))
                    report['stagedResidency']=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedResidency'],16),96)))
                    report['stagedResidencyDetails']=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedResidencyDetails'],16),96)))
                    report['stagedMembershipDetails']=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedMembershipDetails'],16),96)))
                    staged_count=read_full('CyberpunkVR_RetainedStagedCount')
                    if staged_count>2048 or read_full('CyberpunkVR_RetainedStagedRowBytes')!=64:raise ValueError('Staged source row ABI differs')
                    staged_pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedRows'],16),8))
                    staged_raw=reader.read(staged_pointer,staged_count*64) if staged_count else b''
                    report['stagedSourcesDiagnostic']=dict(stats=staged_stats,rows=list(struct.iter_unpack('<8Q',staged_raw)))
                    a.out.with_name(a.out.stem+'-staged-sources.bin').write_bytes(staged_raw)
            if a.retained_ia_snapshots:
                source_baseline=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedBaseline'],16),128)))
                report['sourceBaselineDiagnostic']=source_baseline
                report['instanceGpuPhase']=[read_full('CyberpunkVR_RetainedInstanceGpuPhase'),read_full('CyberpunkVR_NativeInstanceGpuProbePhase')]
                report['instanceOverlayError']=reader.read(int(symbols['CyberpunkVR_RetainedInstanceOverlayError'],16),256).split(b'\0',1)[0].decode('ascii',errors='replace')
            arrays={name:list(struct.unpack(f'<{count}Q',reader.read(int(symbols['CyberpunkVR_RetainedGeometry'+name],16),count*8)))
                    for name,count in (('Stats',16),('Different',12),('Bytes',2),('Ends',4),('Indirect',2),('IndirectRejected',8))}
            report['retainedGeometry']=decode_retained(a.retained_geometry,retained_read('Stage'),retained_read('State'),retained_read('Verified'),
                                                      arrays['Stats'],arrays['Different'],arrays['Bytes'],arrays['Ends'],arrays['Indirect'],arrays['IndirectRejected'])
            if a.retained_cbv_snapshots:
                report['cbvPoolFailure']=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_SourceCbvSnapshotsPoolFailure'],16),128)))
            if a.retained_source_tables:
                count=read_full('CyberpunkVR_RetainedNativeTableCount')
                if count>2048 or read_full('CyberpunkVR_RetainedNativeTableRowBytes')!=128:raise ValueError('Native descriptor reservation ABI changed')
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedNativeTableRows'],16),8))
                native_tables=reader.read(pointer,count*128) if count else b''
                a.out.with_name(a.out.stem+'-native-tables.bin').write_bytes(native_tables)
                report['nativeTableDiagnostics']=[list(r) for r in struct.iter_unpack('<16Q',native_tables)][-4:]
                entry_count=read_full('CyberpunkVR_RetainedNativeTableEntryCount')
                if entry_count>131072 or read_full('CyberpunkVR_RetainedNativeTableEntryBytes')!=64:raise ValueError('Native descriptor entry ABI changed')
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedNativeTableEntries'],16),8))
                native_entries=reader.read(pointer,entry_count*64) if entry_count else b''
                a.out.with_name(a.out.stem+'-native-table-entries.bin').write_bytes(native_entries)
                ring_count=read_full('CyberpunkVR_RetainedNativeTableRingCount')
                if ring_count>2048 or read_full('CyberpunkVR_RetainedNativeTableRingBytes')!=128:raise ValueError('Native descriptor ring ABI changed')
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedNativeTableRings'],16),8))
                native_rings=reader.read(pointer,ring_count*128) if ring_count else b''
                a.out.with_name(a.out.stem+'-native-table-rings.bin').write_bytes(native_rings)
            if a.retained_cbv_snapshots and report['retainedGeometry']['state']==4:
                from failed_source_evidence import capture as capture_failed_source
                try:
                    report['failedSourceEvidence']=capture_failed_source(
                        lambda name,size:reader.read(int(symbols[name],16),size),reader.read,a.out)
                except Exception as error:
                    report['failedSourceEvidenceError']=str(error)
            if a.retained_source_stream:
                if a.retained_source_selectors:
                    report['t5FinalizeDiagnostic']=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_StructuredBufferFinalizeDiagnostic'],16),64)))
                from source_stream_report import ROW_BYTES,analyze as analyze_source_stream
                if read_full('CyberpunkVR_RetainedSourceStreamRowBytes')!=ROW_BYTES:raise ValueError('Source stream ABI changed')
                count=read_full('CyberpunkVR_RetainedSourceStreamCount')
                if count>2048:raise ValueError('Source stream capacity exceeded')
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceStreamRows'],16),8))
                block=reader.read(pointer,count*ROW_BYTES) if count else b''
                stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceStreamStats'],16),128)))
                a.out.with_name(a.out.stem+'-source-stream.bin').write_bytes(block)
                report['sourceStream']=dict(stats=stats,rows=list(struct.iter_unpack('<48Q',block)))
                report['sourceStream']=analyze_source_stream(stats,block,report['retainedGeometry'])
                if a.retained_ia_snapshots:
                    from staged_sources_report import analyze_baseline
                    report['sourceBaseline']=analyze_baseline(source_baseline,report['sourceStream'])
                if guarded_raw is not None:
                    from guarded_tables_report import analyze as analyze_guarded
                    report['guardedTables']=analyze_guarded(guarded_stats,guarded_raw,report['sourceStream'])
                if staged_raw is not None:
                    from staged_sources_report import analyze as analyze_staged
                    report['stagedSources']=analyze_staged(staged_stats,staged_raw,report['sourceStream'],report['guardedTables'])
                from indirect_binding_report import ROW_BYTES as INDIRECT_ROW_BYTES,analyze as analyze_indirect_bindings
                if read_full('CyberpunkVR_RetainedIndirectBindingsRowBytes')!=INDIRECT_ROW_BYTES:raise ValueError('Indirect binding ABI changed')
                count=read_full('CyberpunkVR_RetainedIndirectBindingsCount')
                if count>2048:raise ValueError('Indirect binding capacity exceeded')
                pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedIndirectBindingsRows'],16),8))
                block=reader.read(pointer,count*INDIRECT_ROW_BYTES) if count else b''
                stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedIndirectBindingsStats'],16),128)))
                a.out.with_name(a.out.stem+'-indirect-bindings.bin').write_bytes(block)
                report['indirectBindings']=dict(stats=stats,rows=list(struct.iter_unpack('<56Q',block)))
                report['indirectBindings']=analyze_indirect_bindings(stats,block,report['sourceStream'])
                if a.retained_source_cbvs:
                    from source_cbv_report import ROW_BYTES as CBV_ROW_BYTES,analyze as analyze_source_cbvs
                    if read_full('CyberpunkVR_RetainedSourceCbvRowBytes')!=CBV_ROW_BYTES:raise ValueError('Source CBV ABI changed')
                    count=read_full('CyberpunkVR_RetainedSourceCbvCount')
                    if count>32768:raise ValueError('Source CBV capacity exceeded')
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceCbvRows'],16),8))
                    block=reader.read(pointer,count*CBV_ROW_BYTES) if count else b''
                    stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceCbvStats'],16),128)))
                    a.out.with_name(a.out.stem+'-source-cbvs.bin').write_bytes(block)
                    report['sourceCbvs']=dict(stats=stats,rows=list(struct.iter_unpack('<24Q',block)))
                    if read_full('CyberpunkVR_RetainedSourceCbvDiagnosticBytes')!=128:raise ValueError('Source CBV diagnostic ABI changed')
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceCbvDiagnostics'],16),8))
                    diagnostics=reader.read(pointer,count*128) if count else b''
                    a.out.with_name(a.out.stem+'-source-cbv-diagnostics.bin').write_bytes(diagnostics)
                    report['sourceCbvDiagnostics']=list(struct.iter_unpack('<16Q',diagnostics))
                    report['sourceCbvs']=analyze_source_cbvs(stats,block,report['sourceStream'])
                    producer_count=read_full('CyberpunkVR_RetainedSourceCbvProducerCount')
                    if producer_count!=count or producer_count>2048*28 or read_full('CyberpunkVR_RetainedSourceCbvProducerRowBytes')!=320:
                        raise ValueError('Source CBV producer ABI/count differs')
                    producer_pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceCbvProducerRows'],16),8))
                    producer_raw=reader.read(producer_pointer,producer_count*320) if producer_count else b''
                    a.out.with_name(a.out.stem+'-cbv-producers.bin').write_bytes(producer_raw)
                    from source_cbv_producer_report import analyze as analyze_cbv_producers
                    report['sourceCbvProducers']=analyze_cbv_producers(producer_raw,report['sourceStream'],report['sourceCbvs'])
                if a.retained_source_details:
                    if read_full('CyberpunkVR_RetainedSourceDetailsRowBytes')!=1024:raise ValueError('Source detail ABI changed')
                    count=read_full('CyberpunkVR_RetainedSourceDetailsCount')
                    if count>32768:raise ValueError('Source detail capacity exceeded')
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceDetailsRows'],16),8))
                    block=reader.read(pointer,count*1024) if count else b''
                    stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceDetailsStats'],16),128)))
                    a.out.with_name(a.out.stem+'-source-details.bin').write_bytes(block)
                    report['sourceDetails']=dict(stats=stats,rows=list(struct.iter_unpack('<128Q',block)))
                    from source_details_report import analyze as analyze_source_details
                    report['sourceDetails']=analyze_source_details(stats,block,report['sourceStream'],report['sourceCbvs'])
                if a.retained_cbv_snapshots:
                    if read_full('CyberpunkVR_RetainedSourceCbvSnapshotsRowBytes')!=256 or read_full('CyberpunkVR_RetainedSourceCbvSnapshotsCommandBytes')!=192:
                        raise ValueError('Source CBV snapshot ABI changed')
                    count=read_full('CyberpunkVR_RetainedSourceCbvSnapshotsCount');commands=read_full('CyberpunkVR_RetainedSourceCbvSnapshotsCommandCount')
                    blob_bytes=read_full('CyberpunkVR_RetainedSourceCbvSnapshotsBlobBytes')
                    if count>32768 or commands>2048 or blob_bytes>128*1024*1024:raise ValueError('Source CBV snapshot capacity exceeded')
                    def snapshot_buffer(name,size):
                        pointer,=struct.unpack('<Q',reader.read(int(symbols[name],16),8))
                        return reader.read(pointer,size) if size else b''
                    block=snapshot_buffer('CyberpunkVR_RetainedSourceCbvSnapshotsRows',count*256)
                    command_block=snapshot_buffer('CyberpunkVR_RetainedSourceCbvSnapshotsCommands',commands*192)
                    blob=snapshot_buffer('CyberpunkVR_RetainedSourceCbvSnapshotsBlob',blob_bytes)
                    stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceCbvSnapshotsStats'],16),128)))
                    a.out.with_name(a.out.stem+'-cbv-snapshots.bin').write_bytes(block)
                    a.out.with_name(a.out.stem+'-cbv-snapshot-commands.bin').write_bytes(command_block)
                    a.out.with_name(a.out.stem+'-cbv-snapshot-payloads.bin').write_bytes(blob)
                    report['cbvSnapshots']=dict(stats=stats,rows=list(struct.iter_unpack('<32Q',block)),commands=list(struct.iter_unpack('<24Q',command_block)),blobBytes=len(blob))
                    from source_cbv_snapshot_report import analyze as analyze_cbv_snapshots
                    report['cbvSnapshots']=analyze_cbv_snapshots(stats,block,command_block,report['sourceStream'],report['sourceCbvs'],report['sourceDetails'],blob,report['sourceCbvProducers'])
                    if a.retained_source_selectors:
                        selector_stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceSelectorsStats'],16),128)))
                        selector_blocks=[]
                        for name,words,limit in (('Command',32,2048),('Input',24,4096),('Slot',16,65536),('T5',32,2048)):
                            if read_full(f'CyberpunkVR_RetainedSourceSelectors{name}Bytes')!=words*8:raise ValueError('Source selector ABI changed')
                            count=read_full(f'CyberpunkVR_RetainedSourceSelectors{name}Count')
                            if count>limit:raise ValueError('Source selector capacity exceeded')
                            suffix={'Command':'Commands','Input':'Inputs','Slot':'Slots','T5':'T5'}[name]
                            data=snapshot_buffer(f'CyberpunkVR_RetainedSourceSelectors{suffix}',count*words*8);selector_blocks.append(data)
                            a.out.with_name(a.out.stem+f'-selector-{name.lower()}.bin').write_bytes(data)
                        selector_bytes=read_full('CyberpunkVR_RetainedSourceSelectorsBlobBytes')
                        if selector_bytes>1024*1024:raise ValueError('Source selector payload capacity exceeded')
                        selector_blob=snapshot_buffer('CyberpunkVR_RetainedSourceSelectorsBlob',selector_bytes)
                        a.out.with_name(a.out.stem+'-selector-payloads.bin').write_bytes(selector_blob)
                        from source_selector_capture_report import analyze as analyze_source_selectors
                        t5_buffers={}
                        for suffix,words in (('Native',96),('Snapshot',32)):
                            if read_full(f'CyberpunkVR_RetainedT5{suffix}RowBytes')!=words*8:raise ValueError('t5 ABI changed')
                            count=read_full(f'CyberpunkVR_RetainedT5{suffix}Count')
                            if count>2048:raise ValueError('t5 capacity exceeded')
                            t5_buffers[suffix]=snapshot_buffer(f'CyberpunkVR_RetainedT5{suffix}Rows',count*words*8)
                            a.out.with_name(a.out.stem+f'-t5-{suffix.lower()}.bin').write_bytes(t5_buffers[suffix])
                        for suffix in ('Snapshot','Intent'):
                            count=read_full(f'CyberpunkVR_RetainedT5{suffix}BlobBytes')
                            if count>8*1024*1024:raise ValueError('t5 blob capacity exceeded')
                            t5_buffers[suffix+'Blob']=snapshot_buffer(f'CyberpunkVR_RetainedT5{suffix}Blob',count)
                            a.out.with_name(a.out.stem+f'-t5-{suffix.lower()}-payloads.bin').write_bytes(t5_buffers[suffix+'Blob'])
                        diagnostic_count=read_full('CyberpunkVR_RetainedT5DiagnosticCount')
                        if diagnostic_count>2048 or read_full('CyberpunkVR_RetainedT5DiagnosticRowBytes')!=256:raise ValueError('t5 diagnostic ABI changed')
                        report['t5Diagnostics']={}
                        for suffix in ('Copy','Mark'):
                            t5_diagnostic=snapshot_buffer(f'CyberpunkVR_RetainedT5{suffix}DiagnosticRows',diagnostic_count*256)
                            a.out.with_name(a.out.stem+f'-t5-{suffix.lower()}-diagnostic.bin').write_bytes(t5_diagnostic)
                            report['t5Diagnostics'][suffix]=list(struct.iter_unpack('<32Q',t5_diagnostic))
                        from t5_snapshot_report import analyze as analyze_t5
                        piece_count=read_full('CyberpunkVR_RetainedT5PieceCount')
                        if piece_count>65536 or read_full('CyberpunkVR_RetainedT5PieceRowBytes')!=128:raise ValueError('t5 piece ABI changed')
                        t5_pieces=snapshot_buffer('CyberpunkVR_RetainedT5PieceRows',piece_count*128)
                        a.out.with_name(a.out.stem+'-t5-pieces.bin').write_bytes(t5_pieces)
                        report['t5Snapshots']=analyze_t5(t5_buffers['Native'],t5_buffers['Snapshot'],t5_buffers['SnapshotBlob'],t5_buffers['IntentBlob'],report['sourceStream'],selector_blocks[3],t5_pieces)
                        coverage_count=read_full('CyberpunkVR_RetainedT5CoverageCount')
                        if coverage_count>2048 or read_full('CyberpunkVR_RetainedT5CoverageRowBytes')!=160:
                            raise ValueError('t5 coverage ABI changed')
                        coverage_raw=snapshot_buffer('CyberpunkVR_RetainedT5CoverageRows',coverage_count*160)
                        a.out.with_name(a.out.stem+'-t5-coverage.bin').write_bytes(coverage_raw)
                        from t5_snapshot_report import analyze_coverage
                        report['t5Coverage']=analyze_coverage(coverage_raw,selector_blocks[3],report['sourceStream'])
                        report['sourceSelectors']=analyze_source_selectors(selector_stats,*selector_blocks,selector_blob,report['sourceStream'],report['cbvSnapshots'],blob,report['t5Snapshots'])
                        coverage_count=read_full('CyberpunkVR_RetainedSelectorCoverageCount')
                        if coverage_count>4096 or read_full('CyberpunkVR_RetainedSelectorCoverageRowBytes')!=224:
                            raise ValueError('Selector coverage ABI changed')
                        generic_raw=snapshot_buffer('CyberpunkVR_RetainedSelectorCoverageRows',coverage_count*224)
                        a.out.with_name(a.out.stem+'-selector-coverage.bin').write_bytes(generic_raw)
                        from selector_coverage_report import analyze as analyze_selector_coverage
                        report['selectorCoverage']=analyze_selector_coverage(generic_raw,report['sourceStream'],report['sourceCbvs'],report['sourceSelectors'])
                        event_count=read_full('CyberpunkVR_RetainedSelectorCacheEventCount')
                        if event_count>4096 or read_full('CyberpunkVR_RetainedSelectorCacheEventRowBytes')!=288:
                            raise ValueError('Selector cache-event ABI changed')
                        event_raw=snapshot_buffer('CyberpunkVR_RetainedSelectorCacheEventRows',event_count*288)
                        a.out.with_name(a.out.stem+'-selector-cache-events.bin').write_bytes(event_raw)
                        from selector_cache_event_report import analyze as analyze_selector_cache_events
                        report['selectorCacheEvents']=analyze_selector_cache_events(event_raw,report['sourceStream'],report['sourceCbvs'],report['sourceSelectors'])
                if a.retained_source_tables:
                    if read_full('CyberpunkVR_RetainedSourceTablesRowBytes')!=128 or read_full('CyberpunkVR_RetainedSourceTablesCommandBytes')!=192:
                        raise ValueError('Source table ABI changed')
                    count=read_full('CyberpunkVR_RetainedSourceTablesCount');commands=read_full('CyberpunkVR_RetainedSourceTablesCommandCount')
                    if count>65536 or commands>2048:raise ValueError('Source table capacity exceeded')
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceTablesRows'],16),8))
                    block=reader.read(pointer,count*128) if count else b''
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceTablesCommands'],16),8))
                    command_block=reader.read(pointer,commands*192) if commands else b''
                    stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceTablesStats'],16),128)))
                    a.out.with_name(a.out.stem+'-source-tables.bin').write_bytes(block)
                    a.out.with_name(a.out.stem+'-source-table-commands.bin').write_bytes(command_block)
                    report['sourceTables']=dict(stats=stats,rows=list(struct.iter_unpack('<16Q',block)),commands=list(struct.iter_unpack('<24Q',command_block)))
                    from source_tables_report import analyze as analyze_source_tables
                    report['sourceTables']=analyze_source_tables(stats,command_block,block,report['sourceStream'],report['sourceCbvs'])
                    from native_descriptor_report import analyze as analyze_native_descriptors
                    report['nativeTables']=analyze_native_descriptors(native_tables,report['sourceStream'],report['sourceTables'],native_entries,native_rings)
                if a.retained_source_selectors:
                    assembly_blocks=[]
                    for name,suffix,words,limit in (('Command','Commands',32,2048),('Table','Tables',16,65536),('MissingCbv','MissingCbvs',16,65536)):
                        prefix='CyberpunkVR_RetainedSourceAssembly'
                        count=read_full(prefix+name+'Count')
                        if count>limit or read_full(prefix+name+'Bytes')!=words*8:
                            raise ValueError('Source assembly ABI/capacity changed')
                        data=snapshot_buffer(prefix+suffix,count*words*8)
                        assembly_blocks.append(data)
                        a.out.with_name(a.out.stem+f'-assembly-{name.lower()}.bin').write_bytes(data)
                    assembly_stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceAssemblyStats'],16),128)))
                    from source_assembly_report import analyze as analyze_source_assembly
                    report['sourceAssembly']=analyze_source_assembly(assembly_stats,*assembly_blocks,
                        report['sourceStream'],report['cbvSnapshots'],report['sourceSelectors'])
                    bank_stats=list(struct.unpack('<6Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceAssemblyBankStats'],16),48)))
                    if bank_stats[0] not in (0,1) or bank_stats[1] not in (0,1) or bank_stats[3]>1000000 or bank_stats[5]:
                        raise ValueError('Source assembly bank bounds/execution differ')
                    if bank_stats[1]:
                        if not bank_stats[0] or bank_stats[4] or bank_stats[2]!=assembly_stats[9] or bank_stats[2]!=assembly_stats[10]:
                            raise ValueError('Created GPU bank differs from assembled source set')
                    elif bank_stats[2] or bank_stats[3] or assembly_stats[10] or (bank_stats[0] and not bank_stats[4]&0x80000000):
                        raise ValueError('Uncreated GPU bank acquired descriptors/draws')
                    report['sourceAssembly']['bankStats']=bank_stats
                    bank_failure=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceAssemblyBankFailure'],16),32)))
                    if bank_stats[1] and (bank_failure[0] or bank_failure[3]):
                        raise ValueError('Created GPU bank retains a failed validation stage')
                    if not bank_stats[0] and any(bank_failure):
                        raise ValueError('Unattempted GPU bank acquired failure diagnostics')
                    report['sourceAssembly']['bankFailure']=bank_failure
                    cache_stats=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceAssemblyCacheStats'],16),32)))
                    if guarded_raw is not None:
                        from guarded_tables_report import validate_cache
                        validate_cache(guarded_cache,cache_stats,report['guardedTables']['rows'],report['sourceAssembly']['commands'])
                        report['guardedTables']['cacheStats']=guarded_cache
                    elif cache_stats[0]>1000000 or cache_stats[1]!=cache_stats[3] or cache_stats[2]+cache_stats[3]!=sum(row[28] for row in report['sourceAssembly']['commands'] if row[21]):
                        raise ValueError('Source assembly immutable-cache totals differ')
                    report['sourceAssembly']['cpuTableCache']=dict(zip(('uniqueDescriptors','uniqueTables','hits','misses'),cache_stats))
                    owner_count=read_full('CyberpunkVR_RetainedSourceCbvCopyOwnerCount')
                    if owner_count>2048 or read_full('CyberpunkVR_RetainedSourceCbvCopyOwnerRowBytes')!=64:
                        raise ValueError('Source CBV copy-owner ABI changed')
                    copy_owners=snapshot_buffer('CyberpunkVR_RetainedSourceCbvCopyOwnerRows',owner_count*64)
                    a.out.with_name(a.out.stem+'-source-cbv-copy-owners.bin').write_bytes(copy_owners)
                    overlay_count=read_full('CyberpunkVR_RetainedT5OverlayCount')
                    if overlay_count>2048 or read_full('CyberpunkVR_RetainedT5OverlayRowBytes')!=256:
                        raise ValueError('Private t5 overlay ABI changed')
                    overlay_rows=snapshot_buffer('CyberpunkVR_RetainedT5OverlayRows',overlay_count*256)
                    overlay_size=read_full('CyberpunkVR_RetainedT5OverlayBlobBytes')
                    if overlay_size>8*1024*1024:raise ValueError('Private t5 overlay payload capacity exceeded')
                    overlay_blob=snapshot_buffer('CyberpunkVR_RetainedT5OverlayBlob',overlay_size)
                    overlay_stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedT5OverlayStats'],16),128)))
                    a.out.with_name(a.out.stem+'-t5-overlay.bin').write_bytes(overlay_rows)
                    a.out.with_name(a.out.stem+'-t5-overlay-payloads.bin').write_bytes(overlay_blob)
                    from t5_overlay_report import analyze as analyze_t5_overlay
                    report['t5Overlay']=analyze_t5_overlay(overlay_stats,overlay_rows,overlay_blob,report['sourceStream'],
                        report['cbvSnapshots'],blob,report['t5Snapshots'],t5_buffers['SnapshotBlob'],report['sourceAssembly'],copy_owners)
                if a.retained_ia_snapshots:
                    ia_blocks=[]
                    for suffix,words,limit in (('Command',16,2048),('Observation',67,2048*17),('View',16,2048*17)):
                        count=read_full(f'CyberpunkVR_RetainedIa{suffix}Count')
                        if count>limit or read_full(f'CyberpunkVR_RetainedIa{suffix}Bytes')!=words*8:
                            raise ValueError('Native IA snapshot ABI/capacity changed')
                        data=snapshot_buffer(f'CyberpunkVR_RetainedIa{suffix}s',count*words*8)
                        a.out.with_name(a.out.stem+f'-ia-{suffix.lower()}.bin').write_bytes(data);ia_blocks.append(data)
                    from ia_snapshot_report import analyze as analyze_ia_snapshots
                    selection_count=read_full('CyberpunkVR_RetainedIaSelectionCount')
                    if selection_count>2048 or read_full('CyberpunkVR_RetainedIaSelectionBytes')!=40:raise ValueError('IA selection ABI changed')
                    selections=snapshot_buffer('CyberpunkVR_RetainedIaSelections',selection_count*40)
                    a.out.with_name(a.out.stem+'-ia-selections.bin').write_bytes(selections)
                    report['iaSnapshots']=analyze_ia_snapshots(*ia_blocks,report['sourceStream'],selections)
                    from ia_snapshot_report import analyze_reuse
                    reuse_count=read_full('CyberpunkVR_RetainedIaReuseCount');transfer_count=read_full('CyberpunkVR_RetainedIaTransferCount')
                    if reuse_count>2048*17 or transfer_count>2048:raise ValueError('IA reuse bound differs')
                    reuse=snapshot_buffer('CyberpunkVR_RetainedIaReuseRows',reuse_count*64)
                    transfers=snapshot_buffer('CyberpunkVR_RetainedIaTransferRows',transfer_count*40)
                    a.out.with_name(a.out.stem+'-ia-reuse.bin').write_bytes(reuse)
                    a.out.with_name(a.out.stem+'-ia-transfers.bin').write_bytes(transfers)
                    report['iaReuse']=analyze_reuse(reuse,transfers,report['iaSnapshots'],report['sourceStream'])
                    instance_count=read_full('CyberpunkVR_RetainedInstanceCount')
                    instance_bytes=read_full('CyberpunkVR_RetainedInstanceBlobBytes')
                    if instance_count>2048 or instance_bytes>16*1024*1024 or read_full('CyberpunkVR_RetainedInstanceRowBytes')!=384:
                        raise ValueError('Native instance writer ABI/capacity changed')
                    instance_rows=snapshot_buffer('CyberpunkVR_RetainedInstanceRows',instance_count*384)
                    instance_blob=snapshot_buffer('CyberpunkVR_RetainedInstanceBlob',instance_bytes)
                    a.out.with_name(a.out.stem+'-instance-writers.bin').write_bytes(instance_rows)
                    a.out.with_name(a.out.stem+'-instance-writer-payloads.bin').write_bytes(instance_blob)
                    from instance_writer_report import analyze as analyze_instance_writers
                    report['instanceWriters']=analyze_instance_writers(instance_rows,instance_blob,report['sourceStream'])
                    report['instanceWriters']['firstObserverLoss']=read_full('CyberpunkVR_NativeInstanceObserverFirstLoss')
                    report['instanceWriters']['diagnostics']=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_NativeInstanceObserverDiagnostics'],16),192)))
                    gpu_count=read_full('CyberpunkVR_RetainedInstanceGpuCount');gpu_size=read_full('CyberpunkVR_RetainedInstanceGpuBlobBytes')
                    if gpu_count!=instance_count or gpu_size>16*1024*1024 or read_full('CyberpunkVR_RetainedInstanceGpuRowBytes')!=192:
                        raise ValueError('Instance GPU probe ABI or extent changed')
                    gpu_rows=snapshot_buffer('CyberpunkVR_RetainedInstanceGpuRows',gpu_count*192)
                    gpu_blob=snapshot_buffer('CyberpunkVR_RetainedInstanceGpuBlob',gpu_size)
                    a.out.with_name(a.out.stem+'-instance-gpu.bin').write_bytes(gpu_rows)
                    a.out.with_name(a.out.stem+'-instance-gpu-payloads.bin').write_bytes(gpu_blob)
                    from instance_gpu_report import analyze as analyze_instance_gpu
                    report['instanceGpu']=analyze_instance_gpu(gpu_rows,gpu_blob,instance_blob,report['instanceWriters'],report['sourceStream'])
                    overlay_count=read_full('CyberpunkVR_RetainedInstanceOverlayCount');overlay_size=read_full('CyberpunkVR_RetainedInstanceOverlayBlobBytes')
                    if overlay_count>2048 or overlay_size>16*1024*1024 or read_full('CyberpunkVR_RetainedInstanceOverlayRowBytes')!=192:
                        raise ValueError('Instance overlay ABI or extent changed')
                    overlay_stats=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_RetainedInstanceOverlayStats'],16),64)))
                    overlay_rows=snapshot_buffer('CyberpunkVR_RetainedInstanceOverlayRows',overlay_count*192)
                    overlay_blob=snapshot_buffer('CyberpunkVR_RetainedInstanceOverlayBlob',overlay_size)
                    a.out.with_name(a.out.stem+'-instance-overlay.bin').write_bytes(overlay_rows)
                    a.out.with_name(a.out.stem+'-instance-overlay-payloads.bin').write_bytes(overlay_blob)
                    from instance_overlay_report import analyze as analyze_instance_overlay
                    report['instanceOverlay']=analyze_instance_overlay(overlay_stats,overlay_rows,overlay_blob,gpu_blob,report['instanceGpu'],report['sourceStream'],report['instanceWriters'],
                        mode=read_full('CyberpunkVR_RetainedInstanceOverlayMode'))
                    private_count=read_full('CyberpunkVR_RetainedPrivateGeometryCount')
                    if private_count!=instance_count or read_full('CyberpunkVR_RetainedPrivateGeometryRowBytes')!=128:
                        raise ValueError('Private geometry ABI/occurrences differ')
                    private_rows=snapshot_buffer('CyberpunkVR_RetainedPrivateGeometryRows',private_count*128)
                    a.out.with_name(a.out.stem+'-private-geometry.bin').write_bytes(private_rows)
                    from private_geometry_report import analyze as analyze_private_geometry
                    report['privateGeometry']=analyze_private_geometry(private_rows,report['sourceStream'],report['iaSnapshots'],report['instanceGpu'],report['instanceOverlay'])
                count=read_full('CyberpunkVR_RetainedSourceBoundaryCount')
                if count!=2:raise ValueError('Both source CPU pass boundaries were not observed')
                boundary=reader.read(int(symbols['CyberpunkVR_RetainedSourceBoundaryRows'],16),count*160)
                a.out.with_name(a.out.stem+'-source-boundaries.bin').write_bytes(boundary)
                from source_boundary_report import analyze as analyze_boundaries
                report['sourceBoundaries']=analyze_boundaries(boundary,report['sourceStream'],
                    report.get('iaSnapshots'),report.get('instanceOverlay'),report.get('instanceGpu'),require_cbvs=a.retained_cbv_snapshots)
                from source_boundary_report import submission as analyze_submission
                queue_order=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedSourceQueueOrder'],16),128)))
                report['sourceSubmissionOrder']=analyze_submission(queue_order,report['sourceBoundaries'],required=a.owned_coupled)
                if a.retained_source_selectors:
                    material_count=read_full('CyberpunkVR_RetainedMaterialCount')
                    source_count=read_full('CyberpunkVR_RetainedMaterialSourceCount')
                    if material_count>65536 or source_count>2048 or read_full('CyberpunkVR_RetainedMaterialRowBytes')!=192 or read_full('CyberpunkVR_RetainedMaterialSourceBytes')!=40:
                        raise ValueError('Material resource ABI or bounds differ')
                    material_raw=snapshot_buffer('CyberpunkVR_RetainedMaterialRows',material_count*192)
                    material_sources=snapshot_buffer('CyberpunkVR_RetainedMaterialSources',source_count*40)
                    a.out.with_name(a.out.stem+'-material-resources.bin').write_bytes(material_raw)
                    a.out.with_name(a.out.stem+'-material-sources.bin').write_bytes(material_sources)
                    from material_resource_report import analyze as analyze_material_resources
                    buffer_count=read_full('CyberpunkVR_RetainedMaterialBufferCount')
                    if buffer_count>4096 or read_full('CyberpunkVR_RetainedMaterialBufferRowBytes')!=320:
                        raise ValueError('Material buffer nomination ABI or bounds differ')
                    buffer_raw=snapshot_buffer('CyberpunkVR_RetainedMaterialBufferRows',buffer_count*320)
                    a.out.with_name(a.out.stem+'-material-buffers.bin').write_bytes(buffer_raw)
                    from material_resource_report import analyze_buffers,analyze_buffer_copies
                    report['materialBuffers']=analyze_buffers(buffer_raw,report['sourceStream'],material_raw)
                    copy_count=read_full('CyberpunkVR_RetainedMaterialBufferCopyCount')
                    if copy_count>4096 or read_full('CyberpunkVR_RetainedMaterialBufferCopyRowBytes')!=128:
                        raise ValueError('Material buffer copy ABI or bounds differ')
                    copy_raw=snapshot_buffer('CyberpunkVR_RetainedMaterialBufferCopyRows',copy_count*128)
                    a.out.with_name(a.out.stem+'-material-buffer-copies.bin').write_bytes(copy_raw)
                    report['materialBufferCopies']=analyze_buffer_copies(copy_raw,report['materialBuffers'])
                    report['materialResources']=analyze_material_resources(material_raw,material_sources,report['sourceStream'],report['sourceAssembly'],report['t5Overlay'],report['materialBufferCopies'])
                if a.retained_indirect_snapshots:
                    from indirect_snapshot_report import ROW_BYTES as SNAPSHOT_ROW_BYTES,analyze as analyze_indirect_snapshots
                    if read_full('CyberpunkVR_RetainedIndirectSnapshotsRowBytes')!=SNAPSHOT_ROW_BYTES:raise ValueError('Indirect snapshot ABI changed')
                    count=read_full('CyberpunkVR_RetainedIndirectSnapshotsCount')
                    if count>2048:raise ValueError('Indirect snapshot capacity exceeded')
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_RetainedIndirectSnapshotsRows'],16),8))
                    block=reader.read(pointer,count*SNAPSHOT_ROW_BYTES) if count else b''
                    stats=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedIndirectSnapshotsStats'],16),128)))
                    a.out.with_name(a.out.stem+'-indirect-snapshots.bin').write_bytes(block)
                    report['indirectSnapshots']=dict(stats=stats,rows=list(struct.iter_unpack('<24Q',block)))
                    report['indirectSnapshots']=analyze_indirect_snapshots(stats,block,report['sourceStream'],report['indirectBindings'],report['retainedGeometry'])
            if staged_raw is not None and retained_read('State')==3:
                completed_staged_stats=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_RetainedStagedStats'],16),96)))
                if completed_staged_stats[:10]!=staged_stats[:10]:raise ValueError('Staged source construction changed during readback')
                report['stagedSources']=analyze_staged(completed_staged_stats,staged_raw,report['sourceStream'],report['guardedTables'],complete=True)
            if a.prepared_current:
                from read_prepared_pair import validate_current_source
                capture_export()
                current_source=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedCurrent'],16),128)))
                packets=report['stagedSources']['ready']
                if a.prepared_tail:
                    report['preparedTail']=validate_prepared_tail(capture_tail(),report['sourceStream'],report['stagedSources'])
                    packets=report['preparedTail']['sourceCount']
                capture_full_t5(verify=True)
                capture_actual_membership(verify=True)
                capture_borrowed_tables(verify=True)
                capture_fixed_assets(verify=True)
                report['preparedCurrentSource']=validate_current_source(current_source,report['sourceStream']['stats'][1],packets)
                if a.prepared_infix:
                    report['preparedInfix']=validate_prepared_infix(capture_infix(),report['sourceStream']['stats'][1],
                        report['preparedCurrentSource'],packets)
                    if report['submissionFrontier'].get('boundaryFailure',0):
                        raise ValueError('Prepared infix submission frontier rejected the native boundary')
            if read_full('CyberpunkVR_NativeStereoProbeState')!=2:raise RuntimeError('Retained native draw capture did not finish')
            native_raw,target_raw=capture_native_targets()
            from shader_snapshot_readback import export as export_shaders
            report['nativeShaderSources']=export_shaders(reader,symbols,a.out.with_name(a.out.stem+'-native-shaders'))
        if a.paired_gbuffer:
            deadline=time.monotonic()+15
            while time.monotonic()<deadline and (paired_read('State') in (0,1,2) or read_full('CyberpunkVR_NativeStereoProbeState')==1):time.sleep(.02)
            arrays={name:list(struct.unpack(f'<{count}Q',reader.read(int(symbols['CyberpunkVR_PairedGeometry'+name],16),count*8)))
                    for name,count in (('Stats',16),('Different',12),('Bytes',2),('CameraDiagnostics',16),('Changed',12))}
            report['pairedGeometry']=decode_paired_geometry(paired_read('State'),arrays['Stats'],arrays['Different'],arrays['Bytes'],arrays['CameraDiagnostics'],arrays['Changed'])
            report['pairedGeometry']['family']=a.paired_gbuffer_family
            native_raw,target_raw=capture_native_targets()
        if a.schedule_preparation and not a.owned_coupled:
            schedule_seq=read('ScheduleSeq');stats=struct.unpack('<16I',reader.read(address('ScheduleStats'),64))
            ranges=struct.unpack('<4Q',reader.read(address('ScheduleRanges'),32))
            edge_pointer,=struct.unpack('<Q',reader.read(address('ScheduleEdges'),8))
            if schedule_seq&1 or stats[7]>65536:raise ValueError('Invalid CPU schedule publication')
            cpu_edges=list(struct.iter_unpack('<2I',reader.read(edge_pointer,stats[7]*8))) if stats[7] else []
            if schedule_seq!=read('ScheduleSeq'):raise ValueError('CPU schedule publication changed')
            report['cpuSchedule']=dict(mode=a.schedule_preparation,stats=list(stats),before=list(schedule_before),
                attempts=stats[0]-schedule_before[0],planned=stats[1]-schedule_before[1],applied=stats[2]-schedule_before[2],
                failures=stats[3]-schedule_before[3],failure=stats[4],ranges=[hex(v) for v in ranges],edges=cpu_edges)
        rows = decode(blocks['Records'], blocks['Packets'],a.frames,a.pass_targets)
        if a.graph_plan:
            report['graphCapture']=dict(state=read('GraphState'),failure=read('GraphFailure'),nodes=read('GraphNodeCount'),edges=read('GraphEdgeCount'))
            if read('GraphState')!=2 or read('GraphFailure'):raise RuntimeError(f"Native graph snapshot failed: {report['graphCapture']}")
            graph_seq=read('GraphSeq');node_count=read('GraphNodeCount');edge_count=read('GraphEdgeCount')
            if graph_seq&1 or node_count>8192 or edge_count>65536:raise ValueError('Invalid graph publication')
            graph_blocks={}
            for name,amount in (('Nodes',node_count*104),('Edges',edge_count*32)):
                pointer,=struct.unpack('<Q',reader.read(address('Graph'+name),8))
                graph_blocks[name]=reader.read(pointer,amount) if amount else b''
                a.out.with_name(a.out.stem+'-graph-'+name.lower()+'.bin').write_bytes(graph_blocks[name])
            meta=struct.unpack('<4Q',reader.read(address('GraphMeta'),32))
            if graph_seq!=read('GraphSeq'):raise ValueError('Graph publication changed')
            graph=decode_graph(graph_blocks['Nodes'],graph_blocks['Edges'],meta[1]);graph['meta']=list(meta)
            graph_path=a.out.with_name(a.out.stem+'-graph.json');graph_path.write_text(json.dumps(graph,indent=2)+'\n')
            report['graphPlan']=dict(file=graph_path.name,nodes=node_count,edges=edge_count,
                nodeKinds=graph['summary']['nodeKinds'],views=graph['summary']['views'],edgePhases=graph['summary']['edgePhases'],
                invalidEndpointEdges=graph['summary']['invalidEndpointEdges'],cullingNodes=len(graph['summary']['culling']))
            if a.pass_targets:
                from graph_parent_report import ROW_BYTES,analyze as analyze_parents,fingerprint as graph_fingerprint,frontier as decode_frontier
                if read('GraphParentBytes')!=ROW_BYTES:raise ValueError('Graph parent publication ABI mismatch')
                parent_count=read('GraphParentCount');parent_seq=read('Seq')
                if parent_seq&1 or parent_count>8192:raise ValueError('Invalid graph-parent publication')
                pointer,=struct.unpack('<Q',reader.read(address('GraphParents'),8))
                parents=reader.read(pointer,parent_count*ROW_BYTES) if parent_count else b''
                if parent_seq!=read('Seq'):raise ValueError('Graph-parent publication changed')
                a.out.with_name(a.out.stem+'-graph-parents.bin').write_bytes(parents)
                source_hash=graph_fingerprint(graph_blocks['Nodes'],graph_blocks['Edges'],meta[1])
                report['graphParents']=analyze_parents(parents,graph,rows,source_hash)
                frontier_rows={name:list(struct.unpack(f'<{amount}Q',reader.read(address('Frontier'+name),amount*8)))
                      for name,amount in (('Stats',16),('Roles',22),('Cuts',6),('Adds',6))}
                if not a.owned_coupled:
                    report['pairedFrontier']=decode_frontier(frontier_rows['Stats'],frontier_rows['Roles'],frontier_rows['Cuts'],frontier_rows['Adds'],source_hash)
                if a.whole_gbuffer_plan:
                    from whole_gbuffer_report import analyze as analyze_whole_gbuffer,shared_owners
                    values=list(struct.unpack('<16Q',reader.read(address('OwnedGraphStats'),128)))
                    if values[0:2]!=[3,3] or values[14] or values[15]!=source_hash or values[4:7]!=[1,1,1] or values[2]!=node_count or values[9]+values[10]>65536:
                        raise RuntimeError('Native whole-discard sorting failed')
                    owned_blocks={}
                    for name,size in (('Nodes',values[2]*104),('Edges',(values[9]+values[10])*32)):
                        pointer,=struct.unpack('<Q',reader.read(address('OwnedGraph'+name),8))
                        owned_blocks[name]=reader.read(pointer,size)
                        a.out.with_name(a.out.stem+'-owned-graph-'+name.lower()+'.bin').write_bytes(owned_blocks[name])
                    full=list(struct.unpack('<16Q',reader.read(address('WholePassStats'),128)))
                    context=list(struct.unpack('<24Q',reader.read(address('ContextOrderStats'),192)))
                    count=read('ContextOrderWitnessCount')
                    if read('ContextOrderWitnessBytes')!=24 or count>4096:raise ValueError('Native context-prefix witness ABI/count changed')
                    pointer,=struct.unpack('<Q',reader.read(address('ContextOrderWitnesses'),8))
                    witnesses=reader.read(pointer,count*24) if count else b''
                    a.out.with_name(a.out.stem+'-context-witnesses.bin').write_bytes(witnesses)
                    report['wholeGBufferPlan']=analyze_whole_gbuffer(full,context,witnesses,graph_blocks['Nodes'],graph_blocks['Edges'],owned_blocks['Nodes'],owned_blocks['Edges'],meta[1])
                    owner_stats=list(struct.unpack('<8Q',reader.read(address('SharedOwnerStats'),64)))
                    if read('SharedOwnerRowBytes')!=32 or owner_stats[6]>32768:raise ValueError('Shared native object order ABI/count changed')
                    pointer,=struct.unpack('<Q',reader.read(address('SharedOwnerRows'),8))
                    owner_rows=reader.read(pointer,owner_stats[6]*32) if owner_stats[6] else b''
                    a.out.with_name(a.out.stem+'-shared-owner-order.bin').write_bytes(owner_rows)
                    report['wholeGBufferPlan']['sharedObjects']=shared_owners(owner_stats,owner_rows,graph_blocks['Nodes'],graph_blocks['Edges'],owned_blocks['Nodes'],owned_blocks['Edges'],meta[1])
                if a.owned_graph_sort:
                    values=list(struct.unpack('<16Q',reader.read(address('OwnedGraphStats'),128)))
                    report['ownedGraphSort']=dict(stats=values,executionRedirected=False,definitionsRetained=False)
                    if (values[0]!=3 or values[1]!=2 or values[14] or values[15]!=source_hash or values[4:7]!=[1,1,1]
                            or values[2]!=node_count or values[3]!=edge_count or values[9]+values[10]>65536):
                        raise RuntimeError('Native owned graph sort/validation failed')
                    for name,size in (('Nodes',values[2]*104),('Edges',(values[9]+values[10])*32)):
                        pointer,=struct.unpack('<Q',reader.read(address('OwnedGraph'+name),8))
                        raw=reader.read(pointer,size)
                        a.out.with_name(a.out.stem+'-owned-graph-'+name.lower()+'.bin').write_bytes(raw)
                    deadline=time.monotonic()+10
                    while time.monotonic()<deadline:
                        job_values=list(struct.unpack('<8Q',reader.read(address('GraphJobsStats'),64)))
                        if job_values[0] in (3,4):break
                        time.sleep(.02)
                    compatibility=list(struct.unpack('<8Q',reader.read(address('GraphJobsCompatibility'),64)))
                    report['graphJobLease']=dict(stats=job_values,compatibility=compatibility,epoch=read('GraphJobsEpoch'),definitionOwnershipProved=False)
                    pending_count=read('GraphJobsPendingCount')
                    if pending_count>4:raise ValueError('Native graph pending receipts exceed their bounded capacity')
                    report['graphJobLease']['selection']=list(struct.unpack('<8Q',reader.read(address('GraphJobsSelection'),64)))
                    report['graphJobLease']['sourceReject']=list(struct.unpack('<8Q',reader.read(address('GraphJobsSourceReject'),64)))
                    report['graphJobLease']['pending']=[list(row) for row in struct.iter_unpack('<8Q',reader.read(address('GraphJobsPending'),pending_count*64))]
                    report['definitionGenerations']=dict(stats=list(struct.unpack('<16Q',reader.read(address('GenerationStats'),128))),
                        ticket=list(struct.unpack('<4Q',reader.read(address('GenerationTicket'),32))),
                        reject=list(struct.unpack('<8Q',reader.read(address('GenerationReject'),64))),rejectPhase=read('GenerationRejectPhase'),coverageLost=read('GenerationCoverageLost'))
                    definition_values=list(struct.unpack('<16Q',reader.read(address('DefinitionStats'),128)))
                    definition_ticket=list(struct.unpack('<8Q',reader.read(address('DefinitionTicket'),64)))
                    report['definitionSource']=dict(stats=definition_values,ticket=definition_ticket,definitionOwnershipProved=False)
                    resource_values=list(struct.unpack('<12Q',reader.read(address('ResourceUsesStats'),96)))
                    report['resourceUses']=dict(stats=resource_values,markerContract=read('ResourceMarkerContract'),readWriteAccessProved=False)
                    if resource_values[0]!=3 or resource_values[3]!=source_hash or resource_values[8]!=72 or resource_values[9]!=1 or resource_values[10] or resource_values[11]!=1:
                        raise RuntimeError('Native pre-compiler resource input capture was not verified')
                    if resource_values[4]>4096 or resource_values[5]>4096 or resource_values[6]>64 or resource_values[7]>24:
                        raise ValueError('Native resource inventory exceeds its bounded ABI')
                    for name,size in (('ResourceUsesRows',resource_values[5]*72),('ResourceUseIds',resource_values[4]*8),
                                      ('ResourceAuxKeys',resource_values[6]*4),('ResourceBoundaryIds',resource_values[7]*8)):
                        pointer,=struct.unpack('<Q',reader.read(address(name),8))
                        raw=reader.read(pointer,size) if size else b''
                        a.out.with_name(a.out.stem+'-'+name.lower()+'.bin').write_bytes(raw)
                    if graph_seq!=read('GraphSeq') or read('GraphState')!=2:
                        raise ValueError('Owned graph/resource publication changed')
                    if job_values[0]!=3 or job_values[1]!=node_count or job_values[5]!=1 or job_values[6] or job_values[7]!=source_hash:
                        raise RuntimeError('Complete native graph job lease was not verified')
                    definition_seq=read('DefinitionSeq')
                    if definition_values!=list(struct.unpack('<16Q',reader.read(address('DefinitionStats'),128))):
                        raise ValueError('Definition source stats changed before binary capture')
                    if definition_seq&1 or definition_values[1]>128 or definition_values[2]>8192 or definition_values[3]>8192:
                        raise ValueError('Invalid bounded native definition source publication')
                    definition_blocks={}
                    for name,size in (('Owners',definition_values[1]*64),('Mappings',definition_values[2]*24),('Entries',definition_values[3]*8)):
                        pointer,=struct.unpack('<Q',reader.read(address('Definition'+name),8))
                        raw=reader.read(pointer,size) if size else b'';definition_blocks[name]=raw
                        a.out.with_name(a.out.stem+'-definition-'+name.lower()+'.bin').write_bytes(raw)
                    if definition_seq!=read('DefinitionSeq'):raise ValueError('Native definition source publication changed')
                    from definition_source_report import validate as validate_definitions
                    report['definitionSource']['mapping']=validate_definitions(definition_values,definition_blocks['Owners'],definition_blocks['Mappings'],definition_blocks['Entries'],
                        graph_blocks['Nodes'],graph_blocks['Edges'],meta[1],job_values,read('GraphJobsEpoch'),definition_ticket)
                    tree=list(struct.unpack('<12Q',reader.read(address('DefinitionTreeStats'),96)))
                    report['definitionTree']=dict(stats=tree,destructorSafetyProved=False,nativeOwnershipProved=False)
                    if tree[0]!=2 or tree[1:4]!=[definition_ticket[4],definition_ticket[5],definition_ticket[0]]:
                        raise RuntimeError('Native definition child tree was not captured for this consumer')
                    if tree[4]!=definition_values[3] or tree[4]>8192 or tree[5]>4096 or tree[6]>8192 or tree[7]>4096 or tree[8]!=2 or tree[9] or tree[10]>4096:
                        raise ValueError('Native definition child tree count/ABI mismatch')
                    tree_blocks={}
                    for name,size in (('Roots',tree[4]*16),('Nodes',tree[5]*64),('Handles',tree[6]*48),('Arrays',tree[7]*56),('Allocators',tree[8]*32),('Strings',tree[10]*128)):
                        pointer,=struct.unpack('<Q',reader.read(address('DefinitionTree'+name),8))
                        raw=reader.read(pointer,size) if size else b''
                        tree_blocks[name]=raw
                        a.out.with_name(a.out.stem+'-definition-tree-'+name.lower()+'.bin').write_bytes(raw)
                    if definition_seq!=read('DefinitionSeq'):raise ValueError('Native definition child publication changed')
                    from definition_tree_report import decode as decode_tree
                    report['definitionTree']['inventory']=decode_tree(tree,tree_blocks,meta[1],definition_blocks['Entries'])
                    deadline=time.monotonic()+10
                    while True:
                        aggregate=list(struct.unpack('<8Q',reader.read(address('DefinitionAggregate'),64)))
                        report['definitionSource']['aggregate']=aggregate
                        report['definitionSource']['nativeConsumers']=list(struct.unpack('<10Q',reader.read(address('NativeConsumers'),80)))
                        report['definitionSource']['consumerReject']=list(struct.unpack('<8Q',reader.read(address('NativeConsumerReject'),64)))
                        if aggregate[0]==definition_ticket[0] and aggregate[2:4]==[1,1]:break
                        if time.monotonic()>=deadline:raise RuntimeError('Native execution aggregate did not complete')
                        time.sleep(.02)
                    if aggregate[4:8]!=definition_ticket[4:6]+definition_ticket[2:4] or not aggregate[1] or aggregate[1]==definition_ticket[1]:
                        raise RuntimeError('Native execution aggregate differs from the pinned consumer')
                    declaration=list(struct.unpack('<8Q',reader.read(address('DefinitionDeclaration'),64)))
                    report['definitionSource']['declaration']=declaration
                    if declaration[0]!=definition_ticket[0] or not declaration[1] or declaration[2:7]!=[1,1,1,1,0] or declaration[7]!=definition_ticket[2]:
                        raise RuntimeError('Native declaration/producer completion was not verified')
                    consumer_values=list(struct.unpack('<10Q',reader.read(address('NativeConsumers'),80)))
                    report['definitionSource']['aggregate']=aggregate
                    report['definitionSource']['nativeConsumers']=consumer_values
                    if consumer_values[5] or consumer_values[6]:raise RuntimeError('Native consumer registry has unsupported or overflowed consumers')
                    generation_seq=read('GenerationSeq')
                    generation_stats=list(struct.unpack('<16Q',reader.read(address('GenerationStats'),128)))
                    generation_ticket=list(struct.unpack('<4Q',reader.read(address('GenerationTicket'),32)))
                    report['definitionGenerations']=dict(stats=generation_stats,ticket=generation_ticket,
                        reject=list(struct.unpack('<8Q',reader.read(address('GenerationReject'),64))),rejectPhase=read('GenerationRejectPhase'),coverageLost=read('GenerationCoverageLost'))
                    generation_count=read('GenerationCount')
                    if generation_seq&1 or generation_count>128:raise ValueError('Invalid bounded generation publication')
                    generation_rows=reader.read(address('GenerationRows'),generation_count*96) if generation_count else b''
                    if generation_seq!=read('GenerationSeq'):raise ValueError('Native generation publication changed')
                    generation_source=dict(report['definitionSource']['mapping'],pairGeneration=definition_ticket[5])
                    from definition_generation_report import validate as validate_generations
                    if report['definitionGenerations']['coverageLost']:raise RuntimeError('Native root admission coverage was lost')
                    report['definitionGenerations']['binding']=validate_generations(generation_stats,generation_ticket,generation_rows,generation_source)
                    a.out.with_name(a.out.stem+'-definition-generations.bin').write_bytes(generation_rows)
        if a.pair_prepare:
            pair_state = read('PairState')
            values = struct.unpack('<8Q', reader.read(address('PairData'), 64))
            report['pairPreparation'] = dict(state=pair_state, compatible=read('PairCompatible'),
                eligibility=list(struct.unpack('<8Q', reader.read(address('PairEligibility'),64))),
                root=list(struct.unpack('<8Q',reader.read(address('PairRoot'),64))),mainRoot=a.main_root_pair,
                merged=a.merge_root,rootsSkipped=read('RootSkipped'),rootsMerged=read('PairMerged'),rootsCompleted=read('PairCompleted'),
                **dict(zip(('vrcamName','mainName','activeViews','vrcamView','mainView','vrcamStorage','mainStorage','distinctStorage'),values)))
            count=read('RootObservedCount')
            if count>16:raise ValueError('Invalid root observation count')
            raw=reader.read(address('RootObserved'),count*64) if count else b''
            report['rootObserved']=[list(r) for r in struct.iter_unpack('<8Q',raw)]
        if seq != read('Seq'):
            raise RuntimeError('Preparation publication changed')
        if a.paired_cameras:
            trace_seq=camera_read('Seq');trace_count=camera_read('Count')
            if camera_read('State')!=2 or trace_seq&1 or trace_count>512:raise ValueError('Incomplete camera trace')
            trace_raw=reader.read(int(symbols['CyberpunkVR_SinglePassTraceRecords'],16),trace_count*4616)
            if trace_seq!=camera_read('Seq'):raise ValueError('Camera trace changed')
            camera_path=a.out.with_name(a.out.stem+'-cameras.bin');camera_path.write_bytes(trace_raw)
            views=[report['pairPreparation']['mainView'],report['pairPreparation']['vrcamView']]
            report['pairedCameras']=compare_cameras(decode_cameras(trace_raw),rows,a.merge_frames,views,bool(a.camera_turn))
            report['pairedCameras']['file']=camera_path.name
        frequency = c.c_int64()
        k.QueryPerformanceFrequency(c.byref(frequency))
        report.update(qpcFrequency=frequency.value, records=rows, recordHash=hashlib.sha256(blocks['Records']).hexdigest(),
                      summary=analyze(rows, blocks['Packets'], frequency.value))
        report['summary']['nativeWorkChanged'] = a.pair_prepare and report['pairPreparation']['state'] in (2,3)
        if report['failure']:
            raise RuntimeError('Preparation collector failed')
        for frame in range(1, a.frames+1):
            if any(not any(r['frame'] == frame and r['kind'] == 6 and r['side'] == eye and r['count'] for r in rows) for eye in (0, 1)):
                raise RuntimeError('Missing geometry consumption for a requested eye/frame')
        if any(r['status'] for r in rows if r['side']>=0 or r['kind'] in (7,8)) or not report['summary']['completedStages']:
            raise RuntimeError('Invalid/incomplete native preparation data')
        depth = [r for r in report['summary']['consumes'] if r['stage'] == 3 and r['count']]
        if (not a.pair_prepare and len(depth) != a.frames*2) or any(len(r['matchingStageEnds']) != 1 for r in depth):
            raise RuntimeError('Depth-pass consumption did not match one complete prepared list per eye/frame')
        if a.pair_prepare:
            if report['pairPreparation']['state']==4 and not a.main_root_pair:
                report['admissionRejected']='Outer request does not own MAIN output bindings; original request preserved'
            elif report['pairPreparation']['state'] != 2 or not report['pairPreparation']['distinctStorage']:
                raise RuntimeError('Native paired graph preparation did not produce two independent views')
        if a.merge_root:
            skips=[r for r in rows if r['kind']==11]
            complete=[r for r in rows if r['kind']==17]
            if (any(report['pairPreparation'][name]!=a.merge_frames for name in ('rootsSkipped','rootsMerged','rootsCompleted'))
                    or len(skips)!=a.merge_frames or len(depth)!=a.frames*2 or len(complete)!=a.merge_frames
                    or any(not r['flags0'] for r in complete)
                    or [r['frame'] for r in complete]!=[r['frame'] for r in skips]
                    or any(sorted(r['side'] for r in depth if r['frame']==frame)!=[0,1] for frame in range(1,a.frames+1))
                    or [r['frame'] for r in skips]!=list(range(1,a.merge_frames+1))):
                raise RuntimeError('Merged roots did not replace the requested consecutive renders with preserved output ownership')
            report['eyeCopyOrder']=[eye_copy_order(rows,r['frame'],report['pairPreparation']['vrcamView']) for r in skips]
            if not all(check['passed'] for check in report['eyeCopyOrder']):
                raise RuntimeError('Merged VRCAM depth-to-copy GPU list order is not established')
        if a.schedule_preparation and not a.owned_coupled:
            schedule=report['cpuSchedule'];expected_apply=a.merge_frames if a.schedule_preparation=='apply' else 0
            if schedule['attempts']!=a.merge_frames or schedule['planned']!=a.merge_frames or schedule['applied']!=expected_apply or schedule['failures']:
                raise RuntimeError(f"CPU preparation schedule rejected: { {k:v for k,v in schedule.items() if k!='edges'} }")
            schedule['ordering']=[]
            for frame in range(1,a.merge_frames+1):
                stages=[r['sequence'] for r in rows if r['frame']==frame and r['side']==0 and r['kind']==1 and r['stage']==3]
                ends=[r['sequence'] for r in rows if r['frame']==frame and r['side']==1 and r['kind']==19]
                early=bool(stages and ends) and max(ends)<min(stages)
                schedule['ordering'].append(dict(frame=frame,mainDepth=stages,eyeMaterialized=ends,early=early))
                if a.schedule_preparation=='apply' and not early:raise RuntimeError('VRCAM packets were not produced before MAIN depth preparation')
            proxy=report['summary']['proxyPreparation']
            if not proxy['typed'] or proxy['incomplete'] or any(not any(f['frame']==frame and all(f['ranges']) and all(f['calls'])
                    for f in proxy['frames']) for frame in range(1,a.merge_frames+1)):
                raise RuntimeError('Missing or incomplete native proxy preparation for a paired frame')
        if a.paired_cameras and not report['pairedCameras']['passed']:
            cameras=report['pairedCameras']
            if cameras['motionRequired'] and not cameras['movingFrames'] and all(f['passed'] for f in cameras['frames']):
                raise RuntimeError('Camera constants match, but head motion was not observed within the bounded paired frames')
            raise RuntimeError('Early peer camera does not match the native depth camera')
        if a.native_depth_timing:validate_native_timing(report['earlyDepthGpu'])
        elif a.early_depth_gpu:validate_early(report['earlyDepthGpu'],a.early_depth_gpu,a.whole_depth_target,a.optimized_depth,a.reject_depth_commit)
        if a.early_depth_gpu:validate_material(report['earlyDepthGpu']['materials'])
        if a.material_hints and a.material_depth:validate_textures(report['earlyDepthGpu']['materials'])
        if a.retained_geometry:validate_retained(report['retainedGeometry'],a.retained_geometry,a.retained_stage)
        if a.paired_gbuffer:validate_paired_geometry(report['pairedGeometry'])
        if a.pass_targets:
            from pass_target_order import analyze as analyze_targets
            report['passTargets']=analyze_targets(report,native_raw,target_raw)
            if report['passTargets']['depthScopes']<2 or {g['side'] for g in report['passTargets']['gbufferConsumers']}!={0,1}:
                raise RuntimeError('Target capture did not include complete depth and G-buffer scopes for both eyes')
        if a.owned_reference:
            from owned_execution_report import decode as decode_owned_execution,decode_compiler
            expected_epoch=read('GraphJobsEpoch');expected_pair=struct.unpack('<Q',reader.read(address('PairGeneration'),8))[0]
            report['ownedExecution']=wait_owned_execution(read_full,
                lambda name,count:struct.unpack(f'<{count}Q',reader.read(int(symbols[name],16),count*8)),expected_epoch,expected_pair,
                on_snapshot=lambda snapshot:report.update(ownedExecution=snapshot))
            if report['ownedExecution']['closing']:raise RuntimeError('Owned execution entered sticky closing')
            report['ownedExecution']['proof']=decode_owned_execution(report['ownedExecution']['stats'],report['ownedExecution']['resources'],
                expected_epoch=expected_epoch,expected_pair=expected_pair)
            report['ownedExecution']['proof']['compiler']=decode_compiler(report['ownedExecution']['compiler'],
                report['ownedExecution']['stats'],report['ownedExecution']['resources'])
            schedule=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_OwnedExecutionSchedule'],16),128)))
            report['ownedExecution']['scheduleRaw']=schedule
            if schedule[4]>220 or schedule[5]>65536:raise ValueError('Owned schedule source exceeds bounds')
            def schedule_buffer(name,size):
                pointer,=struct.unpack('<Q',reader.read(int(symbols[name],16),8))
                return reader.read(pointer,size) if size else b''
            schedule_nodes=schedule_buffer('CyberpunkVR_OwnedExecutionScheduleNodes',schedule[4]*104)
            schedule_edges=schedule_buffer('CyberpunkVR_OwnedExecutionScheduleEdges',schedule[5]*32)
            schedule_bindings=schedule_buffer('CyberpunkVR_OwnedExecutionScheduleBindings',schedule[4]*104)
            a.out.with_name(a.out.stem+'-schedule-source-nodes.bin').write_bytes(schedule_nodes)
            a.out.with_name(a.out.stem+'-schedule-source-edges.bin').write_bytes(schedule_edges)
            a.out.with_name(a.out.stem+'-schedule-source-bindings.bin').write_bytes(schedule_bindings)
            if a.scene_split:
                split_seq=read_full('CyberpunkVR_OwnedExecutionSeq')
                split_stats=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_OwnedSceneSplitStats'],16),192)))
                report['ownedExecution']['sceneSplit']=dict(stats=split_stats,verified=False)
                split_count=2 if a.scene_split_eyes=='both' else 1
                if (split_seq&1 or split_stats[0]!=split_count or split_stats[5]!=split_count
                        or not 0<split_stats[6]<=220-split_count or split_stats[7]>65536-2*split_count
                        or split_stats[8]!=split_stats[6]+split_count or split_stats[9]!=split_stats[7]+2*split_count):
                    raise ValueError('Native scene split graph exceeds bounded publication')
                split_nodes=schedule_buffer('CyberpunkVR_OwnedSceneSplitSourceNodes',split_stats[6]*104)
                split_edges=schedule_buffer('CyberpunkVR_OwnedSceneSplitSourceEdges',split_stats[7]*32)
                split_mapping=schedule_buffer('CyberpunkVR_OwnedSceneSplitMappingRows',split_stats[8]*12)
                split_sections=reader.read(int(symbols['CyberpunkVR_OwnedSceneSplitSections'],16),96) if split_count==2 else None
                if split_seq!=read_full('CyberpunkVR_OwnedExecutionSeq'):
                    raise ValueError('Native scene split publication changed during capture')
                for suffix,raw in (('nodes',split_nodes),('edges',split_edges),('mappings',split_mapping)):
                    a.out.with_name(a.out.stem+'-scene-split-'+suffix+'.bin').write_bytes(raw)
                if split_sections is not None:
                    a.out.with_name(a.out.stem+'-scene-split-sections.bin').write_bytes(split_sections)
                from scene_split_report import decode as decode_scene_split
                report['ownedExecution']['sceneSplit']=decode_scene_split(split_stats,split_nodes,split_edges,split_mapping,
                    schedule=schedule,owned_stats=report['ownedExecution']['stats'],resources=report['ownedExecution']['resources'],
                    source_nodes=schedule_nodes,source_edges=schedule_edges,bindings=schedule_bindings,
                    execution_nodes=graph_blocks['Nodes'],execution_edges=graph_blocks['Edges'],image_base=meta[1],sections_raw=split_sections)
                report['ownedExecution']['schedule']=report['ownedExecution']['sceneSplit']
            else:
                from owned_execution_report import decode_schedule
                report['ownedExecution']['schedule']=decode_schedule(schedule,report['ownedExecution']['stats'],report['ownedExecution']['resources'],
                    schedule_nodes,schedule_edges,graph_blocks['Nodes'],graph_blocks['Edges'],meta[1],bindings=schedule_bindings,coupled=a.owned_coupled)
            if 'CyberpunkVR_OwnedStageStats' in symbols:
                stage_seq=read_full('CyberpunkVR_OwnedExecutionSeq')
                stage_stats=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_OwnedStageStats'],16),64)))
                report['ownedExecution']['nativeStages']=dict(stats=stage_stats,verified=False)
                if stage_seq&1 or stage_stats[5]>220 or stage_stats[6]>8192:
                    raise ValueError('Native stage observation exceeds its bounded publication')
                stage_definitions=schedule_buffer('CyberpunkVR_OwnedStageDefinitions',stage_stats[5]*128)
                stage_entries=schedule_buffer('CyberpunkVR_OwnedStageEntries',stage_stats[6]*48)
                if stage_seq!=read_full('CyberpunkVR_OwnedExecutionSeq'):
                    raise ValueError('Native stage observation changed during capture')
                a.out.with_name(a.out.stem+'-native-stage-definitions.bin').write_bytes(stage_definitions)
                a.out.with_name(a.out.stem+'-native-stage-entries.bin').write_bytes(stage_entries)
                from native_stage_report import decode as decode_native_stages
                report['ownedExecution']['nativeStages'].update(decode_native_stages(stage_stats,stage_definitions,stage_entries,
                    schedule_nodes,report['ownedExecution']['stats'],meta[1]),verified=True)
            if 'CyberpunkVR_OwnedSceneStats' in symbols:
                scene_seq=read_full('CyberpunkVR_OwnedExecutionSeq')
                scene_stats=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_OwnedSceneStats'],16),64)))
                report['ownedExecution']['sceneSections']=dict(stats=scene_stats,verified=False)
                if scene_seq&1 or scene_stats[5]>2 or scene_stats[6]>22:
                    raise ValueError('Native scene sections exceed their bounded publication')
                scene_roots=schedule_buffer('CyberpunkVR_OwnedSceneRoots',scene_stats[5]*112)
                scene_children=schedule_buffer('CyberpunkVR_OwnedSceneChildren',scene_stats[6]*80)
                if scene_seq!=read_full('CyberpunkVR_OwnedExecutionSeq'):
                    raise ValueError('Native scene sections changed during capture')
                a.out.with_name(a.out.stem+'-scene-roots.bin').write_bytes(scene_roots)
                a.out.with_name(a.out.stem+'-scene-children.bin').write_bytes(scene_children)
                from scene_sections_report import decode as decode_scene_sections
                report['ownedExecution']['sceneSections'].update(decode_scene_sections(scene_stats,scene_roots,scene_children,
                    schedule_nodes,report['ownedExecution']['stats'],meta[1],report['ownedExecution'].get('nativeStages')),verified=True)
            if a.scene_split:
                from scene_split_report import validate_publication as validate_scene_publication
                report['ownedExecution']['sceneSplit']['publication']=validate_scene_publication(
                    report,frontier=report.get('submissionFrontier'),require_frontier=True)
                if a.scene_access:
                    from scene_access_report import validate as validate_scene_access
                    report['sceneAccess']=validate_scene_access(report['sceneAccess'],report['ownedExecution']['sceneSplit']['publication'],
                        owned_stats=report['ownedExecution']['stats'])
            report['ownedExecution']['proof']['mode']='scene-split' if a.scene_split else 'coupled' if a.owned_coupled else 'reference'
            admission=report['ownedExecution']['admission'];owned_stats=report['ownedExecution']['stats']
            if admission[:4]!=[owned_stats[1],owned_stats[2],owned_stats[3],owned_stats[4]] or not admission[4] or not admission[5]:
                raise RuntimeError('Owned admission lost its native source cache binding')
            retirement=report['ownedExecution']['retirement']
            if retirement[0] not in (0,1) or retirement[1]>256 or retirement[2]>128 or retirement[3] or retirement[7]:
                raise RuntimeError('Owned execution retirement state is incomplete or out of bounds')
            report['ownedExecution']['proof']['finalFreeAllowed']=bool(retirement[0])
            executed_node_count=report['ownedExecution']['sceneSplit']['executionNodes'] if a.scene_split else report['ownedExecution']['stats'][8]
            if (report['ownedExecution']['stats'][5]!=meta[0] or executed_node_count!=node_count or
                    report['ownedExecution']['resources'][6]!=source_hash):
                raise RuntimeError('Executed owned graph differs from the captured declaration graph')
            if a.whole_gbuffer_plan:
                def replay_rows(count):
                    pointer,=struct.unpack('<Q',reader.read(int(symbols['CyberpunkVR_OwnedResourceReplayRows'],16),8))
                    return reader.read(pointer,count*32) if count else b''
                snapshot=read_resource_replay_snapshot(read_full,
                    lambda name,count:struct.unpack(f'<{count}Q',reader.read(int(symbols[name],16),count*8)),replay_rows)
                raw=snapshot.pop('raw');report['resourceReplay']=snapshot
                replay,proof,registration,reset,async_scope=(snapshot[key] for key in ('stats','proof','registration','reset','asyncScope'))
                a.out.with_name(a.out.stem+'-resource-replay.bin').write_bytes(raw)
                from resource_replay_report import validate as validate_resource_replay
                report['resourceReplay']=validate_resource_replay(replay,proof,raw,report['ownedExecution']['stats'])
                report['resourceReplay']['registration']=registration
                report['resourceReplay']['reset']=reset
                report['resourceReplay']['asyncScope']=async_scope
                if registration[0]!=replay[1] or registration[3:6]!=[proof[3],proof[3],0] or reset[0]!=replay[1] or reset[4:]!=[0xB79598,1,1,1]:
                    raise RuntimeError('Native replay registration/reset seal was not complete')
                if async_scope[0]!=replay[1] or not async_scope[1] or async_scope[1]!=async_scope[2] or async_scope[2]!=async_scope[5] or async_scope[3:5]!=[0,0]:
                    raise RuntimeError('Native asynchronous resource scope closure was not complete')
        report['passed'] = True
    except Exception as error:
        report['error'] = str(error)
        if a.scene_access and scene_access_owned and 'sceneAccess' not in report:
            capture_scene_access_failure(report,lambda name,size:reader.read(int(symbols[name],16),size),reader.read)
        if a.owned_reference and 'CyberpunkVR_OwnedExecutionStats' in symbols:
            from owned_execution_failure import capture as capture_owned_failure
            report['ownedExecutionFailure']=capture_owned_failure(read_full,
                lambda name,count:struct.unpack(f'<{count}Q',reader.read(int(symbols[name],16),count*8)),
                available=lambda name:name in symbols)
        if a.scene_split and 'CyberpunkVR_OwnedSceneSplitStats' in symbols:
            try:
                report['sceneSplitFailureStats']=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_OwnedSceneSplitStats'],16),192)))
                report['sceneSplitControlAtFailure']=read_full('CyberpunkVR_OwnedSceneSplitEnable')
                if report['sceneSplitFailureStats'][0]==2 and 'CyberpunkVR_OwnedSceneSplitSections' in symbols:
                    report['sceneSplitFailureSections']=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_OwnedSceneSplitSections'],16),96)))
            except Exception as evidence_error:report['sceneSplitEvidenceError']=str(evidence_error)
        if armed and a.retained_geometry:
            try:
                report['failureProgress']={name:read(name) for name in ('Request','State','Seq','Count','PacketCount','Failure')}
                report['failureProgress']['retainedState']=retained_read('State')
                report['failureProgress']['retainedStats']=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_RetainedGeometryStats'],16),128)))
                if a.retained_cbv_snapshots and retained_read('State')==4 and 'failedSourceEvidence' not in report:
                    from failed_source_evidence import capture as capture_failed_source
                    report['failedSourceEvidence']=capture_failed_source(
                        lambda name,size:reader.read(int(symbols[name],16),size),reader.read,a.out)
            except Exception as evidence_error:
                report['failureEvidenceError']=str(evidence_error)
            if a.prepared_current:
                try:capture_actual_membership()
                except Exception as evidence_error:report['actualMembershipError']=str(evidence_error)
                try:capture_fixed_assets()
                except Exception as evidence_error:report['fixedAssetsError']=str(evidence_error)
                try:capture_borrowed_tables()
                except Exception as evidence_error:report['borrowedTablesError']=str(evidence_error)
                try:capture_full_t5()
                except Exception as evidence_error:report['fullT5Error']=str(evidence_error)
                try:capture_export()
                except Exception as evidence_error:report['nativeExportError']=str(evidence_error)
                if a.prepared_tail:
                    try:report.setdefault('failureProgress',{})['targetHandoff']=capture_handoff()
                    except Exception as evidence_error:report['targetHandoffError']=str(evidence_error)
                    try:capture_tail()
                    except Exception as evidence_error:report['preparedTailError']=str(evidence_error)
                if a.prepared_infix:
                    try:capture_infix()
                    except Exception as evidence_error:report['preparedInfixError']=str(evidence_error)
                try:capture_frontier()
                except Exception as evidence_error:report['submissionFrontierError']=str(evidence_error)
                progress=report.setdefault('failureProgress',{})
                for name,symbol,words in (
                        ('nativeProbeState','CyberpunkVR_NativeStereoProbeState',0),
                        ('liveBindings','CyberpunkVR_NativeStereoLiveBindings',0),
                        ('liveFailure','CyberpunkVR_NativeStereoLiveFailure',0),
                        ('preparedPairState','CyberpunkVR_PreparedPairState',0),
                        ('preparedPairStats','CyberpunkVR_PreparedPairStats',16),
                        ('preparedCurrent','CyberpunkVR_PreparedCurrent',16),
                        ('preparedPairResidency','CyberpunkVR_PreparedPairResidency',8)):
                    try:
                        progress[name]=list(struct.unpack(f'<{words}Q',reader.read(int(symbols[symbol],16),words*8))) if words else read_full(symbol)
                    except Exception as evidence_error:
                        progress.setdefault('currentEvidenceErrors',{})[name]=str(evidence_error)
    finally:
        try:
            if armed:
                if scene_access_owned:
                    from scene_access_report import restore_controls as restore_scene_access_controls
                    report['sceneAccessCleanup']=restore_scene_access_controls(read_full,lambda name,value:write(name,value,''))
                    if not report['sceneAccessCleanup']['restored']:report['passed']=False
                if a.scene_split and 'CyberpunkVR_SubmissionFrontierEnable' in symbols:
                    if 'submissionFrontier' not in report:
                        try:capture_frontier()
                        except Exception as evidence_error:report['submissionFrontierError']=str(evidence_error)
                    try:
                        write('CyberpunkVR_SubmissionFrontierEnable',0,'')
                        report['sceneSplitFrontierCleanup']=read_full('CyberpunkVR_SubmissionFrontierEnable')
                        if report['sceneSplitFrontierCleanup']:raise RuntimeError('Scene split frontier observer was not restored')
                    except Exception as cleanup_error:
                        report['sceneSplitFrontierCleanupError']=str(cleanup_error);report['passed']=False
                if a.owned_reference and 'CyberpunkVR_OwnedSceneSplitEnable' in symbols:
                    try:
                        write('CyberpunkVR_OwnedSceneSplitEnable',0,'')
                        report['sceneSplitControlCleanup']=read_full('CyberpunkVR_OwnedSceneSplitEnable')
                        if report['sceneSplitControlCleanup']:raise RuntimeError('Native scene split control was not restored')
                    except Exception as cleanup_error:
                        report['sceneSplitCleanupError']=str(cleanup_error);report['passed']=False
                if a.prepared_current:
                    if 'actualMembership' not in report:
                        try:capture_actual_membership(verify=report.get('passed',False))
                        except Exception as evidence_error:
                            report['actualMembershipError']=str(evidence_error);report['passed']=False
                    if 'CyberpunkVR_PreparedActualMembershipEnable' in symbols:
                        try:
                            write('CyberpunkVR_PreparedActualMembershipEnable',0,'')
                            report['actualMembershipControlCleanup']=read_full('CyberpunkVR_PreparedActualMembershipEnable')
                            if report['actualMembershipControlCleanup']:raise RuntimeError('Actual membership control was not restored')
                        except Exception as cleanup_error:
                            report['actualMembershipCleanupError']=str(cleanup_error);report['passed']=False
                    if 'fixedAssets' not in report:
                        try:capture_fixed_assets(verify=report.get('passed',False))
                        except Exception as evidence_error:
                            report['fixedAssetsError']=str(evidence_error);report['passed']=False
                    if 'CyberpunkVR_RetainedFixedAssetsEnable' in symbols:
                        try:
                            write('CyberpunkVR_RetainedFixedAssetsEnable',0,'')
                            report['fixedAssetsControlCleanup']=read_full('CyberpunkVR_RetainedFixedAssetsEnable')
                            if report['fixedAssetsControlCleanup']:raise RuntimeError('Fixed-assets control was not restored')
                        except Exception as cleanup_error:
                            report['fixedAssetsCleanupError']=str(cleanup_error);report['passed']=False
                    if 'borrowedTables' not in report:
                        try:capture_borrowed_tables(verify=report.get('passed',False))
                        except Exception as evidence_error:
                            report['borrowedTablesError']=str(evidence_error);report['passed']=False
                    if 'CyberpunkVR_RetainedBorrowedTablesEnable' in symbols:
                        try:
                            write('CyberpunkVR_RetainedBorrowedTablesEnable',0,'')
                            report['borrowedTablesControlCleanup']=read_full('CyberpunkVR_RetainedBorrowedTablesEnable')
                            if report['borrowedTablesControlCleanup']:raise RuntimeError('Borrowed-table control was not restored')
                        except Exception as cleanup_error:
                            report['borrowedTablesCleanupError']=str(cleanup_error);report['passed']=False
                    if 'fullT5' not in report:
                        try:capture_full_t5(verify=report.get('passed',False))
                        except Exception as evidence_error:
                            report['fullT5Error']=str(evidence_error);report['passed']=False
                    if 'CyberpunkVR_RetainedFullT5Enable' in symbols:
                        try:
                            write('CyberpunkVR_RetainedFullT5Enable',0,'')
                            report['fullT5ControlCleanup']=read_full('CyberpunkVR_RetainedFullT5Enable')
                            if report['fullT5ControlCleanup']:raise RuntimeError('Full T5 control was not restored')
                        except Exception as cleanup_error:
                            report['fullT5CleanupError']=str(cleanup_error);report['passed']=False
                    try:capture_export()
                    except Exception as evidence_error:
                        report['nativeExportError']=str(evidence_error);report['passed']=False
                    if 'CyberpunkVR_PreparedNativeExportEnable' in symbols:
                        try:
                            write('CyberpunkVR_PreparedNativeExportEnable',0,'')
                            report['nativeExportControlCleanup']=read_full('CyberpunkVR_PreparedNativeExportEnable')
                            if report['nativeExportControlCleanup']:raise RuntimeError('Native export control was not restored')
                        except Exception as cleanup_error:
                            report['nativeExportCleanupError']=str(cleanup_error);report['passed']=False
                try:
                    if a.owned_graph_sort:write('OwnedGraphMode',0);write('GraphJobsMode',0);write('DefinitionMode',0);write('DefinitionRequest',0)
                    if a.whole_gbuffer_plan:write('OwnedGraphMode',0);write('CyberpunkVR_OwnedResourceReplayEnable',0,'')
                    if a.owned_reference:
                        write('CyberpunkVR_OwnedExecutionRequest',0,'');write('CyberpunkVR_OwnedExecutionMode',0,'')
                    report['cleanup'] = restore_controls(write, lambda: k.WaitForSingleObject(handle, 0)==0,a.graph_plan,bool(a.schedule_preparation),camera_write if a.paired_cameras else None)
                except OSError as error:
                    report['cleanupError']=str(error)
                    report['passed']=False
                if a.early_depth_gpu:
                    try:write('AllStages',0)
                    except OSError as error:report['cleanupError']=str(error);report['passed']=False
                    try:write('CameraStages',1,'CyberpunkVR_EarlyDepth')
                    except OSError as error:report['cleanupError']=str(error);report['passed']=False
                    for name in early_controls:
                        try:write(name,0,'')
                        except OSError as error:
                            if k.WaitForSingleObject(handle,0)==0:break
                            report['cleanupError']=str(error);report['passed']=False
                if a.retained_geometry or a.paired_gbuffer:
                    owned=(('CyberpunkVR_PairedGeometryRequest',0),('CyberpunkVR_PairedGeometryState',0),('CyberpunkVR_PairedGeometryOrdinal',0),('CyberpunkVR_PairedGeometryFamily',0)) if a.paired_gbuffer else (
                           ('CyberpunkVR_RetainedGeometryRequest',0),('CyberpunkVR_RetainedGeometryState',0),('CyberpunkVR_RetainedGeometryStage',7))
                    cleanup=(*owned,*((('CyberpunkVR_RetainedSourceStreamEnable',0),) if a.retained_source_stream else ()),
                             *((('CyberpunkVR_PreparedVrsRefinementEnable',0),) if a.allow_vrs_refinement else ()),
                             *((('CyberpunkVR_PreparedTailEnable',0),) if a.prepared_tail else ()),
                             *((('CyberpunkVR_PreparedInfixEnable',0),) if a.prepared_infix else ()),
                             *((('CyberpunkVR_PreparedCurrentEnable',0),('CyberpunkVR_SubmissionFrontierEnable',0)) if a.prepared_current else ()),
                             *((('CyberpunkVR_RetainedIndirectSnapshotsEnable',0),) if a.retained_indirect_snapshots else ()),('CyberpunkVR_PreparationAllStages',0),
                             *((('CyberpunkVR_RetainedSourceCbvEnable',0),('CyberpunkVR_SourceCbvBindingEnable',0)) if a.retained_source_cbvs else ()),
                             *((('CyberpunkVR_RetainedSourceTablesEnable',0),) if a.retained_source_tables else ()),
                             *((('CyberpunkVR_RetainedSourceDetailsEnable',0),) if a.retained_source_details else ()),
                             *((('CyberpunkVR_RetainedSourceCbvSnapshotsEnable',0),('CyberpunkVR_SourceCbvSnapshotsEnable',0),('CyberpunkVR_SourceCbvSnapshotsUploadEnable',0)) if a.retained_cbv_snapshots else ()),
                             *((('CyberpunkVR_NativeIaSnapshotsEnable',0),('CyberpunkVR_NativeInstanceObserverEnable',0),('CyberpunkVR_NativeInstanceObserverResetRequest',0)) if a.retained_ia_snapshots else ()),
                             *((('CyberpunkVR_StructuredBufferSnapshotsEnable',0),) if a.retained_source_selectors else ()),
                             ('CyberpunkVR_GeometryPacketRequest',0),('CyberpunkVR_GeometryPacketState',0),('CyberpunkVR_GeometryPacketResolve',0),
                             ('CyberpunkVR_NativeStereoProbeRequest',0),('CyberpunkVR_NativeStereoProbeState',0),('CyberpunkVR_NativeStereoAllDraws',0))
                    for name,value in cleanup:
                        try:write(name,value,'')
                        except OSError as error:
                            if k.WaitForSingleObject(handle,0)==0:break
                            report['cleanupError']=str(error);report['passed']=False
        finally:
            try:
                if turn:report['cameraPoseCleanup']=turn.restore()
            except Exception as error:
                report['cameraPoseCleanupError']=str(error);report['passed']=False
            finally:
                if handle:k.CloseHandle(handle)
                reader.close()
                a.out.write_text(json.dumps(report, indent=2)+'\n')
                printed={k:v for k,v in report.items() if k not in ('records', 'summary')}
                if 'cpuSchedule' in printed:printed['cpuSchedule']={k:v for k,v in printed['cpuSchedule'].items() if k!='edges'}
                if 'passTargets' in printed:printed['passTargets']={k:v for k,v in printed['passTargets'].items() if k not in ('groups','resources','sharedAcrossEyes')}
                if a.compact:
                    printed={key:value for key,value in report.items() if value is None or isinstance(value,(str,int,float,bool))}
                    printed['reportFile']=str(a.out.resolve())
                print(json.dumps(printed, indent=2))
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
