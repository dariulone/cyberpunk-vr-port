# mirror_re — Apartment-Mirror Render Path, Full Reverse (build 2.31, IDA base 0x140000000)

**Generated 2026-06-28.** Complete decompile/disasm of the entire apartment-mirror render path, end to end.
Parts 1–4 below contain the per-function dumps; this header is the executive map + the decisive conclusion.

## ★ DECISIVE CONCLUSION — the mirror has NO settable "camera"; it is `reflect(main camera, mirror plane)`

The apartment mirror is **`worldMirrorNode`** = a **scene node placed in the world with a transform (a plane)**.
Its render is a **planar reflection of the MAIN camera across that plane** — computed every frame, transient.
There is **no independent camera object you can set** for the mirror:

- The "mirror camera" we kept finding in memory (orthonormal, **det = −1**, pos = reflected player) is the
  **OUTPUT** of the reflection, not an input. det = −1 because a planar reflection flips handedness.
- The reflection MATH is **`sub_141D54EE0`** (simd≈227, 33 negate ops, size 0x146C): it reads the renderer
  global `qword_143427C00` (`[+0x4638]` view slot) AND the camera object (`[r14+0x20]` → `+0x70/0x74/0x78`
  = camera orientation, the known camera layout) and builds the mirror's view/projection matrices. Reflecting
  the main camera across the mirror plane is exactly an `I − 2·n·nᵀ` plane-reflection (the 33 negate/sub ops).
- The per-frame **locate hook is a SINGLE main/player camera (~1×/frame)** — confirmed by
  `docs/redengine-job-render-arch.md` (195/200 hit ONE descriptor) and live this session. Reflections use a
  DIFFERENT path → the mirror camera never passes through the locate hook → you cannot steer the mirror by the
  player-camera hook (writing its pos there only TELEPORTS the player; pos must stay at head-center for IK).

**So: the controllable input to the mirror is the PLANE (worldMirrorNode transform) + the main camera — never a
free camera. A reflection is always handedness-flipped (det −1), so it is NOT directly a stereo eye view (which
needs main+IPD, det +1). It could only become a usable view by 2D-flipping the captured RT.**

## Full chain (top → bottom), with RVAs

```
SCENE / GAMEPLAY LAYER
  worldMirrorNode / worldMirrorNodeInstance   (RTTI registrars sub_1416EA3A0 / sub_1416EA50C; vtable 0x142DD9B08)
    → stores the mirror's transform/plane (scene placement); m_mirrorType (GetMirrorType sub_1413AEFC0)
  RenderProxyCustomData_Mirror                (registrar sub_1415F3888; vtable off_142D21AA8) = render-side data
  GATING: m_isInMirrorsArea / questEntityManagerToggleMirrorsArea (sub_14162B134) — apartment/quest ONLY

RENDER LAYER  (one FlushRenderScene job/frame → dispatcher iterates the render-graph)
  work-fn sub_140AA3904 ( *a1 = view-item )
    → dispatcher sub_140219730   (branches on stage-type *(view-state+0x334); mirror branch ≠ 55)
        → MIRROR deferred pass-set BUILD  sub_141D43040  (0x456F, 0 SIMD = PURE registration:
             registers depthprepass_mirror_opaque_notxaa + gbuffer + lighting passes, key=r14d via GBuffer name)
        → planar view builder  sub_14078A6B8  ("renderstage_planar_reflection" ×2; 9 arena slots via sub_1401F0F80)
  REFLECT MATH  sub_141D54EE0  (simd≈227)  ← reads renderer global (main cam) + camera orient → builds reflected
        view/proj matrices.  Caller chain: sub_141D53FA0 → sub_141D56900 (TLS+410 gate) → sub_141D54EE0
  view-const builder sub_140789724 (reads cam [a2+0x18], FOV, tanf→proj)  — generic, shared with main view
  camera writer sub_140788A9C  → writes the per-view camera matrices into the SINGLE shared view-state @ renderer+0x4658
```

## Why every "use the mirror for the right eye" lever failed (cross-ref, all in this doc + memory)

1. **Overwrite the det−1 camera in memory** → black flicker (we DO reach the render) but it's transient (copied
   per-frame by generic copiers sub_14028DB28 etc.), signature not unique (water/SSR are det<0 too) → crashes.
2. **Hook the camera writer / locate hook** → that's the MAIN player camera (single, ~1/frame); writing pos
   teleports the player. The mirror camera isn't there.
3. **Clone/producer-double a view** → arena-slot collision / mgr tear (docs).
4. The mirror's input is a PLANE, not a camera — there is nothing camera-shaped to hand our eye pose to.

## What this means for VR (honest)

The mirror proves the engine renders a full-lit second view (with TPP body) — but it is structurally a
**plane reflection of the main camera**, gated to mirror-areas, handedness-flipped. It is **not** a vehicle for a
stereo right-eye view. The only way it could yield an eye image is: control the mirror PLANE so the reflection
≈ a forward view, then horizontally-flip the captured RT — but the gating (apartment-only) and the flip make it
impractical vs. the shipping AER path.

---


# ============================================================
# mirror_re — FULL apartment-mirror render-path reverse (build 2.31, IDA base 0x140000000)
# Goal: prove mirror = reflect(main camera, mirror plane) — NOT a settable camera —
# and map every function + the plane/transform storage. Sections by layer.
# ============================================================

# PART 1 — SCENE / GAMEPLAY LAYER (worldMirrorNode, gating, render-proxy registration)

## worldMirrorNode RTTI registrar  `sub_1416EA3A0`  (RVA 0x16EA3A0, size 0xC1, simd=0)
**Callers (2):** data 1416EA398 sub_1416EA398, data 144A85A48 ?
**Callees:** sub_1416EA464 x1, _Init_thread_header x1, sub_142048670 x1, sub_1403CE194 x1, atexit x1, _Init_thread_footer x1
```c
__int64 __fastcall sub_1416EA3A0(__int64 a1)
{
  __int64 v1; // rbx
  _QWORD *v2; // rax
  __int64 v4; // [rsp+30h] [rbp+8h] BYREF

  v4 = a1;
  v1 = sub_1416EA464();
  if ( dword_14443E148 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_14443E148);
    if ( dword_14443E148 == -1 )
    {
      v2 = (_QWORD *)sub_142048670(&v4, "worldMirrorNode");
      sub_1403CE194(&qword_14443E150, *v2, 120);
      qword_14443E150 = (__int64)off_142DD9A00;
      atexit(sub_141DDF9F0);
      Init_thread_footer(&dword_14443E148);
    }
  }
  qword_14342E5B0 = (__int64)&qword_14443E150;
  dword_14443E1C4 = 8;
  return sub_1403CE500(&qword_14443E150, v1);
}
```

## worldMirrorNodeInstance RTTI registrar  `sub_1416EA50C`  (RVA 0x16EA50C, size 0xC3, simd=0)
**Callers (2):** data 1416EA504 sub_1416EA504, data 144A85A60 ?
**Callees:** sub_1416EA5D0 x1, _Init_thread_header x1, sub_142048670 x1, sub_1403CE194 x1, atexit x1, _Init_thread_footer x1
```c
__int64 __fastcall sub_1416EA50C(__int64 a1)
{
  __int64 v1; // rbx
  _QWORD *v2; // rax
  __int64 v4; // [rsp+30h] [rbp+8h] BYREF

  v4 = a1;
  v1 = sub_1416EA5D0();
  if ( dword_14443DE70 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_14443DE70);
    if ( dword_14443DE70 == -1 )
    {
      v2 = (_QWORD *)sub_142048670(&v4, "worldMirrorNodeInstance");
      sub_1403CE194(&qword_14443DE78, *v2, 256);
      qword_14443DE78 = (__int64)off_142DD9B08;
      atexit(sub_141DDF9C0);
      Init_thread_footer(&dword_14443DE70);
    }
  }
  qword_14342E5A8 = (__int64)&qword_14443DE78;
  dword_14443DEEC = 16;
  return sub_1403CE500(&qword_14443DE78, v1);
}
```

## GetMirrorType getter  `sub_1413AEFC0`  (RVA 0x13AEFC0, size 0xB3, simd=0)
**Callers (2):** CALL 1413AEDE6 sub_1413AEBC8, data 144A4D768 ?
**Callees:** sub_14028A65C x1, _Init_thread_header x1, sub_1403CDF14 x1, atexit x1, _Init_thread_footer x1, sub_1401320AC x1
```c
__int64 __fastcall sub_1413AEFC0(_QWORD *a1)
{
  __int64 v2; // rcx
  void *v4; // [rsp+40h] [rbp+8h] BYREF

  sub_14028A65C(&v4, "GetMirrorType");
  if ( dword_144098208 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_144098208);
    if ( dword_144098208 == -1 )
    {
      sub_1403CDF14((unsigned int)&unk_144098210, *a1, (_DWORD)v4, (_DWORD)v4, (__int64)sub_1421BBF9C, 0);
      atexit(sub_141DA63A0);
      Init_thread_footer(&dword_144098208);
    }
  }
  v2 = *a1 + 72LL;
  v4 = &unk_144098210;
  return sub_1401320AC(v2, &v4);
}
```

## mirrorType field/prop registrar (RTTI)  `sub_1413AEBC8`  (RVA 0x13AEBC8, size 0x2A3, simd=0)
**Callers (2):** CALL 1413AEBB4 sub_1413AEBA4, data 144A4D744 ?
**Callees:** sub_14059C0E4 x5, sub_14059BD08 x4, sub_142048670 x4, sub_14059C088 x4, sub_14059C3EC x1, sub_140F4CCB8 x1, sub_14059B854 x1, sub_140B93764 x1, sub_140B93904 x1, sub_1413AF074 x1, sub_1413AEFC0 x1, sub_1413AEF20 x1, sub_14101B7E0 x1, sub_1413AEE6C x1
```c
__int64 __fastcall sub_1413AEBC8(__int64 a1)
{
  __int64 v2; // rcx
  __int64 v3; // rdi
  __int64 v4; // rbx
  _QWORD *v5; // rax
  __int64 v6; // rcx
  __int64 v7; // rdi
  __int64 v8; // rbx
  _QWORD *v9; // rax
  __int64 v10; // rcx
  __int64 v11; // rdi
  __int64 v12; // rbx
  _QWORD *v13; // rax
  __int64 v14; // rcx
  __int64 v15; // rdi
  __int64 v16; // rbx
  _QWORD *v17; // rax
  __int64 v18; // rcx
  __int64 v19; // rcx
  _QWORD v21[2]; // [rsp+40h] [rbp-C0h] BYREF
  char v22; // [rsp+50h] [rbp-B0h]
  int v23; // [rsp+64h] [rbp-9Ch]
  __int64 v24; // [rsp+68h] [rbp-98h]
  __int64 v25; // [rsp+70h] [rbp-90h]
  int v26; // [rsp+78h] [rbp-88h]
  _BYTE v27[64]; // [rsp+80h] [rbp-80h] BYREF
  _BYTE *v28; // [rsp+C0h] [rbp-40h]
  __int64 v29; // [rsp+C8h] [rbp-38h]
  _BYTE v30[40]; // [rsp+D0h] [rbp-30h] BYREF
  __int64 v31; // [rsp+F8h] [rbp-8h]
  _BYTE v32[192]; // [rsp+100h] [rbp+0h] BYREF
  __int64 v33; // [rsp+1E0h] [rbp+E0h] BYREF
  char v34; // [rsp+1E8h] [rbp+E8h] BYREF

  sub_14059C3EC(v30);
  v29 = 64;
  v21[0] = 0;
  v25 = 0x10B000000LL;
  v28 = v27;
  v21[1] = 0;
  v24 = 0;
  v22 = 0;
  v23 = 0;
  v26 = 0;
  v27[0] = 0;
  if ( qword_1433A6E08 )
    *(_QWORD *)(a1 + 16) = qword_1433A6E08;
  sub_14059C0E4(v2, v30);
  v3 = sub_140F4CCB8();
  v4 = *(_QWORD *)sub_14059BD08(v27, &v33);
  v5 = (_QWORD *)sub_142048670(&v34, "textureAtlas");
  sub_14059C088(v30, a1, 64, *v5, v4, v3, 0);
  v31 |= 1uLL;
  sub_14059C0E4(v6, v30);
  v7 = sub_14059B854();
  v8 = *(_QWORD *)sub_14059BD08(v27, &v33);
  v9 = (_QWORD *)sub_142048670(&v34, "texturePartId");
  sub_14059C088(v30, a1, 88, *v9, v8, v7, 0);
  v31 |= 1uLL;
  sub_14059C0E4(v10, v30);
  v11 = sub_140B93764();
  v12 = *(_QWORD *)sub_14059BD08(v27, &v33);
  v13 = (_QWORD *)sub_142048670(&v34, "tileType");
  sub_14059C088(v30, a1, 96, *v13, v12, v11, 0);
  v31 |= 1uLL;
  sub_14059C0E4(v14, v30);
  v15 = sub_140B93904();
  v16 = *(_QWORD *)sub_14059BD08(v27, &v33);
  v17 = (_QWORD *)sub_142048670(&v34, "mirrorType");
  sub_14059C088(v30, a1, 97, *v17, v16, v15, 0);
  v31 |= 1uLL;
  v33 = a1;
  sub_1413AF074(&v33);
  v33 = a1;
  sub_1413AEFC0(&v33);
  v33 = a1;
  sub_1413AEF20(&v33);
  v33 = a1;
  sub_14101B7E0(&v33);
  v33 = a1;
  sub_1413AEE6C(&v33);
  sub_14059C0E4(v18, v30);
  sub_140B376F0(v19, v21);
  sub_1403109DC(v27);
  sub_14041EE64(v21);
  return sub_14059C358(v32);
}
```

## RenderProxyCustomData_Mirror RTTI registrar  `sub_1415F3888`  (RVA 0x15F3888, size 0xC1, simd=0)
**Callers (2):** data 1415F3880 sub_1415F3880, data 144A75608 ?
**Callees:** sub_1415F394C x1, _Init_thread_header x1, sub_142048670 x1, sub_1403CE194 x1, atexit x1, _Init_thread_footer x1
```c
__int64 __fastcall sub_1415F3888(__int64 a1)
{
  __int64 v1; // rbx
  _QWORD *v2; // rax
  __int64 v4; // [rsp+30h] [rbp+8h] BYREF

  v4 = a1;
  v1 = sub_1415F394C();
  if ( dword_14484CB30 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_14484CB30);
    if ( dword_14484CB30 == -1 )
    {
      v2 = (_QWORD *)sub_142048670(&v4, "RenderProxyCustomData_Mirror");
      sub_1403CE194(&qword_14484CB38, *v2, 64);
      qword_14484CB38 = (__int64)off_142D21AA8;
      atexit(sub_141E191A0);
      Init_thread_footer(&dword_14484CB30);
    }
  }
  qword_143438088 = (__int64)&qword_14484CB38;
  dword_14484CBAC = 16;
  return sub_1403CE500(&qword_14484CB38, v1);
}
```

## questEntityManagerToggleMirrorsArea (GATING)  `sub_14162B134`  (RVA 0x162B134, size 0xC1, simd=0)
**Callers (2):** data 14162B12C sub_14162B12C, data 144A790A0 ?
**Callees:** sub_14162B1F8 x1, _Init_thread_header x1, sub_142048670 x1, sub_1403CE194 x1, atexit x1, _Init_thread_footer x1
```c
__int64 __fastcall sub_14162B134(__int64 a1)
{
  __int64 v1; // rbx
  _QWORD *v2; // rax
  __int64 v4; // [rsp+30h] [rbp+8h] BYREF

  v4 = a1;
  v1 = sub_14162B1F8();
  if ( dword_144769898 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_144769898);
    if ( dword_144769898 == -1 )
    {
      v2 = (_QWORD *)sub_142048670(&v4, "questEntityManagerToggleMirrorsArea_NodeType");
      sub_1403CE194(&qword_1447698A0, *v2, 112);
      qword_1447698A0 = (__int64)off_142D4B740;
      atexit(sub_141E0AB30);
      Init_thread_footer(&dword_144769898);
    }
  }
  qword_143435D48 = (__int64)&qword_1447698A0;
  dword_144769914 = 8;
  return sub_1403CE500(&qword_1447698A0, v1);
}
```

## questIsInMirrorsAreaMapArrayElement (GATING)  `sub_141495B00`  (RVA 0x1495B00, size 0xC1, simd=0)
**Callers (2):** data 141495AF8 sub_141495AF8, data 144A5D458 ?
**Callees:** sub_141495C58 x1, _Init_thread_header x1, sub_142048670 x1, sub_1403CE194 x1, atexit x1, _Init_thread_footer x1
```c
__int64 __fastcall sub_141495B00(__int64 a1)
{
  __int64 v1; // rbx
  _QWORD *v2; // rax
  __int64 v4; // [rsp+30h] [rbp+8h] BYREF

  v4 = a1;
  v1 = sub_141495C58();
  if ( dword_1446F6428 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_1446F6428);
    if ( dword_1446F6428 == -1 )
    {
      v2 = (_QWORD *)sub_142048670(&v4, "questIsInMirrorsAreaMapArrayElement");
      sub_1403CE194(&qword_1446F6430, *v2, 16);
      qword_1446F6430 = (__int64)off_142BEB0A8;
      atexit(sub_141E071A0);
      Init_thread_footer(&dword_1446F6428);
    }
  }
  qword_143434960 = (__int64)&qword_1446F6430;
  dword_1446F64A4 = 8;
  return sub_1403CE500(&qword_1446F6430, v1);
}
```

*(part 1 done)*


# PART 2 — RENDER LAYER (work-fn → dispatcher → mirror pass + builders)

## FG-build WORK-FN (per-view job)  `sub_140AA3904`  (RVA 0xAA3904, size 0xB4, simd=0)
**Role:** *a1=view-item; builds ctx; calls dispatcher
**Callers (3):** data 14016800E sub_140167FC8, data 1402935AE sub_140293568, data 1449C1BF0 ?
**Callees:** sub_1401E6EB0 x2, sub_140AA39B8 x1, sub_140219730 x1, sub_140142F88 x1, sub_1402F0DE8 x1
```c
PVOID __fastcall sub_140AA3904(_QWORD *a1, __int64 a2)
{
  __int64 v2; // r9
  PVOID result; // rax
  __int64 v5[2]; // [rsp+20h] [rbp-68h] BYREF
  _BYTE v6[56]; // [rsp+30h] [rbp-58h] BYREF
  __int64 v7; // [rsp+68h] [rbp-20h]

  v2 = *(_QWORD *)(a2 + 32);
  if ( v2 )
    *(_BYTE *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 410LL) = *(_BYTE *)(*(_QWORD *)v2 + 33LL);
  sub_140AA39B8(v6, a2, a1);
  sub_140219730(qword_143427C00, v6);
  if ( v7 )
    sub_1401E6EB0(v7);
  sub_140142F88(v6);
  result = NtCurrentTeb()->ThreadLocalStoragePointer;
  *(_BYTE *)(*(_QWORD *)result + 410LL) = -1;
  if ( a1 )
  {
    if ( *a1 )
      sub_1401E6EB0(*a1);
    v5[1] = 0;
    v5[0] = (__int64)a1;
    return (PVOID)sub_1402F0DE8(v5);
  }
  return result;
}
```

## per-pass DISPATCHER  `sub_140219730`  (RVA 0x219730, size 0xA00, simd=0)
**Role:** branches on stage-type *(viewstate+0x334); mirror=sub_141D43040 when !=55
**Callers (2):** CALL 140AA3952 sub_140AA3904, data 14494B45C ?
**Callees:** sub_14204C110 x5, sub_1407E34B0 x3, sub_14021A130 x3, sub_1401FE460 x2, sub_1428E7088 x2, sub_140BE1E7C x2, sub_140985EB0 x2, sub_1401E6680 x2, sub_1401EC68C x2, unknown_libname_1 x2, sub_14204568C x2, sub_142045394 x2, sub_140234EA8 x2, sub_140218C30 x2
```c
__int64 __fastcall sub_140219730(__int64 a1, __int64 a2)
{
  __int64 v3; // r15
  char v4; // di
  __int64 v5; // r14
  __int64 v6; // rbx
  __int64 v7; // r13
  __int64 v8; // rcx
  __int64 v9; // rax
  __int64 *v10; // rcx
  int v11; // r12d
  __int64 v12; // rax
  __int64 v13; // rax
  int v14; // r10d
  _QWORD *v15; // rsi
  __int64 v16; // rcx
  int v17; // edx
  __int64 v18; // rcx
  int v19; // r8d
  int v20; // r9d
  int v21; // r10d
  __int64 v22; // rdx
  __int64 v23; // rcx
  __int64 v24; // rdx
  __int64 v25; // rcx
  __int64 v26; // r8
  int v27; // esi
  char v28; // al
  char v29; // cl
  unsigned int v30; // r12d
  int v31; // ecx
  bool v32; // r13
  unsigned int v33; // ebx
  __int64 v34; // r15
  __int64 v35; // rax
  bool v36; // cc
  char v37; // al
  int v38; // r9d
  int v39; // edx
  __int64 v40; // rcx
  __int64 v41; // rax
  __int64 *v42; // rcx
  int v43; // r9d
  __int64 v44; // rbx
  unsigned __int64 v45; // rsi
  __int64 v46; // rbx
  int v47; // r8d
  unsigned int v48; // r13d
  __int64 v49; // rsi
  __int64 v50; // rbx
  __int64 v51; // rcx
  int v52; // r8d
  int v53; // r9d
  __int64 v54; // r9
  __int64 v55; // rax
  int v56; // r8d
  int v57; // edx
  int v58; // ecx
  __int64 v59; // rax
  __int64 v60; // rdx
  __int64 v61; // r13
  int v62; // r9d
  __int64 v63; // rax
  __int64 v64; // rdx
  unsigned __int64 v65; // rsi
  __int64 v66; // rbx
  unsigned int v67; // eax
  unsigned int v68; // eax
  unsigned int v69; // r8d
  unsigned int v70; // r13d
  __int64 v71; // r14
  unsigned int v72; // r12d
  __int64 v73; // r15
  _QWORD *v74; // rsi
  __int64 v75; // rax
  __int64 v76; // rbx
  char v77; // al
  __int64 v78; // rdx
  __int64 v79; // rdx
  __int64 v80; // rax
  __int64 v81; // rdx
  __int64 v82; // rax
  __int64 v83; // rdx
  int v85; // [rsp+20h] [rbp-E0h]
  char v86; // [rsp+20h] [rbp-E0h]
  int v87; // [rsp+20h] [rbp-E0h]
  bool v88; // [rsp+50h] [rbp-B0h]
  bool v89; // [rsp+51h] [rbp-AFh]
  unsigned __int64 v90; // [rsp+58h] [rbp-A8h] BYREF
  _QWORD *v91; // [rsp+60h] [rbp-A0h] BYREF
  int v92; // [rsp+68h] [rbp-98h]
  unsigned int v93; // [rsp+6Ch] [rbp-94h]
  unsigned int v94; // [rsp+70h] [rbp-90h]
  __int64 v95; // [rsp+78h] [rbp-88h] BYREF
  __int64 v96; // [rsp+80h] [rbp-80h] BYREF
  __int64 v97; // [rsp+88h] [rbp-78h]
  __int64 v98; // [rsp+90h] [rbp-70h]
  char v99; // [rsp+98h] [rbp-68h]
  __int64 v100; // [rsp+A0h] [rbp-60h]
  char v101[8]; // [rsp+A8h] [rbp-58h] BYREF
  __int64 v102; // [rsp+B0h] [rbp-50h]
  _BYTE v103[32]; // [rsp+C0h] [rbp-40h] BYREF
  __int64 v104; // [rsp+E0h] [rbp-20h]
  _BYTE v105[32]; // [rsp+110h] [rbp+10h] BYREF
  _BYTE v106[128]; // [rsp+130h] [rbp+30h] BYREF
  __int64 v108; // [rsp+1C8h] [rbp+C8h] BYREF
  unsigned __int64 v109; // [rsp+1D0h] [rbp+D0h] BYREF
  __int64 v110; // [rsp+1D8h] [rbp+D8h] BYREF

  v108 = a2;
  v3 = a1;
  sub_142046098(v101, 4);
  LOBYTE(v109) = byte_1432FF8A0;
  sub_140244AE0(&unk_14347F6E8, &v109);
  v4 = 0;
  v102 = (*(__int64 (__fastcall **)(_QWORD))(**(_QWORD **)(a2 + 56) + 32LL))(*(_QWORD *)(a2 + 56));
  v5 = v102;
  v6 = *(_QWORD *)(v102 + 4000);
  if ( v6 )
    v7 = *(_QWORD *)(v6 + 32);
  else
    v7 = 0;
  v8 = *(_QWORD *)(a2 + 56);
  v91 = (_QWORD *)v7;
  v9 = (*(__int64 (__fastcall **)(__int64))(*(_QWORD *)v8 + 32LL))(v8);
  v10 = *(__int64 **)(a2 + 56);
  v11 = *(_DWORD *)(v9 + 3984);
  v12 = *v10;
  v93 = v11;
  v13 = (*(__int64 (__fastcall **)(__int64 *))(v12 + 32))(v10);
  v14 = *(_DWORD *)v5;
  v92 = *(_DWORD *)(v13 + 3988);
  if ( !v14 || !*(_DWORD *)(v5 + 4) )
  {
    sub_140234EA8(v108, *(_QWORD *)(v3 + 112) + 24LL);
    return sub_1420460B8(v101);
  }
  v15 = (_QWORD *)(v3 + 18008);
  if ( (unsigned int)sub_1401FE460(v3 + 18008) != -1 )
  {
    v16 = *v15;
    if ( byte_1432F7488 )
    {
      if ( (unsigned __int8)sub_1407E34B0(v16, 0) )
      {
        LOBYTE(v19) = byte_1432F84F0;
        LOBYTE(v17) = byte_1432F8528;
        sub_141D50410(v18, v17, v19, dword_143589DD0, v21, v20, dword_143464C30, dword_143464CD0, dword_143464C80);
        v18 = *v15;
      }
      if ( (unsigned __int8)sub_1407E34B0(v18, 1) )
      {
        LOBYTE(v22) = byte_1432F81A8;
        sub_14290FC24(v23, v22);
        v23 = *v15;
      }
      if ( *(_BYTE *)(v23 + 1072) && (unsigned __int8)sub_1407E34B0(v23, 2) )
      {
        LOBYTE(v26) = byte_143589E20;
        LOBYTE(v24) = byte_1432F8608;
        sub_141D506F0(v25, v24, v26);
      }
    }
    else
    {
      LOBYTE(v109) = sub_140C4FD0C(v16);
      sub_14021A130(&off_1432F84C0, &v109);
      LOBYTE(v109) = sub_14021B490(*v15);
      sub_14021A130(&off_1432F84F8, &v109);
      LOBYTE(v109) = sub_14078931C(*v15);
      sub_14021A130(&off_1432F85D8, &v109);
    }
  }
  v27 = v92;
  if ( (v92 == 3 || v92 == 4 || v92 == 5 || v92 == 6 || (unsigned int)(v92 - 7) <= 1) && v6 )
    sub_1428E7088(v6);
  if ( (v11 == 6 || (unsigned int)(*(_DWORD *)(v5 + 820) - 69) <= 1) && v6 )
    sub_1428E7088(v6);
  if ( !v7 )
    goto LABEL_33;
  v98 = v5;
  v28 = v99 & 0xFD | (2 * sub_140CCA300(v5 + 800));
  if ( v11 <= 1 || (v29 = 0, v11 == 4) )
    v29 = 1;
  v96 = 0;
  v99 = v29 | v28 & 0xFE;
  v97 = *(_QWORD *)v5;
  sub_14079ACA0(v7, &v96, v5 + 320);
  if ( v11 == 5 )
LABEL_33:
    v30 = 0;
  else
    v30 = *(_DWORD *)(v7 + 84);
  v31 = *(_DWORD *)(v5 + 820);
  v94 = v30;
  LODWORD(v100) = v31;
  v32 = (unsigned int)(v31 - 1) <= 1;
  v88 = v32;
  v89 = (unsigned int)(v31 - 69) <= 1;
  if ( (unsigned int)(v31 - 1) <= 1 || (LOBYTE(v110) = 0, (unsigned int)(v31 - 3) <= 1) )
    LOBYTE(v110) = 1;
  v33 = 0;
  v90 = 0xCBF29CE484222325uLL;
  if ( v30 )
  {
    v34 = v108;
    do
      sub_140BE1E7C(&v90, *(_QWORD *)(v34 + 56), v33++);
    while ( v33 < v30 );
    v3 = a1;
  }
  if ( !*(_BYTE *)(v5 + 336) || (v35 = *(_QWORD *)(v5 + 3976)) == 0 || (v36 = *(_DWORD *)(v35 + 84) == 0, v37 = 1, v36) )
    v37 = 0;
  LOBYTE(v109) = v37;
  sub_14204C110(&v90, &v109);
  sub_14204C110(&v90, &byte_1433106F8);
  sub_14204C110(&v90, &byte_1433106C0);
  sub_14204C110(&v90, &byte_143310730);
  LOBYTE(v109) = sub_141D4AB40(*(_QWORD *)(qword_143427C00 + 18008));
  sub_14204C110(&v90, &v109);
  if ( !v30 || (v38 = v30, v27 == 3) )
    v38 = 1;
  v39 = *(_DWORD *)(v3 + 19620);
  v40 = *(_QWORD *)(v3 + 17952);
  LOBYTE(v109) = 0;
  v41 = sub_140983C80(v40, v39, v90, v38, (__int64)&v109);
  v44 = v41;
  v95 = v41;
  v45 = v41 + 16;
  v90 = v41 + 16;
  if ( !(_BYTE)v109 )
    goto LABEL_85;
  if ( !v30 )
  {
    v42 = *(__int64 **)(v41 + 128);
    v86 = 0;
    v46 = *v42;
LABEL_53:
    LOBYTE(v43) = v32;
    sub_140982C7C((_DWORD)v42, v46, v46 + 112, v43, v86);
    sub_140985EB0(v45, v46, v47, 0, v87);
    goto LABEL_84;
  }
  if ( v92 == 3 )
  {
    v86 = 1;
    v46 = **(_QWORD **)(v41 + 128);
    goto LABEL_53;
  }
  v48 = 0;
  v49 = 0;
  do
  {
    if ( !(unsigned __int8)sub_1401E4B60(*(_QWORD *)(v49 + v91[9]) + 20LL) || v93 != 2 )
    {
      v109 = 0xCBF29CE484222325uLL;
      sub_140BE1E7C(&v109, *(_QWORD *)(v108 + 56), v48);
      v50 = sub_1414D5C0C(v44, &v109);
      if ( !(unsigned __int8)sub_1428E6F14(v50, &v109) )
        goto LABEL_81;
      if ( v93 < 2 )
  // ... (180 more lines)
```

