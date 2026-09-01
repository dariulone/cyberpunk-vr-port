#!/usr/bin/env python3
# Author the VRCAM assets for every resolution the launcher offers.
#
# The second eye is an entRenderToTextureCameraComponent on the player entity, one per render
# resolution, all shipped disabled; modules/vrcam_select.lua switches on the single one whose name
# matches the launcher's pick. So a resolution that exists in the launcher but not in the .ent is a
# dead menu entry -- the log says "not found among N vrcam_* component(s)" and there is no second
# view at all.
#
# Keeping the two lists in step by hand is what went wrong before, so this reads the ladders
# STRAIGHT OUT OF launcher_dialog.cpp. There is one source of truth and it is the launcher.
#
# What it writes:
#   * the four player .ent.json files  -- appends the missing components
#   * texture_from_camera_<W>x<H>.dtex.json for each new resolution
#   * the "components" catalogue in the CET mod's vrcam.json
#
# After running, import the .json files in WolvenKit (Import from JSON) and pack the project.
#
# Usage:
#   python tools/gen_vrcam_assets.py --dry-run
#   python tools/gen_vrcam_assets.py

import argparse
import io
import json
import os
import re
import struct
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# The restructure moved this: src/vr/overlay/launcher_dialog.cpp -> src/Overlay/LauncherDialog.cpp.
# It is still the single source of truth for which resolutions exist.
LAUNCHER = os.path.join(REPO, "src", "Overlay", "LauncherDialog.cpp")
VRCAM_JSON = os.path.join(REPO, "mods", "cet", "CyberpunkVRPort_Stereo", "vrcam.json")

# The WolvenKit project. Not inside the repo: it is a mod project the user edits in WolvenKit, and
# the .ent/.dtex binaries live next to these .json sources.
PROJECT = os.path.join(os.path.expanduser("~"), "Documents", "CyberpunkVRPort")
RAW = os.path.join(PROJECT, "source", "raw")
ARCHIVE = os.path.join(PROJECT, "source", "archive")

ENT_FILES = [
    "base/characters/entities/player/player_ma_fpp.ent.json",
    "base/characters/entities/player/player_wa_fpp.ent.json",
    "ep1/characters/entities/player/player_ma_fpp_ep1.ent.json",
    "ep1/characters/entities/player/player_wa_fpp_ep1.ent.json",
]
# THE BRAINDANCE REPLACER IS A FIFTH PLAYER. A braindance swaps the player for this entity, so without
# the same components the second eye stays attached to the parked original -- measured in the process at
# 2.8 km from the recording, which is the "left eye looks at nothing" report.
#
# Its components carry their own prefix so the selector can tell the two sets apart on whichever entity
# it is looking at, while virtualCameraName stays vrcam_feed_<W>x<H>: the native side derives its view
# key from that name, and it must not care which of the two is live.
#
# AND JOHNNY IS A SIXTH. The Silverhand missions swap the player for johnny_silverhand_replacer.ent the
# same way a braindance swaps in its own replacer, and the effect is the same: measured live while in
# Johnny's body, that entity carried 0 vrcam components out of 121 (V carries 61 of 217), the selector
# said "player changed (1ULL -> 9003388ULL): 0 component(s) released", and there was no second eye.
#
# It shares the braindance prefix ON PURPOSE. The prefix means "the replacer's twin set", not
# "braindance": the plugin hardcodes exactly this string in CyberpunkVR_VrcamBdCamNameHash (see
# src/Stereo/SyncStereo.cpp) and the camera writer accepts either that or the plain vrcam_ name as the
# second view, so a third prefix would need a plugin build and a selector change to buy nothing. The two
# replacers are never live at the same time, and their names cannot collide with V's own set.
REPLACER_FILES = [
    "base/characters/entities/player/replacer/braindance_replacer.ent.json",
    "base/characters/entities/player/replacer/johnny_silverhand_replacer.ent.json",
]
REPLACER_PREFIX = "vrcam_braindance_"
DTEX_DIR = "base/media/tv/entities"
DTEX_DEPOT = "base\\media\\tv\\entities"

# CRUIDs are per-entity component ids. The first seven were assigned by hand when the VRCAM set was
# first authored (2864x3184 = ...459); everything after continues that run. The table is stable and
# shared by all four entities so a component keeps its id wherever it appears -- ids only have to be
# unique WITHIN one entity, but a stable mapping makes two files diffable.
CRUID_BASE = 4305634056304560459
CRUID_SEED = [                       # the order the ids were originally handed out in
    (2864, 3184), (2444, 2444), (2560, 2560), (2816, 2816),
    (3072, 3072), (3584, 3584), (4096, 4096),
]

