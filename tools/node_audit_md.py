"""Turn a CyberpunkVR_ProfDumpNodes() log dump into a named markdown node audit.

The DLL emits WINDOW TOTALS plus honest denominators; all normalisation happens here.

Why the denominators matter: neither number the DLL prints is a frame count. Present count is
inflated by DLSS frame generation, and node_dispatches_* counts node dispatches, because the
graph runner calls the executor directly for most nodes so almost every node is top-level.
The frame count is INFERRED here as the modal per-node call count (see infer_frames).

Why 'self' and not 'incl': the scene-pass driver (+0x1EC1D0) dispatches its scene passes
through the same hook, so inclusive time contains its children. Only self time may be
ranked or summed.

Why passes are matched by content: rtId is a build-order index, so the two views number their
passes differently and comparing rtId numbers between views invents view-only passes that do
not exist (see match_passes).

Usage:
  python tools/node_audit_md.py [--log <testbed.log>] [--out docs/vrcam_node_audit_v2.md]
                               [--dump N]
  --dump N picks the Nth-from-last dump in the log (0 = last, the default).
"""

import argparse
import os
import re
import sys

DEFAULT_LOG = (r"C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077"
               r"\red4ext\plugins\cyberpunkvrport\log\cyberpunkvrport.log")
DEFAULT_OUT = "docs/vrcam_node_audit_v2.md"


def split_dumps(lines):
    """Group the log into dumps. A dump starts at a CFG line and runs to the next one."""
    dumps, cur = [], None
    for ln in lines:
        if "[prof] CFG|" in ln:
            if cur:
                dumps.append(cur)
            cur = []
        if cur is not None:
            cur.append(ln)
    if cur:
        dumps.append(cur)
    return dumps


def parse_kv(line, tag):
    body = line.split("[prof] " + tag + "|", 1)[1].strip()
    out = {}
    for part in body.split("|"):
        if "=" in part:
            k, v = part.split("=", 1)
            out[k.strip()] = v.strip()
    return out


def match_passes(pairs, fm, fv):
    """Match MAIN passes to VRCAM passes by CONTENT, never by rtId number.

    rtId is the build-order index of a top-level pass scope, so one pass added or skipped
    earlier renumbers the whole tail; the two views' numbering is shifted relative to each
    other. Comparing by number invents 'view-only passes' that do not exist. A pass is
    therefore identified by the set of node names dispatched under it.
    """
    sig_m, sig_v, cost_m, cost_v = {}, {}, {}, {}
    for p in pairs:
        rt, nm = p["rtid"], p["name"]
        if p["calls_main"]:
            sig_m.setdefault(rt, set()).add(nm)
            cost_m[rt] = cost_m.get(rt, 0.0) + p["self_main"]
        if p["calls_vrcam"]:
            sig_v.setdefault(rt, set()).add(nm)
            cost_v[rt] = cost_v.get(rt, 0.0) + p["self_vrcam"]

    def jaccard(a, b):
        u = a | b
        return (len(a & b) / len(u)) if u else 0.0

    # Collapse passes with an IDENTICAL node set inside each view FIRST. A view can split one
    # logical stage across several passes -- MAIN draws the GBuffer in 5 passes where VRCAM uses
    # 2, with byte-identical node sets -- and then no similarity measure can tell those passes
    # apart, so a 1:1 pairing assigns them arbitrarily and invents per-pass ratios (that is how
    # "pass 39 costs VRCAM 2.9x" appeared: it was MAIN's split, not a VRCAM regression).
    # Comparing the GROUPS is the only meaningful comparison.
    def group(sig, cost, frames):
        g = {}
        for rt, nodes in sig.items():
            e = g.setdefault(frozenset(nodes), {"rts": [], "ms": 0.0})
            e["rts"].append(rt)
            e["ms"] += cost.get(rt, 0.0) / frames
        return g

    gm, gv = group(sig_m, cost_m, fm), group(sig_v, cost_v, fv)
    matched, used = [], set()
    for kv in sorted(gv, key=lambda k: -gv[k]["ms"]):
        best, best_j = None, 0.0
        for km in gm:
            if km in used:
                continue
            j = jaccard(set(kv), set(km))
            if j > best_j:
                best, best_j = km, j
        if best is not None and best_j >= 0.5:
            used.add(best)
            matched.append({
                "rt_v": gv[kv]["rts"], "rt_m": gm[best]["rts"], "sim": best_j,
                "ms_v": gv[kv]["ms"], "ms_m": gm[best]["ms"],
                "nodes": sorted(set(kv) & set(best)),
                "only_v": sorted(set(kv) - set(best)),
                "only_m": sorted(set(best) - set(kv)),
            })
        else:
            matched.append({
                "rt_v": gv[kv]["rts"], "rt_m": None, "sim": best_j,
                "ms_v": gv[kv]["ms"], "ms_m": 0.0,
                "nodes": sorted(kv), "only_v": sorted(kv), "only_m": [],
            })
    unmatched_m = [{"rt_m": gm[k]["rts"], "ms_m": gm[k]["ms"], "nodes": sorted(k)}
                   for k in sorted(gm, key=lambda k: -gm[k]["ms"]) if k not in used]
    return matched, unmatched_m


