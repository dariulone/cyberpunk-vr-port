# -*- coding: utf-8 -*-
"""Give the belt props extra appearances that EMIT, and point them at our own mesh copies.

WHY, in the user's words: "это свет от неё, а не свет на ней". A light component -- which is what the
first attempt added -- is a light SOURCE: it lights the thigh, the clothes and the wall, while the prop
itself stays as dark as it was. Emission is the surface of the object glowing, and in this engine that
lives in the material.

AND WHY IT CANNOT BE THE OUTLINE INSTEAD, said three times before it landed: the engine's focus
outline -- and its fill, which is the same claim -- reach an object ONLY on ERenderingPlane.RPl_Scene.
A prop drawn in the first-person plane, which is where it has to be to sit correctly with the drawn
gun and the arms, cannot be marked by the vision system at all. Emission does not care about the plane.

## What this builds

    base/vrport/vrp_glow_white*.mi     our materials: opaque, white, three brightnesses
    base/vrport/meshes/<name>.mesh     a copy of each prop mesh with appearances lit / lit2 / lit3
    the prop .ent files                their plain mesh components repointed at our copies

## The base material, and the one that does not work

`metal_base.remt` is the opaque emissive base material -- it is what the game's own emissive building
windows use (_megabuilding_emissive_b.mi), and it takes EmissiveColor, EmissiveEV and an Emissive
texture, all of which our .mi sets to white placeholders so no textures ship with it.

The FIRST choice was `base/fx/shaders/emissive_basic_transparent.mt`, which is what CDPR puts on the
grenade's own little emissive overlay (grenade_emissive.mesh) -- and it is TRANSPARENT. A solid prop
wearing it in the first-person plane is drawn NOWHERE: measured live, the component reports
app=lit, enabled=true and a valid world matrix, and the prop simply vanishes. That is the tell for a
material that loaded but is not drawn in this pass, as opposed to an appearance that was not found.

## Three brightnesses, not one

EmissiveEV is baked into the material and a material cannot be tuned from a live session, so shipping
one number costs a rebuild, a repack and a relaunch for every guess at it. Three appearances cost
about three kilobytes and settle it by looking.

## Why the meshes have to be copied at all

A mesh's appearances and its materials live INSIDE the .mesh, and the prop meshes ship exactly one
appearance (`default`) with their material entries local, stored as raw blobs in `localMaterialBuffer`.
Nothing outside the file can add an appearance to it, and overriding the game's material would make
every grenade in the world glow, thrown ones and NPCs' included. So the copy is the whole cost -- about
200 KB per grenade type -- and the copies are ours to change.

The raw local materials are NOT touched: each lit appearance gets a new EXTERNAL material entry, so
`default` keeps rendering exactly as the game authored it.

Usage:  python tools/make_glow_meshes.py <prop .ent.json dir> <extracted mesh root> <out dir>
"""
import io
import json
import os
import shutil
import sys

V = "$value"
BS = chr(92)
OUR_DIR = "base" + BS + "vrport" + BS + "meshes"
MI_DIR = "base" + BS + "vrport" + BS

# (appearance name, material). Eight COLOURS, all on base/materials/loot_drop_highlight.mt -- the
# game's own "this can be picked up" highlight -- and all identical apart from HighlightColor.
#
# HighlightIntensity is FIXED at 0.16 in every one of them. It began at 0.08, chosen on the picture out
# of a 0.05..0.30 sweep, and was doubled once the mark had to read on a magazine as well as on a
# grenade -- a small dark object needs more than a big one. For scale: CDPR's own uses of this same
# material run 0.15 (dead bodies) to 1.00 (rare loot), so ours still sits at the bottom of that range.
#
# The pulsing is stopped by SolidBlendingDistanceStart = 0 with End = 0.01, a tiny NON-zero range.
# Measured in both directions. CDPR's own `_dimmed` files pin both to 0 and that DOES NOT work here --
# it puts the animation straight back -- so their value is not transferable and the measurement wins.
#
# AND THE RULE THIS MATERIAL DISPROVED: it has NO renderstage_gbuffer_regular pass, only
# renderstage_transparent, and it draws perfectly on the first-person plane. An earlier note here
# claimed the gbuffer pass was what decided that, filtered the game's 298 templates on it, and ruled
# this material out in writing. It was the best of everything tried. Do not filter candidates on an
# inferred rule again -- a .mi is 700 bytes, try it.
LEVELS = [("lit", MI_DIR + "vrp_mark_magenta.mi"),
          ("lit2", MI_DIR + "vrp_mark_white.mi"),
          ("lit3", MI_DIR + "vrp_mark_cyan.mi"),
          ("lit4", MI_DIR + "vrp_mark_green.mi"),
          ("lit5", MI_DIR + "vrp_mark_amber.mi"),
          ("lit6", MI_DIR + "vrp_mark_red.mi"),
          ("lit7", MI_DIR + "vrp_mark_blue.mi"),
          ("lit8", MI_DIR + "vrp_mark_violet.mi"),]


def cname(value):
    return {"$type": "CName", "$storage": "string", "$value": value}