# Copied off the authored vrcam_2864x3184 component. Every field that is not resolution-derived is
# here verbatim, so the generated components are identical to the hand-made ones in everything but
# name and size.
FOV = 68.2399979                     # the game's own default; the plugin overrides it live anyway
NEAR_PLANE = 0.0199999996
FAR_PLANE = 16000
STREAMING_DISTANCE = 12500

# A fixed stamp rather than the wall clock: re-running the generator should produce byte-identical
# files, so an unchanged resolution never shows up as a diff.
EXPORTED_AT = "2026-08-01T00:00:00.0000000Z"


def f32(x):
    """The double nearest to float32(x), printed the way WolvenKit prints floats."""
    v = struct.unpack("f", struct.pack("f", x))[0]
    if v == int(v):
        return int(v)
    return float("%.9g" % v)


def cname(value):
    return {"$type": "CName", "$storage": "string", "$value": value}


def null_resource():
    return {
        "DepotPath": {"$type": "ResourcePath", "$storage": "uint64", "$value": "0"},
        "Flags": "Default",
    }


def hard_transform_binding():
    """entHardTransformBinding on the head's camera slot -- what all VRCAM components hang off."""
    return {
        "$type": "entHardTransformBinding",
        "bindName": cname("slots"),
        "enabled": 1,
        "enableMask": {
            "$type": "entTagMask",
            "excludedTags": {"$type": "redTagList", "tags": [cname("NoBinding")]},
            "hardTags": {"$type": "redTagList", "tags": []},
            "softTags": {"$type": "redTagList", "tags": []},
        },
        "slotName": cname("camera"),
    }


def make_component(w, h, cruid, parent_transform, prefix="vrcam_"):
    """One entRenderToTextureCameraComponent. parent_transform is the handle def or a ref.

    `prefix` names the component only. virtualCameraName keeps the vrcam_feed_<W>x<H> form on purpose:
    the render path identifies the view by the hash of THAT name, so a second set under another prefix
    stays invisible to everything downstream."""
    return {
        "$type": "entRenderToTextureCameraComponent",
        "albedoDynamicTextureRes": null_resource(),
        "aspectRatio": f32(w / h),
        "backgroundColor": {"$type": "Color", "Alpha": 255, "Blue": 0, "Green": 0, "Red": 0},
        "depthCutDistance": 0,
        "depthDynamicTextureRes": null_resource(),
        "dynamicTextureRes": {
            "DepotPath": {
                "$type": "ResourcePath",
                "$storage": "string",
                "$value": "%s\\texture_from_camera_%dx%d.dtex" % (DTEX_DEPOT, w, h),
            },
            "Flags": "Default",
        },
        "env": null_resource(),
        "farPlaneOverride": FAR_PLANE,
        "features": {
            "$type": "entRenderToTextureFeatures",
            "antiAliasing": "RTFP_PC",
            "contactShadows": 1,
            "localShadows": 1,
            "reflections": "RTFP_PC",
            "renderDecals": 1,
            "renderForwardNoTXAA": 1,
            "renderParticles": 1,
            "SSAO": "RTFP_PC",
        },
        "fov": FOV,
        "id": str(cruid),
        # Every one of them ships OFF. Lua turns exactly one on, by name.
        "isEnabled": 0,
        "isReplicable": 0,
        "localTransform": {
            "$type": "WorldTransform",
            "Orientation": {"$type": "Quaternion", "i": 0, "j": 0, "k": 0, "r": 1},
            "Position": {
                "$type": "WorldPosition",
                "x": {"$type": "FixedPoint", "Bits": 0},
                "y": {"$type": "FixedPoint", "Bits": 0},
                "z": {"$type": "FixedPoint", "Bits": 0},
            },
        },
        "motionBlurScale": 1,
        "name": cname(prefix + "%dx%d" % (w, h)),
        "nearPlaneOverride": NEAR_PLANE,
        "normalsDynamicTextureRes": null_resource(),
        "overrideBackgroundColor": 0,
        "params": {"$type": "WorldRenderAreaSettings", "areaParameters": []},
        "parentTransform": parent_transform,
        "particlesDynamicTextureRes": null_resource(),
        "renderingMode": "Shaded",
        "renderSceneLayer": "Default",
        "resolutionHeight": h,
        "resolutionWidth": w,
        "streamingDistance": STREAMING_DISTANCE,
        "virtualCameraName": cname("vrcam_feed_%dx%d" % (w, h)),
        "zoom": 1,
    }


