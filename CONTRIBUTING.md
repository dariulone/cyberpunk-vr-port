# Contributing

## Repository Map

Active runtime development lives in the `rust/` Cargo workspace (`rust/crates/*`, with shaders in `rust/shaders/`). Top-level `src/`, `include/`, and `mods/` hold C++ boundary code and legacy sources from the ongoing Rust migration—check `CMakeLists.txt` and `cmake/` to confirm whether a non-Rust file is compiled into the shipping plugin.

Before starting larger changes, check recent `main` history and related issues or PRs, as module boundaries move quickly.

## Build, Package & Deploy

Plugin builds, pinned `RED4ext.SDK` patching, shader compilation, and release packaging are driven by `xtask` inside `rust/` (using the toolchain pinned in `rust/rust-toolchain.toml`):

```powershell
cd rust
cargo xtask          # lists current build, dist, sdk, and shaders tasks & flags
cargo xtask build    # prepares the patched SDK, configures CMake, and builds the plugin
```

Deployment helpers live in `scripts/` (`deploy_full.ps1` for `cargo xtask dist` packages, `deploy_plugin.ps1` for DLL-only iteration—check each script's `param(...)` header for current options).

## Testing & Performance

- **Tests**: `cargo test` inside `rust/`, plus standalone CTest and simulator suites under `tools/`.
- **Performance**: When working on optimizations, validate the bottleneck before and after using actual GPU or pass work measurements rather than frame-paced intervals alone (see `tools/vr_bench/README.md`).
