local K=require("modules/ink")
local L=require("modules/locale")
local G=require("modules/glamdoc_data")
local Images=require("modules/gameplay_data")
local V={}
local W,X=1780,710 -- standalone panel centred in the 3200-pixel menu canvas
local function tr(ru,en) return L.tr(ru,en) end
local function redraw(kit) kit.rebuild(false,false) end
local function label(index)
  local key=G.labels[index];local text=GetLocalizedText(key)
  return text~="" and text or ({"Launcher","Mantis Blades","Monowire","Gorilla Arms"})[index]
end
local function action(kit,p,x,y,w,text,enabled,fn,key)
  local bg=K.roundFill(p,x,y,w,68,K.NAVY,enabled and .18 or .04,4)
  local edge=K.cutFrame(p,x,y,w,68,enabled and K.CYAN or K.GREY,enabled and .5 or .2,10,1.5)
  local textWidget=K.text(p,x+12,y+2,w-24,64,text,32,enabled and K.TEXT or K.GREY,"center",true)
  local b=kit.button("p1",kit.at(X,kit.P1Y),x,y,w,68,function() if enabled then fn() end end,function(on)
    if not enabled then return end
    bg:SetTintColor(K.color(on and K.YELLOW or K.NAVY));bg:SetOpacity(on and .22 or .18)
    K.tint(edge,on and K.YELLOW or K.CYAN,on and 1 or .5)
    textWidget:SetTintColor(K.color(on and K.YELLOW or K.TEXT))
  end)
  b.key=key;return b