def make_dtex(w, h):
    return {
        "Header": {
            "WolvenKitVersion": "8.19.0",
            "WKitJsonVersion": "0.0.9",
            "GameVersion": 2310,
            "ExportedDateTime": EXPORTED_AT,
            "DataType": "CR2W",
            "ArchiveFileName": os.path.join(
                ARCHIVE, DTEX_DIR.replace("/", os.sep),
                "texture_from_camera_%dx%d.dtex" % (w, h)),
        },
        "Data": {
            "Version": 195,
            "BuildVersion": 0,
            "RootChunk": {
                "$type": "DynamicTexture",
                "cookingPlatform": "PLATFORM_PC",
                "dataFormat": "RGBA_Uint8",
                "generator": None,
                "height": h,
                "mipChain": 0,
                "samplesCount": 1,
                # 1 = the texture follows the component's resolution rather than the backbuffer.
                "scaleToViewport": 1,
                "width": w,
            },
            "EmbeddedFiles": [],
        },
    }


def parse_launcher_resolutions(path):
    """Every {W, H, L"..."} in every ResolutionPreset array, keyed by the array name."""
    text = io.open(path, encoding="utf-8", errors="replace").read()
    ladders = {}
    for m in re.finditer(
            r"static\s+const\s+ResolutionPreset\s+(\w+)\s*\[\s*\]\s*=\s*\{(.*?)\}\s*;",
            text, re.S):
        name, body = m.group(1), m.group(2)
        entries = [(int(a), int(b)) for a, b in re.findall(r"\{\s*(\d+)\s*,\s*(\d+)\s*,", body)]
        if entries:
            ladders[name] = entries
    return ladders


def component_name(c):
    n = c.get("name")
    return n.get("$value") if isinstance(n, dict) else n


def load_json(path):
    """Returns (data, style) where style carries the byte-level details worth preserving."""
    raw = io.open(path, "rb").read()
    style = {
        "bom": raw.startswith(b"\xef\xbb\xbf"),
        "crlf": raw.count(b"\r\n") > raw.count(b"\n") // 2,
    }
    return json.loads(raw.decode("utf-8-sig")), style


def save_json(path, data, style, indent=2):
    body = json.dumps(data, indent=indent, ensure_ascii=False)
    if style.get("crlf"):
        body = body.replace("\n", "\r\n")
    nl = "\r\n" if style.get("crlf") else "\n"
    with io.open(path, "wb") as f:
        if style.get("bom"):
            f.write(b"\xef\xbb\xbf")
        f.write((body + nl).encode("utf-8"))


def backup_once(path):
    """A .orig beside the file, written only the first time. These are hand-authored WolvenKit
    sources; a bad edit is not something to discover after the original is gone."""
    bak = path + ".orig"
    if not os.path.exists(bak):
        with io.open(path, "rb") as src, io.open(bak, "wb") as dst:
            dst.write(src.read())
        return True
    return False


def vrcam_parent_handle(chunks, all_handle_ids):
    """Reuse the binding an existing VRCAM component defines; otherwise mint a fresh handle id.

    Reusing ANY camera-slot binding would work -- the FPP camera has one -- but that aliases this
    component's parentTransform onto another component's object. Only VRCAM's own is shared.
    """
    for ch in chunks:
        if ch.get("$type") != "entRenderToTextureCameraComponent":
            continue
        pt = ch.get("parentTransform")
        if isinstance(pt, dict) and "HandleId" in pt:
            return pt["HandleId"], False
    return str(max(all_handle_ids) + 1), True


