"""One owned launch: verify focus/gameplay, test native packets, then close the game."""
import argparse
import ctypes as c
import hashlib
import json
import os
from pathlib import Path
import queue
import re
import struct
import subprocess
import sys
import threading
import time

ROOT = Path(__file__).resolve().parents[2]


def insert_retained_bank_check(plan):
    for anchor in ('repeat_owned_reference', 'repeat_owned_graph_sort', 'retained_reference'):
        if anchor in plan:
            plan.insert(plan.index(anchor) + 1, 'retained_source_bank')
            return
    raise ValueError('Source bank check has no retained reference stage')


def require_source_banks(first, second=None):
    from source_assembly_report import require_bank_created
    require_bank_created(first)
    if second is not None:
        require_bank_created(second)


def insert_current_checks(plan):
    plan.insert(plan.index('retained_reference')+1,'prepared_current_initial')
    plan.remove('prepared_input_rejection')
    plan.insert(plan.index('turned_prepared_sequence')+1,'prepared_input_rejection')


def current_observer_flags(capture, *, rejected=False, tail=False, allow_vrs_refinement=False, native_export=False, actual_membership=False):
    stats=capture.get('sourceStream',{}).get('stats',[])
    if len(stats)!=16 or type(stats[1]) is not int or not 0<stats[1]<2**64:
        raise ValueError('Current observation lacks its retained source frame')
    return (['--observe-current','--sequence','--source-frame',str(stats[1])]+
            (['--native-tail'] if tail or capture.get('preparedTailRequested') else [])+
            (['--allow-vrs-refinement'] if allow_vrs_refinement or capture.get('allowVrsRefinement') else [])+
            (['--native-export'] if native_export or capture.get('nativeExportRequested') else [])+
            (['--actual-membership'] if actual_membership or capture.get('actualMembershipRequested') else [])+
            (['--reject-inputs'] if rejected else []))


def validate_current_capture(capture, comparison, *, rejected=False, infix=False, tail=False, allow_vrs_refinement=False, native_export=False, full_t5=False, borrowed_tables=False, fixed_assets=False, actual_membership=False):
    from read_prepared_pair import CURRENT_FIELDS, CURRENT_SOURCE_FIELDS, validate_current, validate_current_source
    if (capture.get('passed') is not True or comparison.get('passed') is not True or
            any(type(value.get('pid')) is not int or value['pid']<=0 or
                not isinstance(value.get('dll'),str) or not value['dll'] for value in (capture,comparison)) or
            (capture.get('pid'),capture.get('dll'))!=(comparison.get('pid'),comparison.get('dll')) or
            comparison.get('observedCurrent') is not True or comparison.get('cleanup')!='observation-only'):
        raise ValueError('Current comparison changed its capture process/build or issued another request')
    if (capture.get('allowVrsRefinement',False)!=allow_vrs_refinement or
            comparison.get('allowVrsRefinement',False)!=allow_vrs_refinement):
        raise ValueError('Current capture changed its explicitly selected VRS refinement policy')
    expected=int(current_observer_flags(capture)[3])
    from full_t5_report import validate_capture as validate_full_t5
    validate_full_t5(capture,full_t5)
    from actual_membership_report import validate_join as validate_actual_membership
    validate_actual_membership(capture,comparison,requested=actual_membership,rejected=rejected)
    if actual_membership and not full_t5:raise ValueError('Actual membership requires complete T5 capture')
    from borrowed_tables_report import validate_capture as validate_borrowed_tables
    validate_borrowed_tables(capture,borrowed_tables)
    from fixed_assets_report import validate_capture as validate_fixed_assets
    validate_fixed_assets(capture,fixed_assets)
    if fixed_assets and not borrowed_tables:raise ValueError('Fixed assets require borrowed-table observation')
    if borrowed_tables and not full_t5:raise ValueError('Borrowed tables require the full T5 observation')
    if full_t5 and not ((tail or capture.get('preparedTailRequested')) and (infix or capture.get('preparedInfixRequested'))):
        raise ValueError('Full T5 lacks the current-frame final-tail infix mode')
    if (capture.get('nativeExportRequested',False) is not native_export or
            comparison.get('nativeExportRequested',False) is not native_export):
        raise ValueError('Current capture changed its explicitly selected native export mode')
    if native_export and not (allow_vrs_refinement and (tail or capture.get('preparedTailRequested')) and
                              (infix or capture.get('preparedInfixRequested') or 'preparedInfix' in capture)):
        raise ValueError('Native export lacks the approved current-frame final-tail infix mode')
    if native_export or capture.get('nativeExportAvailable') or comparison.get('nativeExportAvailable') or 'nativeExportRaw' in comparison:
        from native_export_report import decode as decode_export,validate as validate_export
        observed=decode_export(capture.get('nativeExportRaw'))
        if capture.get('nativeExport')!=observed:
            raise ValueError('Native export capture lost its raw telemetry')
        if observed['requested']!=int(native_export) or observed['sourceFrame'] not in ((expected,) if native_export else (0,expected)):
            raise ValueError('Native export capture belongs to a different request or source frame')
        if not native_export:validate_export(capture['nativeExportRaw'],requested=False,frame=expected)
        validated=validate_export(comparison.get('nativeExportRaw'),requested=native_export,frame=expected,rejected=rejected)
        if comparison.get('nativeExport')!=validated or (native_export and comparison.get('nativeExportVerified') is not True):
            raise ValueError('Native export result omitted its validated native roundtrip')
    current=comparison.get('current',{})
    validate_current([current.get(name) for name in CURRENT_FIELDS],len(comparison.get('sources',[])),expected,rejected=rejected)
    source=capture.get('preparedCurrentSource',{})
    packets=capture.get('stagedSources',{}).get('ready')
    if tail or capture.get('preparedTailRequested') or 'preparedTail' in capture:
        from read_preparation import validate_prepared_tail
        from read_prepared_pair import BASELINE_FIELDS,NATIVE_EXIT_FIELDS,NATIVE_RATE_FIELDS,VRS_ATTRIBUTION_FIELDS,NATIVE_ACCEPTANCE_FIELDS,FIELDS,validate_native_exit
        tail_report=capture.get('preparedTail',{})
        if not isinstance(tail_report,dict):raise ValueError('Current capture omitted prepared tail telemetry')
        tail_report=validate_prepared_tail(tail_report.get('stats'),capture.get('sourceStream'),capture.get('stagedSources'))
        packets=tail_report['sourceCount']
        if comparison.get('sources')!=tail_report['sources']:
            raise ValueError('Current comparison omitted, added or reordered a final tail source')
        native=comparison.get('nativeExit',{});baseline=comparison.get('baseline',{})
        if not isinstance(native,dict) or not isinstance(baseline,dict):raise ValueError('Current tail lacks native exit comparison')
        validate_native_exit([native.get(name) for name in NATIVE_EXIT_FIELDS],
            [baseline.get(name) for name in BASELINE_FIELDS],required=True,rejected=rejected,allow_vrs_refinement=allow_vrs_refinement,
            acceptance=[comparison.get('nativeAcceptance',{}).get(name) for name in NATIVE_ACCEPTANCE_FIELDS],
            rate=[comparison.get('nativeRate',{}).get(name) for name in NATIVE_RATE_FIELDS],
            attribution=[comparison.get('vrsAttribution',{}).get(name) for name in VRS_ATTRIBUTION_FIELDS],
            pair_stats=[comparison.get('stats',{}).get(name) for name in FIELDS])
        infix=True
    validate_current_source([source.get(name) for name in CURRENT_SOURCE_FIELDS],expected,packets)
    if current['sourceCount']!=source['packets']:
        raise ValueError('Current comparison omitted a staged source packet')
    if infix or capture.get('preparedInfixRequested') or 'preparedInfix' in capture:
        from read_preparation import INFIX_FIELDS,validate_prepared_infix
        insertion=capture.get('preparedInfix',{})
        if not isinstance(insertion,dict):raise ValueError('Current capture omitted prepared infix telemetry')
        validate_prepared_infix([insertion.get(name) for name in INFIX_FIELDS],expected,source,packets)
    if rejected and comparison.get('inputRejectionVerified') is not True:
        raise ValueError('Current capture did not verify the requested input rejection')


def run_current_rejection(read_fault, write_fault, capture, observe):
    previous=read_fault()
    if type(previous) is not int or previous not in (0,1):
        raise ValueError('Prepared shader-input fault control differs')
    try:
        write_fault(1)
        source=capture()
        return source,observe(source)
    finally:
        write_fault(previous)


sys.path[:0] = [str(ROOT / 'tools/quickboot_tests'), str(ROOT / 'tools/roomscale_tests'),
                str(ROOT / 'tools/swimming_tests')]
import launch_to_save as boot
from live_read import Reader
from live_symbols import resolve
from preview_settings import PreviewSettings
from hud_readiness import wait_for_stable_hud
from paired_interval import run_paired_interval
from paired_head_turn import HeadTurn,verify_motion
from read_constants import report_summary as constant_summary
from early_depth_report import ALL_DRAWS,draw_limit,validate_skinned_alpha_options
from retained_geometry_report import launch_modes as retained_launch_modes,validate as validate_retained,validate_options as validate_retained_options
from paired_geometry_report import validate as validate_paired_geometry,validate_options as validate_paired_geometry_options


def alpha_depth_flags(*,reference=False,optimized=False,skinned=False):
    flags=(['--alpha-depth-reference'] if reference else [])+(['--alpha-depth-optimized'] if optimized else [])
    return flags+(['--skinned-alpha-depth'] if skinned and flags else [])


def validate_owned_graph_repeat(first, second):
    if not first.get('passed') or not second.get('passed') or (first.get('pid'),first.get('dll'))!=(second.get('pid'),second.get('dll')):
        raise ValueError('Owned graph repeat changed process/build or failed')
    epoch=first['graphJobLease']['epoch']
    if not epoch or second['graphJobLease']['epoch']!=((epoch+1)&0xffffffff or 1):
        raise ValueError('Owned graph repeat did not advance the capture epoch')
    for capture in (first,second):
        jobs=capture['graphJobLease']['stats'];owned=capture['ownedGraphSort']['stats'];resources=capture['resourceUses']['stats']
        if jobs[0]!=3 or jobs[1]!=owned[2] or jobs[5:7]!=[1,0] or jobs[7]!=owned[15] or resources[3]!=owned[15] or resources[0]!=3:
            raise ValueError('Owned graph repeat has stale/incomplete source evidence')
        generation=capture.get('definitionGenerations',{})
        ticket=generation.get('ticket',[]);binding=generation.get('binding',{})
        if (len(ticket)!=4 or ticket[1]!=capture['graphJobLease']['epoch'] or ticket[3]!=7
                or generation.get('coverageLost')!=0 or binding.get('consumerId')!=ticket[0]
                or not all(binding.get(name) for name in ('rootAdmissionProved','executionBindingsProved','terminalCompletionProved'))):
            raise ValueError('Owned graph repeat lacks its native generation consumer proof')
    if second['definitionGenerations']['ticket'][0]<=first['definitionGenerations']['ticket'][0]:
        raise ValueError('Owned graph repeat reused an earlier native consumer')


def validate_owned_reference_repeat(first, second):
    from owned_execution_report import decode,decode_compiler
    if (any(type(capture.get('pid')) is not int or capture['pid']<=0 or not isinstance(capture.get('dll'),str) or
            not capture['dll'] or capture.get('passed') is not True for capture in (first,second)) or
            (first['pid'],first['dll'])!=(second['pid'],second['dll'])):
        raise ValueError('Owned reference repeat changed process/build or failed')
    for capture in (first,second):
        execution=capture['ownedExecution']
        sequence=execution['sequence']
        if execution['closing'] or type(sequence) is not int or not 0<=sequence<=0xffffffff or sequence&1:
            raise ValueError('Owned reference repeat has closing/torn telemetry')
        decode(execution['stats'],execution['resources'])
        decode_compiler(execution['compiler'],execution['stats'],execution['resources'])
        if execution['stats'][2]>0xffffffff:
            raise ValueError('Owned reference repeat has an invalid uint32 epoch')
    before=first['ownedExecution']['stats'];after=second['ownedExecution']['stats']
    if after[2]!=((before[2]+1)&0xffffffff or 1) or after[3]==before[3] or after[1]<=before[1]:
        raise ValueError('Owned reference repeat did not select a fresh epoch/pair/consumer')


def validate_scene_split_repeat(first, second):
    from scene_split_report import validate_capture,validate_publication
    validate_owned_reference_repeat(first,second)
    before,after=validate_capture(first),validate_capture(second)
    if (before['stats'][0],before['stats'][5])!=(after['stats'][0],after['stats'][5]):
        raise ValueError('Scene split repeat changed the selected eye mode')
    publication=validate_publication(second,frontier=second.get('submissionFrontier'),require_frontier=True)
    if after['publication']!=publication:
        raise ValueError('Scene split repeat publication differs from its actual native evidence')
    return after


def run_reversible_capture(turn, variants, capture, screenshot, wait_hud, image):
    """Keep pose restoration and readiness checks identical across native probes."""
    variants['passed']=False
    failed=False
    try:
        screenshot('initial')
        variants['pose']=turn.apply(1)
        variants['hud']=wait_hud()
        captured=capture()
        screenshot('turned')
        turn.apply(0)
        variants['returnHud']=wait_hud()
        screenshot('returned')
        variants['motion']=verify_motion(*(image(label) for label in ('initial','turned','returned')))
    except BaseException:
        failed=True
        raise
    finally:
        try:
            variants['poseCleanup']=turn.restore()
            if variants['poseCleanup'] not in ('restored','unchanged'):
                raise RuntimeError('Head-turn pose restoration was not verified')
            variants['cleanupHud']=wait_hud()
        except Exception as error:
            variants['poseCleanupError']=str(error)
            if not failed:raise
    variants['passed']=True
    return captured


def run_logged(command, timeout, on_line=lambda line: None):
    process = subprocess.Popen([str(v) for v in command], stdout=subprocess.PIPE,
                               stderr=subprocess.STDOUT, text=True, encoding='utf-8', errors='replace',
                               env=dict(os.environ, PYTHONIOENCODING='utf-8'))
    lines, pending = [], queue.Queue()

    def pump():
        for line in process.stdout:
            pending.put(line)
        pending.put(None)

    thread = threading.Thread(target=pump, daemon=True)
    thread.start()
    deadline = time.monotonic() + timeout
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('Child test timed out: ' + str(command[-1]))
            try:
                line = pending.get(timeout=min(.25, remaining))
            except queue.Empty:
                continue
            if line is None:
                break
            lines.append(line)
            print(line.rstrip(), flush=True)
            on_line(line)
        code = process.wait(timeout=max(.1, deadline - time.monotonic()))
        if code:
            raise RuntimeError(f'Test failed with exit {code}: {command}')
        return ''.join(lines)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        thread.join(timeout=1)
        process.stdout.close()


