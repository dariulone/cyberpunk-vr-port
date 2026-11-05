"""Survey: every normalize() whose input is a view vector from a point that is not the pixel's eye.

  kind A  normalize(P)          P a perspective divide through a camera-RELATIVE inverse (gCommonConstants rows 0-3 /
                                 8-11 in the RT libraries: the stereo centre / the eye respectively)
  kind B  normalize(row - P)    row = SharedPixelConsts row 0 (stereo centre) or CameraShaderConsts row 36 (the bound
                                 camera block's eye; the head centre in the RT nodes)
  kind C  normalize(Q)          Q = (a kind-A P) + small bias terms (the biased ray start)

For each hit: function, the matrix rows / origin row, and whether the direction reaches a TraceRay direction operand.
Usage: python view_survey.py DIR_OF_LL [glob]"""
import collections
import glob
import re
import sys
from pathlib import Path

DOT = re.compile(r'(?m)^  (%\d+) = call float @dx\.op\.dot3\.f32\(i32 55, float (%[\w.]+), float (%[\w.]+), float (%[\w.]+), '
                 r'float \2, float \3, float \4\)')


def functions(ir):
    for m in re.finditer(r'(?ms)^define [^@]*@"?\\?0?1?\??([\w.]+)[^\n]*\n(.*?)^}', ir):
        yield m.group(1).split('@@')[0], m.group(2)


def handles(ir, body):
    out = {}
    for m in re.finditer(r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandleForLib\.(\w+)\(', body):
        out[m.group(1)] = m.group(2)
    for m in re.finditer(r'(?m)^  (%\d+) = call %dx\.types\.Handle @dx\.op\.createHandle\(i32 57, i8 2, i32 \d+, i32 (\d+)', body):
        out[m.group(1)] = 'b' + m.group(2)
    return out


def analyse(name, body, hs):
    defs = {m.group(1): m.group(2) for m in re.finditer(r'(?m)^  (%[\w.]+) = (.*?)(?:  ;.*)?$', body)}
    loads = {}
    for v, d in defs.items():
        m = re.match(r'call %dx\.types\.CBufRet\.\w+ @dx\.op\.cbufferLoadLegacy\.\w+\(i32 59, %dx\.types\.Handle (%\d+), i32 (\d+)\)', d)
        if m:
            loads[v] = (hs.get(m.group(1), '?'), int(m.group(2)))

    def cb_of(v):
        m = re.match(r'extractvalue %dx\.types\.CBufRet\.\w+ (%\d+), (\d)', defs.get(v, ''))
        return loads.get(m.group(1)) if m else None

    def rows_under(v, depth=7, seen=None):
        seen = seen if seen is not None else set()
        if depth < 0 or v in seen or v not in defs:
            return set()
        seen.add(v)
        c = cb_of(v)
        if c:
            return {c}
        out = set()
        for o in re.findall(r'%[\w.]+', defs[v]):
            out |= rows_under(o, depth - 1, seen)
        return out

    def is_divide(v):
        return re.fullmatch(r'fdiv fast float (%[\w.]+), (%[\w.]+)', defs.get(v, ''))

    def matrix(v):
        rs = rows_under(v)
        by = collections.defaultdict(set)
        for cb, r in rs:
            by[cb].add(r)
        return {cb: sorted(r) for cb, r in by.items()}

    # uses map for taint
    uses = collections.defaultdict(list)
    for v, d in defs.items():
        for o in set(re.findall(r'%[\w.]+', d)):
            uses[o].append(v)
    traces = [m for m in re.finditer(r'call void @dx\.op\.traceRay\.[\w.]+\(i32 157, [^,]+, i32 \S+, i32 \S+, i32 \S+, i32 \S+, '
                                     r'i32 \S+, float (\S+), float (\S+), float (\S+), float \S+, float (\S+), float (\S+), float ([^,]+),', body)]
    dirs = {v for t in traces for v in t.group(4, 5, 6)}
    origins = {v for t in traces for v in t.group(1, 2, 3)}

    def reaches(starts, targets, limit=4000):
        seen, todo = set(), list(starts)
        while todo and len(seen) < limit:
            v = todo.pop()
            if v in seen:
                continue
            seen.add(v)
            if v in targets:
                return True
            todo += uses.get(v, [])
        return False

    hits = []
    for m in DOT.finditer(body):
        dot, xs = m.group(1), m.group(2, 3, 4)
        kind = what = None
        if all(is_divide(x) for x in xs):
            kind, what = 'A', matrix(xs[0])
        else:
            subs = [re.fullmatch(r'fsub fast float (%[\w.]+), (%[\w.]+)', defs.get(x, '')) for x in xs]
            if all(subs):
                a, b = subs[0].groups()
                ca, cbb = cb_of(a), cb_of(b)
                if ca and (is_divide(b) or rows_under(b)):
                    kind, what = 'B', '%s row %d - P' % ca
                elif cbb and (is_divide(a) or rows_under(a)):
                    kind, what = 'B', 'P - %s row %d' % cbb
            if not kind:
                adds = [re.fullmatch(r'fadd fast float (%[\w.]+), (%[\w.]+)', defs.get(x, '')) for x in xs]
                if all(adds):
                    inner = [o for o in adds[0].groups() if re.fullmatch(r'fsub fast float (%[\w.]+), %[\w.]+', defs.get(o, ''))]
                    if inner and is_divide(re.fullmatch(r'fsub fast float (%[\w.]+), %[\w.]+', defs[inner[0]]).group(1)):
                        kind, what = 'C', matrix(re.fullmatch(r'fsub fast float (%[\w.]+), %[\w.]+', defs[inner[0]]).group(1))
        if not kind:
            continue
        rsq = [u for u in uses.get(dot, []) if 'unary.f32(i32 25' in defs.get(u, '')]
        normed = [u for r in rsq for u in uses.get(r, [])]
        hits.append((kind, what, reaches(normed, dirs), reaches(normed, origins), dot))
    return hits, len(traces)


def main():
    src = Path(sys.argv[1])
    pattern = sys.argv[2] if len(sys.argv) > 2 else '*.ll'
    for f in sorted(src.glob(pattern)):
        ir = f.read_text(errors='replace')
        for name, body in functions(ir):
            hs = handles(ir, body)
            hits, ntrace = analyse(name, body, hs)
            for kind, what, to_dir, to_org, dot in hits:
                print('%s %-34s %s %-60s trace-dir=%s trace-org=%s %s' % (f.stem, name[:34], kind, str(what)[:60], to_dir, to_org, dot))


if __name__ == '__main__':
    main()
