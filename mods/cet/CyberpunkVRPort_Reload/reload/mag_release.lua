-- A logical B edge can hand the seated prop directly to an available grip.
-- H3VR2 build 5b67417c: MagazineRelease.OnMagazineRemove calls GrabMagazine,
-- which hands the virtual item to InteractionHand.ForceGrab. CPVR owns the B binding.
-- Item type is queried only on an eligible edge; other reloads keep their own rules.
return function(weapon, edge, gripAvailable, inReach, busy)
    if not (edge and gripAvailable and inReach) or busy then return false end
    if type(GetVRTwoHandActive) == 'function' and GetVRTwoHandActive() then return false end
    local ok, pistol = pcall(function()
        return WeaponObject.IsOfType(weapon:GetItemID(), gamedataItemType.Wea_Handgun)
    end)
    return ok and pistol
end