def finish_upload_install(backend, symbols, nonce, on_state, *, sleep=time.sleep):
    import install_upload_observer as installer
    try:
        return installer.install(backend, symbols, expected_nonce=nonce)
    except installer.HeldRecovery as held:
        session = held.session
        previous = None
        while True:
            try:
                state = session.state('recovery_required')
                signature = (state['error'], state['code_valid'], state['event_held'], state['attached'])
                if signature != previous:
                    on_state(state)
                    previous = signature
            except BaseException as error:
                session.error = 'Recovery report failed: ' + str(error)
            try:
                command = 'retry-detach' if session.code_valid else 'terminate'
                return installer.recover_once(session, command, owned_game=True,
                                              terminate_on_unrecoverable=True)
            except BaseException as error:
                session.error = str(error)
                # The debugger must stay alive until its exact target is safe
                # or confirmed terminated; observation deadlines cannot kill it.
                try:
                    sleep(.25)
                except KeyboardInterrupt:
                    pass


def install_disabled_upload_observer(pid, game, out, check):
    import install_upload_observer as installer
    dll = game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll'
    exe = game / 'bin/x64/Cyberpunk2077.exe'
    with exe.open('rb') as stream:
        exe_hash = hashlib.file_digest(stream, 'sha256').hexdigest()
    resolved = resolve(pid, dll, list(installer.SYMBOLS))
    symbols = {name: int(resolved[name], 16) for name in installer.SYMBOLS}
    backend = installer.WindowsBackend(pid, dll, exe, exe_hash, terminate_allowed=True)
    result = None
    path = out / 'upload-observer-install.json'
    try:
        identity = backend.identity()
        if identity.dll_sha256 != resolved['sha256'] or identity.dll_base != int(resolved['module_base'], 16):
            raise RuntimeError('Upload installation DLL identity changed')
        prefix = installer.PREFIX
        if any(installer.read32(backend, symbols[prefix + name]) for name in
               ('PrepareRequest', 'AcceptRequest', 'Enable', 'ExternalInstallState')):
            raise RuntimeError('Upload installation requires a fresh idle process')
        installer.write32(backend, symbols[prefix + 'PrepareRequest'], 1)
        deadline = time.monotonic() + 5
        plan = None
        while time.monotonic() < deadline:
            check()
            phase = installer.read32(backend, symbols[prefix + 'ExternalInstallState'])
            if phase == 1:
                plan = installer.Session(backend, identity, symbols).snapshot()
                break
            if phase > 1:
                raise RuntimeError(f'Native upload preparation refused: {phase}')
            time.sleep(.01)
        if plan is None:
            raise TimeoutError('Native upload preparation did not publish a plan')
        result = finish_upload_install(backend, symbols, plan.words[1],
                                       lambda state: installer.save_state(path, state))
        installer.save_state(path, result)
        if result['outcome'] != 'committed_detached_accept_requested':
            raise RuntimeError('Upload installation did not complete: ' + result['outcome'])
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            check()
            phase = installer.read32(backend, symbols[prefix + 'ExternalInstallState'])
            if phase == 2 and not installer.read32(backend, symbols[prefix + 'AcceptRequest']):
                if installer.read32(backend, symbols[prefix + 'Enable']):
                    raise RuntimeError('Installation unexpectedly enabled the observer')
                result['nativeAcknowledged'] = True
                installer.save_state(path, result)
                return result
            if phase > 2:
                raise RuntimeError(f'Native upload installation refused its receipt: {phase}')
            time.sleep(.01)
        raise TimeoutError('Native upload installation did not acknowledge the receipt')
    finally:
        backend.close()


def close_game(pid, executable):
    k = boot.kernel32
    k.WaitForSingleObject.argtypes = [c.c_void_p, c.c_uint32]
    k.WaitForSingleObject.restype = c.c_uint32
    handle = k.OpenProcess(0x101001, False, pid)  # synchronize, query, terminate
    if not handle:
        if pid in boot.process_ids('Cyberpunk2077.exe'):
            raise RuntimeError('Could not open the still-running game for shutdown')
        return 'already_exited'
    try:
        path, length = c.create_unicode_buffer(32768), c.c_uint32(32768)
        if not k.QueryFullProcessImageNameW(handle, 0, path, c.byref(length)):
            if k.WaitForSingleObject(handle, 0) == 0:
                return 'already_exited'
            raise c.WinError(c.get_last_error())
        if Path(path.value).resolve() != executable.resolve():
            raise RuntimeError('Refusing to close a PID that no longer identifies this game')
        for row in boot.windows_for(pid):
            if row['class'] in (boot.GAME_CLASS, boot.LAUNCHER_CLASS):
                boot.user32.PostMessageW(row['hwnd'], 0x10, 0, 0)
        if k.WaitForSingleObject(handle, 5000) == 0:
            return 'closed'
        if not k.TerminateProcess(handle, 0) or k.WaitForSingleObject(handle, 10000) != 0:
            raise RuntimeError('Game did not exit after the tests')
        return 'terminated_after_close_timeout'
    finally:
        k.CloseHandle(handle)


def observe_shutdown(pid, started, baseline_reporters, seconds=5):
    """Report writers/reporters may outlive the game process that faulted."""
    deadline=time.monotonic()+seconds
    crash=None;reporters=set()
    while True:
        crash=boot.crash_report(pid,started) or crash
        current=boot.new_reporters(baseline_reporters)
        reporters.update(current)
        if current:boot.close_reporters(current)
        if time.monotonic()>=deadline:break
        time.sleep(.2)
    return dict(crash=crash,reporters=sorted(reporters))