def respath(value):
    return {"DepotPath": {"$type": "ResourcePath", "$storage": "string", "$value": value},
            "Flags": "Default"}


def val(x):
    return x.get(V, x) if isinstance(x, dict) else x


def unwrap(x):
    return x.get("Data", x) if isinstance(x, dict) and "Data" in x else x


def next_handle(doc):
    """The next free HandleId in the WHOLE file.

    Handle ids are file-global, not per-array, and getting that wrong is silent until the import: a
    mesh already spends HandleId 1 on its preloadLocalMaterialInstances, so an appearance numbered
    from the appearances array alone collides with it and the whole file is rejected with nothing
    more helpful than "The JSON value could not be converted ... LineNumber: 181".
    """
    top = [0]

    def walk(node):
        if isinstance(node, dict):
            if "HandleId" in node:
                try:
                    top[0] = max(top[0], int(node["HandleId"]))
                except (TypeError, ValueError):
                    pass
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(doc)
    return top[0] + 1


def add_lit_appearance(path):
    """Add one appearance per brightness, each with its own external emissive material."""
    doc = json.load(io.open(path, encoding="utf-8"))
    root = doc["Data"]["RootChunk"]
    apps = root.setdefault("appearances", [])
    if any(val(unwrap(a).get("name")) == LEVELS[0][0] for a in apps):
        return "already lit"

    base = None
    for a in apps:
        if val(unwrap(a).get("name")) == "default":
            base = unwrap(a)
            break
    if base is None and apps:
        base = unwrap(apps[0])
    if base is None:
        return "no appearance to model on -- skipped"
    chunks = len(base.get("chunkMaterials") or []) or 1

    ext = root.setdefault("externalMaterials", [])
    entries = root.setdefault("materialEntries", [])
    for app_name, mi in LEVELS:
        mat_name = "vrp_" + app_name
        index = None
        for i, e in enumerate(ext):
            if val((e.get("DepotPath") or {})) == mi:
                index = i
                break
        if index is None:
            ext.append(respath(mi))
            index = len(ext) - 1

        if not any(val(unwrap(m).get("name")) == mat_name for m in entries):
            entry = {"$type": "CMeshMaterialEntry", "index": index, "isLocalInstance": 0,
                     "name": cname(mat_name)}
            # Mirror however the file wraps its existing entries: handle-wrapped or bare.
            if entries and isinstance(entries[0], dict) and "Data" in entries[0]:
                entry = {"HandleId": str(next_handle(doc)), "Data": entry}
            entries.append(entry)

        appearance = {"$type": "meshMeshAppearance", "chunkMaterials": [cname(mat_name)] * chunks,
                      "name": cname(app_name), "tags": []}
        if apps and isinstance(apps[0], dict) and "Data" in apps[0]:
            appearance = {"HandleId": str(next_handle(doc)), "Data": appearance}
        apps.append(appearance)

    io.open(path, "w", encoding="utf-8", newline="").write(json.dumps(doc, indent=2))
    return "%d lit appearance(s), %d chunk(s) each" % (len(LEVELS), chunks)


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    ent_dir, mesh_root, out_dir = sys.argv[1], sys.argv[2], sys.argv[3]
    mesh_out = os.path.join(out_dir, "meshes")
    os.makedirs(mesh_out, exist_ok=True)

    wanted = {}
    for name in sorted(os.listdir(ent_dir)):
        if not name.endswith(".ent.json"):
            continue
        doc = json.load(io.open(os.path.join(ent_dir, name), encoding="utf-8"))
        touched = 0
        for c in doc["Data"]["RootChunk"].get("components") or []:
            if c.get("$type") != "entMeshComponent":
                continue
            src = val(((c.get("mesh") or {}).get("DepotPath") or {}))
            if not src:
                continue
            leaf = src.split(BS)[-1]
            if not src.startswith("base" + BS + "vrport" + BS):
                wanted[leaf] = src
                c["mesh"]["DepotPath"]["$value"] = OUR_DIR + BS + leaf
                touched += 1
            else:
                wanted[leaf] = src
        if touched:
            io.open(os.path.join(ent_dir, name), "w", encoding="utf-8",
                    newline="").write(json.dumps(doc, indent=2))
        print("%-34s repointed: %d" % (name, touched))

    print("")
    print("copying %d mesh(es)" % len(wanted))
    missing = []
    for leaf, src in sorted(wanted.items()):
        found = None
        folder = src.split(BS)[-2] if BS in src else ""
        for base, _, files in os.walk(mesh_root):
            if leaf in files and folder and folder in base.replace("/", BS):
                found = os.path.join(base, leaf)
                break
        if found is None:
            for base, _, files in os.walk(mesh_root):
                if leaf in files:
                    found = os.path.join(base, leaf)
                    break
        if found is None:
            missing.append(leaf)
            continue
        shutil.copyfile(found, os.path.join(mesh_out, leaf))
    if missing:
        print("NOT FOUND:", ", ".join(missing))
        return 1
    print("copied:", len(wanted) - len(missing))
    return 0


if __name__ == "__main__":
    sys.exit(main())
