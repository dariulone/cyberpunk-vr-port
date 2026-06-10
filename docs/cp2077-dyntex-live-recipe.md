---
name: cp2077-dyntex-live-recipe
description: live DynamicTexture wiring recipe captured via sub_1412BAACC hook — resref@0x1E0 is depot hash, handle is GPU render-texture class 0x2B1BAA0
metadata:
  type: project
---

Hooked sub_1412BAACC (RVA 0x12BAACC, DynTexWire, cold/F8-installed) and captured a LIVE
engine-wired DynamicTexture from the inventory puppet-preview. This is the recipe option-C needs.

**RTT-camera component layout (working binding):**
- `comp+0x1E0` (`m_dynamicTextureRes`) = **8-byte depot resref HASH** (raRef), e.g. 0xD55FB557EAEBA05B.
  Our CreateInstance shell had this = 0 -> no handle -> no render (the proven no-op crux).
- `comp+0x1E8` = resolved GPU handle pointer (filled by the depot once 0x1E0 holds a valid resref).

**Bound GPU render-texture object (handle, class vtable RVA 0x2B1BAA0):**
- +0x00 vtable (RVA 0x2B1BAA0); +0x08 self-ptr (intrusive ref idiom)
- +0x30 and +0x80 = the SAME depot resref hash (token identity)
- +0x40 = resolution WxH (720x720 = 0x2D0 for puppet-preview)
- +0x54 = format/flags (0x00010006)
- +0x98 = in-module sub-object ptr (RVA 0x31A9280) — generator/descriptor candidate
- +0xB0 = heap sub-structure ptr; followed by an ascending small-int array (rt-id/slot table)

**Decisive consequence:** the remaining crux is reduced to ONE primitive — produce a
depot-registered GPU render-texture (class 0x2B1BAA0) and obtain its resref, then put that resref in
comp+0x1E0; the engine auto-loads (sub_1423E7768) + registers the camera (sub_1412BAACC ->
sub_1423E4658) + renders. Reverse the constructor of class 0x2B1BAA0 to get create+depot-register.
Corrects [[cp2077-rtt-no-drivable-api]] direction with a concrete in-process recipe.
See [[cp2077-render-texmgr-handle]] (texMgr handle), [[cp2077-rtt-instance-vtable]].