def check_both_eyes(pixels):
    if len(pixels) < 54 or pixels[:2] != b'BM':
        raise ValueError('Not a BMP screenshot')
    offset, = struct.unpack_from('<I', pixels, 10)
    width, height = struct.unpack_from('<ii', pixels, 18)
    bpp, = struct.unpack_from('<H', pixels, 28)
    compression, = struct.unpack_from('<I', pixels, 30)
    if bpp not in (24, 32) or compression != 0 or width < 32 or abs(height) < 32 or width % 2:
        raise ValueError('Unsupported stereo screenshot layout')
    stride, step = ((width * bpp + 31) // 32) * 4, bpp // 8
    h, eye_width = abs(height), width // 2
    if offset + stride * h > len(pixels):
        raise ValueError('Truncated screenshot')
    fractions = []
    for eye in range(2):
        lit = count = 0
        for y in range(h // 2, h * 9 // 10, max(1, h // 40)):
            row = h - 1 - y if height > 0 else y
            for x in range(eye_width // 10, eye_width * 9 // 10, max(1, eye_width // 40)):
                at = offset + row * stride + (eye * eye_width + x) * step
                lit += max(pixels[at:at + 3]) > 8
                count += 1
        fractions.append(lit / count)
    if min(fractions) <= .1:
        raise ValueError(f'One or both scene images are black: {fractions}')
    return dict(width=width, height=h, nonblackFraction=fractions)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=180)
    parser.add_argument('--install-upload-observer', action='store_true',
                        help='Install the disabled reservation observer under an external debugger event')
    parser.add_argument('--native-draws', action='store_true', help='Correlate resolved packets with actual D3D draw bindings')
    parser.add_argument('--family-manifest', type=Path, help='Create/census linked native stereo families in this launch only')
    parser.add_argument('--source-selector-manifest',type=Path,help='Track original b4 producer intent and compact current descriptor slots')
    parser.add_argument('--native-constants', action='store_true', help='Own/revalidate used constant data and construct a paired bank')
    parser.add_argument('--freeze-geometry', action='store_true', help='Acquire eligible geometry at the actual GPU draw position')
    parser.add_argument('--compare-depth', action='store_true', help='Compare original/paired game shaders on the owned geometry')
    parser.add_argument('--preparation-trace', action='store_true', help='Capture CPU preparation stages and native pass consumption')
    parser.add_argument('--cpu-collector', action='store_true', help='Initialize/destroy an owned native CPU collector without starting jobs')
    parser.add_argument('--cpu-query', action='store_true', help='Also run and verify private native CPU query tasks')
    parser.add_argument('--cpu-fine', action='store_true', help='Compare fine visibility using actual native candidate ranges and mesh IDs')
    parser.add_argument('--cpu-scene', action='store_true', help='Compare full native scene-pool traversal using owned candidates')
    parser.add_argument('--cpu-peer', action='store_true', help='Compare early MAIN and VRCAM visibility against the native same-frame output')
    parser.add_argument('--bounds-trace', action='store_true', help='Observe native bounds writes during the peer CPU probe')
    parser.add_argument('--cpu-repeat', type=int, choices=range(1,9), default=1, help='Repeat the owned CPU probe within the same launch')
    parser.add_argument('--check-pair-admission', '--pair-prepare', dest='pair_prepare', action='store_true',
                        help='Check rejection of an unowned paired-output graph')
    parser.add_argument('--main-root-pair', action='store_true', help='Test one output-owning MAIN root with both registered views')
    parser.add_argument('--merge-root', action='store_true', help='Verify a warmup then replace one standalone VRCAM root with paired MAIN')
    parser.add_argument('--merge-root-frames', type=int, choices=range(1,8), default=1, help='Consecutive bounded merged frames, followed by normal rendering')
    parser.add_argument('--paired-interval', action='store_true', help='After the bounded warmup, verify 1200 paired frames and images while pairing remains active')
    parser.add_argument('--paired-head-turn', action='store_true', help='Also test and restore a ten-degree simulator head turn during the paired interval')
    parser.add_argument('--eager-depth',choices=('compare','replace'),help='Prepare VRCAM depth before MAIN; replace first verifies a full comparison interval')
    parser.add_argument('--graph-plan',action='store_true',help='Capture the merged native dependency graph')
    parser.add_argument('--schedule-preparation',choices=('check','apply'),help='Check or apply the bounded early CPU preparation schedule')
    parser.add_argument('--paired-cameras',action='store_true',help='Verify early VRCAM camera constants for the shared depth pass')
    parser.add_argument('--paired-camera-turn',action='store_true',help='Also require a changed native pose/history during the camera comparison')
    parser.add_argument('--early-depth-gpu',type=draw_limit,help='Compare this many, or all, compatible depth draws with same-frame native references')
    parser.add_argument('--whole-depth-target',action='store_true',help='Also route and compare the complete native depth target')
    parser.add_argument('--reflection-dxc',type=Path,help='Optional explicit dxcompiler.dll for unconverted depth shader reflection')
    parser.add_argument('--optimized-depth',action='store_true',help='Run the bounded no-reference depth route')
    parser.add_argument('--reject-depth-commit',action='store_true',help='Test the no-reference route with native fallback forced')
    parser.add_argument('--depth-benchmark',type=int,choices=range(2,17),help='After exact reference validation, alternate this many native/optimized GPU timing pairs in the same launch')
    parser.add_argument('--observer-profile',action='store_true',help='Measure eight seconds of metadata/constant/family observer overhead before captures')
    parser.add_argument('--schedule-benchmark',action='store_true',help='Measure four 240-frame CPU schedule intervals with the plan cache off/on/on/off')
    parser.add_argument('--early-depth-interval',type=int,choices=range(240,1201),help='After exact reference validation, test this many continuous depth frames and a cancelled interval')
    parser.add_argument('--depth-variants',action='store_true',help='Also compare full depth after a reversible head turn and exercise forced native fallback')
    parser.add_argument('--origin-benchmark',action='store_true',help='Also run a legacy-unwind interval before the scoped-origin interval in the same launch')
    parser.add_argument('--instance-benchmark',action='store_true',help='Compare per-draw and combined instance read states in the same owned launch')
    parser.add_argument('--descriptor-benchmark',action='store_true',help='Compare sparse/dense/dense/sparse descriptor metadata during verified depth intervals')
    parser.add_argument('--native-camera',action='store_true',help='Keep native heaps and use the audited MAIN-prefix two-camera shader')
    parser.add_argument('--native-camera-stages',type=int,choices=(1,3),default=1,help='Exercise VS only (1) or scoped VS+PS binding/restoration (3)')
    parser.add_argument('--skinning-depth',action='store_true',help='Exercise exact native skinning resources in merged depth')
    parser.add_argument('--alpha-depth-reference',action='store_true',help='Render exact static alpha family in the paired reference target')
    parser.add_argument('--alpha-depth-optimized',action='store_true',help='Capture material hints, then test optimized static alpha and native fallback')
    parser.add_argument('--skinned-alpha-depth',action='store_true',help='Explicitly exercise skinned alpha with skinning and an alpha depth mode')
    parser.add_argument('--material-depth',action='store_true',help='Capture alpha material GPU bytes in a whole-depth reference')
    parser.add_argument('--pass-targets',action='store_true',help='Record native geometry target identities and submission dependencies in a paired root')
    parser.add_argument('--retained-geometry',choices=('reference','replace'),help='Validate retained native geometry; replace first runs its reference in this launch')
    parser.add_argument('--retained-stage',type=int,choices=(6,7),default=7)
    parser.add_argument('--paired-gbuffer',action='store_true',help='Test a delayed paired native G-buffer draw without suppressing native eyes')
    parser.add_argument('--paired-gbuffer-family',type=int,choices=(40,70),default=40,help='Exact audited native reference shader pair')
    parser.add_argument('--owned-graph-sort',action='store_true',help='Compare native sorting on owned graph arrays; never redirect native jobs')
    parser.add_argument('--whole-gbuffer-plan',action='store_true',help='Repeat native whole-discard sorting and CPU context-prefix validation without redirecting execution')
    parser.add_argument('--retained-source-stream',action='store_true',help='Own every source occurrence in both complete native geometry intervals')
    parser.add_argument('--retained-ia-snapshots',action='store_true',help='Observe and copy admissible full source IA views without changing draw coordinates')
    parser.add_argument('--retained-source-bank',action='store_true',help='Require actual GPU-bank creation for every source occurrence, with no native execution')
    parser.add_argument('--prepared-pair',action='store_true',help='Compare an owned-input native view-instanced draw after both source captures')
    parser.add_argument('--prepared-sequence',action='store_true',help='Compare an ordered mixed-eye source sequence in persistent texture arrays')
    parser.add_argument('--prepared-sequence-turn',action='store_true',help='Recapture and compare the prepared sequence after a reversible simulator head turn')
    parser.add_argument('--prepared-current',action='store_true',help='Observe owned current-frame sequences for initial, repeat, head-turn and fault-injected captures')
    parser.add_argument('--prepared-infix',action='store_true',help='Verify diagnostic owned consumer insertion between original native lists for every current-frame capture')
    parser.add_argument('--prepared-tail',action='store_true',help='Verify each complete final stage-9 source tail against its original native exit')
    parser.add_argument('--allow-vrs-refinement',action='store_true',help='Accept the approved shared finer VRS map with complete native-rate attribution proof')
    parser.add_argument('--native-export',action='store_true',help='Require owned native output copies in initial, repeat, turn and rejected-input captures')
    parser.add_argument('--full-t5',action='store_true',help='Require independently compared complete T5 views in all four current-frame captures')
    parser.add_argument('--actual-membership',action='store_true',help='Use actual private B4/T5 membership in all four current-frame captures')
    parser.add_argument('--borrowed-tables',action='store_true',help='Require descriptor no-overwrite witnesses in all four current-frame captures')
    parser.add_argument('--fixed-assets',action='store_true',help='Exercise fixed native allocation bootstrap across borrowed dynamic ranges in all four captures')
    parser.add_argument('--owned-coupled',action='store_true',help='One-frame experimental coupled CPU/GPU graph execution, without source snapshots or native draw suppression')
    parser.add_argument('--scene-split',action='store_true',help='Execute and repeat a one-eye native SceneDriver split, preserving all original draw work')
    parser.add_argument('--scene-split-eyes',choices=('one','both'),default='one',help='Select one peer eye or both eyes; both implies --scene-split')
    parser.add_argument('--scene-access',action='store_true',help='Observe peer-tail/main-prefix operands in initial, repeat and turned captures')
    parser.add_argument('--retained-indirect-snapshots',action='store_true',help='Freeze each indirect argument at its actual call and use it in the array reference')
    parser.add_argument('--retained-source-cbvs',action='store_true',help='Verify fresh native backing ownership for every reflected source CBV')
    parser.add_argument('--retained-source-tables',action='store_true',help='Verify immutable finite source descriptor tables; keep bindless domains deferred')
    parser.add_argument('--retained-source-details',action='store_true',help='Observe actual CBV resource heap, creation and native allocation/state metadata')
    parser.add_argument('--retained-cbv-snapshots',action='store_true',help='Verify original-occurrence DEFAULT CBV copies and completed GPU payload readback')
    parser.add_argument('--retained-upload-snapshots','--retained-direct-upload',dest='retained_direct_upload',action='store_true',help='Also copy qualified completed UPLOAD writers in their exact original queued epoch')
    parser.add_argument('--owned-reference',action='store_true',help='Execute and repeat an owned graph with unchanged native dependencies')
    parser.add_argument('--owned-retirement',action='store_true',help='Also exercise real native cache eviction/destruction and restore all test CVars')
    parser.add_argument('--retirement-cycles',type=int,choices=range(1,9),default=1,help='Repeat native retirement and reference restoration in the same process')
    parser.add_argument('--camera-benchmark',action='store_true',help='Compare private banks with native camera bindings in one launch')
    parser.add_argument('--metadata-benchmark',action='store_true',help='Compare full/live/live/full descriptor metadata with the same native-camera depth route')
    parser.add_argument('--upload-benchmark',action='store_true',help='Compare legacy/receipt/receipt/legacy upload lookup in the same native-camera launch')
    parser.add_argument('--geometry-cost',action='store_true',help='Measure the single-pass ceiling: per-eye RenderElements vertex-only/skip intervals in this launch')
    args = parser.parse_args()
    if args.scene_split_eyes=='both':args.scene_split=True
    from scene_access_report import validate_options as validate_scene_access_options
    try:validate_scene_access_options(args.scene_access,eyes=args.scene_split_eyes,retained_geometry=args.retained_geometry,pass_targets=args.pass_targets)
    except ValueError as error:parser.error(str(error))
    from scene_split_report import validate_options as validate_scene_split_options
    try:validate_scene_split_options(args.scene_split,prepared_current=args.prepared_current,prepared_infix=args.prepared_infix,
        native_export=args.native_export,prepared_pair=args.prepared_pair,prepared_sequence=args.prepared_sequence,
        source_stream=args.retained_source_stream)
    except ValueError as error:parser.error(str(error))
    if args.scene_split:args.owned_coupled=True
    if args.actual_membership and not (args.full_t5 and args.prepared_current and args.prepared_infix and args.prepared_tail):
        parser.error('--actual-membership requires --full-t5, --prepared-current, --prepared-infix and --prepared-tail')
    if args.fixed_assets and not args.borrowed_tables:parser.error('--fixed-assets requires --borrowed-tables')
    if args.borrowed_tables and not (args.full_t5 and args.prepared_current and args.prepared_infix and args.prepared_tail):
        parser.error('--borrowed-tables requires --full-t5, --prepared-current, --prepared-infix and --prepared-tail')
    if args.full_t5 and not (args.prepared_current and args.prepared_infix and args.prepared_tail):
        parser.error('--full-t5 requires --prepared-current, --prepared-infix and --prepared-tail')
    if args.native_export and not (args.prepared_current and args.prepared_infix and args.prepared_tail and args.allow_vrs_refinement):
        parser.error('--native-export requires --prepared-current, --prepared-infix, --prepared-tail and --allow-vrs-refinement')
    if args.allow_vrs_refinement and not args.prepared_tail:parser.error('--allow-vrs-refinement requires --prepared-tail')
    if args.prepared_tail and not args.prepared_infix:parser.error('--prepared-tail requires --prepared-infix')
    if args.prepared_infix and not (args.prepared_current and args.owned_coupled):
        parser.error('--prepared-infix requires --prepared-current and --owned-coupled')
    if args.prepared_current:
        if args.prepared_pair or args.owned_retirement:
            parser.error('--prepared-current cannot share legacy pair requests or retirement cycles')
        args.prepared_sequence=True;args.prepared_sequence_turn=True
    if args.owned_coupled:
        args.owned_reference=True
        if args.whole_gbuffer_plan or args.owned_retirement:
            parser.error('--owned-coupled cannot apply the legacy graph plan or retirement tests')
    if args.whole_gbuffer_plan:
        args.owned_reference=True
        if args.owned_graph_sort:parser.error('--whole-gbuffer-plan uses owned reference execution, not legacy diagnostic source tickets')
        if args.retained_geometry!='reference' or args.retained_stage!=7 or not args.pass_targets:
            parser.error('--whole-gbuffer-plan requires retained reference stage7 and pass targets')
    if args.retained_source_stream and not (args.whole_gbuffer_plan or args.owned_coupled):parser.error('--retained-source-stream requires an owned whole-pass schedule')
    if args.retained_source_stream and args.owned_coupled and args.retained_stage!=7:parser.error('Coupled source capture requires the whole G-buffer discard pass')
    if args.retained_ia_snapshots and not args.retained_source_stream:parser.error('--retained-ia-snapshots requires --retained-source-stream')
    if args.retained_indirect_snapshots and not args.retained_source_stream:parser.error('--retained-indirect-snapshots requires --retained-source-stream')
    if args.retained_source_cbvs and not args.retained_source_stream:parser.error('--retained-source-cbvs requires --retained-source-stream')
    if args.retained_source_tables and not args.retained_source_cbvs:parser.error('--retained-source-tables requires --retained-source-cbvs')
    if args.retained_source_details and not args.retained_source_cbvs:parser.error('--retained-source-details requires --retained-source-cbvs')
    if args.retained_cbv_snapshots and not (args.retained_source_tables and args.retained_source_details):parser.error('--retained-cbv-snapshots requires --retained-source-tables and --retained-source-details')
    if args.retained_direct_upload and not args.retained_cbv_snapshots:parser.error('--retained-direct-upload requires --retained-cbv-snapshots')
    if args.source_selector_manifest and (not args.retained_cbv_snapshots or args.native_constants or not args.source_selector_manifest.is_file()):
        parser.error('--source-selector-manifest requires original CBV snapshots, an existing catalog and no legacy constant capture')
    if args.retained_source_bank and (not args.source_selector_manifest or args.retained_geometry!='reference'):
        parser.error('--retained-source-bank requires source selector snapshots and retained reference geometry')
    if (args.prepared_pair or args.prepared_sequence) and (not args.source_selector_manifest or not args.retained_ia_snapshots or args.retained_geometry!='reference'):
        parser.error('Prepared native comparisons require retained reference, source selectors and owned IA snapshots')
    if args.prepared_sequence_turn and (not args.prepared_sequence or not (args.whole_gbuffer_plan or args.owned_coupled) or args.owned_retirement):
        parser.error('--prepared-sequence-turn requires --prepared-sequence, an owned whole-pass schedule and no retirement cycle')
    if args.retirement_cycles!=1 and not args.owned_retirement:parser.error('--retirement-cycles requires --owned-retirement')
    if args.owned_retirement:args.owned_reference=True
    if args.owned_reference and (args.owned_graph_sort or args.retained_geometry!='reference' or not args.pass_targets):
        parser.error('--owned-reference requires retained reference geometry and pass targets, without diagnostic owned sorting')
    if args.paired_gbuffer_family!=40 and not args.paired_gbuffer:parser.error('--paired-gbuffer-family requires --paired-gbuffer')
    if args.owned_graph_sort and (not args.pass_targets or not(args.retained_geometry or args.paired_gbuffer)):
        parser.error('--owned-graph-sort requires a complete paired geometry/graph capture')
    try:validate_retained_options(args.retained_geometry,args.retained_stage,pass_targets=args.pass_targets,
        paired_apply=args.schedule_preparation!='check',
        conflicts=(args.early_depth_gpu,args.whole_depth_target,args.optimized_depth,args.reject_depth_commit,args.native_camera,args.native_camera_stages!=1,
                   args.skinning_depth,args.alpha_depth_reference,args.alpha_depth_optimized,args.skinned_alpha_depth,args.material_depth,
                   args.native_constants,args.freeze_geometry,args.compare_depth,args.paired_interval,args.paired_head_turn,args.eager_depth,
                   args.depth_benchmark,args.early_depth_interval,args.depth_variants,args.schedule_benchmark,args.origin_benchmark,
                   args.instance_benchmark,args.descriptor_benchmark,args.camera_benchmark,args.metadata_benchmark,args.upload_benchmark,
                   args.paired_cameras,args.paired_camera_turn,args.observer_profile,args.merge_root_frames!=1))
    except ValueError as error:parser.error(str(error))
    if args.retained_geometry:args.native_draws=True;args.schedule_preparation='apply'
    try:validate_paired_geometry_options(args.paired_gbuffer,pass_targets=args.pass_targets,
        paired_apply=args.schedule_preparation!='check',conflicts=(args.retained_geometry,args.early_depth_gpu,args.native_constants,
        args.freeze_geometry,args.compare_depth,args.paired_interval,args.paired_head_turn,args.depth_benchmark,args.paired_cameras,
        args.paired_camera_turn,args.merge_root_frames!=1,args.whole_depth_target,args.optimized_depth,args.reject_depth_commit,
        args.native_camera,args.native_camera_stages!=1,args.skinning_depth,args.alpha_depth_reference,args.alpha_depth_optimized,
        args.skinned_alpha_depth,args.material_depth,args.eager_depth,args.early_depth_interval,args.depth_variants,
        args.schedule_benchmark,args.origin_benchmark,args.instance_benchmark,args.descriptor_benchmark,args.camera_benchmark,
        args.metadata_benchmark,args.upload_benchmark,args.observer_profile))
    except ValueError as error:parser.error(str(error))
    if args.paired_gbuffer:
        if not args.family_manifest:parser.error('--paired-gbuffer requires --family-manifest')
        args.native_draws=True;args.schedule_preparation='apply'
    if args.origin_benchmark:
        if args.schedule_benchmark:parser.error('Choose one CPU benchmark')
        args.early_depth_interval=args.early_depth_interval or 240
    if args.instance_benchmark:
        if args.origin_benchmark or args.schedule_benchmark:parser.error('Choose one isolated CPU/GPU benchmark')
        args.early_depth_interval=args.early_depth_interval or 240
    if args.descriptor_benchmark:
        if args.origin_benchmark or args.instance_benchmark or args.schedule_benchmark:parser.error('Choose one isolated CPU/GPU benchmark')
        args.early_depth_interval=args.early_depth_interval or 240
    if args.camera_benchmark:
        if args.descriptor_benchmark or args.origin_benchmark or args.instance_benchmark or args.schedule_benchmark:parser.error('Choose one isolated benchmark')
        args.native_camera=True;args.early_depth_interval=args.early_depth_interval or 240
    if args.metadata_benchmark:
        if args.camera_benchmark or args.descriptor_benchmark or args.origin_benchmark or args.instance_benchmark or args.schedule_benchmark:parser.error('Choose one isolated benchmark')
        args.native_camera=True;args.early_depth_interval=args.early_depth_interval or 240
    if args.upload_benchmark:
        if args.metadata_benchmark or args.camera_benchmark or args.descriptor_benchmark or args.origin_benchmark or args.instance_benchmark or args.schedule_benchmark:parser.error('Choose one isolated benchmark')
        args.native_camera=True;args.early_depth_interval=args.early_depth_interval or 240
    if args.schedule_benchmark:
        if (args.schedule_preparation=='check' or args.early_depth_interval or args.paired_interval or args.paired_head_turn or
                args.eager_depth or args.native_draws or args.early_depth_gpu or args.depth_benchmark or args.family_manifest):
            parser.error('Schedule benchmark requires applied preparation and owns its paired intervals')
        args.schedule_preparation='apply'
    if args.early_depth_interval:
        if args.optimized_depth or args.reject_depth_commit or args.early_depth_gpu not in (None,ALL_DRAWS):parser.error('Continuous depth requires an all-draw reference warmup')
        args.early_depth_gpu=ALL_DRAWS;args.whole_depth_target=True
    if args.depth_variants:
        if args.optimized_depth or args.reject_depth_commit or args.early_depth_gpu not in (None,ALL_DRAWS):parser.error('Depth variants require an all-draw reference warmup')
        args.early_depth_gpu=ALL_DRAWS;args.whole_depth_target=True
    if args.observer_profile and (not args.native_draws and not args.early_depth_gpu and not args.depth_benchmark):parser.error('Observer profiling requires native binding observation')
    if args.depth_benchmark:
        if args.optimized_depth or args.reject_depth_commit or args.early_depth_gpu not in (None,ALL_DRAWS):parser.error('Depth benchmark requires an all-draw reference, not forced fallback')
        args.early_depth_gpu=ALL_DRAWS;args.whole_depth_target=True
    if args.alpha_depth_optimized:
        if args.alpha_depth_reference or args.optimized_depth or args.early_depth_interval or args.depth_variants or args.early_depth_gpu not in (None,ALL_DRAWS):
            parser.error('Optimized alpha uses its own material/reference/fallback sequence')
        args.native_camera=True;args.early_depth_gpu=ALL_DRAWS;args.whole_depth_target=True;args.material_depth=True
    if args.alpha_depth_reference:
        if args.optimized_depth or args.early_depth_interval or args.material_depth or args.early_depth_gpu not in (None,ALL_DRAWS):
            parser.error('Alpha reference requires all-draw reference mode without optimized/interval/material capture')
        args.native_camera=True;args.early_depth_gpu=ALL_DRAWS;args.whole_depth_target=True
    if args.optimized_depth:args.whole_depth_target=True
    if args.reject_depth_commit and not args.optimized_depth:parser.error('--reject-depth-commit requires --optimized-depth')
    if args.whole_depth_target and not args.early_depth_gpu:parser.error('--whole-depth-target requires --early-depth-gpu')
    if args.reflection_dxc and not args.family_manifest:parser.error('--reflection-dxc requires --family-manifest')
    if args.early_depth_gpu:
        if not args.family_manifest or args.native_constants:parser.error('Early depth requires --family-manifest and no frozen/late constant capture')
        args.native_draws=True;args.schedule_preparation='apply'
    if args.pass_targets and not (args.early_depth_gpu or args.retained_geometry or args.paired_gbuffer):parser.error('--pass-targets requires a geometry capture')
    if args.native_camera_stages==3 or args.skinning_depth:args.native_camera=True
    if args.native_camera and not args.early_depth_gpu:parser.error('--native-camera requires an early-depth test')
    if args.material_depth and (not args.whole_depth_target or args.optimized_depth or args.early_depth_interval or args.depth_variants):
        parser.error('--material-depth requires one whole-depth reference without interval/variant modes')
    if args.paired_camera_turn:args.paired_cameras=True
    if args.paired_cameras:
        if args.schedule_preparation=='check':parser.error('Paired cameras require actual early preparation')
        args.schedule_preparation='apply'
    if args.merge_root_frames>3 and not args.paired_cameras:parser.error('More than three merged frames requires --paired-cameras')
    if args.schedule_preparation:args.graph_plan=True
    if args.eager_depth:args.paired_interval=True
    if args.graph_plan:args.merge_root=True
    if args.paired_head_turn:args.paired_interval=True
    if args.paired_interval:args.merge_root=True
    if args.paired_interval and args.schedule_preparation=='check':parser.error('Use schedule-preparation apply for an active interval')
    if args.merge_root_frames!=1 and not args.merge_root:parser.error('--merge-root-frames requires --merge-root')
    if args.merge_root:
        if args.main_root_pair:parser.error('Choose main-root-pair or merge-root')
        args.main_root_pair=True
    if args.main_root_pair:
        if args.pair_prepare:parser.error('Choose admission-only or MAIN-root paired selection')
        args.pair_prepare=True
    if args.cpu_peer:
        args.cpu_scene=True
    if args.cpu_scene:
        args.cpu_fine=True
    if args.cpu_fine:
        args.cpu_query=True
    if args.cpu_query:
        args.cpu_collector=True
    if args.cpu_repeat>1 and not args.cpu_collector:
        parser.error('--cpu-repeat requires a CPU collector/query/fine/scene test')
    if args.bounds_trace and not args.cpu_peer:
        parser.error('--bounds-trace requires --cpu-peer')
    if args.pair_prepare:
        args.preparation_trace = True
    if args.native_constants and (not args.family_manifest or not args.native_draws):
        parser.error('--native-constants requires --family-manifest and --native-draws')
    if args.freeze_geometry and not args.native_constants:
        parser.error('--freeze-geometry requires --native-constants')
    if args.compare_depth and not args.freeze_geometry:
        parser.error('--compare-depth requires --freeze-geometry')
    try:validate_skinned_alpha_options(args.skinned_alpha_depth,args.skinning_depth,args.alpha_depth_reference,args.alpha_depth_optimized,
        continuous=bool(args.early_depth_interval or args.paired_interval or args.schedule_benchmark or args.depth_benchmark))
    except ValueError as error:parser.error(str(error))
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if boot.process_ids('Cyberpunk2077.exe') or boot.process_ids('qrenderdoc.exe') or boot.process_ids('renderdoccmd.exe'):
        raise RuntimeError('Close the game and capture replay before this owned launch')
    game, pid, reader = boot.DEFAULT_GAME, None, None
    symbols, startup = {}, None
    plan = ['focus_and_save', 'stable_hud', 'resolved_packets', 'both_eye_images', 'four_frame_packets',
            'repeat_packet_request', 'cpu_prepare', 'controls_restored', 'crash_stability']
    if args.install_upload_observer:
        plan.insert(plan.index('stable_hud') + 1, 'upload_observer_install')
    if args.native_draws:
        plan.insert(plan.index('resolved_packets') + 1, 'native_instances_verified')
        plan.insert(plan.index('native_instances_verified') + 1, 'descriptor_capture')
    if args.family_manifest:
        plan.insert(plan.index('stable_hud') + 1, 'family_registry')
    if args.observer_profile:plan.insert(plan.index('resolved_packets'),'observer_profile')
    if args.native_constants:
        plan.insert(plan.index('descriptor_capture') + 1, 'constant_bank')
        plan.insert(plan.index('constant_bank'), 'geometry_pairing')
        plan.insert(plan.index('constant_bank') + 1, 'geometry_ownership')
        plan.insert(plan.index('geometry_ownership') + 1, 'full_instance_batches')
        if args.compare_depth:
            plan.insert(plan.index('constant_bank') + 1, 'original_shader_depth')
    report = dict(planned=plan, completed=[], passed=False, preparedInfixRequested=args.prepared_infix, preparedTailRequested=args.prepared_tail,
                  allowVrsRefinement=args.allow_vrs_refinement,nativeExportRequested=args.native_export,fullT5Requested=args.full_t5,
                  borrowedTablesRequested=args.borrowed_tables,fixedAssetsRequested=args.fixed_assets,actualMembershipRequested=args.actual_membership,
                  sceneSplitRequested=args.scene_split,sceneSplitEyes=args.scene_split_eyes if args.scene_split else None,
                  sceneAccessRequested=args.scene_access)
    if args.preparation_trace:
        if args.merge_root:
            plan.insert(plan.index('both_eye_images'),'pair_warmup')
            plan.insert(plan.index('both_eye_images'),'warmup_hud')
        plan.insert(plan.index('both_eye_images'), 'preparation_order')
        if args.retained_geometry:
            for index,mode in enumerate(retained_launch_modes(args.retained_geometry)):
                plan.insert(plan.index('preparation_order')+1+index,'retained_'+mode)
            if args.owned_graph_sort:plan.insert(plan.index('both_eye_images'),'repeat_owned_graph_sort')
            if args.owned_reference:
                plan.insert(plan.index('preparation_order')+1,'owned_reference')
                plan.insert(plan.index('both_eye_images'),'repeat_owned_reference')
                if args.scene_split:
                    plan.insert(plan.index('owned_reference')+1,'native_scene_split')
                    plan.insert(plan.index('repeat_owned_reference')+1,'repeat_native_scene_split')
                    plan.insert(plan.index('repeat_native_scene_split')+1,'turned_native_scene_split')
                    if args.scene_access:
                        for anchor,name in (('native_scene_split','initial_scene_access'),('repeat_native_scene_split','repeat_scene_access'),
                                            ('turned_native_scene_split','turned_scene_access')):
                            plan.insert(plan.index(anchor)+1,name)
                if args.whole_gbuffer_plan:plan.insert(plan.index('owned_reference')+1,'whole_gbuffer_plan')
            if args.retained_source_bank:insert_retained_bank_check(plan)
            for name,enabled in (('prepared_pair',args.prepared_pair),('prepared_sequence',args.prepared_sequence)):
                if not enabled:continue
                anchor='retained_source_bank' if args.retained_source_bank else 'both_eye_images'
                plan.insert(plan.index(anchor),name)
                if name=='prepared_sequence':plan.insert(plan.index(name)+1,'prepared_input_rejection')
            if args.owned_retirement:
                index=plan.index('retained_source_bank' if args.retained_source_bank else 'repeat_owned_reference')+1
                plan[index:index]=['native_retirement','restored_owned_reference']
                for cycle in range(2,args.retirement_cycles+1):
                    index=plan.index('post_pair_hud') if 'post_pair_hud' in plan else plan.index('both_eye_images')
                    plan[index:index]=[f'native_retirement_{cycle}',f'restored_owned_reference_{cycle}']
            if args.prepared_sequence_turn:
                anchor='retained_source_bank' if args.retained_source_bank else 'prepared_sequence'
                plan.insert(plan.index(anchor)+1,'turned_prepared_sequence')
            if args.prepared_current:insert_current_checks(plan)
        if args.paired_gbuffer:plan.insert(plan.index('preparation_order')+1,'paired_gbuffer')
        if args.pair_prepare:
            plan.insert(plan.index('both_eye_images'), 'post_pair_hud')
        if args.depth_benchmark:plan.insert(plan.index('both_eye_images'),'depth_benchmark')
        if args.schedule_benchmark:plan.insert(plan.index('both_eye_images'),'schedule_benchmark')
        if args.early_depth_interval:
            if args.origin_benchmark:plan.insert(plan.index('both_eye_images'),'early_depth_baseline')
            if args.instance_benchmark:plan.insert(plan.index('both_eye_images'),'early_depth_instance_baseline')
            if args.descriptor_benchmark:plan.insert(plan.index('both_eye_images'),'descriptor_benchmark')
            if args.camera_benchmark:plan.insert(plan.index('both_eye_images'),'early_depth_camera_baseline')
            if args.metadata_benchmark:plan.insert(plan.index('both_eye_images'),'metadata_benchmark')
            if args.upload_benchmark:plan.insert(plan.index('both_eye_images'),'upload_benchmark')
            for name in ('early_depth_interval','early_depth_cancel','post_depth_hud'):plan.insert(plan.index('both_eye_images'),name)
        if args.pass_targets:plan.insert(plan.index('preparation_order'),'pass_targets')
        if args.depth_variants:plan.insert(plan.index('both_eye_images'),'depth_variants')
        if args.paired_interval:
            if args.eager_depth=='replace':plan.insert(plan.index('both_eye_images'),'eager_reference')
            plan.insert(plan.index('both_eye_images'), 'paired_interval')
            plan.insert(plan.index('both_eye_images'), 'post_interval_hud')
    if args.geometry_cost:
        plan.insert(plan.index('both_eye_images'),'geometry_cost')
        plan.insert(plan.index('both_eye_images'),'post_cost_hud')
    if args.cpu_collector:
        plan.insert(plan.index('both_eye_images'), 'cpu_collector')
        if args.cpu_repeat>1:
            plan.insert(plan.index('both_eye_images'), 'cpu_collector_repeat')
        plan.insert(plan.index('both_eye_images'), 'post_collector_hud')
    started = time.time()
    baseline_reporters = boot.reporter_pids() | boot.reporter_window_pids()
    simulator = Path.home() / 'AppData/Local/OpenXR-Simulator'
    preview = PreviewSettings(simulator / 'settings.json')
    settings_probe=None

    def check():
        crash = boot.crash_report(pid, started) if pid else None
        if crash or boot.new_reporters(baseline_reporters):
            report['crash'] = crash or 'New CrashReporter without PID-matched report yet'
            boot.close_reporters(boot.new_reporters(baseline_reporters))
            raise RuntimeError(report['crash'])

    def still_running():
        check()
        return pid in boot.process_ids('Cyberpunk2077.exe')

    def launch_line(line):
        nonlocal pid, startup
        match = re.fullmatch(r'Launched PID (\d+)\s*', line)
        if match:
            pid = int(match[1])
            report['pid'] = pid
        if line.startswith('{'):
            value = json.loads(line)
            if value.get('ready') and value.get('pid') == pid:
                startup = value

    def script(name, *parameters, timeout=30):
        check()
        return run_logged([sys.executable, '-B', ROOT / 'tools/single_pass_tests' / name,
                           *parameters, *(['--compact'] if name=='read_preparation.py' else [])], timeout)

    retained_owned=False
    def write32(name,value):
        k=reader.k;k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
        handle=k.OpenProcess(0x101028,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        try:
            data,done=c.c_uint32(value),c.c_size_t()
            if not k.WriteProcessMemory(handle,int(symbols[name],16),c.byref(data),4,c.byref(done)) or done.value!=4:
                raise c.WinError(c.get_last_error())
        finally:k.CloseHandle(handle)

    def observe_current(label,capture,*,rejected=False):
        path=out/f'prepared-current-{label}.json'
        script('read_prepared_pair.py','--pid',pid,'--out',path,*current_observer_flags(capture,rejected=rejected,tail=args.prepared_tail,allow_vrs_refinement=args.allow_vrs_refinement,native_export=args.native_export,actual_membership=args.actual_membership),timeout=90)
        comparison=json.loads(path.read_text())
        validate_current_capture(capture,comparison,rejected=rejected,infix=args.prepared_infix,tail=args.prepared_tail,allow_vrs_refinement=args.allow_vrs_refinement,native_export=args.native_export,full_t5=args.full_t5,borrowed_tables=args.borrowed_tables,fixed_assets=args.fixed_assets,actual_membership=args.actual_membership)
        if args.prepared_infix:comparison['preparedInfix']=capture['preparedInfix']
        if args.prepared_tail:comparison['preparedTail']=capture['preparedTail']
        if args.full_t5:comparison['fullT5']=capture['fullT5']
        if args.borrowed_tables:comparison['borrowedTables']=capture['borrowedTables']
        if args.fixed_assets:comparison['fixedAssets']=capture['fixedAssets']
        return comparison

    def clear_selector_tracking():
        if not reader:return
        values=resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',['CyberpunkVR_RegisteredIntentEnable','CyberpunkVR_RegisteredIntentCacheStats','CyberpunkVR_RegisteredStructuredIntentStats','CyberpunkVR_SourceSrvBindingEnable','CyberpunkVR_StructuredBufferSnapshotsEnable'])
        address=int(values['CyberpunkVR_RegisteredIntentEnable'],16)
        if args.source_selector_manifest:
            k=reader.k;k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
            handle=k.OpenProcess(0x101028,False,pid)
            if not handle:raise c.WinError(c.get_last_error())
            try:
                for name in ('CyberpunkVR_SourceSrvBindingEnable','CyberpunkVR_StructuredBufferSnapshotsEnable','CyberpunkVR_RegisteredIntentEnable'):
                    data,done=c.c_uint32(0),c.c_size_t()
                    if not k.WriteProcessMemory(handle,int(values[name],16),c.byref(data),4,c.byref(done)) or done.value!=4:raise c.WinError(c.get_last_error())
            finally:k.CloseHandle(handle)
            deadline=time.monotonic()+2
            while True:
                cache=list(struct.unpack('<10Q',reader.read(int(values['CyberpunkVR_RegisteredIntentCacheStats'],16),80)))
                structured=list(struct.unpack('<2Q',reader.read(int(values['CyberpunkVR_RegisteredStructuredIntentStats'],16),16)))
                if not cache[0] and not cache[1] and not cache[9] and not any(structured):break
                if time.monotonic()>=deadline:raise RuntimeError('Disabled source-intent cache did not drop its current ranges')
                time.sleep(.05)
            report['selectorCacheCleanup']=dict(currentBytes=cache[0],ranges=cache[1],retainedPayloadBytes=cache[2],owners=cache[4],requested=cache[9])
            report['selectorCacheCleanup'].update(structuredBytes=structured[0],structuredRanges=structured[1])
        return struct.unpack('<I',reader.read(address,4))[0]
    def clear_retained():
        if not retained_owned or not reader:return
        if args.prepared_current:
            write32('CyberpunkVR_PreparedActualMembershipEnable',0)
            report['actualMembershipControlCleanup']=read32('CyberpunkVR_PreparedActualMembershipEnable')
            if report['actualMembershipControlCleanup']:raise RuntimeError('Actual membership control was not restored')
            write32('CyberpunkVR_RetainedFixedAssetsEnable',0)
            report['fixedAssetsControlCleanup']=read32('CyberpunkVR_RetainedFixedAssetsEnable')
            if report['fixedAssetsControlCleanup']:raise RuntimeError('Fixed-assets control was not restored')
            write32('CyberpunkVR_RetainedBorrowedTablesEnable',0)
            report['borrowedTablesControlCleanup']=read32('CyberpunkVR_RetainedBorrowedTablesEnable')
            if report['borrowedTablesControlCleanup']:raise RuntimeError('Borrowed-table control was not restored')
            write32('CyberpunkVR_RetainedFullT5Enable',0)
            report['fullT5ControlCleanup']=read32('CyberpunkVR_RetainedFullT5Enable')
            if report['fullT5ControlCleanup']:raise RuntimeError('Full T5 control was not restored')
            write32('CyberpunkVR_PreparedNativeExportEnable',0)
            report['nativeExportControlCleanup']=read32('CyberpunkVR_PreparedNativeExportEnable')
            if report['nativeExportControlCleanup']:raise RuntimeError('Native export control was not restored')
        controls={'Request':0,'State':0,'Stage':7,'Verified':0}
        k=reader.k;k.WriteProcessMemory.argtypes=[c.c_void_p,c.c_void_p,c.c_void_p,c.c_size_t,c.POINTER(c.c_size_t)]
        handle=k.OpenProcess(0x101028,False,pid)
        if not handle:raise c.WinError(c.get_last_error())
        try:
            for name,value in controls.items():
                data,done=c.c_uint32(value),c.c_size_t()
                if not k.WriteProcessMemory(handle,int(symbols['CyberpunkVR_RetainedGeometry'+name],16),c.byref(data),4,c.byref(done)) or done.value!=4:
                    raise c.WinError(c.get_last_error())
            report['retainedCleanup']={name:struct.unpack('<I',reader.read(int(symbols['CyberpunkVR_RetainedGeometry'+name],16),4))[0] for name in controls}
            if report['retainedCleanup']!=controls:raise RuntimeError('Retained geometry controls were not restored')
        finally:k.CloseHandle(handle)
        if args.prepared_current:
            for name in ('CyberpunkVR_PreparedVrsRefinementEnable','CyberpunkVR_PreparedTailEnable','CyberpunkVR_PreparedInfixEnable','CyberpunkVR_PreparedCurrentEnable','CyberpunkVR_PreparedPairInputFault','CyberpunkVR_SubmissionFrontierEnable'):write32(name,0)
            if read32('CyberpunkVR_PreparedPairState') not in (0,5,6):
                raise RuntimeError('Current prepared comparison is still active during cleanup')
            write32('CyberpunkVR_PreparedPairState',0)

    def screenshot(label):
        request=simulator/'screenshot_request.json'
        if request.exists():raise RuntimeError('Another screenshot request is pending')
        requested=time.time();request.write_text(json.dumps(dict(eye='both',layer='all')))
        deadline=time.monotonic()+10
        while time.monotonic()<deadline:
            check();status=simulator/'screenshot_status.json'
            if status.exists() and status.stat().st_mtime>=requested:
                pixels=(simulator/'screenshot.bmp').read_bytes()
                result=check_both_eyes(pixels);(out/(label+'.bmp')).write_bytes(pixels)
                return result
            time.sleep(.1)
        raise TimeoutError('No fresh simulator screenshot')

    def packet_test(name, frames, resolve_inputs=False):
        path = out / (name + '.json')
        parameters = ['--pid', pid, '--out', path, '--frames', frames]
        if resolve_inputs:
            parameters += ['--resolve-inputs']
            if args.native_draws:
                parameters += ['--native-draws']
                if args.native_constants:
                    parameters += ['--native-constants']
                    if args.freeze_geometry:
                        parameters += ['--freeze-geometry']
        script('read_geometry_packets.py', *parameters)
        value = json.loads(path.read_text())
        rows = value['records']
        if value['full'] or {r['side'] for r in rows} != {0, 1} or any(r['status'] not in (0, 1) for r in rows):
            raise RuntimeError('Incomplete two-eye packet capture')
        if not all(any(r['frame'] == frame and r['side'] == side and r['count'] for r in rows)
                   for frame in range(1, frames + 1) for side in (0, 1)):
            raise RuntimeError('Missing view in a requested packet frame')
        report[name] = dict(records=len(rows), packets=sum(r['count'] for r in rows), frames=frames,
                            bytesOwned=sum(r['count'] for r in rows) * 16)
        report['completed'].append(name)

    try:
        report['previewWindow'] = preview.start()
        if args.owned_retirement:
            from retirement_settings import NativeSettingsProbe
            settings_probe=NativeSettingsProbe(game,lambda:bool(pid and pid in boot.process_ids('Cyberpunk2077.exe')),check)
            settings_probe.install()
        launch = [sys.executable, '-B', ROOT / 'tools/quickboot_tests/launch_to_save.py','--timeout',args.timeout]
        if args.native_draws:
            launch.append('--observe-native-bindings')
        if args.bounds_trace:
            launch.append('--bounds-trace')
        if args.merge_root:
            launch.append('--paired-root')
        if args.family_manifest:
            launch.extend(['--family-manifest', args.family_manifest.resolve()])
        if args.source_selector_manifest:
            launch.extend(['--source-selector-manifest',args.source_selector_manifest.resolve()])
        if args.reflection_dxc:
            launch.extend(['--reflection-dxc',args.reflection_dxc.resolve()])
        run_logged(launch, args.timeout + 10, launch_line)
        if not startup or not startup.get('focusSequence', {}).get('verified'):
            raise RuntimeError('Startup did not verify game then Codex focus and gameplay')
        report['startup'] = startup
        report['completed'].append('focus_and_save')
        hud = startup.get('stableHud', {})
        if (hud.get('stableMilliseconds', 0) < 1000 or hud.get('submittedFrames', 0) < 30 or
                hud.get('sourceSerialAdvances', 0) < 1 or hud.get('last', {}).get('menu') != 0):
            raise RuntimeError('Startup did not verify stable fresh HUD submissions')
        report['stable_hud'] = hud
        report['completed'].append('stable_hud')
        names = ['CyberpunkVR_GeometryPacketHookCompatible', 'CyberpunkVR_GeometryPacketRequest',
                 'CyberpunkVR_GeometryPacketState', 'CyberpunkVR_GeometryPacketResolve',
                 'CyberpunkVR_PrepareProfileEnable', 'CyberpunkVR_RuntimeDiagnostics',
                 'CyberpunkVR_StereoGpuProbeRequest', 'CyberpunkVR_StereoGpuProbePrepareGateMs',
                 'CyberpunkVR_SinglePassTwinUpload', 'CyberpunkVR_NativeStereoProbeAssetStatus', 'g_menuModeValue',
                 'CyberpunkVR_NativeStereoBindingHooks', 'CyberpunkVR_StereoFamilyAssetStatus', 'CyberpunkVR_StereoFamilyCount',
                 'CyberpunkVR_ConstantUploadCount', 'CyberpunkVR_ConstantCaptureEnable', 'CyberpunkVR_ConstantBankRequest', 'CyberpunkVR_ConstantBankPairCount',
                 'CyberpunkVR_ConstantBankPrepareGeometry', 'CyberpunkVR_GeometryLeaseDrawCount', 'CyberpunkVR_GeometryLeaseResourceCount']
        names.extend(('CyberpunkVR_PairedGeometryRequest','CyberpunkVR_PairedGeometryState','CyberpunkVR_PairedGeometryOrdinal','CyberpunkVR_PairedGeometryFamily'))
        names.extend(('CyberpunkVR_PreparedActualMembershipEnable','CyberpunkVR_RetainedFixedAssetsEnable','CyberpunkVR_RetainedBorrowedTablesEnable','CyberpunkVR_RetainedFullT5Enable','CyberpunkVR_PreparedNativeExportEnable','CyberpunkVR_PreparedVrsRefinementEnable','CyberpunkVR_PreparedTailEnable','CyberpunkVR_PreparedInfixEnable','CyberpunkVR_PreparedCurrentEnable','CyberpunkVR_PreparedPairInputFault','CyberpunkVR_PreparedPairState','CyberpunkVR_SubmissionFrontierEnable'))
        symbols = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', names)
        if args.retained_geometry:
            symbols.update(resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',['CyberpunkVR_RetainedGeometry*']))
        reader = Reader(pid, game / 'bin/x64/Cyberpunk2077.exe', 0, 0)
        read32 = lambda name: struct.unpack('<I', reader.read(int(symbols[name], 16), 4))[0]
        report['nativeStartup'] = dict(menuMode=read32('g_menuModeValue'), hookCompatible=read32(names[0]))
        report['pairedGeometryStartup']={n:read32('CyberpunkVR_PairedGeometry'+n) for n in ('Request','State','Ordinal','Family')}
        if any(report['pairedGeometryStartup'].values()):raise RuntimeError('Paired geometry is not initially idle')
        report['preparedStartup']={name:read32(name) for name in ('CyberpunkVR_PreparedActualMembershipEnable','CyberpunkVR_RetainedFixedAssetsEnable','CyberpunkVR_RetainedBorrowedTablesEnable','CyberpunkVR_RetainedFullT5Enable','CyberpunkVR_PreparedNativeExportEnable','CyberpunkVR_PreparedVrsRefinementEnable','CyberpunkVR_PreparedTailEnable','CyberpunkVR_PreparedInfixEnable','CyberpunkVR_PreparedCurrentEnable','CyberpunkVR_PreparedPairInputFault','CyberpunkVR_PreparedPairState','CyberpunkVR_SubmissionFrontierEnable')}
        if any(report['preparedStartup'].values()):raise RuntimeError('Prepared comparison controls were not initially idle')
        if read32(names[0]) != 1:
            raise RuntimeError('Independent packet hook unavailable')
        if read32('g_menuModeValue'):
            raise RuntimeError('Native menu reopened after stable gameplay readiness')
        if args.cpu_peer:
            script('read_startup_evidence.py','--pid',pid,'--out',out/'startup-ready.json')
        report['dll'] = symbols['sha256']
        if args.install_upload_observer:
            report['uploadObserverInstall'] = install_disabled_upload_observer(pid, game, out, check)
            report['completed'].append('upload_observer_install')
        if args.retained_geometry:
            report['retainedStartup']={name:read32('CyberpunkVR_RetainedGeometry'+name) for name in ('Request','State','Stage','Verified')}
            if report['retainedStartup']!=dict(Request=0,State=0,Stage=7,Verified=0):raise RuntimeError('Retained geometry controls were not initially idle')
            retained_owned=True
        report['gpuProbeAssetStatus'] = read32('CyberpunkVR_NativeStereoProbeAssetStatus')
        report['bindingObservationHooks'] = read32('CyberpunkVR_NativeStereoBindingHooks')
        report['familyStartup'] = dict(assetStatus=read32('CyberpunkVR_StereoFamilyAssetStatus'), count=read32('CyberpunkVR_StereoFamilyCount'))
        if not args.family_manifest and any(report['familyStartup'].values()):
            raise RuntimeError('Experimental family package leaked into an ordinary launch')
        report['constantStartup'] = dict(uploadCount=read32('CyberpunkVR_ConstantUploadCount'), capture=read32('CyberpunkVR_ConstantCaptureEnable'), request=read32('CyberpunkVR_ConstantBankRequest'), pairs=read32('CyberpunkVR_ConstantBankPairCount'))
        if not args.family_manifest and any(report['constantStartup'].values()):
            raise RuntimeError('Experimental constant ownership leaked into an ordinary launch')
        report['geometryStartup'] = dict(request=read32('CyberpunkVR_ConstantBankPrepareGeometry'),
                                        draws=read32('CyberpunkVR_GeometryLeaseDrawCount'), resources=read32('CyberpunkVR_GeometryLeaseResourceCount'))
        if not args.family_manifest and any(report['geometryStartup'].values()):
            raise RuntimeError('Experimental geometry ownership leaked into an ordinary launch')
        if not args.native_draws and not args.family_manifest and report['gpuProbeAssetStatus']!=3 and report['bindingObservationHooks']:
            raise RuntimeError('Diagnostic binding observers leaked into an ordinary launch')
        if args.family_manifest:
            path = out / 'families.json'
            script('read_families.py', '--pid', pid, '--manifest', args.family_manifest.resolve(), '--out', path)
            report['family_registry'] = {key: value for key, value in json.loads(path.read_text()).items() if key != 'records'}
            report['completed'].append('family_registry')
        if args.observer_profile:
            from observation_profile import run as profile_observers
            report['observer_profile']=profile_observers(pid,game,8,check)
            (out/'observer-profile.json').write_text(json.dumps(report['observer_profile'],indent=2)+'\n')
            report['completed'].append('observer_profile')
            print(json.dumps(report['observer_profile'],indent=2),flush=True)
        packet_test('resolved_packets', 1, True)
        script('analyze_geometry_packets.py', out / 'resolved_packets.json')
        if args.native_draws:
            script('verify_packet_instances.py', out / 'resolved_packets.json')
            verified = json.loads((out / 'resolved_packets-instances-verified.json').read_text())
            report['native_instances_verified'] = verified
            if verified['failures'] or not verified['totals'].get('preuploadedAddressVerified'):
                raise RuntimeError('Native instance ranges failed verification')
            report['completed'].append('native_instances_verified')
            script('read_descriptor_audit.py',out/'resolved_packets.json','--pid',pid)
            report['descriptor_capture']=json.loads((out/'resolved_packets-descriptors.json').read_text())
            report['completed'].append('descriptor_capture')
        if args.native_constants:
            run_logged([ROOT / 'build/single-pass-tests/Release/mesh_plan_capture.exe', out / 'resolved_packets', out / 'mesh-plan.json'], 20)
            script('verify_mesh_plan.py', out / 'resolved_packets.json', out / 'mesh-plan.json', '--out', out / 'geometry-pairs.json')
            report['geometry_pairing'] = {key: value for key, value in json.loads((out / 'geometry-pairs.json').read_text()).items() if key != 'mappedPairs'}
            report['completed'].append('geometry_pairing')
            script('read_constants.py', '--pid', pid, '--capture', out / 'resolved_packets.json', '--families', out / 'families.json',
                   '--manifest', args.family_manifest.resolve(), '--geometry', out / 'geometry-pairs.json', '--out', out / 'constants.json',
                   *(['--compare-depth'] if args.compare_depth else []))
            report['constant_bank'] = constant_summary(json.loads((out / 'constants.json').read_text()))
            report['completed'].append('constant_bank')
            if args.compare_depth:
                report['original_shader_depth'] = report['constant_bank']['depthComparison']
                report['completed'].append('original_shader_depth')
            script('read_geometry_leases.py', '--pid', pid, '--capture', out/'resolved_packets.json', '--constants', out/'constants.json', '--out', out/'geometry-leases.json')
            report['geometry_ownership'] = json.loads((out/'geometry-leases.json').read_text())
            report['completed'].append('geometry_ownership')
            script('verify_full_instances.py', '--capture', out/'resolved_packets.json', '--constants', out/'constants.json', '--out', out/'full-instances.json')
            report['full_instance_batches'] = {k:v for k,v in json.loads((out/'full-instances.json').read_text()).items() if k!='pairs'}
            report['completed'].append('full_instance_batches')
        if args.preparation_trace:
            if args.merge_root:
                script('read_preparation.py','--pid',pid,'--frames',2,'--out',out/'pair-warmup.json','--main-root-pair')
                report['pair_warmup']=json.loads((out/'pair-warmup.json').read_text())['pairPreparation']
                report['completed'].append('pair_warmup')
                def warmup_running():
                    check()
                    return pid in boot.process_ids('Cyberpunk2077.exe')
                report['warmup_hud']=wait_for_stable_hud(pid,game,15,warmup_running)
                report['completed'].append('warmup_hud')
            preparation_parameters=['--pid', pid, '--frames', args.merge_root_frames+1 if args.merge_root else 2,
                   *(['--owned-coupled'] if args.owned_coupled else []),
                   *(['--scene-split','--scene-split-eyes',args.scene_split_eyes] if args.scene_split else []),
                   *(['--scene-access'] if args.scene_access else []),
                   *(['--merge-root','--merge-frames',args.merge_root_frames] if args.merge_root else ['--main-root-pair'] if args.main_root_pair else ['--pair-prepare'] if args.pair_prepare else []),
                   *(['--graph-plan'] if args.graph_plan else []),
                   *(['--schedule-preparation',args.schedule_preparation] if args.schedule_preparation else []),
                   *(['--paired-cameras'] if args.paired_cameras else []),
                   *(['--camera-turn',simulator] if args.paired_camera_turn else []),
                   *(['--early-depth-gpu',args.early_depth_gpu] if args.early_depth_gpu else []),
                   *(['--native-camera'] if args.native_camera else []),
                   '--native-camera-stages',args.native_camera_stages,
                   *(['--skinning-depth'] if args.skinning_depth else []),
                   *alpha_depth_flags(reference=args.alpha_depth_reference,skinned=args.skinned_alpha_depth),
                   *(['--material-depth'] if args.material_depth else []),
                   *(['--pass-targets'] if args.pass_targets else []),
                   *(['--retained-geometry','reference','--retained-stage',args.retained_stage] if args.retained_geometry else []),
                   *(['--paired-gbuffer','--paired-gbuffer-family',args.paired_gbuffer_family] if args.paired_gbuffer else []),
                   *(['--owned-graph-sort'] if args.owned_graph_sort else []),
                   *(['--whole-gbuffer-plan'] if args.whole_gbuffer_plan else []),
                   *(['--retained-source-stream'] if args.retained_source_stream else []),
                   *(['--retained-ia-snapshots'] if args.retained_ia_snapshots else []),
                   *(['--prepared-current'] if args.prepared_current else []),
                   *(['--prepared-infix'] if args.prepared_infix else []),
                   *(['--prepared-tail'] if args.prepared_tail else []),
                   *(['--allow-vrs-refinement'] if args.allow_vrs_refinement else []),
                   *(['--native-export'] if args.native_export else []),
                   *(['--full-t5'] if args.full_t5 else []),
                   *(['--actual-membership'] if args.actual_membership else []),
                   *(['--borrowed-tables'] if args.borrowed_tables else []),
                   *(['--fixed-assets'] if args.fixed_assets else []),
                   *(['--retained-indirect-snapshots'] if args.retained_indirect_snapshots else []),
                   *(['--retained-source-cbvs'] if args.retained_source_cbvs else []),
                   *(['--retained-source-tables'] if args.retained_source_tables else []),
                   *(['--retained-source-details'] if args.retained_source_details else []),
                   *(['--retained-cbv-snapshots'] if args.retained_cbv_snapshots else []),
                   *(['--retained-direct-upload'] if args.retained_direct_upload else []),
                   *(['--retained-source-selectors'] if args.source_selector_manifest else []),
                   *(['--owned-reference'] if args.owned_reference else []),
                   *(['--whole-depth-target'] if args.whole_depth_target else []),
                   *(['--optimized-depth'] if args.optimized_depth else []),
                   *(['--reject-depth-commit'] if args.reject_depth_commit else [])]
            if args.paired_gbuffer:
                report['pairedGeometryAttempts']=[]
                for ordinal in range(32):
                    attempt_path=out/f'paired-ordinal-{ordinal}.json'
                    try:
                        script('read_preparation.py',*preparation_parameters,'--out',attempt_path,'--paired-gbuffer-ordinal',ordinal)
                    except RuntimeError:
                        if not attempt_path.is_file():raise
                        attempt=json.loads(attempt_path.read_text())
                        geometry=attempt.get('pairedGeometry',{})
                        validate_paired_geometry(geometry,require_contribution=False)
                        if attempt.get('error')!='Paired reference must visibly contribute native G-buffer pixels in both eyes':raise
                        if attempt.get('cleanup')!='restored':raise RuntimeError('Blank reference did not restore controls')
                        report['pairedGeometryAttempts'].append(dict(ordinal=ordinal,contributing=False,geometry=geometry))
                        continue
                    attempt=json.loads(attempt_path.read_text())
                    report['pairedGeometryAttempts'].append(dict(ordinal=ordinal,contributing=True,geometry=attempt['pairedGeometry']))
                    (out/'preparation.json').write_text(json.dumps(attempt,indent=2)+'\n')
                    break
                else:raise RuntimeError('No contributing paired G-buffer candidate in bounded same-process search')
            else:script('read_preparation.py',*preparation_parameters,'--out',out/'preparation.json')
            prep = json.loads((out/'preparation.json').read_text())
            report['preparation_order'] = dict(passed=prep['passed'], records=len(prep['records']), stages=len(prep['summary']['completedStages']), kinds=prep['summary']['kinds'])
            if args.pair_prepare:
                report['preparation_order']['pairPreparation'] = prep['pairPreparation']
                report['preparation_order']['admissionRejected'] = prep.get('admissionRejected')
                report['preparation_order']['eyeCopyOrder'] = prep.get('eyeCopyOrder')
                if args.graph_plan:report['preparation_order']['graphPlan']=prep['graphPlan']
                if args.owned_coupled:report['preparation_order']['coupledSchedule']=prep['ownedExecution']['schedule']
                elif args.schedule_preparation:report['preparation_order']['cpuSchedule']={k:v for k,v in prep['cpuSchedule'].items() if k!='edges'}
                if args.schedule_preparation:report['preparation_order']['proxyPreparation']=prep['summary']['proxyPreparation']
                if args.paired_cameras:report['preparation_order']['pairedCameras']=prep['pairedCameras']
                if args.early_depth_gpu:report['preparation_order']['earlyDepthGpu']=prep['earlyDepthGpu']
                if args.pass_targets:
                    report['pass_targets']={k:v for k,v in prep['passTargets'].items() if k not in ('groups','resources','sharedAcrossEyes')}
                    report['completed'].append('pass_targets')
            report['completed'].append('preparation_order')
            if args.owned_reference:
                report['owned_reference']=prep['ownedExecution']
                report['completed'].append('owned_reference')
                if args.scene_split:
                    from scene_split_report import validate_capture as validate_scene_split_capture
                    report['native_scene_split']=validate_scene_split_capture(prep)
                    report['completed'].append('native_scene_split')
                    if args.scene_access:
                        from scene_access_report import validate_capture as validate_scene_access_capture
                        report['initial_scene_access']=validate_scene_access_capture(prep,requested=True)
                        report['completed'].append('initial_scene_access')
                if args.whole_gbuffer_plan:
                    report['whole_gbuffer_plan']=prep['wholeGBufferPlan'];report['resourceReplay']=prep['resourceReplay'];report['completed'].append('whole_gbuffer_plan')
            if args.pass_targets:
                if not prep.get('graphParents',{}).get('completeGeometryParents'):
                    raise RuntimeError('Exact native graph parents are incomplete for geometry')
                report['graphParents']={k:v for k,v in prep['graphParents'].items() if k!='rows'}
                if not args.owned_coupled:report['pairedFrontier']=prep['pairedFrontier']
                if args.owned_graph_sort:
                    report['ownedGraphSort']=prep['ownedGraphSort'];report['graphJobLease']=prep['graphJobLease'];report['resourceUses']=prep['resourceUses'];report['definitionSource']=prep['definitionSource'];report['definitionGenerations']=prep['definitionGenerations'];report['definitionTree']=dict(stats=prep['definitionTree']['stats'])
                    if args.whole_gbuffer_plan:
                        report['whole_gbuffer_plan']=prep['wholeGBufferPlan'];report['completed'].append('whole_gbuffer_plan')
            if args.paired_gbuffer:
                validate_paired_geometry(prep['pairedGeometry'])
                report['paired_gbuffer']=prep['pairedGeometry'];report['completed'].append('paired_gbuffer')
                report['pairedGeometryImages']=screenshot('paired-gbuffer')
            if args.retained_geometry:
                validate_retained(prep['retainedGeometry'],'reference',args.retained_stage)
                if args.retained_source_stream:
                    report['sourceStream']=prep['sourceStream'];report['indirectBindings']=prep['indirectBindings']
                if args.retained_indirect_snapshots:report['indirectSnapshots']=prep['indirectSnapshots']
                if args.retained_source_cbvs:report['sourceCbvs']=prep['sourceCbvs']
                if args.retained_source_tables:report['sourceTables']=prep['sourceTables']
                if args.retained_source_details:report['sourceDetails']=prep['sourceDetails']
                if args.retained_cbv_snapshots:report['cbvSnapshots']=prep['cbvSnapshots']
                if args.retained_ia_snapshots:report['iaSnapshots']=prep['iaSnapshots']
                if args.source_selector_manifest:report['sourceSelectors']=prep['sourceSelectors']
                if args.source_selector_manifest:report['t5Snapshots']=prep['t5Snapshots']
                if args.source_selector_manifest:report['sourceAssembly']=prep['sourceAssembly']
                if args.source_selector_manifest:report['selectorCacheEvents']=prep['selectorCacheEvents']
                if args.source_selector_manifest:report['t5Overlay']=prep['t5Overlay']
                report['retained_reference']=prep['retainedGeometry'];report['completed'].append('retained_reference')
                report['retainedReferenceImages']=screenshot('retained-reference')
                if args.prepared_current:
                    report['prepared_current_initial']=observe_current('initial',prep)
                    report['completed'].append('prepared_current_initial')
                if args.owned_graph_sort or args.owned_reference:
                    script('read_preparation.py',*preparation_parameters,'--out',out/'preparation-repeat.json')
                    repeated=json.loads((out/'preparation-repeat.json').read_text())
                    validate_retained(repeated['retainedGeometry'],'reference',args.retained_stage)
                    if args.retained_source_stream:
                        report['repeatSourceStream']=repeated['sourceStream'];report['repeatIndirectBindings']=repeated['indirectBindings']
                    if args.retained_indirect_snapshots:report['repeatIndirectSnapshots']=repeated['indirectSnapshots']
                    if args.retained_source_cbvs:report['repeatSourceCbvs']=repeated['sourceCbvs']
                    if args.retained_source_tables:report['repeatSourceTables']=repeated['sourceTables']
                    if args.retained_source_details:report['repeatSourceDetails']=repeated['sourceDetails']
                    if args.retained_cbv_snapshots:report['repeatCbvSnapshots']=repeated['cbvSnapshots']
                    if args.retained_ia_snapshots:report['repeatIaSnapshots']=repeated['iaSnapshots']
                    if args.source_selector_manifest:report['repeatSourceSelectors']=repeated['sourceSelectors']
                    if args.source_selector_manifest:report['repeatT5Snapshots']=repeated['t5Snapshots']
                    if args.source_selector_manifest:report['repeatSourceAssembly']=repeated['sourceAssembly']
                    if args.source_selector_manifest:report['repeatSelectorCacheEvents']=repeated['selectorCacheEvents']
                    if args.source_selector_manifest:report['repeatT5Overlay']=repeated['t5Overlay']
                    if args.owned_graph_sort:
                        validate_owned_graph_repeat(prep,repeated)
                        report['repeat_owned_graph_sort']=dict(epoch=repeated['graphJobLease']['epoch'],jobs=repeated['graphJobLease'],
                            owned=repeated['ownedGraphSort'],resourceUses=repeated['resourceUses'],definitionSource=repeated['definitionSource'],definitionGenerations=repeated['definitionGenerations'])
                        if args.whole_gbuffer_plan:report['repeat_owned_graph_sort']['wholeGBufferPlan']=repeated['wholeGBufferPlan']
                        report['completed'].append('repeat_owned_graph_sort')
                    else:
                        validate_owned_reference_repeat(prep,repeated)
                        report['repeat_owned_reference']=repeated['ownedExecution']
                        if args.whole_gbuffer_plan:report['repeatWholeGBufferPlan']=repeated['wholeGBufferPlan'];report['repeatResourceReplay']=repeated['resourceReplay']
                        report['completed'].append('repeat_owned_reference')
                        if args.scene_split:
                            report['repeat_native_scene_split']=validate_scene_split_capture(repeated)
                            report['completed'].append('repeat_native_scene_split')
                            if args.scene_access:
                                report['repeat_scene_access']=validate_scene_access_capture(repeated,requested=True)
                                if report['repeat_scene_access']['stats'][3]<=report['initial_scene_access']['stats'][3]:
                                    raise RuntimeError('Scene access repeat did not advance the observer epoch')
                                report['completed'].append('repeat_scene_access')
                if args.scene_split:
                    variants=report['turned_native_scene_split']=dict(passed=False)
                    def capture_scene_split_turn():
                        path=out/'preparation-scene-split-turned.json'
                        script('read_preparation.py',*preparation_parameters,'--out',path)
                        captured=json.loads(path.read_text())
                        validate_retained(captured['retainedGeometry'],'reference',args.retained_stage)
                        variants['sceneSplit']=validate_scene_split_repeat(repeated,captured)
                        if args.scene_access:
                            variants['sceneAccess']=validate_scene_access_capture(captured,requested=True)
                            if variants['sceneAccess']['stats'][3]<=report['repeat_scene_access']['stats'][3]:
                                raise RuntimeError('Turned scene access did not advance the observer epoch')
                        variants['capture']=str(path)
                        return captured
                    run_reversible_capture(HeadTurn(simulator),variants,capture_scene_split_turn,
                        lambda label:screenshot('scene-split-turn-'+label),
                        lambda:wait_for_stable_hud(pid,game,15,still_running),
                        lambda label:(out/('scene-split-turn-'+label+'.bmp')).read_bytes())
                    report['completed'].append('turned_native_scene_split')
                    if args.scene_access:
                        report['turned_scene_access']=variants['sceneAccess']
                        report['completed'].append('turned_scene_access')
                for name,enabled,flag in (('prepared_pair',args.prepared_pair,'--all-pairs'),('prepared_sequence',args.prepared_sequence,'--sequence')):
                    if not enabled:continue
                    if args.prepared_current:
                        report[name]=observe_current('repeat',repeated)
                    else:
                        path=out/(name.replace('_','-')+'.json')
                        script('read_prepared_pair.py','--pid',pid,'--out',path,flag,timeout=90)
                        report[name]=json.loads(path.read_text())
                    report['completed'].append(name)
                    if name=='prepared_sequence' and not args.prepared_current:
                        rejected=out/'prepared-input-rejection.json'
                        script('read_prepared_pair.py','--pid',pid,'--out',rejected,'--sequence','--reject-inputs',timeout=90)
                        report['prepared_input_rejection']=json.loads(rejected.read_text())
                        report['completed'].append('prepared_input_rejection')
                if args.retained_source_bank:
                    require_source_banks(prep['sourceAssembly'],repeated['sourceAssembly'] if args.owned_graph_sort or args.owned_reference else None)
                    report['completed'].append('retained_source_bank')
                if args.prepared_sequence_turn:
                    variants=report['turned_prepared_sequence']=dict(passed=False)
                    def capture_prepared_turn():
                        capture=out/'preparation-turned.json'
                        script('read_preparation.py',*preparation_parameters,'--out',capture)
                        turned=json.loads(capture.read_text())
                        validate_retained(turned['retainedGeometry'],'reference',args.retained_stage)
                        validate_owned_reference_repeat(repeated,turned)
                        if args.retained_source_bank:require_source_banks(turned['sourceAssembly'])
                        variants['capture']=str(capture)
                        variants['sourceBank']=turned['sourceAssembly']['bankStats']
                        if args.prepared_current:
                            variants['comparison']=observe_current('turned',turned)
                        else:
                            output=out/'prepared-sequence-turned.json'
                            script('read_prepared_pair.py','--pid',pid,'--out',output,'--sequence',timeout=90)
                            variants['comparison']=json.loads(output.read_text())
                        return turned
                    turned=run_reversible_capture(HeadTurn(simulator),variants,capture_prepared_turn,
                        lambda label:screenshot('prepared-turn-'+label),
                        lambda:wait_for_stable_hud(pid,game,15,still_running),
                        lambda label:(out/('prepared-turn-'+label+'.bmp')).read_bytes())
                    report['completed'].append('turned_prepared_sequence')
                if args.prepared_current:
                    rejection_path=out/'preparation-current-rejection.json'
                    def capture_current_rejection():
                        script('read_preparation.py',*preparation_parameters,'--out',rejection_path)
                        captured=json.loads(rejection_path.read_text())
                        validate_retained(captured['retainedGeometry'],'reference',args.retained_stage)
                        validate_owned_reference_repeat(turned,captured)
                        return captured
                    rejected_capture,comparison=run_current_rejection(
                        lambda:read32('CyberpunkVR_PreparedPairInputFault'),
                        lambda value:write32('CyberpunkVR_PreparedPairInputFault',value),capture_current_rejection,
                        lambda captured:observe_current('rejected',captured,rejected=True))
                    report['preparedCurrentRejectionCapture']=str(rejection_path)
                    report['prepared_input_rejection']=comparison
                    report['completed'].append('prepared_input_rejection')
                if args.owned_retirement:
                    from read_retirement import Telemetry
                    from retirement_stress import run as run_retirement,validate_cycles
                    telemetry=Telemetry(pid,game)
                    cycles=[];current=repeated
                    try:
                        for cycle in range(1,args.retirement_cycles+1):
                            suffix='' if cycle==1 else f'-{cycle}'
                            name='native_retirement' if cycle==1 else f'native_retirement_{cycle}'
                            retirement=run_retirement(settings_probe,telemetry,current,out/f'native-retirement{suffix}.json',check)
                            cycles.append(retirement)
                            report[name]={key:retirement[key] for key in ('passed','proof','restoration','state')}
                            report['completed'].append(name)
                            path=out/f'restored-reference{suffix}.json'
                            script('read_preparation.py',*preparation_parameters,'--out',path)
                            restored=json.loads(path.read_text())
                            validate_owned_reference_repeat(current,restored)
                            validate_retained(restored['retainedGeometry'],'reference',args.retained_stage)
                            name='restored_owned_reference' if cycle==1 else f'restored_owned_reference_{cycle}'
                            report[name]=dict(execution=restored['ownedExecution'],retained=restored['retainedGeometry'])
                            report['completed'].append(name)
                            current=restored
                        report['retirementCycles']=validate_cycles(cycles,args.retirement_cycles)
                    finally:telemetry.close()
                if args.retained_geometry=='replace':
                    script('read_preparation.py','--pid',pid,'--frames',2,'--out',out/'retained-replace.json',
                           '--merge-root','--merge-frames',1,'--graph-plan','--schedule-preparation','apply','--pass-targets',
                           '--retained-geometry','replace','--retained-stage',args.retained_stage)
                    replacement=json.loads((out/'retained-replace.json').read_text())
                    if (replacement['pid'],replacement['dll'])!=(prep['pid'],prep['dll']) or not replacement['passed']:
                        raise RuntimeError('Retained replacement did not use the verified reference process/build')
                    validate_retained(replacement['retainedGeometry'],'replace',args.retained_stage)
                    report['retained_replace']=replacement['retainedGeometry'];report['completed'].append('retained_replace')
                    report['retainedReplacementImages']=screenshot('retained-replace')
            if args.material_depth:
                script('read_preparation.py','--pid',pid,'--frames',2,'--out',out/'material-textures.json',
                       '--merge-root','--merge-frames',1,'--graph-plan','--schedule-preparation','apply',
                       '--early-depth-gpu','all','--whole-depth-target','--native-camera','--native-camera-stages',args.native_camera_stages,
                       *(['--skinning-depth'] if args.skinning_depth else []),'--material-depth','--material-hints',out/'preparation.json')
                report['material_textures']=json.loads((out/'material-textures.json').read_text())['earlyDepthGpu']['materials']
                if args.alpha_depth_optimized:
                    report['alpha_optimized']=[]
                    for rejected in (False,True):
                        name='alpha-native-fallback' if rejected else 'alpha-optimized'
                        script('read_preparation.py','--pid',pid,'--frames',2,'--out',out/(name+'.json'),
                               '--merge-root','--merge-frames',1,'--graph-plan','--schedule-preparation','apply',
                               '--early-depth-gpu','all','--whole-depth-target','--native-camera','--native-camera-stages',args.native_camera_stages,
                               *(['--skinning-depth'] if args.skinning_depth else []),
                               *alpha_depth_flags(optimized=True,skinned=args.skinned_alpha_depth),'--optimized-depth',
                               '--material-hints',out/'material-textures.json',*(['--reject-depth-commit'] if rejected else []))
                        report['alpha_optimized'].append(json.loads((out/(name+'.json')).read_text())['earlyDepthGpu'])
                        screenshot(name)
            if args.pair_prepare:
                report['post_pair_hud'] = wait_for_stable_hud(pid, game, 15, still_running)
                report['completed'].append('post_pair_hud')
            if args.depth_benchmark:
                from depth_benchmark import run as benchmark_depth
                report['depth_benchmark']=benchmark_depth(pid,out/'depth-timing',args.depth_benchmark,check,
                    lambda:wait_for_stable_hud(pid,game,15,still_running))
                report['completed'].append('depth_benchmark')
            if args.schedule_benchmark:
                report['schedule_benchmark']=[]
                for index,cache in enumerate((False,True,True,False)):
                    label=f'schedule-{index}-'+('cached' if cache else 'uncached')
                    sample=run_paired_interval(pid,game,240,check,lambda name:screenshot(label+'-'+name),
                        out/(label+'.json'),schedule=True,schedule_cache=cache)
                    report['schedule_benchmark'].append(dict(label=label,final=sample['final'],cpu=sample['cpuSchedule']))
                report['schedule_benchmark_hud']=wait_for_stable_hud(pid,game,15,still_running)
                report['completed'].append('schedule_benchmark')
            if args.early_depth_interval:
                from early_depth_interval import run as continuous_depth
                if args.upload_benchmark:
                    report['upload_benchmark']=[]
                    for label,fast in (('legacy-before',False),('receipt-first',True),('receipt-second',True),('legacy-after',False)):
                        name='upload-'+label
                        sample=continuous_depth(pid,game,args.early_depth_interval,check,
                            lambda image,prefix=name:screenshot(prefix+'-'+image),out/(name+'.json'),
                            native_camera=True,fast_uploads=fast,profile_descriptors=True,camera_stages=args.native_camera_stages)
                        report['upload_benchmark'].append({k:v for k,v in sample.items() if k not in ('rows','images')})
                        wait_for_stable_hud(pid,game,15,still_running)
                    report['completed'].append('upload_benchmark')
                if args.metadata_benchmark:
                    report['metadata_benchmark']=[]
                    for label,live in (('full-before',False),('live-first',True),('live-second',True),('full-after',False)):
                        name='metadata-'+label
                        sample=continuous_depth(pid,game,args.early_depth_interval,check,
                            lambda image,prefix=name:screenshot(prefix+'-'+image),out/(name+'.json'),
                            native_camera=True,live_metadata=live,profile_descriptors=True,camera_stages=args.native_camera_stages)
                        report['metadata_benchmark'].append({k:v for k,v in sample.items() if k not in ('rows','images')})
                        wait_for_stable_hud(pid,game,15,still_running)
                    report['completed'].append('metadata_benchmark')
                if args.camera_benchmark:
                    name='early_depth_camera_baseline'
                    report[name]=continuous_depth(pid,game,args.early_depth_interval,check,
                        lambda label:screenshot(name+'-'+label),out/(name+'.json'),native_camera=False)
                    report['completed'].append(name);wait_for_stable_hud(pid,game,15,still_running)
                if args.descriptor_benchmark:
                    report['descriptor_benchmark']=[]
                    for index,dense in enumerate((False,True,True,False)):
                        name=f'descriptor-{index}-'+('dense' if dense else 'sparse')
                        sample=continuous_depth(pid,game,args.early_depth_interval,check,
                            lambda label,prefix=name:screenshot(prefix+'-'+label),out/(name+'.json'),
                            dense_descriptors=dense,profile_descriptors=True)
                        report['descriptor_benchmark'].append({k:v for k,v in sample.items() if k not in ('rows','images')})
                        wait_for_stable_hud(pid,game,15,still_running)
                    report['completed'].append('descriptor_benchmark')
                if args.origin_benchmark:
                    name='early_depth_baseline'
                    report[name]=continuous_depth(pid,game,args.early_depth_interval,check,
                        lambda label:screenshot(name+'-'+label),out/(name+'.json'),scoped_origin=False)
                    report['completed'].append(name)
                    wait_for_stable_hud(pid,game,15,still_running)
                if args.instance_benchmark:
                    name='early_depth_instance_baseline'
                    report[name]=continuous_depth(pid,game,args.early_depth_interval,check,
                        lambda label:screenshot(name+'-'+label),out/(name+'.json'),combined_instances=False)
                    report['completed'].append(name)
                    wait_for_stable_hud(pid,game,15,still_running)
                for name,cancel in (('early_depth_interval',0),('early_depth_cancel',60)):
                    report[name]=continuous_depth(pid,game,args.early_depth_interval,check,
                        lambda label,prefix=name:screenshot(prefix+'-'+label),out/(name+'.json'),cancel,native_camera=args.native_camera,camera_stages=args.native_camera_stages,skinning=args.skinning_depth)
                    report['completed'].append(name)
                    report['post_depth_hud']=wait_for_stable_hud(pid,game,15,still_running)
                report['completed'].append('post_depth_hud')
            if args.depth_variants:
                turn=HeadTurn(simulator)
                variants=report['depth_variants']=dict(passed=False,samples=[])
                try:
                    screenshot('depth-variant-initial')
                    for label,pose,rejected in (('turned',1,False),('returned',0,False),('rejected',None,True)):
                        check()
                        if pose is not None:turn.apply(pose)
                        ready=wait_for_stable_hud(pid,game,15,still_running)
                        output=out/('depth-variant-'+label+'.json')
                        script('read_preparation.py','--pid',pid,'--frames',2,'--merge-root','--schedule-preparation','apply',
                               '--early-depth-gpu','all','--whole-depth-target','--out',output,
                               *(['--native-camera'] if args.native_camera else []),
                               '--native-camera-stages',args.native_camera_stages,
                               *(['--skinning-depth'] if args.skinning_depth else []),
                               *alpha_depth_flags(reference=args.alpha_depth_reference and not rejected,skinned=args.skinned_alpha_depth),
                               *(['--optimized-depth','--reject-depth-commit'] if rejected else []))
                        sample=json.loads(output.read_text())
                        variants['samples'].append(dict(label=label,hud=ready,passed=sample['passed'],depth=sample['earlyDepthGpu']))
                        wait_for_stable_hud(pid,game,15,still_running)
                        screenshot('depth-variant-'+label)
                    variants['motion']=verify_motion(*[(out/('depth-variant-'+label+'.bmp')).read_bytes() for label in ('initial','turned','returned')])
                    variants['passed']=True
                    report['completed'].append('depth_variants')
                finally:variants['poseCleanup']=turn.restore()
            if args.paired_interval:
                def interval(prefix,mode):
                    turn=HeadTurn(simulator) if args.paired_head_turn else None
                    try:
                        result=run_paired_interval(pid,game,1200,check,lambda label:screenshot(prefix+label),
                            out/(prefix+'paired-interval.json'),turn.apply if turn else None,mode,args.schedule_preparation=='apply')
                        if turn:result['motion']=verify_motion(*[(out/f'{prefix}paired-active-{i}.bmp').read_bytes() for i in range(3)])
                        return result
                    finally:
                        if turn:report[prefix+'paired_pose_cleanup']=turn.restore()
                if args.eager_depth=='replace':
                    report['eager_reference']=interval('eager-reference-',1)
                    report['completed'].append('eager_reference')
                report['paired_interval']=interval('',2 if args.eager_depth=='replace' else 1 if args.eager_depth else 0)
                report['completed'].append('paired_interval')
                report['post_interval_hud']=wait_for_stable_hud(pid,game,15,still_running)
                report['completed'].append('post_interval_hud')
        if args.cpu_collector:
            runs=[]
            for index in range(args.cpu_repeat):
                path=out/('cpu-collector.json' if not index else f'cpu-collector-{index+1}.json')
                script('read_cpu_collector.py','--pid',pid,'--out',path,
                       *(['--peer'] if args.cpu_peer else ['--scene'] if args.cpu_scene else ['--fine'] if args.cpu_fine else ['--query'] if args.cpu_query else []))
                runs.append(json.loads(path.read_text()))
                if not index:
                    report['cpu_collector']=runs[0]
                    report['completed'].append('cpu_collector')
            if args.cpu_repeat>1:
                report['cpu_collector_repeat']=runs
                report['completed'].append('cpu_collector_repeat')
            def collector_running():
                check()
                return pid in boot.process_ids('Cyberpunk2077.exe')
            report['post_collector_hud']=wait_for_stable_hud(pid,game,15,collector_running)
            report['completed'].append('post_collector_hud')
        if args.geometry_cost:
            from geometry_cost_report import NAMES as cost_names,LiveProbe,run as run_cost,validate as validate_cost,ceiling as cost_ceiling
            cost_symbols=resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',list(cost_names))
            probe=LiveProbe(reader,cost_symbols,pid,check)
            def cost_image(row):
                if not row['control']:return
                request=simulator/'screenshot_request.json'
                if request.exists():raise RuntimeError('Another screenshot request is pending')
                requested=time.time();request.write_text(json.dumps(dict(eye='both',layer='all')))
                deadline=time.monotonic()+10
                while time.monotonic()<deadline:
                    check();status=simulator/'screenshot_status.json'
                    if status.exists() and status.stat().st_mtime>=requested:
                        (out/f"geometry-cost-{row['label']}.bmp").write_bytes((simulator/'screenshot.bmp').read_bytes());return
                    time.sleep(.1)
                raise TimeoutError('No fresh simulator screenshot during the cost interval')
            cost_results,cost_cleanup=run_cost(probe,game/'red4ext/plugins/cyberpunkvrport/config/vrport.ini',on_interval=cost_image)
            (out/'geometry-cost.json').write_text(json.dumps(dict(results=cost_results,cleanup=cost_cleanup),indent=2)+chr(10),encoding='utf-8')
            validate_cost(cost_results)
            if any(cost_cleanup.values()):raise RuntimeError('Geometry cost controls were not restored')
            report['geometry_cost']=dict(ceiling=cost_ceiling(cost_results),cleanup=cost_cleanup,
                intervals=[{key:value for key,value in row.items() if key!='rawGroups'} for row in cost_results])
            report['completed'].append('geometry_cost')
            def cost_running():
                check()
                return pid in boot.process_ids('Cyberpunk2077.exe')
            report['post_cost_hud']=wait_for_stable_hud(pid,game,15,cost_running)
            report['completed'].append('post_cost_hud')
        report['both_eye_images']=screenshot('stereo')
        report['completed'].append('both_eye_images')
        packet_test('four_frame_packets', 4)
        packet_test('repeat_packet_request', 1)
        script('profile_prepare.py', '--pid', pid, '--seconds', 8, '--out', out / 'cpu_prepare.json')
        report['completed'].append('cpu_prepare')
        if args.retained_geometry:clear_retained()
        report['controls'] = {name: read32(name) for name in names[1:9]}
        report['controls']['CyberpunkVR_RegisteredIntentEnable']=clear_selector_tracking()
        if args.retained_geometry:
            for name in ('Request','State','Verified'):report['controls']['CyberpunkVR_RetainedGeometry'+name]=read32('CyberpunkVR_RetainedGeometry'+name)
        more = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',
                       ['CyberpunkVR_NativeStereoProbeRequest','CyberpunkVR_NativeStereoProbeState','CyberpunkVR_NativeStereoAllDraws'])
        for name in ('CyberpunkVR_NativeStereoProbeRequest','CyberpunkVR_NativeStereoProbeState','CyberpunkVR_NativeStereoAllDraws'):
            report['controls'][name] = struct.unpack('<I',reader.read(int(more[name],16),4))[0]
        if args.family_manifest:
            family = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_StereoFamilyObserve'])
            report['controls']['CyberpunkVR_StereoFamilyObserve'] = struct.unpack('<I', reader.read(int(family['CyberpunkVR_StereoFamilyObserve'], 16), 4))[0]
        if args.native_constants:
            controls = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_ConstantCaptureEnable', 'CyberpunkVR_ConstantBankRequest', 'CyberpunkVR_ConstantBankPairCount', 'CyberpunkVR_ConstantBankPrepareGeometry', 'CyberpunkVR_ConstantBankCompareDepth'])
            for name in ('CyberpunkVR_ConstantCaptureEnable', 'CyberpunkVR_ConstantBankRequest', 'CyberpunkVR_ConstantBankPairCount', 'CyberpunkVR_ConstantBankPrepareGeometry', 'CyberpunkVR_ConstantBankCompareDepth'):
                report['controls'][name] = struct.unpack('<I', reader.read(int(controls[name], 16), 4))[0]
        freeze = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll', ['CyberpunkVR_GeometryFreezeEnable'])
        report['controls']['CyberpunkVR_GeometryFreezeEnable'] = struct.unpack('<I', reader.read(int(freeze['CyberpunkVR_GeometryFreezeEnable'], 16), 4))[0]
        preparation_names=('CyberpunkVR_PreparationRequest','CyberpunkVR_PreparationState','CyberpunkVR_PreparationPairRequest',
                           'CyberpunkVR_PreparationRunRequest','CyberpunkVR_PreparationRunState','CyberpunkVR_PreparationRunStop','CyberpunkVR_EagerDepthMode',
                           'CyberpunkVR_PreparationGraphRequest','CyberpunkVR_PreparationGraphState','CyberpunkVR_PreparationScheduleMode','CyberpunkVR_PreparationCameraTrace','CyberpunkVR_PreparationAllStages',
                           'CyberpunkVR_SinglePassTraceRequest','CyberpunkVR_SinglePassTraceState','CyberpunkVR_SinglePassTraceNodeFilter',
                           'CyberpunkVR_EarlyDepthRequest','CyberpunkVR_EarlyDepthState','CyberpunkVR_EarlyDepthWholeTarget','CyberpunkVR_EarlyDepthRejectCommit',
                           'CyberpunkVR_EarlyDepthSkinning',
                           'CyberpunkVR_EarlyDepthAlphaReference',
                           'CyberpunkVR_EarlyDepthAlphaOptimized',
                           'CyberpunkVR_EarlyDepthSkinnedAlpha',
                           'CyberpunkVR_EarlyDepthMaterialCapture',
                           'CyberpunkVR_EarlyDepthMaterialHintCount',
                           'CyberpunkVR_EarlyDepthRunRequest','CyberpunkVR_EarlyDepthRunState','CyberpunkVR_EarlyDepthRunStop',
                           'CyberpunkVR_NativeStereoLiveBindings','CyberpunkVR_NativeStereoLiveFailure',
                           'CyberpunkVR_ObservationProfileEnable','CyberpunkVR_PairedGeometryRequest','CyberpunkVR_PairedGeometryState','CyberpunkVR_PairedGeometryOrdinal','CyberpunkVR_PairedGeometryFamily',
                           'CyberpunkVR_PreparationOwnedGraphMode','CyberpunkVR_PreparationGraphJobsMode',
                           'CyberpunkVR_PreparationDefinitionMode','CyberpunkVR_PreparationDefinitionRequest',
                           'CyberpunkVR_OwnedExecutionMode','CyberpunkVR_OwnedExecutionRequest','CyberpunkVR_OwnedExecutionClosing',
                           'CyberpunkVR_OwnedSceneSplitEnable',
                           'CyberpunkVR_OwnedResourceReplayEnable','CyberpunkVR_RetainedSourceStreamEnable','CyberpunkVR_RetainedIndirectSnapshotsEnable',
                           'CyberpunkVR_RetainedSourceCbvEnable','CyberpunkVR_SourceCbvBindingEnable','CyberpunkVR_RetainedSourceTablesEnable','CyberpunkVR_RetainedSourceDetailsEnable',
                           'CyberpunkVR_RetainedSourceCbvSnapshotsEnable','CyberpunkVR_SourceCbvSnapshotsEnable','CyberpunkVR_SourceCbvSnapshotsUploadEnable',
                           'CyberpunkVR_NativeIaSnapshotsEnable','CyberpunkVR_NativeInstanceObserverEnable','CyberpunkVR_NativeInstanceObserverResetRequest',
                           'CyberpunkVR_SourceSrvBindingEnable','CyberpunkVR_StructuredBufferSnapshotsEnable',
                           'CyberpunkVR_UploadReservationEnable','CyberpunkVR_UploadReservationPrepareRequest','CyberpunkVR_UploadReservationAcceptRequest',
                           'CyberpunkVR_GeometryCostProbe','CyberpunkVR_GeometryCostNode',
                           'CyberpunkVR_PreparedPairRequest','CyberpunkVR_PreparedPairState','CyberpunkVR_PreparedPairSelection','CyberpunkVR_PreparedPairInputFault','CyberpunkVR_PreparedActualMembershipEnable','CyberpunkVR_RetainedFixedAssetsEnable','CyberpunkVR_RetainedBorrowedTablesEnable','CyberpunkVR_RetainedFullT5Enable','CyberpunkVR_PreparedNativeExportEnable','CyberpunkVR_PreparedVrsRefinementEnable','CyberpunkVR_PreparedTailEnable','CyberpunkVR_PreparedInfixEnable','CyberpunkVR_PreparedCurrentEnable','CyberpunkVR_SubmissionFrontierEnable')
        if args.scene_access:preparation_names+=('CyberpunkVR_SceneAccessEnable','CyberpunkVR_SceneChildEnable')
        preparation = resolve(pid, game / 'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',list(preparation_names))
        for name in preparation_names:
            report['controls'][name] = struct.unpack('<I', reader.read(int(preparation[name], 16), 4))[0]
        collector=resolve(pid,game/'red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll',['CyberpunkVR_CpuCollectorRequest','CyberpunkVR_CpuCollectorState'])
        for name in ('CyberpunkVR_CpuCollectorRequest','CyberpunkVR_CpuCollectorState'):
            report['controls'][name]=struct.unpack('<I',reader.read(int(collector[name],16),4))[0]
        if any(report['controls'].values()):
            raise RuntimeError('A test control was not restored')
        report['completed'].append('controls_restored')
        for _ in range(20):
            check()
            time.sleep(.1)
        report['completed'].append('crash_stability')
        report['passed'] = report['completed'] == plan
    except Exception as error:
        report['error'] = str(error)
        crash = boot.crash_report(pid, started) if pid else None
        if crash:
            report['crash'] = crash
        elif pid and pid in boot.process_ids('Cyberpunk2077.exe') and not startup:
            try:
                script('read_startup_evidence.py', '--pid', pid, '--out', out / 'startup-evidence.json', timeout=10)
            except Exception as evidence_error:
                report['startupEvidenceError'] = str(evidence_error)
    finally:
        if settings_probe and settings_probe.baseline is not None and not settings_probe.restored and pid in boot.process_ids('Cyberpunk2077.exe'):
            try:report['nativeSettingsCleanup']=settings_probe.restore_live()
            except Exception as error:report['nativeSettingsCleanupError']=str(error);report['passed']=False
        if retained_owned:
            try:clear_retained()
            except Exception as error:
                report['retainedCleanupError']=str(error);report['passed']=False
        if reader:
            if pid in boot.process_ids('Cyberpunk2077.exe'):
                try:report['selectorTrackingCleanup']=clear_selector_tracking()
                except Exception as error:report['selectorTrackingCleanupError']=str(error);report['passed']=False
            reader.close()
        if pid:
            try:
                report['shutdown'] = close_game(pid, game / 'bin/x64/Cyberpunk2077.exe')
            except Exception as error:
                report['shutdownError'] = str(error)
                report['passed'] = False
            try:
                report['shutdownObservation']=observe_shutdown(pid,started,baseline_reporters)
                if report['shutdownObservation']['crash'] or report['shutdownObservation']['reporters']:
                    report['crash']=report['shutdownObservation']['crash'] or 'New CrashReporter during shutdown'
                    report['passed']=False
            except Exception as error:
                report['shutdownCheckError']=str(error);report['passed']=False
        try:
            boot.close_reporters(boot.new_reporters(baseline_reporters))
        except Exception as error:
            report['reporterCleanupError']=str(error);report['passed']=False
        try:
            if settings_probe:settings_probe.remove_after_exit()
        except Exception as error:
            report['nativeSettingsModCleanupError']=str(error);report['passed']=False
        try:
            if pid and pid in boot.process_ids('Cyberpunk2077.exe'):
                raise RuntimeError('Game still owns simulator settings; restoration deferred')
            report['previewRestored'] = preview.restore()
        except Exception as error:
            report['previewRestoreError'] = str(error)
            report['passed'] = False
        report['remaining'] = [name for name in plan if name not in report['completed']]
        report['elapsedSeconds'] = time.time() - started
        (out / 'summary.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        printed={key:value for key,value in report.items() if not isinstance(value,(dict,list)) or
                 key in ('plan','completed','remaining','shutdown','shutdownObservation','controls')}
        printed['reportFile']=str(out/'summary.json')
        print(json.dumps(printed, indent=2), flush=True)
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
