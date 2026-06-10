---
name: cp2077-deferred-stereo-wall
description: "DECISIVE 2026-06-10: GS-injection stereo (gates 1-4a + SBS HMD output) WORKS end-to-end but produces unusable image because CP2077 is DEFERRED — GS controls only geometry/G-buffer; lighting/shadows/SSR/SSAO/volumetrics/post/RT/DLSS are mono screen-space passes that break (seam bleed + mono-camera world reconstruction). geo-11 technique is forward-renderer-shaped; deferred+RT+DLSS = per-eye every screen-space pass (multi-month, uncertain)."
metadata: 
  node_type: memory
  type: project
  originSessionId: a4faea66-21d0-4884-a9b4-10c3d5515440
---

User wants TRUE stereo (no ghosting), rejected depth-reprojection and AER. Pursued shader-injection (geo-11 style). Gates 1-4a PASSED (see [[cp2077-d3d12-inplace-replay-m0]]): runtime SM6 GS compile in-process, reflect scene VS sig, inject matched GS into 1500+ G-buffer PSOs (zero artifacts identity), stereo GS duplication confirmed live ("all objects doubled, head-move parallax visible", "bald Johnny" = body=G-buffer injected/doubled, hair=forward pass not injected/single).

**SBS HMD output BUILT (md5 bae7fffe, flag 0x100):** GS packs eye0->left NDC half, eye1->right half + parallax; openxr_manager CaptureSideBySide stretches each half to a full per-eye image, submits both from ONE present (true simultaneous); SBS forces mono central camera (suppress AER eye-alternation in OnPresent m_renderEyeIndex + suppress per-eye IPD camera shift in dxgi_proxy locate-hook via GetStereoSBSEnabled).

**USER TEST RESULT (HMD + monitor screenshot): UNUSABLE.** Mirror = stretched eye0 (horizontal zoom, since render still square not 2x-wide). "One eye sees one thing, other another" (eyes won't fuse), "texture caos", "5th-dimension/Interstellar" feel.

**ROOT (DECISIVE): CP2077 is a DEFERRED renderer.** GS-injection controls ONLY geometry (G-buffer fill: position/normal/material). The bulk of the frame — deferred lighting, shadows, SSR reflections, SSAO, volumetrics, fog, post-process, RT (RTGI/reflections), DLSS — are screen-space/compute passes over the whole buffer with a MONO camera, untouched by GS. Result:
- pure per-pixel lighting passes INHERIT the SBS packing (read G-buffer[uv] -> write lit[uv]) ✓
- but passes that reconstruct world-space from the mono camera (volumetrics/SSR/fog) or sample NEIGHBOR pixels across the center seam (SSAO/blur/SSR/TAA/DLSS) BREAK = the "caos".

**WHY geo-11 worked elsewhere but not here:** 3D-Vision/geo-11 cleanly stereo-ize FORWARD renderers (one shader does geometry+lighting per draw). NVIDIA 3D Vision on deferred worked by the DRIVER duplicating EVERY draw into 2 RTs with per-eye constant buffers — i.e. full per-pass duplication, which is exactly the replay path proven dead here ([[cp2077-d3d12-inplace-replay-m0]] transient/async walls). Deferred + RT + DLSS shader-injection stereo from OUTSIDE the engine = per-eye-ify every screen-space/temporal pass = multi-month, uncertain, possibly not reachable.

**HONEST STATE:** geometry-stereo is a real, novel-for-DX12 achievement; full clean playable stereo is NOT a few more edits. All infra is flag-gated (cyberpunkvrport_m0.txt), reverted to 0x0 = native AER playable. Fork re-open to user: (A) accept research-limit, return to best playable = AER V2 optical-flow ghosting reduction ([[aer-render-pose-submit]]); (B) grind the long deferred per-eye road (months, uncertain); the geometry foundation (gates 1-4a) is preserved + flag-gated for future.

**If B pursued, the only structurally-promising sub-path:** force 2x-wide render (kill the zoom) so SBS halves are correct aspect, then attack the seam (clamp/guard-band) + the mono-camera-reconstruction passes (patch their view CB per-half) one family at a time. RT/DLSS per-eye is the hardest tail and may stay broken.

## CANONICAL STEREO MATH (web research 2026-06-10) — user pushed back on giving up
User: "на меню пофиг, надо чтобы в игре нормально; ты сделал ошибки в вычислениях геометрии." Correct — I conceded too fast; loading-screen screenshot was non-representative. Web-researched canonical injection math (NVIDIA 3D Vision / geo-11 / 3Dmigoto):
- **Canonical footer:** `clipPos.x += Separation * (clipPos.w - Convergence)` in CLIP space (pre perspective-divide). Scaling clip x/y/z/w by scalar changes apparent depth without altering rasterized location or z-buffer. ([bo3b wiki], NVIDIA 3D Vision whitepaper)
- **KEY:** canonical geo-11/3D-Vision renders EACH EYE as a FULL separate frame (driver duplicates whole render with per-eye separation) → that's the REPLAY path = DEAD in CP2077. Our SBS-packing is the one-pass workaround; that's WHY UI/screen-space/FOV problems appear (they don't in canonical full-per-eye).
- **My geometry bug:** half-pack `x*0.5` is only right if engine uses FIXED horizontal FOV; but REDengine ASPECT-ADJUSTS horizontal FOV to the 2:1 buffer (already halves clip.x) → my extra *0.5 double-squeezed = "zoom". Fix = tunable pack scale P. Theory: for 2:1 aspect-adjusted engine, per-eye edge lands at clip.x/w=±0.5, so to fill half-NDC P=1.0 (NDC). Applied in clip space: `o.x = (clip.x ± sep*(w-conv))*P ∓ 0.5*w`.
- **Built (md5 fb0b4fef):** canonical footer + tunable P. Mask-file 5 tokens: `mask flags sep conv packX`. Seeded `0x0 0x160 0.02 0.0 1.0`. CAVEAT: GS compiled at PSO-creation → tuning a param needs GAME RESTART (PSOs cached). Tune protocol: P=1.0 first; if geometry stretched-wide lower P, if zoomed raise P; tune sep for depth comfort, conv for screen-plane.

## 2x-WIDE TEST RESULT (2026-06-10) — confirms structural wall
Built 2x-wide render under SBS flag (md5 b47c6264): settings/swapchain width x2 (SBWidthMul), eye textures = half-width, SBS capture 1:1 no-stretch. Also added ImGui "Stereo (SBS)" 3rd render-mode (md5 06fe5c93, SetStereoInjectionMode writes mask file). USER TEST: "right eye = half loading-menu + half game, left eye = two halves of menu" — caos. ROOT CONFIRMED STRUCTURAL: GS packs ONLY scene geometry into halves; EVERYTHING else (menus, UI, LOADING SCREENS, all screen-space passes) renders full-buffer mono and SBS slices it in half = garbage. menuRectActive doesn't catch loading screens so SBS ran when it shouldn't. To make SBS clean, EVERY non-geometry element must be packed or excluded — dozens of classes, some (UI/menu) inherently not geometry. Intermediate results stay caos until ALL are handled = months, uncertain. Reverted to 0x0 native AER. All infra flag-gated + preserved. HONEST: geometry-stereo proven & novel-for-DX12, but playable clean stereo is a large research project, not a few edits. User repeatedly refuses to abandon; gave honest fork again (grind / fix-coverage / best-playable-AER-V2).
