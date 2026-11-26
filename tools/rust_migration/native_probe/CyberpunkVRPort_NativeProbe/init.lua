-- The port's natives called from the game (tools/rust_migration/probe_session.ps1 -NativeTest): installed into the
-- port's Lua mods for one run and taken out after it. In each pass the probe script asks for, every case below is
-- called through CET and its result printed to scripting.log as one "[native-probe]" line; the probe script collects
-- those lines. Only natives that read state are called: a case must not change the game.
local cases = {
    { 'VRCyberwareFrame' }, { 'VRMeleeInputFrame' }, { 'VRMeleeThrowFrame' }, { 'VRMeleeThrowEnabled' },
    { 'VRWeaponTriggerEnabled' }, { 'VRWeaponInLeftHand' }, { 'VRLeftHanded' }, { 'VRSceneOwnsBody' },
    { 'VRPhysicalMelee' }, { 'VRMeleeTwoHandedActive' }, { 'VRCyberwarePoint', 0, 0 },
    { 'VRCyberwareCollisionInfo' }, { 'VRLauncherControllerAim' }, { 'VRHandHolds', 0 }, { 'VRHandHolds', 1 },
    { 'GetVRLadderDebug', 0 },
    -- src/Natives/HandBody.cpp
    { 'VRViewWorldPos' }, { 'VRViewWorldRot' }, { 'VRPalmWorldPos', 1 }, { 'VRPalmModelPos', 1 },
    { 'VRPalmModelRot', 0 }, { 'VRPalmCamLocal', 1 }, { 'VRCamModelPos' }, { 'VRHandRawModel', 1 },
    { 'VRHandRawRot', 0 }, { 'VRHandRawWorld', 0 }, { 'VRHandMotion', 1, false }, { 'VRBodyBonePos', 4 },
    { 'VRBoneSnapCount' }, { 'VRBoneModelPos', 0 }, { 'VRPlayerEnginePos' }, { 'VRPlayerEngineRot' },
    { 'VRSmokeMouthDist' }, { 'VRSmokeMouthDistL' }, { 'VRSmokeMouthWorldPos' }, { 'GetVRAnimPoseStats', 0 }, { 'GetVRAnimPoseStats', 1 },
    { 'VRWorldToModel', function() return Vector4.new(0, 0, 0, 1) end },
    { 'VRWorldDirToModel', function() return Vector4.new(0, 1, 0, 0) end },
    { 'GetVRSmokeCigVisualScaleY', function() return nil end }, { 'VRBodyProbe', function() return nil end },
    { 'VRDumpBodyFuncs' }, { 'DumpPlayerBoneNames' },
    -- src/Natives/PoseRig.cpp
    { 'VRPairs', -3, 0 }, { 'VRPairs', -5, 0 }, { 'VRRigStatus', 0 }, { 'VRRigStatus', 3 }, { 'VRSmallRig', -3, 0 },
    { 'VRPoseCensus', -3, 0 }, { 'VRRigWriteDiag', -1, 0 }, { 'VRRigTrack', 0, -1 }, { 'VRRigBone', 0, 0, 0 },
    { 'VRScannerSlotGet', 0, 0 }, { 'VRScannerSlotGet', 3, 2 }, { 'VRScannerSlotGet', 9, 2 }, { 'VRTwoHandStatus' },
    { 'VRRestFingerStatus' }, { 'VRRestFingerStatusRight' }, { 'GetVRCarryReach' }, { 'GetVRCarryNear' },
    { 'VRWeaponRigStatus', -1 }, { 'VRWeaponRigStatus', -2 }, { 'VRWeaponRigStatus', 0 }, { 'VRWeaponRigNames' },
    { 'VRWeaponPartLocal', 0 }, { 'GetVRWeaponAim' },
    -- src/Natives/VrikArming.cpp
    { 'VRMirrorDoubleStats' },
    -- src/Natives/Calibration.cpp (dumps into the log folder)
    { 'DumpRuntimeClassFunctions' }, { 'DumpAnimControllerComponents' }, { 'DumpAnimVTable' },
    { 'LogVRDiag', 0, 0, 0, 0, 0, 0, 1 },
    -- src/Natives/UiPopup.cpp (VRUiPopup(0) only restates a closed popup)
    { 'VRUiPopup', 0 }, { 'VRPopupVisualState', function() return CName.new('None') end, false },
    -- src/Natives/OrientationProvider.cpp
    { 'GetVRProvDump', 100 }, { 'GetVRProvDump', 103 }, { 'GetVRProvDump', 104 }, { 'GetVRProvDump', 102 },
    { 'GetVRMainEyeOffset' }, { 'GetVRSharedSlot', 144 }, { 'GetVRSharedSlot', 999 }, { 'GetVRTwoHandActive' },
    { 'GetVRTwoHandLeftCtrl' }, { 'GetVRMeleeTrigger' }, { 'GetVRPhysicalCrouch' }, { 'GetVRCarryOffset', 0 },
    { 'VRPhysicalOpticsEnabled' }, { 'VRPhysicalOpticsCameraReady' },
    -- src/Natives/KeypadInput.cpp, WorldTouch.cpp, VendorRoute.cpp (nothing held, nothing to touch)
    { 'VRKeypadUpdate', function() return nil end },
    { 'VRWorldTouchPointer', function() return nil end, function() return Vector4.new(0, 0, 0, 1) end,
      function() return Vector4.new(0, 0, 1, 0) end },
    { 'VRMenuVendorPoll', false, function() return nil end },
    { 'VRMenuVendorResolve', function() return nil end, false },
    -- src/Natives/EntityAttach.cpp: refusals only -- the drawn weapon's root is already carried by the slot
    { 'VRAttachToSlot', function() return nil end, function() return nil end, function() return CName.new('WeaponRight') end },
    { 'VRDetachFromSlot', function() return nil end },
    { 'VRAttachToSlot', function()
        return Game.GetTransactionSystem():GetItemInSlot(Game.GetPlayer(), TweakDBID.new('AttachmentSlots.WeaponRight'))
    end, function()
        return Game.GetPlayer():FindComponentByName(CName.new('ItemAttachmentSlots'))
    end, function() return CName.new('WeaponRight') end },
    -- src/Natives/InteractionAction.cpp (no hub shown: nothing takes it), Performance.cpp, NeckCap.cpp (the VRIK
    -- module makes the same call a few times a second)
    { 'VRInteractionAction', function() return CName.new('None') end },
    { 'VRInteractionAction', function() return CName.new('ChoiceScrollUp') end },
    { 'VRShouldScanBodyCapsules', function() return Game.GetPlayer() end },
    { 'VRShouldScanBodyCapsules', function() return nil end },
    { 'VRReflexTiming', false }, { 'GetVRMouseYDisabled' }, { 'VRVrcamEnableRequest' }, { 'VRNeckCapSync' },
    -- src/Natives/RemoteCamera.cpp: the states the game is in anyway (no takeover, no braindance, default switches)
    { 'VRInBraindance', 0 }, { 'VRRemoteCamera', 0, 0, 0, 0 }, { 'VRPlayerCamera', 0, 0, 0, 0 },
    { 'VRSceneCamera', 0, 0, 0, 0, 0, 0, 0, 1 }, { 'VRBraindance', 0, 0 }, { 'VRBdSceneFov', 0 },
    { 'VRTakeoverEntity', '' }, { 'VRBdFovWrite', 1 }, { 'VRBdHeadWrite', 1 }, { 'VRHudSecondEye', 0 },
    -- src/Natives/WristGuard.cpp: report and detect only
    { 'VRWristGuard', 2 }, { 'VRWristGuard', 3 }, { 'VRWristGuard', 4 },
    -- src/Lobby/SaveLooks.cpp (every save and look the save folder holds), CreationTarget.cpp (nothing swapped: a null
    -- entity changes nothing)
    { 'VRLobbySaveLooks' }, { 'VRCreationTarget', function() return nil end },
    -- src/Hooks/LeftHandedAnimation.cpp's native: off, with no controller (nothing held, nothing to empty)
    { 'VRLeftHandedAnimations', function() return nil end, false, function() return CName.new('None') end,
      function() return CName.new('None') end },
    -- src/Audio/PortSound.cpp through src/Natives/VrMenu.cpp: at volume 0 (silent) -- a one-shot, a missing file, a
    -- looped channel with its seam crossfaded, and that channel ended
    { 'VRMenuPlayWav', 'CyberpunkVRPort_Lobby/sounds/menu_appear.ogg', 0.0 }, { 'VRMenuPlayWav', 'probe_missing.ogg', 0.0 },
    { 'VRSoundChannel', 'native_probe', 'CyberpunkVRPort_Lobby/sounds/ambient.ogg', 0.0, 0.0, 1.0, 3.0, 0.25, 0.0 },
    { 'VRSoundChannel', 'native_probe', '', 0.0, 0.0, -1.0, 0.0, 0.0, 0.0 },
    -- src/Natives/VrMenu.cpp: no swipe, the pointers against a plane 1 m ahead, the menu's switches left as they are
    { 'VRMenuGesture' }, { 'VRGripFromFingers' }, { 'VRMenuCapture', false }, { 'VRWeaponWheel', false },
    { 'VRMenuBlink', 0.0 },
    { 'VRMenuPointer', function() return Vector4.new(0, 1.5, -1, 1) end, function() return Vector4.new(1, 0, 0, 0) end,
      function() return Vector4.new(0, 1, 0, 0) end, function() return Vector4.new(1, 0.6, 0, 0) end },
    -- src/Overlay/VrOverlay.cpp through VRMenuOpenSettings: the panel asked for (pass 1), then shown by the Lua bridge
    -- and so left as it is (pass 2)
    { 'VRMenuOpenSettings' },
    -- src/Natives/InputPrompts.cpp (right-handed, on foot: the parts as they are)
    { 'VRPromptPart', function() return CName.new('x') end }, { 'VRPromptPart', function() return CName.new('left_trigger_hold') end },
    { 'VRPromptPart', function() return CName.new('None') end },
    -- src/Natives/ActionReads.cpp: the state machines' reads swapped in (the ladder's and the swimming's reports)
    { 'GetVRLadderDebug', 21 }, { 'GetVRSwimmingDebug', 16 },
}

