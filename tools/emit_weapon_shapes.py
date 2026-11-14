"""Writes each weapon's collision shape for CyberpunkVRPort_HandCollision: a few boxes fitted to the game's own vertices.

Usage: python tools/emit_weapon_shapes.py <dir of serialized .mesh.json> <out .lua> [most boxes] [least gain]

A held weapon has no collider in the game: of 943 firearm meshes one has a physics parameter, a magazine's box, and no firearm entity
or appearance has a collider component. The boxes are fitted by tools/weapon_boxes/fit_boxes (C++, built by its build.bat): the box
with the most to gain cut in two across any axis, again and again, the cuts' faces kept where they meet. This script only reads the
meshes and writes the files: each family's vertex streams go to the fitter as they are in the meshes.

The serialized mesh carries its vertex buffer: renderResourceBlob.renderBuffer.Bytes, base64. Stream 0 starts each vertex with the
position as four signed 16-bit normalised numbers, quantised by the header's quantizationScale and Offset. Skinned meshes are in bind
pose in the weapon's frame (origin at the grip, +Y along the barrel, +Z up), so the parts of one weapon simply add up.

Twelve boxes at most a gun by default, each cut gaining at least 0.6 % of the outline: the script adds a mounted muzzle and a scope
as two more, and the physics hands take fourteen.
"""
import base64
import glob
import json
import os
import re
import struct
import subprocess
import sys
import tempfile

SKIP = re.compile(r"decal|shadow|mag_stdr|mag_ext|mag_drum|silencer|muzzle|scope|sight_|attachment|_fx|glass|lod\d|dying_night|neon|_d\d\d_|wilson|krausser|rook", re.I)
NAME = re.compile(r"^(w_[a-z0-9_]+?__[a-z0-9_]+?)__", re.I)
HERE = os.path.dirname(os.path.abspath(__file__))
FITTER = os.path.join(HERE, "weapon_boxes", "fit_boxes.exe")

src, out = sys.argv[1], sys.argv[2]
most = int(sys.argv[3]) if len(sys.argv) > 3 else 12
# the least share of the outline a cut must gain (fit_boxes' own default is 0.015)
least = sys.argv[4] if len(sys.argv) > 4 else '0.006'
if not os.path.exists(FITTER):
    sys.exit("build tools/weapon_boxes first (build.bat): " + FITTER)


def streams_of(path):
    """The mesh's LOD-0 position streams: (scale, offset, count, stride, raw bytes) each."""
    with open(path, encoding="utf-8") as f:
        root = json.load(f)["Data"]["RootChunk"]
    blob = root["renderResourceBlob"]["Data"]
    head = blob["header"]
    raw = base64.b64decode(blob["renderBuffer"]["Bytes"])
    scale, offset = head["quantizationScale"], head["quantizationOffset"]
    s = (scale["X"], scale["Y"], scale["Z"])
    o = (offset["X"], offset["Y"], offset["Z"])
    out = []
    for c in head["renderChunkInfos"]:
        if not c.get("lodMask", 1) & 1:
            continue
        n = c["numVertices"]
        if n == 0:
            continue
        offs = c["chunkVertices"]["byteOffsets"]["Elements"]
        layout = c["chunkVertices"]["vertexLayout"]["elements"]["Elements"]
        first = layout[0]
        if first.get("usage") != "PS_Position" or first.get("type") != "PT_Short4N":
            raise ValueError("layout %s %s" % (first.get("usage"), first.get("type")))
        stride = (offs[1] - offs[0]) // n
        if stride < 8:
            raise ValueError("stride %d" % stride)
        out.append((s, o, n, stride, raw[offs[0]: offs[0] + n * stride]))
    return out


families = {}
counts = {}
bad = 0
for path in glob.glob(os.path.join(src, "**", "*.json"), recursive=True):
    name = os.path.basename(path).split(".")[0]
    m = NAME.match(name)
    if not m or SKIP.search(name):
        continue
    try:
        streams = streams_of(path)
    except (KeyError, ValueError, TypeError, struct.error) as e:
        bad += 1
        print("skipped", name, e)
        continue
    if not streams:
        continue
    # a mesh placed far from the grip (a world prop under a weapon's name): its quantisation's reach says so
    if any(abs(o[k]) + abs(s[k]) > 3.0 for s, o, _, _, _ in streams for k in range(3)):
        continue
    key = m.group(1).lower()
    families.setdefault(key, []).extend(streams)
    counts[key] = counts.get(key, 0) + 1

tmp = tempfile.mkdtemp(prefix="weapon_boxes_")
points, boxes_txt = os.path.join(tmp, "points.bin"), os.path.join(tmp, "boxes.txt")
with open(points, "wb") as f:
    f.write(b"WPTS" + struct.pack("<I", len(families)))
    for key in sorted(families):
        b = key.encode()
        f.write(struct.pack("<H", len(b)) + b + struct.pack("<I", len(families[key])))
        for s, o, n, stride, raw in families[key]:
            f.write(struct.pack("<3f3fII", *s, *o, n, stride))
            f.write(raw)
r = subprocess.run([FITTER, points, boxes_txt, str(most)] + ([least] if least else []), capture_output=True, text=True)
print(r.stdout.strip())
if r.returncode:
    sys.exit("fit_boxes failed: " + r.stderr)

fitted = []
with open(boxes_txt, encoding="utf-8") as f:
    lines = f.read().split("\n")
i = 0
while i < len(lines) and lines[i].strip():
    key, n, fill = lines[i].split()
    n = int(n)
    fitted.append((key, float(fill), [lines[i + 1 + j].split() for j in range(n)]))
    i += 1 + n

lua = [
    "-- GENERATED by tools/emit_weapon_shapes.py (boxes fitted by tools/weapon_boxes/fit_boxes) from the vertices of the game's",
    "-- meshes -- do not edit.",
    "--",
    "-- Each weapon as a few boxes in its own frame: origin at the grip, +Y along the barrel, +Z up, metres.",
    "-- { minX, maxX, minY, maxY, minZ, maxZ }, by the name its mesh components start with.",
    "return {",
]
for key, fill, bx in fitted:
    lua.append("    %s = {" % key)
    for b in bx:
        lua.append("        { %s }," % ", ".join(b))
    lua.append("    },")
lua.append("}")
with open(out, "w", newline="\r\n") as f:
    f.write("\n".join(lua) + "\n")
print("weapons", len(fitted), "skipped meshes", bad)
for key, fill, bx in fitted[:3] + fitted[-3:]:
    print(key, "parts", counts.get(key), "boxes", len(bx), "fills %.2f" % fill)
