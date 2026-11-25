"""Build the standalone katana scabbard archive. Run with the path to cp77tools."""
import argparse
import copy
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
RAW = ROOT / "mods/archive/source/raw/vrport"
sys.path.insert(0, str(ROOT / "tools/vr_lobby_assets"))
from gltf import Builder
from make_lobby_scene import cname, depot, game_material

NAME = "cf_katana_scabbard"
DEPOT = "vrport\\holster\\shell\\"


def geometry(path):
    mesh = Builder()
    # Elliptical, hollow body in weapon-local space. The opening is at the guard.
    def ring(z, width, depth):
        return [(width * math.cos(i * math.tau / 12),
                 depth * math.sin(i * math.tau / 12) - 0.036 * z * z, z) for i in range(12)]

    def connect(a, b, material):
        for i in range(len(a)):
            j = (i + 1) % len(a)
            mesh.quad(material, a[i], a[j], b[j], b[i])

    opening = ring(0.012, 0.018, 0.040)
    outer = ring(0.012, 0.023, 0.046)
    connect(opening, outer, "trim")
    for z, width, depth, material in [(0.031, .023, .046, "trim"), (.037, .021, .044, "trim"),
                                      (.30, .020, .042, "lacquer"), (.62, .019, .039, "lacquer"),
                                      (.90, .017, .032, "lacquer"), (.923, .016, .028, "trim")]:
        next_ring = ring(z, width, depth)
        connect(outer, next_ring, material)
        outer = next_ring
    for i in range(12):
        mesh.tri("trim", (0., -.036 * .923**2, .923), outer[i], outer[(i + 1) % 12])
    inner = ring(.90, .013, .026)
    connect(inner, opening, "lacquer")
    mesh.write(path, NAME)


def build(cli, stage):
    def run(*args):
        result = subprocess.run([cli, *map(str, args)], capture_output=True, text=True)
        if result.returncode and "Imported 1/1 file(s)" not in result.stdout:
            raise RuntimeError(result.stdout + result.stderr)
    def write(path, doc):
        path.write_text(json.dumps(doc), encoding="utf-8")
    def read(path):
        return json.loads(path.read_text(encoding="utf-8-sig"))

    src = stage / "source"
    src.mkdir()
    mesh_path = src / (NAME + ".mesh")
    mesh_json = mesh_path.with_suffix(".mesh.json")
    shutil.copyfile(RAW / "ui/vr_lobby_plane.mesh.json", mesh_json)
    run("convert", "deserialize", mesh_json, "-o", src)
    glb = src / (NAME + ".glb")
    geometry(glb)
    run("import", glb, "-o", src, "-k")
    run("convert", "serialize", mesh_path, "-o", src)
    doc = read(mesh_json)
    root = doc["Data"]["RootChunk"]
    assert len(root["renderResourceBlob"]["Data"]["header"]["renderChunkInfos"]) == 2
    root["materialEntries"] = []
    root["preloadLocalMaterialInstances"] = []
    names = []
    for i, (name, color, metal, rough) in enumerate([
            ("lacquer", (.026, .016, .022), .18, .26), ("trim", (.32, .19, .069), .82, .32)]):
        names.append(cname(name))
        base, values = game_material(dict(base=color, metal=metal, roughness=rough, emission_strength=0))
        root["materialEntries"].append(dict({"$type": "CMeshMaterialEntry"}, index=i, isLocalInstance=1, name=cname(name)))
        root["preloadLocalMaterialInstances"].append({"HandleId": str(100+i), "Data": {
            "$type": "CMaterialInstance", "audioTag": cname("None"), "baseMaterial": {"DepotPath": depot(base),
            "Flags": "Default"}, "cookingPlatform": "PLATFORM_None", "enableMask": 0, "metadata": None,
            "resourceVersion": 4, "values": values}})
    root["appearances"][0]["Data"]["chunkMaterials"] = names
    write(mesh_json, doc)
    ent = read(RAW / "holster/shell/vrp_shell_cknife_l_w.ent.json")
    template = ent["Data"]["RootChunk"]
    comp = template["components"][0]
    comp["mesh"]["DepotPath"] = depot(DEPOT + NAME + ".mesh")
    comp["meshAppearance"] = cname("default")
    template["compiledData"]["Data"]["Chunks"][1] = copy.deepcopy(comp)
    ent_json = src / (NAME + ".ent.json")
    write(ent_json, ent)
    pack = stage / "vrport_katana"
    cooked = pack / "vrport/holster/shell"
    cooked.mkdir(parents=True)
    for file in (ent_json, mesh_json):
        run("convert", "deserialize", file, "-o", cooked)
        assert (cooked / file.stem).is_file()
    run("pack", pack, "-o", stage)
    shutil.copyfile(stage / "vrport_katana.archive", ROOT / "mods/archive/vrport_katana.archive")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    args = parser.parse_args()
    (ROOT / "build").mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(dir=ROOT / "build", prefix="scabbard-") as work:
        build(str(args.cli.resolve()), Path(work))
