-- CyberpunkVRPort_Weapon -- "bullet from the weapon barrel" VR aim.
--
-- The F10 weapon-aim toggle selects controller 6DoF (Hand Aim) when enabled and HMD 3DoF
-- (Decoupled VR Head Aim) when disabled. BOTH launch from the live weapon muzzle, so nothing here
-- depends on which one is selected.
--
-- The aim ENABLE toggle lives in the VR imgui overlay (dxgi "Controls" -> "Bullet from weapon
-- barrel", writes shared[58]). This script:
--   1) installs the GetOrientation VMT instrument once (InstallVRProvInstrument) -- this ALSO
--      installs the override hooks that redirect the shot down the barrel (slot 33 / mode 6),
--   2) publishes the weapon muzzle world orientation each frame (SetVRMuzzleQuat) -- drives both
--      the launch override and the overlay barrel laser dot, and
--   3) publishes the live camera zoom (SetVRZoomLevel) for DIAGNOSTICS ONLY -- never use it to
--      scale the dot or the projection: MAIN's projection already carries the ADS magnification,
--      and applying both double-zooms ordinary weapons, and
--   4) VR MOTION MELEE: detects a real controller swing (weapon moved fast relative to the player)
--      and fires the game's NATIVE melee attack along the blade via redscript PlayerPuppet:VRMeleeSwing
--      (mod CyberpunkVRPort_Melee). The native box-sweep does collision/damage/reaction/stamina, so
--      it behaves like the flat game. A fast swing = power/cleave. No-op for guns (self-filtering).

-- ONE DEBUG SWITCH FOR THE WHOLE PORT, read from shared slot [156].
--
-- The plugin republishes the launcher's DEBUG checkbox there every frame, so this bridge obeys the
-- same switch as everything else and can be flipped without editing a file. It used to be a
-- hardcoded `local DEBUG = true`, which is how one session left 26 449 lines and 5 MB of per-frame
-- state in this mod's log alone.
--
-- Cached for a quarter second: this is called from onUpdate and a shared-memory read per frame to
-- decide whether to not log is a poor trade.
local dbgCache, dbgAt = false, -1.0
local function vrDebug()
    local now = (os and os.clock and os.clock()) or 0.0
    if now - dbgAt > 0.25 then
        dbgAt = now
        dbgCache = (type(GetVRSharedSlot) == 'function') and (GetVRSharedSlot(156) > 0.5) or false
    end
    return dbgCache
end

-- Routine chatter. Anything that must survive DEBUG=0 -- a failure, a one-time fact -- calls
-- logAlways instead.
local function logAlways(fmt, ...)
    local ok, s = pcall(string.format, fmt, ...)
    if ok then spdlog.info("[CyberpunkVRPort_Weapon] " .. s) end
end
local function logf(fmt, ...)
    if not vrDebug() then return end
    logAlways(fmt, ...)
end

local installed = false
local installTimer = 0.0
local cyberware = require('cyberware')
local physicalThrow = require('physical_throw')
local cyberwareLastError

-- Ordinary melee uses the same tracking/swing/native-damage module as arm cyberware.
local meleeEnabled = true

-- WEAPON DRAW SOUND (any weapon, not only melee): the draw anim never plays in VR, so its equip
-- audio never fires; redscript VREquipSound replays the weapon's own equip event on the entity.
-- Trigger: active-weapon ENTITY changed (holster respawns the entity, so re-draws count too).
-- First observation after mod load is swallowed (savegame restore is not a draw).
local equipSndInit = false
local equipSndId = nil

-- VR GUARD — native block/deflect via the game's own mitigation STATS, ZERO debuffs.
-- damageManager.script gates the player's incoming-hit mitigation purely on stats:
--   IsBlocking==1   -> melee hits WasBlocked (stamina damage instead of health); with the Blades
--                      perk (Reflexes_Right_Perk_2_1) + stamina, bullets WasBulletDeflected;
--   IsDeflecting==1 -> melee hits WasDeflected('Parry' — the attacker staggers); with the perk,
--                      bullets WasBulletParried (reflected AT the shooter).
-- The flat game sets IsBlocking from the PSM Block state — which also drags in the debuffs
-- (AimWalk slow-walk, sprint interrupt, block anims). We set the stats DIRECTLY and never touch
-- the PSM (no LT, no 'MeleeBlock' action):
--   blade pointing FORWARD (thrust cone = attack intent)   -> guard OFF
--   blade in ANY other orientation (across/up/down/reverse) -> guard ON, same frame
--   guard OFF->ON transition -> IsDeflecting for a short PARRY window (raise-to-parry gesture,
--                               native stagger / perk bullet-reflect), then settles to IsBlocking
-- No PSM state => full walk/sprint speed, no anim events, frame-instant transitions both ways,
-- and it composes with swings freely (your own slash keeps the guard up unless the blade points
-- forward mid-arc — native-VR "always guarded" feel). Stamina still drains per blocked hit
-- natively (DealStaminaDamage), so it is not god mode.
local GUARD_THRUST_DOT   = 0.50  -- blade within ~60° of body-forward = attack intent, guard off
local GUARD_PARRY_WINDOW = 0.25  -- s of IsDeflecting right after guard entry (gesture parry)
local guardClock = 0.0           -- accumulated onUpdate time (drives the parry window)
local guardParryUntil = -1.0
local guardWasOn = false
local guardBlockMod = nil        -- IsBlocking stat modifier handle (applied = guarding)
local guardParryMod = nil        -- IsDeflecting stat modifier handle (applied = parry window)

-- Apply/remove the two stat modifiers to match the wanted phase. Idempotent per frame.

