"""Request a completed-source probe or observe an already-issued current-frame comparison."""
import argparse
import ctypes as c
import json
from pathlib import Path
import struct
import sys
import time

ROOT = Path(__file__).resolve().parents[2]
sys.path[:0] = [str(ROOT/'tools/roomscale_tests'), str(ROOT/'tools/swimming_tests')]
from live_read import Reader
from live_symbols import resolve
from native_export_report import decode as decode_native_export, validate as validate_native_export

FIELDS = ('sourceLeft', 'sourceRight', 'meshBytes', 'meshEqual', 'referenceDraws', 'pairedDraws',
          'differenceLeft', 'differenceRight', 'contributionLeft', 'contributionRight', 'error', 'stage',
          'destinationList', 'generation', 'registeredAssets', 'nativeSuppressed')
GPU_FIELDS = ('iaVertices', 'iaPrimitives', 'vsInvocations', 'gsInvocations', 'gsPrimitives',
              'clipInvocations', 'clipPrimitives', 'psInvocations', 'hsInvocations', 'dsInvocations',
              'csInvocations', 'samplesPassed')
DEPTH_FIELDS = ('depthEnable', 'depthFunc', 'depthWriteMask', 'stencilEnable', 'stencilReadMask',
                'stencilWriteMask', 'frontFunc', 'backFunc', 'stencilReference', 'frontFail',
                'frontDepthFail', 'frontPass', 'backPass', 'predicateCpu', 'predicateGpu', 'queriesRecorded')
BATCH_FIELDS = ('leftSources','rightSources','candidatePairs','sharedDraws','leftOnly','rightOnly',
                'plannedDraws','referenceRecorded','optimizedRecorded','meshFallbacks','bankRejected',
                'orderVerified','targetInitializations','imageReadbacks','nativeSuppressed','sequenceMode')
MESH_FIELDS = ('pairs','ranges','comparedBytes','dispatches','equal','fallback','disabled','resultReadbackBytes',
               'meshReadbackBytes','preDrawCpuFenceWaits','pairedRecorded','fallbackRecorded')
INPUT_FIELDS = ('sources','ranges','comparedBytes','dispatches','expectedUploadBytes','accepted','rejected','resultReadbackBytes')
VRS_GPU_FIELDS = ('sources','dispatches','readbackBytes','differences')
RESIDENCY_FIELDS = ('sourceLeases','physicalPageables','state','markedSubmitted','associatedFence',
                    'retired','lastHRESULT','processQuarantineCount')
CURRENT_FIELDS = ('mode','sourceFrame','recordedFrame','submittedFrame','recorded','closed','submitted',
                  'completed','sourceCount','nativeCallbacksUsed','CPUCompletionWaits','nativeSuppressed',
                  'error','assetLeases','reserved0','reserved1')
CURRENT_SOURCE_FIELDS = ('armed','frame','packets','recordedBeforeLastSourceSubmit','mainWasSubmitted',
                         'peerWasSubmitted','payloadsProcessed','cbvPayloadComplete','overlayStage',
                         'submitted','mainSubmitted','peerSubmitted','queue','mainSignal','peerSignal','error')
BASELINE_FIELDS = ('eyes','colors','seededOutputs','comparedPlanes','baselinePayloadBytes','baselineNonzeroColorPixels',
                   'referenceColorDeltaLeft','referenceColorDeltaRight','optimizedColorDeltaLeft','optimizedColorDeltaRight',
                   'referenceDepthDeltaLeft','referenceDepthDeltaRight','optimizedDepthDeltaLeft','optimizedDepthDeltaRight',
                   'referenceStencilDeltaLeft','referenceStencilDeltaRight','optimizedStencilDeltaLeft','optimizedStencilDeltaRight',
                   'rejectedUnchanged','rejectedExecutionZero','positiveExecution','fullBaselineCompared','inputRejected','queriesRead')
NATIVE_EXIT_FIELDS = ('mode','eyes','planes','bytes','referenceColorLeft','referenceColorRight','optimizedColorLeft','optimizedColorRight',
                      'referenceDepthLeft','referenceDepthRight','optimizedDepthLeft','optimizedDepthRight',
                      'referenceStencilLeft','referenceStencilRight','optimizedStencilLeft','optimizedStencilRight',
                      'referenceTotalLeft','referenceTotalRight','optimizedTotalLeft','optimizedTotalRight',
                      'compared','nativeEquivalent','inputRejected','abiVersion')
NATIVE_EXIT_SAMPLE_FIELDS = ('eye','slot','plane','referenceDifferent','optimizedDifferent','referenceFirstX','referenceFirstY',
                             'referenceValue','nativeAtReference','optimizedFirstX','optimizedFirstY','optimizedValue','nativeAtOptimized',
                             'stride','component','bytes')
NATIVE_RATE_FIELDS = ('mode','eyes','recordedDraws','planes','payloadBytes','colorLeft','colorRight','depthLeft','depthRight',
                      'stencilLeft','stencilRight','totalLeft','totalRight','compared','equivalent','inputRejected')
VRS_ATTRIBUTION_FIELDS = ('tileSize','mapWidth','mapHeight','eyeUniformLeft','eyeUniformRight',
                          'referenceColorDiffInsideLeft','referenceColorDiffInsideRight',
                          'referenceColorDiffOutsideLeft','referenceColorDiffOutsideRight',
                          'originalRateCompared','commonAlwaysFiner','valid')
NATIVE_ACCEPTANCE_FIELDS = ('policy','nativeRateEquivalent','attributionValidAndFiner','unexplainedLeft','unexplainedRight',
                            'pairDifferentLeft','pairDifferentRight','accepted')


