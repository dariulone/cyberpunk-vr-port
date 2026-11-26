"""Offline regression for async fixture cleanup status; no process is opened."""
import argparse
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import run as audit


class CleanupTests(unittest.TestCase):
    def fixture(self):
        obj = audit.Audit.__new__(audit.Audit)
        obj.bridge = Mock()
        obj.result = {'status': 'completed', 'cleanupErrors': [], 'fixtureToken': 'fixture'}
        obj.saved = {}; obj.sim = None; obj.head = None; obj.recording = False
        obj.owner = 'fixture'; obj.fg_original = None; obj.process = Mock(); obj.save = Mock()
        obj.plugin = Path(tempfile.gettempdir()) / 'nonexistent-render-audit-fixture'
        return obj

    def test_pending_is_polled_until_confirmed(self):
        obj = self.fixture()
        obj.bridge.call.side_effect = [{'complete': False, 'vehiclePending': True}, {'complete': True}]
        with patch.object(audit.time, 'sleep'), patch.object(audit.time, 'monotonic', return_value=0):
            obj.cleanup_owned('fixture')
        self.assertEqual(obj.bridge.call.call_count, 2)
        self.assertTrue(obj.result['cleanup']['complete'])

    def test_pending_failure_does_not_hide_restoration_or_token(self):
        obj = self.fixture()
        obj.saved = {'CyberpunkVR_XrRateLog': 0}
        obj.bridge.call.return_value = {'complete': False, 'vehiclePending': True}
        with patch.object(audit.time, 'monotonic', side_effect=[0, 6]):
            obj.close()
        self.assertEqual(obj.result['status'], 'cleanup-pending')
        self.assertEqual(obj.result['fixtureToken'], 'fixture')
        self.assertEqual(len(obj.result['cleanupErrors']), 1)
        obj.process.close.assert_called_once()
        obj.process.write32.assert_called_once_with('CyberpunkVR_XrRateLog', 0)
        obj.save.assert_called_once()

    def test_completed_body_with_cleanup_errors_is_failure(self):
        obj = Mock()
        obj.result = {'cleanupErrors': [{'error': 'pending'}]}
        args = argparse.Namespace(seconds=10, throttle=0)
        with patch.object(audit.argparse.ArgumentParser, 'parse_args', return_value=args), patch.object(audit, 'Audit', return_value=obj):
            with self.assertRaisesRegex(RuntimeError, 'Cleanup/restoration'):
                audit.main()
        obj.run.assert_called_once(); obj.close.assert_called_once()


if __name__ == '__main__':
    unittest.main()