-- CAMERA RECOIL, KILLED AT THE WEAPON. The game kicks the player's heading on every shot -- measured
-- in the plugin at ~1 deg peak within 150 ms of a round, read straight out of the heading delta. In a
-- flat shooter that kick IS recoil; in VR you aim with the controller, so all of it lands on the head
-- and reads as a sideways jerk of the view. The hand recoil the plugin now applies replaces it.
--
-- A MULTIPLIER ON THE WEAPON, not 316 TweakXL entries. The values live in per-weapon inline stat
-- modifiers (Items.Base_<Weapon>_Constant_Stats_inline6/7 and friends -- 316 of them across the game,
-- and that list was written out before this was tried), so overriding them by data means enumerating
-- every weapon and still missing every modded one. One multiplier of zero on the equipped weapon
-- covers all of them, including weapons this mod has never heard of.
--
-- Re-applied when the weapon entity changes, because the modifier lives on the ENTITY: a fresh draw
-- is a fresh entity and starts with the game's own stats again.
-- Peak camera kick each weapon family owns, in the game's own degrees, lifted straight out of
-- TweakDB (max RecoilKickMax across that family's variants). This IS the per-weapon table -- the
-- live stats system was tried first and returns a flat zero for these, because the recoil system
-- reads them from the weapon RECORD and never registers them as tracked stats.
local RECOIL_KICK_BY_FAMILY = {
    ['achilles'] = 2.0,
    ['ajax'] = 1.3,
    ['ashura'] = 1.0,
    ['buck'] = 8.0,
    ['burya'] = 9.0,
    ['carnage'] = 12.0,
    ['chao'] = 0.4,
    ['copperhead'] = 0.9,
    ['cpo'] = 4.0,
    ['crusher'] = 5.6,
    ['defender'] = 0.466,
    ['dian'] = 0.24,
    ['grad'] = 8.0,
    ['guillotine'] = 0.65,
    ['hmg'] = 0.8,
    ['igla'] = 8.0,
    ['kappa'] = 0.24,
    ['kenshin'] = 0.8,
    ['kolac'] = 3.8,
    ['kyubi'] = 1.4,
    ['lexington'] = 1.0,
    ['liberty'] = 2.0,
    ['ma70'] = 0.6,
    ['masamune'] = 1.2,
    ['nekomata'] = 7.2,
    ['nova'] = 3.5,
    ['nue'] = 3.2,
    ['omaha'] = 2.8,
    ['overture'] = 4.0,
    ['palica'] = 5.6,
    ['pozhar'] = 5.4,
    ['pulsar'] = 0.85,
    ['quasar'] = 1.2,
    ['rocketlauncher'] = 15.0,
    ['saratoga'] = 0.6,
    ['satara'] = 9.6,
    ['senkoh'] = 0.8,
    ['shingen'] = 0.28,
    ['sidewinder'] = 0.56,
    ['silverhand'] = 1.9,
    ['slaughtomatic'] = 1.3,
    ['sor22'] = 3.5,
    ['tactician'] = 11.0,
    ['testera'] = 10.4,
    ['umbra'] = 1.05,
    ['unity'] = 2.25,
    ['yukimura'] = 0.7,
    ['zhuo'] = 2.8,
}

-- FELT RECOIL, WHERE THE GAME'S NUMBER IS NOT THE ANSWER. The table above is TweakDB's own kick and it
-- is what every weapon is scaled by; these are the ones where the result was judged in the headset and
-- came back wrong. Kept separate from the generated table on purpose -- that one is data and must stay
-- regenerable, this one is a handful of measured corrections with a reason each.
--
--   unity  2.25 -> 0.87   Its own number puts it at 38.4 deg, which is inside the ceiling's compression
--                         where every heavy pistol lands within four degrees of every other. Halved after
--                         firing it: 19.1 deg, and it sits below the knee again,
--                         where the ladder still has room to tell weapons apart.
local RECOIL_KICK_OVERRIDE = {
    ['unity'] = 0.87,
}

local recoilKilled = nil       -- entity id hash the modifiers are currently attached to
-- THE KICK IS CACHED BY WEAPON TYPE, and that is not an optimisation.
--
-- The multiplier that kills the camera kick lives on the weapon ENTITY, so the second time the same gun
-- is drawn its RecoilKickMax already reads 0 -- our own doing. Reading it then and publishing it would
-- hand the plugin a zero; skipping the publish (which is what happened) leaves the plugin on the
-- PREVIOUS weapon's number. Measured exactly that way: Overture in hand, 0.989 published, which is the
-- Lexington. Cached by record id, the first honest read is the one that counts, for every later draw.
local recoilKickCache = {}
local RECOIL_STATS = { 'RecoilKickMin', 'RecoilKickMax', 'RecoilKickMinADS', 'RecoilKickMaxADS',
                       'RecoilAngle', 'RecoilAngleADS' }
-- THE GAME'S OWN CAMERA KICK, REMOVED (0): the shot no longer reaches the head at all. It was kept at 20%
-- for a while (2026-10-16). The hand spring is separate and keeps its own scale (CyberpunkVR_RecoilScale).
local CAMERA_KICK_SCALE = 0.0
local cameraKickMods = {}      -- stats-object key -> the modifier set attached to it
-- THE CAMERA KICK OFF ONE WEAPON: the multiplier set on its item's own stats object (or `fallback`, the gun's
-- entity, when the item has none). By item, so a gun can be prepared before it is drawn. Returns pcall's pair.
local function zeroCameraKickItem(itemID, fallback, wid)
    local sid = nil
    pcall(function()
        local data = Game.GetTransactionSystem():GetItemData(Game.GetPlayer(), itemID)
        sid = data and data:GetStatsObjectID() or nil
    end)
    if not (sid or fallback) then return false, 'no stats object' end
    return pcall(function()
        local ss = Game.GetStatsSystem()
        local target = sid or fallback
        -- ONE MODIFIER SET PER STATS OBJECT. The item's stats object outlives the weapon entity, so every
        -- draw of the same gun lands here again; with a factor of zero that was harmless (0 * 0 = 0), with
        -- 0.2 a second set would make it 0.04. So the previous set is taken off first -- after a reload the
        -- old handles are on nothing and the removal is a no-op.
        local key = nil
        pcall(function() key = sid and tostring(sid.entityHash) or nil end)
        key = key or tostring(wid)
        for _, m in ipairs(cameraKickMods[key] or {}) do pcall(function() ss:RemoveModifier(target, m) end) end
        local mods = {}
        for _, name in ipairs(RECOIL_STATS) do
            local st = gamedataStatType[name]
            if st then
                -- Multiplier, not Additive: the kick is a positive number the weapon owns, and a factor
                -- scales it whatever that number is.
                local m = RPGManager.CreateStatModifier(st, gameStatModifierType.Multiplier, CAMERA_KICK_SCALE)
                ss:AddModifier(target, m)
                mods[#mods + 1] = m
            end
        end
        cameraKickMods[key] = mods
    end)
end
local function zeroCameraKick(wpn, wid) return zeroCameraKickItem(wpn:GetItemID(), wpn:GetEntityID(), wid) end

-- THE WEAPON'S RECOIL PROFILE, off its record: the record key, the family name, the game's peak kick and the
-- class. The weapon in hand publishes these as its own; with a gun in each hand the left one does the same for
-- the left hand (leftGunFrame).
local function recoilProfile(wpn)
    -- WHICH WEAPON, FROM ITS RECORD NAME. `Items.Preset_Lexington_Neon` -> "lexington". The table above
    -- holds the game's own peak kick per family, which is the number the hand spring scales its angle
    -- and its settle time by.
    --
    -- READING IT LIVE DOES NOT WORK, and that is measured, not assumed: GetStatValue returns 0 for
    -- RecoilKickMax whether it is asked with the weapon's entity id or with the StatsObjectID its item
    -- data carries (log: `kick=0 sid=true`). The recoil system takes these out of the weapon record and
    -- never registers them as tracked stats, so the value has to come from the record side -- and a
    -- table generated from TweakDB is exactly that, just resolved ahead of time.
    local key = nil
    pcall(function() key = TDBID.ToStringDEBUG(ItemID.GetTDBID(wpn:GetItemID())) end)
    -- AND FROM friendlyName, because the record name is not always the weapon's name. Quest and iconic
    -- guns are named after the quest that hands them out: River's revolver is `Items.sq029_rivers_gun`,
    -- which says nothing about what it is -- measured as `kick=nil` in the probe. Its friendlyName is
    -- `w_revolver_malorian_overture`, which says everything, and it is the same identifier the animation
    -- paths use. Both strings are searched, so a weapon has to hide its family from both to be missed.
    local fname = nil
    if key then pcall(function() fname = TweakDB:GetFlat(key .. '.friendlyName') end) end
    local kick = nil
    local famName = nil
    if key then
        local low = string.lower(key .. ' ' .. tostring(fname or ''))
        -- LONGEST MATCH WINS, because `pairs` has no order and a record name can contain more than one
        -- family: whichever key happened to come first would decide, and it would decide differently
        -- from run to run. The longest match is the specific one.
        local best = 0
        for fam, v in pairs(RECOIL_KICK_BY_FAMILY) do
            if #fam > best and string.find(low, fam, 1, true) then kick = v; famName = fam; best = #fam end
        end
    end
    -- ...AND THE NAME ITSELF, for the two-hand grip. That hold is a property of the weapon -- a pistol's
    -- support hand is on the same grip, a rifle's is out on the handguard -- so the plugin keeps one
    -- captured file per weapon and needs a name to key it by. It cannot work one out on its own: the rig
    -- signature identifies only the thirteen weapons the reload knows, while the family is already
    -- resolved right here. Per DRAW, not per frame, so the pose path still takes nothing from CET.
    -- WEAPONS THE RECOIL TABLE DOES NOT NAME still need a name for their two-hand hold. Ticon and
    -- Tamayura have no RecoilKickMax rows in TweakDB, so the family match above finds nothing and the
    -- fallback was the RECORD id -- which produced CyberpunkVR_TwoHandGrip_itemscraftable_legendary_ticon
    -- .ini, a file keyed to one VARIANT: capture on the legendary and the common one has no hold at all.
    -- These two tokens are the same ones the reload module matches those weapons by. The EP1 Osprey, Rasetsu,
    -- Hercules and Warden are missing from the generated table as well, so they get a token here too (the
    -- Warden's hold came out keyed `itemspreset_warden_default` before it had one).
    -- MELEE HAS NO RecoilKickMax AT ALL, so the generated table above names none of it and every blade
    -- fell through to the record id -- one hold file per VARIANT, which is no hold at all. These are
    -- handle shapes rather than weapon names: a Butcher's Knife and a Chef's Knife are the same grip,
    -- a katana and a sledgehammer are not. Matched by the same longest-wins rule as the two above, so
    -- 'baton' beats 'bat' and 'pipe_wrench' beats 'pipe' without depending on this list's order.
    if not famName and key then
        local low2 = string.lower(key .. ' ' .. tostring(fname or ''))
        local best2 = 0
        for _, w in ipairs({
            'ticon', 'tamayura', 'osprey', 'rasetsu', 'hercules', 'warden',
            -- blades
            'katana', 'machete', 'kukri', 'knife', 'tanto', 'tomahawk', 'chainsword', 'axe',
            -- blunt
            'baton', 'sledgehammer', 'hammer', 'kanabo', 'crowbar', 'pipe_wrench', 'iron_pipe',
            'tire_iron', 'bat', 'wrench', 'pipe',
            -- cyberware arms, which are also a hold
            'mantis', 'monowire', 'gorilla', 'projectilelauncher',
        }) do
            if #w > best2 and string.find(low2, w, 1, true) then famName = w; best2 = #w end
        end
    end
    -- NEVER LEAVE THE PREVIOUS WEAPON'S NUMBER IN PLACE. Publishing nothing when a weapon is unknown
    -- means the hand keeps kicking like whatever was drawn before it -- silently, and wrongly. The
    -- reference kick is the honest answer to "unknown": it is the Lexington, i.e. the angle that was
    -- tuned in the headset.
    if famName and RECOIL_KICK_OVERRIDE[famName] then kick = RECOIL_KICK_OVERRIDE[famName] end
    if not (kick and kick > 0.0) then kick = 1.0 end
    -- AND WHAT KIND OF WEAPON IT IS, because the recoil splits the same impulse differently for a
    -- shouldered rifle and a pistol held out on an arm. Taken from the record's own itemType rather
    -- than from a list of names: TweakDB already knows, and a name list would be wrong the first time
    -- a quest gun is named after its quest -- the exact trap the kick lookup above documents.
    local cls = 0
    local itName = nil
    if key then
        local it = nil
        pcall(function() it = TweakDB:GetFlat(key .. '.itemType') end)
        if it then pcall(function() itName = TDBID.ToStringDEBUG(it) end) end
        local low = string.lower(tostring(itName or ''))
        -- shotgun first: it is the one class that wants both halves, and 'shotgundual' contains none
        -- of the other tokens
        if string.find(low, 'shotgun', 1, true) then cls = 3
        elseif string.find(low, 'handgun', 1, true) or string.find(low, 'revolver', 1, true) then cls = 1
        -- SNIPERS BEFORE RIFLES, or 'sniperrifle' would be caught by the 'rifle' token below and lose
        -- its own class. The precision family goes with them: it is the semi-automatic half of the same
        -- thing and fires the same class of round.
        elseif string.find(low, 'sniperrifle', 1, true) or string.find(low, 'precisionrifle', 1, true) then cls = 4
        elseif string.find(low, 'rifle', 1, true) or string.find(low, 'machinegun', 1, true)
            or string.find(low, 'submachine', 1, true) then cls = 2
        -- MELEE IS ITS OWN CLASS, not "unknown". It used to fall through as 0 -- "the record did not
        -- say" -- which is a different statement and would keep a blade on the base hand-filter speed
        -- for ever. None of these tokens collide with the firearm ones above.
        elseif string.find(low, 'katana', 1, true)  or string.find(low, 'blade', 1, true)
            or string.find(low, 'knife', 1, true)   or string.find(low, 'machete', 1, true)
            or string.find(low, 'axe', 1, true)     or string.find(low, 'hammer', 1, true)
            or string.find(low, 'club', 1, true)    or string.find(low, 'chainsword', 1, true)
            or string.find(low, 'sword', 1, true)   or string.find(low, 'fists', 1, true)
            or string.find(low, 'melee', 1, true)   then cls = 5
        end
    end
    -- Cyberware names such as Cyb_StrongArms and Cyb_NanoWires do not contain
    -- the blade/fists tokens above. Use the game's melee predicate as the
    -- authority, including modded melee records and cyberware arms.
    pcall(function() if WeaponObject.IsMelee(wpn:GetItemID()) then cls = 5 end end)
    return key, famName, kick, cls, itName
end

local function killCameraRecoil(wpn, wid)
    if not wpn or recoilKilled == wid then return end
    local key, famName, kick, cls, itName = recoilProfile(wpn)
    if type(SetVRWeaponName) == 'function' then
        SetVRWeaponName(famName or (key and string.lower(key)) or '')
    end
    if type(SetVRWeaponKick) == 'function' then SetVRWeaponKick(kick) end
    if type(SetVRWeaponClass) == 'function' then SetVRWeaponClass(cls) end
    -- ...AND INTO THE PROBE FILE, not only into the log. The module's spdlog stops accepting lines after
    -- a mod reload (measured, and the reason this probe file exists at all), so anything that has to be
    -- readable after the fact goes where the kick already goes.
    VRP_lastClass, VRP_lastItemType = cls, itName
    pcall(function()
        local f = io.open('recoil_probe.txt', 'a')
        if f then
            f:write(string.format('class=%d type=%s key=%s', cls, tostring(itName), tostring(key)))
            f:write(string.char(10))
            f:close()
        end
    end)
    logAlways('recoil: itemType=%s class=%d', tostring(itName), cls)
    logAlways('recoil: key=%s kick=%s', tostring(key), tostring(kick))
    -- STRAIGHT TO A FILE, because the module's spdlog log stopped accepting lines after a mod reload
    -- (the file was reopened and nothing more was appended, while the value provably reached the
    -- plugin). A diagnostic that can go quiet is worse than none: this one is opened, written, flushed
    -- and closed on the spot, so what it says is what happened.
    pcall(function()
        local f = io.open('recoil_probe.txt', 'a')
        if f then
            f:write(string.format('key=%s kick=%s wid=%s cls=%s type=%s', tostring(key), tostring(kick),
                                  tostring(wid), tostring(VRP_lastClass), tostring(VRP_lastItemType)))
            f:write(string.char(10))
            f:close()
        end
    end)

    local ok2, err = zeroCameraKick(wpn, wid)
    if ok2 then recoilKilled = wid else logAlways('recoil: kill failed: %s', tostring(err)) end
end

-- THE LEFT GUN, while the game reports a gun in each hand. GetActiveWeapon answers with the right one only, so
-- the left gun kept its own camera kick and every round it fired still threw the head; it is taken off that gun
-- the same way. Its kick and class go to the plugin as well, so the left hand gets that gun's recoil and haptic.
-- ...AND BEFORE IT IS DRAWN. Measured: the left gun reads its recoil while it comes out, before the first frame that
-- shows it in the hand, so multipliers put on there land one draw late -- the first draw of the pair still threw
-- the head and every draw after it did not. The item's stats object outlives the gun entity, so the gun EQUIPPED
-- as the left partner (Weapon slot 3) is prepared while it is holstered, renewed twice a second because a loaded
-- save starts new stats objects (the set is taken off before it is put back, so nothing compounds).
-- LEFT-HANDED, EVERY WEAPON SLOT'S GUN IS THE LEFT HAND'S: a gun drawn on its own goes there (CyberpunkVRPort_LeftHanded)
-- and reads its recoil while it comes out the same way, so all of them are prepared, not only the partner -- measured:
-- the first draw of an Overture into the left hand threw the head 18 px (and left it 9 px off), the second 0.
local LEFT_PARTNER_SLOT = 3
local leftKilled = nil
local leftPrepClock = 0.5
local WEAPON_LEFT = nil
local function leftGunFrame(pl, dt)
    if not WEAPON_LEFT then WEAPON_LEFT = TweakDBID.new('AttachmentSlots.WeaponLeft') end
    local gun = Game.GetTransactionSystem():GetItemInSlot(pl, WEAPON_LEFT)
    if not gun then
        if leftKilled then
            leftKilled = nil
            if type(SetVRLeftWeaponKick) == 'function' then SetVRLeftWeaponKick(0.0, 0) end
        end
        leftPrepClock = leftPrepClock + (dt or 0.0)
        if leftPrepClock >= 0.5 then
            leftPrepClock = 0.0
            local data = EquipmentSystem.GetData(pl)
            local first = (type(VRLeftHanded) == 'function' and VRLeftHanded() == true) and 0 or LEFT_PARTNER_SLOT
            local last = math.min(LEFT_PARTNER_SLOT, data:GetNumberOfSlots(gamedataEquipmentArea.Weapon, true) - 1)
            for slot = first, last do
                local id = data:GetItemInEquipSlot(gamedataEquipmentArea.Weapon, slot)
                if ItemID.IsValid(id) then zeroCameraKickItem(id, nil, nil) end
            end
        end
        return
    end
    leftPrepClock = 0.5
    local wid = tostring(gun:GetEntityID().hash)
    if wid == leftKilled then return end
    local key, _, kick, cls = recoilProfile(gun)
    if type(SetVRLeftWeaponKick) == 'function' then SetVRLeftWeaponKick(kick, cls) end
    local ok, err = zeroCameraKick(gun, wid)
    if ok then leftKilled = wid else logAlways('recoil: left kill failed: %s', tostring(err)) end
    logAlways('recoil: left key=%s kick=%s class=%d', tostring(key), tostring(kick), cls)
end

local function guardStats(pl, wantParry, wantBlock)
    local ss = Game.GetStatsSystem()
    local id = pl:GetEntityID()
    if wantParry and not guardParryMod then
        guardParryMod = RPGManager.CreateStatModifier(gamedataStatType.IsDeflecting, gameStatModifierType.Additive, 1.0)
        ss:AddModifier(id, guardParryMod)
    elseif not wantParry and guardParryMod then
        ss:RemoveModifier(id, guardParryMod)
        guardParryMod = nil
    end
    if wantBlock and not guardBlockMod then
        guardBlockMod = RPGManager.CreateStatModifier(gamedataStatType.IsBlocking, gameStatModifierType.Additive, 1.0)
        ss:AddModifier(id, guardBlockMod)
    elseif not wantBlock and guardBlockMod then
        ss:RemoveModifier(id, guardBlockMod)
        guardBlockMod = nil
    end
end

-- Publish the muzzle world orientation. The plugin (SetVRMuzzleQuat) uses it for the launch
-- override (bullet leaves the barrel) AND writes the muzzle forward to shared mem for the
-- overlay's barrel laser dot.
local muzzlePosWarned = false
local aimHitProbed = false

-- THE WEAPON IN THE LEFT HAND. See the block above carryTick().
local CARRY_LEFT      = true
local carryOn         = false     -- the weapon is in AttachmentSlots.WeaponLeft right now
local carryItemID     = nil       -- what to put back, and where it came from
local carryRGripWas   = false
local carryGrip       = nil       -- the weapon's pose in the LEFT bone's frame, captured once
local carryHoming     = false     -- easing the offset back to nothing
local carryW          = 0.0       -- that ease's weight, 1 = in the left hand, 0 = home
local carryV          = 0.0
local CARRY_HOME_MS   = 260.0     -- how long the weapon takes to travel back into the right hand
local CARRY_HOME_ZETA = 0.70
-- HOW NEAR THE RIGHT HAND HAS TO BE TO TAKE THE WEAPON BACK, metres. A grip pressed across the room
-- used to snatch it out of the left hand; taking a gun back is a reach. Same order as the two-hand
-- grip's own radius, which was settled by feel at 6 cm for a fingertip offer -- this is a whole hand
-- closing on a rifle, so it is wider.
-- 18 cm, down from 30 (the return-to-hand radius was too big): 30 cm is
-- most of a forearm, so the grip took the weapon back from a hand that was merely nearby.
local CARRY_BACK_DIST = 0.18
local SLOT_RIGHT      = 'AttachmentSlots.WeaponRight'
-- Ours, so its customOffset can be tuned without moving the cigarette that also lives in WeaponLeft.
-- Falls back to the stock slot when the tweak has not been loaded yet (it needs a game launch).
-- The STOCK slot, because what places the weapon is the entry in the player's ItemAttachmentSlots
-- component (moved above), not the TweakDB record -- customOffset on the record was measured to do
-- nothing at all on this path.
local SLOT_LEFT       = 'AttachmentSlots.WeaponLeft'
local SLOT_LEFT_STOCK = 'AttachmentSlots.WeaponLeft'
local PLANE_WEAPON    = 2         -- ERenderingPlane.RPl_Weapon: the first-person weapon plane
local carryLeftSlot   = nil       -- which of the two actually took it, so it goes back from there
-- KEEPING THE WEAPON WHERE IT WAS. See the block above carryMeasure().
local carryOffsets    = {}        -- weapon record id -> {x,y,z}, the converged slot offset
local carryWant       = nil       -- {x,y,z} world position the weapon must keep
local carryKey        = nil       -- which weapon that is
local carryFixTries   = 0         -- iterations left for the current hand-over
local carryPending    = false     -- slot zeroed; measure and move on the next tick
local CARRY_TOL_M     = 0.01      -- a centimetre is close enough to stop
local CARRY_MAX_TRIES = 3
local muzzlePosProbed = false
local muzzleEnumDone = false

-- Per-frame reads without a closure or a fresh CName each time (CET's collector pays for both).
local function weaponHash(w) return tostring(w:GetEntityID().hash) end
local meleeFor, meleeWas = nil, false
local staticGroup = nil

-- WHERE THE BARREL LINE HITS THE WORLD. The overlay's dot used to mark a fixed 20 m down the barrel, so
-- it was only true at 20 m. This is the same line traced by the game's own SpatialQueriesSystem:
-- position and surface normal, about 5 us, and it goes through no hook of the port's -- in particular
-- not through the hitscan provider, where a ray claimed as ours also fires the hand recoil.
local function aimHit(x, y, z, q)
    local f = Quaternion.GetForward(q)
    if not f then return end
    local o = Vector4.new(x, y, z, 1)
    local far = Vector4.new(x + f.x * 200.0, y + f.y * 200.0, z + f.z * 200.0, 1)
    local sq = Game.GetSpatialQueriesSystem()
    if not sq then return end
    staticGroup = staticGroup or CName.new("Static")
    local ok, res = sq:SyncRaycastByCollisionGroup(o, far, staticGroup, false, false)
    if ok and res and res.position then
        local n = res.normal
        SetVRAimHit(res.position.x, res.position.y, res.position.z,
                    n and n.x or 0.0, n and n.y or 0.0, n and n.z or 0.0)
        if not aimHitProbed then
            aimHitProbed = true
            logf("aimHit OK: (%.3f, %.3f, %.3f) n=(%.2f, %.2f, %.2f)",
                 res.position.x, res.position.y, res.position.z,
                 n and n.x or 0.0, n and n.y or 0.0, n and n.z or 0.0)
        end
    end
end

local function updateMuzzle(wpn)
    local xf = wpn:GetMuzzleSlotWorldTransform()
    if not xf then return end
    local q = xf.Orientation or (xf.GetOrientation and xf:GetOrientation())
    if q and type(SetVRMuzzleQuat) == 'function' then
        SetVRMuzzleQuat(q.i, q.j, q.k, q.r)
    end
    -- The POSITION half of the same transform, which used to be dropped on the floor. The launch
    -- override replaced the shot's direction with the muzzle's and left its origin at the game
    -- camera -- i.e. at the left eye -- so the bullet flew parallel to the barrel but started an
    -- IPD away from the eye that was aiming. pcall'd because a wrong accessor here would take the
    -- whole weapon mod down with it, and with it the barrel dot and the aim override.
    -- Position, fetched the same way the line above fetches orientation: field first, then
    -- getter. The probe said xf.Position is nil outright, and WorldTransform exposes it as
    -- GetWorldPosition() -- exactly the field-or-method shape already used for the quaternion.
    -- Enumerated, not guessed: this object exposes `position` (lower case) and GetPosition().
    -- `Position` and GetWorldPosition() -- the two names I tried first -- do not exist on it.
    local pos = xf.position
    if not pos and xf.GetPosition then pos = xf:GetPosition() end

    -- Neither exists on this object, so stop guessing names one per round-trip and ask it what
    -- it has. Orientation is the control: that one is known to work, so if it does not show up
    -- in the listing then the listing itself is the thing that does not work here.
    if not pos and not muzzleEnumDone then
        muzzleEnumDone = true
        local keys = {}
        pcall(function()
            for k, _ in pairs(xf) do keys[#keys + 1] = tostring(k) end
        end)
        logf("muzzle xf: type=%s  keys=[%s]", type(xf), table.concat(keys, ", "))
        local names = { 'Position', 'position', 'WorldPosition', 'Translation', 'Trans',
                        'Orientation', 'GetWorldPosition', 'GetPosition', 'ToVector4',
                        'GetOrientation' }
        local found = {}
        for _, n in ipairs(names) do
            local t = 'nil'
            pcall(function() t = type(xf[n]) end)
            if t ~= 'nil' then found[#found + 1] = n .. '=' .. t end
        end
        logf("muzzle xf members: %s", table.concat(found, "  "))
        -- And the weapon itself, in case the muzzle position is reachable from there instead.
        local wt = 'nil'
        pcall(function() wt = tostring(wpn:GetWorldPosition()) end)
        logf("muzzle fallback: wpn:GetWorldPosition() = %s", wt)
    end

    if type(SetVRMuzzlePos) == 'function' and pos then
        local x, y, z
        -- WorldPosition keeps 17-bit fixed point, the same 1/131072 the render camera and the
        -- instance transforms use; a plain Vector4 keeps floats. Take whichever this build has.
        pcall(function()
            if type(pos.x) == 'number' then
                x, y, z = pos.x, pos.y, pos.z
            elseif pos.x and pos.x.Bits then
                x, y, z = pos.x.Bits / 131072.0, pos.y.Bits / 131072.0, pos.z.Bits / 131072.0
            elseif pos.ToVector4 then
                local v = pos:ToVector4()
                if v then x, y, z = v.x, v.y, v.z end
            elseif WorldPosition and WorldPosition.ToVector4 then
                local v = WorldPosition.ToVector4(pos)
                if v then x, y, z = v.x, v.y, v.z end
            end
        end)
        if x then
            SetVRMuzzlePos(x, y, z)
            if not muzzlePosProbed then
                muzzlePosProbed = true
                logf("muzzlePos OK: (%.4f, %.4f, %.4f)", x, y, z)
            end
            -- Where that line hits the world: see aimHit.
            if type(SetVRAimHit) == 'function' and q then
                pcall(aimHit, x, y, z, q)
            end
        elseif not muzzlePosWarned then
            muzzlePosWarned = true
            logf("muzzlePos: got %s but no component accessor matched (x type=%s)",
                 type(pos), type(pos.x))
        end
    elseif not muzzlePosWarned then
        muzzlePosWarned = true
        logf("muzzlePos: native=%s pos=%s", type(SetVRMuzzlePos), type(pos))
    end
end

-- HAND THE WEAPON OVER, AND TAKE IT BACK. Right grip while the left grip is held = the right hand lets
-- go and the left one keeps it; right grip again = back to the right hand. The left grip does not have
-- to stay down in between -- once it is in the left hand it stays there.
--
-- The move is the game's own three calls (transaction system), with three arguments that differ from
-- the AI examples for reasons written out above the state block: the entity is NOT destroyed, the
-- same object is re-attached, and the left slot's restrictions are ignored because a firearm is not on
-- its list.
-- THE SLOT ENTRY ON THE PLAYER, AND HOW TO MOVE IT.
--
-- ItemAttachmentSlots holds one entry per slot with relativePosition/relativeRotation in the bone's
-- frame. Element-wise assignment does not stick -- measured: the value reads back unchanged -- so the
-- array is copied, the entry replaced, and the whole array written back. Returns the entry index so
-- the caller can put the slot back where it found it.
local function slotEntryIndex(name)
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local c = pl:FindComponentByName(CName.new('ItemAttachmentSlots'))
    if not c then return nil end
    local arr = c.slots
    if not arr then return nil end
    for i = 1, #arr do
        if string.find(tostring(arr[i].slotName), name, 1, true) then return i, c end
    end
    return nil
end

-- POSITION AND ROTATION TOGETHER. Position alone puts the weapon on the right point wearing the left
-- slot's orientation, and since the two hand slots' axes are nowhere near each other the gun then
-- hangs low and to the right. Both fields, one write, whole array back.
local function slotSetRel(name, x, y, z, qi, qj, qk, qr)
    local idx, c = slotEntryIndex(name)
    if not (idx and c) then return false end
    -- NOTHING IS WRITTEN IF NOTHING CHANGES. The write has to be the WHOLE slots array or it is lost
    -- (measured), so it also rewrites the entries holding the weapon, the cigarette and the props --
    -- churn on the attachment machinery every time, for no change at all.
    local same = false
    pcall(function()
        local e = c.slots[idx]
        local p, r = e.relativePosition, e.relativeRotation
        same = p and math.abs(p.x - x) < 1e-5 and math.abs(p.y - y) < 1e-5 and math.abs(p.z - z) < 1e-5
        if same and qi and r then
            same = math.abs(r.i - qi) < 1e-5 and math.abs(r.j - qj) < 1e-5
               and math.abs(r.k - qk) < 1e-5 and math.abs(r.r - qr) < 1e-5
        end
    end)
    if same then return true end
    local ok = pcall(function()
        local arr = c.slots
        local e = arr[idx]
        e.relativePosition = Vector3.new(x, y, z)
        if qi then e.relativeRotation = Quaternion.new(qi, qj, qk, qr) end
        arr[idx] = e
        c.slots = arr            -- the whole array, or the write is lost
    end)
    return ok
end

-- Quaternion odds and ends the carry needs. Spelled out rather than pulled in, because the only other
-- copy in this port is in the plugin and a paraphrase of it would be a silent divergence.
local function qmul(a1, a2, a3, a4, b1, b2, b3, b4)
    return a4*b1 + a1*b4 + a2*b3 - a3*b2,
           a4*b2 - a1*b3 + a2*b4 + a3*b1,
           a4*b3 + a1*b2 - a2*b1 + a3*b4,
           a4*b4 - a1*b1 - a2*b2 - a3*b3
end
local function qrotv(i, j, k, r, x, y, z)
    local tx, ty, tz = 2.0*(j*z - k*y), 2.0*(k*x - i*z), 2.0*(i*y - j*x)
    return x + r*tx + (j*tz - k*ty),
           y + r*ty + (k*tx - i*tz),
           z + r*tz + (i*ty - j*tx)
end
-- Slerp from identity toward (i,j,k,r) by w -- the way home for the offset's rotation half.
local function qslerpId(i, j, k, r, w)
    if r < 0.0 then i, j, k, r = -i, -j, -k, -r end
    local li, lj, lk, lr
    if r > 0.9995 then
        li, lj, lk, lr = i*w, j*w, k*w, 1.0 + (r - 1.0)*w
    else
        local th0 = math.acos(r)
        local th  = th0 * w
        local s0  = math.sin(th0)
        local sa, sb = math.sin(th0 - th)/s0, math.sin(th)/s0
        li, lj, lk, lr = i*sb, j*sb, k*sb, sa + r*sb
    end
    local l = math.sqrt(li*li + lj*lj + lk*lk + lr*lr)
    if l < 1e-6 then return 0.0, 0.0, 0.0, 1.0 end
    return li/l, lj/l, lk/l, lr/l
end

-- A quaternion from a world-space basis (columns X, Y, Z), Shepperd's method: the branch on the
-- largest diagonal term is what keeps it stable when the trace is small.
local function quatFromBasis(X, Y, Z)
    local m = { {X[1], Y[1], Z[1]}, {X[2], Y[2], Z[2]}, {X[3], Y[3], Z[3]} }
    local tr = m[1][1] + m[2][2] + m[3][3]
    local i, j, k, r
    if tr > 0 then
        local s = math.sqrt(tr + 1.0) * 2
        r = 0.25 * s; i = (m[3][2]-m[2][3])/s; j = (m[1][3]-m[3][1])/s; k = (m[2][1]-m[1][2])/s
    elseif m[1][1] > m[2][2] and m[1][1] > m[3][3] then
        local s = math.sqrt(1.0 + m[1][1] - m[2][2] - m[3][3]) * 2
        r = (m[3][2]-m[2][3])/s; i = 0.25*s; j = (m[1][2]+m[2][1])/s; k = (m[1][3]+m[3][1])/s
    elseif m[2][2] > m[3][3] then
        local s = math.sqrt(1.0 + m[2][2] - m[1][1] - m[3][3]) * 2
        r = (m[1][3]-m[3][1])/s; i = (m[1][2]+m[2][1])/s; j = 0.25*s; k = (m[2][3]+m[3][2])/s
    else
        local s = math.sqrt(1.0 + m[3][3] - m[1][1] - m[2][2]) * 2
        r = (m[2][1]-m[1][2])/s; i = (m[1][3]+m[3][1])/s; j = (m[2][3]+m[3][2])/s; k = 0.25*s
    end
    return i, j, k, r
end

local function slotGetRel(name)
    local idx, c = slotEntryIndex(name)
    if not (idx and c) then return nil end
    local v = nil
    pcall(function() v = c.slots[idx].relativePosition end)
    return v
end

-- A SLOT'S FRAME, IN WORLD SPACE, WITHOUT ATTACHING ANYTHING TO IT.
--
-- CreateSlotPositionProvider takes a localOffset expressed in the slot's own frame, so asking it for
-- the slot at the origin and then at each unit axis and subtracting gives that frame's basis. Four
-- queries, no side effects, and it works before the weapon is moved -- which is the requirement:
-- everything has to be known BEFORE the hand-over, not corrected after it.
--
-- GameObject.GetSlotTransform would answer this in one call and is not exposed to scripts (measured:
-- nil on the player), which is why this takes the long way round.
local function slotFrame(name)
    local pl = Game.GetPlayer()
    if not pl then return nil end
    local function at(x, y, z)
        local pp, ok, pos = nil, false, nil
        pcall(function()
            pp = IPositionProvider.CreateSlotPositionProvider(pl, CName.new(name), Vector3.new(x, y, z))
        end)
        if pp then pcall(function() ok, pos = pp:CalculatePosition() end) end
        if ok and pos then return pos end
        return nil
    end
    local o, x, y, z = at(0, 0, 0), at(1, 0, 0), at(0, 1, 0), at(0, 0, 1)
    if not (o and x and y and z) then return nil end
    return {
        ox = o.x, oy = o.y, oz = o.z,
        xx = x.x - o.x, xy = x.y - o.y, xz = x.z - o.z,
        yx = y.x - o.x, yy = y.y - o.y, yz = y.z - o.z,
        zx = z.x - o.x, zy = z.y - o.y, zz = z.z - o.z,
    }
end

-- ONE CORRECTION STEP, run on a tick AFTER the weapon has landed in the left slot.
--
-- The weapon is where the attachment put it; carryWant is where it has to be. The difference is a
-- world vector, and customOffset is expressed in the slot's frame, so it is rotated by the inverse of
-- the weapon's own orientation -- the weapon is attached to that slot, so its frame IS the slot's
-- frame up to the weapon's own (constant) attachment rotation. That constant is why this iterates
-- instead of stepping once: if it is not identity the first correction overshoots or undershoots
-- slightly, and the second one takes out what is left.
local function carryMeasure()
    if not carryWant or carryFixTries <= 0 then return end
    local pl = Game.GetPlayer()
    local ts = Game.GetTransactionSystem()
    if not (pl and ts and carryLeftSlot) then return end
    local slot = TweakDBID.new(carryLeftSlot)
    local w = ts:GetItemInSlot(pl, slot)
    if not w then return end          -- the slot map answers a frame late; try again next tick

    local now = w:GetWorldPosition()
    local dx, dy, dz = carryWant.x - now.x, carryWant.y - now.y, carryWant.z - now.z
    local err = math.sqrt(dx*dx + dy*dy + dz*dz)
    if err <= CARRY_TOL_M then
        logAlways('carry: placed, error %.1f mm after %d fix(es)', err * 1000.0,
                  CARRY_MAX_TRIES - carryFixTries)
        carryFixTries = 0
        return
    end

    -- world delta -> the weapon's own frame
    local q = w:GetWorldOrientation()
    local lx, ly, lz = dx, dy, dz
    if q then
        -- v' = conj(q) * v * q, written out: rotate by the inverse quaternion
        local qi, qj, qk, qr = -q.i, -q.j, -q.k, q.r
        local tx = 2.0 * (qj * dz - qk * dy)
        local ty = 2.0 * (qk * dx - qi * dz)
        local tz = 2.0 * (qi * dy - qj * dx)
        lx = dx + qr * tx + (qj * tz - qk * ty)
        ly = dy + qr * ty + (qk * tx - qi * tz)
        lz = dz + qr * tz + (qi * ty - qj * tx)
    end

    -- A CHECK, NOT A CORRECTION. The offset was decided before the move; if it is wrong the honest
    -- thing is to say by how much and in which direction, not to shuffle the weapon around while the
    -- player is looking at it. The local components are what the offset would have to change BY.
    carryFixTries = 0
    logAlways('carry: check -- off by %.0f mm, local delta (%.3f %.3f %.3f)',
              err * 1000.0, lx, ly, lz)
end

-- WHETHER THE LEFT SLOT IS KNOWN TO BE CLEAN. Reading it pulls the player's whole ItemAttachmentSlots
-- array through CET -- twice, plus a string per entry -- and that ran every frame. Only this module ever
-- displaces the entry (the hand-over below), so it is read every frame while that may be the case, and
-- otherwise re-confirmed every two seconds: that still catches a load or a mod reload, which leave no
-- trace in this state.
local leftClean, leftCheckedAt = false, -10.0
local slotRightID, slotLeftID = nil, nil

local function carryTick(dt)
    if not CARRY_LEFT then return end
    -- LEFT-HANDED THE WEAPON IS ALREADY THE LEFT HAND'S, drawn onto its slot by the game itself: nothing to hand over.
    if not carryOn and type(VRWeaponInLeftHand) == 'function' and VRWeaponInLeftHand() then return end
    local pl = Game.GetPlayer()
    if not pl then return end
    -- Two guns in hand (the game's own dual wielding): the left slot holds the second gun, and nothing
    -- here may hand the right one over into it or reset its placement.
    local okDual, dual = pcall(function() return GameObject.IsDualWielding(pl) end)
    if okDual and dual then return end
    -- An implanted arm cannot be handed over as a loose weapon.
    if cyberware.kind(pl:GetActiveWeapon(),pl)~=0 then return end
    local ts = Game.GetTransactionSystem()
    if not ts then return end
    slotRightID = slotRightID or TweakDBID.new(SLOT_RIGHT)
    slotLeftID  = slotLeftID or TweakDBID.new(SLOT_LEFT_STOCK)
    local RIGHT, LEFT = slotRightID, slotLeftID

    local rg, lg = 0.0, 0.0
    if type(GetVRSharedSlot) == 'function' then
        rg = GetVRSharedSlot(49) or 0.0     -- right grip analog
        lg = GetVRSharedSlot(155) or 0.0    -- left grip pressed
    end
    local rDown  = rg > 0.5
    local rFresh = rDown and not carryRGripWas
    carryRGripWas = rDown

    -- THE RENDERING PLANE IS STILL OPEN. Moving the item between slots drops it off the first-person
    -- weapon plane and the game re-asserts it on a shot or a zoom; nothing here touches the camera to
    -- force that, by choice. Left as it is until a mechanism turns up that does not.
    -- THE SLOT IS CLEAN WHENEVER NOTHING HANGS ON IT. Catches the abnormal exits: holstered mid-carry,
    -- a load, a mod reload, a reload module that took the weapon.
    if not rFresh then
        local now = os.clock()
        if leftClean and now - leftCheckedAt < 2.0 then return end
        if ts:GetItemInSlot(pl, LEFT) == nil then
            local cur = slotGetRel('WeaponLeft')
            if cur and (math.abs(cur.x) + math.abs(cur.y) + math.abs(cur.z)) > 0.0005 then
                slotSetRel('WeaponLeft', 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0)
                logAlways('carry: left slot empty but displaced -- reset')
            end
            leftClean, leftCheckedAt = cur ~= nil, now
        end
        return
    end

    if not carryOn then
        -- NOT FOR PISTOLS: a one-handed weapon has no reason to change hands. Class 1 is
        -- handgun/revolver, resolved per draw from the record's own itemType; class 0 ("the record did
        -- not say") keeps the feature rather than losing it silently.
        if VRP_lastClass == 1 then return end
        -- the left hand has to be ON the gun: this is "the right hand lets go", not "the gun jumps"
        if lg <= 0.5 then return end
        local w = ts:GetItemInSlot(pl, RIGHT)
        if not w then return end
        local id = w:GetItemID()

        -- THE PLACEMENT, MEASURED BEFORE ANYTHING MOVES: the weapon's own transform against the target
        -- bone's frame. PropLeft rides the same bone as WeaponLeft and is never displaced, so it is the
        -- clean frame to measure in -- no need to unpick our own offset out of the reading.
        local W = slotFrame('PropLeft')
        local wp, wq = nil, nil
        pcall(function() wp = w:GetWorldPosition() end)
        pcall(function() wq = w:GetWorldOrientation() end)
        if not (W and wp and wq) then
            logAlways('carry: cannot measure (frame=%s pos=%s rot=%s)',
                      tostring(W ~= nil), tostring(wp ~= nil), tostring(wq ~= nil))
            return
        end
        local dx, dy, dz = wp.x - W.ox, wp.y - W.oy, wp.z - W.oz
        local ox = dx * W.xx + dy * W.xy + dz * W.xz          -- transpose(R_bone) * delta
        local oy = dx * W.yx + dy * W.yy + dz * W.yz
        local oz = dx * W.zx + dy * W.zy + dz * W.zz
        local bi, bj, bk, br = quatFromBasis({W.xx, W.xy, W.xz}, {W.yx, W.yy, W.yz},
                                             {W.zx, W.zy, W.zz})
        local ci, cj, ck, cr = -bi, -bj, -bk, br
        local ri = cr*wq.i + ci*wq.r + cj*wq.k - ck*wq.j
        local rj = cr*wq.j - ci*wq.k + cj*wq.r + ck*wq.i
        local rk = cr*wq.k + ci*wq.j - cj*wq.i + ck*wq.r
        local rr = cr*wq.r - ci*wq.i - cj*wq.j - ck*wq.k
        leftClean = false                 -- displaced from here on: the idle check reads it every frame
        local wrote = slotSetRel('WeaponLeft', ox, oy, oz, ri, rj, rk, rr)

        -- THE MOVE, AND THE FIFTH ARGUMENT IS THE WHOLE STORY. Passing the item OBJECT hands the game
        -- the visual it already has; with nil there -- how this was written, and what every note said
        -- to do -- the game resolves and rebuilds that visual for the new slot, and the rebuild
        -- dissolves in, which is the fade. With the object passed there is nothing to rebuild, and the
        -- plane argument still puts the weapon on the first-person plane.
        --
        -- shouldDestroyEntity stays false: it is the same entity the reload, the two-hand hold and the
        -- muzzle publishing all key their state to. keepWorldTransform is NOT passed -- with it the
        -- weapon is left standing in world space, measured kilometres away.
        local okR, okA = false, false
        pcall(function() okR = ts:RemoveItemFromSlot(pl, RIGHT, false) end)
        pcall(function() okA = ts:AddItemToSlot(pl, LEFT, id, true, w, PLANE_WEAPON) end)
        if not okA then
            logAlways('carry: add to the left slot refused (remove=%s)', tostring(okR))
        end
        carryOn = okA and true or false
        carryLeftSlot = SLOT_LEFT_STOCK
        carryItemID = id
        if carryOn then
            if type(SetVRCarryLeft) == 'function' then pcall(function() SetVRCarryLeft(1) end) end
        else
            -- put it back the same way it came, object and plane included
            pcall(function() ts:RemoveItemFromSlot(pl, LEFT, false) end)
            pcall(function() ts:AddItemToSlot(pl, RIGHT, id, true, w, PLANE_WEAPON) end)
            slotSetRel('WeaponLeft', 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0)
        end
        logAlways('carry: -> LEFT pos (%.3f %.3f %.3f) rot (%.3f %.3f %.3f %.3f) wrote=%s ok=%s',
                  ox, oy, oz, ri, rj, rk, rr, tostring(wrote), tostring(carryOn))
    else
        leftClean = false                 -- taken back below; the idle check confirms the reset
        local from = TweakDBID.new(carryLeftSlot or SLOT_LEFT_STOCK)
        local w = ts:GetItemInSlot(pl, from)
        -- THE REACH: the plugin answers it against its own radius (xr_carry_radius), the same one the
        -- finger preview springs on, so the fingers closing on the weapon and the button taking it
        -- happen at exactly the same distance.
        if w then
            local near = -1
            if type(GetVRCarryNear) == 'function' then
                pcall(function() near = GetVRCarryNear() or -1 end)
            end
            if near == 0 then
                logAlways('carry: right hand too far to take it back')
                return
            end
        end
        local id = carryItemID or (w and w:GetItemID())
        local okA = false
        if id then
            -- the same way back, object and plane both passed. `w` is read BEFORE the remove and stays
            -- valid across it, because the entity is not destroyed.
            pcall(function() ts:RemoveItemFromSlot(pl, from, false) end)
            pcall(function() okA = ts:AddItemToSlot(pl, RIGHT, id, true, w, PLANE_WEAPON) end)
            if not okA then logAlways('carry: add to the right slot refused') end
        end
        carryOn = not okA
        if okA then
            carryItemID, carryLeftSlot = nil, nil
        end
        -- identity again: PropLeft and the cigarette live on the same bone, and the next hand-over
        -- depends on this being clean.
        pcall(function() slotSetRel('WeaponLeft', 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0) end)
        if type(SetVRCarryLeft) == 'function' then pcall(function() SetVRCarryLeft(0) end) end
        logAlways('carry: -> RIGHT ok=%s', tostring(okA))
    end
end

-- A LOADED SAVE'S WEAPON, IN THE HAND THE CONTROLS USE. A save keeps which hand held the weapon, and a load draws
-- only the right hand's; the restore hands a lone weapon to the hand the controls use (EquipmentSystemPlayerData.
-- OnRestored in CyberpunkVRPort_LeftHanded), so left-handed it comes up in neither. From the player's attach this
-- watches for RESTORE_WATCH seconds: once the save's left-hand weapon has waited RESTORE_SETTLE seconds with the
-- hands empty and the game running, it is drawn (PlayerPuppet.VRLeftHandedDrawRestored); a lone weapon in the hand
-- the controls do not use changes hands the way a toggle does; a weapon already in the right hand ends the watch.
local RESTORE_WATCH = 120.0
local RESTORE_SETTLE = 1.5
local restoreWatch = nil    -- seconds the watch still runs; nil when no load is being watched
local restoreHeld = 0.0     -- seconds the current finding has held

registerForEvent('onInit', function()
    logf("weapon-aim init")
    Observe('PlayerPuppet', 'OnGameAttached', function() restoreWatch, restoreHeld = RESTORE_WATCH, 0.0 end)
end)

-- THE WEAPON CHANGES HANDS WITH THE CONTROLS. Left-handed, a weapon drawn on its own is the left hand's
-- (CyberpunkVRPort_LeftHanded); when the setting turns over with one out, it is put away
-- (PlayerPuppet.VRLeftHandedRehand) and drawn again into the other hand once both hands are empty -- on the change,
-- not every frame, so a weapon the player put in a hand by other means stays where it is. A weapon drawn meanwhile,
-- or hands that stay full for REHAND_WAIT seconds, call the redraw off.
local REHAND_WAIT = 3.0
local handedWas = nil
local rehandLeft = nil      -- seconds the redraw still waits for empty hands

-- What the load left in the hands: 'draw' (the save's left-hand weapon, not drawn), 'rehand' (a lone weapon in the
-- hand the controls do not use), 'done' (a weapon where it belongs), or nil while there is nothing to go on yet.
local function restoreFinding(pl, left)
    local r = Game.GetSystemRequestsHandler()
    if r:IsPreGame() or r:IsGamePaused() then return nil end
    local rw, lw = GameObject.GetRightActiveWeapon(pl), GameObject.GetLeftActiveWeapon(pl)
    if (left and rw and not lw) or (not left and lw and not rw) then return 'rehand' end
    if rw or lw then return 'done' end
    if pl:VRLeftHandedRestorePending() then return 'draw' end
    return nil
end

local function restoreFrame(pl, dt, left)
    if type(pl.VRLeftHandedRestorePending) ~= 'function' then restoreWatch = nil; return end
    restoreWatch = restoreWatch - dt
    if restoreWatch <= 0.0 then
        restoreWatch = nil
        logAlways('handedness: nothing to restore after the load')
        return
    end
    local f = restoreFinding(pl, left)
    if f == 'done' then restoreWatch = nil; return end
    restoreHeld = f and (restoreHeld + dt) or 0.0
    if not f or restoreHeld < RESTORE_SETTLE then return end
    restoreWatch = nil
    if f == 'draw' then
        logAlways('handedness: the save\'s left-hand weapon drawn=%s', tostring(pl:VRLeftHandedDrawRestored()))
    else
        local away = pl:VRLeftHandedRehand()
        rehandLeft = away and REHAND_WAIT or nil
        logAlways('handedness: the save\'s weapon came up in the other hand, put away to change hands=%s', tostring(away))
    end
end

local function handednessFrame(pl, dt)
    if type(VRLeftHanded) ~= 'function' or type(pl.VRLeftHandedRehand) ~= 'function' then return end
    dt = dt or 0.016
    local now = VRLeftHanded() == true
    if handedWas ~= nil and now ~= handedWas then
        local away = pl:VRLeftHandedRehand()
        rehandLeft = away and REHAND_WAIT or nil
        logAlways('handedness: left-handed=%s, weapon put away to change hands=%s', tostring(now), tostring(away))
    end
    handedWas = now
    if restoreWatch and not rehandLeft then restoreFrame(pl, dt, now) end
    if not rehandLeft then return end
    rehandLeft = rehandLeft - (dt or 0.016)
    if pl:VRLeftHandedHandsFree() then
        rehandLeft = nil
        logAlways('handedness: drawn again=%s', tostring(pl:VRLeftHandedRedraw()))
    elseif rehandLeft <= 0.0 then
        rehandLeft = nil
        logAlways('handedness: the hands did not empty, no redraw')
    end
end

-- The per-frame weapon work as a plain function: `pcall(function() ... end)` built a closure every frame,
-- and the mods' per-frame garbage is what CET's collector pays for (the GC step in CyberpunkVRPort_Stereo).
local function weaponFrame(dt)
    local pl = Game.GetPlayer()
    local wpn = pl and pl:GetActiveWeapon()
    -- THE MUZZLE GOES FIRST, AND NOTHING IS ALLOWED IN FRONT OF IT. Everything in this callback
    -- shares one pcall, so whatever runs first owns the frame: put something ahead of this line and
    -- a throw in it stops the muzzle quaternion from being published at all, the plugin keeps
    -- yesterday's orientation, and the bullet leaves the barrel pointing the wrong way. That was
    -- tried -- the recoil block was moved above this line to isolate it -- and the aim broke
    -- immediately. Isolation belongs in the OTHER direction: the muzzle keeps its place and the
    -- newcomer gets its own pcall.
    -- Melee weapons need not expose a muzzle transform. Keep firearm aim
    -- first, but isolate missing muzzle data from weapon/hold selection.
    if wpn then pcall(updateMuzzle, wpn) end
    -- The weapon's id, read once a frame: the recoil block and the draw sound below both key on it.
    local curWid = nil
    if wpn then
        local okW, w = pcall(weaponHash, wpn)
        if okW then curWid = w end
    end
    if wpn then
        local wid = curWid
        local okR, errR = pcall(killCameraRecoil, wpn, wid)
        if not okR then
            pcall(function()
                local f = io.open('recoil_probe.txt', 'a')
                if f then f:write('killCameraRecoil threw: ' .. tostring(errR) .. string.char(10)); f:close() end
            end)
        end
    end
    if pl then pcall(leftGunFrame, pl, dt) end
    if pl then pcall(handednessFrame, pl, dt) end

    -- weapon draw sound (see equipSnd* header): fires on entity change, any weapon class
    if not equipSndInit then
        equipSndInit = true
        equipSndId = curWid
    elseif curWid ~= equipSndId then
        equipSndId = curWid
        recoilKilled = nil
        if wpn and pl and pl.VREquipSound then
            pcall(function() pl:VREquipSound(wpn) end)
        end
    end

    -- Publish the LIVE camera zoom so the dxgi overlay scales the barrel laser dot by the real
    -- DIAGNOSTIC ONLY (scope changes GetZoom, NOT FOV; PSM.ZoomLevel is only a level index).
    -- The overlay takes ADS magnification from MAIN's own projection, not from this.
    if type(SetVRZoomLevel) == 'function' then
        local cam = pl and pl:GetFPPCameraComponent()
        if cam then
            local z = cam:GetZoom()
            if z and z > 0.0 then SetVRZoomLevel(z) end
        end
    end

    guardClock = guardClock + (dt or 0.016)
    local cyberwareKind=cyberware.kind(wpn,pl)
    if cyberwareKind~=0 then
        -- Never run the ordinary 1.25m right-weapon trace on arm cyberware,
        -- even while the rig is rebinding or a sample is unavailable.
        guardStats(pl,false,false);guardWasOn=false
        return
    end

    -- VR MOTION MELEE: probe every frame the weapon is being SWUNG (speed relative to the player,
    -- so walking/turning doesn't count); the redscript helper does precise per-NPC enter detection
    -- and queues a native damage hit. Detection requires a melee weapon in the right hand AND the
    -- redscript helper VRMeleeBladeHit to be compiled in.
    if not (meleeEnabled and pl and wpn and pl.VRMeleeBladeHit) then
        -- guard cleanup on unequip/holster/death: the stats must never outlive the blade in hand
        if pl then guardStats(pl, false, false) else guardParryMod = nil; guardBlockMod = nil end
        guardWasOn = false
        return
    end
    -- An item does not change class: asked once per weapon entity, not every frame.
    local isMelee = false
    if curWid ~= nil and meleeFor == curWid then
        isMelee = meleeWas
    else
        pcall(function() isMelee = WeaponObject.IsMelee(wpn:GetItemID()) end)
        meleeFor, meleeWas = curWid, isMelee
    end
    if not isMelee then
        guardStats(pl, false, false)
        guardWasOn = false
        return
    end

    local wp = wpn:GetWorldPosition()
    local pp = pl:GetWorldPosition()
    local fwd = wpn:VRMeleeDirection()
    if not (wp and pp and fwd) then return end

    -- VR GUARD decision (see the header above): guard ON unless the blade points into the
    -- forward thrust cone. thrust = dot(normalized 3D blade fwd, normalized horizontal body
    -- fwd): forward-horizontal ≈ 1 (no guard), up/down/across ≈ 0, reverse < 0 (guard).
    local guardOn = false
    local pfwd = pl:GetWorldForward()
    if pfwd then
        local pfx, pfy = pfwd.x, pfwd.y
        local pfl = math.sqrt(pfx*pfx + pfy*pfy)
        local bfx, bfy, bfz = fwd.x, fwd.y, fwd.z
        local bfl = math.sqrt(bfx*bfx + bfy*bfy + bfz*bfz)
        if pfl > 0.001 and bfl > 0.001 then
            local thrust = (bfx*pfx + bfy*pfy) / (pfl * bfl)
            guardOn = thrust < GUARD_THRUST_DOT
        end
    end
    if guardOn and not guardWasOn then
        guardParryUntil = guardClock + GUARD_PARRY_WINDOW   -- fresh raise => parry window
    end
    guardWasOn = guardOn
    if guardOn then
        local parry = guardClock < guardParryUntil
        guardStats(pl, parry, not parry)
    else
        guardStats(pl, false, false)
    end

    -- Physical melee updates independently below, so a gun-only muzzle or
    -- recoil early return cannot disable melee swing processing.

end

local function updateWeapon(dt)
    pcall(carryTick, dt)
    -- carryMeasure is not called any more: it was the post-hoc correction of a placement that
    -- is now rebuilt from the hands every frame, so there is nothing left for it to correct.
    -- install the GetOrientation VMT instrument + override hooks once, after RTTI is ready
    if not installed then
        installTimer = installTimer + (dt or 0.016)
        if installTimer > 3.0 and type(InstallVRProvInstrument) == 'function' then
            local r = 0
            pcall(function() r = InstallVRProvInstrument() end)
            -- The weapon-aim family: XFORM-GETTER, the shot bracket and physArgSnapshot. All of it has
            -- been sitting at installed=0, which is why every one of those counters reads zero in the
            -- dump. It is what the launch ORIGIN has to be found with -- the provider slots do not
            -- carry it (slots 3..42 return nothing that looks like a world position). Read-only until
            -- something is told to mutate.
            if type(InstallWeaponAimHook) == 'function' then
                logf('InstallWeaponAimHook = %s', tostring(InstallWeaponAimHook()))
            else
                logf('InstallWeaponAimHook: native missing')
            end
            logf("InstallVRProvInstrument = %s", tostring(r))
            installed = true
        end
    end

    pcall(weaponFrame, dt)
end

local cyberwareClock=0
local function combatFrame(dt)
    local pl=Game.GetPlayer();local wpn=pl and pl:GetActiveWeapon()
    -- Runtime weapon hooks need a live player, not the front-end/loading
    -- scene while the game is still constructing script/resource types.
    if pl then pcall(updateWeapon,dt) end
    local kind=cyberware.kind(wpn,pl)
    Game.VRCyberwareSelect(cyberware.inputKind())
    cyberware.handEquipment(pl)
    local throwing=physicalThrow.update(dt,pl,wpn)
    if kind~=0 then cyberware.update(dt,pl,wpn,kind,cyberwareClock)
    elseif cyberware.inputKind()==5 and meleeEnabled and not throwing then
        local pos=wpn:GetWorldPosition()
        local forward=wpn:VRMeleeDirection()
        if pos and forward then cyberware.ordinary(dt,pl,wpn,pos,forward,pl:GetWorldPosition()) end
    end
end

registerForEvent('onUpdate',function(dt)
    -- CET keeps one handler per event in a mod. Registering cyberware as a
    -- second onUpdate replaced weapon-name/hold selection and ordinary melee.
    -- Keep one entry point with isolated weapon and tracked-combat updates.
    cyberwareClock=cyberwareClock+(dt or 0)
    local ok,err=pcall(combatFrame,dt)
    if not ok then
        cyberware.state.error=tostring(err)
        if cyberwareLastError~=tostring(err) then cyberwareLastError=tostring(err);logAlways('cyberware combat: %s',cyberwareLastError) end
    end
end)

registerForEvent('onShutdown', function()
    pcall(function()
        local pl = Game.GetPlayer()
        if pl then guardStats(pl, false, false) end
    end)
end)
return {CyberwareStatus=function() return cyberware.state end,ThrowStatus=function() return physicalThrow.state end}
