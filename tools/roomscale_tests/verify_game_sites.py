"""Read-only validation of the compiled hook sites against the installed PE.

The sites come from the headers the plugin is built with (include/Hooks/RoomscaleSites.hpp, src/Hooks/BodyHeading.cpp),
so the check follows the game patch the build targets instead of one executable's hash. 2026-11-07: the old version
asserted the 2.31 executable's SHA-256 first and failed on every 2.5 install before checking a single site; the
literal 2.31 addresses it also held run only on that executable now."""
from pathlib import Path
import hashlib
import re
import struct
import sys

root = Path(__file__).resolve().parents[2]
data = Path(sys.argv[1]).read_bytes()
digest = hashlib.sha256(data).hexdigest()
GAME_231 = "a7de82945c03e041fc7339fcf9066224d98db2f5d80fea50f7947bb350a60991"
pe = struct.unpack_from("<I", data, 0x3c)[0]
assert data[pe:pe+4] == b"PE\0\0"
count = struct.unpack_from("<H", data, pe+6)[0]
opt_size = struct.unpack_from("<H", data, pe+20)[0]
imagebase = struct.unpack_from("<Q", data, pe+24+24)[0]
headers = [data[pe+24+opt_size+i*40:pe+24+opt_size+i*40+40] for i in range(count)]
sections = [struct.unpack_from("<IIII", h, 8) for h in headers]
executable = [(va, vsize) for h, (vsize, va, raw_size, raw) in zip(headers, sections)
              if struct.unpack_from("<I", h, 36)[0] & 0x20000000]


def read_rva(rva, size):
    for vsize, va, raw_size, raw in sections:
        if va <= rva and rva + size <= va + raw_size:
            return data[raw + rva - va:raw + rva - va + size]
    raise AssertionError(f"unmapped RVA {rva:#x}")


def in_code(rva):
    return any(va <= rva < va + vsize for va, vsize in executable)


header = (root / "include/Hooks/RoomscaleSites.hpp").read_text()
sites = re.findall(r'Site\s+(\w+)\{(0x[0-9A-Fa-f]+),\s*"([0-9A-Fa-f]+)"\}', header)
assert len(sites) == 6
site = {}
for name, rva, code in sites:
    expected_bytes = bytes.fromhex(code)
    assert read_rva(int(rva, 16), len(expected_bytes)) == expected_bytes, name
    site[name] = int(rva, 16)
    print(f"PASS {name}: RVA {rva}, {len(expected_bytes)} original bytes")

consts = {n: int(v, 16) for n, v in re.findall(r'constexpr\s+\w+\s+(\w+)\s*=\s*(0x[0-9A-Fa-f]+)', header)}
# the solver's and the two swimming readers' velocity calls return to the hooked return sites, and all three call the
# hooked velocity getter -- the solver-only subtraction depends on exactly that
for ret_name in ("SolverVelocityReturn", "SwimmingDivingVelocityReturn", "SwimmingSurfaceVelocityReturn"):
    ret = consts[ret_name]
    call = read_rva(ret - 5, 5)
    assert call[0] == 0xE8 and ret + struct.unpack_from("<i", call, 1)[0] == site["Velocity"], ret_name
print("PASS solver and swimming velocity calls return to the hooked sites and call the hooked getter")
for vt in ("PhysicalProviderVtable", "PhysicalAdapterVtable", "CctBackendVtable"):
    slots = [struct.unpack("<Q", read_rva(consts[vt] + 8 * i, 8))[0] - imagebase for i in range(4)]
    assert all(in_code(s) for s in slots), vt
print("PASS provider, adapter and controller vtables point into code")

body_source = (root / 'src/Hooks/BodyHeading.cpp').read_text()
body_sites = re.findall(r'Matches\((0x[0-9A-Fa-f]+),"([0-9A-Fa-f]+)"\)', body_source)
assert len(body_sites) == 2
for rva, code in body_sites:
    original = bytes.fromhex(code)
    assert read_rva(int(rva, 16), len(original)) == original, 'native body source/commit observer signature'
print('PASS body source / commit observer signatures')

if digest == GAME_231:
    # the 2.31 executable's own literals, kept for that build
    assert read_rva(0x4c6c9c, 15) == bytes.fromhex('488B41088B4914488D0C4948C1E104'), 'native swimming action reader'
    call = read_rva(0x6abbaf, 5)
    assert call[0] == 0xe8 and 0x6abbb4 + struct.unpack_from("<i", call, 1)[0] == 0x20479ec
    for address, expected_function in ((0x2b5d3c0, 0x8e869c), (0x2b5d3c8, 0x4c2018),
                                       (0x2ac9bb0, 0x926020), (0x2b5d490, 0x57b714), (0x2b5d3d0, 0xd455b0)):
        assert struct.unpack("<Q", read_rva(address, 8))[0] == imagebase + expected_function
    print("PASS 2.31 literal dispatch slots")
else:
    print("executable %s... is not 2.31: the header sites above are the check" % digest[:16])
