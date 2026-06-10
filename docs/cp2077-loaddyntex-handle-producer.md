---
name: cp2077-loaddyntex-handle-producer
description: handle producer LOCATED = component slot-12 UPDATE vmethod drives LoadDynamicTexture chain (resref->handle); render-thread/job-state gated; corrected component vtable addr
metadata:
  type: project
---

Located the exact resref->handle producer for entRenderToTextureCameraComponent (the linchpin sought across prior sessions).

CORRECTION: component vtable = exe+0x307BFD0 (abs 0x14307BFD0). Earlier "0x143007BFD0" read was a typo (extra digit -> out of range -> 0xFF). RTTI name string "entRenderToTextureCameraComponent" @0x142DE2C10; profiling string "RenderToTextureCameraComponent/LoadDynamicTexture" @0x14307BF08 sits right before the vtable.

Component vmethod map (vtable 0x14307BFD0), lifecycle cluster in region 0x1423E7xxx:
- slot 12 (off 0x60) = sub_1423E759C = per-frame UPDATE. Drives the handle producer. (virtual; also in vtables 0x142B03B70, 0x144B7D02C)
- slot 11 (off 0x58) = sub_1423E741C = property-setter (matches prop GUID hashes 0xAA2EDC954BA204FF etc -> writes comp+0x9F6/9F7/9F8). NOT onAttach.
- slot 17 (off 0x88) = sub_1423E72EC = magic-static scoped op.
- slot 7 (off 0x38) = sub_1423E7518.
onAttach not yet positively identified (slot-guessing abandoned).

LoadDynamicTexture chain (the handle producer):
sub_1423E759C (update) -> sub_1423E7768 (worker, ~0x101) -> sub_141C9F5B0 (wrapper: opens "...LoadDynamicTexture" profile scope; reads gs:[0x58] TLS job-state in prologue) -> payload sub_141C9F660 -> sub_14095EC88 (texmgr region 0x95Exxx) -> sub_14014299C. Wrapper tail-jumps sub_1401A1B10.

DECISIVE consequence: the GPU-backed handle (comp+0x1E8) is produced ONLY when the engine runs this component's slot-12 UPDATE on the render thread (gs:[0x58] gated). External calls deadlock (consistent with all prior). So getting the handle = getting the engine to TICK our component's update, which requires the component enrolled/attached on a render-active entity.

Static analysis is now EXHAUSTED for the remaining question: whether attaching our right-eye entRenderToTextureCameraComponent to the live render-active player entity makes the engine tick its update (-> LoadDynamicTexture -> handle). That is entity-system runtime behavior, only answerable empirically. But the experiment is now CLEAN + instrumented: success signal = comp+0x1E8 becomes non-null after attach (read via rpm.ps1). This supersedes 35 sessions of blind handle-allocation attempts. Links: [[cp2077-attach-is-render-thread-virtual]] [[cp2077-rtt-view-create-fn]] [[cp2077-isenabled-necessary-not-sufficient]] [[cp2077-rttcam-census-no-player-cam]].