def validate_rate_diagnostics(rate, attribution, baseline, native_exit):
    for values,count in ((rate,16),(attribution,12)):
        if (not isinstance(values,(list,tuple)) or len(values)!=count or
                any(type(value) is not int or not 0<=value<2**64 for value in values)):
            raise ValueError('Prepared native-rate diagnostic ABI differs')
    result=dict(nativeRate=dict(zip(NATIVE_RATE_FIELDS,rate)),vrsAttribution=dict(zip(VRS_ATTRIBUTION_FIELDS,attribution)))
    if not rate[0]:
        if any(rate) or any(attribution):raise ValueError('Native-rate diagnostics appeared outside native mode')
        return result
    if not rate[13] and not attribution[11]:return result
    if (not isinstance(baseline,(list,tuple)) or len(baseline)!=24 or
            any(type(value) is not int or not 0<=value<2**64 for value in baseline) or
            not 0<baseline[4]<=128*1024*1024 or baseline[4]%34):
        raise ValueError('Completed native-rate diagnostics lack a bounded baseline extent')
    pixels=baseline[4]//34
    if rate[13]:
        if (list(rate[0:2])!=[1,2] or not 0<rate[2]<=512 or list(rate[3:5])!=[10,baseline[4]] or rate[13]!=1 or
                rate[15] not in (0,1) or any(value>3*pixels for value in rate[5:7]) or
                any(value>pixels for value in rate[7:11])):
            raise ValueError('Completed native-rate diagnostic bounds differ')
        totals=[sum(rate[offset+eye] for offset in (5,7,9)) for eye in range(2)]
        if list(rate[11:13])!=totals or rate[14]!=int(not rate[15] and not any(totals)):
            raise ValueError('Native-rate diagnostic totals or equality marker differ')
    if attribution[11]:
        if (attribution[11]!=1 or not 0<attribution[0]<2**32 or
                not 0<attribution[1]<=512 or not 0<attribution[2]<=512 or
                any(attribution[index] not in (0,1) for index in (3,4,9,10)) or
                any(value>3*pixels for value in attribution[5:9])):
            raise ValueError('Completed VRS attribution diagnostic bounds differ')
        if (not isinstance(native_exit,(list,tuple)) or len(native_exit)!=24 or
                any(type(value) is not int or not 0<=value<2**64 for value in native_exit) or native_exit[20]!=1 or
                [attribution[5+eye]+attribution[7+eye] for eye in range(2)]!=list(native_exit[4:6])):
            raise ValueError('VRS attribution did not account for every primary reference color difference')
    return result


def validate_native_acceptance(acceptance, native, rate, attribution, pair_stats, baseline, *, allow_vrs_refinement=False, rejected=False):
    for values,count in ((acceptance,8),(native,24),(pair_stats,16)):
        if (not isinstance(values,(list,tuple)) or len(values)!=count or
                any(type(value) is not int or not 0<=value<2**64 for value in values)):
            raise ValueError('Prepared native acceptance ABI differs')
    validate_rate_diagnostics(rate,attribution,baseline,native)
    if not native[0]:
        if any(acceptance) or allow_vrs_refinement:raise ValueError('Native acceptance policy appeared outside native-tail mode')
        return dict(zip(NATIVE_ACCEPTANCE_FIELDS,acceptance))
    rate_equal=int(rate[0]==1 and rate[13]==1 and rate[14]==1 and not rate[15] and not any(rate[5:13]))
    attributed=int(all(attribution[index]==1 for index in (3,4,9,10,11)))
    unexplained=[min(2**64-1,attribution[7+eye]+sum(native[offset+eye] for offset in (8,10,12,14))) for eye in range(2)]
    complete=list(native[:3])==[1,2,10] and native[20]==1
    accepted=int(not rejected and not native[22] and not rate[15] and complete and not any(pair_stats[6:8]) and
                 (native[21]==1 or (allow_vrs_refinement and rate_equal and attributed and not any(unexplained))))
    expected=[int(allow_vrs_refinement),rate_equal,attributed,*unexplained,*pair_stats[6:8],accepted]
    if list(acceptance)!=expected:raise ValueError('Native acceptance differs from explicit policy or underlying output and VRS proofs')
    return dict(zip(NATIVE_ACCEPTANCE_FIELDS,acceptance))


def validate_native_exit(values, baseline, *, required=False, rejected=False, allow_vrs_refinement=False,
                         acceptance=None, rate=None, attribution=None, pair_stats=None):
    if (not isinstance(values,(list,tuple)) or len(values)!=24 or
            any(type(value) is not int or not 0<=value<2**64 for value in values)):
        raise ValueError('Prepared native exit ABI differs')
    if not required:
        if any(values[:23]) or values[23] not in (0,1):raise ValueError('Native exit comparison appeared outside native-tail mode')
        if allow_vrs_refinement:raise ValueError('VRS refinement requires a native-tail comparison')
        if acceptance is not None:validate_native_acceptance(acceptance,values,rate,attribution,pair_stats,baseline)
        return dict(zip(NATIVE_EXIT_FIELDS,values))
    if (not isinstance(baseline,(list,tuple)) or len(baseline)!=24 or
            any(type(value) is not int or not 0<=value<2**64 for value in baseline)):
        raise ValueError('Native exit comparison lacks its complete baseline extent')
    payload=baseline[4]
    if (list(baseline[:4])!=[2,3,4,10] or not 0<payload<=128*1024*1024 or payload%34 or list(values[:4])!=[1,2,10,payload] or
            values[20]!=1 or values[22]!=int(rejected) or values[23]!=1):
        raise ValueError('Prepared native exit did not compare both eyes and every output plane')
    pixels=payload//34
    for offset,limit in ((4,3*pixels),(8,pixels),(12,pixels)):
        if any(value>limit for value in values[offset:offset+4]):raise ValueError('Native exit differences exceed their plane extent')
    totals=[sum(values[offset+i] for offset in (4,8,12)) for i in range(4)]
    if list(values[16:20])!=totals or values[21]!=int(not any(totals)):
        raise ValueError('Native exit totals or equality marker differ from compared planes')
    accepted=None
    if allow_vrs_refinement or acceptance is not None:
        accepted=validate_native_acceptance(acceptance,values,rate,attribution,pair_stats,baseline,
            allow_vrs_refinement=allow_vrs_refinement,rejected=rejected)
    if not rejected and ((not allow_vrs_refinement and any(totals)) or
                         (accepted is not None and accepted['accepted']!=1)):
        raise ValueError('Prepared tail did not satisfy its explicit native exit acceptance policy')
    return dict(zip(NATIVE_EXIT_FIELDS,values))


