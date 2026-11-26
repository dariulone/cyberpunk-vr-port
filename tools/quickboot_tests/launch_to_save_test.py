import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, call, patch

import launch_to_save as boot


class ContinueInputTests(unittest.TestCase):
    def test_continue_phase_without_positive_panel_proof_never_allows_input(self):
        status=dict(phase='awaiting_continue',player=True,worldValid=True,preGame=False,loading=True,paused=False)
        self.assertFalse(boot.can_press_continue(status))
        for value in (False,None,0,1,'true',{},[]):
            with self.subTest(proof=value):
                self.assertFalse(boot.can_press_continue(dict(status,continueReady=value)))
        self.assertTrue(boot.can_press_continue(dict(status,continueReady=True)))

    def test_continue_panel_still_requires_a_loaded_player_and_active_loading_screen(self):
        status=dict(phase='awaiting_continue',continueReady=True,player=True,worldValid=True,
                    preGame=False,loading=True,paused=False)
        self.assertTrue(boot.can_press_continue(status))
        for key,value in (('phase','loading'),('phase','world_ready'),('player',False),('worldValid',False),
                          ('preGame',True),('loading',False),('paused',True)):
            with self.subTest(key=key,value=value):
                self.assertFalse(boot.can_press_continue(dict(status,**{key:value})))
        for key in status:
            incomplete=status.copy();del incomplete[key]
            with self.subTest(missing=key):self.assertFalse(boot.can_press_continue(incomplete))
        self.assertFalse(boot.can_press_continue(None))

    def test_focus_and_release_are_processed_before_returning_to_codex(self):
        import ctypes
        events=[]
        def send(_count,pointer,_size):
            value=ctypes.cast(pointer,ctypes.POINTER(boot.Input)).contents
            events.append(('key',value.payload.key.flags))
            return 1
        row=dict(hwnd=100,**{'class':boot.GAME_CLASS})
        with patch.object(boot,'process_id',return_value=77),patch.object(boot,'focus_window',side_effect=lambda _:events.append('focus')),\
             patch.object(boot.user32,'GetForegroundWindow',return_value=100),patch.object(boot.user32,'SendInput',side_effect=send),\
             patch.object(boot.time,'sleep',side_effect=lambda seconds:events.append(('wait',seconds))),\
             patch.object(boot,'restore_foreground',side_effect=lambda *_:events.append('restore')):
            boot.press_continue(row,77,200,True)
        self.assertEqual(events,['focus',('wait',.25),('key',8),('wait',.25),('key',10),('wait',.25),'restore'])

    def test_lost_focus_never_sends_the_continue_key(self):
        row=dict(hwnd=100,**{'class':boot.GAME_CLASS})
        with patch.object(boot,'process_id',side_effect=[77,77,99]),patch.object(boot,'focus_window'),\
             patch.object(boot.user32,'GetForegroundWindow',return_value=100),patch.object(boot.user32,'SendInput') as send,\
             patch.object(boot.time,'sleep'),patch.object(boot,'restore_foreground') as restore:
            with self.assertRaises(RuntimeError):boot.press_continue(row,77,200,True)
        send.assert_not_called();restore.assert_called_once_with(200,77)