def parse_dump(lines):
    cfg, audit, nodes, passes, pairs = {}, {}, [], [], []
    for ln in lines:
        if "[prof] CFG|" in ln:
            cfg = parse_kv(ln, "CFG")
        elif "[prof] AUDIT|" in ln:
            audit = parse_kv(ln, "AUDIT")
        elif "[prof] N|" in ln:
            f = ln.split("[prof] N|", 1)[1].rstrip("\r\n").split("|")
            if len(f) < 10:
                continue
            nodes.append({
                "ord_main": int(f[0]), "ord_vrcam": int(f[1]),
                "rva": f[2], "name": f[3],
                "calls_main": int(f[4]), "calls_vrcam": int(f[5]),
                "self_main": float(f[6]), "self_vrcam": float(f[7]),
                "incl_main": float(f[8]), "incl_vrcam": float(f[9]),
            })
        elif "[prof] P|" in ln:
            f = ln.split("[prof] P|", 1)[1].rstrip("\r\n").split("|")
            if len(f) < 5:
                continue
            passes.append({
                "rtid": int(f[0]),
                "calls_main": int(f[1]), "calls_vrcam": int(f[2]),
                "incl_main": float(f[3]), "incl_vrcam": float(f[4]),
            })
        elif "[prof] PAIRWARN|" in ln:
            audit["pair_overflow"] = parse_kv(ln, "PAIRWARN").get("overflow", "?")
        elif "[prof] R|" in ln:
            f = ln.split("[prof] R|", 1)[1].rstrip("\r\n").split("|")
            if len(f) < 11:
                continue
            pairs.append({
                "rtid": int(f[0]), "rva": f[1], "name": f[2],
                "calls_main": int(f[3]), "calls_vrcam": int(f[4]),
                "nested_main": int(f[5]), "nested_vrcam": int(f[6]),
                "owner_main": int(f[7]), "owner_vrcam": int(f[8]),
                "self_main": float(f[9]), "self_vrcam": float(f[10]),
            })
    return cfg, audit, nodes, passes, pairs


def infer_frames(nodes, key):
    """Frames rendered for one view, inferred from the node call counts.

    The DLL's view_frames_* counters turned out NOT to be per-frame: the engine's graph
    runner calls the node executor directly for most nodes, so nearly every node is a
    top-level dispatch and the counter really counts node-dispatches. The robust
    denominator is the MODAL call count instead: the large majority of nodes run exactly
    once per view-frame, so the most common call count IS the frame count (every other
    node's count comes out as a clean multiple of it).
    """
    counts = {}
    for r in nodes:
        c = r[key]
        if c > 0:
            counts[c] = counts.get(c, 0) + 1
    if not counts:
        return 0, 0
    frames, votes = max(counts.items(), key=lambda kv: (kv[1], -kv[0]))
    return frames, votes


