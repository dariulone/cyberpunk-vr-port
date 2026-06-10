---
name: cp2077-rtt-camera-native-stereo
description: option-C FRONT solved natively via entRenderToTextureCameraComponent (RTTI-driven 2nd full-scene view = right eye)
metadata:
  type: project
---

CP2077 true-simultaneous-stereo (option C) FRONT part is solvable NATIVELY, no FrameGraph monolith reverse, no hot-patch. RTTI dump found **entRenderToTextureCameraComponent** (base entBaseCameraComponent, size 0xA10) — a data-driven render-to-texture camera with exactly the needed fields:

- camera: localTransform@0xC0, worldTransform@0xE0, parentTransform(handle:entITransformBinding)@0x90, fov@0x128, nearPlaneOverride@0x168, farPlaneOverride@0x16c, aspectRatio@0x9e8, zoom@0x15c
- OUTPUT RTs (the per-view RT we could NOT find at view/spec/camobj level — here explicit): dynamicTextureRes(DynamicTexture)@0x1e0 = color; depth/albedo/normals/particles DynamicTextureRes@0x1f8/210/228/240 = full G-buffer/MRT => renders FULL deferred scene, not reduced; resolutionWidth@0x258 / resolutionHeight@0x25c
- control: isEnabled@0x8b, renderingMode(entRenderToTextureMode)@0x290, features(entRenderToTextureFeatures)@0x9f6
- features struct (size 8): renderDecals, renderParticles, renderForwardNoTXAA, antiAliasing(enum), contactShadows, localShadows, SSAO(enum), reflections(enum) => per-eye pass toggles = the C optimization lever
- NO member/static funcs => purely set-properties + isEnabled; engine renders it as another view; FrameGraph auto-allocates its RT (confirmed [[cp2077-framegraph-rt-routing]]).

Plan: LEFT = native main view; RIGHT = an entRenderToTextureCameraComponent on player, localTransform=head+IPD, fov/near/far matched, dynamicTextureRes=our DynamicTexture(eye res), isEnabled=true => both eyes from ONE sim frame = true stereo. BACK = grab that DynamicTexture's ID3D12Resource via d3d12 proxy and submit as right eye.

Supersedes the pessimism in [[cp2077-952xx-is-generic-task-scheduler]] / view-injection dead-ends: native lever exists. entRenderToTextureMode + entRenderToTextureFeaturesPlatform are ENUMS (GetClass null). Other RTTI hits: worldMirrorNode/Instance + RenderProxyCustomData_Mirror (the apartment mirror node path), renderDevEnvProbeView/GIProbeView. red4ext_plugin is CyberpunkVR_Hands.dll (builds in src/red4ext_plugin/build, deploy to red4ext/plugins/CyberpunkVR_Hands/, Vortex-managed).

## Attach API (Codeware) + confirmed in-game user
- entRenderToTextureCameraComponent extends entCameraComponent (RTTI: base entBaseCameraComponent). renderingMode enum: Shaded=0 (full scene), GBufferOnly=1. featuresPlatform enum: RTFP_All=0, RTFP_PC=1, RTFP_PC_PS5_XSX=2, RTFP_Consoles=3, RTFP_None=4 (set RTFP_None to disable SSAO/reflections/AA per-eye).
- gameuiHolocallCameraComponent EXTENDS entRenderToTextureCameraComponent => holocalls already drive this (live scene->texture) = proof the mechanism works & is game-maintained.
- DynamicTexture extends ITexture: width/height/dataFormat(DynamicTextureDataFormat)/scaleToViewport/mipChain/samplesCount/generator(IDynamicTextureGenerator). Generators incl CRenderTextureMaterial, ImageTextureGenerator.
- Codeware attach: CallbackSystem.Get():RegisterCallback(n"Entity/Assemble", target, cb) with EntityTarget.Type(n"PlayerPuppet"); in cb EntityBuilderEvent:GetEntityBuilder():AddComponent(comp). EntityBuilderWrapper/Template/Appearance each have AddComponent(ref<IComponent>). Codeware also has SpawnEntity/GetEntityComponent spawner API + EntityBuilder for fresh entities.
- Env has CET + Codeware + ArchiveXL installed. CET Lua print -> bin/x64/plugins/cyber_engine_tweaks/scripting.log (readable). Existing CET mod "CameraProbe" (unrelated FPP-offset probe) confirms Game.GetPlayer()/GetFPPCameraComponent()/onUpdate/onDraw/hotkeys work.
- Virtual-camera registry (alt path): entVirtualCameraComponent (source, named via Set/GetVirtualCameraName, isEnabled, resW/H) + entVirtualCameraViewComponent (sink: virtualCameraName + targetPlaneSize). Output internal/named => harder to grab than RTT->DynamicTexture; prefer RTT path for VR grab.
- NEXT (M-PROTO): instantiate entRenderToTextureCameraComponent on player (Codeware Entity/Assemble AddComponent), set localTransform=head+IPD, fov/near/far matched, resW/H=eye res, renderingMode=Shaded, bind a DynamicTexture target, isEnabled=true; verify via dxgi RT-diag that a 2nd full-scene RT at eye res appears; then grab that DynamicTexture's ID3D12Resource and submit as right eye.

