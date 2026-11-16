# -*- coding: utf-8 -*-
"""Add the two HAND PHYSICS capsules to a player entity template.

A kinematic body pushes dynamic objects and nothing pushes it, which is what a hand shoving a bottle wants. Bound to
the WeaponLeft / WeaponRight slots like the hold carriers beside them (add_hold_components.py), so the engine carries
them in the same pass that draws the hand. Authored off: CyberpunkVRPort_HandCollision turns one on only for an empty
hand well clear of the body, because a physics body inside the character controller's cylinder is what once launched the
player. Named VRPortHand_ and not VRPortBody_: that prefix sends the basketball mod to their physical body interface,
and the hit proxies behind it are the crash of 2026-09-09.

THE FILTER IS THEIR OWN, from engine/physics/collision_presets.json. "World Dynamic" was the first answer and everything
wrong with it was in that file: it is a PlayerBlocker and a VehicleBlocker, it answers Visibility queries (the module's
own rays found the capsule and stopped the hand on it, and the arm shook), and it contacts Destructible, whose breaking
is what a shoved prop did. Its own type is "Dynamic" alone and it collides with Dynamic and Debris. A contact takes both
sides' masks, so nothing else is touched: the Debris preset has Dynamic in its mask but not Debris, which is why a type of
"Debris" passed through most small things, and Player, AI, Vehicle, Destructible and Terrain are in no mask of the hand's.
The character controller has Dynamic in its mask too, so the hand's capsule stays off near the body. No query detects it.

Usage:  python tools/ent/add_hand_physics.py <player.ent.json> [more.json ...]
Idempotent: a file that already carries them is left alone.
"""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from add_hold_components import add, cname, enable_mask, identity_transform, BIND_TO

LEFT = 'VRPortHand_l'
RIGHT = 'VRPortHand_r'
RADIUS = 0.05
HEIGHT = 0.08


def capsule(name, slot, handle, comp_id):
    return {
        '$type': 'entColliderComponent',
        'colliders': [{
            'HandleId': str(handle),
            'Data': {
                '$type': 'physicsColliderCapsule',
                'filterData': None,
                'height': HEIGHT,
                'isImported': 0,
                'isQueryShapeOnly': 0,
                'localToBody': {
                    '$type': 'Transform',
                    'orientation': {'$type': 'Quaternion', 'i': 0, 'j': 0, 'k': 0, 'r': 1},
                    'position': {'$type': 'Vector4', 'W': 0, 'X': 0, 'Y': 0, 'Z': 0},
                },
                'material': cname('None'),
                'materialApperanceOverrides': [],
                'radius': RADIUS,
                'tag': cname('None'),
                'volumeModifier': 1,
            },
        }],
        'comOffset': {
            '$type': 'Transform',
            'orientation': {'$type': 'Quaternion', 'i': 0, 'j': 0, 'k': 0, 'r': 1},
            'position': {'$type': 'Vector4', 'W': 0, 'X': 0, 'Y': 0, 'Z': 0},
        },
        'dynamicTrafficSetting': {'$type': 'TrafficGenDynamicTrafficSetting', 'impact': 'Ignored'},
        'filterData': {
            'HandleId': str(handle + 1),
            'Data': {
                '$type': 'physicsFilterData',
                'customFilterData': {
                    'HandleId': str(handle + 3),
                    'Data': {
                        '$type': 'physicsCustomFilterData',
                        'collideWith': [cname('Dynamic'), cname('Debris')],
                        'collisionType': [cname('Dynamic')],
                        'queryDetect': [],
                    },
                },
                'preset': cname('None'),
                'queryFilter': {'$type': 'physicsQueryFilter', 'mask1': '0', 'mask2': '0'},
                'simulationFilter': {'$type': 'physicsSimulationFilter', 'mask1': '0', 'mask2': '0'},
            },
        },
        'id': comp_id,
        'inertia': {'$type': 'Vector3', 'X': 1, 'Y': 1, 'Z': 1},
        'isEnabled': 0,
        'isReplicable': 0,
        'localTransform': identity_transform(),
        'mass': 1,
        'massOverride': -1,
        'name': cname(name),
        'parentTransform': {
            'HandleId': str(handle + 2),
            'Data': {
                '$type': 'entHardTransformBinding',
                'bindName': cname(BIND_TO),
                'enabled': 1,
                'enableMask': enable_mask(),
                'slotName': cname(slot),
            },
        },
        'simulationType': 'Kinematic',
        'startInactive': 1,
        'useCCD': 0,
        'volume': 0,
    }


def patch(path):
    return add(path, (LEFT, RIGHT), lambda handle, base_id: [
        capsule(LEFT, 'WeaponLeft', handle, str(base_id)),
        capsule(RIGHT, 'WeaponRight', handle + 4, str(base_id + 1))])


if __name__ == '__main__':
    for target in sys.argv[1:]:
        where, what = patch(target)
        print('%-40s %s' % (os.path.basename(where), what))
