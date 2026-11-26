-- The named surfaces are authored disabled. Native panels and glass return to
-- their previous visibility when an attachment takes over or the weapon leaves VR.
local sights = {
    guillotine = {camera=true, hide={'w_smg__budget_guillotine__base1_1413','w_smg__budget_guillotine__base1_2436'}},
    hercules = {camera=true, hide={'w_special__militech_hercules__base1_3813','w_special__militech_hercules__base1_1840',
        'w_special__militech_hercules__base1_6261'}},
    pozhar = {hide={'w_rifle_precision__techtronika_pozhar__base1_5102','w_rifle_precision__techtronika_pozhar__base1_1880',
        'w_rifle_precision__techtronika_pozhar__base1_01_screen','w_rifle_precision__techtronika_pozhar__base1_01_screen7450'}},
    pozhar_xmod = {hide={'w_rifle_precision__techtronika_pozhar__base1_03_indicator_front','w_rifle_precision__techtronika_pozhar__base1_03_indicator_front3711',
        'w_rifle_precision__techtronika_pozhar__base1_03_holo','w_rifle_precision__techtronika_pozhar__base1_01_screen7450'}},
    ticon = {hide={'w_handgun__militech_ticon__base1_8364','w_handgun__militech_ticon__base1_7021'}},
    dante = {attachment=true, surface='vr_builtin_dante',
        hide={'WorldWidget0856','WorldWidget0352','w_att__scope_short_05_7842','w_att__scope_short_05_2507'}},
    shingen = {scopeOffset={x=0,y=.057893,z=.159352},hide={}},
    yukimura = {scopeOffset={x=0,y=0,z=.07},hide={}},
    ajax = {scopeOffset={x=0,y=.0678,z=.06661},hide={'on_weapon_ui'}},
    burya = {scopeOffset={x=0,y=-.00365,z=.0724},hide={'w_revolver__techtronika_burya__base1_8120'}},
    senkoh = {keepEyeRelief=true,hide={'w_smg__senkoh_lx__base1_01_indicator_front',
        'w_smg__senkoh_lx__base1_01_indicator_front6451','w_smg__senkoh_lx__base1_01_indicator_front5314'}},
}
local bySurface = {}
for key,sight in pairs(sights) do
    sight.key=key
    sight.surface='vr_builtin_'..key
    sight.panel=sight.camera and (sight.surface..'_panel') or nil
    bySurface[sight.surface]=sight
    local hidden={}
    for _,name in ipairs(sight.hide) do hidden[name]=true end
    sight.hide=hidden
end
return bySurface
