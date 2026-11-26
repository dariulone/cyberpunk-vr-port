"""Give the weapons whose iconic appearances carry a scope as their own components (Panam's Grad: sniper_01, Buck's
Grad: long_01, the scavenger Grad and Masamune's scope preset: long_02, Breakthrough: sniper_03) the port's optical
surfaces in those appearances.

Such a scope is no attachment: the preset's scope part is a dummy (Items.Panam_scope, Items.Breakthrough_scope) and the
weapon's appearance draws the scope itself, so the optics mod found the scope's profile on the weapon but none of the
surfaces build_scope_assets.py adds to the attachment's .app, and fell back to the native view (no camera image). The
same components are added here, built from the weapon appearance's own scope components and panels; the meshes are
the attachment's (base/vrport/optics/<key>_*.mesh).

    py -3.13 embed_scope_optics.py --app <weapon .app.json> --scope-app <key>=<our attachment .app.json> ... --out <json>
"""
import argparse, copy, json, re
from pathlib import Path

import build_scope_assets as assets

ROOT = Path(__file__).resolve().parents[2]
PROFILES = ROOT / 'mods/cet/CyberpunkVRPort_Optics/optics/profiles.lua'


def profiles():
    """Main component name -> (key, reticle layers) from the generated profiles.lua."""
    out = {}
    for line in PROFILES.read_text(encoding='utf-8').splitlines():
        m = re.match(r"\s*\['([^']+)'\] = \{ key='([^']+)'.*?(?:reticles=\{(.*)\})? \},$", line)
        if not m:
            continue
        layers = [dict(re.findall(r"(\w+)='([^']*)'", body)) for body in re.findall(r'\{([^{}]*)\}', m.group(3) or '')]
        out[m.group(1)] = (m.group(2), layers)
    return out


def panel_template(scope_app):
    """The authored vr_optic_panel of an attachment .app, its bindings resolved inline."""
    handles = {v['HandleId']: v['Data'] for v in assets.walk(scope_app) if 'HandleId' in v and 'Data' in v}
    for app in scope_app['Data']['RootChunk']['appearances']:
        for chunk in app['Data']['compiledData']['Data']['Chunks']:
            if chunk['name']['$value'] == 'vr_optic_panel':
                panel = copy.deepcopy(chunk)
                for field in ('parentTransform', 'skinning', 'meshTargetBinding'):
                    binding = panel.get(field)
                    if binding and binding.get('HandleId') != '-1':
                        panel[field] = {'HandleId': '0', 'Data': copy.deepcopy(binding.get('Data') or handles[binding['HandleRefId']])}
                return panel
    raise KeyError('vr_optic_panel')


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--app', type=Path, required=True)
    ap.add_argument('--scope-app', action='append', required=True, help='key=path of our attachment .app.json')
    ap.add_argument('--out', type=Path, required=True)
    args = ap.parse_args()
    doc = assets.read(args.app)
    templates = {}
    for item in args.scope_app:
        key, path = item.split('=', 1)
        templates[key] = panel_template(assets.read(Path(path)))
    known = profiles()
    done = []
    for app in doc['Data']['RootChunk']['appearances']:
        data = app['Data']
        chunks = data['compiledData']['Data']['Chunks']
        names = {c['name']['$value'] for c in chunks}
        assert not any(n.startswith('vr_optic_') for n in names), 'already authored: ' + data['name']['$value']
        found = [n for n in names if n in known]
        if not found:
            continue
        main = found[0]
        key, layers = known[main]
        if key not in templates:
            raise SystemExit('no --scope-app for ' + key + ' (appearance ' + data['name']['$value'] + ')')
        mesh = assets.path_of(next(c for c in chunks if c['name']['$value'] == main)['mesh'])
        # Ids above every attachment's own (id_base): the attachment's surfaces never meet these on one entity,
        # but a weapon may still carry an attachment scope next to its own.
        assets.add_components(doc, mesh, key, templates[key], reticles=layers,
                              appearances={data['name']['$value']}, id_base=500)
        done.append((data['name']['$value'], key))
    args.out.write_text(json.dumps(doc, indent=2), encoding='utf-8')
    print(args.app.name, done)


if __name__ == '__main__':
    main()