class AutoProceedTests(unittest.TestCase):
    def setUp(self):
        self.send=Mock()
        self.auto=boot.AutoProceed(self.send)
        self.dialog=dict(phase='loading',fired=True,preGame=True,
                         loadConfirmation=dict(kind='modded-save',ready=True))
        self.loading=dict(phase='awaiting_continue',continueReady=True,player=True,worldValid=True,
                          preGame=False,loading=True,paused=False)

    def test_only_recognized_owned_save_dialog_can_be_confirmed(self):
        for change in (dict(fired=False),dict(preGame=False),dict(phase='world_ready'),
                       dict(loadConfirmation=dict(kind='delete-save',ready=True)),
                       dict(loadConfirmation=dict(kind='modded-save',ready=False))):
            self.assertFalse(self.auto.update(dict(self.dialog,**change),0))
        self.send.assert_not_called()
        self.assertTrue(self.auto.update(self.dialog,0))
        self.send.assert_called_once_with('modded-save')

    def test_loading_needs_native_readiness_and_stops_when_world_is_ready(self):
        self.assertFalse(self.auto.update(self.loading,0))
        self.assertTrue(self.auto.update(self.loading,0,loading_ready=True))
        world=dict(phase='world_ready',player=True,worldValid=True,preGame=False,loading=False,paused=False,stableFrames=40)
        self.assertFalse(self.auto.update(world,4,loading_ready=True))
        self.send.assert_called_once_with('loading-screen')

    def test_retry_is_bounded_and_requires_the_same_visible_prompt(self):
        self.assertTrue(self.auto.update(self.dialog,0))
        self.assertFalse(self.auto.update(self.dialog,2.99))
        self.assertFalse(self.auto.update(dict(self.dialog,loadConfirmation={}),3))
        for now in (3,6,9): self.assertTrue(self.auto.update(self.dialog,now))
        with self.assertRaises(RuntimeError): self.auto.update(self.dialog,12)
        self.assertEqual(self.send.call_count,4)

    def test_simulator_input_releases_button_and_does_not_focus_a_window(self):
        import sys
        gate=Mock();gate.__enter__=Mock(return_value=gate);gate.__exit__=Mock(return_value=False)
        factory=Mock(return_value=gate)
        with patch.dict(sys.modules,{'simulator_input':Mock(PadGate=factory)}),patch.object(boot.time,'sleep'),\
             patch.object(boot,'focus_window') as focus,patch.object(boot.user32,'SendInput') as keyboard:
            boot.press_simulator_confirm(77,Path('game'),Path('simulator.dll'))
        self.assertEqual(gate.jump.call_args_list,[call(False),call(True),call(False)])
        gate.__exit__.assert_called_once();focus.assert_not_called();keyboard.assert_not_called()


class NativeResumeGateTests(unittest.TestCase):
    def test_continue_layer_mode_requires_fresh_mode_two_with_separate_panel_proof(self):
        for mode in (0,1,3,4,-1):
            gate=boot.NativeResumeGate(expected_mode=2)
            for tick in range(1,5):self.assertFalse(gate.observe(mode,tick,tick*.2))
        gate=boot.NativeResumeGate(expected_mode=2)
        self.assertFalse(gate.observe(2,10,0.0))
        self.assertTrue(gate.observe(2,11,0.11))
        self.assertFalse(gate.observe(2,11,1.0))
        self.assertFalse(gate.observe(2,12,1.1))
        self.assertTrue(gate.observe(2,13,1.21))
        self.assertFalse(gate.observe(-1,0,2.0))
        self.assertFalse(gate.observe(2,14,2.1))
        self.assertTrue(gate.observe(2,15,2.21))
        status=dict(phase='awaiting_continue',player=True,worldValid=True,preGame=False,loading=True,paused=False)
        self.assertFalse(boot.can_press_continue(status))
        self.assertTrue(boot.can_press_continue(dict(status,continueReady=True)))

    def test_menu_visibility_is_not_native_loading_completion(self):
        gate=boot.NativeResumeGate()
        for i in range(10):self.assertFalse(gate.observe(2,i+1,i*.1))
        self.assertFalse(gate.observe(4,11,1.0))
        self.assertTrue(gate.observe(4,12,1.11))

    def test_stale_sample_or_reopened_loading_resets_wait(self):
        gate=boot.NativeResumeGate()
        self.assertFalse(gate.observe(4,1,0))
        self.assertFalse(gate.observe(4,1,1))
        self.assertFalse(gate.observe(2,2,2))
        self.assertFalse(gate.observe(4,3,3))
        self.assertTrue(gate.observe(4,4,3.11))