## PLANAR-REFLECTION FG builder  `sub_14078A6B8`  (RVA 0x78A6B8, size 0xC22, simd=2)
**Role:** references 'renderstage_planar_reflection' x2; registers 9 arena slots
**Callers (3):** data 14312CB68 ?, data 1432346C8 ?, data 144994B0C ?
**Callees:** sub_1401F3D20 x11, sub_1401F0F80 x9, sub_1401F3A6C x8, sub_14023AF5C x4, sub_140153F94 x4, sub_1401F881C x4, sub_140157ABC x2, sub_14060A86C x2, sub_1401F5ABC x2, sub_14020DED8 x2, sub_1401ED9A4 x1, sub_14078B4F4 x1, sub_140787548 x1, sub_14077113C x1
*(no decompile — disasm):*
```
  14078A6B8  mov     rax, rsp                              
  14078A6BB  mov     [rax+8], rbx                          
  14078A6BF  mov     [rax+10h], rdx                        
  14078A6C3  push    rbp                                   
  14078A6C4  push    rsi                                   
  14078A6C5  push    rdi                                   
  14078A6C6  push    r12                                   
  14078A6C8  push    r13                                   
  14078A6CA  push    r14                                   
  14078A6CC  push    r15                                   
  14078A6CE  lea     rbp, [rax-308h]                       
  14078A6D5  sub     rsp, 3D0h                             
  14078A6DC  and     [rbp+300h+arg_18], 0                  
  14078A6E3  mov     r13, rdx                              
  14078A6E6  mov     rcx, r13                              
  14078A6E9  movaps  xmmword ptr [rax-48h], xmm6           
  14078A6ED  mov     edx, 16h                              
  14078A6F2  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A6F7  test    al, al                                
  14078A6F9  jz      loc_14078AA5A                         
  14078A6FF  mov     r9, [r13+18h]                         
  14078A703  mov     edx, 14h                              
  14078A708  mov     rcx, r13                              
  14078A70B  mov     rbx, [r9+1D50h]                       
  14078A712  mov     r10, [r9+1D60h]                       
  14078A719  mov     [rbp+300h+var_338], rbx               
  14078A71D  mov     [rbp+300h+var_320], r10               
  14078A721  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A726  lea     r12d, [rdx-13h]                       
  14078A72A  mov     r15d, 7FFFFFFEh                       
  14078A730  test    al, al                                
  14078A732  jnz     loc_14078B2A4                         
  14078A738  mov     eax, [rbx+0DCh]                       
  14078A73E  sub     eax, r12d                             
  14078A741  cmp     eax, r15d                             
  14078A744  setnbe  al                                    
  14078A747  test    byte ptr [r9+3F4h], 8                 
  14078A74F  jnz     loc_14078B245                         
  14078A755  test    al, al                                
  14078A757  jnz     loc_14078B245                         
  14078A75D  mov     r8b, 1Bh                              
  14078A760  lea     rdx, [rsp+400h+var_3A0]               
  14078A765  mov     rcx, r13                              
  14078A768  call    sub_1401ED9A4                           -> sub_1401ED9A4
  14078A76D  movzx   eax, [rsp+400h+var_390]               
  14078A772  mov     r14d, 3                               
  14078A778  add     eax, r14d                             
  14078A77B  shr     eax, 2                                
  14078A77E  mov     [rbp+300h+var_370], eax               
  14078A781  cmp     eax, r12d                             
  14078A784  jnb     short loc_14078A78D                   
  14078A786  movzx   eax, r12w                             
  14078A78A  mov     [rbp+300h+var_370], eax               
  14078A78D  mov     [rsp+400h+var_390], ax                
  14078A792  mov     word ptr [rsp+400h+var_38C], ax       
  14078A797  movzx   eax, word ptr [rsp+400h+var_38E]      
  14078A79C  add     eax, r14d                             
  14078A79F  shr     eax, 2                                
  14078A7A2  mov     [rbp+300h+arg_18], eax                
  14078A7A8  cmp     eax, r12d                             
  14078A7AB  jnb     short loc_14078A7B7                   
  14078A7AD  movzx   eax, r12w                             
  14078A7B1  mov     [rbp+300h+arg_18], eax                
  14078A7B7  lea     r9, [rsp+400h+var_3A0]                
  14078A7BC  mov     word ptr [rsp+400h+var_38E], ax       
  14078A7C1  mov     r8d, 5C83AD89h                        
  14078A7C7  mov     word ptr [rsp+400h+var_38C+2], ax     
  14078A7CC  lea     rdx, [rbp+300h+var_2F8]               
  14078A7D0  call    sub_14078B4F4                           -> sub_14078B4F4
  14078A7D5  lea     r9, [rsp+400h+var_3A0]                
  14078A7DA  mov     [rbp+300h+var_37B], 12h               
  14078A7DE  mov     r8d, 34A28D5h                         
  14078A7E4  lea     rdx, [rbp+300h+var_308]               
  14078A7E8  mov     rcx, r13                              
  14078A7EB  call    sub_140157ABC                           -> sub_140157ABC
  14078A7F0  lea     r9, [rsp+400h+var_3A0]                
  14078A7F5  mov     [rbp+300h+var_37B], r12b              
  14078A7F9  mov     r8d, 4D3A702Ah                        
  14078A7FF  lea     rdx, [rbp+300h+var_318]               
  14078A803  mov     rcx, r13                              
  14078A806  call    sub_140157ABC                           -> sub_140157ABC
  14078A80B  test    [r13+30h], r12b                       
  14078A80F  jnz     loc_14078B2B0                         
  14078A815  mov     esi, [r13+34h]                        
  14078A819  mov     rcx, [r13+8]                          
  14078A81D  lea     rdx, [rsp+400h+var_3A0]               
  14078A822  mov     r8b, [r13+38h]                        
  14078A826  shl     esi, 18h                              
  14078A829  xor     esi, 15EAB19Ch                        
  14078A82F  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A834  mov     rcx, [rcx]                            
  14078A837  mov     byte ptr [rsp+400h+var_3A0], 4        
  14078A83C  mov     dword ptr [rsp+400h+var_3A0+4], esi   
  14078A840  mov     [rsp+400h+var_398], r12d              
  14078A845  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A84A  test    [r13+30h], r12b                       
  14078A84E  jnz     loc_14078B2B7                         
  14078A854  mov     edi, [r13+34h]                        
  14078A858  mov     rcx, [r13+8]                          
  14078A85C  lea     rdx, [rsp+400h+var_3A0]               
  14078A861  mov     r8b, [r13+38h]                        
  14078A865  shl     edi, 18h                              
  14078A868  xor     edi, 3A4A52ABh                        
  14078A86E  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A873  mov     rcx, [rcx]                            
  14078A876  mov     byte ptr [rsp+400h+var_3A0], 4        
  14078A87B  mov     dword ptr [rsp+400h+var_3A0+4], edi   
  14078A87F  mov     [rsp+400h+var_398], r12d              
  14078A884  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A889  mov     edx, 14h                              
  14078A88E  mov     qword ptr [rbp+300h+var_2D0], r13     
  14078A892  mov     rcx, r13                              
  14078A895  mov     dword ptr [rbp+300h+var_2D0+8], edi   
  14078A898  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A89D  lea     ebx, [rdx-12h]                        
  14078A8A0  test    al, al                                
  14078A8A2  jnz     loc_141EF766C                         
  14078A8A8  mov     [rbp+300h+var_358], 0FD3DDE00h        
  14078A8AF  lea     rdx, [rbp+300h+var_360]               
  14078A8B3  xor     ecx, ecx                              
  14078A8B5  mov     eax, ebx                              
  14078A8B7  and     qword ptr [rdx], 0                    
  14078A8BB  mov     r15d, [rdx+8]                         
  14078A8BF  mov     [rbp+300h+var_320], rcx               
  14078A8C3  mov     [rbp+300h+var_2C0], rcx               
  14078A8C7  mov     [rbp+300h+var_2B8], r15d              
  14078A8CB  test    bl, al                                
  14078A8CD  jz      short loc_14078A909                   
  14078A8CF  mov     rcx, [rbp+300h+var_360]               
  14078A8D3  and     eax, 0FFFFFFFDh                       
  14078A8D6  mov     dword ptr [rsp+400h+var_3A8], eax     
  14078A8DA  test    rcx, rcx                              
  14078A8DD  jz      short loc_14078A909                   
  14078A8DF  mov     r8b, [rcx+38h]                        
  14078A8E3  lea     rdx, [rsp+400h+var_3A0]               
  14078A8E8  mov     rcx, [rcx+8]                          
  14078A8EC  mov     eax, [rbp+300h+var_358]               
  14078A8EF  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A8F4  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A8F9  mov     rcx, [rcx]                            
  14078A8FC  mov     dword ptr [rsp+400h+var_3A0+4], eax   
  14078A900  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A905  mov     eax, dword ptr [rsp+400h+var_3A8]     
  14078A909  test    r12b, al                              
  14078A90C  jnz     loc_141EF76D8                         
  14078A912  test    [r13+30h], bl                         
  14078A916  jz      short loc_14078A95A                   
  14078A918  mov     rax, [r13+18h]                        
  14078A91C  mov     r9, [rax+1E10h]                       
  14078A923  lea     rax, aRenderstagePla; "renderstage_planar_reflection"  -> &aRenderstagePla
  14078A92A  mov     rcx, rax                              
  14078A92D  mov     [rbp+300h+var_360], rax               
  14078A931  call    sub_14060A86C                           -> sub_14060A86C
  14078A936  movsd   xmm0, [rbp+300h+var_360]              
  14078A93B  lea     rdx, [rbp+300h+var_360]               
  14078A93F  xor     r8d, r8d                              
  14078A942  movsd   [rbp+300h+var_360], xmm0              
  14078A947  mov     rcx, r9                               
  14078A94A  mov     [rbp+300h+var_358], eax               
  14078A94D  call    sub_140787548                           -> sub_140787548
  14078A952  test    al, al                                
  14078A954  jnz     loc_14078AA7A                         
  14078A95A  mov     rax, [rbp+300h+var_320]               
  14078A95E  test    rax, rax                              
  14078A961  jz      short loc_14078A987                   
  14078A963  mov     rcx, [rax+8]                          
  14078A967  lea     rdx, [rsp+400h+var_3A0]               
  14078A96C  mov     r8b, [rax+38h]                        
  14078A970  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A975  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A97A  mov     rcx, [rcx]                            
  14078A97D  mov     dword ptr [rsp+400h+var_3A0+4], r15d  
  14078A982  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A987  mov     rcx, [r13+8]                          
  14078A98B  lea     rdx, [rsp+400h+var_3A0]               
  14078A990  mov     r8b, [r13+38h]                        
  14078A994  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A999  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A99E  mov     rcx, [rcx]                            
  14078A9A1  mov     dword ptr [rsp+400h+var_3A0+4], edi   
  14078A9A5  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A9AA  mov     rcx, [r13+8]                          
  14078A9AE  lea     rdx, [rsp+400h+var_3A0]               
  14078A9B3  mov     r8b, [r13+38h]                        
  14078A9B7  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A9BC  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A9C1  mov     rcx, [rcx]                            
  14078A9C4  mov     dword ptr [rsp+400h+var_3A0+4], esi   
  14078A9C8  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A9CD  mov     rdx, [rbp+300h+var_318]               
  14078A9D1  test    rdx, rdx                              
  14078A9D4  jz      short loc_14078A9FC                   
  14078A9D6  mov     rcx, [rdx+8]                          
  14078A9DA  mov     r8b, [rdx+38h]                        
  14078A9DE  lea     rdx, [rsp+400h+var_3A0]               
  14078A9E3  mov     eax, [rbp+300h+var_310]               
  14078A9E6  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A9EB  mov     rcx, [rcx]                            
  14078A9EE  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A9F3  mov     dword ptr [rsp+400h+var_3A0+4], eax   
  14078A9F7  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A9FC  mov     rdx, [rbp+300h+var_308]               
  14078AA00  test    rdx, rdx                              
  14078AA03  jz      short loc_14078AA2B                   
  14078AA05  mov     rcx, [rdx+8]                          
  14078AA09  mov     r8b, [rdx+38h]                        
  14078AA0D  lea     rdx, [rsp+400h+var_3A0]               
  14078AA12  mov     eax, [rbp+300h+var_300]               
  14078AA15  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078AA1A  mov     rcx, [rcx]                            
  14078AA1D  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078AA22  mov     dword ptr [rsp+400h+var_3A0+4], eax   
  14078AA26  call    sub_1401F0F80                           -> sub_1401F0F80
  14078AA2B  mov     rdx, [rbp+300h+var_2F8]               
  14078AA2F  test    rdx, rdx                              
  14078AA32  jz      short loc_14078AA5A                   
  14078AA34  mov     rcx, [rdx+8]                          
  14078AA38  mov     r8b, [rdx+38h]                        
  14078AA3C  lea     rdx, [rsp+400h+var_3A0]               
  14078AA41  mov     eax, [rbp+300h+var_2F0]               
  14078AA44  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078AA49  mov     rcx, [rcx]                            
  14078AA4C  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078AA51  mov     dword ptr [rsp+400h+var_3A0+4], eax   
  14078AA55  call    sub_1401F0F80                           -> sub_1401F0F80
  14078AA5A  lea     r11, [rsp+400h+var_30]                
  14078AA62  mov     rbx, [r11+40h]                        
  14078AA66  movaps  xmm6, xmmword ptr [r11-10h]           
  14078AA6B  mov     rsp, r11                              
  14078AA6E  pop     r15                                   
  14078AA70  pop     r14                                   
  14078AA72  pop     r13                                   
  14078AA74  pop     r12                                   
  14078AA76  pop     rdi                                   
  14078AA77  pop     rsi                                   
  14078AA78  pop     rbp                                   
  14078AA79  retn                                          
  14078AA7A  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  14078AA81  mov     edx, 14h                              
  14078AA86  mov     rcx, r13                              
  14078AA89  mov     [rbp+300h+var_330], rax               
  14078AA8D  call    sub_14023AF5C                           -> sub_14023AF5C
  14078AA92  test    al, al                                
  14078AA94  jnz     loc_141EF7711                         
  14078AA9A  mov     rax, [rbp+300h+var_338]               
  14078AA9E  mov     ebx, [rax+0DCh]                       
  14078AAA4  mov     rdx, [rbp+300h+var_2F8]               
  14078AAA8  mov     dword ptr [rsp+400h+var_3B0], ebx     
  14078AAAC  test    rdx, rdx                              
  14078AAAF  jz      loc_14078B22C                         
  14078AAB5  mov     rcx, [rdx+8]                          
  14078AAB9  lea     r8, [rbp+300h+var_2F0]                
  14078AABD  mov     r9b, [rdx+38h]                        
  14078AAC1  lea     rdx, [rsp+400h+var_3A8]               
  14078AAC6  mov     rcx, [rcx]                            
  14078AAC9  call    sub_1401F3D20                           -> sub_1401F3D20
  14078AACE  mov     eax, dword ptr [rsp+400h+var_3A8]     
  14078AAD2  xorps   xmm1, xmm1                            
  14078AAD5  mov     dword ptr [rsp+400h+var_3A8], eax     
  14078AAD9  lea     rcx, [rsp+400h+var_3A8]               
```

## renderstage_planar_reflection user 2  `sub_14078B824`  (RVA 0x78B824, size 0x43, simd=0)
**Callers (2):** data 1400E5BF0 sub_1400E5BF0, data 144994B6C ?
**Callees:** sub_140A26354 x1
```c
__int64 *sub_14078B824()
{
  if ( !byte_1435397E9 )
  {
    sub_140A26354(0x5225E1FED15B89BALL, "renderstage_planar_reflection");
    byte_1435397E9 = 1;
  }
  qword_143814628 = 0x5225E1FED15B89BALL;
  return &qword_143814628;
}
```

## view-CONST builder (view+proj)  `sub_140789724`  (RVA 0x789724, size 0x9B1, simd=58)
**Role:** reads camera [a2+0x18], FOV, tanf -> projection; writes matrices
**Callers (4):** CALL 14015833C sub_140157B24, CALL 14078AD8B sub_14078A6B8, data 143234644 ?, data 144994AC4 ?
**Callees:** memset x4, sub_1407896F8 x4, sub_1429ED740 x3, sub_14028DB28 x2, sub_14023AF5C x2, sub_14028E298 x2, sub_1408023AC x2, sub_14078A0D8 x1, tanf x1, sub_1401E3EE8 x1, sub_1401DA684 x1, sub_1401E412C x1, sub_14078A214 x1, sub_14078A168 x1
*(no decompile — disasm):*
```
  140789724  mov     rax, rsp                              
  140789727  mov     [rax+20h], r9d                        
  14078972B  mov     [rax+18h], r8d                        
  14078972F  push    rbp                                   
  140789730  push    rbx                                   
  140789731  push    rsi                                   
  140789732  push    rdi                                   
  140789733  push    r12                                   
  140789735  push    r13                                   
  140789737  push    r14                                   
  140789739  push    r15                                   
  14078973B  lea     rbp, [rax-0B98h]                      
  140789742  sub     rsp, 0C58h                            
  140789749  mov     r12, [rdx+18h]                        
  14078974D  mov     r14, rcx                              
  140789750  mov     rcx, [rdx]                            
  140789753  mov     edi, r9d                              
  140789756  movaps  xmmword ptr [rax-58h], xmm6           
  14078975A  mov     ebx, r8d                              
  14078975D  movaps  xmmword ptr [rax-68h], xmm7           
  140789761  mov     rsi, rdx                              
  140789764  movaps  xmmword ptr [rax-78h], xmm8           
  140789769  movaps  xmmword ptr [rax-88h], xmm9           
  140789771  movaps  xmmword ptr [rax-98h], xmm10          
  140789779  movaps  xmmword ptr [rax-0A8h], xmm11         
  140789781  movaps  xmmword ptr [rax-0B8h], xmm12         
  140789789  movaps  xmmword ptr [rax-0C8h], xmm13         
  140789791  movaps  xmmword ptr [rax-0D8h], xmm14         
  140789799  movaps  xmmword ptr [rax-0E8h], xmm15         
  1407897A1  mov     rax, [rcx]                            
  1407897A4  call    qword ptr [rax+20h]                   
  1407897A7  mov     rdx, [rsi+18h]                        
  1407897AB  movss   xmm0, cs:dword_1431EF260                -> &dword_1431EF260
  1407897B3  movss   xmm1, cs:dword_1431EF2FC                -> &dword_1431EF2FC
  1407897BB  movss   xmm14, dword ptr [rax+720h]           
  1407897C4  mov     rcx, [rdx+1D50h]                      
  1407897CB  lea     rdx, [rbp+0B90h+arg_8]                
  1407897D2  movss   xmm15, dword ptr [rax+724h]           
  1407897DB  mov     [rsp+0C90h+var_C70], rcx              
  1407897E0  lea     rcx, [rbp+0B90h+arg_0]                
  1407897E7  movss   [rbp+0B90h+arg_0], xmm0               
  1407897EF  movss   [rbp+0B90h+arg_8], xmm1               
  1407897F7  call    sub_14078A0D8                           -> sub_14078A0D8
  1407897FC  movss   xmm0, dword ptr [r12+90h]             
  140789806  mulss   xmm0, cs:dword_1431EEFA4; X             -> &dword_1431EEFA4
  14078980E  call    tanf                                    -> tanf
  140789813  movss   xmm1, dword ptr [r12+9Ch]             
  14078981D  lea     rcx, [r14+40h]; Dst                   
  140789821  movss   xmm12, dword ptr [r12+98h]            
  14078982B  movaps  xmm9, xmm0                            
  14078982F  maxss   xmm1, cs:dword_1431EEE5C                -> &dword_1431EEE5C
  140789837  movaps  xmm6, cs:xmmword_1431EFE70              -> &xmmword_1431EFE70
  14078983E  mov     r13d, 40h ; '@'                       
  140789844  movaps  xmm7, cs:xmmword_1431EFFC0              -> &xmmword_1431EFFC0
  14078984B  mov     r8d, r13d; Size                       
  14078984E  movaps  xmm10, cs:xmmword_1431F0390             -> &xmmword_1431F0390
  140789856  xor     edx, edx; Val                         
  140789858  movaps  xmm11, cs:xmmword_1431F0E80             -> &xmmword_1431F0E80
  140789860  movups  xmmword ptr [r14], xmm6               
  140789864  movups  xmmword ptr [r14+10h], xmm7           
  140789869  movups  xmmword ptr [r14+20h], xmm10          
  14078986E  divss   xmm9, xmm1                            
  140789873  movups  xmmword ptr [r14+30h], xmm11          
  140789878  call    memset                                  -> memset
  14078987D  movups  xmmword ptr [r14+40h], xmm6           
  140789882  lea     r15, [r14+80h]                        
  140789889  movups  xmmword ptr [r14+50h], xmm7           
  14078988E  mov     rcx, r15; Dst                         
  140789891  movups  xmmword ptr [r14+60h], xmm10          
  140789896  mov     r8d, r13d; Size                       
  140789899  xor     edx, edx; Val                         
  14078989B  movups  xmmword ptr [r14+70h], xmm11          
  1407898A0  call    memset                                  -> memset
  1407898A5  movups  xmmword ptr [r15], xmm6               
  1407898A9  lea     r13, [r14+0C0h]                       
  1407898B0  movups  xmmword ptr [r15+10h], xmm7           
  1407898B5  xor     edx, edx; Val                         
  1407898B7  movups  xmmword ptr [r15+20h], xmm10          
  1407898BC  mov     rcx, r13; Dst                         
  1407898BF  movups  xmmword ptr [r15+30h], xmm11          
  1407898C4  lea     r8d, [rdx+40h]; Size                  
  1407898C8  call    memset                                  -> memset
  1407898CD  movups  xmmword ptr [r13+0], xmm6             
  1407898D2  xor     r15d, r15d                            
  1407898D5  movups  xmmword ptr [r13+10h], xmm7           
  1407898DA  movups  xmmword ptr [r13+20h], xmm10          
  1407898DF  movups  xmmword ptr [r13+30h], xmm11          
  1407898E4  mov     [r14+100h], r15                       
  1407898EB  mov     [r14+108h], r15                       
  1407898F2  mov     [r14+110h], r15                       
  1407898F9  mov     [r14+118h], r15                       
  140789900  mov     [r14+120h], r15                       
  140789907  mov     [r14+128h], r15                       
  14078990E  mov     [r14+130h], r15                       
  140789915  lea     rcx, [r14+140h]; Dst                  
  14078991C  mov     [r14+138h], r15                       
  140789923  xor     edx, edx; Val                         
  140789925  lea     r8d, [r15+40h]; Size                  
  140789929  call    memset                                  -> memset
  14078992E  lea     rdx, [r12+70h]                        
  140789933  lea     rcx, [rbp+0B90h+var_A20]              
  14078993A  call    sub_14028DB28                           -> sub_14028DB28
  14078993F  cmp     cs:byte_1432F86B0, r15b                 -> &byte_1432F86B0
  140789946  lea     r9d, [r15+1]                          
  14078994A  jz      short loc_140789960                   
  14078994C  lea     edx, [r15+49h]                        
  140789950  mov     rcx, rsi                              
  140789953  call    sub_14023AF5C                           -> sub_14023AF5C
  140789958  test    al, al                                
  14078995A  jnz     loc_141EF7546                         
  140789960  movaps  xmm4, [rbp+0B90h+var_730]             
  140789967  lea     rdx, [rbp+0B90h+var_BA0]              
  14078996B  movaps  xmm2, [rbp+0B90h+var_710]             
  140789972  lea     rcx, [rbp+0B90h+var_730]              
  140789979  movaps  xmm3, xmm4                            
  14078997C  movaps  xmm1, xmm2                            
  14078997F  shufps  xmm3, [rbp+0B90h+var_720], 44h ; 'D'  
  140789987  shufps  xmm1, [rbp+0B90h+var_700], 44h ; 'D'  
  14078998F  movaps  xmm0, xmm3                            
  140789992  shufps  xmm4, [rbp+0B90h+var_720], 0EEh       
  14078999A  shufps  xmm2, [rbp+0B90h+var_700], 0EEh       
  1407899A2  shufps  xmm0, xmm1, 88h                       
  1407899A6  movups  xmmword ptr [r14], xmm0               
  1407899AA  movaps  xmm0, xmm4                            
  1407899AD  shufps  xmm0, xmm2, 88h                       
  1407899B1  shufps  xmm3, xmm1, 0DDh                      
  1407899B5  shufps  xmm4, xmm2, 0DDh                      
  1407899B9  movups  xmmword ptr [r14+20h], xmm0           
  1407899BE  movups  xmmword ptr [r14+10h], xmm3           
  1407899C3  movups  xmmword ptr [r14+30h], xmm4           
  1407899C8  call    sub_1401E3EE8                           -> sub_1401E3EE8
  1407899CD  movaps  xmm5, [rbp+0B90h+var_9D0]             
  1407899D4  movss   xmm13, dword ptr cs:xmmword_1431F1C40   -> &xmmword_1431F1C40
  1407899DD  movups  xmm4, xmmword ptr [rax]               
  1407899E0  movups  xmm3, xmmword ptr [rax+20h]           
  1407899E4  movaps  xmm2, xmm4                            
  1407899E7  shufps  xmm2, xmmword ptr [rax+10h], 44h ; 'D'
  1407899EC  movaps  xmm1, xmm3                            
  1407899EF  shufps  xmm3, xmmword ptr [rax+30h], 0EEh     
  1407899F4  movaps  xmm0, xmm2                            
  1407899F7  shufps  xmm1, xmmword ptr [rax+30h], 44h ; 'D'
  1407899FC  shufps  xmm4, xmmword ptr [rax+10h], 0EEh     
  140789A01  lea     rax, [r14+80h]                        
  140789A08  shufps  xmm0, xmm1, 88h                       
  140789A0C  movups  xmmword ptr [r14+40h], xmm0           
  140789A11  movaps  xmm0, xmm4                            
  140789A14  shufps  xmm0, xmm3, 88h                       
  140789A18  movups  xmmword ptr [r14+60h], xmm0           
  140789A1D  shufps  xmm2, xmm1, 0DDh                      
  140789A21  movups  xmmword ptr [r14+50h], xmm2           
  140789A26  movaps  xmm2, [rbp+0B90h+var_9B0]             
  140789A2D  shufps  xmm4, xmm3, 0DDh                      
  140789A31  movaps  xmm1, xmm2                            
  140789A34  shufps  xmm1, [rbp+0B90h+var_9A0], 44h ; 'D'  
  140789A3C  movaps  xmm3, xmm5                            
  140789A3F  shufps  xmm3, [rbp+0B90h+var_9C0], 44h ; 'D'  
  140789A47  shufps  xmm5, [rbp+0B90h+var_9C0], 0EEh       
  140789A4F  movaps  xmm0, xmm3                            
  140789A52  shufps  xmm2, [rbp+0B90h+var_9A0], 0EEh       
  140789A5A  movups  xmmword ptr [r14+70h], xmm4           
  140789A5F  shufps  xmm0, xmm1, 88h                       
  140789A63  movups  xmmword ptr [rax], xmm0               
  140789A66  movaps  xmm0, xmm5                            
  140789A69  shufps  xmm0, xmm2, 88h                       
  140789A6D  movups  xmmword ptr [rax+20h], xmm0           
  140789A71  shufps  xmm3, xmm1, 0DDh                      
  140789A75  movups  xmmword ptr [rax+10h], xmm3           
  140789A79  shufps  xmm5, xmm2, 0DDh                      
  140789A7D  movups  xmmword ptr [rax+30h], xmm5           
  140789A81  movups  xmmword ptr [r13+0], xmm6             
  140789A86  movups  xmmword ptr [r13+10h], xmm7           
  140789A8B  movups  xmmword ptr [r13+20h], xmm10          
  140789A90  movups  xmmword ptr [r13+30h], xmm11          
  140789A95  mov     [r14+138h], r15                       
  140789A9C  cmp     cs:byte_1432F86E8, r15b                 -> &byte_1432F86E8
  140789AA3  jz      short loc_140789ABA                   
  140789AA5  mov     edx, 49h ; 'I'                        
  140789AAA  mov     rcx, rsi                              
  140789AAD  call    sub_14023AF5C                           -> sub_14023AF5C
  140789AB2  test    al, al                                
  140789AB4  jnz     loc_141EF7574                         
  140789ABA  mov     rsi, [rsi+18h]                        
  140789ABE  movss   xmm8, cs:Y                              -> &Y
  140789AC7  add     rsi, 0EF0h                            
  140789ACE  cmp     [rbp+0B90h+arg_40], r15b              
  140789AD5  jnz     loc_140789D74                         
  140789ADB  movzx   ecx, [rbp+0B90h+arg_12]               
  140789AE2  movaps  xmm5, xmm8                            
  140789AE6  movzx   eax, bx                               
  140789AE9  movaps  xmm4, xmm8                            
  140789AED  mulss   xmm12, xmm9                           
  140789AF2  cmp     bx, cx                                
  140789AF5  movd    xmm7, ecx                             
  140789AF9  xorps   xmm9, xmm13                           
  140789AFD  movd    xmm6, eax                             
  140789B01  cmovb   bx, cx                                
  140789B05  movzx   eax, [rbp+0B90h+arg_20]               
  140789B0C  cvtdq2ps xmm7, xmm7                           
  140789B0F  movd    xmm3, eax                             
  140789B13  movzx   eax, [rbp+0B90h+arg_22]               
  140789B1A  cvtdq2ps xmm3, xmm3                           
  140789B1D  movd    xmm2, eax                             
  140789B21  cvtdq2ps xmm2, xmm2                           
  140789B24  movzx   eax, di                               
  140789B27  divss   xmm5, xmm3                            
  140789B2B  divss   xmm4, xmm2                            
  140789B2F  movss   dword ptr [r14+114h], xmm2            
  140789B38  movaps  xmm1, xmm5                            
  140789B3B  movss   xmm2, cs:flt_1431EF15C                  -> &flt_1431EF15C
  140789B43  addss   xmm1, xmm5                            
  140789B47  movaps  xmm0, xmm4                            
  140789B4A  movss   dword ptr [r14+104h], xmm7            
  140789B53  addss   xmm0, xmm4                            
  140789B57  movss   dword ptr [r14+110h], xmm3            
  140789B60  cvtdq2ps xmm6, xmm6                           
  140789B63  movss   dword ptr [r14+10Ch], xmm0            
  140789B6C  movss   dword ptr [r14+108h], xmm1            
  140789B75  movd    xmm1, eax                             
  140789B79  movzx   eax, [rbp+0B90h+arg_1A]               
  140789B80  cvtdq2ps xmm1, xmm1                           
  140789B83  movd    xmm0, eax                             
  140789B87  movzx   eax, [rbp+0B90h+arg_28]               
  140789B8E  cvtdq2ps xmm0, xmm0                           
  140789B91  mulss   xmm1, xmm5                            
  140789B95  mulss   xmm0, xmm4                            
  140789B99  movss   dword ptr [r14+120h], xmm1            
  140789BA2  movaps  xmm1, xmm2                            
  140789BA5  movss   dword ptr [r14+100h], xmm6            
  140789BAE  movss   dword ptr [r14+124h], xmm0            
  140789BB7  movd    xmm0, eax                             
  140789BBB  movzx   eax, [rbp+0B90h+arg_2A]               
  140789BC2  cvtdq2ps xmm0, xmm0                           
  140789BC5  movss   dword ptr [r14+118h], xmm12           
  140789BCE  divss   xmm1, xmm0                            
  140789BD2  movd    xmm0, eax                             
  140789BD6  movzx   eax, [rbp+0B90h+arg_30]               
  140789BDD  cvtdq2ps xmm0, xmm0                           
  140789BE0  mulss   xmm1, xmm6                            
  140789BE4  divss   xmm2, xmm0                            
  140789BE8  movss   dword ptr [r14+128h], xmm1            
  140789BF1  movd    xmm0, eax                             
  140789BF5  movzx   eax, [rbp+0B90h+arg_32]               
  140789BFC  cvtdq2ps xmm0, xmm0                           
  140789BFF  mulss   xmm2, xmm7                            
  140789C03  movss   dword ptr [r14+11Ch], xmm9            
  140789C0C  movss   dword ptr [r14+12Ch], xmm2            
  140789C15  movss   xmm2, cs:dword_1431EF230                -> &dword_1431EF230
  140789C1D  movaps  xmm1, xmm2                            
  140789C20  divss   xmm1, xmm0                            
  140789C24  movd    xmm0, eax                             
  140789C28  mov     eax, cs:dword_143438A30                 -> &dword_143438A30
  140789C2E  cvtdq2ps xmm0, xmm0                           
  140789C31  mov     [r14+140h], eax                       
  140789C38  mulss   xmm1, xmm6                            
  140789C3C  divss   xmm2, xmm0                            
  140789C40  movss   dword ptr [r14+130h], xmm1            
  140789C49  mulss   xmm2, xmm7                            
  140789C4D  movss   dword ptr [r14+134h], xmm2            
  140789C56  mov     eax, [r12+3F0h]                       
  140789C5E  mov     [r14+144h], eax                       
```

