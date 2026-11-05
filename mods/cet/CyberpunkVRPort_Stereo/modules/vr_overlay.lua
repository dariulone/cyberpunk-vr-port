-- The VR panel never pauses gameplay or enters the native menu state.
-- A selected game action opens ESC and delegates to its actual item handler.
local M = { error=nil, active=false, phase="idle", waiting=0 }
local actions, pending, requestedClose = {}, nil, false
local function englishLabel(a)
    local names={OnClosePauseMenu="CONTINUE",OnSwitchToLoadGame="LOAD GAME",OnSwitchToSettings="SETTINGS",OnCreditsPicker="CREDITS",OnBuyGame="BUY GAME"}
    if names[a.event] then return names[a.event] end
    local kind=a.kind or (a.action and a.action:match(":%s*([%w_]+)"))
    return ({Save="SAVE GAME",QuickSave="QUICKSAVE",ExitToMainMenu="MAIN MENU",ExitGame="QUIT GAME"})[kind] or "GAME ACTION"
end
local function controllers()
    local layer=Game.VRInkLayer(CName.new("inkMenuLayer"))
    local pause, ingame, background
    for _,c in ipairs(layer and layer:GetGameControllers() or {}) do
        local name=NameToString(c:GetClassName())
        if name=="PauseMenuGameController" then pause=c end
        if name=="gameuiInGameMenuGameController" then ingame=c end
        if name=="PauseMenuBackgroundGameController" then background=true end
    end
    return pause,ingame,background
end
local function nativeItems(pause)
    local root=pause and pause:GetRootWidget()
    local list=root and root:GetWidgetByPathName(CName.new("mainListVert"))
    local result={}
    if list then for i=0,math.min(list:GetNumChildren(),16)-1 do
        local widget=list:GetWidgetByIndex(i)
        local item=widget and widget:GetController()
        local data=item and item:GetData()
        if data then result[#result+1]={index=i,controller=item,label=tostring(data.label),
            event=NameToString(data.eventName),action=tostring(data.action)} end
    end end
    return result
end
local function defaultItems(handler)
    local specs={
        {"UI-Labels-Resume","OnClosePauseMenu"},
        {"UI-ResourceExports-SaveGame",nil,"Save"},
        {"UI-ScriptExports-LoadGame0","OnSwitchToLoadGame"},
        {"UI-Labels-Settings","OnSwitchToSettings"},
        {"UI-Labels-Credits","OnCreditsPicker"},
        {"UI-Labels-ExitToMenu",nil,"ExitToMainMenu"},
        {"UI-Labels-CloseGame",nil,"ExitGame"},
    }
    actions={}
    for _,s in ipairs(specs) do
        if s[2]~="OnSwitchToLoadGame" or handler:RequestSavesCountSync()>0 then
            local action={event=s[2],kind=s[3]};action.label=englishLabel(action)
            actions[#actions+1]=action
        end
    end
end
local function labels()
    local out={};for _,a in ipairs(actions) do out[#out+1]=a.label:gsub("[\r\n]"," ") end
    return table.concat(out,"\n")
end
local function same(a,b)
    if a.event and a.event~="None" then return a.event==b.event end
    if a.action then return a.action==b.action and a.nativeLabel==b.label end
    return a.kind and b.action:match(":%s*([%w_]+)")==a.kind
end
function M.tick()
    local ok,err=pcall(function()
        local command=Game.VROverlayCommand()
        if command==0 and M.phase=="idle" then return end
        local pause,ingame,background=controllers()
        if command==1 then
            M.phase="opening";M.waiting=180;requestedClose=false;pending=nil
            if pause then
                actions={};for _,a in ipairs(nativeItems(pause)) do
                    actions[#actions+1]={label=englishLabel(a),nativeLabel=a.label,event=a.event,action=a.action}
                end
            elseif #actions==0 then defaultItems(Game.GetSystemRequestsHandler()) end
        elseif command==2 then
            M.phase="idle";M.active=false;pending=nil
            Game.VROverlayMenu(false,false,labels());return
        elseif command>=100 and command<116 then
            pending=actions[command-99]
            if pending and pending.event=="OnClosePauseMenu" then
                M.phase="idle";M.active=false;pending=nil
                Game.VROverlayMenu(false,false,labels());return
            end
            if pending and ingame then
                M.phase="native";M.waiting=180
                if not pause then ingame:SpawnMenuInstanceEvent(CName.new("OnOpenPauseMenu")) end
            else M.phase="idle" end
        end
        if M.phase=="opening" then
            M.waiting=M.waiting-1
            if pause or background then
                if ingame and not requestedClose then
                    ingame:SpawnMenuInstanceEvent(CName.new("OnClosePauseMenu"));requestedClose=true
                end
            elseif ingame then M.phase="panel" end
            if M.waiting<=0 then M.phase="idle";error("Could not leave the native pause presentation") end
        elseif M.phase=="native" then
            M.waiting=M.waiting-1
            if pause and pending then
                for _,a in ipairs(nativeItems(pause)) do if same(pending,a) then
                    pause:OnMenuItemActivated(a.index,a.controller);pending=nil;break
                end end
                -- If no matching item exists, leave the real menu visible.
                M.phase="idle";pending=nil
            elseif M.waiting<=0 then M.phase="idle";pending=nil end
        elseif M.phase=="panel" and (pause or background or not ingame) then M.phase="idle" end
        local ready=M.phase=="panel" and pause==nil and not background
        M.active=Game.VROverlayMenu(ready,ingame~=nil,labels())
        if M.phase=="panel" and not M.active then M.phase="idle" end
    end)
    if ok then M.error=nil
    else
        pcall(function() Game.VROverlayMenu(false,false,"") end)
        M.phase="idle";M.active=false
        if M.error~=tostring(err) then M.error=tostring(err);print("[Stereo.VR menu] "..M.error) end
    end
end
function M.shutdown()
    pcall(function() Game.VROverlayMenu(false,false,"") end)
    M.phase="idle";M.active=false
end
return M