class FocusSequenceTests(unittest.TestCase):
    def setUp(self):
        self.now = 0.0
        self.activate = Mock()
        self.focus = boot.LaunchFocus(77, 200, lambda: self.now, self.activate)
        self.row = dict(hwnd=100, **{'class': boot.GAME_CLASS})
        self.pid = patch.object(boot, 'process_id', return_value=77)
        self.pid.start()
        self.addCleanup(self.pid.stop)

    def test_five_seconds_game_then_one_second_codex(self):
        self.focus.update(self.row)
        for now in (0.0, 4.99):
            self.now = now
            self.focus.update(self.row)
        self.activate.assert_not_called()
        self.now = 5.0
        self.focus.update(self.row)
        self.activate.assert_called_once_with(100)
        self.now = 5.99
        self.focus.update(self.row)
        self.assertFalse(self.focus.complete)
        self.now = 6.0
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_args_list, [call(100), call(200)])
        self.assertEqual(self.focus.report(), dict(verified=True, gameAfterSeconds=5.0, codexAfterGameSeconds=1.0))
        self.now = 30.0
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_count, 2)

    def test_launcher_and_other_process_do_not_start_clock(self):
        self.focus.update(dict(self.row, **{'class': boot.LAUNCHER_CLASS}))
        self.focus.update(None)
        with patch.object(boot, 'process_id', return_value=999):
            self.focus.update(self.row)
        self.assertIsNone(self.focus.seen)
        self.now = 100.0
        self.focus.update(self.row)
        self.assertEqual(self.focus.seen, 100.0)

    def test_late_launcher_resets_preliminary_game_window_timer(self):
        self.focus.update(self.row)
        self.now = 2.0
        self.focus.update(self.row, launcher_visible=True)
        self.assertIsNone(self.focus.seen)
        self.now = 3.0
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.activate.assert_not_called()
        self.now = 8.0
        self.focus.update(self.row)
        self.activate.assert_called_once_with(100)

    def test_delay_measured_after_actual_game_activation(self):
        def delayed(_):
            self.now += .2
        self.activate.side_effect = delayed
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.assertEqual(self.focus.game_at, 5.2)
        self.now = 6.0
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_count, 1)
        self.now = 6.21
        self.focus.update(self.row)
        self.assertTrue(self.focus.complete)

    def test_activation_failure_never_publishes_success(self):
        self.focus.update(self.row)
        self.now = 5.0
        self.activate.side_effect = RuntimeError('blocked')
        with self.assertRaises(RuntimeError):
            self.focus.update(self.row)
        self.assertIsNone(self.focus.game_at)
        self.assertFalse(self.focus.complete)

    def test_codex_failure_never_publishes_completion(self):
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.now = 6.0
        self.activate.side_effect = RuntimeError('closed')
        with self.assertRaises(RuntimeError):
            self.focus.update(self.row)
        self.assertFalse(self.focus.complete)

    def test_transient_game_refusal_retries_and_starts_codex_delay_after_verification(self):
        self.activate.side_effect = [boot.ForegroundRejected('busy'), None, None]
        self.focus.update(self.row)
        self.now = 5.0
        self.assertIn('retrying', self.focus.update(self.row))
        self.assertIsNone(self.focus.game_at)
        self.now = 5.05
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_count, 1)
        self.now = 5.2
        self.focus.update(self.row)
        self.assertEqual(self.focus.game_at, 5.2)
        self.now = 6.0
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_args_list, [call(100), call(100)])
        self.now = 6.21
        self.focus.update(self.row)
        self.assertEqual(self.activate.call_args_list, [call(100), call(100), call(200)])
        self.assertTrue(self.focus.complete)

    def test_transient_codex_refusal_retries_without_refocusing_game(self):
        self.activate.side_effect = [None, boot.ForegroundRejected('busy'), None]
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.now = 6.0
        self.assertIn('retrying', self.focus.update(self.row))
        self.assertFalse(self.focus.complete)
        self.now = 6.2
        self.focus.update(self.row)
        self.assertTrue(self.focus.complete)
        self.assertEqual(self.activate.call_args_list, [call(100), call(200), call(200)])

    def test_refusal_deadline_is_not_extended_by_each_retry_or_missing_window(self):
        self.activate.side_effect = boot.ForegroundRejected('busy')
        self.focus.update(self.row)
        for now in (5.0, 5.2, 6.8):
            self.now = now
            self.focus.update(self.row)
            self.assertIsNone(self.focus.game_at)
        attempts = self.activate.call_count
        self.now = 7.0
        with self.assertRaisesRegex(TimeoutError, 'within 2.0s'):
            self.focus.update(None)
        self.assertEqual(self.activate.call_count, attempts)
        self.assertFalse(self.focus.complete)

    def test_codex_has_its_own_bounded_retry_deadline(self):
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.activate.side_effect = boot.ForegroundRejected('busy')
        for now in (6.0, 7.5):
            self.now = now
            self.focus.update(self.row)
        self.now = 8.0
        with self.assertRaises(TimeoutError):
            self.focus.update(self.row)
        self.assertEqual(self.focus.game_at, 5.0)
        self.assertFalse(self.focus.complete)

    def test_retry_never_targets_launcher_or_another_process(self):
        self.activate.side_effect = boot.ForegroundRejected('busy')
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.now = 5.2
        self.focus.update(dict(self.row, **{'class': boot.LAUNCHER_CLASS}))
        with patch.object(boot, 'process_id', return_value=999):
            self.focus.update(self.row)
        self.activate.assert_called_once_with(100)
        self.assertIsNone(self.focus.game_at)

    def test_late_launcher_cancels_pending_retry_and_restarts_real_viewport_clock(self):
        self.activate.side_effect = boot.ForegroundRejected('busy')
        self.focus.update(self.row)
        self.now = 5.0
        self.focus.update(self.row)
        self.now = 5.2
        self.focus.update(self.row, launcher_visible=True)
        self.now = 10.0
        self.activate.side_effect = None
        self.focus.update(self.row)
        self.assertEqual(self.focus.seen, 10.0)
        self.now = 14.99
        self.focus.update(self.row)
        self.activate.assert_called_once_with(100)
        self.now = 15.0
        self.focus.update(self.row)
        self.assertEqual(self.focus.game_at, 15.0)