## CAMERA WRITER -> renderer+0x4658 view-state  `sub_140788A9C`  (RVA 0x788A9C, size 0x880, simd=54)
**Role:** per-view: reads cam matrices, writes into the single shared view-state
**Callers (3):** data 14312E158 ?, data 144994A94 ?, data 144994AA0 ?
**Callees:** sub_1407896F8 x4, sub_1429ED740 x3, sub_1408023AC x2, sub_14028DB28 x2, sub_1401E412C x2, sub_1401E3EE8 x2, sub_14028E298 x2, sub_14023AF5C x1, sub_14078A124 x1, sub_1407305B0 x1, sub_1401DA684 x1, sub_14078A214 x1, sub_14078A168 x1, sub_14078933C x1
*(no decompile — disasm):*
```
  140788A9C  mov     rax, rsp                              
  140788A9F  mov     [rax+8], rbx                          
  140788AA3  mov     [rax+10h], rsi                        
  140788AA7  mov     [rax+18h], rdi                        
  140788AAB  mov     [rax+20h], r14                        
  140788AAF  push    rbp                                   
  140788AB0  lea     rbp, [rax-0B18h]                      
  140788AB7  sub     rsp, 0C10h                            
  140788ABE  movaps  xmmword ptr [rax-18h], xmm6           
  140788AC2  mov     r14, rdx                              
  140788AC5  movaps  xmmword ptr [rax-28h], xmm7           
  140788AC9  mov     rbx, rcx                              
  140788ACC  movaps  xmmword ptr [rax-38h], xmm8           
  140788AD1  movaps  xmmword ptr [rax-48h], xmm9           
  140788AD6  movaps  xmmword ptr [rax-58h], xmm10          
  140788ADB  movaps  xmmword ptr [rax-68h], xmm11          
  140788AE0  movaps  xmmword ptr [rax-78h], xmm12          
  140788AE5  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  140788AEC  cmp     qword ptr [rax+4658h], 0              
  140788AF4  jz      loc_1407892DC                         
  140788AFA  mov     rcx, [rdx]                            
  140788AFD  mov     rax, [rcx]                            
  140788B00  call    qword ptr [rax+20h]                   
  140788B03  mov     edx, [rax+0F94h]                      
  140788B09  test    edx, edx                              
  140788B0B  jz      short loc_140788B2B                   
  140788B0D  cmp     edx, 4                                
  140788B10  jnz     loc_1407892DC                         
  140788B16  mov     edx, 46h ; 'F'                        
  140788B1B  mov     rcx, r14                              
  140788B1E  call    sub_14023AF5C                           -> sub_14023AF5C
  140788B23  test    al, al                                
  140788B25  jz      loc_1407892DC                         
  140788B2B  test    byte ptr [r14+30h], 2                 
  140788B30  jz      loc_1407892DC                         
  140788B36  cmp     byte ptr [rbx+18h], 0                 
  140788B3A  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  140788B41  mov     rdi, [rax+4658h]                      
  140788B48  jz      short loc_140788B5A                   
  140788B4A  mov     rcx, r14                              
  140788B4D  call    sub_14078A124                           -> sub_14078A124
  140788B52  test    al, al                                
  140788B54  jz      short loc_140788B5A                   
  140788B56  mov     al, 1                                 
  140788B58  jmp     short loc_140788B5C                   
  140788B5A  xor     al, al                                
  140788B5C  or      [rdi+1F2h], al                        
  140788B62  mov     rax, [r14+20h]                        
  140788B66  test    rax, rax                              
  140788B69  jz      loc_1407892B7                         
  140788B6F  cmp     dword ptr [rax+54h], 0                
  140788B73  jbe     loc_1407892B7                         
  140788B79  mov     rcx, [r14+18h]                        
  140788B7D  mov     edx, 59h ; 'Y'                        
  140788B82  add     rcx, 17D0h                            
  140788B89  call    sub_1407305B0                           -> sub_1407305B0
  140788B8E  test    al, al                                
  140788B90  jnz     loc_1407892DC                         
  140788B96  movss   xmm12, cs:dword_1431EEE78               -> &dword_1431EEE78
  140788B9F  mov     byte ptr [rdi+1F3h], 1                
  140788BA6  mov     rsi, [r14+18h]                        
  140788BAA  mov     eax, [rsi+0B0h]                       
  140788BB0  mov     [rdi+1D0h], eax                       
  140788BB6  mov     eax, [rsi+0B4h]                       
  140788BBC  mov     [rdi+1D4h], eax                       
  140788BC2  movss   xmm0, cs:dword_143464DC0                -> &dword_143464DC0
  140788BCA  movss   dword ptr [rdi+1E8h], xmm0            
  140788BD2  movss   xmm1, cs:dword_143464E10                -> &dword_143464E10
  140788BDA  movss   dword ptr [rdi+1ECh], xmm1            
  140788BE2  mov     rbx, [r14+18h]                        
  140788BE6  add     rbx, 0EF0h                            
  140788BED  mov     al, [rbx]                             
  140788BEF  mov     [rdi+1F0h], al                        
  140788BF5  cmp     byte ptr [rbx], 0                     
  140788BF8  jz      loc_140789074                         
  140788BFE  movups  xmm0, xmmword ptr [rsi+0C0h]          
  140788C05  lea     rcx, [rbx+20h]                        
  140788C09  movss   xmm7, cs:Y                              -> &Y
  140788C11  lea     rdx, [rsp+0C10h+var_BB8+8]            
  140788C16  movd    xmm2, dword ptr [rbx+8]               
  140788C1B  movups  xmm1, xmmword ptr [rsi+0D0h]          
  140788C22  movd    xmm3, dword ptr [rbx+10h]             
  140788C27  movd    xmm4, dword ptr [rsi+78h]             
  140788C2C  movaps  xmm11, cs:xmmword_1431F0E80             -> &xmmword_1431F0E80
  140788C34  movaps  xmm10, cs:xmmword_1431EFE70             -> &xmmword_1431EFE70
  140788C3C  movaps  xmm9, cs:xmmword_1431EFFC0              -> &xmmword_1431EFFC0
  140788C44  movaps  xmm8, cs:xmmword_1431F0390              -> &xmmword_1431F0390
  140788C4C  movaps  [rbp+0B10h+var_A30], xmm0             
  140788C53  movups  xmm0, xmmword ptr [rsi+0E0h]          
  140788C5A  movaps  [rbp+0B10h+var_A10], xmm0             
  140788C61  movd    xmm0, dword ptr [rbx+0Ch]             
  140788C66  cvtdq2ps xmm2, xmm2                           
  140788C69  cvtdq2ps xmm0, xmm0                           
  140788C6C  mulss   xmm2, xmm12                           
  140788C71  mulss   xmm0, xmm12                           
  140788C76  movaps  xmm6, xmm2                            
  140788C79  movaps  [rbp+0B10h+var_A20], xmm1             
  140788C80  movd    xmm2, dword ptr [rsi+70h]             
  140788C85  unpcklps xmm6, xmm0                           
  140788C88  movd    xmm0, dword ptr [rsi+74h]             
  140788C8D  cvtdq2ps xmm2, xmm2                           
  140788C90  cvtdq2ps xmm0, xmm0                           
  140788C93  mulss   xmm2, xmm12                           
  140788C98  mulss   xmm0, xmm12                           
  140788C9D  movaps  xmm5, xmm2                            
  140788CA0  movaps  [rbp+0B10h+var_A00], xmm11            
  140788CA8  cvtdq2ps xmm3, xmm3                           
  140788CAB  unpcklps xmm5, xmm0                           
  140788CAE  cvtdq2ps xmm4, xmm4                           
  140788CB1  mulss   xmm3, xmm12                           
  140788CB6  mulss   xmm4, xmm12                           
  140788CBB  unpcklps xmm3, xmm7                           
  140788CBE  movlhps xmm6, xmm3                            
  140788CC1  unpcklps xmm4, xmm7                           
  140788CC4  movlhps xmm5, xmm4                            
  140788CC7  subps   xmm5, xmm6                            
  140788CCA  movaps  [rbp+0B10h+var_B30], xmm10            
  140788CCF  movaps  [rbp+0B10h+var_B20], xmm9             
  140788CD4  movaps  [rbp+0B10h+var_B10], xmm8             
  140788CD9  movaps  xmm2, xmm5                            
  140788CDC  movaps  xmm1, xmm5                            
  140788CDF  shufps  xmm2, xmm5, 0AAh                      
  140788CE3  movaps  xmm0, xmm5                            
  140788CE6  shufps  xmm1, xmm5, 55h ; 'U'                 
  140788CEA  unpcklps xmm2, xmm7                           
  140788CED  unpcklps xmm0, xmm1                           
  140788CF0  movlhps xmm0, xmm2                            
  140788CF3  movaps  [rbp+0B10h+var_B00], xmm0             
  140788CF7  call    sub_1401DA684                           -> sub_1401DA684
  140788CFC  lea     r8, [rsp+0C10h+var_BF8+8]             
  140788D01  lea     rdx, [rbp+0B10h+var_B30]              
  140788D05  lea     rcx, [rbp+0B10h+var_AB0]              
  140788D09  movups  xmm4, xmmword ptr [rax]               
  140788D0C  movups  xmm3, xmmword ptr [rax+20h]           
  140788D10  movaps  xmm2, xmm4                            
  140788D13  shufps  xmm2, xmmword ptr [rax+10h], 44h ; 'D'
  140788D18  movaps  xmm1, xmm3                            
  140788D1B  shufps  xmm1, xmmword ptr [rax+30h], 44h ; 'D'
  140788D20  movaps  xmm0, xmm2                            
  140788D23  shufps  xmm4, xmmword ptr [rax+10h], 0EEh     
  140788D28  shufps  xmm3, xmmword ptr [rax+30h], 0EEh     
  140788D2D  shufps  xmm0, xmm1, 88h                       
  140788D31  movaps  [rsp+0C10h+var_BF8+8], xmm0           
  140788D36  movaps  xmm0, xmm4                            
  140788D39  shufps  xmm0, xmm3, 88h                       
  140788D3D  shufps  xmm2, xmm1, 0DDh                      
  140788D41  shufps  xmm4, xmm3, 0DDh                      
  140788D45  movaps  [rsp+0C10h+var_BD8+8], xmm0           
  140788D4A  movaps  [rsp+0C10h+var_BE8+8], xmm2           
  140788D4F  movaps  [rsp+0C10h+var_BC8+8], xmm4           
  140788D54  call    sub_1408023AC                           -> sub_1408023AC
  140788D59  lea     r8, xmmword_1434678C0                   -> &xmmword_1434678C0
  140788D60  lea     rdx, [rbp+0B10h+var_AB0]              
  140788D64  lea     rcx, [rbp+0B10h+var_B30]              
  140788D68  call    sub_1408023AC                           -> sub_1408023AC
  140788D6D  movaps  xmm0, [rbp+0B10h+var_B30]             
  140788D71  movaps  xmm1, [rbp+0B10h+var_B20]             
  140788D75  lea     rdx, [rbp+0B10h+var_A30]              
  140788D7C  movaps  [rsp+0C10h+var_BE8+8], xmm1           
  140788D81  lea     rcx, [rbp+0B10h+var_610]              
  140788D88  movaps  xmm1, [rbp+0B10h+var_B00]             
  140788D8C  movaps  [rsp+0C10h+var_BF8+8], xmm0           
  140788D91  movaps  xmm0, [rbp+0B10h+var_B10]             
  140788D95  movaps  [rsp+0C10h+var_BC8+8], xmm1           
  140788D9A  movaps  [rsp+0C10h+var_BD8+8], xmm0           
  140788D9F  call    sub_1407896F8                           -> sub_1407896F8
  140788DA4  lea     rdx, [rsi+70h]                        
  140788DA8  lea     rcx, [rbp+0B10h+var_410]              
  140788DAF  call    sub_14028DB28                           -> sub_14028DB28
  140788DB4  and     [rbp+0B10h+var_A0], 0                 
  140788DBB  lea     rcx, [rbp+0B10h+var_410]              
  140788DC2  and     [rbp+0B10h+var_9C], 0                 
  140788DC9  and     [rbp+0B10h+var_90], 0                 
  140788DD0  mov     [rbp+0B10h+var_98], 1                 
  140788DDA  mov     [rbp+0B10h+var_94], 1                 
  140788DE4  call    sub_1401E412C                           -> sub_1401E412C
  140788DE9  lea     rdx, [rbp+0B10h+var_120]              
  140788DF0  lea     rcx, [rbp+0B10h+var_590]              
  140788DF7  call    sub_1407896F8                           -> sub_1407896F8
  140788DFC  lea     rdx, [rsp+0C10h+var_BF8+8]            
  140788E01  lea     rcx, [rbp+0B10h+var_510]              
  140788E08  call    sub_1407896F8                           -> sub_1407896F8
  140788E0D  lea     rdx, [rbx+0B0h]                       
  140788E14  lea     rcx, [rbp+0B10h+var_490]              
  140788E1B  call    sub_1407896F8                           -> sub_1407896F8
  140788E20  lea     r8, [rbp+0B10h+var_610]               
  140788E27  movaps  [rsp+0C10h+var_BF8+8], xmm10          
  140788E2D  lea     rdx, [rbp+0B10h+var_590]              
  140788E34  movaps  [rsp+0C10h+var_BE8+8], xmm9           
  140788E3A  lea     rcx, [rbp+0B10h+var_A30]              
  140788E41  movaps  [rsp+0C10h+var_BD8+8], xmm8           
  140788E47  movaps  [rsp+0C10h+var_BC8+8], xmm11          
  140788E4D  call    sub_1429ED740                           -> sub_1429ED740
  140788E52  lea     rdx, [rbp+0B10h+var_B30]              
  140788E56  lea     rcx, [rsp+0C10h+var_BB8+8]            
  140788E5B  movups  xmm0, xmmword ptr [rax]               
  140788E5E  movups  xmm1, xmmword ptr [rax+10h]           
  140788E62  movups  [rsp+0C10h+var_BB8+8], xmm0           
  140788E67  movups  xmm0, xmmword ptr [rax+20h]           
  140788E6B  movups  [rsp+0C10h+var_BA8+8], xmm1           
  140788E70  movups  xmm1, xmmword ptr [rax+30h]           
  140788E74  movups  [rbp+0B10h+var_B90], xmm0             
  140788E78  movups  xmm0, xmmword ptr [rax+40h]           
  140788E7C  movups  [rbp+0B10h+var_B80], xmm1             
  140788E80  movups  xmm1, xmmword ptr [rax+50h]           
  140788E84  movups  [rbp+0B10h+var_B70], xmm0             
  140788E88  movups  xmm0, xmmword ptr [rax+60h]           
  140788E8C  movups  [rbp+0B10h+var_B60], xmm1             
  140788E90  movups  xmm1, xmmword ptr [rax+70h]           
  140788E94  movups  [rbp+0B10h+var_B50], xmm0             
  140788E98  movups  [rbp+0B10h+var_B40], xmm1             
  140788E9C  call    sub_14078A214                           -> sub_14078A214
  140788EA1  lea     r8, [rbp+0B10h+var_510]               
  140788EA8  mov     rbx, rax                              
  140788EAB  lea     rdx, [rbp+0B10h+var_490]              
  140788EB2  lea     rcx, [rsp+0C10h+var_BB8+8]            
  140788EB7  call    sub_1429ED740                           -> sub_1429ED740
  140788EBC  mov     r8, rbx                               
  140788EBF  lea     rdx, [rbp+0B10h+var_AB0]              
  140788EC3  lea     rcx, [rbp+0B10h+var_A30]              
  140788ECA  movups  xmm0, xmmword ptr [rax]               
  140788ECD  movups  [rbp+0B10h+var_AB0], xmm0             
  140788ED1  movups  xmm1, xmmword ptr [rax+10h]           
  140788ED5  movups  [rbp+0B10h+var_AA0], xmm1             
  140788ED9  movups  xmm0, xmmword ptr [rax+20h]           
  140788EDD  movups  [rbp+0B10h+var_A90], xmm0             
  140788EE4  movups  xmm1, xmmword ptr [rax+30h]           
  140788EE8  movups  [rbp+0B10h+var_A80], xmm1             
  140788EEF  movups  xmm0, xmmword ptr [rax+40h]           
  140788EF3  movups  [rbp+0B10h+var_A70], xmm0             
  140788EFA  movups  xmm1, xmmword ptr [rax+50h]           
  140788EFE  movups  [rbp+0B10h+var_A60], xmm1             
  140788F05  movups  xmm0, xmmword ptr [rax+60h]           
  140788F09  movups  [rbp+0B10h+var_A50], xmm0             
  140788F10  movups  xmm1, xmmword ptr [rax+70h]           
  140788F14  movups  [rbp+0B10h+var_A40], xmm1             
  140788F1B  call    sub_1429ED740                           -> sub_1429ED740
  140788F20  movups  xmm0, xmmword ptr [rax]               
  140788F23  lea     rdx, [rsp+0C10h+var_BF8+8]            
  140788F28  movups  xmm1, xmmword ptr [rax+10h]           
  140788F2C  lea     rcx, [rsp+0C10h+var_BB8+8]            
  140788F31  movups  [rsp+0C10h+var_BB8+8], xmm0           
  140788F36  movups  xmm0, xmmword ptr [rax+20h]           
  140788F3A  movups  [rsp+0C10h+var_BA8+8], xmm1           
  140788F3F  movups  xmm1, xmmword ptr [rax+30h]           
  140788F43  movups  [rbp+0B10h+var_B90], xmm0             
  140788F47  movups  xmm0, xmmword ptr [rax+40h]           
  140788F4B  movups  [rbp+0B10h+var_B80], xmm1             
  140788F4F  movups  xmm1, xmmword ptr [rax+50h]           
  140788F53  movups  [rbp+0B10h+var_B70], xmm0             
  140788F57  movups  xmm0, xmmword ptr [rax+60h]           
  140788F5B  movups  [rbp+0B10h+var_B60], xmm1             
  140788F5F  movups  xmm1, xmmword ptr [rax+70h]           
  140788F63  movups  [rbp+0B10h+var_B50], xmm0             
  140788F67  movups  [rbp+0B10h+var_B40], xmm1             
  140788F6B  call    sub_14078A168                           -> sub_14078A168
  140788F70  movaps  xmm1, [rsp+0C10h+var_BD8+8]           
  140788F75  lea     rcx, [rdi+60h]                        
  140788F79  movaps  xmm8, [rsp+0C10h+var_BF8+8]           
  140788F7F  lea     rdx, [rsp+0C10h+var_BB8+8]            
  140788F84  movaps  xmm0, xmm1                            
  140788F87  movaps  xmm7, xmm8                            
  140788F8B  shufps  xmm0, [rsp+0C10h+var_BC8+8], 44h ; 'D'
  140788F91  shufps  xmm1, [rsp+0C10h+var_BC8+8], 0EEh     
  140788F97  shufps  xmm7, [rsp+0C10h+var_BE8+8], 44h ; 'D'
  140788F9D  shufps  xmm8, [rsp+0C10h+var_BE8+8], 0EEh     
  140788FA4  movaps  xmm6, xmm7                            
  140788FA7  shufps  xmm6, xmm0, 88h                       
  140788FAB  movaps  xmm5, xmm8                            
  140788FAF  shufps  xmm7, xmm0, 0DDh                      
  140788FB3  movaps  xmm4, xmm6                            
  140788FB6  shufps  xmm5, xmm1, 88h                       
  140788FBA  movaps  xmm3, xmm6                            
  140788FBD  shufps  xmm8, xmm1, 0DDh                      
  140788FC2  movaps  xmm0, xmm5                            
  140788FC5  shufps  xmm0, xmm8, 44h ; 'D'                 
  140788FCA  movaps  xmm2, xmm5                            
  140788FCD  shufps  xmm4, xmm7, 44h ; 'D'                 
  140788FD1  movaps  xmm1, xmm4                            
  140788FD4  shufps  xmm3, xmm7, 0EEh                      
  140788FD8  shufps  xmm1, xmm0, 88h                       
  140788FDC  movups  xmmword ptr [rdi+20h], xmm1           
  140788FE0  shufps  xmm4, xmm0, 0DDh                      
  140788FE4  movaps  xmm0, xmm3                            
  140788FE7  movups  xmmword ptr [rdi+30h], xmm4           
  140788FEB  shufps  xmm2, xmm8, 0EEh                      
  140788FF0  shufps  xmm0, xmm2, 88h                       
  140788FF4  movups  xmmword ptr [rdi+40h], xmm0           
  140788FF8  shufps  xmm3, xmm2, 0DDh                      
  140788FFC  movups  xmmword ptr [rdi+50h], xmm3           
  140789000  movups  xmmword ptr [rcx], xmm6               
  140789003  movups  xmmword ptr [rcx+10h], xmm7           
  140789007  movups  xmmword ptr [rcx+20h], xmm5           
  14078900B  movups  xmmword ptr [rcx+30h], xmm8           
  140789010  call    sub_1401E3EE8                           -> sub_1401E3EE8
  140789015  movups  xmm4, xmmword ptr [rax]               
  140789018  movups  xmmword ptr [rcx], xmm4               
  14078901B  movaps  xmm2, xmm4                            
  14078901E  movups  xmm0, xmmword ptr [rax+10h]           
  140789022  movups  xmmword ptr [rcx+10h], xmm0           
```

*(part 2 done)*


# PART 3 — MIRROR DEFERRED PASS + REFLECT-MATH region (141D4xxxx/141D5xxxx) + PLANE search

## MIRROR deferred pass-set BUILD  `sub_141D43040`  (RVA 0x1D43040, size 0x456F, simd=0)
**Role:** registers depthprepass_mirror_opaque + gbuffer + lighting; key=r14d; 0 SIMD (pure registration)
**Callers (2):** CALL 140219D4A sub_140219730, data 144AEFD98 ?
**Callees:** sub_1409837C0 x73, sub_1409853B4 x40, sub_141321968 x34, sub_140985B54 x30, sub_1409852F4 x23, sub_140985354 x23, sub_1407305B0 x21, sub_1409846D0 x14, sub_1409842B0 x13, sub_140985204 x11, sub_140985C2C x10, sub_140985068 x9, sub_1409850CC x9, sub_140985164 x8
```c
__int64 __fastcall sub_141D43040(__int64 a1, __int64 a2, __int64 a3, _QWORD *a4, __int64 a5)
{
  __int64 v5; // rsi
  __int64 v7; // r9
  unsigned int v10; // r15d
  int v11; // edx
  int v12; // ecx
  int v13; // r8d
  int v14; // edx
  int v15; // ecx
  __int64 v16; // rcx
  __int64 v17; // rdx
  __int64 v18; // rax
  __int64 v19; // rdx
  bool v20; // al
  bool v21; // r11
  bool v22; // bl
  bool v23; // r10
  bool v24; // r9
  bool v25; // r8
  bool v26; // cl
  int v27; // edx
  __int64 v28; // rcx
  __int64 v29; // r8
  int v30; // edx
  char v31; // al
  bool v32; // al
  int v33; // edx
  int v34; // edx
  unsigned int v35; // eax
  int v36; // edx
  int v37; // edx
  __int64 v38; // rax
  int v39; // r9d
  __int64 v40; // rdi
  __int64 v41; // rax
  __int64 v42; // rax
  int v43; // r9d
  __int64 v44; // rdi
  __int64 v45; // rax
  int v46; // edx
  int v47; // eax
  int v48; // r9d
  __int64 v49; // rax
  int v50; // r9d
  __int64 v51; // rdi
  __int64 v52; // rax
  unsigned int v53; // edi
  unsigned int v54; // eax
  unsigned int v55; // eax
  unsigned int v56; // esi
  _DWORD *v57; // rax
  int v58; // edx
  int v59; // edx
  unsigned int v60; // r13d
  unsigned int v61; // edi
  unsigned int v62; // r14d
  int v63; // r9d
  __int64 v64; // rax
  _DWORD *v65; // rax
  unsigned int v66; // eax
  int v67; // edx
  __int64 v68; // rax
  int v69; // r9d
  __int64 v70; // rdi
  __int64 v71; // rax
  __int64 v72; // rax
  int v73; // r9d
  __int64 v74; // r14
  __int64 v75; // rsi
  __int64 v76; // rax
  int v77; // r9d
  __int64 v78; // rdi
  __int64 v79; // rax
  int v80; // edx
  __int64 v81; // rax
  int v82; // edx
  __int64 v83; // rax
  char v84; // si
  unsigned int v85; // eax
  __int64 v86; // rax
  int v87; // r9d
  __int64 v88; // rdi
  __int64 v89; // rax
  int v90; // edx
  unsigned int v91; // eax
  __int64 v92; // rax
  int v93; // r9d
  __int64 v94; // rdi
  __int64 v95; // rax
  __int64 v96; // rax
  int v97; // r9d
  __int64 v98; // rdi
  __int64 v99; // rax
  __int64 v100; // rax
  int v101; // r9d
  __int64 v102; // rdi
  __int64 v103; // rax
  __int64 v104; // rax
  int v105; // r9d
  __int64 v106; // rdi
  __int64 v107; // rax
  __int64 v108; // rdx
  int v109; // edx
  __int64 v110; // rax
  int v111; // r9d
  __int64 v112; // rdi
  __int64 v113; // rax
  bool v114; // r14
  bool v115; // si
  __int64 v116; // rax
  int v117; // r9d
  __int64 v118; // rdi
  __int64 v119; // rax
  int v120; // edx
  bool v121; // di
  unsigned int v122; // r13d
  int v123; // edx
  int v124; // edx
  unsigned int v125; // eax
  int v126; // edx
  bool v127; // si
  int v128; // edx
  __int64 v129; // rax
  unsigned int v130; // edi
  __int64 v131; // rax
  int v132; // edx
  __int64 v133; // rax
  __int64 v134; // rax
  int v135; // edx
  int v136; // edx
  unsigned int v137; // eax
  int v138; // edx
  char v139; // si
  unsigned int v140; // eax
  int v141; // edx
  __int64 v142; // rax
  __int64 v143; // rax
  int v144; // r9d
  __int64 v145; // rdi
  __int64 v146; // rax
  __int64 v147; // rax
  int v148; // r9d
  __int64 v149; // rdi
  __int64 v150; // rax
  __int64 v151; // rax
  int v152; // r9d
  __int64 v153; // rdi
  __int64 v154; // rax
  int v155; // edx
  int v156; // r9d
  __int64 v157; // rdi
  __int64 v158; // rax
  int v159; // r9d
  __int64 v160; // rdi
  __int64 v161; // rax
  int v162; // edx
  unsigned int v163; // eax
  __int64 v164; // rax
  __int64 v165; // rax
  int v166; // edi
  __int64 v167; // r15
  __int64 v168; // r14
  __int64 v169; // rsi
  __int64 v170; // rdi
  __int64 v171; // rax
  __int64 v172; // rax
  __int64 v173; // r15
  __int64 v174; // r14
  __int64 v175; // rsi
  __int64 v176; // rdi
  __int64 v177; // rax
  __int64 v178; // rdi
  __int64 v179; // rax
  unsigned __int8 v180; // r14
  int v181; // edx
  __int64 v182; // rax
  int v183; // r9d
  __int64 v184; // rax
  int v185; // r9d
  __int64 v186; // r14
  __int64 v187; // rsi
  int v188; // r9d
  __int64 v189; // rdi
  __int64 v190; // rax
  int v191; // edx
  int v192; // r9d
  __int64 v193; // rax
  int v194; // r9d
  __int64 v195; // rdi
  __int64 v196; // rax
  int v197; // edx
  int v198; // r9d
  __int64 v199; // rsi
  __int64 v200; // rdi
  __int64 v201; // rax
  int v202; // edx
  int v203; // r9d
  __int64 v204; // rdi
  __int64 v205; // rax
  int v206; // r9d
  __int64 v207; // rdi
  __int64 v208; // rax
  int v209; // r9d
  __int64 v210; // rdi
  __int64 v211; // rax
  int v212; // edx
  int v213; // edx
  unsigned int v214; // eax
  unsigned int v215; // edi
  __int64 v216; // rcx
  unsigned int v217; // r14d
  unsigned int v218; // ebx
  unsigned int v219; // esi
  __int64 v220; // r8
  __int64 v221; // r8
  __int64 v222; // r8
  __int64 v223; // r8
  __int64 v224; // rdx
  __int64 v225; // rdx
  __int64 v226; // rdx
  __int64 v227; // rdx
  __int64 v228; // r8
  unsigned int v229; // esi
  __int64 v230; // r8
  __int64 v231; // rdx
  __int64 v232; // r8
  __int64 v233; // rdx
  __int64 v234; // r8
  __int64 v235; // r8
  __int64 v236; // r8
  __int64 v237; // rdx
  __int64 v238; // r8
  __int64 v239; // r8
  __int64 v240; // rdx
  __int64 v241; // r8
  __int64 v242; // rdx
  __int64 v243; // r8
  __int64 v244; // rdx
  __int64 v245; // r8
  __int64 v246; // rdx
  __int64 v247; // r8
  __int64 v248; // rdx
  __int64 v249; // rdx
  unsigned int v250; // esi
  __int64 v251; // r8
  __int64 v252; // rdx
  __int64 v253; // r8
  __int64 v254; // rdx
  __int64 v255; // rdx
  __int64 v256; // rdx
  __int64 v257; // rdx
  char v258; // al
  __int64 v259; // rdx
  unsigned int v260; // esi
  __int64 v261; // rdx
  unsigned int v262; // edi
  __int64 v263; // rdx
  int v265; // [rsp+28h] [rbp-D8h]
  char v266; // [rsp+60h] [rbp-A0h] BYREF
  _BYTE v267[3]; // [rsp+61h] [rbp-9Fh] BYREF
  unsigned int v268; // [rsp+64h] [rbp-9Ch] BYREF
  _BYTE v269[4]; // [rsp+68h] [rbp-98h] BYREF
  unsigned int v270; // [rsp+6Ch] [rbp-94h] BYREF
  bool v271; // [rsp+70h] [rbp-90h]
  bool v272; // [rsp+71h] [rbp-8Fh]
  char v273; // [rsp+72h] [rbp-8Eh]
  char v274; // [rsp+73h] [rbp-8Dh] BYREF
  bool v275; // [rsp+74h] [rbp-8Ch]
  char v276; // [rsp+75h] [rbp-8Bh]
  unsigned int i; // [rsp+78h] [rbp-88h] BYREF
  char v278; // [rsp+7Ch] [rbp-84h]
  bool v279; // [rsp+7Dh] [rbp-83h]
  bool v280; // [rsp+7Eh] [rbp-82h]
  bool v281; // [rsp+7Fh] [rbp-81h]
  bool v282; // [rsp+80h] [rbp-80h] BYREF
  int v283; // [rsp+84h] [rbp-7Ch] BYREF
  unsigned int v284; // [rsp+88h] [rbp-78h] BYREF
  unsigned int v285; // [rsp+8Ch] [rbp-74h] BYREF
  unsigned int v286; // [rsp+90h] [rbp-70h] BYREF
  char v287; // [rsp+94h] [rbp-6Ch] BYREF
  bool v288[3]; // [rsp+95h] [rbp-6Bh] BYREF
  _BYTE v289[240]; // [rsp+98h] [rbp-68h] BYREF
  __int64 v290; // [rsp+188h] [rbp+88h]
  int v291; // [rsp+190h] [rbp+90h] BYREF
  unsigned int *v292; // [rsp+198h] [rbp+98h] BYREF
  __int64 v293; // [rsp+1A0h] [rbp+A0h] BYREF
  unsigned int v294; // [rsp+1A8h] [rbp+A8h] BYREF
  unsigned int v295; // [rsp+1ACh] [rbp+ACh] BYREF
  unsigned int v296; // [rsp+1B0h] [rbp+B0h] BYREF
  unsigned int v297; // [rsp+1B4h] [rbp+B4h]
  int v298; // [rsp+1B8h] [rbp+B8h] BYREF
  unsigned int v299; // [rsp+1BCh] [rbp+BCh]
  unsigned int v300; // [rsp+1C0h] [rbp+C0h]
  unsigned int *j; // [rsp+1C8h] [rbp+C8h] BYREF
  unsigned int v302; // [rsp+1D0h] [rbp+D0h]
  unsigned int v303; // [rsp+1D4h] [rbp+D4h]
  int v304; // [rsp+1D8h] [rbp+D8h] BYREF
  int v305; // [rsp+1DCh] [rbp+DCh] BYREF
  unsigned int v306; // [rsp+1E0h] [rbp+E0h] BYREF
  unsigned int v307; // [rsp+1E4h] [rbp+E4h]
  // ... (1343 more lines)
```

## mirror setup wrapper (TLS-gated)  `sub_141D56900`  (RVA 0x1D56900, size 0x7E, simd=0)
**Role:** sets TLS+410, calls sub_141D54EE0
**Callers (2):** data 141D5402C sub_141D53FA0, data 144AF09F8 ?
**Callees:** sub_141D54EE0 x1, sub_1401E6EB0 x1, sub_1402F0DE8 x1
```c
PVOID __fastcall sub_141D56900(__int64 a1, __int64 a2)
{
  __int64 v2; // r9
  PVOID result; // rax
  __int64 v5; // rcx
  __int64 v6[3]; // [rsp+20h] [rbp-18h] BYREF

  v2 = *(_QWORD *)(a2 + 32);
  if ( v2 )
    *(_BYTE *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 410LL) = *(_BYTE *)(*(_QWORD *)v2 + 33LL);
  sub_141D54EE0();
  result = NtCurrentTeb()->ThreadLocalStoragePointer;
  *(_BYTE *)(*(_QWORD *)result + 410LL) = -1;
  if ( a1 )
  {
    v5 = *(_QWORD *)(a1 + 8);
    if ( v5 )
      sub_1401E6EB0(v5);
    v6[0] = a1;
    v6[1] = 0;
    return (PVOID)sub_1402F0DE8(v6);
  }
  return result;
}
```

