---
name: cp2077-viewfamily-layout
description: CP2077 option-C view-element collection layout (group descriptors, spare capacity) read live via the sub_140952C28 iterator hook
metadata:
  type: project
---

Read the live view-element collection layout by hooking the iterator sub_140952C28 (RVA 0x952C28; first insn mov [rsp+8],rbx = clean 5-byte E9 patch). Args: a1=group-descriptor array base, a4=startGroup, a5=endGroup; called twice/frame with ranges [0,4) and [5,0xD) (group 4 is skipped).

**Group descriptor = 16 bytes:** ptr@0 (elements array base), **capacity (uint)@+8**, **count (uint)@+12**. Element stride = 0x80; active flag @ +0x78; scene-view object ptr @ elem+0x08; stage-vtable @ elem+0x60; stage const @ elem+0x00 (0xC00 main / 0xC03 secondary). Element arrays are heap; collection is per-frame transient (addresses change between frames/sessions).

**Live sample (in-world):** group0 count=16 cap=19 (+3 spare, slot0 = main "03_night_city" scene view), group3 count=29 cap=42 (+13), group5 24/28, group11 15/19. Every live group has SPARE CAPACITY past count => a right-eye element can be appended into a spare slot (bump count, fill slot[count]) WITHOUT realloc => memory-safe. Groups 1/12 have count=0 cap=0 with exe-module ptrs (static/empty, ignore).

**Status:** injection is now memory-safe (spare capacity confirmed). BUT a shallow 128-byte clone of slot0 shares elem+0x08 (same scene-view OBJECT) => scheduling it is idempotent (same as the dead M3 8x test [[cp2077-viewfamily-iterator]]) => no 2nd render. A meaningful right-eye render REQUIRES a DISTINCT scene-view object. Next: analyze the main scene-view object's vtable (obj+0x00, e.g. 0x547229A0 region) to find a construct/clone path, OR borrow the engine's mirror/reflection view-object creation. See [[cp2077-view-injection-mirror]], [[redengine-render-view-state]].
