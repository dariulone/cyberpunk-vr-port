# -*- coding: utf-8 -*-
"""Add the two HELD-OBJECT carriers to a player entity template.

WHY THIS EXISTS. A spawned entity placed by script cannot ride a hand: measured on 2026-08-16, the
script's own reading is NOT stale (the plugin's engine-side player position and CET's agree to under
half a millimetre at 8 m/s), but one frame of a running player is 21 cm, and the entity reaches the
screen a frame behind the hand that is drawn from the pose. No lead, filter or anchor fixes that --
it is a race the script is not in.

A COMPONENT ON THE PLAYER'S OWN TEMPLATE is not in that race at all. Bound to a slot through
`entHardTransformBinding`, it is carried by the engine in the same pass that draws the hand, so it is
exact by construction and costs nothing per frame. Runtime `AddComponent` was tried first and does
not stick -- it returns without error and the component is not on the entity afterwards -- so the
component has to exist in the template.

WHAT IT GIVES. Two carriers, one per hand, disabled and empty until something asks for them:

    mesh / meshAppearance   what is being held        (any mesh, swapped live)
    SetLocalTransform       how it sits in the hand   (the measured grip)
    Toggle                  picked up / put down

which is the whole of "hold any object in the hand", not just a magazine.

The slots are `WeaponLeft` / `WeaponRight` on the player's `ItemAttachmentSlots` -- deliberately the
same ones the reload module already reads with GetSlotTransform, so every grip offset it measured
carries over with no rework.

Usage:  python tools/ent/add_hold_components.py <player.ent.json> [more.json ...]
Idempotent: a file that already carries the carriers is left alone. Other pairs of carriers on the same two slots
come from `patch(path, left, right)` (tools/reload_glow: the reload's glow).
"""
import copy
import json
import io
import sys

LEFT = 'vrp_hold_left'
RIGHT = 'vrp_hold_right'
BIND_TO = 'ItemAttachmentSlots'

# A mesh has to be nameable at build time; this one is ours, it is small, and the component ships
# disabled, so nothing is drawn until a script points it at something and turns it on.
PLACEHOLDER_MESH = 'vrbasketball\\vr_basketball.mesh'


def cname(value):
    return {'$type': 'CName', '$storage': 'string', '$value': value}


def enable_mask():
    # copied from the template's own bindings: every one of them carries this exact mask
    return {
        '$type': 'entTagMask',
        'excludedTags': {'$type': 'redTagList', 'tags': [cname('NoBinding')]},
        'hardTags': {'$type': 'redTagList', 'tags': []},
        'softTags': {'$type': 'redTagList', 'tags': []},
    }


def identity_transform():
    return {
        '$type': 'WorldTransform',
        'Orientation': {'$type': 'Quaternion', 'i': 0, 'j': 0, 'k': 0, 'r': 1},
        'Position': {
            '$type': 'WorldPosition',
            'x': {'$type': 'FixedPoint', 'Bits': 0},
            'y': {'$type': 'FixedPoint', 'Bits': 0},
            'z': {'$type': 'FixedPoint', 'Bits': 0},
        },
    }


