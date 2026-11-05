-- The first-launch setup, on the holographic menu's pages (modules/holo.lua) before the main menu: the hand that holds
-- the weapon, the T-pose that measures the body, the player's height (src/Natives/BodySetup.cpp). It comes up until it
-- has been through once (VRFirstSetupNeeded) and ends on the main menu.
--
-- THE T-POSE. Start gives the player three seconds to stand straight and stretch the arms out, then the plugin samples
-- four (VRCalibrationStart): which way is right, the eyes' height over the floor, the arms' span. A sample that never
-- saw the arms out to the sides fails and can be run again; the page follows the plugin's state every frame.
-- THE HEIGHT is offered from the eyes the T-pose measured (VRSuggestedHeight), a centimetre a press either way; the
-- arms and the shoulders are taken from it (Runtimes/BodyProportions.hpp).
local M = {}

local READY_SECONDS, SAMPLE_SECONDS = 3.0, 4.0
-- The pages' pictures: the two hands with their controllers, the T-pose (tools/vr_lobby_assets/make_setup_art.py).
local ART = "vrport\\ui\\vr_lobby_setup.inkatlas"

local TEXTS = {
  ["en-us"] = { title = "Setup", sub = "First launch", handTitle = "Weapon hand", handSub = "Which hand holds the weapon?",
    right = "Right hand", left = "Left hand", later = "You can change this later in the VR settings.",
    calTitle = "Calibration", calSub = "T-pose", cal1 = "Stand up straight and look straight ahead.",
    cal2 = "Stretch both arms out to the sides at shoulder height.", start = "Start", ready = "Get ready",
    hold = "Hold still", failed = "The arms were not seen out to the sides. Try again.", skip = "Skip",
    heightTitle = "Height", heightSub = "Your height", heightRow = "Height",
    height1 = "Your height without shoes: the length of the arms and the shoulders are taken from it.",
    confirm = "Confirm" },
  ["ru-ru"] = { title = "Настройка", sub = "Первый запуск", handTitle = "Рука для оружия",
    handSub = "В какой руке держать оружие?", right = "Правая рука", left = "Левая рука",
    later = "Это можно изменить позже в настройках VR.", calTitle = "Калибровка", calSub = "Т-поза",
    cal1 = "Встаньте прямо и смотрите вперёд.", cal2 = "Вытяните обе руки в стороны на уровне плеч.",
    start = "Начать", ready = "Приготовьтесь", hold = "Не двигайтесь",
    failed = "Руки, вытянутые в стороны, не обнаружены. Попробуйте ещё раз.", skip = "Пропустить",
    heightTitle = "Рост", heightSub = "Ваш рост", heightRow = "Рост",
    height1 = "Ваш рост без обуви: по нему рассчитываются длина рук и плечи.", confirm = "Подтвердить" },
  ["de-de"] = { title = "Einrichtung", sub = "Erster Start", handTitle = "Waffenhand",
    handSub = "In welcher Hand hältst du die Waffe?", right = "Rechte Hand", left = "Linke Hand",
    later = "Das lässt sich später in den VR-Einstellungen ändern.", calTitle = "Kalibrierung", calSub = "T-Pose",
    cal1 = "Stell dich gerade hin und schau geradeaus.", cal2 = "Streck beide Arme auf Schulterhöhe zur Seite.",
    start = "Start", ready = "Mach dich bereit", hold = "Stillhalten",
    failed = "Die zur Seite gestreckten Arme wurden nicht erkannt. Versuch es noch einmal.", skip = "Überspringen",
    heightTitle = "Körpergröße", heightSub = "Deine Körpergröße", heightRow = "Größe",
    height1 = "Deine Größe ohne Schuhe: Armlänge und Schultern werden daraus berechnet.", confirm = "Bestätigen" },
  ["fr-fr"] = { title = "Configuration", sub = "Premier lancement", handTitle = "Main de l'arme",
    handSub = "Dans quelle main tenez-vous l'arme ?", right = "Main droite", left = "Main gauche",
    later = "Modifiable plus tard dans les paramètres VR.", calTitle = "Calibrage", calSub = "Pose en T",
    cal1 = "Tenez-vous droit et regardez devant vous.", cal2 = "Tendez les deux bras sur les côtés, à hauteur d'épaule.",
    start = "Commencer", ready = "Préparez-vous", hold = "Ne bougez pas",
    failed = "Les bras tendus sur les côtés n'ont pas été détectés. Réessayez.", skip = "Passer",
    heightTitle = "Taille", heightSub = "Votre taille", heightRow = "Taille",
    height1 = "Votre taille sans chaussures : la longueur des bras et les épaules en sont déduites.",
    confirm = "Confirmer" },
  ["it-it"] = { title = "Configurazione", sub = "Primo avvio", handTitle = "Mano dell'arma",
    handSub = "In quale mano tieni l'arma?", right = "Mano destra", left = "Mano sinistra",
    later = "Puoi cambiarlo più tardi nelle impostazioni VR.", calTitle = "Calibrazione", calSub = "Posa a T",
    cal1 = "Stai dritto e guarda avanti.", cal2 = "Allarga entrambe le braccia ai lati all'altezza delle spalle.",
    start = "Inizia", ready = "Preparati", hold = "Resta fermo",
    failed = "Le braccia allargate ai lati non sono state rilevate. Riprova.", skip = "Salta",
    heightTitle = "Altezza", heightSub = "La tua altezza", heightRow = "Altezza",
    height1 = "La tua altezza senza scarpe: lunghezza delle braccia e spalle vengono calcolate da essa.",
    confirm = "Conferma" },
  ["es-es"] = { title = "Configuración", sub = "Primer inicio", handTitle = "Mano del arma",
    handSub = "¿En qué mano sostienes el arma?", right = "Mano derecha", left = "Mano izquierda",
    later = "Puedes cambiarlo más tarde en los ajustes de RV.", calTitle = "Calibración", calSub = "Pose en T",
    cal1 = "Ponte recto y mira al frente.", cal2 = "Extiende ambos brazos hacia los lados a la altura de los hombros.",
    start = "Empezar", ready = "Prepárate", hold = "No te muevas",
    failed = "No se han detectado los brazos extendidos a los lados. Inténtalo de nuevo.", skip = "Omitir",
    heightTitle = "Estatura", heightSub = "Tu estatura", heightRow = "Estatura",
    height1 = "Tu estatura sin calzado: de ella se calculan la longitud de los brazos y los hombros.",
    confirm = "Confirmar" },
  ["es-mx"] = { title = "Configuración", sub = "Primer inicio", handTitle = "Mano del arma",
    handSub = "¿En qué mano sostienes el arma?", right = "Mano derecha", left = "Mano izquierda",
    later = "Puedes cambiarlo después en la configuración de RV.", calTitle = "Calibración", calSub = "Pose en T",
    cal1 = "Párate derecho y mira al frente.", cal2 = "Extiende ambos brazos hacia los lados a la altura de los hombros.",
    start = "Comenzar", ready = "Prepárate", hold = "No te muevas",
    failed = "No se detectaron los brazos extendidos a los lados. Inténtalo de nuevo.", skip = "Omitir",
    heightTitle = "Estatura", heightSub = "Tu estatura", heightRow = "Estatura",
    height1 = "Tu estatura sin zapatos: con ella se calculan el largo de los brazos y los hombros.",
    confirm = "Confirmar" },
  ["pl-pl"] = { title = "Konfiguracja", sub = "Pierwsze uruchomienie", handTitle = "Ręka z bronią",
    handSub = "W której ręce trzymasz broń?", right = "Prawa ręka", left = "Lewa ręka",
    later = "Możesz to później zmienić w ustawieniach VR.", calTitle = "Kalibracja", calSub = "Pozycja T",
    cal1 = "Stań prosto i patrz przed siebie.", cal2 = "Wyciągnij obie ręce w bok na wysokości barków.",
    start = "Rozpocznij", ready = "Przygotuj się", hold = "Nie ruszaj się",
    failed = "Nie wykryto rąk wyciągniętych w bok. Spróbuj ponownie.", skip = "Pomiń",
    heightTitle = "Wzrost", heightSub = "Twój wzrost", heightRow = "Wzrost",
    height1 = "Twój wzrost bez butów: na jego podstawie obliczana jest długość rąk i barki.", confirm = "Potwierdź" },
  ["pt-br"] = { title = "Configuração", sub = "Primeira execução", handTitle = "Mão da arma",
    handSub = "Em qual mão você segura a arma?", right = "Mão direita", left = "Mão esquerda",
    later = "Você pode mudar isso depois nas configurações de RV.", calTitle = "Calibração", calSub = "Pose em T",
    cal1 = "Fique reto e olhe para a frente.", cal2 = "Estique os dois braços para os lados na altura dos ombros.",
    start = "Iniciar", ready = "Prepare-se", hold = "Fique parado",
    failed = "Os braços estendidos para os lados não foram detectados. Tente novamente.", skip = "Pular",
    heightTitle = "Altura", heightSub = "Sua altura", heightRow = "Altura",
    height1 = "Sua altura sem sapatos: o comprimento dos braços e os ombros são calculados a partir dela.",
    confirm = "Confirmar" },
  ["cz-cz"] = { title = "Nastavení", sub = "První spuštění", handTitle = "Ruka se zbraní",
    handSub = "Ve které ruce držíte zbraň?", right = "Pravá ruka", left = "Levá ruka",
    later = "Později to lze změnit v nastavení VR.", calTitle = "Kalibrace", calSub = "Póza T",
    cal1 = "Postavte se rovně a dívejte se před sebe.", cal2 = "Upažte obě ruce do stran ve výšce ramen.",
    start = "Začít", ready = "Připravte se", hold = "Nehýbejte se",
    failed = "Ruce upažené do stran nebyly zachyceny. Zkuste to znovu.", skip = "Přeskočit",
    heightTitle = "Výška", heightSub = "Vaše výška", heightRow = "Výška",
    height1 = "Vaše výška bez bot: podle ní se počítá délka paží a ramena.", confirm = "Potvrdit" },
  ["hu-hu"] = { title = "Beállítás", sub = "Első indítás", handTitle = "Fegyvertartó kéz",
    handSub = "Melyik kezedben tartod a fegyvert?", right = "Jobb kéz", left = "Bal kéz",
    later = "Ezt később a VR-beállításokban módosíthatod.", calTitle = "Kalibráció", calSub = "T-póz",
    cal1 = "Állj egyenesen, és nézz előre.", cal2 = "Nyújtsd ki mindkét karodat oldalra vállmagasságban.",
    start = "Indítás", ready = "Készülj", hold = "Ne mozdulj",
    failed = "Nem észleltük az oldalra kinyújtott karokat. Próbáld újra.", skip = "Kihagyás",
    heightTitle = "Testmagasság", heightSub = "A testmagasságod", heightRow = "Magasság",
    height1 = "A testmagasságod cipő nélkül: ebből számoljuk a karhosszt és a vállakat.", confirm = "Megerősítés" },
  ["tr-tr"] = { title = "Kurulum", sub = "İlk çalıştırma", handTitle = "Silah eli",
    handSub = "Silahı hangi elinde tutuyorsun?", right = "Sağ el", left = "Sol el",
    later = "Bunu daha sonra VR ayarlarından değiştirebilirsin.", calTitle = "Kalibrasyon", calSub = "T pozu",
    cal1 = "Dik dur ve ileriye bak.", cal2 = "İki kolunu da omuz hizasında yanlara aç.",
    start = "Başla", ready = "Hazırlan", hold = "Kıpırdama",
    failed = "Yanlara açılmış kollar algılanmadı. Tekrar dene.", skip = "Atla",
    heightTitle = "Boy", heightSub = "Boyun", heightRow = "Boy",
    height1 = "Ayakkabısız boyun: kol uzunluğu ve omuzlar buna göre hesaplanır.", confirm = "Onayla" },
  ["ua-ua"] = { title = "Налаштування", sub = "Перший запуск", handTitle = "Рука для зброї",
    handSub = "У якій руці тримати зброю?", right = "Права рука", left = "Ліва рука",
    later = "Це можна змінити пізніше в налаштуваннях VR.", calTitle = "Калібрування", calSub = "Т-поза",
    cal1 = "Станьте рівно й дивіться вперед.", cal2 = "Витягніть обидві руки в боки на рівні плечей.",
    start = "Почати", ready = "Приготуйтеся", hold = "Не рухайтеся",
    failed = "Руки, витягнуті в боки, не виявлено. Спробуйте ще раз.", skip = "Пропустити",
    heightTitle = "Зріст", heightSub = "Ваш зріст", heightRow = "Зріст",
    height1 = "Ваш зріст без взуття: за ним розраховуються довжина рук і плечі.", confirm = "Підтвердити" },
  ["jp-jp"] = { title = "セットアップ", sub = "初回起動", handTitle = "武器を持つ手",
    handSub = "武器をどちらの手で持ちますか？", right = "右手", left = "左手",
    later = "あとでVR設定から変更できます。", calTitle = "キャリブレーション", calSub = "Tポーズ",
    cal1 = "まっすぐ立って正面を見てください。", cal2 = "両腕を肩の高さで左右に伸ばしてください。",
    start = "開始", ready = "準備してください", hold = "そのまま動かないでください",
    failed = "左右に伸ばした腕を検出できませんでした。もう一度お試しください。", skip = "スキップ",
    heightTitle = "身長", heightSub = "あなたの身長", heightRow = "身長",
    height1 = "靴を履かない身長です。腕の長さと肩幅はここから計算されます。", confirm = "決定" },
  ["kr-kr"] = { title = "설정", sub = "첫 실행", handTitle = "무기를 든 손",
    handSub = "어느 손으로 무기를 드시겠습니까?", right = "오른손", left = "왼손",
    later = "나중에 VR 설정에서 변경할 수 있습니다.", calTitle = "보정", calSub = "T 포즈",
    cal1 = "똑바로 서서 정면을 바라보세요.", cal2 = "양팔을 어깨 높이에서 옆으로 뻗으세요.",
    start = "시작", ready = "준비하세요", hold = "움직이지 마세요",
    failed = "옆으로 뻗은 팔이 감지되지 않았습니다. 다시 시도하세요.", skip = "건너뛰기",
    heightTitle = "키", heightSub = "내 키", heightRow = "키",
    height1 = "신발을 신지 않은 키입니다. 팔 길이와 어깨는 이 값으로 계산됩니다.", confirm = "확인" },
  ["zh-cn"] = { title = "设置", sub = "首次启动", handTitle = "持枪手", handSub = "用哪只手持武器？",
    right = "右手", left = "左手", later = "之后可以在 VR 设置中更改。", calTitle = "校准", calSub = "T 字姿势",
    cal1 = "站直并直视前方。", cal2 = "双臂在肩膀高度向两侧伸直。", start = "开始", ready = "准备",
    hold = "保持不动", failed = "未检测到向两侧伸展的手臂。请重试。", skip = "跳过",
    heightTitle = "身高", heightSub = "你的身高", heightRow = "身高",
    height1 = "不穿鞋时的身高：手臂长度和肩宽据此计算。", confirm = "确认" },
  ["zh-tw"] = { title = "設定", sub = "首次啟動", handTitle = "持槍手", handSub = "用哪隻手持武器？",
    right = "右手", left = "左手", later = "之後可以在 VR 設定中變更。", calTitle = "校準", calSub = "T 字姿勢",
    cal1 = "站直並直視前方。", cal2 = "雙臂在肩膀高度向兩側伸直。", start = "開始", ready = "準備",
    hold = "保持不動", failed = "未偵測到向兩側伸展的手臂。請再試一次。", skip = "略過",
    heightTitle = "身高", heightSub = "你的身高", heightRow = "身高",
    height1 = "不穿鞋時的身高：手臂長度與肩寬據此計算。", confirm = "確認" },
  ["th-th"] = { title = "การตั้งค่า", sub = "เปิดครั้งแรก", handTitle = "มือที่ถืออาวุธ",
    handSub = "คุณถืออาวุธด้วยมือข้างไหน?", right = "มือขวา", left = "มือซ้าย",
    later = "เปลี่ยนได้ภายหลังในการตั้งค่า VR", calTitle = "การปรับเทียบ", calSub = "ท่า T",
    cal1 = "ยืนตัวตรงและมองไปข้างหน้า", cal2 = "กางแขนทั้งสองข้างออกด้านข้างระดับไหล่",
    start = "เริ่ม", ready = "เตรียมตัว", hold = "อยู่นิ่ง ๆ",
    failed = "ไม่พบแขนที่กางออกด้านข้าง ลองอีกครั้ง", skip = "ข้าม",
    heightTitle = "ส่วนสูง", heightSub = "ส่วนสูงของคุณ", heightRow = "ส่วนสูง",
    height1 = "ส่วนสูงโดยไม่สวมรองเท้า ใช้คำนวณความยาวแขนและไหล่", confirm = "ยืนยัน" },
  ["ar-ar"] = { title = "الإعداد", sub = "التشغيل الأول", handTitle = "يد السلاح", handSub = "بأي يد تحمل السلاح؟",
    right = "اليد اليمنى", left = "اليد اليسرى", later = "يمكنك تغيير ذلك لاحقًا من إعدادات الواقع الافتراضي.",
    calTitle = "المعايرة", calSub = "وضعية T", cal1 = "قف مستقيمًا وانظر إلى الأمام.",
    cal2 = "مُدّ ذراعيك إلى الجانبين بمستوى الكتفين.", start = "ابدأ", ready = "استعد", hold = "اثبت مكانك",
    failed = "لم يتم رصد الذراعين ممدودتين إلى الجانبين. حاول مرة أخرى.", skip = "تخطي",
    heightTitle = "الطول", heightSub = "طولك", heightRow = "الطول",
    height1 = "طولك من دون حذاء: يُحسب منه طول الذراعين والكتفان.", confirm = "تأكيد" },
}

