# -*- coding: utf-8 -*-
"""Build one belt-ready magazine per weapon in the game, plus the shotgun shell row.

WHY ONE PER WEAPON. A single pouch on the belt is a lie the moment the player carries something else --
"чтобы для каждого оружия он был свой - иначе херня". Every firearm in this game ships its own
`mag_std_0N` mesh, rigid and weighted to a single bone, so each one can ride the hips exactly like the
grenades already do: rename the bone to `Hips`, give it the inverse bind matrix of a garment known to
work on this rig, and nothing about the geometry is touched.

THE LIST IS NOT WRITTEN DOWN, it is read off the extracted tree -- every
`firearms/<class>/<make>_<model>/entities/meshes/w_*__mag_std_0N.mesh` in the base game and in EP1.
A hand-written list of fifty weapons is a list that is wrong the first time a weapon is missed, and
one was: this started at twenty-four before the count was checked against the archives.

WHAT EACH MESH GETS

  * the re-bind to Hips, with a placement pass;
  * the eight `lit` appearances, so the hand's mark works on it without a separate twin component --
    the module falls back to swapping the appearance when a host has no twin, which halves the number
    of components this would otherwise cost;
  * `lodLevelInfo` pushed out of reach. Not cosmetic: a magazine drops to LOD1 somewhere between forty
    centimetres and three metres, and a belt sits about half a metre from the eye in VR, so the low LOD
    is what the player actually sees -- 153 vertices instead of 566 on the shotgun shell, which reads
    as a hexagon rather than a cylinder.

THE SHELL ROW is three separate meshes rather than one, because the spacing between them has to be
baked: a component transform on a skinned part does not survive the camera. Carnage's shell is gold;
Tactician's is an ordinary red one, which is what a belt should carry.

TUNING lives in notes/belt_mag_chains.json -- every pass ever applied, per element, in order. The mesh
is always rebuilt from the pristine game asset and replayed through the whole chain, because rotations
and translations do not commute: stacking a new pass onto an already-baked result gives a different
order and a different place.

Appending is also the only correct way to apply a pass: a rotation written at runtime turns about the
ENTITY origin, and composing it after the placement reproduces that exactly, while baking it into the
mesh's own centre would not.

`--offsets <offsets.json>` APPENDS the module's current tuning as one more pass and writes it back into
the chain file, so the numbers are never left living only in a session's memory -- which is what nearly
happened to the first round of them.

Usage:  python tools/build_belt_mags.py <src json dir> <out json dir> [--shells-only]
                                        [--offsets <offsets.json>] [--only <key | shell>]
"""
import io
import json
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HOLDER = r"C:\Users\dariulone\.claude\jobs\6adab39b\tmp\beltcopyjson\vrp_belt_gren_front.mesh.json"

# Where a magazine starts out: the belt's own mag pouch. ONE anchor for all of them, because only one
# is ever drawn at a time and the re-bind centres every mesh on it -- so one set of sliders tunes the
# lot instead of fifty.
MAG_PLACE = ["-0.1497", "0.0949", "1.0756", "0", "0", "0"]

# Weapons whose belt magazine is not simply their own `mag_std_01`.
#   chao      -- the port's own vrp_mag_chao.ent uses _02, and that is the one the reload hands over
#   unity     -- shares the Liberty's magazine; the game ships no `constitutional_unity` mag mesh
#   overture  -- a revolver: what the hand brings is its speedloader (`mag_stdr`, what the reload carries as
#                Items.VRPortLoadOverture), and the speedloader is SEVEN meshes -- six rounds and the handle.
#                A tuple is such an object: one belt mesh per part, all placed by the centre of the whole
#                (rebind --centre), the first the element and the rest its row (ucbeltmag_overture_p2..).
#                It used to be one cartridge off the speedloader, which on the belt read as a single round.
SPECIAL = {
    "chao": "w_handgun__kangtao_chao__mag_std_02",
    "unity": "w_handgun__constitutional_liberty__mag_std_01",
    "overture": tuple("w_revolver__malorian_overture__mag_stdr_01_" + p
                      for p in ("handle", "01", "02", "03", "04", "05", "06")),
}

# A model name too short to match a record safely falls back to the manufacturer.
SHORT = {"lx": "senkoh"}