def verify_ent(path, wanted, prefix="vrcam_"):
    """Re-read what was just written and check the invariants that make the file loadable."""
    data, _ = load_json(path)
    root = data["Data"]["RootChunk"]
    comps, package = root["components"], root["compiledData"]["Data"]
    chunks, cruid = package["Chunks"], package["CruidDict"]
    problems = []
    if len(chunks) != len(comps) + 1:
        problems.append("chunks(%d) != components(%d)+1" % (len(chunks), len(comps)))
    for i, c in enumerate(comps):
        if component_name(chunks[i + 1]) != component_name(c):
            problems.append("components[%d] and chunks[%d] disagree" % (i, i + 1))
            break
    if len(cruid) != len(chunks):
        problems.append("CruidDict has %d entries for %d chunks" % (len(cruid), len(chunks)))
    for k, v in cruid.items():
        i = int(k)
        if i and str(chunks[i].get("id")) != str(v):
            problems.append("CruidDict[%s]=%s but chunks[%d].id=%s" % (k, v, i, chunks[i].get("id")))
            break
    defined = set()
    for ch in chunks:
        pt = ch.get("parentTransform")
        if isinstance(pt, dict) and "HandleId" in pt:
            defined.add(pt["HandleId"])
    for ch in chunks:
        pt = ch.get("parentTransform")
        if isinstance(pt, dict) and pt.get("HandleRefId") and pt["HandleRefId"] not in defined:
            problems.append("chunk parentTransform refs undefined handle %s" % pt["HandleRefId"])
            break
    # DUPLICATE IDS. The check that was missing: CRUIDs must be unique within an entity, and the
    # id table used to be recomputed from scratch on every run, so a ladder that grew at the front
    # could hand a new component an id an old one already owned. Twelve such collisions were found
    # by hand before they were ever written; they would have passed every check above.
    seen_ids = {}
    for c in comps:
        cid = str(c.get("id"))
        if cid in seen_ids:
            problems.append("duplicate component id %s: %s and %s"
                            % (cid, seen_ids[cid], component_name(c)))
            break
        seen_ids[cid] = component_name(c)
    names = [component_name(c) for c in comps if c.get("$type") == "entRenderToTextureCameraComponent"]
    if len(names) != len(set(names)):
        problems.append("duplicate vrcam component names")
    missing = [w for w in wanted if (prefix + "%dx%d" % w) not in set(names)]
    if missing:
        problems.append("%d wanted resolution(s) still absent" % len(missing))
    return problems, len(names)


def process_ent(path, wanted, dry_run, prefix="vrcam_"):
    data, style = load_json(path)
    root = data["Data"]["RootChunk"]
    comps = root["components"]
    package = root["compiledData"]["Data"]
    chunks = package["Chunks"]
    cruid = package["CruidDict"]

    # components[i] IS chunks[i+1] -- same object, serialised twice; chunks[0] is the entity itself.
    # Handle DEFINITIONS land in compiledData because it comes first in the document, and
    # `components` carries the refs. Appending to both keeps that invariant and, unlike inserting,
    # leaves every existing CruidDict key pointing at the same chunk.
    if len(chunks) != len(comps) + 1:
        raise SystemExit("%s: chunks(%d) != components(%d)+1 -- unexpected layout, refusing to edit"
                         % (os.path.basename(path), len(chunks), len(comps)))

    have = {component_name(c) for c in comps}
    raw_text = io.open(path, encoding="utf-8-sig").read()
    handle_ids = [int(x) for x in re.findall(r'"HandleId"\s*:\s*"(\d+)"', raw_text)]
    parent_id, mint = vrcam_parent_handle(chunks, handle_ids)

    added = []
    define_here = mint
    for (w, h) in wanted:
        name = prefix + "%dx%d" % (w, h)
        if name in have:
            continue
        cid = cruid_for(w, h)
        # First new chunk in a file that has no VRCAM binding yet carries the definition.
        if define_here:
            chunk_pt = {"HandleId": parent_id, "Data": hard_transform_binding()}
            define_here = False
        else:
            chunk_pt = {"HandleRefId": parent_id}
        chunks.append(make_component(w, h, cid, chunk_pt, prefix))
        comps.append(make_component(w, h, cid, {"HandleRefId": parent_id}, prefix))
        cruid[str(len(chunks) - 1)] = str(cid)
        added.append(name)

    made_backup = False
    if added and not dry_run:
        made_backup = backup_once(path)
        save_json(path, data, style)
    return added, parent_id, mint, made_backup


_CRUID_TABLE = {}


def cruid_for(w, h):
    return _CRUID_TABLE[(w, h)]


def scan_existing_cruids(ent_paths):
    """(name -> id) for every vrcam component already authored, plus every id in use anywhere.

    The second half matters: a fresh id must not collide with ANY component's id in the entity, not
    just another vrcam one, and the vanilla components carry ids of their own.
    """
    named, used = {}, set()
    for path in ent_paths:
        if not os.path.isfile(path):
            continue
        data, _ = load_json(path)
        for c in data["Data"]["RootChunk"]["components"]:
            try:
                cid = int(c["id"])
            except (KeyError, TypeError, ValueError):
                continue
            used.add(cid)
            name = component_name(c)
            if isinstance(name, str) and name.startswith("vrcam_"):   # both prefixes start with it
                named.setdefault(name, cid)
    return named, used