def validate_baseline(baseline, stats, gpu, colors, *, rejected=False):
    for values,size in ((baseline,24),(stats,16),(gpu,36),(colors,12)):
        if (not isinstance(values,(list,tuple)) or len(values)!=size or
                any(type(value) is not int or not 0<=value<2**64 for value in values)):
            raise ValueError('Prepared full-baseline comparison ABI differs')
    # Three four-byte color planes, four-byte depth and one-byte stencil per eye.
    payload=baseline[4]
    if list(baseline[:4])!=[2,3,4,10] or not 0<payload<=128*1024*1024 or payload%34:
        raise ValueError('Prepared comparison omitted a seeded output or full baseline plane')
    pixels=payload//34
    if baseline[5]>6*pixels or any(value>pixels for value in colors) or list(colors[:6])!=list(colors[6:]):
        raise ValueError('Prepared absolute color occupancy differs or exceeds its baseline extent')
    for offset,limit in ((6,3*pixels),(10,pixels),(14,pixels)):
        if (any(value>limit for value in baseline[offset:offset+4]) or
                list(baseline[offset:offset+2])!=list(baseline[offset+2:offset+4])):
            raise ValueError('Reference and optimized baseline-relative plane deltas differ')
    if any(stats[6:8]) or list(stats[8:10])!=list(baseline[6:8]):
        raise ValueError('Complete output equality or reference color-delta counters differ')
    if rejected:
        if (any(baseline[6:18]) or list(baseline[18:])!=[1,1,0,1,1,1] or any(gpu) or
                sum(colors[:6])!=baseline[5]):
            raise ValueError('Rejected inputs changed the pre-interval baseline or executed GPU geometry')
    elif (list(baseline[18:])!=[0,0,1,1,0,1] or
            any(not any(baseline[offset+eye] for offset in (6,10,14)) for eye in range(2)) or
            any(not gpu[draw*12+field] for draw in range(3) for field in (2,7,11))):
        raise ValueError('Prepared draws did not execute and change a baseline plane in both eyes')
    return dict(zip(BASELINE_FIELDS,baseline))


def validate_current_source(values, frame, packets):
    if (not isinstance(values,(list,tuple)) or len(values)!=16 or
            any(type(value) is not int or not 0<=value<2**64 for value in values) or
            type(frame) is not int or not 0<frame<2**64 or type(packets) is not int or not 0<packets<=512):
        raise ValueError('Current source scheduling ABI differs')
    if (list(values[:4])!=[1,frame,packets,1] or any(value not in (0,1) for value in values[4:6]) or
            sum(values[4:6])>1 or any(values[6:9]) or list(values[9:12])!=[1,1,1] or
            not all(values[12:15]) or values[15]):
        raise ValueError('Current source preparation waited for payloads or lost native submission order')
    return dict(zip(CURRENT_SOURCE_FIELDS,values))


def validate_current(values, sources, expected_frame=None, *, rejected=False):
    if (not isinstance(values,(list,tuple)) or len(values)!=16 or
            any(type(value) is not int or not 0<=value<2**64 for value in values) or
            type(sources) is not int or not 0<sources<=512 or
            (expected_frame is not None and (type(expected_frame) is not int or not 0<expected_frame<2**64))):
        raise ValueError('Owned current-frame comparison ABI differs')
    frame=values[1]
    if (values[0]!=1 or not frame or list(values[2:4])!=[frame,frame] or
            list(values[4:9])!=[1,1,1,1,sources] or any(values[9:12]) or
            values[12]!=(29 if rejected else 0) or values[13]>65536 or any(values[14:]) or
            (expected_frame is not None and frame!=expected_frame)):
        raise ValueError('Current comparison lost owned same-frame execution or completed lifecycle')
    return dict(zip(CURRENT_FIELDS,values))


def validate_residency(values, registered_assets):
    if (not isinstance(values, (list, tuple)) or len(values)!=8 or
            any(type(value) is not int or not 0<=value<2**64 for value in values) or
            type(registered_assets) is not int or not 0<=registered_assets<2**64 or values[6]>=2**32):
        raise ValueError('Prepared pair residency ABI differs')
    if not registered_assets:
        if any(values):
            raise ValueError('Asset-free prepared pair acquired residency counters')
    elif (values[0]!=registered_assets or not 0<values[1]<=values[0] or
            list(values[2:])!=[5,1,1,1,0,0]):
        raise ValueError('Prepared pair physical residency was incomplete, failed or quarantined')
    return dict(zip(RESIDENCY_FIELDS, values))


