"""Compile the production lookup functions against real Windows memory protections, without the game."""

from pathlib import Path
import os
import subprocess


def extract_function(path: Path, signature: str) -> str:
    """Copy a complete production function so the smoke test exercises the actual guards."""
    source = path.read_text(encoding="utf-8-sig")
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 1
    cursor = opening + 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


def main() -> None:
    """Build and run isolated low/high-address, wrong-type and inaccessible-memory cases with MSVC."""
    root = Path(__file__).resolve().parents[2]
    output = root / "build" / "low-address-lookup-test"
    output.mkdir(parents=True, exist_ok=True)
    lookup = root / "src/Natives/AnimLookup.cpp"
    functions = "\n".join([
        extract_function(root / "src/Anim/HandSnapshot.cpp", "bool VRIK_IsReadable("),
        extract_function(lookup, "static bool IsValidAnimationSystemPtr("),
        extract_function(lookup, "static RED4ext::world::AnimationSystem* TryKnownAnimationSystemChain("),
    ])
    source = r'''
#include <windows.h>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
// Match CName's value comparison at the RTTI boundary.
struct Name {
    const char* value;
    bool operator==(const char* other) const { return std::strcmp(value, other) == 0; }
};
struct Type { Name name; };
namespace RED4ext::world {
struct RuntimeScene { uint64_t padding, parent; };
struct AnimationSystem { Type* type; };
}
// RTTI is the mocked external boundary; pointer reads and VirtualQuery guards are production code.
Type* SafeGetObjectType(void* p) { return static_cast<RED4ext::world::AnimationSystem*>(p)->type; }
uint64_t SafeReadQword(uint8_t* base, size_t offset) { return *reinterpret_cast<uint64_t*>(base + offset); }
'''
    source += functions
    source += r'''
// A failed assertion must fail Release builds too.
void Check(bool ok, const char* label) { if (!ok) { std::fprintf(stderr, "FAIL: %s\n", label); std::exit(1); } }
// Fixed allocations reproduce the reporter's low Windows addresses and the former accepted high range.
void Run(uintptr_t address) {
    auto* memory = static_cast<uint8_t*>(VirtualAlloc(reinterpret_cast<void*>(address), 0x3000,
        MEM_RESERVE | MEM_COMMIT, PAGE_READWRITE));
    Check(memory != nullptr, "fixed test allocation");
    auto* scene = reinterpret_cast<RED4ext::world::RuntimeScene*>(memory);
    auto* parent = memory + 0x1000;
    auto* system = reinterpret_cast<RED4ext::world::AnimationSystem*>(memory + 0x2000);
    Type correct{{"worldAnimationSystem"}}, wrong{{"anotherSystem"}};
    scene->parent = reinterpret_cast<uintptr_t>(parent);
    *reinterpret_cast<uintptr_t*>(parent + 0x38) = reinterpret_cast<uintptr_t>(system);
    system->type = &correct;
    Check(TryKnownAnimationSystemChain(scene) == system, "valid chain");
    system->type = &wrong;
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "wrong RTTI type");
    system->type = &correct;
    DWORD previous;
    Check(VirtualProtect(system, 0x1000, PAGE_NOACCESS, &previous) != 0, "protect candidate");
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "unreadable candidate");
    Check(VirtualProtect(system, 0x1000, PAGE_READWRITE, &previous) != 0, "restore candidate");
    Check(VirtualProtect(parent, 0x1000, PAGE_READWRITE | PAGE_GUARD, &previous) != 0, "guard parent");
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "guarded parent");
    Check(VirtualProtect(parent, 0x1000, PAGE_READWRITE, &previous) != 0, "restore parent");
    *reinterpret_cast<uintptr_t*>(parent + 0x38) = 0;
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "null candidate");
    scene->parent = 1;
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "invalid parent");
    Check(VirtualProtect(scene, 0x1000, PAGE_NOACCESS, &previous) != 0, "protect scene");
    Check(TryKnownAnimationSystemChain(scene) == nullptr, "unreadable scene");
    Check(VirtualFree(memory, 0, MEM_RELEASE) != 0, "release test allocation");
}
int main() {
    Check(TryKnownAnimationSystemChain(nullptr) == nullptr, "null scene");
    Run(0x20000000ull);
    Run(0x200000000ull);
    Run(0x20000000000ull);
    std::puts("PASS: low/high Windows lookup, wrong RTTI, null/invalid/guarded/inaccessible pointers");
}
'''
    (output / "lookup_test.cpp").write_text(source, encoding="utf-8")
    vswhere = Path(os.environ["ProgramFiles(x86)"]) / "Microsoft Visual Studio/Installer/vswhere.exe"
    installation = subprocess.check_output([
        str(vswhere), "-latest", "-products", "*", "-requires",
        "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath",
    ], text=True).strip()
    if not installation:
        raise RuntimeError("MSVC x64 build tools are required")
    vcvars = Path(installation) / "VC/Auxiliary/Build/vcvars64.bat"
    batch = output / "build_test.cmd"
    batch.write_text(
        f'@echo off\ncall "{vcvars}" >nul\nif errorlevel 1 exit /b 1\n'
        'cl /nologo /std:c++20 /EHsc /W4 /WX lookup_test.cpp /Fe:lookup_test.exe\n', encoding="utf-8",
    )
    subprocess.run(["cmd.exe", "/c", str(batch)], cwd=output, check=True)
    subprocess.run([str(output / "lookup_test.exe")], cwd=output, check=True)


if __name__ == "__main__":
    main()