local function show(value)
    local kind = type(value)
    if kind == 'table' then
        local parts = {}
        for i, item in ipairs(value) do parts[i] = show(item) end
        return '[' .. #value .. '] ' .. table.concat(parts, ' ')
    end
    if kind == 'userdata' then
        local ok, text = pcall(function()
            return string.format('(%.4f,%.4f,%.4f,%.4f)', value.x, value.y, value.z, value.w)
        end)
        if ok then return text end
        return tostring(value)
    end
    if kind == 'number' then return string.format('%.6g', value) end
    return tostring(value)
end

local function run(pass)
    for _, case in ipairs(cases) do
        local name = case[1]
        local fn = Game[name]
        local text
        if not fn then
            text = 'missing'
        else
            local args = {}
            for i = 2, #case do
                local a = case[i]
                if type(a) == 'function' then a = a() end
                args[i - 1] = a
            end
            local ok, result = pcall(fn, table.unpack(args, 1, #case - 1))
            text = ok and show(result) or ('error ' .. tostring(result))
        end
        local shown = {}
        for i = 2, #case do shown[i - 1] = type(case[i]) == 'function' and 'f' or tostring(case[i]) end
        text = text:gsub('\n', ' | ')
        print(string.format('[native-probe] %d %s(%s) -> %s', pass, name, table.concat(shown, ','), text))
    end
end

-- A pass runs when the probe script asks for it: it writes the pass number into go.txt beside this file once its own
-- tests have the game running (a pass made while the game still loads would read the state before the first solve).
local pass, poll = 0, 0.0
registerForEvent('onUpdate', function(delta)
    poll = poll + delta
    if poll < 0.25 then return end
    poll = 0.0
    local f = io.open('go.txt', 'r')
    if not f then return end
    local wanted = tonumber(f:read('*l') or '') or 0
    f:close()
    if wanted <= pass or not Game.GetPlayer() then return end
    pass = wanted
    run(pass)
    print(string.format('[native-probe] pass %d done', pass))
end)