## HEAVY mirror matrix fn (simd~346)  `sub_141D54EE0`  (RVA 0x1D54EE0, size 0x146C, simd=227)
**Role:** reflect-across-plane candidate
**Callers (4):** CALL 141D56933 sub_141D56900, data 1432CF04C ?, data 1432CF0A0 ?, data 144AF0950 ?
**Callees:** sub_14013F898 x3, sub_1409BF14C x2, sub_1402DCFF8 x2, sub_14023AF5C x2, sub_140109A44 x2, memmove x2, sub_1403749A4 x2, sub_142927BF0 x1, sub_14291CB7C x1, sub_1402DE074 x1, powf x1, sub_14058E98C x1, sub_14291FD34 x1, sub_1402DD020 x1
*(no decompile — disasm):*
```
  141D54EE0  mov     rax, rsp                              
  141D54EE3  mov     [rax+10h], rdx                        
  141D54EE7  push    rbp                                   
  141D54EE8  push    rbx                                   
  141D54EE9  push    rsi                                   
  141D54EEA  push    rdi                                   
  141D54EEB  push    r13                                   
  141D54EED  push    r14                                   
  141D54EEF  lea     rbp, [rax-308h]                       
  141D54EF6  sub     rsp, 3D8h                             
  141D54EFD  mov     rdx, [rcx]                            
  141D54F00  xor     r13d, r13d                            
  141D54F03  mov     r10d, cs:dword_1434381C8                -> &dword_1434381C8
  141D54F0A  mov     r14, rcx                              
  141D54F0D  mov     r8, cs:qword_143438A28                  -> &qword_143438A28
  141D54F14  mov     [rax-40h], r15                        
  141D54F18  movzx   r9d, byte ptr [rdx+59h]               
  141D54F1D  movaps  xmmword ptr [rax-68h], xmm7           
  141D54F21  movaps  xmmword ptr [rax-78h], xmm8           
  141D54F26  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  141D54F2D  mov     rdi, [rax+4638h]                      
  141D54F34  test    r9b, r9b                              
  141D54F37  jz      short loc_141D54F54                   
  141D54F39  mov     eax, [rdx+r10*4+17890h]               
  141D54F41  dec     eax                                   
  141D54F43  imul    rcx, rax, 0B0h                        
  141D54F4A  mov     rbx, [rcx+r8+5C0B38h]                 
  141D54F52  jmp     short loc_141D54F57                   
  141D54F54  mov     rbx, r13                              
  141D54F57  mov     eax, [rdx+r10*4+17880h]               
  141D54F5F  mov     r11, r10                              
  141D54F62  dec     eax                                   
  141D54F64  imul    rcx, rax, 0B0h                        
  141D54F6B  mov     r10, [rcx+r8+5C0B38h]                 
  141D54F73  test    r9b, r9b                              
  141D54F76  jz      short loc_141D54F93                   
  141D54F78  mov     eax, [rdx+r11*4+17870h]               
  141D54F80  dec     eax                                   
  141D54F82  imul    rcx, rax, 0B0h                        
  141D54F89  mov     rdx, [rcx+r8+5C0B38h]                 
  141D54F91  jmp     short loc_141D54F96                   
  141D54F93  mov     rdx, r13                              
  141D54F96  mov     r9, r10                               
  141D54F99  movaps  [rsp+400h+var_58+8], xmm6             
  141D54FA1  mov     rcx, rdi                              
  141D54FA4  mov     [rsp+28h], rbx                        
  141D54FA9  call    sub_142927BF0                           -> sub_142927BF0
  141D54FAE  mov     rcx, [r14]                            
  141D54FB1  lea     r8, [rbp+300h+arg_8]                  
  141D54FB8  movss   xmm0, cs:dword_1431EEE78                -> &dword_1431EEE78
  141D54FC0  mov     edx, 80000h                           
  141D54FC5  movss   xmm7, cs:Y                              -> &Y
  141D54FCD  xorps   xmm8, xmm8                            
  141D54FD1  mov     dword ptr [rsp+400h+var_390], r13d    
  141D54FD6  mov     esi, r13d                             
  141D54FD9  mov     [rcx+0E8C8B0h], eax                   
  141D54FDF  mov     rax, [r14]                            
  141D54FE2  mov     r15d, [rax+3EE2A0h]                   
  141D54FE9  lea     rbx, [rax+0AC63B0h]                   
  141D54FF0  mov     eax, 8000h                            
  141D54FF5  mov     [rbp+300h+Src], rbx                   
  141D54FF9  cmp     r15d, eax                             
  141D54FFC  cmova   r15d, eax                             
  141D55000  mov     rax, [r14+20h]                        
  141D55004  mov     [rbp+300h+var_328], r15d              
  141D55008  movd    xmm2, dword ptr [rax+70h]             
  141D5500D  movd    xmm4, dword ptr [rax+74h]             
  141D55012  movd    xmm3, dword ptr [rax+78h]             
  141D55017  mov     rax, [r14]                            
  141D5501A  cvtdq2ps xmm4, xmm4                           
  141D5501D  cvtdq2ps xmm2, xmm2                           
  141D55020  mulss   xmm4, xmm0                            
  141D55024  mulss   xmm2, xmm0                            
  141D55028  cvtdq2ps xmm3, xmm3                           
  141D5502B  movaps  xmm5, xmm2                            
  141D5502E  movss   xmm2, cs:dword_143480970                -> &dword_143480970
  141D55036  unpcklps xmm5, xmm4                           
  141D55039  movaps  xmm4, xmm2                            
  141D5503C  mulss   xmm3, xmm0                            
  141D55040  unpcklps xmm4, xmm2                           
  141D55043  movlhps xmm4, xmm2                            
  141D55046  unpcklps xmm3, xmm7                           
  141D55049  movlhps xmm5, xmm3                            
  141D5504C  movaps  xmm0, xmm5                            
  141D5504F  movaps  [rbp+300h+var_230], xmm5              
  141D55056  subps   xmm0, xmm4                            
  141D55059  addps   xmm4, xmm5                            
  141D5505C  movaps  [rbp+300h+var_100], xmm0              
  141D55063  movss   xmm0, cs:dword_1434809C0                -> &dword_1434809C0
  141D5506B  mov     [rax+178C4h], r13d                    
  141D55072  mov     rax, [r14]                            
  141D55075  movaps  xmmword ptr [rbp+210h], xmm4          
  141D5507C  movss   [rbp+300h+var_32C], xmm0              
  141D55081  mov     [rax+178C0h], r13d                    
  141D55088  mov     eax, r13d                             
  141D5508B  mov     rcx, [r14]                            
  141D5508E  cmp     [rcx+178C8h], eax                     
  141D55094  cmovbe  eax, edx                              
  141D55097  mov     [rcx+178C8h], eax                     
  141D5509D  lea     rax, off_142BC6538                      -> &off_142BC6538
  141D550A4  lea     rcx, [rbp+300h+var_300]               
  141D550A8  mov     [rbp+300h+arg_8], rax                 
  141D550AF  call    sub_14291CB7C                           -> sub_14291CB7C
  141D550B4  mov     rax, [r14]                            
  141D550B7  mov     [rax+1791Ch], r13d                    
  141D550BE  mov     rdi, [r14]                            
  141D550C1  add     rdi, 17940h                           
  141D550C8  mov     rcx, rdi; lpCriticalSection           
  141D550CB  mov     [rbp+300h+lpCriticalSection], rdi     
  141D550D2  call    cs:EnterCriticalSection                 -> &EnterCriticalSection
  141D550D8  mov     rax, [r14]                            
  141D550DB  mov     [rax+1792Ch], r13d                    
  141D550E2  mov     rax, [r14]                            
  141D550E5  mov     [rax+1793Ch], r13d                    
  141D550EC  xor     al, al                                
  141D550EE  mov     byte ptr [rbp+300h+arg_8], al         
  141D550F4  mov     rax, [r14]                            
  141D550F7  mov     [rax+3EE2A4h], r13d                   
  141D550FE  mov     eax, r13d                             
  141D55101  mov     dword ptr [rsp+400h+var_3A0], eax     
  141D55105  test    r15d, r15d                            
  141D55108  jz      loc_141D560B3                         
  141D5510E  movss   xmm5, dword ptr cs:xmmword_1431F1BE0    -> &xmmword_1431F1BE0
  141D55116  movss   xmm6, cs:dword_1431EEFB0                -> &dword_1431EEFB0
  141D5511E  mov     [rsp+3D0h], r12                       
  141D55126  movaps  [rsp+400h+var_88+8], xmm9             
  141D5512F  movaps  [rsp+400h+var_98+8], xmm10            
  141D55138  movaps  [rsp+400h+var_A8+8], xmm11            
  141D55141  movaps  [rsp+400h+var_B8+8], xmm12            
  141D5514A  movaps  [rsp+400h+var_C8+8], xmm13            
  141D55153  movaps  [rsp+400h+var_D8+8], xmm14            
  141D5515C  movaps  [rsp+400h+var_E8+8], xmm15            
  141D55165  nop     word ptr [rax+rax+00000000h]          
  141D55170  mov     r12, [r14]                            
  141D55173  lea     rdx, [rbp+300h+var_180]               
  141D5517A  mov     r9d, eax                              
  141D5517D  lea     rcx, [rbp+300h+var_100]               
  141D55184  add     r9, 36E2Ah                            
  141D5518B  mov     qword ptr [rbp+300h+var_2D0], r12     
  141D5518F  shl     r9, 4                                 
  141D55193  mov     r8, r12                               
  141D55196  add     r9, r12                               
  141D55199  mov     [rbp+300h+var_310], r9                
  141D5519D  mov     r11d, [r9+8]                          
  141D551A1  lea     rax, [r11+0BD5h]                      
  141D551A8  shl     rax, 5                                
  141D551AC  mov     r15, [rax+r12]                        
  141D551B0  mov     qword ptr [rbp+300h+var_2C0], r15     
  141D551B4  movss   xmm2, dword ptr [r15+64h]             
  141D551BA  movsd   xmm4, qword ptr [r15+5Ch]             
  141D551C0  movsd   xmm3, qword ptr [r15+50h]             
  141D551C6  unpcklps xmm2, xmm7                           
  141D551C9  movlhps xmm4, xmm2                            
  141D551CC  movss   xmm2, dword ptr [r15+58h]             
  141D551D2  unpcklps xmm2, xmm7                           
  141D551D5  movlhps xmm3, xmm2                            
  141D551D8  movaps  [rbp+300h+var_180], xmm3              
  141D551DF  movaps  [rbp+300h+var_170], xmm4              
  141D551E6  call    sub_1402DE074                           -> sub_1402DE074
  141D551EB  test    al, al                                
  141D551ED  jz      loc_141D56045                         
  141D551F3  mov     rax, [r12+17968h]                     
  141D551FB  cmp     r15, rax                              
  141D551FE  jnz     short loc_141D55209                   
  141D55200  mov     byte ptr [rbp+300h+arg_8], 1          
  141D55207  jmp     short loc_141D5521B                   
  141D55209  cmp     cs:byte_1432F8AD8, 0                    -> &byte_1432F8AD8
  141D55210  jz      short loc_141D5521B                   
  141D55212  test    rax, rax                              
  141D55215  jnz     loc_141D56045                         
  141D5521B  shl     r11, 5                                
  141D5521F  mov     [rbp+300h+var_380], r11               
  141D55223  movzx   eax, byte ptr [r11+r12+17AB4h]        
  141D5522C  and     al, 3                                 
  141D5522E  cmp     cs:byte_1432F7E28, 0                    -> &byte_1432F7E28
  141D55235  jz      short loc_141D5525D                   
  141D55237  mov     r8d, cs:dword_1434651D0                 -> &dword_1434651D0
  141D5523E  test    r8d, r8d                              
  141D55241  js      short loc_141D5525D                   
  141D55243  mov     rcx, r15                              
  141D55246  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5524B  cmp     r8d, eax                              
  141D5524E  jge     short loc_141D55256                   
  141D55250  movzx   eax, r8b                              
  141D55254  jmp     short loc_141D5525D                   
  141D55256  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5525B  dec     al                                    
  141D5525D  mov     rcx, [r11+r12+17AA0h]                 
  141D55265  movsx   eax, al                               
  141D55268  mov     ebx, eax                              
  141D5526A  mov     qword ptr [rbp+300h+var_270], rcx     
  141D55271  mov     rcx, [r9]                             
  141D55274  lea     rax, [rax+rax*2]                      
  141D55278  mov     [rbp+300h+var_378], rbx               
  141D5527C  lea     rdx, ds:0[rax*8]                      
  141D55284  mov     qword ptr [rbp+300h+var_280], rdx     
  141D5528B  mov     edi, [rcx+18h]                        
  141D5528E  mov     rcx, r15                              
  141D55291  mov     [rbp+300h+arg_18], edi                
  141D55297  call    sub_1402DCFF8                           -> sub_1402DCFF8
  141D5529C  mov     ecx, [rdx+rax+0Ch]                    
  141D552A0  mov     r9d, r13d                             
  141D552A3  movaps  xmm1, cs:xmmword_1431F1E60              -> &xmmword_1431F1E60
  141D552AA  movaps  xmm13, xmm8                           
  141D552AE  movss   xmm2, dword ptr [r15+40h]             
  141D552B4  movaps  xmm11, xmm1                           
  141D552B8  movsd   xmm12, qword ptr [r15+38h]            
  141D552BE  lea     r8, [rdx+rax]                         
  141D552C2  movsd   xmm4, qword ptr [r15+28h]             
  141D552C8  lea     rdx, [r15+118h]                       
  141D552CF  movsd   xmm3, qword ptr [r15+18h]             
  141D552D5  movaps  xmm14, xmm8                           
  141D552D9  mov     rax, [r15+0D8h]                       
  141D552E0  movaps  xmm15, xmm8                           
  141D552E4  unpcklps xmm2, xmm7                           
  141D552E7  movlhps xmm12, xmm2                           
  141D552EB  movss   xmm2, dword ptr [r15+30h]             
  141D552F1  mov     r10, [rax+130h]                       
  141D552F8  unpcklps xmm2, xmm7                           
  141D552FB  movlhps xmm4, xmm2                            
  141D552FE  movss   xmm2, dword ptr [r15+20h]             
  141D55304  unpcklps xmm2, xmm7                           
  141D55307  movlhps xmm3, xmm2                            
  141D5530A  movss   dword ptr [rsp+400h+var_390+4], xmm8  
  141D55311  mov     qword ptr [rsp+400h+var_398], r8      
  141D55316  mov     qword ptr [rsp+400h+var_3A8], rdx     
  141D5531B  mov     [rbp+300h+arg_10], r13d               
  141D55322  mov     [rbp+300h+var_330], ecx               
  141D55325  movaps  [rbp+300h+var_2E0], xmm1              
  141D55329  mov     [rbp+300h+var_370], r10               
  141D5532D  mov     dword ptr [rsp+400h+var_3B0+4], r13d  
  141D55332  dpps    xmm12, xmm12, 7Fh                     
  141D55339  movaps  xmm9, xmm12                           
  141D5533D  dpps    xmm4, xmm4, 7Fh                       
  141D55343  mulss   xmm9, xmm4                            
  141D55348  movaps  xmm0, xmm4                            
  141D5534B  dpps    xmm3, xmm3, 7Fh                       
  141D55351  mulss   xmm0, xmm3                            
  141D55355  mulss   xmm12, xmm3                           
  141D5535A  andps   xmm9, xmm5                            
  141D5535E  movss   dword ptr [rsp+400h+var_3B0], xmm9    
  141D55365  andps   xmm0, xmm5                            
  141D55368  movss   [rbp+300h+var_338], xmm0              
  141D5536D  movaps  xmm0, cs:xmmword_1431F1B90              -> &xmmword_1431F1B90
  141D55374  andps   xmm12, xmm5                           
  141D55378  movaps  [rbp+300h+var_2F0], xmm0              
  141D5537C  movaps  xmm10, xmm0                           
  141D55380  movaps  [rbp+300h+var_240], xmm12             
  141D55388  movaps  xmm0, xmm8                            
  141D5538C  movss   [rbp+300h+arg_0], xmm0                
  141D55394  test    ecx, ecx                              
  141D55396  jz      loc_141D55B23                         
  141D5539C  nop     dword ptr [rax+00h]                   
  141D553A0  mov     ecx, r13d                             
  141D553A3  mov     eax, 1                                
  141D553A8  shl     eax, cl                               
  141D553AA  movzx   eax, ax                               
  141D553AD  test    edi, eax                              
  141D553AF  jz      loc_141D55E1F                         
  141D553B5  movzx   eax, byte ptr [rdx+rbx*2+0Ah]         
  141D553BA  add     eax, r13d                             
  141D553BD  lea     rcx, [rax+rax*2]                      
  141D553C1  mov     rax, [rdx]                            
  141D553C4  add     rcx, rcx                              
  141D553C7  mov     rdx, [rax+rcx*8+8]                    
  141D553CC  movss   xmm5, dword ptr [rdx+50h]             
  141D553D1  movaps  xmm6, xmm5                            
  141D553D4  mulss   xmm6, cs:dword_1431EF0BC                -> &dword_1431EF0BC
  141D553DC  comiss  xmm6, xmm8                            
  141D553E0  jbe     loc_141D55E0B                         
  141D553E6  cmp     byte ptr [r10+20h], 0                 
  141D553EB  jz      short loc_141D553F8                   
  141D553ED  movss   xmm1, dword ptr [rdx+54h]             
  141D553F2  movss   dword ptr [rsp+400h+var_390+4], xmm1  
  141D553F8  mov     r9, [r8]                              
  141D553FB  xor     eax, eax                              
  141D553FD  mov     edx, [r10+0Ch]                        
  141D55401  movzx   r12d, r13w                            
  141D55405  mov     [rbp+300h+var_308], r9                
  141D55409  lea     rcx, [r12+r12*2]                      
  141D5540D  mov     [rbp+300h+var_318], rcx               
  141D55411  test    edx, edx                              
  141D55413  jz      loc_141D55E0B                         
  141D55419  mov     r8, [r10]                             
  141D5541C  movzx   edi, word ptr [r9+rcx*8+0Ah]          
  141D55422  cmp     [r8+rax*2], di                        
  141D55427  jz      short loc_141D55443                   
  141D55429  inc     eax                                   
  141D5542B  cmp     eax, edx                              
  141D5542D  jb      short loc_141D55422                   
  141D5542F  movss   xmm0, [rbp+300h+arg_0]                
  141D55437  mov     r9d, [rbp+300h+arg_10]                
  141D5543E  jmp     loc_141D55AF5                         
  141D55443  cmp     eax, 0FFFFFFFFh                       
  141D55446  jz      short loc_141D5542F                   
  141D55448  movss   xmm0, cs:flt_1431EF5A8; X               -> &flt_1431EF5A8
  141D55450  lea     rbx, [rax+rax*2]                      
  141D55454  shl     rbx, 4                                
  141D55458  movaps  xmm1, xmm5; Y                         
  141D5545B  add     rbx, [r10+10h]                        
```

## mirror SIMD fn (simd~65)  `sub_141D59BF0`  (RVA 0x1D59BF0, size 0xC8C, simd=3)
**Callers (4):** data 14312C748 ?, data 1432CF71C ?, data 1432CF77C ?, data 144AF0EC0 ?
**Callees:** sub_140153F94 x15, sub_142934058 x11, sub_1401F3A6C x9, sub_140777298 x6, sub_140776F8C x6, sub_1401F3D20 x5, sub_1401F881C x4, sub_14023AF5C x3, memset x3, sub_140774BE0 x2, sub_140A850D0 x2, sub_140AE5718 x1, sub_1401EE3CC x1, sub_142933EAC x1
```c
__int64 __fastcall sub_141D59BF0(__int64 a1, __int64 a2)
{
  char v3; // r12
  __int64 result; // rax
  int v5; // ebx
  int v6; // ebx
  bool v7; // zf
  int v8; // ebx
  int v9; // ebx
  int v10; // ebx
  int v11; // ebx
  int v12; // ebx
  int v13; // ebx
  int v14; // ebx
  int v15; // ebx
  int v16; // ebx
  int v17; // ebx
  __int64 v18; // r8
  __int64 v19; // r15
  __int64 v20; // r8
  int v21; // eax
  __int64 v22; // r9
  int v23; // eax
  __int64 v24; // r9
  int v25; // eax
  __int64 v26; // r9
  int v27; // eax
  __int64 v28; // r8
  int v29; // eax
  __int64 v30; // r9
  __int64 v31; // r9
  __int64 v32; // r9
  __int64 v33; // r9
  __int64 v34; // r9
  __int64 v35; // r9
  __int64 v36; // r8
  __int16 v37; // ax
  __int64 v38; // r8
  __int64 v39; // rax
  int v40; // r8d
  int v41; // edx
  __int64 v42; // rax
  int v43; // r8d
  int v44; // edx
  __int64 v45; // rax
  __int64 v46; // r8
  __int64 v47; // rdx
  __int64 v48; // rcx
  __int64 v49; // r8
  __int64 v50; // rax
  int v51; // r8d
  int v52; // edx
  __int64 v53; // rax
  __int64 v54; // r8
  __int64 v55; // rdx
  __int64 v56; // rcx
  int v57; // [rsp+20h] [rbp-E0h]
  int v58; // [rsp+20h] [rbp-E0h]
  int v59; // [rsp+20h] [rbp-E0h]
  __int64 v60; // [rsp+50h] [rbp-B0h] BYREF
  int v61; // [rsp+58h] [rbp-A8h] BYREF
  __int64 v62; // [rsp+60h] [rbp-A0h]
  int v63; // [rsp+68h] [rbp-98h] BYREF
  __int64 v64; // [rsp+70h] [rbp-90h]
  int v65; // [rsp+78h] [rbp-88h] BYREF
  __int64 v66; // [rsp+80h] [rbp-80h]
  int v67; // [rsp+88h] [rbp-78h] BYREF
  __int64 v68; // [rsp+90h] [rbp-70h]
  int v69; // [rsp+98h] [rbp-68h] BYREF
  __int64 v70; // [rsp+A0h] [rbp-60h] BYREF
  int v71; // [rsp+A8h] [rbp-58h] BYREF
  _OWORD Src[75]; // [rsp+B0h] [rbp-50h] BYREF
  __int64 v73; // [rsp+560h] [rbp+460h]
  __int128 v74; // [rsp+568h] [rbp+468h]
  __int64 v75; // [rsp+578h] [rbp+478h]
  __int128 v76; // [rsp+580h] [rbp+480h]
  __int128 v77; // [rsp+590h] [rbp+490h]
  __int128 v78; // [rsp+5A0h] [rbp+4A0h]
  __int64 v79; // [rsp+5B0h] [rbp+4B0h]
  __int64 v80; // [rsp+5B8h] [rbp+4B8h]
  __int128 v81; // [rsp+5C0h] [rbp+4C0h]
  __int128 v82; // [rsp+5D0h] [rbp+4D0h]
  __int128 v83; // [rsp+5E0h] [rbp+4E0h]
  __int128 v84; // [rsp+5F0h] [rbp+4F0h]
  __int128 v85; // [rsp+600h] [rbp+500h]
  __int128 v86; // [rsp+610h] [rbp+510h]
  __int128 v87; // [rsp+620h] [rbp+520h]
  __int128 v88; // [rsp+630h] [rbp+530h]
  __int128 v89; // [rsp+640h] [rbp+540h]
  __int128 v90; // [rsp+650h] [rbp+550h]
  __int128 v91; // [rsp+660h] [rbp+560h]
  _OWORD v92[113]; // [rsp+670h] [rbp+570h] BYREF
  int v93; // [rsp+DC8h] [rbp+CC8h] BYREF

  v3 = sub_14023AF5C(a2, 55);
  result = sub_14023AF5C(a2, 53);
  if ( (_BYTE)result )
  {
    result = sub_14023AF5C(a2, 54);
    if ( (_BYTE)result )
    {
      if ( (*(_BYTE *)(a2 + 48) & 1) != 0 )
        v5 = 0;
      else
        v5 = *(_DWORD *)(a2 + 52);
      v6 = (v5 << 24) ^ 0x61F178D4;
      v93 = v6;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 1) == 0;
      v60 = a2;
      v61 = v6;
      if ( v7 )
        v8 = *(_DWORD *)(a2 + 52);
      else
        v8 = 0;
      v9 = (v8 << 24) ^ 0xEC6C7F3;
      v93 = v9;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 1) == 0;
      v70 = a2;
      v71 = v9;
      if ( v7 )
        v10 = *(_DWORD *)(a2 + 52);
      else
        v10 = 0;
      v11 = (v10 << 24) ^ 0x15EAB19C;
      v93 = v11;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 1) == 0;
      v68 = a2;
      v69 = v11;
      if ( v7 )
        v12 = *(_DWORD *)(a2 + 52);
      else
        v12 = 0;
      v13 = (v12 << 24) ^ 0x63BCF380;
      v93 = v13;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 1) == 0;
      v66 = a2;
      v67 = v13;
      if ( v7 )
        v14 = *(_DWORD *)(a2 + 52);
      else
        v14 = 0;
      v15 = (v14 << 24) ^ 0x64BCF513;
      v93 = v15;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 1) == 0;
      v64 = a2;
      v65 = v15;
      if ( v7 )
        v16 = *(_DWORD *)(a2 + 52);
      else
        v16 = 0;
      v17 = (v16 << 24) ^ 0x65BCF6A6;
      v93 = v17;
      sub_140777298(a2, &v93, 1);
      v7 = (*(_BYTE *)(a2 + 48) & 2) == 0;
      v62 = a2;
      v63 = v17;
      if ( !v7 )
      {
        v19 = *(_QWORD *)(*(_QWORD *)(a2 + 24) + 7536LL);
        Src[0] = xmmword_1431EFE70;
        Src[1] = xmmword_1431EFFC0;
        Src[2] = xmmword_1431F0390;
        Src[3] = xmmword_1431F0E80;
        Src[4] = xmmword_1431EFE70;
        Src[5] = xmmword_1431EFFC0;
        Src[6] = xmmword_1431F0390;
        Src[7] = xmmword_1431F0E80;
        Src[8] = xmmword_1431EFE70;
        Src[9] = xmmword_1431EFFC0;
        Src[10] = xmmword_1431F0390;
        Src[11] = xmmword_1431F0E80;
        Src[12] = xmmword_1431EFE70;
        Src[13] = xmmword_1431EFFC0;
        Src[14] = xmmword_1431F0390;
        Src[15] = xmmword_1431F0E80;
        Src[16] = xmmword_1431EFE70;
        Src[17] = xmmword_1431EFFC0;
        Src[18] = xmmword_1431F0390;
        Src[19] = xmmword_1431F0E80;
        Src[20] = xmmword_1431EFE70;
        Src[21] = xmmword_1431EFFC0;
        Src[22] = xmmword_1431F0390;
        Src[23] = xmmword_1431F0E80;
        Src[24] = xmmword_1431EFE70;
        Src[25] = xmmword_1431EFFC0;
        Src[26] = xmmword_1431F0390;
        Src[27] = xmmword_1431F0E80;
        Src[28] = xmmword_1431EFE70;
        Src[29] = xmmword_1431EFFC0;
        Src[30] = xmmword_1431F0390;
        Src[31] = xmmword_1431F0E80;
        Src[32] = xmmword_1431EFE70;
        Src[33] = xmmword_1431EFFC0;
        Src[34] = xmmword_1431F0390;
        Src[35] = xmmword_1431F0E80;
        Src[36] = xmmword_1431EFE70;
  // ... (304 more lines)
```

## mirror SIMD fn (simd~8)  `sub_141D414D0`  (RVA 0x1D414D0, size 0x208, simd=3)
**Callers (4):** data 14312E098 ?, data 1432CE34C ?, data 1432CE35C ?, data 144AEFAEC ?
**Callees:** sub_14023AF5C x2, sub_14028DB28 x1, sub_140787D84 x1, sub_1408023AC x1, sub_14028E298 x1, sub_14078A124 x1, sub_141D57930 x1, sub_141D57A50 x1
```c
__int64 __fastcall sub_141D414D0(__int64 a1, _QWORD *a2)
{
  __int64 result; // rax
  int v4; // ecx
  __int64 v5; // rbx
  __int64 v6; // rax
  __int64 v7; // rcx
  int v8; // edx
  int v9; // r8d
  __int128 v10; // xmm1
  __int128 v11; // xmm0
  __int128 v12; // xmm1
  __int128 v13; // xmm0
  __int128 v14; // xmm1
  __int128 v15; // xmm0
  __int128 v16; // xmm1
  unsigned __int8 v17; // al
  __int64 v18; // rbx
  _OWORD v19[4]; // [rsp+30h] [rbp-3E8h] BYREF
  _BYTE v20[80]; // [rsp+70h] [rbp-3A8h] BYREF
  __int128 v21; // [rsp+C0h] [rbp-358h]
  __int128 v22; // [rsp+D0h] [rbp-348h]
  __int128 v23; // [rsp+E0h] [rbp-338h]
  __int128 v24; // [rsp+F0h] [rbp-328h]
  _BYTE v25[416]; // [rsp+180h] [rbp-298h] BYREF
  _BYTE v26[216]; // [rsp+320h] [rbp-F8h] BYREF
  _BYTE v27[32]; // [rsp+3F8h] [rbp-20h] BYREF

  result = qword_143427C00;
  if ( *(_QWORD *)(qword_143427C00 + 18048) )
  {
    result = (*(__int64 (__fastcall **)(_QWORD))(*(_QWORD *)*a2 + 32LL))(*a2);
    v4 = *(_DWORD *)(result + 3988);
    if ( !v4 || v4 == 4 && (result = sub_14023AF5C(a2, 70), (_BYTE)result) )
    {
      if ( (a2[6] & 2) != 0 )
      {
        v5 = *(_QWORD *)(qword_143427C00 + 18048);
        v6 = a2[4];
        if ( v6 && *(_DWORD *)(v6 + 84) )
        {
          result = a2[3];
          if ( (*(_DWORD *)(result + 6104) & 0x2000000) != 0 )
            return result;
          *(_DWORD *)(v5 + 184) = 0x40000000;
          *(_DWORD *)(v5 + 188) = -1073741824;
          *(_DWORD *)(v5 + 192) = (*(unsigned __int8 *)(a2[3] + 7492LL) >> 1) & 1;
          v7 = a2[3];
          *(_DWORD *)(v5 + 176) = *(_DWORD *)(v7 + 992);
          *(float *)(v5 + 180) = -*(float *)(v7 + 996);
          sub_14028DB28(v20, a2[3] + 112LL);
          sub_140787D84((unsigned int)v20, v8, v9, 1, 1, 0);
          sub_1408023AC(v19, v25, v26);
          v10 = v19[1];
          *(_OWORD *)(v5 + 112) = v19[0];
          v11 = v19[2];
          *(_OWORD *)(v5 + 128) = v10;
          v12 = v19[3];
          *(_OWORD *)(v5 + 144) = v11;
          v13 = v21;
          *(_OWORD *)(v5 + 160) = v12;
          v14 = v22;
          *(_OWORD *)(v5 + 48) = v13;
          v15 = v23;
          *(_OWORD *)(v5 + 64) = v14;
          v16 = v24;
          *(_OWORD *)(v5 + 80) = v15;
          *(_OWORD *)(v5 + 96) = v16;
          sub_14028E298(v27);
        }
        v17 = sub_14078A124(a2);
        sub_141D57930(*(_QWORD *)(qword_143427C00 + 18048), v17);
        result = sub_14023AF5C(a2, 74);
        if ( (_BYTE)result )
        {
          v18 = *(_QWORD *)(qword_143427C00 + 18048);
          (*(void (__fastcall **)(_QWORD))(*(_QWORD *)*a2 + 32LL))(*a2);
          return sub_141D57A50(v18);
        }
      }
    }
  }
  return result;
}
```

