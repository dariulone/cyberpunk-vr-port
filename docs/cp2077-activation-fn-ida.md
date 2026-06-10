---
name: cp2077-activation-fn-ida
description: CP2077 RTT activation reversed via IDA — activation fn = sub_1404A04AC (virtual); factory's "GPU allocator" was actually memset
metadata:
  type: project
---

IDA Pro 9.2 (unpacked .idb on disk, no RTTI symbols — all `sub_`) probed via `tools\ida_probe.py` → `tools\ida_out.txt`.

**Activation function = `sub_1404A04AC`** (ida=0x1404A04AC). Contains the factory call site (ida=0x141E8ECF7) that creates the RT handle. It is a **virtual method** (data-ref'd from vtables at 0x143218B78 and 0x14496C1EC) AND directly near-called **twice** by `sub_14049F21C` — the closest activation trigger to reverse next.

**CORRECTION (kills [[cp2077-gpu-backing-allocator-found]] claim):** the factory `sub_14221BF64` call at ida=0x141908820 is **`memset`**, not a GPU-backing allocator. Factory = alloc 0xA8 bytes → memset 0 → ctor `sub_1412A3CC0` → wrap in `Ref<>` (`sub_1401456C0`). So the factory does NOT GPU-back the handle; GPU backing still comes downstream on the generate/render pass — confirms the long-standing "generator gap".

Render-proxy class vtable = `exe+0x3009130` (holds RT in proxy+0x268). See [[cp2077-rttcam-attached-vs-not-live]] for the live attach discriminator (comp+0x1E8 handle != 0, comp+0x88 flag bit 0x01000000).

Next: decompile `sub_1404A04AC` + `sub_14049F21C` (Hex-Rays returned None first pass; re-run via `tools\ida_probe2.py` with failure diagnostics) to find what condition triggers activation and whether it's reproducible for our script camera.