end
function V.draw(kit,p)
  local s=G.begin(kit.vendorSession());if not s then return end
  s.drawnRevision=s.revision
  kit.glass(p,W,1210,X,kit.P1Y)
  K.text(p,24,8,1060,76,tr("ГЛЭМДОК","GLAMDOC"),42,K.CYAN)
  K.text(p,1120,12,630,64,"€$ "..s.visit.manager:GetLocalPlayerCurrencyAmount(),34,K.YELLOW,"right")
  local busy=s.pending~=nil
  for i=1,4 do
    local index=i;local x=24+(i-1)*435
    action(kit,p,x,100,420,label(i),not busy and not s.preview,function()
      s.area=index;s.focus=nil;s.confirm=nil;s.revision=s.revision+1;redraw(kit)
    end,"glamdocArea:"..i)
    if s.area==i then K.rect(p,x+8,170,404,3,K.YELLOW,1) end
  end
  local arm=G.arm(s)
  K.text(p,26,185,1725,60,arm and tr("Установлено: ","Installed: ")..label(arm)
    or tr("Имплант рук не установлен. Покупка доступна, примерка и применение требуют совместимого импланта.",
      "No arm cyberware installed. You can purchase styles; preview and apply require compatible arm cyberware."),28,arm and K.TEXT or K.YELLOW,nil,true)
  local entries=G.rows();s.rows=entries
  local rows={}
  for _,entry in ipairs(entries) do
    local v=entry
    rows[#rows+1]={h=124,build=function(c,it,view)
      local selected=s.focus and s.focus.key==v.key
      local bg=K.roundFill(c,4,6,view.cw-8,110,selected and K.YELLOW or K.ICE,selected and .19 or .06,7)
      local x=16
      if v.icon and v.icon~="" and v.icon~="None" then
        local picture=Images.image(TweakDBID.new(v.icon))
        if picture then K.image(c,12,12,90,90,picture.atlas,picture.part,K.WHITE,1);x=116 end
      end
      K.text(c,x,10,view.cw-x-12,49,K.clip(v.name,42),29,K.TEXT)
      K.text(c,x,61,view.cw-x-12,40,v.vendor and ("€$ "..v.price) or tr("В СОБСТВЕННОСТИ","OWNED"),25,v.vendor and K.YELLOW or K.CYAN)
      local b=kit.button("p1",kit.at(view.x,it.y,view),4,6,view.cw-8,110,function()
        if busy then return end
        s.focus=v;s.confirm=nil;redraw(kit)
      end,function(h) bg:SetOpacity(h and .25 or selected and .19 or .06) end)
      b.key="glamdocItem:"..v.key
    end}
  end
  kit.list(p,"p1","glamdoc",X,kit.P1Y,265,815,rows,790,kit.S.keepScroll)
  if #rows==0 then K.text(p,35,310,770,120,tr("Нет доступных стилей для этой категории","No available styles in this category"),32,K.TEXT,nil,true) end
  local x=880;local focus=s.focus
  if focus then
    K.text(p,x,270,850,90,focus.name,34,K.CYAN,nil,true)
    local compatible=G.compatible(focus.raw)
    K.text(p,x,366,850,80,compatible and tr("Примерьте стиль и посмотрите на свои руки.","Preview the style on your own arms.")
      or tr("Для примерки и применения нужен другой имплант рук. Стиль можно купить заранее.",
        "Preview and apply require different arm cyberware. You can buy this style for later."),28,compatible and K.TEXT or K.YELLOW,nil,true)
    action(kit,p,x,460,410,focus.vendor and tr("ПРИМЕРИТЬ","PREVIEW") or tr("ВЫБРАТЬ","SELECT"),compatible and not busy,function()
      local ok=G.select(focus);if not ok then kit.toast(tr("Стиль сейчас недоступен","Style is currently unavailable")) end
      s.confirm=nil;redraw(kit)
    end,"glamdocPreview")
    if focus.vendor then
      local quote=G.quote(focus)
      action(kit,p,x+430,460,410,tr("КУПИТЬ","BUY").."  €$ "..focus.price,quote.allowed,function()
        s.confirm={item=focus,applyAfter=false};redraw(kit)
      end,"glamdocBuy")
    end
  else K.text(p,x,290,850,120,tr("Выберите стиль слева","Select a style on the left"),34,K.TEXT,nil,true) end
  if s.confirm then
    local confirmation=s.confirm;local quote=G.quote(confirmation.item)
    K.roundFill(p,x-10,555,870,430,K.NAVY,.95,12)
    K.text(p,x+20,580,810,90,tr("Подтвердить покупку: ","Confirm purchase: ")..confirmation.item.name,32,K.TEXT,nil,true)
    K.text(p,x+20,687,810,70,"€$ "..(quote.price or confirmation.item.price),40,K.YELLOW)
    if not quote.compatible then K.text(p,x+20,758,810,70,tr("Стиль будет куплен без установки","Style will be purchased without equipping"),28,K.YELLOW,nil,true) end
    action(kit,p,x+20,875,390,tr("НАЗАД","BACK"),true,function() s.confirm=nil;redraw(kit) end,"glamdocBuyCancel")
    action(kit,p,x+430,875,390,tr("ПОДТВЕРДИТЬ","CONFIRM"),quote.allowed,function()
      local ok=G.buy(confirmation.item,confirmation.applyAfter)
      if not ok then kit.toast(tr("Покупка недоступна: проверьте баланс и условия","Purchase unavailable: check funds and requirements")) end
      s.confirm=nil;redraw(kit)
    end,"glamdocBuyConfirm")
  else
    local colors=G.colors()
    if colors then
      K.text(p,x,556,850,50,s.preview and tr("ЦВЕТА ПРИМЕРКИ","PREVIEW COLORS") or tr("ЦВЕТА СТИЛЯ","STYLE COLORS"),30,K.CYAN)
      for channelIndex,channel in ipairs({"primary","secondary"}) do
        local mode=channel;local y=620+(channelIndex-1)*124
        K.text(p,x,y,220,70,channelIndex==1 and tr("Основной","Primary") or tr("Дополнительный","Secondary"),26,K.TEXT)
        for i,color in ipairs(colors) do
          local index=i-1;local bx=x+240+(i-1)*116
          K.roundFill(p,bx,y,94,74,color,1,8)
          if s[channel]==index then K.roundEdge(p,bx-4,y-4,102,82,K.WHITE,1,8) end
          local b=kit.button("p1",kit.at(X,kit.P1Y),bx,y,94,74,function()
            if G.color(mode,index) then redraw(kit) end
          end,nil);b.key="glamdocColor:"..mode..":"..index
        end
      end
      action(kit,p,x,885,410,tr("ПО УМОЛЧАНИЮ","DEFAULT COLORS"),not busy,function() G.resetColors();redraw(kit) end,"glamdocColorsDefault")
      action(kit,p,x+430,885,410,tr("ПОМЕНЯТЬ МЕСТАМИ","SWAP COLORS"),not busy,function() G.swap();redraw(kit) end,"glamdocColorsSwap")
    end
    if s.preview then
      action(kit,p,x,985,840,tr("ЗАКОНЧИТЬ ПРИМЕРКУ","CANCEL PREVIEW"),not busy,function() G.cancelPreview();redraw(kit) end,"glamdocPreviewCancel")
    elseif s.enabled then
      action(kit,p,x,985,840,tr("СНЯТЬ СТИЛЬ","REMOVE STYLE"),not busy,function() G.remove();redraw(kit) end,"glamdocRemove")
    end
  end
  if busy then K.text(p,30,1065,1700,42,tr("Покупка выполняется…","Purchase in progress…"),28,K.YELLOW)
  elseif s.error then K.text(p,30,1065,1700,42,tr("Покупка пока не подтверждена. Проверьте инвентарь и баланс.","Purchase not confirmed yet. Check your inventory and funds."),27,K.YELLOW) end
  action(kit,p,25,1120,825,tr("ОТМЕНА","CANCEL"),true,function() G.finish(false);kit.close() end,"glamdocCancel")
  action(kit,p,900,1120,825,s.preview and tr("КУПИТЬ И ПРИМЕНИТЬ","BUY AND APPLY") or tr("ПРИМЕНИТЬ","APPLY"),not busy,function()
    if s.preview then s.confirm={item=s.preview,applyAfter=true};redraw(kit)
    elseif G.finish(true) then kit.close()
    else kit.toast(tr("Для применения нужен совместимый имплант рук","Compatible arm cyberware is required to apply this style")) end
  end,"glamdocApply")
end
function V.update(kit)
  local s=G.current()
  if not s or s.closed then kit.close();return end
  if s.revision~=s.drawnRevision then redraw(kit) end
end
return V