def validate_current_residency(values, asset_leases, native_registered_assets):
    if (type(native_registered_assets) is not int or native_registered_assets!=0 or
            type(asset_leases) is not int or not 0<=asset_leases<=65536):
        raise ValueError('Owned current comparison fabricated native use registration or asset leases')
    return validate_residency(values,asset_leases)


def validate_vrs(values, gpu, sources):
    if len(values)!=8 or len(gpu)!=4 or any(type(v) is not int or not 0<=v<2**64 for v in values+gpu):
        raise ValueError('GPU VRS reduction ABI differs')
    if not any(values+gpu):
        return
    width,height,cells=values[:3]
    if (not 0<width<=512 or not 0<height<=512 or cells!=width*height or any(v>cells for v in values[3:6]) or
            values[6:]!=[2,0] or not 0<gpu[0]<=sources or gpu[1]!=2 or not cells+8<=gpu[2]<=1024*1024+8 or gpu[3]):
        raise ValueError('GPU VRS reduction differs from original-source per-axis minimum')


def validate_inputs(values, sources, required=False):
    if len(values)!=8 or any(type(v) is not int or not 0<=v<2**64 for v in values):
        raise ValueError('GPU shader-input gate ABI differs')
    if not any(values):
        if required:
            raise ValueError('Prepared sequence omitted its GPU shader-input gate')
        return dict(zip(INPUT_FIELDS, values))
    covered,ranges,byte_count,dispatches,uploaded,accepted,rejected,readback=values
    if (not 0<covered<=sources or not covered<=ranges<=8192 or not 0<byte_count<=uploaded<=4*1024*1024 or
            uploaded%4 or uploaded-byte_count>ranges*3 or not 2<=dispatches<=ranges+1 or (accepted,rejected,readback)!=(1,0,16)):
        raise ValueError('GPU shader-input checks were incomplete, rejected or disabled')
    return dict(zip(INPUT_FIELDS, values))


def validate_input_rejection(state, values, inputs, mesh, results, gpu, colors, depth, sources, baseline, *, actual_membership=False):
    checked=list(inputs)
    if len(checked)!=8 or checked[5:7]!=[0,1]:
        raise ValueError('Cleared membership bitmap was not rejected on the GPU' if actual_membership else 'Corrupted selector was not rejected on the GPU')
    checked[5:7]=[1,0]
    validate_inputs(checked,sources,required=True)
    if (state!=6 or len(values)!=16 or values[10]!=29 or values[15] or any(values[3:10]) or
            len(mesh)!=12 or not mesh[0] or mesh[4:7]!=[0,0,mesh[0]] or len(results)!=mesh[0] or
            not 2<=mesh[3]<=mesh[1]+1 or mesh[7:]!=[mesh[0]*16,0,0,mesh[0],mesh[0]*2] or
            any(row!=[0,0] for row in results) or len(gpu)!=36 or len(colors)!=12 or
            len(depth)!=16 or depth[13:]!=[1,1,3] or any(gpu)):
        raise ValueError('Rejected shader inputs reached geometry, shader execution or native suppression')
    return validate_baseline(baseline,values,gpu,colors,rejected=True)


def validate_mesh(mesh, results, shared, require_shared=True):
    if len(mesh)!=12 or any(type(v) is not int or not 0<=v<2**64 for v in mesh) or not 0<shared<=256:
        raise ValueError('GPU geometry predicate ABI differs')
    pairs,ranges,byte_count,dispatches,equal,fallback,disabled,readback,mesh_readback,waits,paired_recorded,fallback_recorded=mesh
    if (pairs,disabled,readback,mesh_readback,waits,paired_recorded,fallback_recorded)!=(shared,0,shared*16,0,0,shared,shared*2) or not 2<=dispatches<=ranges+1:
        raise ValueError('GPU geometry comparison waited on CPU, omitted a branch or lacked permission')
    if ranges<shared*2 or not byte_count or (require_shared and not equal) or equal+fallback!=shared or len(results)!=shared:
        raise ValueError('GPU geometry comparison has incomplete coverage or no paired execution')
    counts=[0,0]
    for row in results:
        if len(row)!=2 or any(type(v) is not int or v not in (0,1) for v in row) or sum(row)!=1:
            raise ValueError('GPU geometry branches are not complementary')
        counts=[a+b for a,b in zip(counts,row)]
    if counts!=[equal,fallback]:
        raise ValueError('GPU geometry result counts differ from predicates')
    return dict(zip(MESH_FIELDS,mesh))


