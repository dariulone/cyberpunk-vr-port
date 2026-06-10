---
name: cp2077-rtt-proxy-is-registered-type
description: RTT render-proxy (vtable 0x3009130) is a lazily-constructed REGISTERED render type, not a hand-buildable object; attach trigger is render-scene attach, not a callable ctor
metadata:
  type: project
---

Reverse of the RTT render-proxy construction (via IDA, safe read_memory_bytes — `disassemble_function`/`decompile_function` CRASH IDA on hostile functions, use raw-byte decode instead).

**Proxy class (vtable `exe+0x3009130`) construction chain:**
- `sub_14020E734` (0x3c) = proxy base-init: writes vtable `0x3009130` into 6 sub-vtable slots `+0x00/+0x10/.../+0x50` (multi-inheritance object; matches live layout where vtable repeats on +0x00..+0x40).
- `sub_14020E148` (0x3c) = type registration: takes `&sub_14020E734` as ctor fn ptr, size `0x60`, type-id `3`, calls a render-type-descriptor register helper. So the proxy is a **registered render type, constructed lazily by a factory** — NOT something to `new` by hand.

**Consequence (confirms [[cp2077-live-rpm-and-attach-pivot]] conclusion B):**
- There is no simple "create proxy" ctor to call. The proxy is created by the engine's render-scene **attach** path when a component is enrolled into the active render scene.
- RT handle is then built lazily by activation `sub_1404A04AC` (virtual render-graph-node method, see [[cp2077-activation-fn-ida]]) when the node is **visited during active render traversal** — reads node fields `+0x198/+0x19C` (W/H) × `+0x238/+0x23C` (scale) to size the RT, then calls factory `sub_14221BF64`.

**Real linchpin for the project:** make our script-added `entRenderToTextureCameraComponent` go through render-scene attach so the engine creates its proxy + enrolls its node. Component vtable = `exe+0x307BFD0`; one of its vmethods is the render-attach/create-proxy hook the render-scene invokes. This is the [[cp2077-rttcam-attached-vs-not-live]] render-attach lifecycle, which script `AddComponent` skips. Matches the no-clean-API warning in [[cp2077-rtt-no-drivable-api]].