## mirror SIMD fn (simd~11)  `sub_141D4FDC0`  (RVA 0x1D4FDC0, size 0x64F, simd=0)
**Callers (4):** CALL 14037E2B5 sub_14037D5C4, data 1432CEDFC ?, data 1432CEE30 ?, data 144AF0464 ?
**Callees:** sub_14290FDA4 x13, sub_14023AF5C x2, sub_1407896C8 x1, sub_140C4FD0C x1, sub_14021B490 x1, sub_141D50410 x1, sub_141D50B30 x1, sub_1401F6400 x1, sub_14290FCF0 x1, sub_141D4F8E0 x1, sub_141D50BC0 x1, sub_141D4FA80 x1, sub_141D50D40 x1, sub_142A3E9D4 x1
```c
void __fastcall sub_141D4FDC0(
        __int64 a1,
        _QWORD *a2,
        int a3,
        int a4,
        int a5,
        int a6,
        int a7,
        int a8,
        int a9,
        int a10,
        int a11,
        int a12,
        int a13,
        int a14)
{
  unsigned __int8 v18; // al
  __int64 v19; // rdx
  __int64 v20; // xmm0_8
  __int64 v21; // rax
  int v22; // r14d
  int v23; // r13d
  int v24; // r12d
  int v25; // ecx
  int v26; // ecx
  int v27; // xmm1_4
  float v28; // xmm0_4
  unsigned __int8 v29; // r11
  unsigned __int8 v30; // al
  int v31; // r10d
  char v32; // r14
  int v33; // edx
  int v34; // edx
  int v35; // edx
  int v36; // edx
  int v37; // edx
  int v38; // edx
  int v39; // edx
  int v40; // edx
  int v41; // edx
  int v42; // edx
  int v43; // edx
  int v44; // r12d
  int v45; // r14d
  int v46; // edx
  __int64 v47; // r8
  int v48; // esi
  __int64 v49; // rbx
  int v50; // xmm6_4
  int v51; // eax
  __int128 v52; // xmm1
  __int128 v53; // xmm0
  __int128 v54; // xmm1
  __int128 v55; // xmm0
  __int128 v56; // xmm1
  __int128 v57; // xmm0
  __int128 v58; // xmm1
  __int128 v59; // xmm0
  __int64 v60; // rcx
  int v61; // ecx
  int v62; // eax
  int v63; // [rsp+50h] [rbp-B0h] BYREF
  __int64 v64; // [rsp+58h] [rbp-A8h] BYREF
  int v65; // [rsp+60h] [rbp-A0h]
  __int64 v66; // [rsp+68h] [rbp-98h] BYREF
  int v67; // [rsp+70h] [rbp-90h]
  int v68; // [rsp+78h] [rbp-88h]
  __int128 v69; // [rsp+80h] [rbp-80h] BYREF
  _BYTE v70[32]; // [rsp+A0h] [rbp-60h] BYREF
  int v71; // [rsp+C0h] [rbp-40h]
  int v72; // [rsp+C4h] [rbp-3Ch]
  int v73; // [rsp+C8h] [rbp-38h]
  int v74; // [rsp+D0h] [rbp-30h]
  int v75; // [rsp+D4h] [rbp-2Ch]
  char v76; // [rsp+D8h] [rbp-28h]
  int v77; // [rsp+DCh] [rbp-24h]
  int v78; // [rsp+E0h] [rbp-20h]
  int v79; // [rsp+E4h] [rbp-1Ch]
  int v80; // [rsp+E8h] [rbp-18h]
  int v81; // [rsp+ECh] [rbp-14h]
  int v82; // [rsp+F0h] [rbp-10h]
  bool v83; // [rsp+F4h] [rbp-Ch]
  _BYTE v84[32]; // [rsp+100h] [rbp+0h] BYREF
  int v85; // [rsp+120h] [rbp+20h]
  int v86; // [rsp+124h] [rbp+24h]
  int v87; // [rsp+128h] [rbp+28h]
  int v88; // [rsp+130h] [rbp+30h]
  int v89; // [rsp+134h] [rbp+34h]
  char v90; // [rsp+138h] [rbp+38h]
  int v91; // [rsp+13Ch] [rbp+3Ch]
  __int128 v92; // [rsp+140h] [rbp+40h]
  __int128 v93; // [rsp+150h] [rbp+50h]
  __int128 v94; // [rsp+160h] [rbp+60h]
  __int128 v95; // [rsp+170h] [rbp+70h]
  __int128 v96; // [rsp+180h] [rbp+80h]
  __int128 v97; // [rsp+190h] [rbp+90h]
  __int128 v98; // [rsp+1A0h] [rbp+A0h]
  __int128 v99; // [rsp+1B0h] [rbp+B0h]
  int v100; // [rsp+1C4h] [rbp+C4h]
  int v101; // [rsp+1C8h] [rbp+C8h]
  int v102; // [rsp+1CCh] [rbp+CCh]
  int v103; // [rsp+1D0h] [rbp+D0h]
  int v104; // [rsp+1D4h] [rbp+D4h]
  int v105; // [rsp+1D8h] [rbp+D8h]

  EnterCriticalSection(&stru_143480618);
  v18 = sub_14023AF5C(a2, 70);
  v65 = v18;
  if ( (unsigned __int8)sub_1407896C8(a1, v18) )
  {
    v68 = *(_DWORD *)((*(__int64 (__fastcall **)(_QWORD))(*(_QWORD *)*a2 + 32LL))(*a2) + 416);
    v19 = 176LL * (unsigned int)(a10 - 1);
    v20 = *(_QWORD *)(v19 + qword_143438A28 + 193062);
    v67 = *(_DWORD *)(v19 + qword_143438A28 + 193070);
    v21 = a2[3];
    v66 = v20;
    v22 = (unsigned __int16)v20;
    v63 = (unsigned __int16)v20;
    v23 = *(_DWORD *)(v21 + 56);
    v24 = *(_DWORD *)(v21 + 52);
    v25 = *(_DWORD *)(a1 + 980) ^ v23;
    *(_QWORD *)&v69 = 0;
    LODWORD(v21) = *(_DWORD *)(a1 + 976) ^ v24;
    LODWORD(v64) = WORD1(v20);
    v26 = *(_DWORD *)(a1 + 968) | *(_DWORD *)(a1 + 972) | v21 | v25;
    DWORD2(v69) = (unsigned __int16)v20;
    HIDWORD(v69) = WORD1(v20);
    if ( v26
      || *(_DWORD *)(a1 + 984)
       | *(_DWORD *)(a1 + 988)
       | *(_DWORD *)(a1 + 992) ^ (unsigned __int16)v20
       | *(_DWORD *)(a1 + 996) ^ WORD1(v20) )
    {
      v27 = *(_DWORD *)(a1 + 1028);
      sub_140C4FD0C(a1);
      v28 = sub_14021B490();
      if ( !(unsigned __int8)sub_141D50410(a1, v30, v29, *(_DWORD *)(a1 + 1008), v22, v64, v31, v27, LODWORD(v28)) )
        goto LABEL_19;
      *(_QWORD *)(a1 + 968) = 0;
      *(_DWORD *)(a1 + 976) = v24;
      *(_DWORD *)(a1 + 980) = v23;
      *(_OWORD *)(a1 + 984) = v69;
    }
    v32 = sub_14023AF5C(a2, 85);
    sub_14290FDA4(a1, v33, a3, 0, v24, v23, 1, 0);
    sub_14290FDA4(a1, v34, a4, 2, v24, v23, 0, v32);
    sub_14290FDA4(a1, v35, a6, 4, v24, v23, 1, 0);
    sub_14290FDA4(a1, v36, a5, 3, v24, v23, 0, v32);
    if ( (_BYTE)v65 )
    {
      sub_14290FDA4(a1, v37, a12, 14, v24, v23, 1, 0);
      sub_14290FDA4(a1, v38, a7, 5, v24, v23, 1, 0);
      sub_14290FDA4(a1, v39, a8, 6, v24, v23, 1, 0);
      sub_14290FDA4(a1, v40, a9, 7, v24, v23, 1, 0);
      sub_14290FDA4(a1, v41, a11, 11, v24, v23, 1, 0);
      sub_14290FDA4(a1, v42, a13, 12, v24, v23, 1, 0);
      sub_14290FDA4(a1, v43, a14, 15, v24, v23, 1, 0);
    }
    v44 = v64;
    v45 = v63;
    sub_14290FDA4(a1, v37, a10, 1, v63, v64, 1, 0);
    if ( !byte_1432F85D0 )
      sub_14290FDA4(a1, v46, *(_DWORD *)(*(_QWORD *)(a2[3] + 7552LL) + 28LL), 17, 1, 1, 0, 1);
    v48 = sub_141D50B30(*(unsigned __int8 *)(a1 + 1000), *(unsigned int *)(a1 + 1020), v47, WORD1(v66));
    if ( v48 )
    {
      v49 = sub_1401F6400();
      v50 = 1065353216;
      if ( !byte_1432F85D0 )
      {
        sub_14290FCF0(a1, &v69);
        v50 = v69;
      }
      if ( *(float *)&dword_143464D20 >= 0.0000099999997 )
        v50 = dword_143464D20;
      v63 = v68;
      v64 = 0;
      slGetNewFrameToken(&v64, &v63);
      v66 = a1 + 1144;
      if ( (_BYTE)v65 )
      {
        sub_141D4F8E0(v84);
        v51 = *(_DWORD *)(a1 + 1016);
        v52 = *(_OWORD *)(a1 + 304);
        v85 = v48;
        v89 = dword_143464D70;
        v53 = *(_OWORD *)(a1 + 288);
        v86 = v45;
        v93 = v52;
        v54 = *(_OWORD *)(a1 + 336);
        v87 = v44;
        v92 = v53;
        v55 = *(_OWORD *)(a1 + 320);
        v90 = 1;
        v95 = v54;
        v56 = *(_OWORD *)(a1 + 368);
        v91 = 1;
        v94 = v55;
        v57 = *(_OWORD *)(a1 + 352);
        v100 = v51;
        v97 = v56;
  // ... (43 more lines)
```

## PLANE / REFLECT-MATH search
Scanning mirror region 0x1D40000..0x1D60000 for: (a) reads of renderer global qword_143427C00 (main cam);
(b) negation/reflection patterns; (c) functions referencing 'plane'/'mirror' constants.
  sub_141D414D0 simd=3 readsRendererGlobal=True negOps=3 size=0x208
  sub_141D416E0 simd=0 readsRendererGlobal=True negOps=0 size=0x25D
  sub_141D43040 simd=0 readsRendererGlobal=True negOps=0 size=0x456F
  sub_141D49540 simd=1 readsRendererGlobal=True negOps=1 size=0x12DD
  sub_141D4BD30 simd=0 readsRendererGlobal=True negOps=0 size=0x5C0
  sub_141D4CC40 simd=11 readsRendererGlobal=True negOps=4 size=0x477
  sub_141D4D3F0 simd=0 readsRendererGlobal=True negOps=0 size=0xF2
  sub_141D4D640 simd=10 readsRendererGlobal=True negOps=6 size=0x58C
  sub_141D4E150 simd=7 readsRendererGlobal=True negOps=5 size=0x640
  sub_141D4E8B0 simd=0 readsRendererGlobal=True negOps=0 size=0x49B
  sub_141D51050 simd=1 readsRendererGlobal=True negOps=1 size=0x2A4
  sub_141D53070 simd=1 readsRendererGlobal=True negOps=1 size=0x200
  sub_141D53270 simd=1 readsRendererGlobal=True negOps=1 size=0xC2
  sub_141D533E0 simd=0 readsRendererGlobal=True negOps=0 size=0xB1
  sub_141D537D0 simd=0 readsRendererGlobal=True negOps=0 size=0x51
  sub_141D54DD0 simd=0 readsRendererGlobal=True negOps=0 size=0x103
  sub_141D54EE0 simd=227 readsRendererGlobal=True negOps=33 size=0x146C
  sub_141D56350 simd=0 readsRendererGlobal=True negOps=0 size=0xCB
  sub_141D56420 simd=0 readsRendererGlobal=True negOps=0 size=0xBC
  sub_141D56BA0 simd=0 readsRendererGlobal=True negOps=0 size=0x34
  sub_141D56BE0 simd=0 readsRendererGlobal=True negOps=0 size=0x34
  sub_141D56C20 simd=0 readsRendererGlobal=True negOps=0 size=0x39
  sub_141D56F00 simd=0 readsRendererGlobal=True negOps=0 size=0x3A
  sub_141D56F40 simd=1 readsRendererGlobal=True negOps=1 size=0x4E
  sub_141D56FD0 simd=0 readsRendererGlobal=True negOps=0 size=0x3A
  sub_141D57010 simd=0 readsRendererGlobal=True negOps=0 size=0x3A
  sub_141D57310 simd=0 readsRendererGlobal=True negOps=0 size=0x148
  sub_141D57A50 simd=0 readsRendererGlobal=True negOps=0 size=0xAF
  sub_141D59BF0 simd=3 readsRendererGlobal=True negOps=3 size=0xC8C
  sub_141D5B020 simd=27 readsRendererGlobal=False negOps=2 size=0x2CB

*(part 3 done)*


# PART 4 — THE REFLECT-MATH: sub_141D54EE0 (reflect main camera across mirror plane)

## Caller chain to sub_141D54EE0
  <- 141D56933 sub_141D56900
  <- 1432CF04C ?(data)
  <- 1432CF0A0 ?(data)
  <- 144AF0950 ?(data)

## Callees of sub_141D54EE0 (matrix/plane helpers)
  sub_14013F898 x3
  sub_1409BF14C x2
  sub_1402DCFF8 x2
  sub_14023AF5C x2
  sub_140109A44 x2
  memmove x2
  sub_1403749A4 x2
  sub_142927BF0 x1
  sub_14291CB7C x1
  sub_1402DE074 x1
  powf x1
  sub_14058E98C x1
  sub_14291FD34 x1
  sub_1402DD020 x1
  sub_1403A3A30 x1
  sub_14207E91C x1
  sub_142920D84 x1
  sub_14058D030 x1
  sub_14056C530 x1
  sub_1429202CC x1

## sub_141D54EE0 disasm (first 320 insns — renderer-global read + plane + reflect)
```
  141D54EE0  mov     rax, rsp                              
  141D54EE3  mov     [rax+10h], rdx                        
  141D54EE7  push    rbp                                   
  141D54EE8  push    rbx                                   
  141D54EE9  push    rsi                                   
  141D54EEA  push    rdi                                   
  141D54EEB  push    r13                                   
  141D54EED  push    r14                                   
  141D54EEF  lea     rbp, [rax-308h]                       
  141D54EF6  sub     rsp, 3D8h                             
  141D54EFD  mov     rdx, [rcx]                            
  141D54F00  xor     r13d, r13d                            
  141D54F03  mov     r10d, cs:dword_1434381C8                -> &dword_1434381C8
  141D54F0A  mov     r14, rcx                              
  141D54F0D  mov     r8, cs:qword_143438A28                  -> &qword_143438A28
  141D54F14  mov     [rax-40h], r15                        
  141D54F18  movzx   r9d, byte ptr [rdx+59h]               
  141D54F1D  movaps  xmmword ptr [rax-68h], xmm7           
  141D54F21  movaps  xmmword ptr [rax-78h], xmm8           
  141D54F26  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  141D54F2D  mov     rdi, [rax+4638h]                      
  141D54F34  test    r9b, r9b                              
  141D54F37  jz      short loc_141D54F54                   
  141D54F39  mov     eax, [rdx+r10*4+17890h]               
  141D54F41  dec     eax                                   
  141D54F43  imul    rcx, rax, 0B0h                        
  141D54F4A  mov     rbx, [rcx+r8+5C0B38h]                 
  141D54F52  jmp     short loc_141D54F57                   
  141D54F54  mov     rbx, r13                              
  141D54F57  mov     eax, [rdx+r10*4+17880h]               
  141D54F5F  mov     r11, r10                              
  141D54F62  dec     eax                                   
  141D54F64  imul    rcx, rax, 0B0h                        
  141D54F6B  mov     r10, [rcx+r8+5C0B38h]                 
  141D54F73  test    r9b, r9b                              
  141D54F76  jz      short loc_141D54F93                   
  141D54F78  mov     eax, [rdx+r11*4+17870h]               
  141D54F80  dec     eax                                   
  141D54F82  imul    rcx, rax, 0B0h                        
  141D54F89  mov     rdx, [rcx+r8+5C0B38h]                 
  141D54F91  jmp     short loc_141D54F96                   
  141D54F93  mov     rdx, r13                              
  141D54F96  mov     r9, r10                               
  141D54F99  movaps  [rsp+400h+var_58+8], xmm6             
  141D54FA1  mov     rcx, rdi                              
  141D54FA4  mov     [rsp+28h], rbx                        
  141D54FA9  call    sub_142927BF0                           -> sub_142927BF0
  141D54FAE  mov     rcx, [r14]                            
  141D54FB1  lea     r8, [rbp+300h+arg_8]                  
  141D54FB8  movss   xmm0, cs:dword_1431EEE78                -> &dword_1431EEE78
  141D54FC0  mov     edx, 80000h                           
  141D54FC5  movss   xmm7, cs:Y                              -> &Y
  141D54FCD  xorps   xmm8, xmm8                            
  141D54FD1  mov     dword ptr [rsp+400h+var_390], r13d    
  141D54FD6  mov     esi, r13d                             
  141D54FD9  mov     [rcx+0E8C8B0h], eax                   
  141D54FDF  mov     rax, [r14]                            
  141D54FE2  mov     r15d, [rax+3EE2A0h]                   
  141D54FE9  lea     rbx, [rax+0AC63B0h]                   
  141D54FF0  mov     eax, 8000h                            
  141D54FF5  mov     [rbp+300h+Src], rbx                   
  141D54FF9  cmp     r15d, eax                             
  141D54FFC  cmova   r15d, eax                             
  141D55000  mov     rax, [r14+20h]                        
  141D55004  mov     [rbp+300h+var_328], r15d              
  141D55008  movd    xmm2, dword ptr [rax+70h]             
  141D5500D  movd    xmm4, dword ptr [rax+74h]             
  141D55012  movd    xmm3, dword ptr [rax+78h]             
  141D55017  mov     rax, [r14]                            
  141D5501A  cvtdq2ps xmm4, xmm4                           
  141D5501D  cvtdq2ps xmm2, xmm2                           
  141D55020  mulss   xmm4, xmm0                            
  141D55024  mulss   xmm2, xmm0                            
  141D55028  cvtdq2ps xmm3, xmm3                           
  141D5502B  movaps  xmm5, xmm2                            
  141D5502E  movss   xmm2, cs:dword_143480970                -> &dword_143480970
  141D55036  unpcklps xmm5, xmm4                           
  141D55039  movaps  xmm4, xmm2                            
  141D5503C  mulss   xmm3, xmm0                            
  141D55040  unpcklps xmm4, xmm2                           
  141D55043  movlhps xmm4, xmm2                            
  141D55046  unpcklps xmm3, xmm7                           
  141D55049  movlhps xmm5, xmm3                            
  141D5504C  movaps  xmm0, xmm5                            
  141D5504F  movaps  [rbp+300h+var_230], xmm5              
  141D55056  subps   xmm0, xmm4                            
  141D55059  addps   xmm4, xmm5                            
  141D5505C  movaps  [rbp+300h+var_100], xmm0              
  141D55063  movss   xmm0, cs:dword_1434809C0                -> &dword_1434809C0
  141D5506B  mov     [rax+178C4h], r13d                    
  141D55072  mov     rax, [r14]                            
  141D55075  movaps  xmmword ptr [rbp+210h], xmm4          
  141D5507C  movss   [rbp+300h+var_32C], xmm0              
  141D55081  mov     [rax+178C0h], r13d                    
  141D55088  mov     eax, r13d                             
  141D5508B  mov     rcx, [r14]                            
  141D5508E  cmp     [rcx+178C8h], eax                     
  141D55094  cmovbe  eax, edx                              
  141D55097  mov     [rcx+178C8h], eax                     
  141D5509D  lea     rax, off_142BC6538                      -> &off_142BC6538
  141D550A4  lea     rcx, [rbp+300h+var_300]               
  141D550A8  mov     [rbp+300h+arg_8], rax                 
  141D550AF  call    sub_14291CB7C                           -> sub_14291CB7C
  141D550B4  mov     rax, [r14]                            
  141D550B7  mov     [rax+1791Ch], r13d                    
  141D550BE  mov     rdi, [r14]                            
  141D550C1  add     rdi, 17940h                           
  141D550C8  mov     rcx, rdi; lpCriticalSection           
  141D550CB  mov     [rbp+300h+lpCriticalSection], rdi     
  141D550D2  call    cs:EnterCriticalSection                 -> &EnterCriticalSection
  141D550D8  mov     rax, [r14]                            
  141D550DB  mov     [rax+1792Ch], r13d                    
  141D550E2  mov     rax, [r14]                            
  141D550E5  mov     [rax+1793Ch], r13d                    
  141D550EC  xor     al, al                                
  141D550EE  mov     byte ptr [rbp+300h+arg_8], al         
  141D550F4  mov     rax, [r14]                            
  141D550F7  mov     [rax+3EE2A4h], r13d                   
  141D550FE  mov     eax, r13d                             
  141D55101  mov     dword ptr [rsp+400h+var_3A0], eax     
  141D55105  test    r15d, r15d                            
  141D55108  jz      loc_141D560B3                         
  141D5510E  movss   xmm5, dword ptr cs:xmmword_1431F1BE0    -> &xmmword_1431F1BE0
  141D55116  movss   xmm6, cs:dword_1431EEFB0                -> &dword_1431EEFB0
  141D5511E  mov     [rsp+3D0h], r12                       
  141D55126  movaps  [rsp+400h+var_88+8], xmm9             
  141D5512F  movaps  [rsp+400h+var_98+8], xmm10            
  141D55138  movaps  [rsp+400h+var_A8+8], xmm11            
  141D55141  movaps  [rsp+400h+var_B8+8], xmm12            
  141D5514A  movaps  [rsp+400h+var_C8+8], xmm13            
  141D55153  movaps  [rsp+400h+var_D8+8], xmm14            
  141D5515C  movaps  [rsp+400h+var_E8+8], xmm15            
  141D55165  nop     word ptr [rax+rax+00000000h]          
  141D55170  mov     r12, [r14]                            
  141D55173  lea     rdx, [rbp+300h+var_180]               
  141D5517A  mov     r9d, eax                              
  141D5517D  lea     rcx, [rbp+300h+var_100]               
  141D55184  add     r9, 36E2Ah                            
  141D5518B  mov     qword ptr [rbp+300h+var_2D0], r12     
  141D5518F  shl     r9, 4                                 
  141D55193  mov     r8, r12                               
  141D55196  add     r9, r12                               
  141D55199  mov     [rbp+300h+var_310], r9                
  141D5519D  mov     r11d, [r9+8]                          
  141D551A1  lea     rax, [r11+0BD5h]                      
  141D551A8  shl     rax, 5                                
  141D551AC  mov     r15, [rax+r12]                        
  141D551B0  mov     qword ptr [rbp+300h+var_2C0], r15     
  141D551B4  movss   xmm2, dword ptr [r15+64h]             
  141D551BA  movsd   xmm4, qword ptr [r15+5Ch]             
  141D551C0  movsd   xmm3, qword ptr [r15+50h]             
  141D551C6  unpcklps xmm2, xmm7                           
  141D551C9  movlhps xmm4, xmm2                            
  141D551CC  movss   xmm2, dword ptr [r15+58h]             
  141D551D2  unpcklps xmm2, xmm7                           
  141D551D5  movlhps xmm3, xmm2                            
  141D551D8  movaps  [rbp+300h+var_180], xmm3              
  141D551DF  movaps  [rbp+300h+var_170], xmm4              
  141D551E6  call    sub_1402DE074                           -> sub_1402DE074
  141D551EB  test    al, al                                
  141D551ED  jz      loc_141D56045                         
  141D551F3  mov     rax, [r12+17968h]                     
  141D551FB  cmp     r15, rax                              
  141D551FE  jnz     short loc_141D55209                   
  141D55200  mov     byte ptr [rbp+300h+arg_8], 1          
  141D55207  jmp     short loc_141D5521B                   
  141D55209  cmp     cs:byte_1432F8AD8, 0                    -> &byte_1432F8AD8
  141D55210  jz      short loc_141D5521B                   
  141D55212  test    rax, rax                              
  141D55215  jnz     loc_141D56045                         
  141D5521B  shl     r11, 5                                
  141D5521F  mov     [rbp+300h+var_380], r11               
  141D55223  movzx   eax, byte ptr [r11+r12+17AB4h]        
  141D5522C  and     al, 3                                 
  141D5522E  cmp     cs:byte_1432F7E28, 0                    -> &byte_1432F7E28
  141D55235  jz      short loc_141D5525D                   
  141D55237  mov     r8d, cs:dword_1434651D0                 -> &dword_1434651D0
  141D5523E  test    r8d, r8d                              
  141D55241  js      short loc_141D5525D                   
  141D55243  mov     rcx, r15                              
  141D55246  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5524B  cmp     r8d, eax                              
  141D5524E  jge     short loc_141D55256                   
  141D55250  movzx   eax, r8b                              
  141D55254  jmp     short loc_141D5525D                   
  141D55256  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5525B  dec     al                                    
  141D5525D  mov     rcx, [r11+r12+17AA0h]                 
  141D55265  movsx   eax, al                               
  141D55268  mov     ebx, eax                              
  141D5526A  mov     qword ptr [rbp+300h+var_270], rcx     
  141D55271  mov     rcx, [r9]                             
  141D55274  lea     rax, [rax+rax*2]                      
  141D55278  mov     [rbp+300h+var_378], rbx               
  141D5527C  lea     rdx, ds:0[rax*8]                      
  141D55284  mov     qword ptr [rbp+300h+var_280], rdx     
  141D5528B  mov     edi, [rcx+18h]                        
  141D5528E  mov     rcx, r15                              
  141D55291  mov     [rbp+300h+arg_18], edi                
  141D55297  call    sub_1402DCFF8                           -> sub_1402DCFF8
  141D5529C  mov     ecx, [rdx+rax+0Ch]                    
  141D552A0  mov     r9d, r13d                             
  141D552A3  movaps  xmm1, cs:xmmword_1431F1E60              -> &xmmword_1431F1E60
  141D552AA  movaps  xmm13, xmm8                           
  141D552AE  movss   xmm2, dword ptr [r15+40h]             
  141D552B4  movaps  xmm11, xmm1                           
  141D552B8  movsd   xmm12, qword ptr [r15+38h]            
  141D552BE  lea     r8, [rdx+rax]                         
  141D552C2  movsd   xmm4, qword ptr [r15+28h]             
  141D552C8  lea     rdx, [r15+118h]                       
  141D552CF  movsd   xmm3, qword ptr [r15+18h]             
  141D552D5  movaps  xmm14, xmm8                           
  141D552D9  mov     rax, [r15+0D8h]                       
  141D552E0  movaps  xmm15, xmm8                           
  141D552E4  unpcklps xmm2, xmm7                           
  141D552E7  movlhps xmm12, xmm2                           
  141D552EB  movss   xmm2, dword ptr [r15+30h]             
  141D552F1  mov     r10, [rax+130h]                       
  141D552F8  unpcklps xmm2, xmm7                           
  141D552FB  movlhps xmm4, xmm2                            
  141D552FE  movss   xmm2, dword ptr [r15+20h]             
  141D55304  unpcklps xmm2, xmm7                           
  141D55307  movlhps xmm3, xmm2                            
  141D5530A  movss   dword ptr [rsp+400h+var_390+4], xmm8  
  141D55311  mov     qword ptr [rsp+400h+var_398], r8      
  141D55316  mov     qword ptr [rsp+400h+var_3A8], rdx     
  141D5531B  mov     [rbp+300h+arg_10], r13d               
  141D55322  mov     [rbp+300h+var_330], ecx               
  141D55325  movaps  [rbp+300h+var_2E0], xmm1              
  141D55329  mov     [rbp+300h+var_370], r10               
  141D5532D  mov     dword ptr [rsp+400h+var_3B0+4], r13d  
  141D55332  dpps    xmm12, xmm12, 7Fh                     
  141D55339  movaps  xmm9, xmm12                           
  141D5533D  dpps    xmm4, xmm4, 7Fh                       
  141D55343  mulss   xmm9, xmm4                            
  141D55348  movaps  xmm0, xmm4                            
  141D5534B  dpps    xmm3, xmm3, 7Fh                       
  141D55351  mulss   xmm0, xmm3                            
  141D55355  mulss   xmm12, xmm3                           
  141D5535A  andps   xmm9, xmm5                            
  141D5535E  movss   dword ptr [rsp+400h+var_3B0], xmm9    
  141D55365  andps   xmm0, xmm5                            
  141D55368  movss   [rbp+300h+var_338], xmm0              
  141D5536D  movaps  xmm0, cs:xmmword_1431F1B90              -> &xmmword_1431F1B90
  141D55374  andps   xmm12, xmm5                           
  141D55378  movaps  [rbp+300h+var_2F0], xmm0              
  141D5537C  movaps  xmm10, xmm0                           
  141D55380  movaps  [rbp+300h+var_240], xmm12             
  141D55388  movaps  xmm0, xmm8                            
  141D5538C  movss   [rbp+300h+arg_0], xmm0                
  141D55394  test    ecx, ecx                              
  141D55396  jz      loc_141D55B23                         
  141D5539C  nop     dword ptr [rax+00h]                   
  141D553A0  mov     ecx, r13d                             
  141D553A3  mov     eax, 1                                
  141D553A8  shl     eax, cl                               
  141D553AA  movzx   eax, ax                               
  141D553AD  test    edi, eax                              
  141D553AF  jz      loc_141D55E1F                         
  141D553B5  movzx   eax, byte ptr [rdx+rbx*2+0Ah]         
  141D553BA  add     eax, r13d                             
  141D553BD  lea     rcx, [rax+rax*2]                      
  141D553C1  mov     rax, [rdx]                            
  141D553C4  add     rcx, rcx                              
  141D553C7  mov     rdx, [rax+rcx*8+8]                    
  141D553CC  movss   xmm5, dword ptr [rdx+50h]             
  141D553D1  movaps  xmm6, xmm5                            
  141D553D4  mulss   xmm6, cs:dword_1431EF0BC                -> &dword_1431EF0BC
  141D553DC  comiss  xmm6, xmm8                            
  141D553E0  jbe     loc_141D55E0B                         
  141D553E6  cmp     byte ptr [r10+20h], 0                 
  141D553EB  jz      short loc_141D553F8                   
  141D553ED  movss   xmm1, dword ptr [rdx+54h]             
  141D553F2  movss   dword ptr [rsp+400h+var_390+4], xmm1  
  141D553F8  mov     r9, [r8]                              
  141D553FB  xor     eax, eax                              
  141D553FD  mov     edx, [r10+0Ch]                        
  141D55401  movzx   r12d, r13w                            
  141D55405  mov     [rbp+300h+var_308], r9                
  141D55409  lea     rcx, [r12+r12*2]                      
  141D5540D  mov     [rbp+300h+var_318], rcx               
  141D55411  test    edx, edx                              
  141D55413  jz      loc_141D55E0B                         
  141D55419  mov     r8, [r10]                             
  141D5541C  movzx   edi, word ptr [r9+rcx*8+0Ah]          
  141D55422  cmp     [r8+rax*2], di                        
  141D55427  jz      short loc_141D55443                   
  141D55429  inc     eax                                   
  141D5542B  cmp     eax, edx                              
  141D5542D  jb      short loc_141D55422                   
  141D5542F  movss   xmm0, [rbp+300h+arg_0]                
  141D55437  mov     r9d, [rbp+300h+arg_10]                
  141D5543E  jmp     loc_141D55AF5                         
  141D55443  cmp     eax, 0FFFFFFFFh                       
  141D55446  jz      short loc_141D5542F                   
  141D55448  movss   xmm0, cs:flt_1431EF5A8; X               -> &flt_1431EF5A8
  141D55450  lea     rbx, [rax+rax*2]                      
  141D55454  shl     rbx, 4                                
  141D55458  movaps  xmm1, xmm5; Y                         
  141D5545B  add     rbx, [r10+10h]                        
  141D5545F  movss   xmm2, dword ptr [rbx+8]               
  141D55464  movsd   xmm3, qword ptr [rbx]                 
  141D55468  movsd   xmm4, qword ptr [rbx+0Ch]             
  141D5546D  unpcklps xmm2, xmm7                           
  141D55470  movlhps xmm3, xmm2                            
  141D55473  minps   xmm10, xmm3                           
  141D55477  movss   xmm3, dword ptr [rbx+14h]             
  141D5547C  unpcklps xmm3, xmm7                           
  141D5547F  movlhps xmm4, xmm3                            
  141D55482  maxps   xmm11, xmm4                           
  141D55486  movaps  [rbp+300h+var_220], xmm10             
  141D5548E  movaps  [rbp+300h+var_2E0], xmm11             
  141D55493  movaps  [rbp+300h+var_2F0], xmm10             
  141D55498  call    powf                                    -> powf
  141D5549D  mulss   xmm6, cs:dword_1431EF7E8                -> &dword_1431EF7E8
  141D554A5  lea     rcx, [r14+8]                          
  141D554A9  mov     r9d, [rbp+300h+arg_10]                
  141D554B0  mov     edx, 2Eh ; '.'                        
  141D554B5  mulss   xmm9, dword ptr [rbx+24h]             
  141D554BB  movaps  xmm3, xmm0                            
```

## caller sub_141D53FA0 (mirror render entry?)  sub_141D53FA0 (size 0xF5)
```c
__int64 __fastcall sub_141D53FA0(__int64 a1, char a2, __int64 *a3, __int64 *a4, _QWORD *a5)
{
  _QWORD *v8; // rax
  __int64 v9; // r8
  _QWORD v11[3]; // [rsp+20h] [rbp-28h] BYREF
  int v12; // [rsp+38h] [rbp-10h]
  __int16 v13; // [rsp+3Ch] [rbp-Ch]

  if ( dword_144854200 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_144854200);
    if ( dword_144854200 == -1 )
    {
      sub_140624838(&unk_144854210, "BuildTopLevelAccelerationStructure_Emissive", 0);
      atexit(nullsub_5456);
      Init_thread_footer(&dword_144854200);
    }
  }
  v12 = 0;
  v13 = 0;
  v11[0] = sub_141D56900;
  v8 = (_QWORD *)sub_140142C9C(0x50u, 1);
  *v8 = *a5;
  sub_1401EC7EC(v8 + 1, a5 + 1, v8);
  v11[1] = v9;
  v11[2] = &unk_144854210;
  return sub_14095EC88((int)v11, a2, a3, a4);
}
```

## wrapper sub_141D56900  sub_141D56900 (size 0x7E)
```c
PVOID __fastcall sub_141D56900(__int64 a1, __int64 a2)
{
  __int64 v2; // r9
  PVOID result; // rax
  __int64 v5; // rcx
  __int64 v6[3]; // [rsp+20h] [rbp-18h] BYREF

  v2 = *(_QWORD *)(a2 + 32);
  if ( v2 )
    *(_BYTE *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 410LL) = *(_BYTE *)(*(_QWORD *)v2 + 33LL);
  sub_141D54EE0();
  result = NtCurrentTeb()->ThreadLocalStoragePointer;
  *(_BYTE *)(*(_QWORD *)result + 410LL) = -1;
  if ( a1 )
  {
    v5 = *(_QWORD *)(a1 + 8);
    if ( v5 )
      sub_1401E6EB0(v5);
    v6[0] = a1;
    v6[1] = 0;
    return (PVOID)sub_1402F0DE8(v6);
  }
  return result;
}
```


# PART 5 — sub_141D54EE0 FULL reverse (mirror reflect math: inputs/plane/output)

