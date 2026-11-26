# Vendored crates of the shader builder

## naga 30.0.2 (`naga/`)

The crates.io release (MIT OR Apache-2.0, licences beside it), patched in one place and pruned of the front and back
ends the workspace does not enable (`spv-in`, `hlsl-out`, and `wgsl-out` from spirv-builder: `src/back/{dot,glsl,msl,spv}`,
`src/front/{glsl,wgsl}` removed; `Cargo.toml.orig`, `.cargo_vcs_info.json`, `CHANGELOG.md`, `Cargo.lock` dropped).
The workspace takes it through `[patch.crates-io]`.

The patch, marked `CVR PATCH` in `src/front/spv/image.rs` (`BlockContext::get_image_expr_ty`): an element of a binding
array of images (`OpLoad` of an `OpAccessChain` into `Texture2D<uint> inputs[512]`, rust-gpu's `&[Image; N]`) has the
array's base type. naga's sampling path unwrapped `BindingArray` itself; the fetch / read / write / query paths matched
the array's type against `Image` and failed with `InvalidImage` (the same in naga's trunk as of 2026-11-23). Used by
`finer_vrs::reduce` (512 variable-rate images read by index).

To move to a newer naga: take the new release's sources, prune them the same way, carry the patch over if the reader
still needs it, and check `cargo xtask shaders --check` (every DXIL byte for byte, or names only by `dxil_semdiff`).