def build_cruid_table(all_res, existing_named=None, ids_in_use=None):
    """Ids already authored are AUTHORITATIVE; anything new is appended ABOVE the highest of ours.

    THE OLD VERSION WAS ONLY STABLE WHILE THE LADDER SET NEVER GREW AT THE FRONT. It handed out
    CRUID_BASE + n over the seed list followed by the sorted remainder, so inserting a resolution
    that sorts EARLY shifted every id after it by one -- while process_ent deliberately leaves an
    existing component's id alone. Adding the Bigscreen Beyond ladder did exactly that (1920x1552
    sorts before the authored 1920x1880), and the result was measured before anything was written:
    three collisions per entity, twelve in all, each handing a NEW component an id an OLD component
    already owned. CRUIDs must be unique within an entity, and verify_ent did not test for duplicates
    -- it does now.

    So: pin what is authored, and allocate strictly above it. That is stable under every future
    ladder change, not just this one, because an id once handed out is never recomputed.
    """
    existing_named = existing_named or {}
    ids_in_use = set(ids_in_use or ())

    # Whatever the entities say, first. The seed list stays as the historical record of the order the
    # first seven were handed out in, and is only consulted for resolutions nothing has authored yet.
    for wh in CRUID_SEED:
        name = "vrcam_%dx%d" % wh
        if name in existing_named:
            _CRUID_TABLE[wh] = existing_named[name]
    for wh in sorted(all_res):
        name = "vrcam_%dx%d" % wh
        if wh not in _CRUID_TABLE and name in existing_named:
            _CRUID_TABLE[wh] = existing_named[name]

    # Fresh ids continue our own contiguous run rather than starting from the file's global maximum:
    # the vanilla ids are elsewhere entirely, and keeping ours consecutive keeps the diffs readable.
    ours = [v for v in _CRUID_TABLE.values() if CRUID_BASE <= v < CRUID_BASE + 100000]
    nxt = (max(ours) + 1) if ours else CRUID_BASE
    for wh in sorted(all_res):
        if wh in _CRUID_TABLE:
            continue
        while nxt in ids_in_use or nxt in set(_CRUID_TABLE.values()):
            nxt += 1
        _CRUID_TABLE[wh] = nxt
        nxt += 1


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dry-run", action="store_true", help="report what would change, write nothing")
    ap.add_argument("--project", default=PROJECT, help="WolvenKit project root")
    args = ap.parse_args()

    global RAW, ARCHIVE
    RAW = os.path.join(args.project, "source", "raw")
    ARCHIVE = os.path.join(args.project, "source", "archive")
    if not os.path.isdir(RAW):
        raise SystemExit("no source/raw under %s" % args.project)

    ladders = parse_launcher_resolutions(LAUNCHER)
    if not ladders:
        raise SystemExit("parsed no resolution ladders out of %s" % LAUNCHER)

    launcher_res = []
    for name in sorted(ladders):
        for wh in ladders[name]:
            if wh not in launcher_res:
                launcher_res.append(wh)
    launcher_res.sort()

    print("launcher ladders: %d, %d unique resolutions" % (len(ladders), len(launcher_res)))
    for name in sorted(ladders):
        print("   %-28s %s" % (name, ", ".join("%dx%d" % r for r in ladders[name])))
    print()

    # Legacy entries stay authored even when no ladder offers them: 2444x2444 is the fallback the
    # plugin drops to when vrcam.json is missing, and the big square ones predate the FOV work.
    wanted = list(launcher_res)
    for wh in CRUID_SEED:
        if wh not in wanted:
            wanted.append(wh)
    ent_paths = [os.path.join(RAW, rel.replace("/", os.sep)) for rel in ENT_FILES]
    existing_named, ids_in_use = scan_existing_cruids(ent_paths)
    build_cruid_table(wanted, existing_named, ids_in_use)
    wanted.sort()
    fresh = sorted(wh for wh in wanted if "vrcam_%dx%d" % wh not in existing_named)
    print("CRUIDs: %d already authored, %d new (offsets %s)"
          % (len(existing_named), len(fresh),
             "%d..%d" % (cruid_for(*fresh[0]) - CRUID_BASE, cruid_for(*fresh[-1]) - CRUID_BASE)
             if fresh else "none"))
    print()

    total_added = 0
    failures = []
    for rel in ENT_FILES:
        path = os.path.join(RAW, rel.replace("/", os.sep))
        if not os.path.isfile(path):
            print("SKIP  %s (not in the project)" % rel)
            continue
        added, parent_id, minted, backed_up = process_ent(path, wanted, args.dry_run)
        total_added += len(added)
        print("%-46s +%d component(s), parentTransform handle %s%s%s"
              % (os.path.basename(path), len(added), parent_id,
                 " (new)" if minted else " (reused)", "  [.orig saved]" if backed_up else ""))
        if added:
            print("      %s" % ", ".join(added))
        if not args.dry_run:
            problems, count = verify_ent(path, wanted)
            if problems:
                failures.append((os.path.basename(path), problems))
                print("      VERIFY FAILED: %s" % "; ".join(problems))
            else:
                print("      verified: %d vrcam component(s), handles and CRUIDs consistent" % count)

    # THE BRAINDANCE REPLACER, same pass with its own prefix. Its components share the resolutions and
    # the CRUID table with the player's -- ids only have to be unique within one entity -- and differ
    # only in name, so the selector can tell which set belongs to the entity it is holding.
    for rel in REPLACER_FILES:
        path = os.path.join(RAW, rel.replace("/", os.sep))
        if not os.path.isfile(path):
            print("SKIP  %s (not in the project)" % rel)
            continue
        added, parent_id, minted, backed_up = process_ent(path, wanted, args.dry_run, REPLACER_PREFIX)
        total_added += len(added)
        print("%-46s +%d component(s), parentTransform handle %s%s%s"
              % (os.path.basename(path), len(added), parent_id,
                 " (new)" if minted else " (reused)", "  [.orig saved]" if backed_up else ""))
        if added:
            print("      %s" % ", ".join(added[:3]) + (" ..." if len(added) > 3 else ""))
        if not args.dry_run:
            problems, count = verify_ent(path, wanted, REPLACER_PREFIX)
            if problems:
                failures.append((os.path.basename(path), problems))
                print("      VERIFY FAILED: %s" % "; ".join(problems))
            else:
                print("      verified: %d component(s), handles and CRUIDs consistent" % count)

    # Dynamic textures. Only for what has neither a .dtex.json source nor an already-imported
    # .dtex -- the first seven were imported long ago and their JSON was not kept.
    dtex_dir = os.path.join(RAW, DTEX_DIR.replace("/", os.sep))
    if not args.dry_run:
        os.makedirs(dtex_dir, exist_ok=True)
    made = []
    for (w, h) in wanted:
        stem = "texture_from_camera_%dx%d" % (w, h)
        js = os.path.join(dtex_dir, stem + ".dtex.json")
        binary = os.path.join(ARCHIVE, DTEX_DIR.replace("/", os.sep), stem + ".dtex")
        if os.path.isfile(js) or os.path.isfile(binary):
            continue
        if not args.dry_run:
            save_json(js, make_dtex(w, h), style={"bom": False, "crlf": True})
        made.append(stem)
    print()
    print("dynamic textures: %d new .dtex.json" % len(made))
    for m in made:
        print("      %s" % m)

    # The catalogue the launcher reads. Only names listed here are offered, so it has to match what
    # the entities now carry.
    if os.path.isfile(VRCAM_JSON):
        cfg, cfg_style = load_json(VRCAM_JSON)
        cfg["components"] = ["vrcam_%dx%d" % (w, h) for (w, h) in wanted]
        if not args.dry_run:
            save_json(VRCAM_JSON, cfg, cfg_style)
        print()
        print("vrcam.json catalogue: %d entries" % len(cfg["components"]))
    else:
        print()
        print("vrcam.json not found at %s -- catalogue not updated" % VRCAM_JSON)

    print()
    if failures:
        print("VERIFICATION FAILED in %d file(s) -- restore from the .orig backups:" % len(failures))
        for name, problems in failures:
            print("   %s: %s" % (name, "; ".join(problems)))
        return 1
    print("%s %d component(s) added, %d texture(s) written"
          % ("DRY RUN:" if args.dry_run else "done:", total_added, len(made)))
    if not args.dry_run:
        print("next: import the changed .json files in WolvenKit, then pack the project")
    return 0


if __name__ == "__main__":
    sys.exit(main())