# CARRIED ON THE BACK, so no belt magazine at all -- the user's own list. A rifle slung over the
# shoulder is reached for over the shoulder, and a magazine for it sitting on the hip is a second
# place the hand would have to know about. Everything here is dropped: no mesh, no component, no rule.
BACK_CARRIED = {
    "borg", "authority", "ajax", "achilles", "grad", "hercules", "hmg", "kolac",
    "nekomata", "osprey", "rasetsu", "shingen", "sor22", "uragan", "warden", "zhuo",
}

# ONLY THE PUMP GUNS ARE SERVED BY THE SHELL ROW. Their `mag_std_01` is a single loose shell --
# measured: Carnage 2.4 x 7.2 cm on one bone called `mag_std`, Tactician 2.1 x 5.1 -- so a "magazine"
# for them would be the same object twice, a few centimetres from the row that already means it.
#
# The break-action pair is NOT the same thing and stays: what goes into a Satara or a Palica is TWO
# rounds held together and inserted as one, which is a loading clip and reads as its own object on the
# belt. Both were briefly swept into the row on my inference and the user pulled them back out.
SHOTGUN_ROW = {"carnage", "tactician"}

SHELL_SRC = "w_shotgun__constitutional_tactician__mag_std_01"
SHELL_X = ("-0.1270", "-0.1000", "-0.0730")

CHAINS = os.path.join(os.path.dirname(HERE), "notes", "belt_mag_chains.json")

LOD_FAR = 2000.0
NAME = re.compile(r"^w_[a-z_]+__([a-z0-9]+)_([a-z0-9]+)__mag_std_(\d+)$")


def discover(src):
    """key -> mesh stem, straight off the extracted filenames."""
    found = {}
    for f in sorted(os.listdir(src)):
        if not f.endswith(".mesh.json"):
            continue
        stem = f[:-len(".mesh.json")]
        m = NAME.match(stem)
        if m is None:
            continue
        make, model, num = m.group(1), m.group(2), m.group(3)
        key = SHORT.get(model, model)
        if key in SPECIAL or key in BACK_CARRIED or key in SHOTGUN_ROW:
            continue
        # The first variant wins; _02 is a different magazine for the same gun, not a second gun.
        if key not in found or num < found[key][1]:
            found[key] = (stem, num)
    out = {k: v[0] for k, v in found.items()}
    out.update(SPECIAL)
    return {k: v for k, v in out.items()
            if k not in BACK_CARRIED and k not in SHOTGUN_ROW}


def lod_out(path):
    d = json.load(io.open(path, encoding="utf-8"))
    r = d["Data"]["RootChunk"]
    old = list(r.get("lodLevelInfo") or [])
    if len(old) > 1:
        r["lodLevelInfo"] = [old[0]] + [LOD_FAR] * (len(old) - 1)
        io.open(path, "w", encoding="utf-8", newline="").write(json.dumps(d, indent=2))
    return old


def rebind(dst, passes):
    """Re-bind, falling back to --all-to-hips for a magazine built from several rigid parts.

    A magazine is not an articulated thing. The extra bones here are the follower, the clasp, or the
    two cartridges of a break-action shotgun -- parts that move inside the WEAPON and do not move at
    all while the magazine hangs on a belt. Pointing them all at the hips is therefore right, and it
    is a deliberate second attempt rather than the default, because on something that really does
    articulate it would silently misplace whatever hangs off the extra bone.
    """
    base = [sys.executable, os.path.join(HERE, "rebind_to_belt.py"), dst, HOLDER] + passes
    for cmd in (base, base + ["--all-to-hips"]):
        out = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
        if out.returncode == 0:
            lines = [l for l in out.stdout.strip().splitlines() if l]
            return (lines[-1] if lines else "") + ("" if cmd is base else "   [все кости -> Hips]")
    return None


def union_centre(paths):
    """The centre of several meshes' bounding boxes together: where a many-part object is placed from."""
    lo, hi = [1e30] * 3, [-1e30] * 3
    for p in paths:
        bb = json.load(io.open(p, encoding="utf-8"))["Data"]["RootChunk"]["boundingBox"]
        for j, k in enumerate(("X", "Y", "Z")):
            lo[j], hi[j] = min(lo[j], bb["Min"][k]), max(hi[j], bb["Max"][k])
    return ["%.6f" % ((lo[j] + hi[j]) / 2.0) for j in range(3)]


