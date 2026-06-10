---
name: cp2077-perview-camera-layout
description: CP2077 per-view camera object layout (quat/fov/nearfar/view/projection) + RT-not-here finding
metadata:
  type: project
---

CP2077 option-C (true stereo) RE, 2026-06-07. The render-view task `sub_140D69018(a1,a2)` is a
2-line thunk: `return sub_140126D4C(*a1, *(a2+8))`. `*a1` = per-view camera object = `*(taskElem+0x08)`,
reached at runtime via the TaskDispatch TLS (set by the sub_140243980 hook) read inside the locate hook.

The object is SHARED by address across views but REWRITTEN per-view right before each view's locate, so a
main-vs-mirror byte diff (captured at each view's locate) reveals the per-view camera fields. Layout:

- +0x70: orientation quaternion
- +0x80: FOV (95.0), aspect (~2.02)
- +0xA0: near (0.02), far (16000)
- +0xB0..0xD0: view matrix 3x3 (rotation); +0xE0: camera world position
- +0xF0..0x120: transposed copy; +0x130..0x1A0: second matrix (prev-frame/secondary)
- +0x1B0..0x1E0: projection matrix (reversed-Z)
- obj vtable RVA ~0x2B72EE8

=> Right-eye camera injection is fully mapped (shift +0xE0 along camera-right by IPD/2, keep proj).

Also hooked sub_140126D4C entry (rva 0x126D4C, prologue 48 89 5C 24 08 / 57 / 48 83) to capture its rdx =
per-view spec = *(renderViewTask.a2+8). The spec is a STACK temp (rdx points to stack), full of return
addrs + ptrs. Derefed its heap pointers (present-thread, NOT in the hot worker hook — doing it there
stalls save-load => crash): spec carries an asset descriptor (vtRva 0x2C6F9C0), a resource-handle array
(s+0x48/50/58, no vtable), a bounds/frustum obj (vtRva 0x2C6F588), a context obj (vtRva 0x2AC8550).
DECISIVE: these spec heap pointers are BYTE-IDENTICAL between main view and mirror — only camera floats
differ. So the RT is NOT among them (if it were, main vs mirror would differ).

KEY M4 finding (now proven across THREE layers — camera object, spec, spec's pointer targets): the
per-view RENDER TARGET is NOT reachable from any view-level structure. RT routing is downstream inside
FrameGraph execution (the executor [[cp2077-view-family-iterator]] chain:
job -> render-root sub_1401EC46D -> recursive node sub_1401F4315 -> ... -> matrix-builder sub_140788A9C
which writes the single render-view slot renderer+0x4658). RT binding + barrier-tracker (sub_1401F41F8)
re-entrancy are the remaining make-or-break for option C path A (FrameGraph reverse). Chosen path: A
(only A allows sharing view-independent passes = the only real optimization win over plain double-render).
