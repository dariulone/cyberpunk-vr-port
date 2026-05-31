---
name: build-cyberpunkvrport
description: How to build the CyberpunkVRPort dxgi.dll proxy
metadata: 
  node_type: memory
  type: reference
  originSessionId: 0353ad85-8636-45bd-8b7d-635741e848f9
---

CMake (VS 2022) project. `cmake`/`msbuild` are NOT on PATH — use the VS-bundled cmake.
Configured build dir is `build-vs/` (generator: Visual Studio 17 2022). `build/` is empty.

Build the proxy (PowerShell):
```
$cmake = "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe"
& $cmake --build "C:\Users\dariulone\Desktop\CyberpunkVRPort\build-vs" --config Release --target dxgi
```
Output: `build-vs\bin\Release\dxgi.dll`. Deploy by copying it to
`C:\Program Files (x86)\Steam\steamapps\common\Cyberpunk 2077\bin\x64\dxgi.dll`.

MSBuild (if needed): `C:\Program Files\Microsoft Visual Studio\2022\Community\Msbuild\Current\Bin\MSBuild.exe`.
Pre-existing harmless warnings: C4100 (unreferenced params), C4505, C4702 — not from new code.
Targets: `dxgi` (the mod), `d3d12`, `imgui`. Runtime log: `CyberpunkVRProxy.log` in the game dir.
See [[project-cyberpunkvrport]].