class WindowTests(unittest.TestCase):
    def test_world_needs_a_visible_restored_game_viewport(self):
        row=dict(visible=True,minimized=False,width=2560,height=1440,**{'class':boot.GAME_CLASS})
        self.assertTrue(boot.viewport_ready(row))
        self.assertTrue(boot.viewport_ready(dict(row,width=1024,height=768)))
        self.assertTrue(boot.viewport_ready(dict(row,width=320,height=240)))
        for bad in (None,{},dict(row,visible=False),dict(row,minimized=True),dict(row,width=319),dict(row,height=239),
                    dict(row,**{'class':boot.LAUNCHER_CLASS})):
            self.assertFalse(boot.viewport_ready(bad))
        incomplete=row.copy();del incomplete['minimized'];self.assertFalse(boot.viewport_ready(incomplete))

    def test_attach_input_is_always_detached(self):
        ui = Mock()
        ui.IsWindow.return_value = True
        ui.IsIconic.return_value = False
        # initial check, attach loop, post-SetForegroundWindow check, final verification
        ui.GetForegroundWindow.side_effect = [100, 100, 200, 200]
        ui.GetWindowThreadProcessId.side_effect = lambda hwnd, _: {100: 10, 200: 20}[hwnd]
        ui.AttachThreadInput.return_value = True
        with patch.object(boot, 'user32', ui), patch.object(boot.kernel32, 'GetCurrentThreadId', return_value=30):
            boot.focus_window(200)
        self.assertEqual(ui.AttachThreadInput.call_args_list,
                         [call(30, 10, True), call(30, 20, True), call(30, 20, False), call(30, 10, False)])
        ui.keybd_event.assert_not_called()  # granted at once: no synthetic input
        ui.GetForegroundWindow.side_effect = [100, 100, 100, 100]
        with patch.object(boot, 'user32', ui), patch.object(boot.kernel32, 'GetCurrentThreadId', return_value=30):
            with self.assertRaises(boot.ForegroundRejected):
                boot.focus_window(200)
        self.assertEqual(ui.AttachThreadInput.call_args_list[-2:], [call(30, 20, False), call(30, 10, False)])
        self.assertEqual(ui.keybd_event.call_args_list, [call(0x12, 0, 0, 0), call(0x12, 0, 2, 0)])

    def test_foreground_lock_is_released_by_one_alt_tap(self):
        ui = Mock()
        ui.IsWindow.return_value = True
        ui.IsIconic.return_value = False
        ui.GetForegroundWindow.side_effect = [100, 100, 100, 200]  # refused, then granted after ALT
        ui.GetWindowThreadProcessId.side_effect = lambda hwnd, _: {100: 10, 200: 20}[hwnd]
        ui.AttachThreadInput.return_value = True
        with patch.object(boot, 'user32', ui), patch.object(boot.kernel32, 'GetCurrentThreadId', return_value=30):
            boot.focus_window(200)
        self.assertEqual(ui.keybd_event.call_count, 2)
        self.assertEqual(ui.SetForegroundWindow.call_count, 2)
        self.assertEqual(ui.AttachThreadInput.call_args_list[-2:], [call(30, 20, False), call(30, 10, False)])

    def test_api_success_without_verified_foreground_is_retryable_not_success(self):
        ui = Mock()
        ui.IsWindow.return_value = True
        ui.IsIconic.return_value = False
        ui.GetForegroundWindow.return_value = 100
        ui.GetWindowThreadProcessId.return_value = 30
        ui.SetForegroundWindow.return_value = True
        with patch.object(boot, 'user32', ui), patch.object(boot.kernel32, 'GetCurrentThreadId', return_value=30):
            with self.assertRaises(boot.ForegroundRejected):
                boot.focus_window(200)
        ui.SendInput.assert_not_called()

    def test_missing_focus_target_is_fatal_not_retryable(self):
        with patch.object(boot.user32, 'IsWindow', return_value=False):
            with self.assertRaises(RuntimeError) as failed:
                boot.focus_window(200)
        self.assertNotIsInstance(failed.exception, boot.ForegroundRejected)

    def test_codex_package_identity_and_unique_window(self):
        row = dict(hwnd=200, visible=True, width=1000, height=700)
        with patch.object(boot, 'process_ids', return_value={7}), patch.object(boot, 'windows_for', return_value=[row]), \
             patch.object(boot, 'process_path', return_value=Path('C:/WindowsApps/OpenAI.Codex_1/app/ChatGPT.exe')):
            self.assertEqual(boot.find_codex_window(0), 200)
            with patch.object(boot, 'windows_for', return_value=[row, dict(row, hwnd=300, width=1200)]):
                self.assertEqual(boot.find_codex_window(300), 300)
                # two Codex windows and neither had focus: the larger one gets it back
                self.assertEqual(boot.find_codex_window(0), 300)
            with patch.object(boot, 'process_path', return_value=Path('C:/WindowsApps/OpenAI.ChatGPT_1/app/ChatGPT.exe')):
                with self.assertRaises(RuntimeError):
                    boot.find_codex_window(0)

    def test_duplicate_game_windows_rejected(self):
        row = dict(hwnd=100, visible=True, minimized=False, **{'class': boot.GAME_CLASS})
        self.assertEqual(boot.only_visible_window([row], boot.GAME_CLASS), row)
        self.assertIsNone(boot.only_visible_window([dict(row, minimized=True)], boot.GAME_CLASS))
        with self.assertRaises(RuntimeError):
            boot.only_visible_window([row, dict(row, hwnd=101)], boot.GAME_CLASS)