def carrier(name, slot, handle_id, comp_id):
    return {
        '$type': 'entMeshComponent',
        'castLocalShadows': 'Never',
        'castRayTracedGlobalShadows': 'Never',
        'castRayTracedLocalShadows': 'Never',
        'castShadows': 'Never',
        'chunkMask': '18446744073709551615',
        'forcedLodDistance': 'Default',
        'id': comp_id,
        'isEnabled': 0,
        'isReplicable': 0,
        'localTransform': identity_transform(),
        'LODMode': 'AlwaysVisible',
        'mesh': {
            'DepotPath': {'$type': 'ResourcePath', '$storage': 'string', '$value': PLACEHOLDER_MESH},
            'Flags': 'Default',
        },
        'meshAppearance': cname('default'),
        'name': cname(name),
        'navigationImpact': {'$type': 'NavGenNavigationSetting', 'navmeshImpact': 'Ignored'},
        'order': 0,
        'overrideMeshNavigationImpact': 1,
        'parentTransform': {
            'HandleId': str(handle_id),
            'Data': {
                '$type': 'entHardTransformBinding',
                'bindName': cname(BIND_TO),
                'enabled': 1,
                'enableMask': enable_mask(),
                'slotName': cname(slot),
            },
        },
        # FIRST PERSON DRAWS THE GUN AND THE ARMS IN `RPl_Weapon`, not in the scene, which is what keeps them
        # out of the world's depth and lighting. A carrier left on the scene plane is composited against the
        # weapon instead of with it and reads as a magazine you can see the gun through.
        'renderingPlane': 'RPl_Weapon',
        'renderSceneLayerMask': 'Default',
        'version': 1,
        'visualScale': {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1},
    }


def max_handle(node, best=-1):
    if isinstance(node, dict):
        for key in ('HandleId', 'HandleRefId'):
            v = node.get(key)
            if isinstance(v, str) and v.isdigit():
                best = max(best, int(v))
        for v in node.values():
            best = max(best, max_handle(v))
    elif isinstance(node, list):
        for v in node:
            best = max(best, max_handle(v))
    return best


def named(comp):
    n = comp.get('name')
    return n.get('$value') if isinstance(n, dict) else n


def add(path, names, make):
    """Components added to a template's JSON, unless one of `names` is already there: `make(handle, base_id)` returns
    them, `handle` the first free handle id and `base_id` a free component id, each one used once per component."""
    with io.open(path, encoding='utf-8', newline='') as handle_in:
        text = handle_in.read()
    doc = json.loads(text)
    root = doc['Data']['RootChunk']
    comps = root.get('components')
    if comps is None:
        return path, 'no components list'
    if any(named(c) in names for c in comps):
        return path, 'already carried'

    handle = max_handle(doc) + 1
    # The ids are CRUIDs and must not collide with an existing one. Derived from the highest present
    # rather than invented, so two files patched by one run cannot agree by accident.
    ids = [int(c['id']) for c in comps if str(c.get('id', '')).isdigit()]
    base_id = (max(ids) if ids else 0) + 1000

    added = make(handle, base_id)
    comps.extend(added)

    # ...and the cooked package beside it, which is what the engine actually instantiates. Both lists
    # exist in a cooked template and they mirror each other; adding to only one is how a component
    # gets authored and never appears.
    chunks = (root.get('compiledData') or {}).get('Data', {}).get('Chunks')
    added_to_chunks = False
    if isinstance(chunks, list):
        chunks.extend(copy.deepcopy(c) for c in added)
        added_to_chunks = True

    # Written as the file was, its indent, line endings and final newline kept: the sources under
    # mods/archive/source/raw are patched in place, and anything else would rewrite every line of them.
    newline = '\r\n' if '\r\n' in text else '\n'
    second = text.split('\n', 2)[1] if text.count('\n') >= 2 else ''
    indent = (len(second) - len(second.lstrip(' '))) or 2
    out = json.dumps(doc, indent=indent, ensure_ascii=False).replace('\n', newline)
    if text.endswith('\n'):
        out += newline
    with io.open(path, 'w', encoding='utf-8', newline='') as handle_out:
        handle_out.write(out)
    return path, 'added %s (handles from %d, ids from %d, chunks=%s)' % (
        ', '.join(names), handle, base_id, added_to_chunks)


def patch(path, left_name=LEFT, right_name=RIGHT):
    return add(path, (left_name, right_name), lambda handle, base_id: [
        carrier(left_name, 'WeaponLeft', handle, str(base_id)),
        carrier(right_name, 'WeaponRight', handle + 1, str(base_id + 1))])


if __name__ == '__main__':
    for target in sys.argv[1:]:
        where, what = patch(target)
        print('%-40s %s' % (where.split('\\')[-1], what))