local S = { phase = "idle", clock = 0.0, height = 1.75, shown = nil, finish = nil, Holo = nil }

-- A plugin native, or nil when it is missing (an older plugin) or fails.
local function native(fn, ...)
  if type(fn) ~= "function" then return nil end
  local ok, v = pcall(fn, ...)
  if ok then return v end
  return nil
end

local function language()
  local lang = "en-us"
  pcall(function()
    local ss = Game.GetSettingsSystem()
    if ss:HasGroup("/language") and ss:GetGroup("/language"):HasVar(CName.new("OnScreen")) then
      lang = NameToString(ss:GetVar("/language", CName.new("OnScreen")):GetValue())
    end
  end)
  return lang
end

local function texts() return TEXTS[language()] or TEXTS["en-us"] end
M.texts = texts

-- 1.75 m · 5' 9"
local function heightText(h)
  local inches = math.floor(h / 0.0254 + 0.5)
  return string.format("%.2f m · %d' %d\"", h, math.floor(inches / 12), inches % 12)
end

function M.needed() return native(VRFirstSetupNeeded) == true end

local function heightPage(T, Holo)
  return {
    title = T.heightTitle, subtitle = T.heightSub, code = "SETUP//BODY",
    rows = function()
      local step = function(d)
        S.height = math.max(1.40, math.min(2.20, math.floor((S.height + d) * 100 + 0.5) / 100))
        Holo.refresh()
      end
      return {
        { key = "h1", text = true, label = T.height1 },
        { key = "height", label = T.heightRow, value = heightText(S.height),
          left = function() step(-0.01) end, right = function() step(0.01) end },
        { key = "confirm", label = T.confirm, go = function()
            native(VRSetPlayerHeight, S.height)
            if S.finish ~= nil then S.finish() end
          end },
      }
    end,
  }