def verdict(row):
    """Classify a node the way the old audit did, but from ms/view-frame."""
    cm, cv = row["calls_main_pf"], row["calls_vrcam_pf"]
    sm, sv = row["self_main_pf"], row["self_vrcam_pf"]
    if cv == 0:
        return "MAIN-only"
    if cm == 0:
        return "VRCAM-only"
    if sm <= 1e-6 and sv <= 1e-6:
        return "free (both ~0)"
    if sm <= 1e-6:
        return "VRCAM-only cost"
    ratio = sv / sm
    if ratio >= 1.5:
        return "VRCAM-heavy %.1fx" % ratio
    if ratio <= 1.0 / 1.5:
        return "MAIN-heavy %.1fx (vrcam cheap)" % (1.0 / ratio)
    return "symmetric"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--log", default=DEFAULT_LOG)
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--dump", type=int, default=0,
                    help="0 = last dump in the log, 1 = the one before it, ...")
    args = ap.parse_args()

    if not os.path.isfile(args.log):
        sys.exit("log not found: %s" % args.log)
    with open(args.log, encoding="utf-8", errors="replace") as fh:
        lines = [ln for ln in fh if "[prof] " in ln]
    dumps = split_dumps(lines)
    if not dumps:
        sys.exit("no [prof] CFG| dump found in %s -- enable the profiler and press "
                 "'Dump node audit' in the overlay" % args.log)
    idx = len(dumps) - 1 - args.dump
    if idx < 0:
        sys.exit("only %d dump(s) in the log" % len(dumps))
    cfg, audit, nodes, passes, pairs = parse_dump(dumps[idx])
    if not nodes:
        sys.exit("dump has no node rows (profiler was off during the window?)")

    fm, votes_m = infer_frames(nodes, "calls_main")
    fv, votes_v = infer_frames(nodes, "calls_vrcam")
    if fm <= 0 or fv <= 0:
        sys.exit("could not infer a frame count (main=%d vrcam=%d)" % (fm, fv))
    vfm, vfv = float(fm), float(fv)
    win_ms = float(audit.get("window_ms", 0) or 0)
    fps_m = fm / (win_ms / 1000.0) if win_ms else 0.0
    fps_v = fv / (win_ms / 1000.0) if win_ms else 0.0
    # older dumps called these view_frames_*; they were never frame counts
    dispatches_m = float(audit.get("node_dispatches_main",
                                   audit.get("view_frames_main", 0)) or 0)
    dispatches_v = float(audit.get("node_dispatches_vrcam",
                                   audit.get("view_frames_vrcam", 0)) or 0)

    for r in nodes:
        r["calls_main_pf"] = r["calls_main"] / vfm
        r["calls_vrcam_pf"] = r["calls_vrcam"] / vfv
        r["self_main_pf"] = r["self_main"] / vfm
        r["self_vrcam_pf"] = r["self_vrcam"] / vfv
        r["incl_main_pf"] = r["incl_main"] / vfm
        r["incl_vrcam_pf"] = r["incl_vrcam"] / vfv
        r["verdict"] = verdict(r)
    nodes.sort(key=lambda r: -(r["self_vrcam_pf"] + r["self_main_pf"]))

    tot_sm = sum(r["self_main_pf"] for r in nodes)
    tot_sv = sum(r["self_vrcam_pf"] for r in nodes)
    frame_ms = (win_ms / fm) if fm else 0.0

    o = []
    o.append("# VRCAM vs MAIN per-node CPU audit (v2, named)\n")
    o.append("Source: `CyberpunkVR_ProfDumpNodes()` -> testbed.log, parsed by "
             "`tools/node_audit_md.py`.\n")
    o.append("## How to read this\n")
    o.append("- **self** = node's own CPU time, children excluded. This is the ONLY column "
             "you may rank or sum.")
    o.append("- **incl** = self + the nodes this node dispatches. `SceneDrv_ALL_SCENE_PASSES` "
             "contains every scene pass, so inclusive columns overlap and must never be summed.")
    o.append("- Per-frame = per rendered frame of that view. Denominators are INFERRED from the "
             "data as the modal call count: main **%d** frames (%d nodes sit at exactly that "
             "count), vrcam **%d** (%d nodes). Every other node's count comes out a clean "
             "multiple of it." % (fm, votes_m, fv, votes_v))
    o.append("- Not used as divisors: presents=%s (frame generation inflates it) and the DLL's "
             "own view_frames counters (%d / %d) -- those count node *dispatches*, because the "
             "engine's graph runner calls the executor directly for most nodes, so almost every "
             "node is top-level." % (audit.get("presents", "?"), int(dispatches_m),
                                     int(dispatches_v)))
    o.append("- `ord` = first-seen dispatch order for that view (0 = the view never ran it).")
    o.append("- Window: %.1f s wall -> %.1f fps main / %.1f fps vrcam, %.2f ms per frame, "
             "%d nodes.\n" % (win_ms / 1000.0, fps_m, fps_v, frame_ms, len(nodes)))
    o.append("## Bottom line\n")
    o.append("Self time summed over all nodes, per frame:\n")
    o.append("| view | node CPU (self) ms/frame | vs the %.2f ms frame |" % frame_ms)
    o.append("|---|---|---|")
    o.append("| MAIN | %.3f | %.0f%% |" % (tot_sm, 100.0 * tot_sm / frame_ms if frame_ms else 0))
    o.append("| VRCAM | %.3f | %.0f%% |" % (tot_sv, 100.0 * tot_sv / frame_ms if frame_ms else 0))
    o.append("| both | %.3f | %.0f%% |" % (tot_sm + tot_sv,
                                           100.0 * (tot_sm + tot_sv) / frame_ms if frame_ms else 0))
    o.append("")
    o.append("VRCAM/MAIN = **%.2fx**. Summing self is legitimate -- no double counting. But the "
             "last column is CPU time against WALL time, not a share of the frame: node work is "
             "dispatched onto worker threads as well as the render thread, so the total can "
             "exceed 100%% without anything being wrong, and a saving of 1 ms of node CPU does "
             "NOT mean 1 ms off the frame unless that node sits on the critical path.\n"
             % ((tot_sv / tot_sm) if tot_sm else 0.0))

    o.append("## Capture configuration\n")
    o.append("An experiment armed during a capture silently invalidates the table, so the "
             "toggles are recorded with it:\n")
    o.append("| flag | value |")
    o.append("|---|---|")
    for k in sorted(cfg):
        o.append("| `%s` | %s |" % (k, cfg[k]))
    o.append("")

    o.append("## Nodes, ranked by self time\n")
    o.append("| ord m/v | node | rva | calls/frame m/v | self ms/frame m | self ms/frame v | "
             "incl ms/frame v | verdict |")
    o.append("|---|---|---|---|---|---|---|---|")
    for r in nodes:
        o.append("| %d/%d | %s | %s | %.2f / %.2f | %.4f | %.4f | %.4f | %s |" % (
            r["ord_main"], r["ord_vrcam"], r["name"], r["rva"],
            r["calls_main_pf"], r["calls_vrcam_pf"],
            r["self_main_pf"], r["self_vrcam_pf"], r["incl_vrcam_pf"], r["verdict"]))
    o.append("")

    heavy = [r for r in nodes if r["verdict"].startswith("VRCAM-heavy")]
    if heavy:
        o.append("## VRCAM-heavy (>=1.5x MAIN)\n")
        o.append("A big ratio is NOT proof of duplication: for a shared incremental resource "
                 "the first view in the frame (VRCAM) becomes the OWNER and pays once for "
                 "everybody. Check the owner bit `[a2+0x30]&2` before skipping anything.\n")
        o.append("| node | rva | self ms/frame v | ratio |")
        o.append("|---|---|---|---|")
        for r in sorted(heavy, key=lambda x: -x["self_vrcam_pf"]):
            ratio = r["self_vrcam_pf"] / r["self_main_pf"] if r["self_main_pf"] else 0.0
            o.append("| %s | %s | %.4f | %.1fx |" % (r["name"], r["rva"],
                                                     r["self_vrcam_pf"], ratio))
        o.append("")

    overflow = audit.get("pair_overflow")
    if pairs and overflow:
        # The pair table dropped dispatches, so a pass's node set is incomplete and matching
        # it by content would silently compare truncated sets. Refuse rather than mislead.
        o.append("## Passes matched by content -- NOT AVAILABLE\n")
        o.append("The (rtId, node) table overflowed during this capture (**%s dispatches "
                 "dropped**), so each pass's node set is truncated and content matching would "
                 "compare incomplete sets. Re-capture with a build whose pair table is large "
                 "enough; the node and pass tables below are unaffected.\n" % overflow)
        pairs = []
    if pairs:
        matched, unmatched_m = match_passes(pairs, vfm, vfv)
        o.append("## Passes matched by CONTENT (not by rtId number)\n")
        o.append("`rtId` is the build-order index of a top-level pass scope, so a pass added or "
                 "skipped earlier renumbers the whole tail and the two views do NOT share a "
                 "numbering. Comparing rtId numbers between views invents 'view-only passes'. "
                 "Passes below are paired by the set of node names inside them (Jaccard >= 0.5); "
                 "`self ms/frame` sums the nodes' own time under that pass.\n")
        o.append("Passes with an identical node set are collapsed per view before matching, so a "
                 "row can cover several rtIds -- a view may split one stage across more passes "
                 "than the other, and only the group totals are comparable.\n")
        o.append("| vrcam rtIds | main rtIds | sim | self ms/f m | self ms/f v | v/m | nodes in pass |")
        o.append("|---|---|---|---|---|---|---|")
        for m in matched:
            ratio = (m["ms_v"] / m["ms_m"]) if m["ms_m"] > 1e-9 else 0.0
            names = ", ".join(m["nodes"][:4]) + (" +%d" % (len(m["nodes"]) - 4)
                                                if len(m["nodes"]) > 4 else "")
            o.append("| %s | %s | %.2f | %.4f | %.4f | %s | %s |" % (
                ",".join(str(r) for r in sorted(m["rt_v"])),
                ",".join(str(r) for r in sorted(m["rt_m"])) if m["rt_m"] else "**none**",
                m["sim"], m["ms_m"], m["ms_v"],
                ("%.2fx" % ratio) if ratio else "-", names))
        o.append("")
        pv = sum(m["ms_v"] for m in matched)
        pm = sum(m["ms_m"] for m in matched)
        o.append("Paired totals: MAIN %.3f, VRCAM %.3f ms/frame (VRCAM/MAIN %.2fx).\n"
                 % (pm, pv, (pv / pm) if pm else 0.0))
        orphan_v = [m for m in matched if m["rt_m"] is None]
        if orphan_v:
            o.append("VRCAM pass groups with NO main counterpart (%d, %.3f ms/frame): %s. Treat "
                     "with suspicion -- so far every such candidate turned out to be a numbering "
                     "or splitting artefact rather than exclusive work.\n"
                     % (len(orphan_v), sum(m["ms_v"] for m in orphan_v),
                        "; ".join(",".join(str(r) for r in sorted(m["rt_v"])) for m in orphan_v)))
        else:
            o.append("**Every VRCAM pass group has a MAIN counterpart** -- there is no "
                     "VRCAM-exclusive pass work.\n")
        if unmatched_m:
            o.append("MAIN pass groups with no vrcam counterpart (%d, %.3f ms/frame): %s\n"
                     % (len(unmatched_m), sum(u["ms_m"] for u in unmatched_m),
                        "; ".join("rt%s {%s}" % (",".join(str(r) for r in sorted(u["rt_m"])),
                                                 ", ".join(u["nodes"][:3]))
                                  for u in unmatched_m)))

        # who owns the shared per-frame updates
        own = {}
        for p in pairs:
            a = own.setdefault(p["name"], {"om": 0, "ov": 0, "cm": 0, "cv": 0,
                                           "sm": 0.0, "sv": 0.0})
            a["om"] += p["owner_main"]; a["ov"] += p["owner_vrcam"]
            a["cm"] += p["calls_main"]; a["cv"] += p["calls_vrcam"]
            a["sm"] += p["self_main"]; a["sv"] += p["self_vrcam"]
        rows = []
        for nm, a in own.items():
            if a["cm"] == 0 or a["cv"] == 0:
                continue
            fr_m = a["om"] / a["cm"]
            fr_v = a["ov"] / a["cv"]
            if abs(fr_m - fr_v) >= 0.10:
                rows.append((nm, fr_m, fr_v, a["sm"] / vfm, a["sv"] / vfv))
        tot_om = sum(a["om"] for a in own.values())
        tot_ov = sum(a["ov"] for a in own.values())
        tot_cm = sum(a["cm"] for a in own.values())
        tot_cv = sum(a["cv"] for a in own.values())
        if not rows:
            o.append("## Owner bit `[node_arg+0x30] & 2` is FRAME PARITY, not view ownership\n")
            o.append("Measured over this whole capture: the bit is set in **%.1f%%** of MAIN "
                     "dispatches (%d/%d) and **%.1f%%** of VRCAM dispatches (%d/%d), and no node "
                     "shows a per-view difference above 10%%. It is set on alternate frames, "
                     "identically for both views, for every node -- so it does NOT select which "
                     "view drives a shared update. Any reasoning of the form \"VRCAM is the owner "
                     "here, so skipping it starves the resource\" cannot be based on this bit.\n"
                     % (100.0 * tot_om / max(tot_cm, 1), tot_om, tot_cm,
                        100.0 * tot_ov / max(tot_cv, 1), tot_ov, tot_cv))
        if rows:
            o.append("## Owner bit `[node_arg+0x30] & 2` -- who drives the shared update\n")
            o.append("For a shared, incrementally-updated resource the engine gates the update on "
                     "this bit, so exactly one view pays and the other rides along. A node where "
                     "the two views disagree is NOT duplicated work: skipping the OWNER starves "
                     "the resource for both views. Listed when the owner share differs by >=10%%.\n")
            o.append("| node | owner share main | owner share vrcam | self ms/f m | self ms/f v |")
            o.append("|---|---|---|---|---|")
            for nm, fm2, fv2, sm2, sv2 in sorted(rows, key=lambda r: -max(r[3], r[4])):
                o.append("| %s | %.0f%% | %.0f%% | %.4f | %.4f |"
                         % (nm, 100 * fm2, 100 * fv2, sm2, sv2))
            o.append("")

    if passes:
        passes.sort(key=lambda p: -(p["incl_vrcam"] + p["incl_main"]))
        o.append("## SceneDrv scene passes by rtId (inclusive -- overlaps the node rows)\n")
        o.append("Kept only as raw data. Do NOT compare the two columns of one row across views: "
                 "the numbering differs. Use the content-matched table above.\n")
        o.append("| rtId | calls/frame m/v | incl ms/frame m | incl ms/frame v |")
        o.append("|---|---|---|---|")
        for p in passes:
            o.append("| %d | %.2f / %.2f | %.4f | %.4f |" % (
                p["rtid"], p["calls_main"] / vfm, p["calls_vrcam"] / vfv,
                p["incl_main"] / vfm, p["incl_vrcam"] / vfv))
        o.append("")

    out = args.out
    d = os.path.dirname(out)
    if d and not os.path.isdir(d):
        os.makedirs(d)
    with open(out, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("\n".join(o))
    unnamed = [r for r in nodes if r["name"] in ("?", "") or r["name"].startswith("unknown_")]
    print("wrote %s: %d nodes, %d scene passes, window %.1fs" %
          (out, len(nodes), len(passes), win_ms / 1000.0))
    print("view-frames: main=%d vrcam=%d   self totals: main=%.3f ms vrcam=%.3f ms" %
          (int(vfm), int(vfv), tot_sm, tot_sv))
    if unnamed:
        print("unnamed work fns (%d): %s" %
              (len(unnamed), ", ".join(r["rva"] for r in unnamed[:12])))


if __name__ == "__main__":
    main()
