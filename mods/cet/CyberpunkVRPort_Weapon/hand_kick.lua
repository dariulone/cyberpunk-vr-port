-- Presentation references from the linked VR-game prefabs: one/two-hand pose
-- contribution and hand-pose smoothing. These are not peak angles or fire rates.
local classic = { .20, .40, .03, reference = 'M1911A1' } -- Modern1911 uses the same pose settings.
local bold    = { .30, .60, .04, reference = 'G17Gen3' }
local compact = { .15, .25, .04, reference = 'HPoint380' }
local long    = { .20, .20, .06, reference = 'R1022Classic' } -- Shared with Tactical.

-- Footlong.Recoil's shared angular restoring strength is 17 one-handed, 34
-- two-handed (56 only when the game detects a stock). For the same input
-- impulse, a spring's excursion scales with the inverse square root of its
-- stiffness. Use that ratio as a CPVR presentation accent, not as an angle
-- or a measured H3VR peak. The native lever and the player's slider still apply.
local supportedExcursion = math.sqrt(17 / 34)

-- Each row is {controller pose, CP presentation accent, legacy cycle fraction}.
-- The fraction remains available to existing profiles and telemetry.
local handgun = {
    wea_set_unity={classic,1,.180/.300}, wea_set_liberty={bold,1,.185/.275},
    wea_set_nue={bold,1.10,.205/.350}, wea_set_grit={compact,1,.160/.200},
    wea_set_omaha={compact,1,.155/.1777}, wea_set_silverhand={bold,1.42,.220/.300},
    wea_set_chao={compact,.78,.84}, wea_set_kappa={compact,.78,.90},
    wea_set_yukimura={compact,.90,.88}, wea_set_slaught={compact,1,.91},
    wea_set_lexington={classic,.90,.78}, wea_set_kenshin={classic,1,.82},
    wea_set_ticon={classic,1.06,.75},
}
handgun.wea_set_unity_dex=handgun.wea_set_unity
handgun.wea_set_liberty_dex=handgun.wea_set_liberty
handgun.wea_set_chao_voodoo=handgun.wea_set_chao
handgun.wea_set_kenshin_chaos=handgun.wea_set_kenshin
handgun.wea_set_ticon_reed=handgun.wea_set_ticon
handgun.wea_set_yukimura_preset=handgun.wea_set_yukimura
handgun.wea_set_toygun={compact,.45,.92}
local revolver={wea_set_burya={bold,1.50,.390/.600},wea_set_metel={bold,1.35,.405/.600},
    wea_set_nova={classic,1.22,.215/.300},wea_set_overture={bold,1.32,.270/.400},
    wea_set_quasar={classic,1.18,.155/.180}}
revolver.wea_set_nova_suppressor=revolver.wea_set_nova
local rifle={wea_set_kyubi={long,1,.170/.200},wea_set_special_plasma_cutter={long,.84,.82}}
local precision={wea_set_achilles={long,.72,.175/.200},wea_set_sor22={long,.85,.250/.500},
    wea_set_kolac={long,.95,.265/.500}}
local sniper={wea_set_ashura={long,.70,.410/.600},wea_set_grad={long,.95,1.15/2.70},
    wea_set_rasetsu={long,.85,.720/1.20},wea_set_nekomata={long,.92,.450/.600},
    wea_set_osprey={long,.88,.85}}
sniper.wea_set_grad_panam=sniper.wea_set_grad
local families={wea_handgun=handgun,wea_revolver=revolver,wea_rifle=rifle,wea_precisionrifle=precision,wea_sniperrifle=sniper}
-- These records have no usable audio family, or share one with another item.
local rocket={long,1.02,1.55/2.20}
local retroNue={bold,1.10,.205/.400}
local handgunRecords={['Items.Base_Arasaka3D_Gun']=handgun.wea_set_silverhand,
    ['Items.Craftable_Legendary_Yukimura']=handgun.wea_set_yukimura,['Items.BlackwallForce']=handgun.wea_set_chao}
local function mode(key,property)
    local flat=TweakDB:GetFlat(key..property)
    return flat and (TDBID.ToStringDEBUG(flat) or ''):match('^(TriggerMode%.%w+)')
end
return function(_,key,itemType)
    local token=tostring(itemType or ''):lower():match('%f[%w_]wea_[%w_]+')
    local tableForType=families[token]
    if not tableForType or not key then return nil end
    local audio=TweakDB:GetFlat(key..'.audioName')
    local name=type(audio)=='string' and audio or audio and audio.value
    local row=tableForType[name]
    if token=='wea_handgun' then
        row=handgunRecords[key] or row
        if name=='wea_set_nue' and key:match('^Items%.Preset_Nue_Arasaka_2020') then row=retroNue end
    elseif token=='wea_sniperrifle' and name=='wea_set_grad' and (key=='Items.Base_RocketLauncher' or key:match('^Items%.Preset_RocketLauncher_')) then
        row=rocket
    end
    if not row then return nil end
    if not mode(key,'.primaryTriggerMode') and not mode(key,'.secondaryTriggerMode') then return nil end
    local one,two=row[1][1],row[1][2]
    local solo=row[2]*(1+2*(one-.20))
    return solo,solo*supportedExcursion*(1+one)/(1+two),row[3],row[1][3],row[1].reference
end
