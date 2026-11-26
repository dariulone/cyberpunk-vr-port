"""Compare two resource census reports from the same running process."""
import argparse
import json
from pathlib import Path


def compare(before, after):
    if not before.get('pid') or before['pid'] != after.get('pid'):
        raise ValueError('Reports must come from the same process; group IDs are session-local')
    if any(not row.get('consistent') for report in (before, after) for row in report['groups']):
        raise ValueError('A census endpoint contains concurrent, inconsistent counters; request fresh quiescent snapshots before calculating a leak delta')
    old = {row['groupId']: row for row in before['groups']}
    changed = []
    for row in after['groups']:
        prior = old.get(row['groupId'], {})
        delta = {name: row[name] - prior.get(name, 0) for name in ('created', 'released', 'live', 'createdBytes', 'releasedBytes', 'liveBytes')}
        if any(delta.values()):
            changed.append({'kind': row['kind'], 'groupId': row['groupId'],
                            'heapType': row['heapType'], 'width': row['width'], 'height': row['height'], 'format': row['format'],
                            'flags': row['flags'], 'callerModule': row['callerModule'], 'callerRva': row['callerRva'],
                            'stack': row['stack'], 'overflow': row['overflow'], 'consistent': row['consistent'] and prior.get('consistent', 1),
                            'delta': delta})
    changed.sort(key=lambda row: (row['kind'], -row['delta']['liveBytes']))
    return {'pid': after['pid'], 'elapsedMs': after['tickMs'] - before['tickMs'],
            'committedLiveBytesDelta': sum(row['delta']['liveBytes'] for row in changed if row['kind'] == 'committed'),
            'placedFootprintLiveBytesDelta': sum(row['delta']['liveBytes'] for row in changed if row['kind'] == 'placed-footprint'),
            'coverage': {key: after.get(key) for key in ('attachFailures', 'cookieLimitDrops', 'groupOverflows', 'reentrantDrops', 'invalidAllocations', 'modulePinFailures')},
            'groups': changed}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('before', type=Path)
    parser.add_argument('after', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    result = compare(json.loads(args.before.read_text(encoding='utf-8')), json.loads(args.after.read_text(encoding='utf-8')))
    args.out.write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({key: result[key] for key in ('pid', 'elapsedMs', 'committedLiveBytesDelta', 'placedFootprintLiveBytesDelta', 'coverage')}, indent=2))