## caller sub_141D53FA0  (size 0xF5)
```c
__int64 __fastcall sub_141D53FA0(__int64 a1, char a2, __int64 *a3, __int64 *a4, _QWORD *a5)
{
  _QWORD *v8; // rax
  __int64 v9; // r8
  _QWORD v11[3]; // [rsp+20h] [rbp-28h] BYREF
  int v12; // [rsp+38h] [rbp-10h]
  __int16 v13; // [rsp+3Ch] [rbp-Ch]

  if ( dword_144854200 > *(_DWORD *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 16LL) )
  {
    Init_thread_header(&dword_144854200);
    if ( dword_144854200 == -1 )
    {
      sub_140624838(&unk_144854210, "BuildTopLevelAccelerationStructure_Emissive", 0);
      atexit(nullsub_5456);
      Init_thread_footer(&dword_144854200);
    }
  }
  v12 = 0;
  v13 = 0;
  v11[0] = sub_141D56900;
  v8 = (_QWORD *)sub_140142C9C(0x50u, 1);
  *v8 = *a5;
  sub_1401EC7EC(v8 + 1, a5 + 1, v8);
  v11[1] = v9;
  v11[2] = &unk_144854210;
  return sub_14095EC88((int)v11, a2, a3, a4);
}
```

## wrapper sub_141D56900  (size 0x7E)
```c
PVOID __fastcall sub_141D56900(__int64 a1, __int64 a2)
{
  __int64 v2; // r9
  PVOID result; // rax
  __int64 v5; // rcx
  __int64 v6[3]; // [rsp+20h] [rbp-18h] BYREF

  v2 = *(_QWORD *)(a2 + 32);
  if ( v2 )
    *(_BYTE *)(*(_QWORD *)NtCurrentTeb()->ThreadLocalStoragePointer + 410LL) = *(_BYTE *)(*(_QWORD *)v2 + 33LL);
  sub_141D54EE0();
  result = NtCurrentTeb()->ThreadLocalStoragePointer;
  *(_BYTE *)(*(_QWORD *)result + 410LL) = -1;
  if ( a1 )
  {
    v5 = *(_QWORD *)(a1 + 8);
    if ( v5 )
      sub_1401E6EB0(v5);
    v6[0] = a1;
    v6[1] = 0;
    return (PVOID)sub_1402F0DE8(v6);
  }
  return result;
}
```

