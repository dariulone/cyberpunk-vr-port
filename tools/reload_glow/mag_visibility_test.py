"""Validate actual Lua magazine config indices against the compiled production mapper.

Run with Python 3.14 and lupa, passing mag_visibility_test.exe. --configs selects an old source folder.
"""
import argparse
from pathlib import Path
import subprocess
import unittest
from lupa.luajit21 import LuaRuntime

ROOT = Path(__file__).resolve().parents[2]
CONFIGS = ROOT / "mods/cet/CyberpunkVRPort_Reload/reload/weapons"
WEAPONS = ("darra_nova", "darra_quasar", "krausser_grit", "militech_crusher",
           "techtronika_burya", "techtronika_metel", "techtronika_pozhar")
EXE = None


class MagazineVisibilityTests(unittest.TestCase):
    def test_config_tracks_hide_real_seated_magazine(self):
        lua = LuaRuntime()
        for name in WEAPONS:
            with self.subTest(weapon=name):
                config = lua.execute((CONFIGS / (name + ".lua")).read_text(encoding="utf-8-sig"))
                self.assertEqual(config.mag.which, 0)
                result = subprocess.run([str(EXE), str(int(config.mag.showTrack))], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_regression_destination_index_keeps_magazine_in_gun(self):
        result = subprocess.run([str(EXE), "1"], capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn("leaves the seated magazine visible", result.stderr)

    def test_glow_copies_have_no_original_opaque_materials(self):
        import json
        folder = ROOT / "mods/archive/source/raw/vrport/reload_glow"
        paths = list(folder.glob("*.mesh.json"))
        self.assertGreater(len(paths), 40)
        for path in paths:
            with self.subTest(mesh=path.name):
                root = json.loads(path.read_text(encoding="utf-8-sig"))["Data"]["RootChunk"]
                self.assertEqual(root["localMaterialBuffer"]["materials"], [])
                self.assertFalse(root["localMaterialInstances"])
                materials = {entry["DepotPath"]["$value"] for entry in root["externalMaterials"]}
                self.assertEqual(materials, {r"vrport\reload_glow\glow_pulse.mi", r"vrport\reload_glow\glow_steady.mi"})
                for appearance in root["appearances"]:
                    self.assertTrue(all(value["$value"] in ("glow_pulse", "glow_steady")
                                        for value in appearance["Data"]["chunkMaterials"]))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("mapper", type=Path)
    parser.add_argument("--configs", type=Path)
    args, remaining = parser.parse_known_args()
    EXE = args.mapper.resolve()
    if args.configs:
        CONFIGS = args.configs.resolve()
    unittest.main(argv=[__file__, *remaining])
