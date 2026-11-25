import math
import struct
import unittest

from quest_face_probe import FaceSamples, osc_messages


def string(value):
    raw = value.encode() + b"\0"
    return raw + b"\0" * (-len(raw) % 4)


def message(name, value):
    return string('/sl/xrfb/facew/' + name) + string(',f') + struct.pack('>f', value)


def bundle(*messages):
    return b'#bundle\0' + b'\0' * 8 + b''.join(struct.pack('>I', len(m)) + m for m in messages)


class FaceProbeTests(unittest.TestCase):
    def test_split_bundles_preserve_individual_freshness(self):
        samples = FaceSamples()
        samples.accept(osc_messages(bundle(message('LipCornerPullerL', 0.75))), 10.0)
        samples.accept(osc_messages(bundle(message('EyesClosedL', 1))), 10.4)
        fresh = samples.fresh(10.6)
        self.assertNotIn('/sl/xrfb/facew/LipCornerPullerL', fresh)
        self.assertEqual(fresh['/sl/xrfb/facew/EyesClosedL'], 1)
        self.assertEqual(samples.fresh(11), {})

    def test_nested_bundle(self):
        self.assertEqual(len(osc_messages(bundle(bundle(message('JawDrop', 0.5))))), 1)

    def test_reject_truncated_and_oversized_elements(self):
        for packet in (message('JawDrop', 1)[:-4], b'#bundle\0',
                       bundle(message('JawDrop', 1))[:-4],
                       b'#bundle\0' + b'\0' * 8 + struct.pack('>I', 0xffffffff)):
            with self.subTest(packet=packet), self.assertRaises(ValueError):
                osc_messages(packet)

    def test_reject_invalid_values(self):
        samples = FaceSamples()
        for value in (math.nan, math.inf, -0.1, 1.1):
            samples.accept(osc_messages(message('JawDrop', value)), 0)
        samples.accept([('/sl/xrfb/facew/JawDrop', [True]),
                        ('/sl/xrfb/facew/JawDrop', [0.5, 0.2]),
                        ('/unrelated', [0.5])], 0)
        self.assertEqual(samples.accepted, 0)

    def test_limit_nesting(self):
        packet = message('JawDrop', 0.5)
        for _ in range(10):
            packet = bundle(packet)
        with self.assertRaises(ValueError):
            osc_messages(packet)

    def test_ignore_unknown_message_type_in_bundle(self):
        unknown = string('/unrelated') + string(',b') + b'\0' * 4
        messages = osc_messages(bundle(unknown, message('JawDrop', 0.5)))
        self.assertEqual(messages, [('/sl/xrfb/facew/JawDrop', [0.5])])


if __name__ == '__main__':
    unittest.main()
