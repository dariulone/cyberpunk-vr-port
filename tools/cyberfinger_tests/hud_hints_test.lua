local hints = dofile(assert(arg[1]))
local function widget(controller, children)
    return {
        IsVisible = function() return true end, GetOpacity = function() return 1 end,
        GetController = function() return controller end,
        IsA = function() return children ~= nil end,
        GetNumChildren = function() return #children end,
        GetWidgetByIndex = function(_, i) return children[i + 1] end,
    }
end
local function caption(value)
    local w = widget()
    w.value = value
    function w:VRShowControlHint(label, frame)
        if self.value ~= self.decorated then self.original = self.value end
        self.frame = frame
        self.decorated = self.original .. ' [' .. label .. ']'
        self.value = self.decorated
    end
    function w:VRRestoreControlHint(frame)
        if self.frame == frame then return end
        if self.value == self.decorated then self.value = self.original end
        self.decorated = nil
    end
    return w
end
local function ctrl(class)
    return {GetClassName=function() return class end, IsA=function(_, k) return k == class end}
end
local function tree(class, rows)
    local c = ctrl(class)
    c.GetRootWidget = function() return widget(nil, rows) end
    return c
end
local display = ctrl('inkInputDisplayController')
local action = 'Exit'
display.GetInputAction = function() return action end
local buttons = {widget(display)}
local row = ctrl('gameuiInputHintController')
row.textWidgetRef, row.inputDisplayContainer = caption('Salir'), widget(nil, buttons)
local dialogue = ctrl('DialogChoiceLogicController')
dialogue.m_ActiveTextRef, dialogue.m_isSelected, dialogue.m_InputView = caption('Yes'), true, {m_AllItemsNum=2}
local item = ctrl('interactionItemLogicController')
item.m_label, item.vrCFAction, item.vrCFHold = caption('Use'), 'Choice2', true
local roots = {
    tree('gameuiInputHintManagerGameController', {widget(row)}),
    tree('gameuiInteractionsHubGameController', {widget(dialogue), widget(item)}),
    tree('UnrelatedModHud', {widget(row)}),
}
local gamepad = true
Game = {
    GetPlayer=function() return {PlayerLastUsedPad=function() return gamepad end} end,
    VRInkLayer=function() return {GetGameControllers=function() return roots end} end,
}
CName = {new=function(s) return s end}
NameToString = function(s) return s end
inkWidgetRef = {Get=function(w) return w end}
spdlog = {error=error}
function VRCyberFingerHint(name, hold)
    return ({Exit='Hold Left Grip 0.4 s', ChoiceApply='Right Trigger', ChoiceScroll='Right Stick Up/Down',
        Choice2=hold and 'Hold Right Grip' or 'Right Grip'})[name] or ''
end
local function scan() hints.update(.3, true, true) end
scan();scan()
assert(row.textWidgetRef.value == 'Salir [Hold Left Grip 0.4 s]')
assert(dialogue.m_ActiveTextRef.value == 'Yes [Right Stick Up/Down / Right Trigger]')
assert(item.m_label.value == 'Use [Hold Right Grip]')
dialogue.m_hasDedicatedInput = true
buttons[2] = widget(display)
scan()
assert(row.textWidgetRef.value == 'Salir' and dialogue.m_ActiveTextRef.value == 'Yes')
buttons[2] = nil
scan()
row.textWidgetRef.value, action = 'Replacement', 'Unknown'
scan()
assert(row.textWidgetRef.value == 'Replacement', 'recycled caption overwritten')
gamepad = false
scan()
assert(item.m_label.value == 'Use', 'keyboard retained CF label')
action, gamepad = 'Exit', true
scan()
hints.update(0, false)
assert(row.textWidgetRef.value == 'Replacement' and item.m_label.value == 'Use')
print('CyberFinger HUD hint rows passed')