## sub_141D54EE0 FULL disasm (heap WRITES flagged, call/data targets annotated)
```
  141D54EE0  mov     rax, rsp                              
  141D54EE3  mov     [rax+10h], rdx                           <== HEAP WRITE
  141D54EE7  push    rbp                                   
  141D54EE8  push    rbx                                   
  141D54EE9  push    rsi                                   
  141D54EEA  push    rdi                                   
  141D54EEB  push    r13                                   
  141D54EED  push    r14                                   
  141D54EEF  lea     rbp, [rax-308h]                       
  141D54EF6  sub     rsp, 3D8h                             
  141D54EFD  mov     rdx, [rcx]                            
  141D54F00  xor     r13d, r13d                            
  141D54F03  mov     r10d, cs:dword_1434381C8                -> &dword_1434381C8
  141D54F0A  mov     r14, rcx                              
  141D54F0D  mov     r8, cs:qword_143438A28                  -> &qword_143438A28
  141D54F14  mov     [rax-40h], r15                           <== HEAP WRITE
  141D54F18  movzx   r9d, byte ptr [rdx+59h]               
  141D54F1D  movaps  xmmword ptr [rax-68h], xmm7              <== HEAP WRITE
  141D54F21  movaps  xmmword ptr [rax-78h], xmm8              <== HEAP WRITE
  141D54F26  mov     rax, cs:qword_143427C00                 -> &qword_143427C00
  141D54F2D  mov     rdi, [rax+4638h]                      
  141D54F34  test    r9b, r9b                              
  141D54F37  jz      short loc_141D54F54                   
  141D54F39  mov     eax, [rdx+r10*4+17890h]               
  141D54F41  dec     eax                                   
  141D54F43  imul    rcx, rax, 0B0h                        
  141D54F4A  mov     rbx, [rcx+r8+5C0B38h]                 
  141D54F52  jmp     short loc_141D54F57                   
  141D54F54  mov     rbx, r13                              
  141D54F57  mov     eax, [rdx+r10*4+17880h]               
  141D54F5F  mov     r11, r10                              
  141D54F62  dec     eax                                   
  141D54F64  imul    rcx, rax, 0B0h                        
  141D54F6B  mov     r10, [rcx+r8+5C0B38h]                 
  141D54F73  test    r9b, r9b                              
  141D54F76  jz      short loc_141D54F93                   
  141D54F78  mov     eax, [rdx+r11*4+17870h]               
  141D54F80  dec     eax                                   
  141D54F82  imul    rcx, rax, 0B0h                        
  141D54F89  mov     rdx, [rcx+r8+5C0B38h]                 
  141D54F91  jmp     short loc_141D54F96                   
  141D54F93  mov     rdx, r13                              
  141D54F96  mov     r9, r10                               
  141D54F99  movaps  [rsp+400h+var_58+8], xmm6             
  141D54FA1  mov     rcx, rdi                              
  141D54FA4  mov     [rsp+28h], rbx                        
  141D54FA9  call    sub_142927BF0                           -> sub_142927BF0
  141D54FAE  mov     rcx, [r14]                            
  141D54FB1  lea     r8, [rbp+300h+arg_8]                  
  141D54FB8  movss   xmm0, cs:dword_1431EEE78                -> &dword_1431EEE78
  141D54FC0  mov     edx, 80000h                           
  141D54FC5  movss   xmm7, cs:Y                              -> &Y
  141D54FCD  xorps   xmm8, xmm8                            
  141D54FD1  mov     dword ptr [rsp+400h+var_390], r13d    
  141D54FD6  mov     esi, r13d                             
  141D54FD9  mov     [rcx+0E8C8B0h], eax                      <== HEAP WRITE
  141D54FDF  mov     rax, [r14]                            
  141D54FE2  mov     r15d, [rax+3EE2A0h]                   
  141D54FE9  lea     rbx, [rax+0AC63B0h]                   
  141D54FF0  mov     eax, 8000h                            
  141D54FF5  mov     [rbp+300h+Src], rbx                   
  141D54FF9  cmp     r15d, eax                             
  141D54FFC  cmova   r15d, eax                             
  141D55000  mov     rax, [r14+20h]                        
  141D55004  mov     [rbp+300h+var_328], r15d              
  141D55008  movd    xmm2, dword ptr [rax+70h]             
  141D5500D  movd    xmm4, dword ptr [rax+74h]             
  141D55012  movd    xmm3, dword ptr [rax+78h]             
  141D55017  mov     rax, [r14]                            
  141D5501A  cvtdq2ps xmm4, xmm4                           
  141D5501D  cvtdq2ps xmm2, xmm2                           
  141D55020  mulss   xmm4, xmm0                            
  141D55024  mulss   xmm2, xmm0                            
  141D55028  cvtdq2ps xmm3, xmm3                           
  141D5502B  movaps  xmm5, xmm2                            
  141D5502E  movss   xmm2, cs:dword_143480970                -> &dword_143480970
  141D55036  unpcklps xmm5, xmm4                           
  141D55039  movaps  xmm4, xmm2                            
  141D5503C  mulss   xmm3, xmm0                            
  141D55040  unpcklps xmm4, xmm2                           
  141D55043  movlhps xmm4, xmm2                            
  141D55046  unpcklps xmm3, xmm7                           
  141D55049  movlhps xmm5, xmm3                            
  141D5504C  movaps  xmm0, xmm5                            
  141D5504F  movaps  [rbp+300h+var_230], xmm5              
  141D55056  subps   xmm0, xmm4                            
  141D55059  addps   xmm4, xmm5                            
  141D5505C  movaps  [rbp+300h+var_100], xmm0              
  141D55063  movss   xmm0, cs:dword_1434809C0                -> &dword_1434809C0
  141D5506B  mov     [rax+178C4h], r13d                       <== HEAP WRITE
  141D55072  mov     rax, [r14]                            
  141D55075  movaps  xmmword ptr [rbp+210h], xmm4          
  141D5507C  movss   [rbp+300h+var_32C], xmm0              
  141D55081  mov     [rax+178C0h], r13d                       <== HEAP WRITE
  141D55088  mov     eax, r13d                             
  141D5508B  mov     rcx, [r14]                            
  141D5508E  cmp     [rcx+178C8h], eax                     
  141D55094  cmovbe  eax, edx                              
  141D55097  mov     [rcx+178C8h], eax                        <== HEAP WRITE
  141D5509D  lea     rax, off_142BC6538                      -> &off_142BC6538
  141D550A4  lea     rcx, [rbp+300h+var_300]               
  141D550A8  mov     [rbp+300h+arg_8], rax                 
  141D550AF  call    sub_14291CB7C                           -> sub_14291CB7C
  141D550B4  mov     rax, [r14]                            
  141D550B7  mov     [rax+1791Ch], r13d                       <== HEAP WRITE
  141D550BE  mov     rdi, [r14]                            
  141D550C1  add     rdi, 17940h                           
  141D550C8  mov     rcx, rdi; lpCriticalSection           
  141D550CB  mov     [rbp+300h+lpCriticalSection], rdi     
  141D550D2  call    cs:EnterCriticalSection                 -> &EnterCriticalSection
  141D550D8  mov     rax, [r14]                            
  141D550DB  mov     [rax+1792Ch], r13d                       <== HEAP WRITE
  141D550E2  mov     rax, [r14]                            
  141D550E5  mov     [rax+1793Ch], r13d                       <== HEAP WRITE
  141D550EC  xor     al, al                                
  141D550EE  mov     byte ptr [rbp+300h+arg_8], al         
  141D550F4  mov     rax, [r14]                            
  141D550F7  mov     [rax+3EE2A4h], r13d                      <== HEAP WRITE
  141D550FE  mov     eax, r13d                             
  141D55101  mov     dword ptr [rsp+400h+var_3A0], eax     
  141D55105  test    r15d, r15d                            
  141D55108  jz      loc_141D560B3                         
  141D5510E  movss   xmm5, dword ptr cs:xmmword_1431F1BE0    -> &xmmword_1431F1BE0
  141D55116  movss   xmm6, cs:dword_1431EEFB0                -> &dword_1431EEFB0
  141D5511E  mov     [rsp+3D0h], r12                       
  141D55126  movaps  [rsp+400h+var_88+8], xmm9             
  141D5512F  movaps  [rsp+400h+var_98+8], xmm10            
  141D55138  movaps  [rsp+400h+var_A8+8], xmm11            
  141D55141  movaps  [rsp+400h+var_B8+8], xmm12            
  141D5514A  movaps  [rsp+400h+var_C8+8], xmm13            
  141D55153  movaps  [rsp+400h+var_D8+8], xmm14            
  141D5515C  movaps  [rsp+400h+var_E8+8], xmm15            
  141D55165  nop     word ptr [rax+rax+00000000h]          
  141D55170  mov     r12, [r14]                            
  141D55173  lea     rdx, [rbp+300h+var_180]               
  141D5517A  mov     r9d, eax                              
  141D5517D  lea     rcx, [rbp+300h+var_100]               
  141D55184  add     r9, 36E2Ah                            
  141D5518B  mov     qword ptr [rbp+300h+var_2D0], r12     
  141D5518F  shl     r9, 4                                 
  141D55193  mov     r8, r12                               
  141D55196  add     r9, r12                               
  141D55199  mov     [rbp+300h+var_310], r9                
  141D5519D  mov     r11d, [r9+8]                          
  141D551A1  lea     rax, [r11+0BD5h]                      
  141D551A8  shl     rax, 5                                
  141D551AC  mov     r15, [rax+r12]                        
  141D551B0  mov     qword ptr [rbp+300h+var_2C0], r15     
  141D551B4  movss   xmm2, dword ptr [r15+64h]             
  141D551BA  movsd   xmm4, qword ptr [r15+5Ch]             
  141D551C0  movsd   xmm3, qword ptr [r15+50h]             
  141D551C6  unpcklps xmm2, xmm7                           
  141D551C9  movlhps xmm4, xmm2                            
  141D551CC  movss   xmm2, dword ptr [r15+58h]             
  141D551D2  unpcklps xmm2, xmm7                           
  141D551D5  movlhps xmm3, xmm2                            
  141D551D8  movaps  [rbp+300h+var_180], xmm3              
  141D551DF  movaps  [rbp+300h+var_170], xmm4              
  141D551E6  call    sub_1402DE074                           -> sub_1402DE074
  141D551EB  test    al, al                                
  141D551ED  jz      loc_141D56045                         
  141D551F3  mov     rax, [r12+17968h]                     
  141D551FB  cmp     r15, rax                              
  141D551FE  jnz     short loc_141D55209                   
  141D55200  mov     byte ptr [rbp+300h+arg_8], 1          
  141D55207  jmp     short loc_141D5521B                   
  141D55209  cmp     cs:byte_1432F8AD8, 0                    -> &byte_1432F8AD8
  141D55210  jz      short loc_141D5521B                   
  141D55212  test    rax, rax                              
  141D55215  jnz     loc_141D56045                         
  141D5521B  shl     r11, 5                                
  141D5521F  mov     [rbp+300h+var_380], r11               
  141D55223  movzx   eax, byte ptr [r11+r12+17AB4h]        
  141D5522C  and     al, 3                                 
  141D5522E  cmp     cs:byte_1432F7E28, 0                    -> &byte_1432F7E28
  141D55235  jz      short loc_141D5525D                   
  141D55237  mov     r8d, cs:dword_1434651D0                 -> &dword_1434651D0
  141D5523E  test    r8d, r8d                              
  141D55241  js      short loc_141D5525D                   
  141D55243  mov     rcx, r15                              
  141D55246  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5524B  cmp     r8d, eax                              
  141D5524E  jge     short loc_141D55256                   
  141D55250  movzx   eax, r8b                              
  141D55254  jmp     short loc_141D5525D                   
  141D55256  call    sub_1409BF14C                           -> sub_1409BF14C
  141D5525B  dec     al                                    
  141D5525D  mov     rcx, [r11+r12+17AA0h]                 
  141D55265  movsx   eax, al                               
  141D55268  mov     ebx, eax                              
  141D5526A  mov     qword ptr [rbp+300h+var_270], rcx     
  141D55271  mov     rcx, [r9]                             
  141D55274  lea     rax, [rax+rax*2]                      
  141D55278  mov     [rbp+300h+var_378], rbx               
  141D5527C  lea     rdx, ds:0[rax*8]                      
  141D55284  mov     qword ptr [rbp+300h+var_280], rdx     
  141D5528B  mov     edi, [rcx+18h]                        
  141D5528E  mov     rcx, r15                              
  141D55291  mov     [rbp+300h+arg_18], edi                
  141D55297  call    sub_1402DCFF8                           -> sub_1402DCFF8
  141D5529C  mov     ecx, [rdx+rax+0Ch]                    
  141D552A0  mov     r9d, r13d                             
  141D552A3  movaps  xmm1, cs:xmmword_1431F1E60              -> &xmmword_1431F1E60
  141D552AA  movaps  xmm13, xmm8                           
  141D552AE  movss   xmm2, dword ptr [r15+40h]             
  141D552B4  movaps  xmm11, xmm1                           
  141D552B8  movsd   xmm12, qword ptr [r15+38h]            
  141D552BE  lea     r8, [rdx+rax]                         
  141D552C2  movsd   xmm4, qword ptr [r15+28h]             
  141D552C8  lea     rdx, [r15+118h]                       
  141D552CF  movsd   xmm3, qword ptr [r15+18h]             
  141D552D5  movaps  xmm14, xmm8                           
  141D552D9  mov     rax, [r15+0D8h]                       
  141D552E0  movaps  xmm15, xmm8                           
  141D552E4  unpcklps xmm2, xmm7                           
  141D552E7  movlhps xmm12, xmm2                           
  141D552EB  movss   xmm2, dword ptr [r15+30h]             
  141D552F1  mov     r10, [rax+130h]                       
  141D552F8  unpcklps xmm2, xmm7                           
  141D552FB  movlhps xmm4, xmm2                            
  141D552FE  movss   xmm2, dword ptr [r15+20h]             
  141D55304  unpcklps xmm2, xmm7                           
  141D55307  movlhps xmm3, xmm2                            
  141D5530A  movss   dword ptr [rsp+400h+var_390+4], xmm8  
  141D55311  mov     qword ptr [rsp+400h+var_398], r8      
  141D55316  mov     qword ptr [rsp+400h+var_3A8], rdx     
  141D5531B  mov     [rbp+300h+arg_10], r13d               
  141D55322  mov     [rbp+300h+var_330], ecx               
  141D55325  movaps  [rbp+300h+var_2E0], xmm1              
  141D55329  mov     [rbp+300h+var_370], r10               
  141D5532D  mov     dword ptr [rsp+400h+var_3B0+4], r13d  
  141D55332  dpps    xmm12, xmm12, 7Fh                     
  141D55339  movaps  xmm9, xmm12                           
  141D5533D  dpps    xmm4, xmm4, 7Fh                       
  141D55343  mulss   xmm9, xmm4                            
  141D55348  movaps  xmm0, xmm4                            
  141D5534B  dpps    xmm3, xmm3, 7Fh                       
  141D55351  mulss   xmm0, xmm3                            
  141D55355  mulss   xmm12, xmm3                           
  141D5535A  andps   xmm9, xmm5                            
  141D5535E  movss   dword ptr [rsp+400h+var_3B0], xmm9    
  141D55365  andps   xmm0, xmm5                            
  141D55368  movss   [rbp+300h+var_338], xmm0              
  141D5536D  movaps  xmm0, cs:xmmword_1431F1B90              -> &xmmword_1431F1B90
  141D55374  andps   xmm12, xmm5                           
  141D55378  movaps  [rbp+300h+var_2F0], xmm0              
  141D5537C  movaps  xmm10, xmm0                           
  141D55380  movaps  [rbp+300h+var_240], xmm12             
  141D55388  movaps  xmm0, xmm8                            
  141D5538C  movss   [rbp+300h+arg_0], xmm0                
  141D55394  test    ecx, ecx                              
  141D55396  jz      loc_141D55B23                         
  141D5539C  nop     dword ptr [rax+00h]                   
  141D553A0  mov     ecx, r13d                             
  141D553A3  mov     eax, 1                                
  141D553A8  shl     eax, cl                               
  141D553AA  movzx   eax, ax                               
  141D553AD  test    edi, eax                              
  141D553AF  jz      loc_141D55E1F                         
  141D553B5  movzx   eax, byte ptr [rdx+rbx*2+0Ah]         
  141D553BA  add     eax, r13d                             
  141D553BD  lea     rcx, [rax+rax*2]                      
  141D553C1  mov     rax, [rdx]                            
  141D553C4  add     rcx, rcx                              
  141D553C7  mov     rdx, [rax+rcx*8+8]                    
  141D553CC  movss   xmm5, dword ptr [rdx+50h]             
  141D553D1  movaps  xmm6, xmm5                            
  141D553D4  mulss   xmm6, cs:dword_1431EF0BC                -> &dword_1431EF0BC
  141D553DC  comiss  xmm6, xmm8                            
  141D553E0  jbe     loc_141D55E0B                         
  141D553E6  cmp     byte ptr [r10+20h], 0                 
  141D553EB  jz      short loc_141D553F8                   
  141D553ED  movss   xmm1, dword ptr [rdx+54h]             
  141D553F2  movss   dword ptr [rsp+400h+var_390+4], xmm1  
  141D553F8  mov     r9, [r8]                              
  141D553FB  xor     eax, eax                              
  141D553FD  mov     edx, [r10+0Ch]                        
  141D55401  movzx   r12d, r13w                            
  141D55405  mov     [rbp+300h+var_308], r9                
  141D55409  lea     rcx, [r12+r12*2]                      
  141D5540D  mov     [rbp+300h+var_318], rcx               
  141D55411  test    edx, edx                              
  141D55413  jz      loc_141D55E0B                         
  141D55419  mov     r8, [r10]                             
  141D5541C  movzx   edi, word ptr [r9+rcx*8+0Ah]          
  141D55422  cmp     [r8+rax*2], di                        
  141D55427  jz      short loc_141D55443                   
  141D55429  inc     eax                                   
  141D5542B  cmp     eax, edx                              
  141D5542D  jb      short loc_141D55422                   
  141D5542F  movss   xmm0, [rbp+300h+arg_0]                
  141D55437  mov     r9d, [rbp+300h+arg_10]                
  141D5543E  jmp     loc_141D55AF5                         
  141D55443  cmp     eax, 0FFFFFFFFh                       
  141D55446  jz      short loc_141D5542F                   
  141D55448  movss   xmm0, cs:flt_1431EF5A8; X               -> &flt_1431EF5A8
  141D55450  lea     rbx, [rax+rax*2]                      
  141D55454  shl     rbx, 4                                
  141D55458  movaps  xmm1, xmm5; Y                         
  141D5545B  add     rbx, [r10+10h]                        
  141D5545F  movss   xmm2, dword ptr [rbx+8]               
  141D55464  movsd   xmm3, qword ptr [rbx]                 
  141D55468  movsd   xmm4, qword ptr [rbx+0Ch]             
  141D5546D  unpcklps xmm2, xmm7                           
  141D55470  movlhps xmm3, xmm2                            
  141D55473  minps   xmm10, xmm3                           
  141D55477  movss   xmm3, dword ptr [rbx+14h]             
  141D5547C  unpcklps xmm3, xmm7                           
  141D5547F  movlhps xmm4, xmm3                            
  141D55482  maxps   xmm11, xmm4                           
  141D55486  movaps  [rbp+300h+var_220], xmm10             
  141D5548E  movaps  [rbp+300h+var_2E0], xmm11             
  141D55493  movaps  [rbp+300h+var_2F0], xmm10             
  141D55498  call    powf                                    -> powf
  141D5549D  mulss   xmm6, cs:dword_1431EF7E8                -> &dword_1431EF7E8
  141D554A5  lea     rcx, [r14+8]                          
  141D554A9  mov     r9d, [rbp+300h+arg_10]                
  141D554B0  mov     edx, 2Eh ; '.'                        
  141D554B5  mulss   xmm9, dword ptr [rbx+24h]             
  141D554BB  movaps  xmm3, xmm0                            
  141D554BE  inc     r9d                                   
  141D554C1  maxss   xmm6, xmm8                            
  141D554C6  mov     [rbp+300h+arg_10], r9d                
  141D554CD  minss   xmm6, xmm7                            
  141D554D1  mulss   xmm3, xmm6                            
  141D554D5  movaps  xmm0, xmm3                            
  141D554D8  movaps  xmm1, xmm3                            
  141D554DB  mulss   xmm0, dword ptr [rbx+20h]             
  141D554E0  movaps  xmm2, xmm3                            
  141D554E3  mulss   xmm1, dword ptr [rbx+18h]             
  141D554E8  mulss   xmm2, dword ptr [rbx+1Ch]             
  141D554ED  addss   xmm15, xmm0                           
  141D554F2  movss   xmm0, [rbp+300h+var_338]              
  141D554F7  addss   xmm13, xmm1                           
  141D554FC  mulss   xmm0, dword ptr [rbx+2Ch]             
  141D55501  movaps  xmm1, xmm12                           
  141D55505  mulss   xmm1, dword ptr [rbx+28h]             
  141D5550A  addss   xmm14, xmm2                           
  141D5550F  addss   xmm9, xmm0                            
  141D55514  movss   xmm0, [rbp+300h+arg_0]                
  141D5551C  addss   xmm9, xmm1                            
  141D55521  mulss   xmm9, xmm3                            
  141D55526  addss   xmm0, xmm9                            
  141D5552B  movss   [rbp+300h+arg_0], xmm0                
  141D55533  call    sub_14023AF5C                           -> sub_14023AF5C
  141D55538  test    al, al                                
  141D5553A  jz      loc_141D55AE2                         
  141D55540  cmp     cs:byte_1432F8790, 0                    -> &byte_1432F8790
  141D55547  mov     r11, [rbp+300h+var_380]               
  141D5554B  jz      loc_141D55E33                         
  141D55551  mov     rax, qword ptr [rbp+300h+var_2D0]     
  141D55555  lea     r13, [rdi+rdi*2]                      
  141D55559  shl     r13, 4                                
  141D5555D  mov     rcx, r15                              
  141D55560  mov     rax, [r11+rax+17AA0h]                 
  141D55568  add     r13, [rax+118h]                       
  141D5556F  mov     rax, [r13+8]                          
  141D55573  mov     qword ptr [rbp+300h+var_260], rax     
  141D5557A  call    sub_1402DCFF8                           -> sub_1402DCFF8
  141D5557F  mov     ecx, dword ptr [rsp+400h+var_3B0+4]   
  141D55583  lea     rdx, [rcx+rcx*2]                      
  141D55587  mov     rcx, qword ptr [rbp+300h+var_280]     
  141D5558E  mov     rax, [rax+rcx]                        
  141D55592  lea     rcx, [r12+r12*8]                      
  141D55596  add     rcx, rcx                              
  141D55599  mov     rax, [rax+rdx*8]                      
  141D5559D  mov     rax, [rax+0C0h]                       
  141D555A4  movzx   edx, byte ptr [rax+rcx*8+8Ch]         
  141D555AC  sub     edx, 2                                
  141D555AF  jz      short loc_141D555BF                   
  141D555B1  sub     edx, 1Dh                              
  141D555B4  jz      short loc_141D555BF                   
  141D555B6  cmp     edx, 1                                
  141D555B9  jnz     loc_141D55DF9                         
  141D555BF  mov     rax, qword ptr [rbp+300h+var_270]     
  141D555C6  mov     ecx, 1                                
  141D555CB  movaps  xmm12, cs:xmmword_1431F1B90             -> &xmmword_1431F1B90
  141D555D3  movaps  xmm10, cs:xmmword_1431F1E60             -> &xmmword_1431F1E60
  141D555DB  mov     eax, [rax+110h]                       
  141D555E1  test    eax, eax                              
  141D555E3  cmovz   eax, ecx                              
  141D555E6  xor     ebx, ebx                              
  141D555E8  mov     [rsp+78h], eax                        
  141D555EC  mov     eax, dword ptr [rsp+400h+var_3A0]     
  141D555F0  mov     edi, [rsp+78h]                        
  141D555F4  shl     eax, 0Ch                              
  141D555F7  mov     [rbp+300h+var_334], eax               
  141D555FA  xor     r12d, r12d                            
  141D555FD  nop     dword ptr [rax]                       
  141D55600  mov     rdx, [rbp+300h+var_308]               
  141D55604  mov     r8, 0C6A4A7935BD1E995h                
  141D5560E  mov     rax, [rbp+300h+var_318]               
  141D55612  mov     rcx, r15                              
  141D55615  imul    rcx, r8                               
  141D55619  movzx   eax, word ptr [rdx+rax*8+0Ah]         
  141D5561E  imul    rax, r8                               
  141D55622  mov     rdx, rax                              
  141D55625  shr     rdx, 2Fh                              
  141D55629  xor     rdx, rax                              
  141D5562C  mov     rax, rcx                              
  141D5562F  shr     rax, 2Fh                              
  141D55633  xor     rax, rcx                              
  141D55636  mov     rcx, 35A98F4D286A90B9h                
  141D55640  imul    rax, rcx                              
  141D55644  imul    rdx, r8                               
  141D55648  mov     rcx, [r14]                            
  141D5564B  add     rcx, 178D0h                           
  141D55652  xor     rdx, rax                              
  141D55655  mov     rax, r12                              
  141D55658  imul    rdx, r8                               
  141D5565C  shr     rax, 2Fh                              
  141D55660  xor     rax, r12                              
  141D55663  imul    rax, r8                               
  141D55667  xor     rdx, rax                              
  141D5566A  imul    rdx, r8                               
  141D5566E  lea     r8, [rsp+78h]                         
  141D55673  mov     [rsp+78h], rdx                        
  141D55678  lea     rdx, [rbp+300h+var_180]               
  141D5567F  call    sub_14058E98C                           -> sub_14058E98C
  141D55684  mov     r9d, ebx                              
  141D55687  lea     rdx, [rbp+300h+var_160]               
  141D5568E  mov     r8, r15                               
  141D55691  call    sub_14291FD34                           -> sub_14291FD34
  141D55696  lea     rdx, [rbp+300h+var_210]               
  141D5569D  lea     rcx, [rbp+300h+var_2F0]               
  141D556A1  call    sub_1402DD020                           -> sub_1402DD020
  141D556A6  movaps  xmm7, [rbp+300h+var_150]              
  141D556AD  movaps  xmm8, xmm12                           
  141D556B1  movaps  xmm5, [rbp+300h+var_160]              
  141D556B8  movaps  xmm6, xmm10                           
  141D556BC  movaps  xmm4, [rbp+300h+var_140]              
  141D556C3  movaps  xmm3, [rbp+300h+var_130]              
  141D556CA  movss   xmm2, [rbp+300h+var_20C]              
  141D556D2  movss   xmm0, [rbp+300h+var_210]              
  141D556DA  movss   xmm1, [rbp+300h+var_208]              
  141D556E2  shufps  xmm2, xmm2, 0                         
  141D556E6  mulps   xmm2, xmm7                            
  141D556E9  shufps  xmm0, xmm0, 0                         
  141D556ED  mulps   xmm0, xmm5                            
  141D556F0  shufps  xmm1, xmm1, 0                         
  141D556F4  mulps   xmm1, xmm4                            
  141D556F7  addps   xmm2, xmm0                            
  141D556FA  movss   xmm0, [rbp+300h+var_200]              
  141D55702  shufps  xmm0, xmm0, 0                         
  141D55706  mulps   xmm0, xmm5                            
  141D55709  addps   xmm2, xmm1                            
  141D5570C  movss   xmm1, [rbp+300h+var_1F8]              
  141D55714  shufps  xmm1, xmm1, 0                         
  141D55718  mulps   xmm1, xmm4                            
  141D5571B  addps   xmm2, xmm3                            
  141D5571E  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D55723  movss   xmm2, [rbp+300h+var_1FC]              
  141D5572B  shufps  xmm2, xmm2, 0                         
  141D5572F  mulps   xmm2, xmm7                            
  141D55732  mov     [rsp+400h+var_3B4], 3F800000h         
  141D5573A  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D55740  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D55745  addps   xmm2, xmm0                            
  141D55748  movss   xmm0, [rbp+300h+var_1F0]              
  141D55750  shufps  xmm0, xmm0, 0                         
  141D55754  mulps   xmm0, xmm5                            
  141D55757  addps   xmm2, xmm1                            
  141D5575A  addps   xmm2, xmm3                            
  141D5575D  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D55762  movss   xmm2, [rbp+300h+var_1EC]              
  141D5576A  mov     [rsp+400h+var_3B4], 3F800000h         
  141D55772  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D55778  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D5577D  shufps  xmm2, xmm2, 0                         
  141D55781  mulps   xmm2, xmm7                            
  141D55784  addps   xmm2, xmm0                            
  141D55787  movss   xmm1, [rbp+300h+var_1E8]              
  141D5578F  movss   xmm0, [rbp+300h+var_1E0]              
  141D55797  shufps  xmm1, xmm1, 0                         
  141D5579B  mulps   xmm1, xmm4                            
  141D5579E  shufps  xmm0, xmm0, 0                         
  141D557A2  mulps   xmm0, xmm5                            
  141D557A5  addps   xmm2, xmm1                            
  141D557A8  movss   xmm1, [rbp+300h+var_1D8]              
  141D557B0  shufps  xmm1, xmm1, 0                         
  141D557B4  mulps   xmm1, xmm4                            
  141D557B7  addps   xmm2, xmm3                            
  141D557BA  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D557BF  movss   xmm2, [rbp+300h+var_1DC]              
  141D557C7  shufps  xmm2, xmm2, 0                         
  141D557CB  mulps   xmm2, xmm7                            
  141D557CE  mov     [rsp+400h+var_3B4], 3F800000h         
  141D557D6  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D557DC  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D557E1  addps   xmm2, xmm0                            
  141D557E4  movss   xmm0, [rbp+300h+var_1D0]              
  141D557EC  shufps  xmm0, xmm0, 0                         
  141D557F0  mulps   xmm0, xmm5                            
  141D557F3  addps   xmm2, xmm1                            
  141D557F6  movss   xmm1, [rbp+300h+var_1C8]              
  141D557FE  shufps  xmm1, xmm1, 0                         
  141D55802  mulps   xmm1, xmm4                            
  141D55805  addps   xmm2, xmm3                            
  141D55808  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D5580D  movss   xmm2, [rbp+300h+var_1CC]              
  141D55815  shufps  xmm2, xmm2, 0                         
  141D55819  mulps   xmm2, xmm7                            
  141D5581C  mov     [rsp+400h+var_3B4], 3F800000h         
  141D55824  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D5582A  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D5582F  addps   xmm2, xmm0                            
  141D55832  movss   xmm0, [rbp+300h+var_1C0]              
  141D5583A  shufps  xmm0, xmm0, 0                         
  141D5583E  mulps   xmm0, xmm5                            
  141D55841  addps   xmm2, xmm1                            
  141D55844  movss   xmm1, [rbp+300h+var_1B8]              
  141D5584C  shufps  xmm1, xmm1, 0                         
  141D55850  mulps   xmm1, xmm4                            
  141D55853  addps   xmm2, xmm3                            
  141D55856  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D5585B  movss   xmm2, [rbp+300h+var_1BC]              
  141D55863  shufps  xmm2, xmm2, 0                         
  141D55867  mulps   xmm2, xmm7                            
  141D5586A  mov     [rsp+400h+var_3B4], 3F800000h         
  141D55872  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D55878  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D5587D  addps   xmm2, xmm0                            
  141D55880  movss   xmm0, [rbp+300h+var_1B0]              
  141D55888  shufps  xmm0, xmm0, 0                         
  141D5588C  mulps   xmm0, xmm5                            
  141D5588F  addps   xmm2, xmm1                            
  141D55892  movss   xmm1, [rbp+300h+var_1A8]              
  141D5589A  shufps  xmm1, xmm1, 0                         
  141D5589E  mulps   xmm1, xmm4                            
  141D558A1  addps   xmm2, xmm3                            
  141D558A4  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D558A9  movss   xmm2, [rbp+300h+var_1AC]              
  141D558B1  shufps  xmm2, xmm2, 0                         
  141D558B5  mulps   xmm2, xmm7                            
  141D558B8  mov     [rsp+400h+var_3B4], 3F800000h         
  141D558C0  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D558C6  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D558CB  addps   xmm2, xmm0                            
  141D558CE  movss   xmm0, [rbp+300h+var_1A0]              
  141D558D6  shufps  xmm0, xmm0, 0                         
  141D558DA  mulps   xmm0, xmm5                            
  141D558DD  addps   xmm2, xmm1                            
  141D558E0  addps   xmm2, xmm3                            
  141D558E3  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D558E8  movss   xmm2, [rbp+300h+var_19C]              
  141D558F0  shufps  xmm2, xmm2, 0                         
  141D558F4  mov     [rsp+400h+var_3B4], 3F800000h         
  141D558FC  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D55902  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D55907  mulps   xmm2, xmm7                            
  141D5590A  addps   xmm2, xmm0                            
  141D5590D  movss   xmm1, [rbp+300h+var_198]              
  141D55915  lea     rdx, [rbp+300h+var_120]               
  141D5591C  mov     rcx, [r14]                            
  141D5591F  shufps  xmm1, xmm1, 0                         
  141D55923  add     rcx, 17920h                           
  141D5592A  mulps   xmm1, xmm4                            
  141D5592D  addps   xmm2, xmm1                            
  141D55930  addps   xmm2, xmm3                            
  141D55933  movaps  [rsp+400h+var_3C8+8], xmm2            
  141D55938  mov     [rsp+400h+var_3B4], 3F800000h         
  141D55940  minps   xmm8, [rsp+400h+var_3C8+8]            
  141D55946  maxps   xmm6, [rsp+400h+var_3C8+8]            
  141D5594B  movaps  [rbp+300h+var_120], xmm8              
  141D55953  movaps  [rbp+300h+var_110], xmm6              
  141D5595A  call    sub_1403A3A30                           -> sub_1403A3A30
  141D5595F  mov     rcx, [r14]                            
  141D55962  lea     rdx, [rbp+300h+var_2C0]               
  141D55966  add     rcx, 17930h                           
  141D5596D  call    sub_14207E91C                           -> sub_14207E91C
  141D55972  mov     rcx, [rbp+300h+var_310]               
  141D55976  mov     rax, [rcx]                            
  141D55979  test    dword ptr [rax+18h], 40000h           
  141D55980  jz      short loc_141D5598C                   
  141D55982  movss   xmm0, cs:dword_1434650E0                -> &dword_1434650E0
  141D5598A  jmp     short loc_141D55994                   
  141D5598C  movss   xmm0, cs:dword_143465090                -> &dword_143465090
  141D55994  mov     rax, [rbp+300h+var_318]               
  141D55998  mov     rcx, [rbp+300h+var_308]               
  141D5599C  mov     r15, qword ptr [rbp+300h+var_2C0]     
  141D559A0  mov     [rbp+300h+var_28C], ebx               
  141D559A3  mov     [rbp+300h+var_2A8], esi               
  141D559A6  movzx   ecx, word ptr [rcx+rax*8+0Ah]         
  141D559AB  mov     eax, ecx                              
  141D559AD  and     eax, 0FFFh                            
  141D559B2  or      eax, [rbp+300h+var_334]               
  141D559B5  mov     [rbp+300h+var_2B0], eax               
  141D559B8  movzx   eax, cx                               
  141D559BB  imul    rdx, rax, 0F8h                        
  141D559C2  mov     rax, [r15+0D8h]                       
  141D559C9  mov     rcx, [rax+0A0h]                       
  141D559D0  mov     eax, 0AAAAAAABh                       
  141D559D5  mul     dword ptr [rdx+rcx+0E8h]              
  141D559DC  mov     rax, qword ptr [rbp+300h+var_170]     
  141D559E3  shr     edx, 1                                
  141D559E5  mov     [rbp+300h+var_2AC], edx               
  141D559E8  test    rax, rax                              
  141D559EB  jz      short loc_141D559F5                   
  141D559ED  mov     eax, [rax+10h]                        
  141D559F0  mov     [rbp+300h+var_2A4], eax               
  141D559F3  jmp     short loc_141D559FC                   
  141D559F5  mov     [rbp+300h+var_2A4], 0FFFFFFFFh        
  141D559FC  mov     rax, qword ptr [rbp+300h+var_260]     
  141D55A03  mov     r9d, 80000h                           
  141D55A09  mov     r8, [r14]                             
  141D55A0C  movss   [rbp+300h+var_290], xmm9              
  141D55A12  movss   [rbp+300h+var_29C], xmm0              
  141D55A17  movzx   eax, word ptr [rax+58h]               
  141D55A1B  mov     [rbp+300h+var_2A0], ax                
  141D55A1F  mov     rax, [r13+8]                          
  141D55A23  movzx   ecx, word ptr [rax+0BCh]              
  141D55A2A  mov     eax, [r8+178C8h]                      
  141D55A31  mov     [rbp+300h+var_29E], cx                
  141D55A35  xor     ecx, ecx                              
  141D55A37  test    eax, eax                              
  141D55A39  mov     [rbp+300h+var_298], eax               
  141D55A3C  cmovz   ecx, r9d                              
  141D55A40  cmp     dword ptr [r8+1791Ch], 2000h          
  141D55A4B  mov     [rbp+300h+var_294], ecx               
  141D55A4E  jnb     short loc_141D55A9B                   
  141D55A50  lea     eax, [rdx+rsi]                        
  141D55A53  cmp     eax, r9d                              
  141D55A56  jnb     short loc_141D55A9B                   
  141D55A58  lea     rcx, [r8+17910h]                      
  141D55A5F  lea     rdx, [rbp+300h+var_2B0]               
  141D55A63  call    sub_142920D84                           -> sub_142920D84
  141D55A68  mov     rcx, [r14]                            
  141D55A6B  lea     rdx, [rsp+78h]                        
  141D55A70  add     rcx, 178D0h                           
  141D55A77  call    sub_14058D030                           -> sub_14058D030
  141D55A7C  lea     rdx, [rsp+78h]                        
  141D55A81  lea     rcx, [rbp+300h+var_300]               
  141D55A85  mov     [rax], esi                               <== HEAP WRITE
  141D55A87  call    sub_140109A44                           -> sub_140109A44
  141D55A8C  add     esi, [rbp+300h+var_2AC]               
  141D55A8F  mov     rax, [r14]                            
  141D55A92  lea     ecx, [rsi+1]                          
  141D55A95  mov     [rax+178C0h], ecx                        <== HEAP WRITE
  141D55A9B  mov     rax, 395B586CA42E166Bh                
  141D55AA5  inc     ebx                                   
  141D55AA7  sub     r12, rax                              
  141D55AAA  cmp     ebx, edi                              
  141D55AAC  jb      loc_141D55600                         
  141D55AB2  movaps  xmm10, [rbp+300h+var_220]             
  141D55ABA  xorps   xmm8, xmm8                            
  141D55ABE  movaps  xmm12, [rbp+300h+var_240]             
  141D55AC6  movss   xmm7, cs:Y                              -> &Y
  141D55ACE  movss   xmm0, [rbp+300h+arg_0]                
  141D55AD6  mov     r13d, dword ptr [rsp+400h+var_3B0+4]  
  141D55ADB  mov     r9d, [rbp+300h+arg_10]                
  141D55AE2  mov     r11, [rbp+300h+var_380]               
  141D55AE6  mov     rbx, [rbp+300h+var_378]               
  141D55AEA  mov     r10, [rbp+300h+var_370]               
  141D55AEE  movss   xmm9, dword ptr [rsp+400h+var_3B0]    
  141D55AF5  mov     r8, qword ptr [rsp+400h+var_398]      
  141D55AFA  mov     edi, [rbp+300h+arg_18]                
  141D55B00  mov     rdx, qword ptr [rsp+400h+var_3A8]     
  141D55B05  inc     r13d                                  
  141D55B08  mov     dword ptr [rsp+400h+var_3B0+4], r13d  
  141D55B0D  cmp     r13d, [rbp+300h+var_330]              
  141D55B11  jb      loc_141D553A0                         
  141D55B17  movss   xmm6, cs:dword_1431EEFB0                -> &dword_1431EEFB0
  141D55B1F  mov     r12, qword ptr [rbp+300h+var_2D0]     
  141D55B23  test    r9d, r9d                              
  141D55B26  jz      loc_141D55FAF                         
  141D55B2C  mov     r13d, dword ptr [rsp+400h+var_390]    
  141D55B31  lea     rcx, [r15+18h]                        
  141D55B35  movaps  xmm2, xmm15                           
  141D55B39  lea     rdx, [rbp+300h+var_160]               
  141D55B40  unpcklps xmm2, xmm0                           
  141D55B43  and     edi, 0FFFF0000h                       
  141D55B49  movaps  xmm15, xmm13                          
  141D55B4D  or      edi, r13d                             
  141D55B50  inc     r13d                                  
  141D55B53  unpcklps xmm15, xmm14                         
  141D55B57  mov     dword ptr [rsp+400h+var_390], r13d    
  141D55B5C  movlhps xmm15, xmm2                           
  141D55B60  call    sub_14056C530                           -> sub_14056C530
  141D55B65  movss   xmm3, dword ptr [rbp+300h+var_2F0]    
  141D55B6A  movss   xmm4, dword ptr [rbp+300h+var_2F0+4]  
  141D55B6F  movaps  xmm10, xmm3                           
  141D55B73  movss   xmm5, dword ptr [rbp+300h+var_2F0+8]  
  141D55B78  addss   xmm10, xmm6                           
  141D55B7D  movss   xmm1, dword ptr [rbp+300h+var_2E0]    
  141D55B82  movaps  xmm8, xmm4                            
  141D55B86  movaps  xmm9, xmm5                            
  141D55B8A  addss   xmm8, xmm6                            
  141D55B8F  addss   xmm9, xmm6                            
  141D55B94  comiss  xmm1, xmm10                           
  141D55B98  jb      short loc_141D55B9E                   
  141D55B9A  movaps  xmm10, xmm1                           
  141D55B9E  movss   xmm0, dword ptr [rbp+300h+var_2E0+4]  
  141D55BA3  comiss  xmm0, xmm8                            
  141D55BA7  jb      short loc_141D55BAD                   
  141D55BA9  movaps  xmm8, xmm0                            
  141D55BAD  movss   xmm2, dword ptr [rbp+300h+var_2E0+8]  
  141D55BB2  comiss  xmm2, xmm9                            
  141D55BB6  jb      short loc_141D55BBC                   
  141D55BB8  movaps  xmm9, xmm2                            
  141D55BBC  subss   xmm1, xmm6                            
  141D55BC0  subss   xmm0, xmm6                            
  141D55BC4  subss   xmm2, xmm6                            
  141D55BC8  comiss  xmm1, xmm3                            
  141D55BCB  jb      short loc_141D55BD0                   
  141D55BCD  movaps  xmm1, xmm3                            
  141D55BD0  comiss  xmm0, xmm4                            
  141D55BD3  jb      short loc_141D55BD8                   
  141D55BD5  movaps  xmm0, xmm4                            
  141D55BD8  comiss  xmm2, xmm5                            
  141D55BDB  jb      short loc_141D55BE0                   
  141D55BDD  movaps  xmm2, xmm5                            
  141D55BE0  movaps  xmm4, [rbp+300h+var_130]              
  141D55BE7  lea     rcx, [rbp+300h+var_368]               
  141D55BEB  shufps  xmm0, xmm0, 0                         
  141D55BEF  movaps  xmm6, xmm1                            
  141D55BF2  mulps   xmm0, [rbp+300h+var_150]              
  141D55BF9  movaps  xmm12, xmm10                          
  141D55BFD  shufps  xmm12, xmm12, 0                       
  141D55C02  movaps  xmm13, xmm8                           
  141D55C06  mulps   xmm12, [rbp+300h+var_160]             
  141D55C0E  movaps  xmm7, xmm0                            
  141D55C11  shufps  xmm13, xmm13, 0                       
  141D55C16  mulps   xmm13, [rbp+300h+var_150]             
  141D55C1E  addps   xmm12, xmm0                           
  141D55C22  shufps  xmm2, xmm2, 0                         
  141D55C26  mulps   xmm2, [rbp+300h+var_140]              
  141D55C2D  shufps  xmm9, xmm9, 0                         
  141D55C32  mulps   xmm9, [rbp+300h+var_140]              
  141D55C3A  addps   xmm12, xmm2                           
  141D55C3E  shufps  xmm6, xmm6, 0                         
  141D55C42  mulps   xmm6, [rbp+300h+var_160]              
  141D55C49  movaps  xmm14, xmm2                           
  141D55C4D  addps   xmm12, xmm4                           
  141D55C51  addps   xmm7, xmm6                            
  141D55C54  addps   xmm13, xmm6                           
  141D55C58  movaps  xmm1, xmm12                           
  141D55C5C  movaps  xmm11, xmm12                          
  141D55C60  shufps  xmm11, xmm12, 55h ; 'U'               
  141D55C65  addps   xmm14, xmm7                           
  141D55C69  movaps  xmm0, xmm11                           
  141D55C6D  movaps  [rbp+300h+var_280], xmm11             
  141D55C75  addps   xmm13, xmm2                           
  141D55C79  addps   xmm9, xmm7                            
  141D55C7D  addps   xmm14, xmm4                           
  141D55C81  addps   xmm13, xmm4                           
  141D55C85  addps   xmm9, xmm4                            
  141D55C89  subss   xmm1, xmm14                           
  141D55C8E  movaps  xmm8, xmm14                           
  141D55C92  shufps  xmm8, xmm14, 55h ; 'U'                
  141D55C97  movaps  xmm10, xmm14                          
  141D55C9B  subss   xmm0, xmm8                            
  141D55CA0  shufps  xmm10, xmm14, 0AAh                    
  141D55CA5  movaps  [rbp+300h+var_2D0], xmm8              
  141D55CAA  movaps  [rbp+300h+var_2C0], xmm10             
  141D55CAF  movss   dword ptr [rbp+300h+var_368], xmm1    
  141D55CB4  movaps  xmm1, xmm13                           
  141D55CB8  subss   xmm1, xmm14                           
  141D55CBD  movss   dword ptr [rbp+300h+var_368+4], xmm0  
  141D55CC2  movaps  xmm0, xmm12                           
  141D55CC6  shufps  xmm0, xmm12, 0AAh                     
  141D55CCB  movaps  [rsp+400h+var_3C8+8], xmm0            
  141D55CD0  subss   xmm0, xmm10                           
  141D55CD5  movss   dword ptr [rbp+300h+var_358], xmm1    
  141D55CDA  movaps  xmm1, xmm9                            
  141D55CDE  subss   xmm1, xmm14                           
  141D55CE3  movss   [rbp+300h+var_360], xmm0              
  141D55CE8  movaps  xmm0, xmm13                           
  141D55CEC  shufps  xmm0, xmm13, 55h ; 'U'                
  141D55CF1  movaps  [rbp+300h+var_240], xmm0              
  141D55CF8  subss   xmm0, xmm8                            
  141D55CFD  movss   dword ptr [rbp+300h+var_348], xmm1    
  141D55D02  movss   dword ptr [rbp+300h+var_358+4], xmm0  
  141D55D07  movaps  xmm0, xmm13                           
  141D55D0B  shufps  xmm0, xmm13, 0AAh                     
  141D55D10  movaps  [rbp+300h+var_260], xmm0              
  141D55D17  subss   xmm0, xmm10                           
  141D55D1C  movss   [rbp+300h+var_350], xmm0              
  141D55D21  movaps  xmm0, xmm9                            
  141D55D25  shufps  xmm0, xmm9, 55h ; 'U'                 
  141D55D2A  movaps  [rbp+300h+var_220], xmm0              
  141D55D31  subss   xmm0, xmm8                            
  141D55D36  movss   dword ptr [rbp+300h+var_348+4], xmm0  
  141D55D3B  movaps  xmm0, xmm9                            
  141D55D3F  shufps  xmm0, xmm9, 0AAh                      
  141D55D44  movaps  [rbp+300h+var_270], xmm0              
  141D55D4B  subss   xmm0, xmm10                           
  141D55D50  movss   [rbp+300h+var_340], xmm0              
  141D55D55  call    sub_14013F898                           -> sub_14013F898
  141D55D5A  lea     rcx, [rbp+300h+var_358]               
  141D55D5E  movaps  xmm6, xmm0                            
  141D55D61  call    sub_14013F898                           -> sub_14013F898
  141D55D66  movaps  xmm7, xmm0                            
  141D55D69  lea     rcx, [rbp+300h+var_348]               
  141D55D6D  call    sub_14013F898                           -> sub_14013F898
  141D55D72  mov     rax, [r14]                            
  141D55D75  movss   xmm3, [rbp+300h+var_360]              
  141D55D7A  movsd   xmm4, [rbp+300h+var_368]              
  141D55D7F  movss   xmm2, [rbp+300h+var_350]              
  141D55D84  mov     ecx, [rax+3EE2A4h]                    
  141D55D8A  unpcklps xmm3, xmm6                           
  141D55D8D  movlhps xmm4, xmm3                            
  141D55D90  movsd   xmm3, [rbp+300h+var_358]              
  141D55D95  unpcklps xmm2, xmm7                           
  141D55D98  movlhps xmm3, xmm2                            
  141D55D9B  movss   xmm2, [rbp+300h+var_340]              
  141D55DA0  unpcklps xmm2, xmm0                           
  141D55DA3  movaps  xmm0, xmm15                           
  141D55DA7  shufps  xmm0, xmm15, 0FFh                     
  141D55DAC  sqrtss  xmm0, xmm0                            
  141D55DB0  imul    rax, rcx, 70h ; 'p'                   
  141D55DB4  mulss   xmm0, [rbp+300h+var_32C]              
  141D55DB9  add     rax, [rbp+300h+Src]                   
  141D55DBD  movss   dword ptr [rax], xmm14                
  141D55DC2  movss   dword ptr [rax+4], xmm8               
  141D55DC8  movss   dword ptr [rax+8], xmm10              
  141D55DCE  movups  xmmword ptr [rax+10h], xmm4              <== HEAP WRITE
  141D55DD2  movups  xmmword ptr [rax+20h], xmm3              <== HEAP WRITE
  141D55DD6  movsd   xmm3, [rbp+300h+var_348]              
  141D55DDB  movlhps xmm3, xmm2                            
  141D55DDE  movups  xmmword ptr [rax+30h], xmm3              <== HEAP WRITE
  141D55DE2  movss   dword ptr [rax+60h], xmm0             
  141D55DE7  mov     [rax+0Ch], edi                           <== HEAP WRITE
  141D55DEA  cmp     byte ptr [r10+20h], 0                 
  141D55DEF  jz      short loc_141D55E40                   
  141D55DF1  movss   xmm0, dword ptr [rsp+400h+var_390+4]  
  141D55DF7  jmp     short loc_141D55E43                   
  141D55DF9  movss   xmm0, [rbp+300h+arg_0]                
  141D55E01  mov     r13d, dword ptr [rsp+400h+var_3B0+4]  
  141D55E06  jmp     loc_141D55AE6                         
  141D55E0B  movss   xmm0, [rbp+300h+arg_0]                
  141D55E13  mov     r9d, [rbp+300h+arg_10]                
  141D55E1A  jmp     loc_141D55B00                         
  141D55E1F  movss   xmm0, [rbp+300h+arg_0]                
  141D55E27  mov     r9d, [rbp+300h+arg_10]                
  141D55E2E  jmp     loc_141D55B05                         
  141D55E33  movss   xmm0, [rbp+300h+arg_0]                
  141D55E3B  jmp     loc_141D55AE6                         
  141D55E40  xorps   xmm0, xmm0                            
  141D55E43  movss   xmm4, cs:flt_1431EF19C                  -> &flt_1431EF19C
  141D55E4B  movaps  xmm10, xmm13                          
  141D55E4F  movaps  xmm1, [rbp+300h+var_260]              
  141D55E56  addss   xmm10, xmm12                          
  141D55E5B  movaps  xmm2, [rbp+300h+var_220]              
  141D55E62  movaps  xmm3, xmm1                            
  141D55E65  addss   xmm3, dword ptr [rsp+400h+var_3C8+8]  
  141D55E6B  movaps  xmm5, [rbp+300h+var_270]              
  141D55E72  bts     edi, 14h                              
  141D55E76  movss   dword ptr [rax+64h], xmm0             
  141D55E7B  movaps  xmm0, [rbp+300h+var_240]              
  141D55E82  addss   xmm10, xmm9                           
  141D55E87  movaps  xmm8, xmm0                            
  141D55E8B  addss   xmm8, xmm11                           
  141D55E90  addss   xmm3, xmm5                            
  141D55E94  mulss   xmm10, xmm4                           
  141D55E99  addss   xmm8, xmm2                            
  141D55E9E  mulss   xmm3, xmm4                            
  141D55EA2  movss   dword ptr [rax+40h], xmm10            
  141D55EA8  movaps  xmm6, xmm10                           
  141D55EAC  subss   xmm6, xmm9                            
  141D55EB1  movaps  xmm11, xmm10                          
  141D55EB5  mulss   xmm8, xmm4                            
  141D55EBA  subss   xmm11, xmm13                          
  141D55EBF  movss   dword ptr [rax+48h], xmm3             
  141D55EC4  movaps  xmm7, xmm3                            
  141D55EC7  movaps  xmm4, xmm3                            
  141D55ECA  subss   xmm7, xmm1                            
  141D55ECE  movss   dword ptr [rax+44h], xmm8             
  141D55ED4  subss   xmm4, xmm5                            
  141D55ED8  movaps  xmm9, xmm8                            
  141D55EDC  mulss   xmm11, xmm11                          
  141D55EE1  subss   xmm9, xmm2                            
  141D55EE6  mulss   xmm6, xmm6                            
  141D55EEA  movaps  xmm2, xmm8                            
  141D55EEE  mulss   xmm7, xmm7                            
  141D55EF2  subss   xmm2, dword ptr [rbp+300h+var_280]    
  141D55EFA  movaps  xmm5, xmm8                            
  141D55EFE  mulss   xmm9, xmm9                            
  141D55F03  subss   xmm8, dword ptr [rbp+300h+var_2D0]    
  141D55F09  subss   xmm5, xmm0                            
  141D55F0D  mulss   xmm4, xmm4                            
  141D55F11  movaps  xmm0, xmm3                            
  141D55F14  mulss   xmm2, xmm2                            
  141D55F18  subss   xmm0, dword ptr [rsp+400h+var_3C8+8]  
  141D55F1E  subss   xmm3, dword ptr [rbp+300h+var_2C0]    
  141D55F23  movaps  xmm1, xmm10                           
  141D55F27  mulss   xmm8, xmm8                            
  141D55F2C  subss   xmm1, xmm12                           
  141D55F31  mulss   xmm5, xmm5                            
  141D55F35  mulss   xmm0, xmm0                            
  141D55F39  subss   xmm10, xmm14                          
  141D55F3E  mulss   xmm3, xmm3                            
  141D55F42  addss   xmm9, xmm6                            
  141D55F47  mulss   xmm1, xmm1                            
  141D55F4B  addss   xmm11, xmm5                           
  141D55F50  mulss   xmm10, xmm10                          
  141D55F55  addss   xmm2, xmm1                            
  141D55F59  xorps   xmm1, xmm1                            
  141D55F5C  addss   xmm9, xmm4                            
  141D55F61  addss   xmm8, xmm10                           
  141D55F66  addss   xmm11, xmm7                           
  141D55F6B  addss   xmm2, xmm0                            
  141D55F6F  xorps   xmm0, xmm0                            
  141D55F72  sqrtss  xmm1, xmm9                            
  141D55F77  addss   xmm8, xmm3                            
  141D55F7C  xorps   xmm3, xmm3                            
  141D55F7F  sqrtss  xmm0, xmm2                            
  141D55F83  sqrtss  xmm3, xmm8                            
  141D55F88  maxss   xmm3, xmm0                            
  141D55F8C  xorps   xmm0, xmm0                            
  141D55F8F  sqrtss  xmm0, xmm11                           
  141D55F94  maxss   xmm1, xmm0                            
  141D55F98  maxss   xmm3, xmm1                            
  141D55F9C  movss   dword ptr [rax+4Ch], xmm3             
  141D55FA1  movups  xmmword ptr [rax+50h], xmm15             <== HEAP WRITE
  141D55FA6  mov     rax, [r14]                            
  141D55FA9  inc     dword ptr [rax+3EE2A4h]               
  141D55FAF  mov     rcx, [r11+r12+17AA0h]                 
  141D55FB7  mov     ebx, 1                                
  141D55FBC  test    [rcx+0EAh], bl                        
  141D55FC2  jz      short loc_141D55FDC                   
  141D55FC4  mov     ebx, 3                                
  141D55FC9  call    sub_1429202CC                           -> sub_1429202CC
  141D55FCE  mov     r11, [rbp+300h+var_380]               
  141D55FD2  test    al, al                                
  141D55FD4  mov     eax, 0Bh                              
  141D55FD9  cmovz   ebx, eax                              
  141D55FDC  mov     rax, [r11+r12+17AA0h]                 
  141D55FE4  mov     r8d, ebx                              
  141D55FE7  movss   xmm7, cs:Y                              -> &Y
  141D55FEF  or      r8d, 4                                
  141D55FF3  movss   xmm6, cs:dword_1431EEFB0                -> &dword_1431EEFB0
  141D55FFB  xor     r13d, r13d                            
  141D55FFE  movss   xmm5, dword ptr cs:xmmword_1431F1BE0    -> &xmmword_1431F1BE0
  141D56006  xorps   xmm8, xmm8                            
  141D5600A  movzx   ecx, word ptr [rax+0EAh]              
  141D56011  mov     eax, 200h                             
  141D56016  and     cx, ax                                
  141D56019  mov     rax, [rbp+300h+var_310]               
  141D5601D  cmp     r13w, cx                              
  141D56021  cmovz   r8d, ebx                              
  141D56025  mov     edx, [rax+0Ch]                        
  141D56028  mov     rax, [r14]                            
  141D5602B  shl     r8d, 18h                              
  141D5602F  or      r8d, edi                              
  141D56032  shl     rdx, 6                                
  141D56036  mov     rcx, [rax+36E290h]                    
  141D5603D  mov     [rdx+rcx+30h], r8d                       <== HEAP WRITE
  141D56042  mov     r8, [r14]                             
  141D56045  mov     eax, dword ptr [rsp+400h+var_3A0]     
  141D56049  inc     eax                                   
  141D5604B  mov     dword ptr [rsp+400h+var_3A0], eax     
  141D5604F  cmp     eax, [rbp+300h+var_328]               
  141D56052  jnz     loc_141D55170                         
  141D56058  cmp     byte ptr [rbp+300h+arg_8], 0          
  141D5605F  movaps  xmm15, [rsp+400h+var_E8+8]            
  141D56068  movaps  xmm14, [rsp+400h+var_D8+8]            
  141D56071  movaps  xmm13, [rsp+400h+var_C8+8]            
  141D5607A  movaps  xmm12, [rsp+400h+var_B8+8]            
  141D56083  movaps  xmm11, [rsp+400h+var_A8+8]            
  141D5608C  movaps  xmm10, [rsp+400h+var_98+8]            
  141D56095  movaps  xmm9, [rsp+400h+var_88+8]             
  141D5609E  mov     r12, [rsp+3D0h]                       
  141D560A6  mov     rdi, [rbp+300h+lpCriticalSection]     
  141D560AD  mov     rbx, [rbp+300h+Src]                   
  141D560B1  jnz     short loc_141D560C0                   
  141D560B3  mov     rax, [r14]                            
  141D560B6  mov     [rax+17968h], r13                        <== HEAP WRITE
  141D560BD  mov     r8, [r14]                             
  141D560C0  cmp     byte ptr [r8+59h], 0                  
  141D560C5  movaps  xmm8, [rsp+400h+var_78+8]             
  141D560CE  movaps  xmm7, [rsp+400h+var_68+8]             
  141D560D6  mov     r15, [rsp+400h+var_38]                
  141D560DE  jz      loc_141D56322                         
  141D560E4  test    rbx, rbx                              
  141D560E7  jz      loc_141D56322                         
  141D560ED  mov     eax, [r8+3EE2A4h]                     
  141D560F4  test    eax, eax                              
  141D560F6  jz      short loc_141D5614C                   
  141D560F8  movaps  xmm6, [rbp+300h+var_230]              
  141D560FF  lea     r9, [rbp+300h+var_230]                
  141D56106  imul    r8, rax, 70h ; 'p'                    
  141D5610A  movaps  [rbp+300h+var_230], xmm6              
  141D56111  mov     rdx, rbx                              
  141D56114  lea     rcx, [rbp+300h+var_190]               
  141D5611B  add     r8, rbx                               
  141D5611E  call    sub_141D54D00                           -> sub_141D54D00
  141D56123  mov     rax, [r14]                            
  141D56126  lea     r8, [rbp+300h+var_190]                
  141D5612D  movaps  [rbp+300h+var_190], xmm6              
  141D56134  mov     ecx, [rax+3EE2A4h]                    
  141D5613A  imul    rdx, rcx, 70h ; 'p'                   
  141D5613E  mov     rcx, rbx                              
  141D56141  add     rdx, rbx                              
  141D56144  call    sub_141D54D60                           -> sub_141D54D60
  141D56149  mov     r8, [r14]                             
  141D5614C  mov     eax, [r8+3EE2A4h]                     
  141D56153  mov     esi, 1                                
  141D56158  mov     [rbx+68h], eax                           <== HEAP WRITE
  141D5615B  mov     rdx, rbx; Src                         
  141D5615E  mov     r8, [r14]                             
  141D56161  mov     eax, cs:dword_1434381C8                 -> &dword_1434381C8
  141D56167  mov     eax, [r8+rax*4+1782Ch]                
  141D5616F  dec     eax                                   
  141D56171  imul    rcx, rax, 0B0h                        
  141D56178  mov     rax, cs:qword_143438A28                 -> &qword_143438A28
  141D5617F  mov     rcx, [rcx+rax+5C0B38h]; Dst           
  141D56187  mov     eax, [r8+3EE2A4h]                     
  141D5618E  cmp     eax, 1                                
  141D56191  cmovb   eax, esi                              
  141D56194  imul    r8, rax, 70h ; 'p'; MaxCount          
  141D56198  call    memmove                                 -> memmove
  141D5619D  lea     rcx, [r14+8]                          
  141D561A1  lea     edx, [rsi+2Dh]                        
  141D561A4  call    sub_14023AF5C                           -> sub_14023AF5C
  141D561A9  test    al, al                                
  141D561AB  jz      loc_141D562DB                         
  141D561B1  cmp     cs:byte_1432F8790, 0                    -> &byte_1432F8790
  141D561B8  jz      loc_141D562DB                         
  141D561BE  mov     rcx, [r14]                            
  141D561C1  lea     rdx, [rbp+300h+arg_8]                 
  141D561C8  mov     eax, [rcx+1791Ch]                     
  141D561CE  mov     [rcx+178C4h], eax                        <== HEAP WRITE
  141D561D4  mov     rax, [r14]                            
  141D561D7  mov     ecx, [rax+178C0h]                     
  141D561DD  mov     dword ptr [rbp+300h+arg_8], ecx       
  141D561E3  lea     rcx, unk_143465240                      -> &unk_143465240
  141D561EA  call    sub_1403749A4                           -> sub_1403749A4
  141D561EF  mov     rdx, [r14]                            
  141D561F2  mov     eax, cs:dword_1434381C8                 -> &dword_1434381C8
  141D561F8  mov     eax, [rdx+rax*4+178A0h]               
  141D561FF  dec     eax                                   
  141D56201  imul    rcx, rax, 0B0h                        
  141D56208  mov     rax, cs:qword_143438A28                 -> &qword_143438A28
  141D5620F  mov     rcx, [rcx+rax+5C0B38h]; Dst           
  141D56217  mov     eax, [rdx+178C4h]                     
  141D5621D  cmp     eax, esi                              
  141D5621F  mov     rdx, [rdx+17910h]; Src                
  141D56226  cmovb   eax, esi                              
  141D56229  lea     r8, [rax+rax*2]                       
  141D5622D  shl     r8, 4; MaxCount                       
  141D56231  call    memmove                                 -> memmove
  141D56236  mov     rax, [r14]                            
  141D56239  mov     rbx, [rax+17900h]                     
  141D56240  mov     eax, [rax+1790Ch]                     
  141D56246  lea     rsi, [rbx+rax*8]                      
  141D5624A  nop     word ptr [rax+rax+00h]                
  141D56250  cmp     rbx, rsi                              
  141D56253  jz      short loc_141D56297                   
  141D56255  mov     rax, [rbx]                            
  141D56258  lea     rdx, [rbp+300h+arg_8]                 
  141D5625F  lea     rcx, [rbp+300h+var_300]               
  141D56263  mov     [rbp+300h+arg_8], rax                 
  141D5626A  call    sub_140246C88                           -> sub_140246C88
  141D5626F  test    rax, rax                              
  141D56272  jnz     short loc_141D56291                   
  141D56274  mov     rcx, [r14]                            
  141D56277  lea     r8, [rbp+300h+arg_8]                  
  141D5627E  add     rcx, 178D0h                           
  141D56285  lea     rdx, [rbp+300h+var_120]               
  141D5628C  call    sub_1421F3DAC                           -> sub_1421F3DAC
  141D56291  add     rbx, 8                                
  141D56295  jmp     short loc_141D56250                   
  141D56297  mov     rax, [r14]                            
  141D5629A  mov     [rax+1790Ch], r13d                       <== HEAP WRITE
  141D562A1  mov     rbx, [rbp+300h+var_300]               
  141D562A5  mov     eax, [rbp+300h+var_2F4]               
  141D562A8  lea     rsi, [rbx+rax*8]                      
  141D562AC  nop     dword ptr [rax+00h]                   
  141D562B0  cmp     rbx, rsi                              
  141D562B3  jz      short loc_141D56322                   
  141D562B5  mov     rcx, [r14]                            
  141D562B8  lea     rdx, [rbp+300h+arg_8]                 
  141D562BF  mov     rax, [rbx]                            
  141D562C2  add     rcx, 17900h                           
  141D562C9  mov     [rbp+300h+arg_8], rax                 
  141D562D0  call    sub_140109A44                           -> sub_140109A44
  141D562D5  add     rbx, 8                                
  141D562D9  jmp     short loc_141D562B0                   
  141D562DB  mov     rax, [r14]                            
  141D562DE  mov     [rax+178C4h], r13d                       <== HEAP WRITE
  141D562E5  mov     rax, [r14]                            
  141D562E8  mov     [rax+1790Ch], r13d                       <== HEAP WRITE
  141D562EF  mov     rcx, [r14]                            
  141D562F2  add     rcx, 178D0h                           
  141D562F9  call    sub_140248A04                           -> sub_140248A04
  141D562FE  mov     rax, [r14]                            
  141D56301  lea     rdx, [rbp+300h+arg_8]                 
  141D56308  lea     rcx, unk_143465240                      -> &unk_143465240
  141D5630F  mov     dword ptr [rbp+300h+arg_8], r13d      
  141D56316  mov     [rax+178C0h], r13d                       <== HEAP WRITE
  141D5631D  call    sub_1403749A4                           -> sub_1403749A4
  141D56322  mov     rcx, rdi; lpCriticalSection           
  141D56325  call    cs:LeaveCriticalSection                 -> &LeaveCriticalSection
  141D5632B  lea     rcx, [rbp+300h+var_300]               
  141D5632F  call    sub_1401316A0                           -> sub_1401316A0
  141D56334  movaps  xmm6, [rsp+400h+var_58+8]             
  141D5633C  add     rsp, 3D8h                             
  141D56343  pop     r14                                   
  141D56345  pop     r13                                   
  141D56347  pop     rdi                                   
  141D56348  pop     rsi                                   
  141D56349  pop     rbx                                   
  141D5634A  pop     rbp                                   
  141D5634B  retn                                          
```  // 1104 insns

