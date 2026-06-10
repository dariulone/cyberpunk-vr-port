---
name: cp2077-view-item-layout
description: Type-A render view-item object (the thing fanned out 1-build-per-view) - vtable rva 0x2AC8688, ctor sub_140294C94, ~0xA70+ bytes, dims 3072 at +0x10; loop hook sub_140167EB4 is unwind-sensitive (crashes on inventory)
metadata:
  type: project
---

Live capture (ViewFanout read-only hook on sub_140167EB4, gameplay) of the Type-A view-item (viewArray[0]):
- `it+0x00` = vtable rva **0x2AC8688**, `it+0x08` = refcount = 1
- `it+0x10` = **0xC00, 0xC00 = 3072 x 3072** (render dims; matches present-RT census)
- `it+0x40..+0x68` = 6 heap sub-object ptrs (camera / RT-set / passes)
- ctor = **sub_140294C94**: sets vtable+refcount=1, calls sub_140294D14 (inits +0x10), then a 4x loop (stride 0x48) writing clamped float view-params at this+0xA70 from static tables => object is LARGE (>0xA70 bytes), structured. dtor-ish = sub_140294BF4.
- 3 ctor callers: sub_140293C7C(0x6b), sub_1404FBAFC(0x3ef), sub_1428CAAFC(0x2c5) - view-item built in several engine paths.

**Gameplay viewArray = 1 item (this view), so 1 FG-build; inventory adds a 2nd.** To get simultaneous, the manager's viewArray ([mgr+0x08], read by sub_140167E28) must hold a 2nd right-eye item.

**WARNING:** hooking the fan-out loop sub_140167EB4 directly is **unwind-sensitive** - works in gameplay but CRASHES on inventory open with base-5 READ in ntdll unwind machinery (same hostility class as the old dtw-hook). The deployed dxgi (11:34) installs this hook on F7 => inventory crashes after F7. Injection must be done at a safe point (manager array / a non-unwind wrapper), and likely gameplay-only (inventory throws through this fn). Relates [[cp2077-view-fanout-submit-chain]] [[cp2077-per-view-independent-fgbuild]].