def part_name(key, i):
    """The belt mesh of a part: the first is the element's own, the rest its row."""
    return "vrp_belt_mag_%s.mesh.json" % key if i == 0 else "vrp_belt_mag_%s_p%d.mesh.json" % (key, i + 1)


def tuning(o):
    """The six numbers this element was dragged to, or nothing if it was never touched."""
    if not o:
        return []
    vals = [float(o.get(k, 0) or 0) for k in ("x", "y", "z", "rx", "ry", "rz")]
    if all(abs(v) < 1e-6 for v in vals):
        return []
    return ["%.5f" % v for v in vals]


def passes(chains, key):
    """Every recorded pass for one element, flattened for the re-bind's argument list."""
    out = []
    for p in (chains.get(key) or []):
        out.extend("%.5f" % float(v) for v in p)
    return out


def main():
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    src, out = sys.argv[1], sys.argv[2]
    shells_only = "--shells-only" in sys.argv
    chains = {}
    if os.path.exists(CHAINS):
        chains = {k: v for k, v in json.load(io.open(CHAINS, encoding="utf-8")).items()
                  if not k.startswith("_")}
    if "--offsets" in sys.argv:
        p = sys.argv[sys.argv.index("--offsets") + 1]
        if os.path.exists(p):
            offsets = json.load(io.open(p, encoding="utf-8"))
            added = 0
            for host, o in offsets.items():
                vals = tuning(o)
                if not vals:
                    continue
                key = None
                if host.startswith("ucbeltmag_"):
                    key = host[len("ucbeltmag_"):]
                elif host == "ucbeltammo_shell1":
                    key = "shell"
                if key is None:
                    continue
                chains.setdefault(key, []).append([float(v) for v in vals])
                added += 1
            if added:
                doc = json.load(io.open(CHAINS, encoding="utf-8")) if os.path.exists(CHAINS) else {}
                doc.update(chains)
                io.open(CHAINS, "w", encoding="utf-8", newline="").write(json.dumps(doc, indent=2))
                print("новых проходов записано в цепочки: %d" % added)
    os.makedirs(out, exist_ok=True)

    sys.path.insert(0, HERE)
    from make_glow_meshes import add_lit_appearance

    only = sys.argv[sys.argv.index("--only") + 1] if "--only" in sys.argv else None
    keys = []
    if not shells_only:
        mags = discover(src)
        for key in sorted(mags):
            if only is not None and key != only:
                continue
            stems = mags[key] if isinstance(mags[key], tuple) else (mags[key],)
            srcs = [os.path.join(src, s + ".mesh.json") for s in stems]
            missing = [s for s in srcs if not os.path.exists(s)]
            if missing:
                print("!! нет исходника для %-12s %s" % (key, ", ".join(os.path.basename(m) for m in missing)))
                continue
            extra = passes(chains, key)
            centre = ["--centre"] + union_centre(srcs) if len(srcs) > 1 else []
            built = []
            for i, s in enumerate(srcs):
                dst = os.path.join(out, part_name(key, i))
                io.open(dst, "w", encoding="utf-8", newline="").write(io.open(s, encoding="utf-8").read())
                if rebind(dst, list(MAG_PLACE) + extra + centre) is None:
                    os.remove(dst)
                    break
                lod_out(dst)
                add_lit_appearance(dst)
                built.append(dst)
            if len(built) != len(srcs):
                print("!! многокостный, пропущен: %s" % key)
                for dst in built:
                    os.remove(dst)
                continue
            keys.append(key)
            print("%-12s %-52s проходов %d%s" % (key, stems[0], 1 + len(extra) // 6,
                                                 "" if len(stems) == 1 else ", частей %d" % len(stems)))

    for i, x in enumerate(SHELL_X if only in (None, "shell") else (), start=1):
        s = os.path.join(src, SHELL_SRC + ".mesh.json")
        dst = os.path.join(out, "vrp_belt_ammo_shell%d.mesh.json" % i)
        io.open(dst, "w", encoding="utf-8", newline="").write(io.open(s, encoding="utf-8").read())
        rebind(dst, [x, "0.1000", "1.0400", "-90", "0", "0"] + passes(chains, "shell"))
        lod_out(dst)
        add_lit_appearance(dst)
        print("гильза %d" % i)

    if keys:
        io.open(os.path.join(out, "_keys.json"), "w", encoding="utf-8").write(json.dumps(keys, indent=2))
        print("")
        print("магазинов собрано: %d" % len(keys))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
