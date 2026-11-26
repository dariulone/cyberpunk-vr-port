import unittest
from diff_reports import compare


def row(group, kind='committed', count=1, size=6553600, consistent=1):
    return dict(groupId=group, kind=kind, heapType=1, width=1280, height=1280, format=28, flags=0,
                callerModule='game.exe', callerRva='0x1234', stack=[], overflow=0, consistent=consistent,
                created=count, released=0, live=count, createdBytes=count * size, releasedBytes=0, liveBytes=count * size)


def report(rows, pid=17):
    return dict(pid=pid, tickMs=100, groups=rows)


class ReportDiffTests(unittest.TestCase):
    def test_placed_footprint_is_separate(self):
        diff = compare(report([row(0)]), report([row(0, count=2), row(1, 'placed-footprint', size=131072)]))
        self.assertEqual(diff['committedLiveBytesDelta'], 6553600)
        self.assertEqual(diff['placedFootprintLiveBytesDelta'], 131072)

    def test_either_inconsistent_endpoint_rejects_the_delta(self):
        for before, after in ((report([row(0, consistent=0)]), report([row(0)])),
                              (report([row(0)]), report([row(0, count=2, consistent=0)]))):
            with self.assertRaisesRegex(ValueError, 'inconsistent'):
                compare(before, after)

    def test_group_ids_cannot_cross_processes(self):
        with self.assertRaisesRegex(ValueError, 'same process'):
            compare(report([row(0)], 17), report([row(0)], 18))


if __name__ == '__main__':
    unittest.main()