## M-PROTO RESULT: FRONT PROVEN (2026-06-07)
CET console one-liner worked: NewObject("entRenderToTextureCameraComponent") + set name/fov=90/resolutionWidth=resolutionHeight=1024/renderingMode=Shaded/isEnabled=true + player:AddComponent(c) => "[RTT] added to PlayerPuppet", NO crash. dxgi RT-diag proved a SECOND full-scene view rendered: 6 distinct 1024x1024 RTs (G-buffer/HDR fmt 28/49/57, binds~1540 = same per-frame pass count as main 3072^2 view) appeared ONLY in the snapshot AFTER AddComponent (0 before, 6 after). Engine auto-allocated the RTs at exactly our requested resolution. No DynamicTexture target was even set yet, still rendered. ~1/9 main-view pixels => cheap => option C is optimizable as hoped.

FRONT of option C = SOLVED natively & maintainably. Repro: CET (overlay ~ -> Console) paste the one-liner, or CET mod RTTStereo at bin/x64/plugins/cyber_engine_tweaks/mods/RTTStereo (buttons/hotkeys).

REMAINING (BACK part):
1. pose right-eye = HMD head pose + IPD (localTransform; currently default) + match fov/near/far/aspect to VR proj.
2. get a STABLE GPU handle to the right-eye final color: bind dynamicTextureRes to a DynamicTexture we control (raRef by path/name) then map its ID3D12Resource via d3d12 proxy; OR grab the transient 1024^2 final-color RT by size/format each frame.
3. composite: copy right-eye color -> OpenXR right swapchain; left = native main view.
4. tune resolution (non-square per VR aspect, higher res) + per-eye feature toggles (RTFP_None on SSAO/reflections) for perf.

## CORRECTION (2026-06-07 later): FRONT NOT yet proven — earlier 1024x1024 was a FALSE POSITIVE
Rigorous unique-resolution retest FAILED: added component with resolutionWidth/Height=1234x1100, console printed success, but RT-diag showed NO 1234x1100 RT anywhere (not even quantized). The 1024x1024 full-scene cluster is a NATIVE engine size (reflections/cubemap/secondary view; co-occurs with ViewProbe viewIdx=1 MULTI). The earlier "before 0 / after 6" was a confound (player entered a scene where the native secondary 1024^2 view activated), NOT caused by AddComponent.
=> entRenderToTextureCameraComponent without a bound dynamicTextureRes target is a NO-OP (RTT camera with no texture = nothing to render into). dynamicTextureRes/depth/albedo/... are raRef/rRef = references to a COOKED DynamicTexture resource by path, NOT a runtime instance. Assigning a NewObject("DynamicTexture") to a raRef likely won't work.
REAL remaining crux = give the component a valid DynamicTexture target. Options: (R1, recommended) reuse shipped entity templates that already wire RTT+texture: base\cinematics\cameras\holocall_camera.ent (uses gameuiHolocallCameraComponent : entRenderToTextureCameraComponent -> DynamicTexture), base\media\tv\entities\virtual_camera.ent; spawn via Codeware SpawnEntity, confirm render via RT-diag, then drive pose head+IPD and grab the texture's ID3D12Resource. (R2) author a custom DynamicTexture via ArchiveXL and bind it. No standalone DynamicTexture .xbm paths found by keyword in Codeware KnownHashes (texture likely defined inside the .ent/.app). RT-diag is cumulative every 1800 frames after frame 360 -> unreliable for toggle; unique resolution is the only unambiguous marker but only takes effect once a real target sizes the render.
