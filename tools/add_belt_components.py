# -*- coding: utf-8 -*-
"""Add a component pair -- the element and its mark twin -- to the belt entity.

WHY A TOOL AND NOT A ONE-OFF. Every belt element the port adds is the same four edits, and getting
any of them wrong is silent: the component has to exist TWICE, once in `RootChunk.components` and
once in `compiledData.Data.Chunks`, and each copy needs its own `entHardTransformBinding` /
`entSkinningBinding` handle pair with ids that collide with nothing else in the file. A component
added to only one of the two lists loads without complaint and never appears on the body.

The model is an existing component of ours, so everything not named here -- the bindings' shape, the
chunk mask, the rendering-plane animation parameter the port drives -- is inherited rather than
re-derived. That parameter is why these are components of the belt and not props in a slot: the belt
already follows the player's rendering plane, so its elements do too.

Every element ships DISABLED. What turns one on is the module's rule for it, evaluated per frame; a
component enabled in the asset would hang on the belt in the menus and in every cutscene.

Usage:
    python tools/add_belt_components.py <ucbeltacc.ent.json> <name> <mesh> <appearance> [more...]
                                        [--no-twin] [--model <component>]

--model copies another component of the entity instead of the grenade holder: the parts of a belt
magazine are made from the magazine element itself.

Each element is three arguments. The mark twin is created alongside with the suffix `_lit` and the
`lit` appearance, and both point at the SAME mesh -- a mark that draws different geometry from the
thing it marks is a mark in the wrong place.
"""
import copy
import io
import json
import sys

V = "$value"
BS = chr(92)
MESH_DIR = "base" + BS + "vrport" + BS + "meshes" + BS
MODEL = "ucbeltgrenade"


def cname(v):
    return {"$type": "CName", "$storage": "string", "$value": v}


def next_handle(doc):
    """The next free HandleId in the WHOLE file -- ids are file-global, not per-array."""
    top = [0]

    def walk(n):
        if isinstance(n, dict):
            if "HandleId" in n:
                try:
                    top[0] = max(top[0], int(n["HandleId"]))
                except (TypeError, ValueError):
                    pass
            for v in n.values():
                walk(v)
        elif isinstance(n, list):
            for v in n:
                walk(v)

    walk(doc)
    return top[0] + 1


def main():
    # --no-twin: the element gets no `_lit` companion. The module falls back to swapping the host's own
    # appearance when it has no twin, and every mesh the port copies carries the eight `lit`
    # appearances -- so for a set as large as one magazine per weapon the twin is fifty components
    # bought for nothing.
    args = [a for a in sys.argv[1:] if a != "--no-twin"]
    no_twin = len(args) != len(sys.argv) - 1
    model_name = MODEL
    if "--model" in args:
        i = args.index("--model")
        model_name = args[i + 1]
        del args[i:i + 2]
    if len(args) < 4 or (len(args) - 1) % 3 != 0:
        print(__doc__)
        return 2
    path = args[0]
    args = args[1:]

    doc = json.load(io.open(path, encoding="utf-8"))
    root = doc["Data"]["RootChunk"]
    comps = root["components"]
    chunks = root["compiledData"]["Data"]["Chunks"]

    model_c = next(c for c in comps if (c.get("name") or {}).get(V) == model_name)
    model_k = next(c for c in chunks
                   if isinstance(c, dict) and (c.get("name") or {}).get(V) == model_name)
    nxt = next_handle(doc)

    added = []
    for i in range(0, len(args), 3):
        base_name, mesh, appearance = args[i], args[i + 1], args[i + 2]
        if BS not in mesh:
            mesh = MESH_DIR + mesh
        pairs = (("", appearance),) if no_twin else (("", appearance), ("_lit", "lit"))
        for suffix, app in pairs:
            name = base_name + suffix
            if any((c.get("name") or {}).get(V) == name for c in comps):
                print("%-28s уже есть -- пропущен" % name)
                continue
            for target, model in ((comps, model_c), (chunks, model_k)):
                c = copy.deepcopy(model)
                c["name"] = cname(name)
                c["meshAppearance"] = cname(app)
                c["isEnabled"] = 0
                c["mesh"] = {"DepotPath": {"$type": "ResourcePath", "$storage": "string",
                                           "$value": mesh}, "Flags": "Default"}
                for field, handle in (("parentTransform", nxt), ("skinning", nxt + 1)):
                    src = model.get(field) or {}
                    if "HandleRefId" in src:
                        c[field] = {"HandleRefId": str(handle)}
                    else:
                        c[field] = copy.deepcopy(src)
                        c[field]["HandleId"] = str(handle)
                target.append(c)
            nxt += 2
            added.append((name, app, mesh.split(BS)[-1]))

    io.open(path, "w", encoding="utf-8", newline="").write(json.dumps(doc, indent=2))
    for name, app, mesh in added:
        print("%-28s app=%-10s %s" % (name, app, mesh))
    print("компонентов теперь: %d,  чанков: %d" % (len(comps), len(chunks)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
