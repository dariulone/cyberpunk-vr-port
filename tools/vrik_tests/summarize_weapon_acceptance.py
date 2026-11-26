"""Check a saved per-frame CPVR magazine take. No game, simulator, third-party APK or extra package required."""
import argparse
import json
import statistics
from pathlib import Path


def summarize(take):
    air = take['air']
    h0, h1, control = (air[k] for k in ('hapticBefore','hapticAfter','positiveControl'))
    a, b = air['before'], air['after']
    assert air['hapticsEnabled'] and b['whooshes'] > a['whooshes'] and b['serial'] == a['serial'] and b['hits'] == a['hits']
    assert h0[:3] == h1[:3] and h0[7] == h1[7] and control[0] > h1[0] and control[7] > h1[7]
    rows = {}
    for family, t in take['magazines'].items():
        assert not t.get('invalid') and t['item'] == f'Items.Preset_{family}_Default', family
        e = t['events']
        assert 2 <= len(e) <= len(t['inputIntervalsMs']) + 1 and t['original'] - t['remaining'] == len(e), family
        last_hi = t['begin']
        for i, event in enumerate(e, 1):
            assert last_hi <= event['lo'] <= event['hi'] <= t['prevTime'], family
            assert event['hi'] - event['lo'] <= .10 and event['delta'] == 1 and event['left'] == t['original'] - i, family
            last_hi = event['hi']
        # Each shot is known only between the previous sample and the sample that saw it.
        # Bound the MEAN over N-1 gaps using first/last brackets. Do not infer a maximum rate.
        scale = 1000 / (len(e) - 1)
        rows[family] = {'rounds':len(e), 'input_ms':round(statistics.mean(t['inputIntervalsMs']),1),
                        'mean_interval_ms':[round((e[-1]['lo']-e[0]['hi'])*scale,1), round((e[-1]['hi']-e[0]['lo'])*scale,1)],
                        'peak_deg':round(t['peakDeg'],2), 'settled_deg':round(t['settledDeg'],3)}
    assert rows, 'no magazine takes'
    return {'air_whooshes':b['whooshes']-a['whooshes'],'air_haptic_events':h1[7]-h0[7],'magazines':rows}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path)
    print(json.dumps(summarize(json.loads(parser.parse_args().capture.read_text(encoding='utf-8'))), indent=2))