def validate_sequence(state, values, batch, sources, plan, mesh=None):
    maximum=2**64-1
    if any(len(row)!=16 or any(type(v) is not int or not 0<=v<=maximum for v in row) for row in (values,batch)):
        raise ValueError('Prepared sequence stats ABI differs')
    if state!=5 or values[10] or values[15] or not values[2] or values[3]!=1 or not all(values[i] for i in (12,13)):
        raise ValueError('Prepared sequence did not prove equal owned output')
    left,right,candidates,shared,left_only,right_only,draws,reference,optimized=batch[:9]
    if not shared or not left or not right or shared>candidates or shared>min(left,right) or batch[9]+batch[10]>candidates:
        raise ValueError('Prepared sequence sharing counts differ')
    recorded=left+right+shared if mesh is not None else left+right-shared
    executed=left+right-shared+(mesh[5] if mesh is not None else 0)
    if mesh is not None and (mesh[0]!=shared or batch[9]!=mesh[5]):
        raise ValueError('Prepared sequence GPU fallback counts differ')
    if (left_only,right_only,draws,reference,optimized)!=(left-shared,right-shared,left+right-shared,left+right,recorded):
        raise ValueError('Prepared sequence lost or duplicated a draw')
    if values[4:8]!=[reference,executed,0,0] or batch[11:]!=[1,4,1,0,1]:
        raise ValueError('Prepared sequence changed outputs, targets, or native work')
    if len(sources)!=left+right or len(sources)>512 or len(plan)!=draws or len(plan)>1024:
        raise ValueError('Prepared sequence row extents differ')
    expected=[[],[]]
    ordinals=set()
    for row in sources:
        if len(row)!=2 or any(type(v) is not int for v in row) or not 0<=row[0]<maximum or row[1] not in (0,1) or row[0] in ordinals:
            raise ValueError('Prepared sequence source identities differ')
        expected[row[1]].append(row[0])
        ordinals.add(row[0])
    if list(map(len,expected))!=[left,right] or any(eye!=sorted(eye) for eye in expected):
        raise ValueError('Prepared sequence input eye order differs')
    actual=[[],[]]
    common=0
    for row in plan:
        if len(row)!=4 or any(type(v) is not int or not 0<=v<=maximum for v in row) or row[0] not in (1,2,3):
            raise ValueError('Prepared sequence plan ABI differs')
        mask,a,b,command=row
        for eye,ordinal in enumerate((a,b)):
            if mask & (1<<eye):
                actual[eye].append(ordinal)
            elif ordinal!=maximum:
                raise ValueError('Prepared sequence has an unexpected eye')
        if mask==3:
            if command!=common:
                raise ValueError('Prepared sequence shared geometry order differs')
            common+=1
        elif command!=maximum:
            raise ValueError('Source-only draw acquired paired geometry')
    if actual!=expected or common!=shared:
        raise ValueError('Prepared sequence reordered or omitted an eye source')
    return dict(zip(BATCH_FIELDS,batch))


def validate_gpu(gpu, colors, depth):
    if (len(gpu) != 36 or len(colors) != 12 or len(depth) != 16 or
            any(type(value) is not int or not 0<=value<2**64 for values in (gpu,colors,depth) for value in values)):
        raise ValueError('Prepared pair GPU diagnostics ABI differs')
    if depth[13:] != [1, 1, 3]:
        raise ValueError('Prepared pair predicate or GPU query execution was not confirmed')
    for draw in range(3):
        if not all(gpu[draw*12+field] for field in (2, 7, 11)):
            raise ValueError('Prepared pair draw did not reach visible pixel shading')
    if colors[:6] != colors[6:]:
        raise ValueError('Prepared pair has unequal absolute color occupancy')


def validate_descriptors(values, draws):
    if len(values)!=4 or any(type(v) is not int or v<0 for v in values) or not 0<values[0]<=1048576 or not 0<values[1]<=1048576 or values[2:]!=[draws,0] or not draws:
        raise ValueError('Prepared sequence did not use its shared direct-input descriptor bank')


def retry_empty(state, values):
    return (state == 6 and len(values) == 16 and values[10] == 21 and values[3:8] == [1, 2, 1, 0, 0]
            and not values[15] and (not values[8] or not values[9]))


def retry_fallback(state, values, mesh, results, depth):
    # Structural candidates may contain different instances. A correctly
    # rendered fallback is not a shared-draw success, but is safe to skip.
    if (state!=6 or len(values)!=16 or values[3:8]!=[0,2,2,0,0] or values[10]!=17 or values[15]
            or len(depth)!=16 or depth[13:]!=[1,1,3]):
        return False
    try:
        validate_mesh(mesh,results,1,require_shared=False)
    except ValueError:
        return False
    return mesh[4:7]==[0,1,0]


def exhausted(state, values, selection, count, successful):
    return (state == 6 and len(values) == 16 and values[10] == 24 and not any(values[:10]+values[11:])
            and selection == count and successful > 0)


