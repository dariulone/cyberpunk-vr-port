"""Cook the left-handed CyberFinger belt using the port's existing skinned meshes."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import uuid

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / "mods/archive/build/base/vrport"
STEM = "vrp_belt_cf_lh"


def crossbody(value):
    if isinstance(value, list):
        return [crossbody(entry) for entry in value]
    if isinstance(value, dict):
        return {key: crossbody(entry) for key, entry in value.items()}
    if isinstance(value, str):
        if value.startswith("base\\vrport\\meshes\\vrp_belt_gren_") and value.endswith("_lh.mesh"):
            path = value.removesuffix("_lh.mesh") + ".mesh"
            if not (SOURCE / "meshes" / Path(path).name).is_file():
                raise ValueError(f"Missing stock grenade mesh {path}")
            return path
        if value.startswith("base\\vrport\\belt\\") or value.startswith("vrp_belt_lh_&"):
            return value.replace("_lh", "_cf_lh")
    return value


def build(cli):
    stage = ROOT / "build" / ("crossbody-belt-" + uuid.uuid4().hex)
    pack = stage / "vrport_cf_belt"
    output = pack / "base/vrport/belt"
    output.mkdir(parents=True)
    files = ["belt/vrp_belt_lh.ent", "vrp_props.csv"]
    for body in ("m", "w"):
        files.extend([f"belt/vrp_belt_{body}_lh.app", f"belt/vrp_belt_part_{body}_lh.ent"])
    for name in files:
        src = SOURCE / name
        subprocess.run([cli, "convert", "serialize", src, "-o", stage], check=True)
        doc = json.loads((stage / (src.name + ".json")).read_text(encoding="utf-8-sig"))
        if name == "vrp_props.csv":
            table = doc["Data"]["RootChunk"]
            row = next(row for row in table["compiledData"] if row[0] == "vrp_belt_lh")
            table["compiledData"] = [[STEM, crossbody(row[1]), *row[2:]]]
            target = output.parent / "cf_belt_factory.csv"
        else:
            doc["Data"] = crossbody(doc["Data"])
            target = output / src.name.replace("_lh", "_cf_lh")
        data = stage / (target.name + ".json")
        data.write_text(json.dumps(doc), encoding="utf-8")
        subprocess.run([cli, "convert", "deserialize", data, "-o", target.parent], check=True)
        if not target.is_file():
            raise RuntimeError(f"Missing cooked resource {target}")
    subprocess.run([cli, "pack", pack, "-o", stage], check=True)
    shutil.copyfile(stage / "vrport_cf_belt.archive", ROOT / "mods/archive/vrport_cf_belt.archive")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("cli", type=Path)
    build(parser.parse_args().cli.resolve())
