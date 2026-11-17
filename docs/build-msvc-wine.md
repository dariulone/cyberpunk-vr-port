# Building the plugin on Linux with MSVC under Wine (2026-11-17)

## What it is

`cyberpunkvrport.dll` built from a Linux machine by the real MSVC toolchain, run under Wine through
[msvc-wine](https://github.com/mstorsjo/msvc-wine), with CMake and Ninja on the Linux side. It is the same compiler
as a Windows build, so the MSVC ABI that RED4ext.SDK and the game's classes need is kept -- the reason this route is
taken over clang-cl or MinGW (MinGW's Itanium ABI does not match the game's classes, and the sources use `__try`).

## One-time setup

```
sudo pacman -S --needed wine msitools python ninja cmake samba
git clone https://github.com/mstorsjo/msvc-wine ~/src/msvc-wine
cd ~/src/msvc-wine && ./vsdownload.py --accept-license --dest ~/msvc && ./install.sh ~/msvc
rustup target add x86_64-pc-windows-msvc
git submodule update --init --recursive
```

`~/msvc` is the default location; `MSVC_WINE_ROOT` points the toolchain file elsewhere. The download is a few GB
and accepts Microsoft's licence for the Build Tools and the Windows SDK.

**dxc.** msvc-wine fetches only part of the Windows SDK and `dxc.exe` is not in it. Unpack the DirectXShaderCompiler
release zip (github.com/microsoft/DirectXShaderCompiler/releases, `dxc_<date>.zip`) so that `~/dxc/bin/x64/dxc.exe`
exists, with its `dxil.dll` beside it. The zip's paths use backslashes, which `unzip` mishandles; `bsdtar -xf` takes
them. Arch's `directx-shader-compiler` package is no substitute: it has no `libdxil.so`, so its DXIL goes out
unsigned and D3D12 refuses it. `-DCVR_DXC_EXECUTABLE=<path>` picks a particular `dxc.exe` (for instance the version a
Windows build uses, for identical bytecode).

## Building

```
cmake -S . -B build-wine -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_TOOLCHAIN_FILE=$PWD/cmake/msvc-wine.cmake
cmake --build build-wine --target cyberpunkvrport_stereo
```

`build-wine/` is ignored by git. The toolchain file path must be absolute: it is handed on to the FidelityFX build.

## Where it stands

The whole plugin builds: `build-wine/bin/red4ext/plugins/cyberpunkvrport/cyberpunkvrport.dll` (PE32+ x86-64 DLL,
exporting RED4ext's `Main`, `Query` and `Supports`), its PDB, and `redscript/scc_lib.dll` beside it. The
world-marker shaders are signed (a non-zero digest after the `DXBC` magic of each `.dxil` under
`build-wine/generated`). dumpbin lists `Supports` under a `std::_Ref_count_base` name: both return 0, and
`/OPT:ICF` folds identical functions, as it does in a Windows build.

The FidelityFX shader permutations are the part that can go wrong without an error: a generated header such as
`build-wine/fidelityfx-native-build/src/backends/shaders/dx12/ffx_opticalflow_compute_luminance_pyramid_pass_permutations.h`
must have a field in its `PermutationKey`, not an empty struct.

A checkout made while the RED4ext.SDK submodule still pointed at the fork (`marklove5102/RED4ext.SDK`) needs
`git submodule sync externals/RED4ext.SDK` and `git submodule update --init externals/RED4ext.SDK`.

## In game

Installed into game 2.5 with RED4ext 1.31.1 and redscript 0.5.32 under Proton-CachyOS 11.0 and SteamVR
(2026-11-17):

- Built against RED4ext.SDK `a34f0ad0`, it played the whole Nomad prologue up to the first rescue mission.
- Built against `26cf8670`, the SDK the submodule now pins, it loaded, played and shut down cleanly.
- The port's frame generation is unusable on this setup, and the 0.2.45 release DLL fails the same way. With
  FidelityFX frame generation on, the overlay showed about 8 real fps, 0 generated frames, and about 120 ms of CPU
  and GPU time per frame, against about 1 ms for the generation itself; DLSS-G behaved the same. This is not caused
  by the Wine build: it looks like a synchronisation stall in the frame-generation path under Proton (VKD3D-Proton),
  and is not yet investigated.
- Under Wine the desktop mirror calls `SetWindowPos` on almost every present (`DesktopMirror: client placed` floods
  the log), because the window manager never gives the window the exact geometry asked for. Turning the mirror
  off did not change the frame-generation result.
- Not exercised: the bundled `scc_lib.dll`, which only compiles the scripts when redscript is not installed.

## What is not yet known

- **Shader bytecode.** The world-marker shaders come from the DXC release, not the Windows SDK's `dxc`: same source,
  newer compiler, slightly different bytecode. They are signed (see above).
- **Compiler version.** msvc-wine fetched MSVC 19.52, which need not match a Windows machine's.
- **Incremental rebuilds.** FidelityFX_SC may write its gcc-style depfiles with `Z:` paths, so Ninja can rebuild
  those shaders more often than needed. Costs time only.