end

local function calibrationPage(T, Holo)
  return {
    title = T.calTitle, subtitle = T.calSub, code = "SETUP//T-POSE",
    rows = function()
      local rows = {
        { key = "c1", text = true, label = T.cal1 },
        { key = "c2", text = true, label = T.cal2 },
        { key = "pose", picture = { ART, "tpose" } },
      }
      if S.phase == "ready" then
        rows[#rows + 1] = { key = "state", label = T.ready, detail = tostring(math.ceil(READY_SECONDS - S.clock)),
                            enabled = false }
      elseif S.phase == "sampling" then
        local p = native(VRCalibrationProgress) or 0
        rows[#rows + 1] = { key = "state", label = T.hold, detail = string.format("%d%%", math.floor(p * 100)),
                            enabled = false }
      else
        if S.phase == "failed" then rows[#rows + 1] = { key = "failed", text = true, warn = true, label = T.failed } end
        rows[#rows + 1] = { key = "start", label = T.start, go = function()
            S.phase, S.clock = "ready", 0.0
            Holo.refresh()
          end }
        rows[#rows + 1] = { key = "skip", label = T.skip, go = function()
            S.phase = "idle"
            S.height = native(VRSuggestedHeight) or 1.75
            Holo.push(heightPage(T, Holo))
          end }
      end
      return rows
    end,
    onBack = function() if S.phase == "ready" then S.phase = "idle" end end,
  }
end

local function handPage(T, Holo)
  local choose = function(left)
    native(VRSetLeftHanded, left)
    S.phase = "idle"
    Holo.push(calibrationPage(T, Holo))
  end
  return {
    title = T.handTitle, subtitle = T.handSub, code = "SETUP//HAND",
    rows = function()
      local left = native(VRLeftHanded) == true
      -- the hands as the player sees them: the left on the left, the right on the right
      return {
        { key = "hands", cards = {
            { key = "left", picture = { ART, "hand_left" }, label = T.left, on = left, go = function() choose(true) end },
            { key = "right", picture = { ART, "hand_right" }, label = T.right, on = not left,
              go = function() choose(false) end } } },
        { key = "later", text = true, label = T.later },
      }
    end,
  }
end

-- Holo: modules/holo.lua; finish: what comes after the last page (the main menu).
function M.root(Holo, finish)
  S.Holo, S.finish, S.phase = Holo, finish, "idle"
  return handPage(texts(), Holo)
end

-- Every frame while the lobby is up: the T-pose's countdown and sample, the page following them.
function M.update(dt)
  local Holo = S.Holo
  if Holo == nil or not Holo.isOpen() then return end
  local shown
  if S.phase == "ready" then
    S.clock = S.clock + (dt or 0)
    if S.clock >= READY_SECONDS then
      native(VRCalibrationStart, SAMPLE_SECONDS)
      S.phase = "sampling"
    end
    shown = "ready" .. math.ceil(READY_SECONDS - S.clock)
  elseif S.phase == "sampling" then
    local state = native(VRCalibrationState) or 0
    if state == 2 then
      S.phase = "idle"
      S.height = native(VRSuggestedHeight) or 1.75
      Holo.push(heightPage(texts(), Holo))
    elseif state == 3 or state == 0 then
      S.phase = "failed"
    end
    shown = S.phase .. math.floor((native(VRCalibrationProgress) or 0) * 20)
  else
    shown = S.phase
  end
  if shown ~= S.shown then
    S.shown = shown
    Holo.refresh()
  end
end

return M