def validate(state, values):
    if len(values) != 16 or any(type(value) is not int or not 0 <= value < 2**64 for value in values):
        raise ValueError('Prepared pair stats ABI differs')
    if state != 5 or values[10] or values[15]:
        raise ValueError('Prepared native pair failed or suppressed original rendering')
    if values[0] == values[1] or not values[2] or values[3:8] != [1, 2, 1, 0, 0] or not all(values[i] for i in (12, 13)):
        raise ValueError('Prepared pair did not prove mesh equality and equal owned eye outputs')
    return dict(zip(FIELDS, values))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pid', type=int, required=True)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=60)
    parser.add_argument('--attempts', type=int, default=64)
    parser.add_argument('--all-pairs', action='store_true', help='Continue through the completed capture after a visible match')
    parser.add_argument('--sequence', action='store_true', help='Compare the complete prepared source sequence in persistent texture arrays')
    parser.add_argument('--observe-current', action='store_true', help='Observe the already-issued owned current-frame sequence without changing controls')
    parser.add_argument('--native-tail', action='store_true', help='Require completed final-tail comparison against the original native exit')
    parser.add_argument('--allow-vrs-refinement', action='store_true', help='Accept the approved shared finer VRS map only with complete native-rate attribution proof')
    parser.add_argument('--native-export', action='store_true', help='Require completed owned native output copies and unconditional roundtrip comparison')
    parser.add_argument('--actual-membership', action='store_true', help='Require actual B4/full-T5 owned-bitmap checks and GPU predicate AND')
    parser.add_argument('--source-frame', type=int, help='Require this exact retained source frame in current observation mode')
    parser.add_argument('--reject-inputs', action='store_true', help='Require GPU rejection: corrupt expected bytes or clear bitmaps in actual-membership mode')
    parser.add_argument('--game', type=Path, default=Path(r'C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077'))
    args = parser.parse_args()
    if args.actual_membership and not (args.observe_current and args.native_tail):
        parser.error('--actual-membership requires --observe-current and --native-tail')
    if args.native_export and not (args.observe_current and args.native_tail and args.allow_vrs_refinement):
        parser.error('--native-export requires --observe-current, --native-tail and --allow-vrs-refinement')
    if args.allow_vrs_refinement and not args.native_tail:parser.error('--allow-vrs-refinement requires --native-tail')
    if args.native_tail and not args.observe_current:parser.error('--native-tail requires --observe-current')
    if not 1 <= args.attempts <= 64:
        parser.error('--attempts must be between 1 and 64')
    if args.sequence and args.all_pairs:
        parser.error('--sequence and --all-pairs are separate GPU comparisons')
    if args.reject_inputs and not args.sequence:
        parser.error('--reject-inputs requires --sequence')
    if args.observe_current and not args.sequence:
        parser.error('--observe-current requires --sequence')
    if args.source_frame is not None and (not args.observe_current or not 0<args.source_frame<2**64):
        parser.error('--source-frame requires --observe-current and a positive uint64 frame')
    dll = args.game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
    names = ['CyberpunkVR_PreparedPair*', 'CyberpunkVR_PreparedBatch*', 'CyberpunkVR_PreparedCurrent*', 'CyberpunkVR_PreparedBaseline', 'CyberpunkVR_PreparedNativeExit*', 'CyberpunkVR_PreparedNativeExport*', 'CyberpunkVR_PreparedActualMembership*', 'CyberpunkVR_PreparedNativeRate', 'CyberpunkVR_PreparedVrsAttribution', 'CyberpunkVR_PreparedNativeAcceptance', 'CyberpunkVR_NativeStereoLiveBindings', 'CyberpunkVR_NativeStereoProbeState']
    symbols = resolve(args.pid, dll, names)
    reader = Reader(args.pid, args.game/'bin/x64/Cyberpunk2077.exe', 0, 0)
    kernel = c.WinDLL('kernel32', use_last_error=True)
    kernel.OpenProcess.argtypes = [c.c_uint32, c.c_int, c.c_uint32]
    kernel.OpenProcess.restype = c.c_void_p
    kernel.CloseHandle.argtypes = [c.c_void_p]
    kernel.WaitForSingleObject.argtypes = [c.c_void_p, c.c_uint32]
    kernel.WriteProcessMemory.argtypes = [c.c_void_p, c.c_void_p, c.c_void_p, c.c_size_t, c.POINTER(c.c_size_t)]
    handle = kernel.OpenProcess(0x100000 | 0x400 | (0 if args.observe_current else 0x38), False, args.pid)
    if not handle:
        reader.close()
        raise c.WinError(c.get_last_error())
    def read(name):
        return struct.unpack('<I', reader.read(int(symbols[name], 16), 4))[0]
    def write(name, value):
        data, done = c.c_uint32(value), c.c_size_t()
        if not kernel.WriteProcessMemory(handle, int(symbols[name], 16), c.byref(data), 4, c.byref(done)) or done.value != 4:
            raise c.WinError(c.get_last_error())
    report = dict(pid=args.pid, dll=symbols['sha256'], passed=False, nativeTailRequested=args.native_tail,
                  allowVrsRefinement=args.allow_vrs_refinement,acceptedUnderVrsRefinement=False,
                  nativeExportRequested=args.native_export,nativeExportAvailable='CyberpunkVR_PreparedNativeExport' in symbols,
                  actualMembershipRequested=args.actual_membership,actualMembershipAvailable='CyberpunkVR_PreparedActualMembershipStats' in symbols)
    previous = {}
    try:
        if not args.observe_current:
            if read('CyberpunkVR_PreparedPairState') in (1, 2, 3, 4) or read('CyberpunkVR_PreparedPairRequest'):
                raise RuntimeError('An existing native pair probe is still active')
            for name in ('CyberpunkVR_NativeStereoLiveBindings','CyberpunkVR_NativeStereoProbeState','CyberpunkVR_PreparedPairSelection','CyberpunkVR_PreparedPairInputFault'):
                previous[name] = read(name)
            write('CyberpunkVR_NativeStereoLiveBindings', 1)
            write('CyberpunkVR_NativeStereoProbeState', 1)
            write('CyberpunkVR_PreparedPairInputFault',int(args.reject_inputs))
        report['inputRejectionRequested']=args.reject_inputs
        report['observedCurrent']=args.observe_current
        report['protectionProvenance']='sdk_residency' if args.observe_current else 'native_use_registration_and_sdk_residency'
        selection=0
        report['attempts']=[]
        report['successfulPairs']=[]
        if not args.observe_current:
            write('CyberpunkVR_PreparedPairSelection', selection)
            write('CyberpunkVR_PreparedPairState', 0)
            write('CyberpunkVR_PreparedPairRequest', 2 if args.sequence else 1)
        deadline = time.monotonic()+args.timeout
        while True:
            if kernel.WaitForSingleObject(handle, 0) == 0:
                raise RuntimeError('Game exited during the native pair probe')
            state = read('CyberpunkVR_PreparedPairState')
            values = list(struct.unpack('<16Q', reader.read(int(symbols['CyberpunkVR_PreparedPairStats'], 16), 128)))
            report.update(state=state, stats=dict(zip(FIELDS, values)))
            current=None
            if args.observe_current:
                current=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_PreparedCurrent'],16),128)))
                report['current']=dict(zip(CURRENT_FIELDS,current))
                if report['actualMembershipAvailable']:
                    from actual_membership_report import read_report as read_membership
                    report['actualMembership']=read_membership(lambda name,size:reader.read(int(symbols[name],16),size))
                elif args.actual_membership:raise ValueError('Actual membership telemetry is missing from this build')
                if report['nativeExportAvailable']:
                    exported=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_PreparedNativeExport'],16),192)))
                    report['nativeExportRaw']=exported
                    report['nativeExport']=decode_native_export(exported)
                elif args.native_export:
                    raise ValueError('Native export telemetry is missing from this build')
            residency=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_PreparedPairResidency'],16),64)))
            report['residency']=dict(zip(RESIDENCY_FIELDS,residency))
            report['selectionCount']=read('CyberpunkVR_PreparedPairSelectionCount')
            report['candidates']=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_PreparedPairCandidates'],16),128)))
            report['stateMismatch']=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_PreparedPairStateMismatch'],16),192)))
            report['vrs']=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_PreparedPairVrs'],16),64)))
            vrs_gpu=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_PreparedPairVrsGpu'],16),32)))
            report['gpuVrs']=dict(zip(VRS_GPU_FIELDS,vrs_gpu))
            gpu=list(struct.unpack('<36Q',reader.read(int(symbols['CyberpunkVR_PreparedPairGpu'],16),288)))
            colors=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_PreparedPairColor'],16),96)))
            baseline=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_PreparedBaseline'],16),192)))
            report['baseline']=dict(zip(BASELINE_FIELDS,baseline))
            native_exit=list(struct.unpack('<24Q',reader.read(int(symbols['CyberpunkVR_PreparedNativeExit'],16),192)))
            report['nativeExit']=dict(zip(NATIVE_EXIT_FIELDS,native_exit))
            native_rate=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_PreparedNativeRate'],16),128)))
            attribution=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_PreparedVrsAttribution'],16),96)))
            report['nativeRateRaw']=native_rate;report['vrsAttributionRaw']=attribution
            report['nativeRate']=dict(zip(NATIVE_RATE_FIELDS,native_rate))
            report['vrsAttribution']=dict(zip(VRS_ATTRIBUTION_FIELDS,attribution))
            acceptance=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_PreparedNativeAcceptance'],16),64)))
            report['nativeAcceptanceRaw']=acceptance
            report['nativeAcceptance']=dict(zip(NATIVE_ACCEPTANCE_FIELDS,acceptance))
            if args.native_tail:
                samples=reader.read(int(symbols['CyberpunkVR_PreparedNativeExitSamples'],16),160*8)
                report['nativeExitSamples']=[dict(zip(NATIVE_EXIT_SAMPLE_FIELDS,row)) for row in struct.iter_unpack('<16Q',samples)]
            depth=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_PreparedPairDepthState'],16),128)))
            report['gpu']={name:dict(zip(GPU_FIELDS,gpu[i*12:(i+1)*12])) for i,name in enumerate(('leftReference','rightReference','paired'))}
            report['colorOccupancy']={name:colors[i*3:(i+1)*3] for i,name in enumerate(('leftReference','rightReference','leftPaired','rightPaired'))}
            report['depthState']=dict(zip(DEPTH_FIELDS,depth))
            descriptor_counts=list(struct.unpack('<4Q',reader.read(int(symbols['CyberpunkVR_PreparedBatchDescriptors'],16),32)))
            report['descriptorBank']=dict(zip(('referenceDescriptors','pairedDescriptors','pairedDraws','extraSnapshots'),descriptor_counts))
            mesh=list(struct.unpack('<12Q',reader.read(int(symbols['CyberpunkVR_PreparedPairMeshStats'],16),96)))
            if mesh[0]>256:
                raise ValueError('GPU mesh results exceed bounded storage')
            mesh_results=[list(row) for row in struct.iter_unpack('<2Q',reader.read(int(symbols['CyberpunkVR_PreparedPairMeshResults'],16),mesh[0]*16))] if mesh[0] else []
            report['gpuMesh']=dict(zip(MESH_FIELDS,mesh))
            report['gpuMeshResults']=mesh_results
            inputs=list(struct.unpack('<8Q',reader.read(int(symbols['CyberpunkVR_PreparedPairInputStats'],16),64)))
            report['gpuInputs']=dict(zip(INPUT_FIELDS,inputs))
            if args.actual_membership:
                from actual_membership_report import POLICY,AND_POLICY
                report['gpuInputPolicy']=POLICY
                report['gpuInputAggregation']=AND_POLICY
                report['inputFaultPolicy']=('All membership bitmaps cleared; actual private B4/T5 unchanged'
                    if args.reject_inputs else 'No input fault requested; actual private B4/T5 unchanged')
            else:report['gpuInputPolicy']='GPU byte comparison against captured selector expectations'
            if args.sequence:
                batch=list(struct.unpack('<16Q',reader.read(int(symbols['CyberpunkVR_PreparedBatchStats'],16),128)))
                source_count=read('CyberpunkVR_PreparedBatchSourceCount')
                plan_count=read('CyberpunkVR_PreparedBatchPlanCount')
                if source_count>512 or plan_count>1024:
                    raise ValueError('Prepared sequence export exceeds its bounded storage')
                sources=[list(row) for row in struct.iter_unpack('<2Q',reader.read(int(symbols['CyberpunkVR_PreparedBatchSources'],16),source_count*16))] if source_count else []
                plan=[list(row) for row in struct.iter_unpack('<4Q',reader.read(int(symbols['CyberpunkVR_PreparedBatchPlan'],16),plan_count*32))] if plan_count else []
                report['batch']=dict(zip(BATCH_FIELDS,batch))
                report['sources']=sources
                report['plan']=plan
            report['referenceVrsPolicy']='GPU common per-axis finer rate, checked after rendering; original game draws unchanged'
            report['referenceBaselinePolicy']='Both variants start from the full pre-interval color, depth and stencil baseline; admitted-source subset diagnostic, not full native pass equivalence'
            if args.native_tail:
                report['referenceBaselinePolicy']='Both variants start immediately before the complete final stage-9 tail and compare all color, depth and stencil planes to its original native exit'
                report['nativeOutputPolicy']=('Owned native export with unconditional roundtrip readback; rejected inputs must preserve the original native exit'
                                              if args.native_export else 'Native output remains unchanged')
            current_matches=not args.observe_current or (current[0]==1 and (args.source_frame is None or current[1]==args.source_frame))
            if not read('CyberpunkVR_PreparedPairRequest') and state in (5, 6) and current_matches:
                report.update(validate_rate_diagnostics(native_rate,attribution,baseline,native_exit))
                if args.observe_current:
                    report['current']=validate_current(current,source_count,args.source_frame,rejected=args.reject_inputs)
                    report['residency']=validate_current_residency(residency,current[13],values[14])
                    if report['actualMembershipAvailable']:
                        from actual_membership_report import validate as validate_membership
                        report['actualMembership']=validate_membership(report['actualMembership'],requested=args.actual_membership,
                            frame=current[1],sources=sources,shared=batch[3],inputs=inputs,rejected=args.reject_inputs)
                        report['actualMembershipVerified']=args.actual_membership
                    if report['nativeExportAvailable']:
                        report['nativeExport']=validate_native_export(report['nativeExportRaw'],requested=args.native_export,
                            frame=current[1],rejected=args.reject_inputs)
                        report['nativeExportVerified']=args.native_export
                        if args.native_export:
                            report['nativeExportComparison']='original native exit' if args.reject_inputs else 'paired output'
                elif args.reject_inputs or args.sequence or state==5 or values[14]:
                    report['residency']=validate_residency(residency,values[14])
                acceptance_options=dict(required=args.native_tail,allow_vrs_refinement=args.allow_vrs_refinement,
                    acceptance=acceptance,rate=native_rate,attribution=attribution,pair_stats=values)
                if args.reject_inputs:
                    report['baseline']=validate_input_rejection(state,values,inputs,mesh,mesh_results,gpu,colors,depth,source_count,baseline,
                        actual_membership=args.actual_membership)
                    report['nativeExit']=validate_native_exit(native_exit,baseline,rejected=True,**acceptance_options)
                    validate_vrs(report['vrs'],vrs_gpu,source_count)
                    report['inputRejectionVerified']=True
                    report['passed']=True
                    break
                if args.sequence:
                    validate_mesh(mesh,mesh_results,batch[3])
                    report['batch']=validate_sequence(state,values,batch,sources,plan,mesh)
                    validate_gpu(gpu,colors,depth)
                    report['baseline']=validate_baseline(baseline,values,gpu,colors)
                    report['nativeExit']=validate_native_exit(native_exit,baseline,**acceptance_options)
                    report['acceptedUnderVrsRefinement']=bool(args.allow_vrs_refinement and not native_exit[21] and acceptance[7])
                    validate_descriptors(descriptor_counts,batch[3])
                    validate_inputs(inputs,source_count,required=True)
                    validate_vrs(report['vrs'],vrs_gpu,source_count)
                    report['passed']=True
                    break
                report['attempts'].append({key:report[key] for key in ('state','stats','candidates','gpu','colorOccupancy','depthState','gpuMesh','gpuMeshResults','gpuInputs','residency','baseline')})
                report['attempts'][-1]['selection']=selection
                if args.all_pairs and exhausted(state,values,selection,report['selectionCount'],len(report['successfulPairs'])):
                    report['exhausted']=True
                    report['passed']=True
                    break
                if state==5:
                    validate(state,values)
                    validate_gpu(gpu,colors,depth)
                    report['baseline']=validate_baseline(baseline,values,gpu,colors)
                    report['nativeExit']=validate_native_exit(native_exit,baseline,**acceptance_options)
                    validate_descriptors(descriptor_counts,1)
                    validate_mesh(mesh,mesh_results,1)
                    validate_inputs(inputs,2)
                    validate_vrs(report['vrs'],vrs_gpu,2)
                    report['successfulPairs'].append(report['attempts'][-1])
                    if not args.all_pairs:
                        report['passed']=True
                        break
                if (retry_empty(state,values) or retry_fallback(state,values,mesh,mesh_results,depth) or (state==5 and args.all_pairs)) and selection+1 < args.attempts and time.monotonic() < deadline:
                    selection+=1
                    write('CyberpunkVR_PreparedPairSelection',selection)
                    write('CyberpunkVR_PreparedPairState',0)
                    write('CyberpunkVR_PreparedPairRequest',1)
                    continue
                if state==5:
                    raise RuntimeError('Prepared pair enumeration exceeded its time or attempt bound')
                validate(state, values)
            if time.monotonic() >= deadline:
                raise TimeoutError('Native pair remains incomplete in the same live process')
            time.sleep(.05)
    except Exception as error:
        report['error'] = str(error)
    finally:
        try:
            if args.observe_current:
                report['cleanup']='observation-only'
            elif kernel.WaitForSingleObject(handle, 0) != 0:
                for name, value in previous.items():
                    write(name, value)
                write('CyberpunkVR_PreparedPairRequest', 0)
                if read('CyberpunkVR_PreparedPairState') in (0, 5, 6):
                    write('CyberpunkVR_PreparedPairState', 0)
                report['cleanup'] = 'restored'
        except Exception as error:
            report['cleanupError'] = str(error)
            report['passed'] = False
        kernel.CloseHandle(handle)
        reader.close()
        args.out.write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
        print(json.dumps(report, indent=2), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