## callee sub_14013F898 (size 0x62)
```c
void __fastcall sub_14013F898(float *a1)
{
  float v1; // xmm4_4
  float v2; // xmm3_4
  float v3; // xmm5_4
  float v4; // xmm0_4

  v1 = a1[1];
  v2 = *a1;
  v3 = a1[2];
  v4 = fsqrt((float)((float)(v2 * v2) + (float)(v1 * v1)) + (float)(v3 * v3));
  if ( v4 != 0.0 )
  {
    *a1 = v2 * (float)(1.0 / v4);
    a1[1] = v1 * (float)(1.0 / v4);
    a1[2] = v3 * (float)(1.0 / v4);
  }
}
```

## callee sub_1402DCFF8 (size 0x19)
```c
void *__fastcall sub_1402DCFF8(__int64 a1)
{
  __int64 v1; // rax

  v1 = *(_QWORD *)(a1 + 216);
  if ( v1 )
    return (void *)(v1 + 56);
  else
    return &unk_1438138C8;
}
```

## callee sub_1403749A4 (size 0x35)
```c
void __fastcall sub_1403749A4(__int64 a1, int *a2)
{
  bool v2; // zf
  int v3; // r9d
  int v4; // r8d
  int v5; // eax

  v2 = *(_BYTE *)(a1 + 64) == 0;
  v3 = *(_DWORD *)(a1 + 48);
  v4 = *a2;
  *(_DWORD *)(a1 + 48) = *a2;
  if ( !v2 )
  {
    v5 = *(_DWORD *)(a1 + 52);
    if ( v4 >= v5 )
      v5 = v4;
    v4 = v5;
    if ( *(_DWORD *)(a1 + 56) < v5 )
      v4 = *(_DWORD *)(a1 + 56);
    *(_DWORD *)(a1 + 48) = v4;
  }
  if ( v3 != v4 )
    sub_1403749DC();
}
```

## callee sub_14058E98C (size 0x82)
```c
__int64 __fastcall sub_14058E98C(__int64 a1, __int64 a2, __int64 a3)
{
  int v5; // eax
  __int64 v7; // [rsp+20h] [rbp-20h] BYREF
  int v8; // [rsp+28h] [rbp-18h]
  __int64 v9; // [rsp+30h] [rbp-10h]
  int v10; // [rsp+58h] [rbp+18h] BYREF
  __int64 v11; // [rsp+68h] [rbp+28h] BYREF

  v7 = a1;
  if ( (unsigned __int8)sub_1402ADD28(a1, a3, &v10, &v11) )
  {
    v8 = v10;
    v9 = v11;
  }
  else
  {
    v5 = *(_DWORD *)(a1 + 12);
    v9 = 0;
    v8 = v5;
  }
  *(_QWORD *)a2 = 0;
  if ( (__int64 *)a2 != &v7 )
  {
    *(_QWORD *)a2 = v7;
    *(_DWORD *)(a2 + 8) = v8;
    *(_QWORD *)(a2 + 16) = v9;
  }
  return a2;
}
```

## callee sub_14056C530 (size 0xD6)
```c
__int64 __fastcall sub_14056C530(_DWORD *a1, __int64 a2)
{
  *(_OWORD *)a2 = xmmword_1431EFE70;
  *(_OWORD *)(a2 + 16) = xmmword_1431EFFC0;
  *(_OWORD *)(a2 + 32) = xmmword_1431F0390;
  *(_OWORD *)(a2 + 48) = xmmword_1431F0E80;
  *(_DWORD *)a2 = *a1;
  *(_DWORD *)(a2 + 4) = a1[4];
  *(_DWORD *)(a2 + 8) = a1[8];
  *(_DWORD *)(a2 + 16) = a1[1];
  *(_DWORD *)(a2 + 20) = a1[5];
  *(_DWORD *)(a2 + 24) = a1[9];
  *(_DWORD *)(a2 + 32) = a1[2];
  *(_DWORD *)(a2 + 36) = a1[6];
  *(_DWORD *)(a2 + 40) = a1[10];
  *(_DWORD *)(a2 + 48) = a1[3];
  *(_DWORD *)(a2 + 52) = a1[7];
  *(_DWORD *)(a2 + 56) = a1[11];
  *(_DWORD *)(a2 + 12) = 0;
  *(_DWORD *)(a2 + 28) = _mm_shuffle_ps((__m128)xmmword_1431F0E80, (__m128)xmmword_1431F0E80, 85).m128_u32[0];
  *(_DWORD *)(a2 + 44) = _mm_shuffle_ps((__m128)xmmword_1431F0E80, (__m128)xmmword_1431F0E80, 170).m128_u32[0];
  *(_DWORD *)(a2 + 60) = _mm_shuffle_ps((__m128)xmmword_1431F0E80, (__m128)xmmword_1431F0E80, 255).m128_u32[0];
  *(float *)(a2 + 48) = (float)(int)a1[3] * 0.0000076293945;
  *(float *)(a2 + 52) = (float)(int)a1[7] * 0.0000076293945;
  *(float *)(a2 + 56) = (float)(int)a1[11] * 0.0000076293945;
  return a2;
}
```


# PART 6 — CORRECTION + the REAL planar-reflection path
**CORRECTION:** sub_141D54EE0 (PART 4/5) is NOT the mirror reflect — its caller sub_141D53FA0 =
'BuildTopLevelAccelerationStructure_Emissive' → it is RAY-TRACING BVH math (mis-picked by SIMD heuristic).
The real planar reflection builder is sub_14078A6B8 ('renderstage_planar_reflection'). Analyzing it + callees.

## sub_14078A6B8 callees (SIMD = reflect-matrix candidates)
  sub_14023AF5C x4 simd=0 size=0x30
  sub_1401ED9A4 x1 simd=1 size=0x7F
  sub_14078B4F4 x1 simd=0 size=0x65
  sub_140157ABC x2 simd=0 size=0x65
  sub_1401F0F80 x9 simd=0 size=0x288
  sub_14060A86C x2 simd=0 size=0x17
  sub_140787548 x1 simd=0 size=0x67
  sub_1401F3D20 x11 simd=0 size=0xD3
  sub_14077113C x1 simd=0 size=0x8C
  sub_1401FAAEC x1 simd=0 size=0x64
  sub_1401F9BBC x1 simd=0 size=0x492
  sub_1401F6350 x1 simd=4 size=0xAF
  sub_1401E2C94 x1 simd=143 size=0xBC4
  memset x1 simd=0 size=0x190
  sub_140289B24 x1 simd=0 size=0xA1
  sub_141D57280 x1 simd=0 size=0x8B
  sub_1406EC098 x1 simd=0 size=0x23
  sub_14020E130 x1 simd=0 size=0x15
  sub_140789724 x1 simd=58 size=0x9B1
  sub_1401EE3CC x1 simd=0 size=0x6A
  sub_1401F3A6C x8 simd=0 size=0x2B1
  sub_140775268 x1 simd=0 size=0x47
  sub_1401F5438 x1 simd=0 size=0x143
  sub_1401EE18C x1 simd=0 size=0x61
  sub_140153F94 x4 simd=0 size=0x16C
  sub_1401F5ABC x2 simd=0 size=0x68
  sub_1401F881C x4 simd=0 size=0xA5
  sub_1401E6EB0 x1 simd=0 size=0x15
  sub_14020DED8 x2 simd=1 size=0xF5

## sub_14078A6B8 disasm (camera/plane/reflect — first 200 insns)
```
  14078A6B8  mov     rax, rsp                              
  14078A6BB  mov     [rax+8], rbx                          
  14078A6BF  mov     [rax+10h], rdx                        
  14078A6C3  push    rbp                                   
  14078A6C4  push    rsi                                   
  14078A6C5  push    rdi                                   
  14078A6C6  push    r12                                   
  14078A6C8  push    r13                                   
  14078A6CA  push    r14                                   
  14078A6CC  push    r15                                   
  14078A6CE  lea     rbp, [rax-308h]                       
  14078A6D5  sub     rsp, 3D0h                             
  14078A6DC  and     [rbp+300h+arg_18], 0                  
  14078A6E3  mov     r13, rdx                              
  14078A6E6  mov     rcx, r13                              
  14078A6E9  movaps  xmmword ptr [rax-48h], xmm6           
  14078A6ED  mov     edx, 16h                              
  14078A6F2  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A6F7  test    al, al                                
  14078A6F9  jz      loc_14078AA5A                         
  14078A6FF  mov     r9, [r13+18h]                         
  14078A703  mov     edx, 14h                              
  14078A708  mov     rcx, r13                              
  14078A70B  mov     rbx, [r9+1D50h]                       
  14078A712  mov     r10, [r9+1D60h]                       
  14078A719  mov     [rbp+300h+var_338], rbx               
  14078A71D  mov     [rbp+300h+var_320], r10               
  14078A721  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A726  lea     r12d, [rdx-13h]                       
  14078A72A  mov     r15d, 7FFFFFFEh                       
  14078A730  test    al, al                                
  14078A732  jnz     loc_14078B2A4                         
  14078A738  mov     eax, [rbx+0DCh]                       
  14078A73E  sub     eax, r12d                             
  14078A741  cmp     eax, r15d                             
  14078A744  setnbe  al                                    
  14078A747  test    byte ptr [r9+3F4h], 8                 
  14078A74F  jnz     loc_14078B245                         
  14078A755  test    al, al                                
  14078A757  jnz     loc_14078B245                         
  14078A75D  mov     r8b, 1Bh                              
  14078A760  lea     rdx, [rsp+400h+var_3A0]               
  14078A765  mov     rcx, r13                              
  14078A768  call    sub_1401ED9A4                           -> sub_1401ED9A4
  14078A76D  movzx   eax, [rsp+400h+var_390]               
  14078A772  mov     r14d, 3                               
  14078A778  add     eax, r14d                             
  14078A77B  shr     eax, 2                                
  14078A77E  mov     [rbp+300h+var_370], eax               
  14078A781  cmp     eax, r12d                             
  14078A784  jnb     short loc_14078A78D                   
  14078A786  movzx   eax, r12w                             
  14078A78A  mov     [rbp+300h+var_370], eax               
  14078A78D  mov     [rsp+400h+var_390], ax                
  14078A792  mov     word ptr [rsp+400h+var_38C], ax       
  14078A797  movzx   eax, word ptr [rsp+400h+var_38E]      
  14078A79C  add     eax, r14d                             
  14078A79F  shr     eax, 2                                
  14078A7A2  mov     [rbp+300h+arg_18], eax                
  14078A7A8  cmp     eax, r12d                             
  14078A7AB  jnb     short loc_14078A7B7                   
  14078A7AD  movzx   eax, r12w                             
  14078A7B1  mov     [rbp+300h+arg_18], eax                
  14078A7B7  lea     r9, [rsp+400h+var_3A0]                
  14078A7BC  mov     word ptr [rsp+400h+var_38E], ax       
  14078A7C1  mov     r8d, 5C83AD89h                        
  14078A7C7  mov     word ptr [rsp+400h+var_38C+2], ax     
  14078A7CC  lea     rdx, [rbp+300h+var_2F8]               
  14078A7D0  call    sub_14078B4F4                           -> sub_14078B4F4
  14078A7D5  lea     r9, [rsp+400h+var_3A0]                
  14078A7DA  mov     [rbp+300h+var_37B], 12h               
  14078A7DE  mov     r8d, 34A28D5h                         
  14078A7E4  lea     rdx, [rbp+300h+var_308]               
  14078A7E8  mov     rcx, r13                              
  14078A7EB  call    sub_140157ABC                           -> sub_140157ABC
  14078A7F0  lea     r9, [rsp+400h+var_3A0]                
  14078A7F5  mov     [rbp+300h+var_37B], r12b              
  14078A7F9  mov     r8d, 4D3A702Ah                        
  14078A7FF  lea     rdx, [rbp+300h+var_318]               
  14078A803  mov     rcx, r13                              
  14078A806  call    sub_140157ABC                           -> sub_140157ABC
  14078A80B  test    [r13+30h], r12b                       
  14078A80F  jnz     loc_14078B2B0                         
  14078A815  mov     esi, [r13+34h]                        
  14078A819  mov     rcx, [r13+8]                          
  14078A81D  lea     rdx, [rsp+400h+var_3A0]               
  14078A822  mov     r8b, [r13+38h]                        
  14078A826  shl     esi, 18h                              
  14078A829  xor     esi, 15EAB19Ch                        
  14078A82F  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A834  mov     rcx, [rcx]                            
  14078A837  mov     byte ptr [rsp+400h+var_3A0], 4        
  14078A83C  mov     dword ptr [rsp+400h+var_3A0+4], esi   
  14078A840  mov     [rsp+400h+var_398], r12d              
  14078A845  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A84A  test    [r13+30h], r12b                       
  14078A84E  jnz     loc_14078B2B7                         
  14078A854  mov     edi, [r13+34h]                        
  14078A858  mov     rcx, [r13+8]                          
  14078A85C  lea     rdx, [rsp+400h+var_3A0]               
  14078A861  mov     r8b, [r13+38h]                        
  14078A865  shl     edi, 18h                              
  14078A868  xor     edi, 3A4A52ABh                        
  14078A86E  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A873  mov     rcx, [rcx]                            
  14078A876  mov     byte ptr [rsp+400h+var_3A0], 4        
  14078A87B  mov     dword ptr [rsp+400h+var_3A0+4], edi   
  14078A87F  mov     [rsp+400h+var_398], r12d              
  14078A884  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A889  mov     edx, 14h                              
  14078A88E  mov     qword ptr [rbp+300h+var_2D0], r13     
  14078A892  mov     rcx, r13                              
  14078A895  mov     dword ptr [rbp+300h+var_2D0+8], edi   
  14078A898  call    sub_14023AF5C                           -> sub_14023AF5C
  14078A89D  lea     ebx, [rdx-12h]                        
  14078A8A0  test    al, al                                
  14078A8A2  jnz     loc_141EF766C                         
  14078A8A8  mov     [rbp+300h+var_358], 0FD3DDE00h        
  14078A8AF  lea     rdx, [rbp+300h+var_360]               
  14078A8B3  xor     ecx, ecx                              
  14078A8B5  mov     eax, ebx                              
  14078A8B7  and     qword ptr [rdx], 0                    
  14078A8BB  mov     r15d, [rdx+8]                         
  14078A8BF  mov     [rbp+300h+var_320], rcx               
  14078A8C3  mov     [rbp+300h+var_2C0], rcx               
  14078A8C7  mov     [rbp+300h+var_2B8], r15d              
  14078A8CB  test    bl, al                                
  14078A8CD  jz      short loc_14078A909                   
  14078A8CF  mov     rcx, [rbp+300h+var_360]               
  14078A8D3  and     eax, 0FFFFFFFDh                       
  14078A8D6  mov     dword ptr [rsp+400h+var_3A8], eax     
  14078A8DA  test    rcx, rcx                              
  14078A8DD  jz      short loc_14078A909                   
  14078A8DF  mov     r8b, [rcx+38h]                        
  14078A8E3  lea     rdx, [rsp+400h+var_3A0]               
  14078A8E8  mov     rcx, [rcx+8]                          
  14078A8EC  mov     eax, [rbp+300h+var_358]               
  14078A8EF  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A8F4  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A8F9  mov     rcx, [rcx]                            
  14078A8FC  mov     dword ptr [rsp+400h+var_3A0+4], eax   
  14078A900  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A905  mov     eax, dword ptr [rsp+400h+var_3A8]     
  14078A909  test    r12b, al                              
  14078A90C  jnz     loc_141EF76D8                         
  14078A912  test    [r13+30h], bl                         
  14078A916  jz      short loc_14078A95A                   
  14078A918  mov     rax, [r13+18h]                        
  14078A91C  mov     r9, [rax+1E10h]                       
  14078A923  lea     rax, aRenderstagePla; "renderstage_planar_reflection"  -> &aRenderstagePla
  14078A92A  mov     rcx, rax                              
  14078A92D  mov     [rbp+300h+var_360], rax               
  14078A931  call    sub_14060A86C                           -> sub_14060A86C
  14078A936  movsd   xmm0, [rbp+300h+var_360]              
  14078A93B  lea     rdx, [rbp+300h+var_360]               
  14078A93F  xor     r8d, r8d                              
  14078A942  movsd   [rbp+300h+var_360], xmm0              
  14078A947  mov     rcx, r9                               
  14078A94A  mov     [rbp+300h+var_358], eax               
  14078A94D  call    sub_140787548                           -> sub_140787548
  14078A952  test    al, al                                
  14078A954  jnz     loc_14078AA7A                         
  14078A95A  mov     rax, [rbp+300h+var_320]               
  14078A95E  test    rax, rax                              
  14078A961  jz      short loc_14078A987                   
  14078A963  mov     rcx, [rax+8]                          
  14078A967  lea     rdx, [rsp+400h+var_3A0]               
  14078A96C  mov     r8b, [rax+38h]                        
  14078A970  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A975  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A97A  mov     rcx, [rcx]                            
  14078A97D  mov     dword ptr [rsp+400h+var_3A0+4], r15d  
  14078A982  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A987  mov     rcx, [r13+8]                          
  14078A98B  lea     rdx, [rsp+400h+var_3A0]               
  14078A990  mov     r8b, [r13+38h]                        
  14078A994  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A999  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A99E  mov     rcx, [rcx]                            
  14078A9A1  mov     dword ptr [rsp+400h+var_3A0+4], edi   
  14078A9A5  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A9AA  mov     rcx, [r13+8]                          
  14078A9AE  lea     rdx, [rsp+400h+var_3A0]               
  14078A9B3  mov     r8b, [r13+38h]                        
  14078A9B7  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A9BC  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A9C1  mov     rcx, [rcx]                            
  14078A9C4  mov     dword ptr [rsp+400h+var_3A0+4], esi   
  14078A9C8  call    sub_1401F0F80                           -> sub_1401F0F80
  14078A9CD  mov     rdx, [rbp+300h+var_318]               
  14078A9D1  test    rdx, rdx                              
  14078A9D4  jz      short loc_14078A9FC                   
  14078A9D6  mov     rcx, [rdx+8]                          
  14078A9DA  mov     r8b, [rdx+38h]                        
  14078A9DE  lea     rdx, [rsp+400h+var_3A0]               
  14078A9E3  mov     eax, [rbp+300h+var_310]               
  14078A9E6  mov     byte ptr [rsp+400h+var_3A0+1], r8b    
  14078A9EB  mov     rcx, [rcx]                            
  14078A9EE  mov     byte ptr [rsp+400h+var_3A0], 5        
  14078A9F3  mov     dword ptr [rsp+400h+var_3A0+4], eax   
```

## top reflect-candidate callees (decompile)

### sub_1401E2C94 simd=143 size=0xBC4 callers:39
*(no decompile — disasm head)*
```
  1401E2C94  mov     rax, rsp
  1401E2C97  mov     [rax+8], rbx
  1401E2C9B  mov     [rax+10h], rsi
  1401E2C9F  mov     [rax+18h], rdi
  1401E2CA3  push    rbp
  1401E2CA4  push    r14
  1401E2CA6  push    r15
  1401E2CA8  lea     rbp, [rax-878h]
  1401E2CAF  sub     rsp, 960h
  1401E2CB6  movaps  xmmword ptr [rax-28h], xmm6
  1401E2CBA  mov     rsi, rcx
  1401E2CBD  movaps  xmmword ptr [rax-38h], xmm7
  1401E2CC1  lea     rcx, [rbp+870h+var_450]
  1401E2CC8  movaps  xmmword ptr [rax-48h], xmm8
  1401E2CCD  mov     rdi, rdx
  1401E2CD0  movaps  xmmword ptr [rax-58h], xmm9
  1401E2CD5  movaps  xmmword ptr [rax-68h], xmm10
  1401E2CDA  movaps  xmmword ptr [rax-78h], xmm11
  1401E2CDF  movaps  xmmword ptr [rax-88h], xmm12
  1401E2CE7  movaps  xmmword ptr [rax-98h], xmm13
  1401E2CEF  movaps  xmmword ptr [rax-0A8h], xmm14
  1401E2CF7  movaps  xmmword ptr [rax-0B8h], xmm15
  1401E2CFF  mov     r14d, r9d
  1401E2D02  mov     r15d, r8d
  1401E2D05  call    sub_14028DB28
  1401E2D0A  and     dword ptr [rsp+970h+var_948], 0
  1401E2D0F  lea     rcx, [rbp+870h+var_450]
  1401E2D16  mov     eax, 1
  1401E2D1B  xorps   xmm2, xmm2
  1401E2D1E  mov     r9d, eax
  1401E2D21  mov     [rsp+970h+var_950], eax
  1401E2D25  xorps   xmm1, xmm1
  1401E2D28  call    sub_140787D84
  1401E2D2D  movups  xmm0, xmmword ptr [rdi+2C0h]
  1401E2D34  lea     rcx, [rbp+870h+Dst]; Dst
  1401E2D3B  movups  xmm2, xmmword ptr [rdi+2D0h]
  1401E2D42  xor     edx, edx; Val
  1401E2D44  movups  xmm1, xmmword ptr [rdi+2E0h]
  1401E2D4B  mov     r8d, 220h; Size
  1401E2D51  movups  xmm9, xmmword ptr [rdi+2B0h]
  1401E2D59  movaps  xmm13, [rbp+870h+var_1A0]
  1401E2D61  movaps  xmm8, xmm9
  1401E2D65  movaps  xmm11, [rbp+870h+var_180]
  1401E2D6D  movaps  xmm12, xmm13
  1401E2D71  mov     bl, [rdi+384h]
  1401E2D77  movaps  xmm10, xmm11
  1401E2D7B  shufps  xmm12, [rbp+870h+var_190], 44h ; 'D'
  1401E2D84  shufps  xmm13, [rbp+870h+var_190], 0EEh
  1401E2D8D  shufps  xmm10, [rbp+870h+var_170], 44h ; 'D'
  1401E2D96  shufps  xmm11, [rbp+870h+var_170], 0EEh
  1401E2D9F  shufps  xmm8, xmm0, 44h ; 'D'
  1401E2DA4  shufps  xmm9, xmm0, 0EEh
  1401E2DA9  movaps  xmm7, xmm8
  1401E2DAD  movaps  xmm0, xmm2
  1401E2DB0  movaps  xmm6, xmm9
  1401E2DB4  shufps  xmm0, xmm1, 44h ; 'D'
  1401E2DB8  shufps  xmm2, xmm1, 0EEh
  1401E2DBC  shufps  xmm7, xmm0, 88h
  1401E2DC0  shufps  xmm8, xmm0, 0DDh
  1401E2DC5  shufps  xmm6, xmm2, 88h
```

### sub_140789724 simd=58 size=0x9B1 callers:4
*(no decompile — disasm head)*
```
  140789724  mov     rax, rsp
  140789727  mov     [rax+20h], r9d
  14078972B  mov     [rax+18h], r8d
  14078972F  push    rbp
  140789730  push    rbx
  140789731  push    rsi
  140789732  push    rdi
  140789733  push    r12
  140789735  push    r13
  140789737  push    r14
  140789739  push    r15
  14078973B  lea     rbp, [rax-0B98h]
  140789742  sub     rsp, 0C58h
  140789749  mov     r12, [rdx+18h]
  14078974D  mov     r14, rcx
  140789750  mov     rcx, [rdx]
  140789753  mov     edi, r9d
  140789756  movaps  xmmword ptr [rax-58h], xmm6
  14078975A  mov     ebx, r8d
  14078975D  movaps  xmmword ptr [rax-68h], xmm7
  140789761  mov     rsi, rdx
  140789764  movaps  xmmword ptr [rax-78h], xmm8
  140789769  movaps  xmmword ptr [rax-88h], xmm9
  140789771  movaps  xmmword ptr [rax-98h], xmm10
  140789779  movaps  xmmword ptr [rax-0A8h], xmm11
  140789781  movaps  xmmword ptr [rax-0B8h], xmm12
  140789789  movaps  xmmword ptr [rax-0C8h], xmm13
  140789791  movaps  xmmword ptr [rax-0D8h], xmm14
  140789799  movaps  xmmword ptr [rax-0E8h], xmm15
  1407897A1  mov     rax, [rcx]
  1407897A4  call    qword ptr [rax+20h]
  1407897A7  mov     rdx, [rsi+18h]
  1407897AB  movss   xmm0, cs:dword_1431EF260
  1407897B3  movss   xmm1, cs:dword_1431EF2FC
  1407897BB  movss   xmm14, dword ptr [rax+720h]
  1407897C4  mov     rcx, [rdx+1D50h]
  1407897CB  lea     rdx, [rbp+0B90h+arg_8]
  1407897D2  movss   xmm15, dword ptr [rax+724h]
  1407897DB  mov     [rsp+0C90h+var_C70], rcx
  1407897E0  lea     rcx, [rbp+0B90h+arg_0]
  1407897E7  movss   [rbp+0B90h+arg_0], xmm0
  1407897EF  movss   [rbp+0B90h+arg_8], xmm1
  1407897F7  call    sub_14078A0D8
  1407897FC  movss   xmm0, dword ptr [r12+90h]
  140789806  mulss   xmm0, cs:dword_1431EEFA4; X
  14078980E  call    tanf
  140789813  movss   xmm1, dword ptr [r12+9Ch]
  14078981D  lea     rcx, [r14+40h]; Dst
  140789821  movss   xmm12, dword ptr [r12+98h]
  14078982B  movaps  xmm9, xmm0
  14078982F  maxss   xmm1, cs:dword_1431EEE5C
  140789837  movaps  xmm6, cs:xmmword_1431EFE70
  14078983E  mov     r13d, 40h ; '@'
  140789844  movaps  xmm7, cs:xmmword_1431EFFC0
  14078984B  mov     r8d, r13d; Size
  14078984E  movaps  xmm10, cs:xmmword_1431F0390
  140789856  xor     edx, edx; Val
  140789858  movaps  xmm11, cs:xmmword_1431F0E80
  140789860  movups  xmmword ptr [r14], xmm6
  140789864  movups  xmmword ptr [r14+10h], xmm7
```

## PART 6 — FINDING (desk reverse): the reflection is computed in the SCENE-NODE layer, not the render layer
- sub_141D54EE0 = RT BVH ("BuildTopLevelAccelerationStructure_Emissive"), NOT mirror reflect. (mis-picked by SIMD heuristic; corrected.)
- The planar builder sub_14078A6B8 ("renderstage_planar_reflection") calls only GENERIC matrix code:
  sub_1401E2C94 (simd 143 but **39 callers** = generic 4x4 transform/transpose, reads [rdi+0x2B0..0x2E0]) and
  sub_140789724 (simd 58 = generic view+proj const builder, reads camera [a2+0x18]). Neither is mirror-specific.
- THEREFORE: there is NO distinct "reflect across plane" function in the RENDER layer. The render layer is fed a
  camera that is ALREADY reflected. The reflection reflect(main_camera, mirror_plane) is computed UPSTREAM in the
  **worldMirrorNode scene-node processing** (gameplay/scene-traversal layer), and the render path treats the
  reflected camera as a normal per-view camera (which is why sub_140788A9C only ever showed the main view +
  cascades — the mirror's reflected camera enters the view list from the scene node, separately).
- CONTROLLABLE INPUT (confirmed by the whole chain): the **worldMirrorNode's transform/plane** (a placed scene
  node). The reflected camera = reflection of the main camera across that plane. To change what the mirror shows,
  change the PLANE (scene-node transform) — a RED4ext/Codeware-accessible world entity — not a render hook.
- For a stereo eye: a plane reflection is always det−1 (handedness flipped). Only a plane positioned so the
  reflection ≈ a forward view + a 2D horizontal flip of the captured RT could yield an eye image — and the
  apartment gate (m_isInMirrorsArea) still applies. This is the structural ceiling of the mirror lever.

## PART 7 — RED4ext SDK check: the mirror node exposes NO runtime-controllable reflection/plane/camera
From externals/RED4ext.SDK Generated headers (game reflection data):
- `world::Node` (serialized base, size 0x38) has NO transform field — only isVisibleInGame/isHostOnly. The node's
  world placement (position/orientation = the MIRROR PLANE) is stored in the STREAMING SECTOR (baked at world
  build), applied to the NodeInstance at instantiation — NOT a settable node property.
- `world::MirrorNode : MeshNode` (size 0x78) adds only cullingBoxExtents (+0x60) / cullingBoxOffset (+0x6C). NO
  plane / reflection / camera / mirrorType field.
- `world::MirrorNodeInstance : MeshNodeInstance` (size 0x100) is EMPTY — zero own reflection fields.
- `RenderProxyCustomData_Mirror` (size 0x40) = 0x30 unnamed bytes (no named fields).
DECISIVE: there is NO RED4ext/Codeware-settable handle on the mirror's reflection or plane. The plane is
world-baked; the reflected camera is native-computed. Combined with PART 6 (no render-layer reflect fn) this
closes the mirror lever at BOTH layers: render-side (generic, fed an already-reflected cam) AND scene-side
(no exposed fields, plane baked in the streaming sector). Even with full native control of the node transform,
the output is a det−1 plane reflection (flipped, not a stereo eye) and gated to mirror-areas. The mirror is a
fully-mapped DEAD END as a right-eye vehicle — documented end to end in this file.