class StatusTests(unittest.TestCase):
    def test_missing_report_preserves_bounded_live_evidence_before_cleanup(self):
        for failed in (False,True):
            events=[]
            def capture(*args,**kwargs):
                events.append('capture')
                self.assertEqual(kwargs['timeout'],10)
                self.assertEqual(args[0][-2],'77')
                if failed:raise boot.subprocess.TimeoutExpired(args[0],10)
                return Mock(stdout='saved live threads',stderr='')
            with self.subTest(failed=failed), tempfile.TemporaryDirectory() as temporary, \
                 patch.object(boot,'ROOT',Path(temporary)),patch.object(boot.time,'monotonic',side_effect=[0,0,4]), \
                 patch.object(boot.time,'sleep'),patch.object(boot,'crash_report',return_value=None), \
                 patch.object(boot,'new_reporters',return_value={88}),patch.object(boot,'process_path',return_value=Path('CrashReporter.exe')), \
                 patch.object(boot,'windows_for',return_value=[]),patch.object(boot.subprocess,'run',side_effect=capture), \
                 patch.object(boot,'close_reporters',side_effect=lambda pids:events.append(('close',pids))):
                with self.assertRaisesRegex(RuntimeError,'no PID-matched report'):
                    boot.fail_for_crash(77,0,set(),'reporter appeared')
                self.assertEqual(events,['capture',('close',{88})])

    def test_existing_native_report_does_not_capture_live_dump(self):
        with patch.object(boot.time,'monotonic',side_effect=[0,0]),patch.object(boot,'crash_report',return_value='native report'), \
             patch.object(boot,'new_reporters',return_value={88}),patch.object(boot,'process_path',return_value=Path('CrashReporter.exe')), \
             patch.object(boot,'windows_for',return_value=[]),patch.object(boot.subprocess,'run') as capture, \
             patch.object(boot,'close_reporters') as close:
            with self.assertRaisesRegex(RuntimeError,'native report'):
                boot.fail_for_crash(77,0,set(),'reporter appeared')
            capture.assert_not_called();close.assert_called_once_with({88})

    def test_reflection_runtime_is_explicit_and_never_inherited(self):
        parent={'CVR_STEREO_REFLECTION_DXC':'old.dll','OTHER':'kept'}
        self.assertEqual(boot.reflection_environment(parent,None),{'OTHER':'kept'})
        compiler=Path('dxcompiler.dll')
        child=boot.reflection_environment(parent,compiler)
        self.assertEqual(child['CVR_STEREO_REFLECTION_DXC'],str(compiler.resolve()))
        self.assertEqual(child['OTHER'],'kept');self.assertEqual(parent['CVR_STEREO_REFLECTION_DXC'],'old.dll')

    def test_native_continue_requires_closed_ui_and_no_prior_gameplay_hud(self):
        status = dict(phase='world_ready', player=True, worldValid=True, preGame=False,
                      loading=False, paused=False, stableFrames=30,
                      menuEvidence=dict(menuBlackboard=False, menuControllers=[
                          dict(**{'class': 'gameuiInGameMenuGameController'}, visible=False),
                          dict(**{'class': 'CursorGameController'}, visible=True)]))
        sample = dict(menu=1, prepared=0, submitted=0)
        self.assertTrue(boot.can_confirm_native_continue(sample, status))
        for key, value in (('menu', 0), ('prepared', 1), ('submitted', 1)):
            self.assertFalse(boot.can_confirm_native_continue(dict(sample, **{key: value}), status))
        for key, value in (('loading', True), ('paused', True), ('stableFrames', 29), ('player', False), ('menuEvidence', {})):
            self.assertFalse(boot.can_confirm_native_continue(sample, dict(status, **{key: value})))
        for evidence in (dict(menuBlackboard=True, menuControllers=status['menuEvidence']['menuControllers']),
                         dict(menuBlackboard=False, menuControllers=[dict(**{'class': 'SingleplayerMenuGameController'}, visible=True)])):
            self.assertFalse(boot.can_confirm_native_continue(sample, dict(status, menuEvidence=evidence)))

    def test_world_requires_all_gameplay_signals(self):
        ready = dict(phase='world_ready', player=True, worldValid=True, preGame=False,
                     loading=False, paused=False, stableFrames=30)
        self.assertTrue(boot.world_ready(ready))
        self.assertFalse(boot.world_ready(None))
        for key, value in (('phase', 'loading'), ('player', False), ('worldValid', False),
                           ('preGame', True), ('loading', True), ('paused', True), ('stableFrames', 29)):
            self.assertFalse(boot.world_ready(dict(ready, **{key: value})), key)
            incomplete = ready.copy()
            del incomplete[key]
            self.assertFalse(boot.world_ready(incomplete), key)

    def test_status_requires_current_run(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'status.json'
            self.assertIsNone(boot.read_status(path, 'new'))
            path.write_text('{broken')
            self.assertIsNone(boot.read_status(path, 'new'))
            path.write_text(json.dumps(dict(runId='old', phase='world_ready')))
            self.assertIsNone(boot.read_status(path, 'new'))
            path.write_text(json.dumps(dict(runId='new', phase='world_ready')))
            self.assertEqual(boot.read_status(path, 'new')['phase'], 'world_ready')

    def test_crash_report_matches_pid(self):
        with tempfile.TemporaryDirectory() as temporary, patch.object(boot, 'REPORT_QUEUE', Path(temporary)):
            path = Path(temporary) / 'Cyberpunk2077-20260930-190000-77-88'
            path.mkdir()
            (path / 'stacktrace.txt').write_text('Access violation')
            self.assertIn('Access violation', boot.crash_report(77, 0))
            self.assertIsNone(boot.crash_report(78, 0))
            self.assertIsNone(boot.crash_report(77, path.stat().st_mtime + 1))


if __name__ == '__main__':
    unittest.main()
