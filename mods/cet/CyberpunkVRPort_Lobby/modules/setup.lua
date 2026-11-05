-- The first-launch setup, on the holographic menu's pages (modules/holo.lua) before the main menu: the hand that holds
-- the weapon (left-handed, then its bindings), the body drawn whole or as its hands alone, the T-pose that measures the
-- body, the player's height (src/Natives/BodySetup.cpp). It comes up until it has been through once
-- (VRFirstSetupNeeded) and ends on the main menu.
--
-- THE T-POSE. Start gives the player three seconds to stand straight and stretch the arms out, then the plugin samples
-- four (VRCalibrationStart): which way is right, the eyes' height over the floor, the arms' span. A sample that never
-- saw the arms out to the sides fails and can be run again; the page follows the plugin's state every frame.
-- THE HEIGHT is offered from the eyes the T-pose measured (VRSuggestedHeight), a centimetre a press either way; the
-- arms and the shoulders are taken from it (Runtimes/BodyProportions.hpp).
local M = {}

local READY_SECONDS, SAMPLE_SECONDS = 3.0, 4.0
-- The pages' pictures: the two hands with their controllers, the whole body and the hands alone, the T-pose
-- (tools/vr_lobby_assets/make_setup_art.py).
local ART = "vrport\\ui\\vr_lobby_setup.inkatlas"

local TEXTS = {
  ["en-us"] = { title = "Setup", sub = "First launch", handTitle = "Weapon hand", handSub = "Which hand holds the weapon?",
    right = "Right hand", left = "Left hand", later = "You can change this later in the VR settings.",
    calTitle = "Calibration", calSub = "T-pose", cal1 = "Stand up straight and look straight ahead.",
    cal2 = "Stretch both arms out to the sides at shoulder height.", start = "Start", ready = "Get ready",
    hold = "Hold still", failed = "The arms were not seen out to the sides. Try again.", skip = "Skip",
    heightTitle = "Height", heightSub = "Your height", heightRow = "Height",
    height1 = "Your height without shoes: the length of the arms and the shoulders are taken from it.",
    confirm = "Confirm",
    bodyTitle = "Body", bodySub = "Show the whole body or only the hands?", bodyFull = "Full body", bodyHands = "Hands only",
    bindTitle = "Controls", bindSub = "Which bindings for the left hand?", bindSame = "Same as right-handed",
    bindWalk = "Left stick: walk", bindTurn = "Left stick: turn",
    bindNote = "The left trigger fires the weapon. With the left stick options the buttons are mirrored." },
  ["ru-ru"] = { title = "Настройка", sub = "Первый запуск", handTitle = "Рука для оружия",
    handSub = "В какой руке держать оружие?", right = "Правая рука", left = "Левая рука",
    later = "Это можно изменить позже в настройках VR.", calTitle = "Калибровка", calSub = "Т-поза",
    cal1 = "Встаньте прямо и смотрите вперёд.", cal2 = "Вытяните обе руки в стороны на уровне плеч.",
    start = "Начать", ready = "Приготовьтесь", hold = "Не двигайтесь",
    failed = "Руки, вытянутые в стороны, не обнаружены. Попробуйте ещё раз.", skip = "Пропустить",
    heightTitle = "Рост", heightSub = "Ваш рост", heightRow = "Рост",
    height1 = "Ваш рост без обуви: по нему рассчитываются длина рук и плечи.", confirm = "Подтвердить",
    bodyTitle = "Тело", bodySub = "Показывать всё тело или только руки?", bodyFull = "Аватар", bodyHands = "Только руки",
    bindTitle = "Управление", bindSub = "Какие биндинги для левой руки?", bindSame = "Как у правой руки",
    bindWalk = "Левый стик: ходьба", bindTurn = "Левый стик: поворот",
    bindNote = "Стреляет левый курок. В вариантах с левым стиком кнопки отзеркалены." },
  ["de-de"] = { title = "Einrichtung", sub = "Erster Start", handTitle = "Waffenhand",
    handSub = "In welcher Hand hältst du die Waffe?", right = "Rechte Hand", left = "Linke Hand",
    later = "Das lässt sich später in den VR-Einstellungen ändern.", calTitle = "Kalibrierung", calSub = "T-Pose",
    cal1 = "Stell dich gerade hin und schau geradeaus.", cal2 = "Streck beide Arme auf Schulterhöhe zur Seite.",
    start = "Start", ready = "Mach dich bereit", hold = "Stillhalten",
    failed = "Die zur Seite gestreckten Arme wurden nicht erkannt. Versuch es noch einmal.", skip = "Überspringen",
    heightTitle = "Körpergröße", heightSub = "Deine Körpergröße", heightRow = "Größe",
    height1 = "Deine Größe ohne Schuhe: Armlänge und Schultern werden daraus berechnet.", confirm = "Bestätigen",
    bodyTitle = "Körper", bodySub = "Den ganzen Körper zeigen oder nur die Hände?", bodyFull = "Ganzer Körper", bodyHands = "Nur Hände",
    bindTitle = "Steuerung", bindSub = "Welche Belegung für die linke Hand?", bindSame = "Wie Rechtshänder",
    bindWalk = "Linker Stick: Gehen", bindTurn = "Linker Stick: Drehen",
    bindNote = "Der linke Abzug feuert. Bei den Optionen mit dem linken Stick sind die Tasten gespiegelt." },
  ["fr-fr"] = { title = "Configuration", sub = "Premier lancement", handTitle = "Main de l'arme",
    handSub = "Dans quelle main tenez-vous l'arme ?", right = "Main droite", left = "Main gauche",
    later = "Modifiable plus tard dans les paramètres VR.", calTitle = "Calibrage", calSub = "Pose en T",
    cal1 = "Tenez-vous droit et regardez devant vous.", cal2 = "Tendez les deux bras sur les côtés, à hauteur d'épaule.",
    start = "Commencer", ready = "Préparez-vous", hold = "Ne bougez pas",
    failed = "Les bras tendus sur les côtés n'ont pas été détectés. Réessayez.", skip = "Passer",
    heightTitle = "Taille", heightSub = "Votre taille", heightRow = "Taille",
    height1 = "Votre taille sans chaussures : la longueur des bras et les épaules en sont déduites.",
    confirm = "Confirmer",
    bodyTitle = "Corps", bodySub = "Afficher tout le corps ou seulement les mains ?", bodyFull = "Corps entier", bodyHands = "Mains seules",
    bindTitle = "Commandes", bindSub = "Quelles commandes pour la main gauche ?", bindSame = "Comme droitier",
    bindWalk = "Stick gauche : marcher", bindTurn = "Stick gauche : tourner",
    bindNote = "La gâchette gauche tire. Avec les options du stick gauche, les boutons sont inversés." },
  ["it-it"] = { title = "Configurazione", sub = "Primo avvio", handTitle = "Mano dell'arma",
    handSub = "In quale mano tieni l'arma?", right = "Mano destra", left = "Mano sinistra",
    later = "Puoi cambiarlo più tardi nelle impostazioni VR.", calTitle = "Calibrazione", calSub = "Posa a T",
    cal1 = "Stai dritto e guarda avanti.", cal2 = "Allarga entrambe le braccia ai lati all'altezza delle spalle.",
    start = "Inizia", ready = "Preparati", hold = "Resta fermo",
    failed = "Le braccia allargate ai lati non sono state rilevate. Riprova.", skip = "Salta",
    heightTitle = "Altezza", heightSub = "La tua altezza", heightRow = "Altezza",
    height1 = "La tua altezza senza scarpe: lunghezza delle braccia e spalle vengono calcolate da essa.",
    confirm = "Conferma",
    bodyTitle = "Corpo", bodySub = "Mostrare tutto il corpo o solo le mani?", bodyFull = "Corpo intero", bodyHands = "Solo mani",
    bindTitle = "Comandi", bindSub = "Quali comandi per la mano sinistra?", bindSame = "Come per destrimani",
    bindWalk = "Stick sinistro: cammina", bindTurn = "Stick sinistro: gira",
    bindNote = "Il grilletto sinistro spara. Con le opzioni dello stick sinistro i pulsanti sono specchiati." },
  ["es-es"] = { title = "Configuración", sub = "Primer inicio", handTitle = "Mano del arma",
    handSub = "¿En qué mano sostienes el arma?", right = "Mano derecha", left = "Mano izquierda",
    later = "Puedes cambiarlo más tarde en los ajustes de RV.", calTitle = "Calibración", calSub = "Pose en T",
    cal1 = "Ponte recto y mira al frente.", cal2 = "Extiende ambos brazos hacia los lados a la altura de los hombros.",
    start = "Empezar", ready = "Prepárate", hold = "No te muevas",
    failed = "No se han detectado los brazos extendidos a los lados. Inténtalo de nuevo.", skip = "Omitir",
    heightTitle = "Estatura", heightSub = "Tu estatura", heightRow = "Estatura",
    height1 = "Tu estatura sin calzado: de ella se calculan la longitud de los brazos y los hombros.",
    confirm = "Confirmar",
    bodyTitle = "Cuerpo", bodySub = "¿Mostrar todo el cuerpo o solo las manos?", bodyFull = "Cuerpo completo", bodyHands = "Solo manos",
    bindTitle = "Controles", bindSub = "¿Qué controles para la mano izquierda?", bindSame = "Como diestro",
    bindWalk = "Stick izquierdo: andar", bindTurn = "Stick izquierdo: girar",
    bindNote = "El gatillo izquierdo dispara. Con las opciones del stick izquierdo los botones se invierten." },
  ["es-mx"] = { title = "Configuración", sub = "Primer inicio", handTitle = "Mano del arma",
    handSub = "¿En qué mano sostienes el arma?", right = "Mano derecha", left = "Mano izquierda",
    later = "Puedes cambiarlo después en la configuración de RV.", calTitle = "Calibración", calSub = "Pose en T",
    cal1 = "Párate derecho y mira al frente.", cal2 = "Extiende ambos brazos hacia los lados a la altura de los hombros.",
    start = "Comenzar", ready = "Prepárate", hold = "No te muevas",
    failed = "No se detectaron los brazos extendidos a los lados. Inténtalo de nuevo.", skip = "Omitir",
    heightTitle = "Estatura", heightSub = "Tu estatura", heightRow = "Estatura",
    height1 = "Tu estatura sin zapatos: con ella se calculan el largo de los brazos y los hombros.",
    confirm = "Confirmar",
    bodyTitle = "Cuerpo", bodySub = "¿Mostrar todo el cuerpo o solo las manos?", bodyFull = "Cuerpo completo", bodyHands = "Solo manos",
    bindTitle = "Controles", bindSub = "¿Qué controles para la mano izquierda?", bindSame = "Como diestro",
    bindWalk = "Stick izquierdo: caminar", bindTurn = "Stick izquierdo: girar",
    bindNote = "El gatillo izquierdo dispara. Con las opciones del stick izquierdo los botones se invierten." },
  ["pl-pl"] = { title = "Konfiguracja", sub = "Pierwsze uruchomienie", handTitle = "Ręka z bronią",
    handSub = "W której ręce trzymasz broń?", right = "Prawa ręka", left = "Lewa ręka",
    later = "Możesz to później zmienić w ustawieniach VR.", calTitle = "Kalibracja", calSub = "Pozycja T",
    cal1 = "Stań prosto i patrz przed siebie.", cal2 = "Wyciągnij obie ręce w bok na wysokości barków.",
    start = "Rozpocznij", ready = "Przygotuj się", hold = "Nie ruszaj się",
    failed = "Nie wykryto rąk wyciągniętych w bok. Spróbuj ponownie.", skip = "Pomiń",
    heightTitle = "Wzrost", heightSub = "Twój wzrost", heightRow = "Wzrost",
    height1 = "Twój wzrost bez butów: na jego podstawie obliczana jest długość rąk i barki.", confirm = "Potwierdź",
    bodyTitle = "Ciało", bodySub = "Pokazywać całe ciało czy tylko dłonie?", bodyFull = "Całe ciało", bodyHands = "Tylko dłonie",
    bindTitle = "Sterowanie", bindSub = "Jakie przypisanie dla lewej ręki?", bindSame = "Jak dla praworęcznych",
    bindWalk = "Lewa gałka: chodzenie", bindTurn = "Lewa gałka: obrót",
    bindNote = "Lewy spust strzela. W opcjach z lewą gałką przyciski są odwrócone." },
  ["pt-br"] = { title = "Configuração", sub = "Primeira execução", handTitle = "Mão da arma",
    handSub = "Em qual mão você segura a arma?", right = "Mão direita", left = "Mão esquerda",
    later = "Você pode mudar isso depois nas configurações de RV.", calTitle = "Calibração", calSub = "Pose em T",
    cal1 = "Fique reto e olhe para a frente.", cal2 = "Estique os dois braços para os lados na altura dos ombros.",
    start = "Iniciar", ready = "Prepare-se", hold = "Fique parado",
    failed = "Os braços estendidos para os lados não foram detectados. Tente novamente.", skip = "Pular",
    heightTitle = "Altura", heightSub = "Sua altura", heightRow = "Altura",
    height1 = "Sua altura sem sapatos: o comprimento dos braços e os ombros são calculados a partir dela.",
    confirm = "Confirmar",
    bodyTitle = "Corpo", bodySub = "Mostrar o corpo inteiro ou só as mãos?", bodyFull = "Corpo inteiro", bodyHands = "Só as mãos",
    bindTitle = "Controles", bindSub = "Quais controles para a mão esquerda?", bindSame = "Igual ao destro",
    bindWalk = "Analógico esquerdo: andar", bindTurn = "Analógico esquerdo: girar",
    bindNote = "O gatilho esquerdo atira. Nas opções do analógico esquerdo os botões são espelhados." },
  ["cz-cz"] = { title = "Nastavení", sub = "První spuštění", handTitle = "Ruka se zbraní",
    handSub = "Ve které ruce držíte zbraň?", right = "Pravá ruka", left = "Levá ruka",
    later = "Později to lze změnit v nastavení VR.", calTitle = "Kalibrace", calSub = "Póza T",
    cal1 = "Postavte se rovně a dívejte se před sebe.", cal2 = "Upažte obě ruce do stran ve výšce ramen.",
    start = "Začít", ready = "Připravte se", hold = "Nehýbejte se",
    failed = "Ruce upažené do stran nebyly zachyceny. Zkuste to znovu.", skip = "Přeskočit",
    heightTitle = "Výška", heightSub = "Vaše výška", heightRow = "Výška",
    height1 = "Vaše výška bez bot: podle ní se počítá délka paží a ramena.", confirm = "Potvrdit",
    bodyTitle = "Tělo", bodySub = "Zobrazit celé tělo, nebo jen ruce?", bodyFull = "Celé tělo", bodyHands = "Jen ruce",
    bindTitle = "Ovládání", bindSub = "Jaké ovládání pro levou ruku?", bindSame = "Jako pro praváky",
    bindWalk = "Levá páčka: chůze", bindTurn = "Levá páčka: otáčení",
    bindNote = "Levá spoušť střílí. U možností s levou páčkou jsou tlačítka zrcadlena." },
  ["hu-hu"] = { title = "Beállítás", sub = "Első indítás", handTitle = "Fegyvertartó kéz",
    handSub = "Melyik kezedben tartod a fegyvert?", right = "Jobb kéz", left = "Bal kéz",
    later = "Ezt később a VR-beállításokban módosíthatod.", calTitle = "Kalibráció", calSub = "T-póz",
    cal1 = "Állj egyenesen, és nézz előre.", cal2 = "Nyújtsd ki mindkét karodat oldalra vállmagasságban.",
    start = "Indítás", ready = "Készülj", hold = "Ne mozdulj",
    failed = "Nem észleltük az oldalra kinyújtott karokat. Próbáld újra.", skip = "Kihagyás",
    heightTitle = "Testmagasság", heightSub = "A testmagasságod", heightRow = "Magasság",
    height1 = "A testmagasságod cipő nélkül: ebből számoljuk a karhosszt és a vállakat.", confirm = "Megerősítés",
    bodyTitle = "Test", bodySub = "Az egész test látszódjon, vagy csak a kezek?", bodyFull = "Teljes test", bodyHands = "Csak a kezek",
    bindTitle = "Irányítás", bindSub = "Milyen kiosztás legyen a bal kézhez?", bindSame = "Mint jobbkezesnél",
    bindWalk = "Bal analóg: járás", bindTurn = "Bal analóg: fordulás",
    bindNote = "A bal ravasz lő. A bal analóg kar lehetőségeinél a gombok tükrözve vannak." },
  ["tr-tr"] = { title = "Kurulum", sub = "İlk çalıştırma", handTitle = "Silah eli",
    handSub = "Silahı hangi elinde tutuyorsun?", right = "Sağ el", left = "Sol el",
    later = "Bunu daha sonra VR ayarlarından değiştirebilirsin.", calTitle = "Kalibrasyon", calSub = "T pozu",
    cal1 = "Dik dur ve ileriye bak.", cal2 = "İki kolunu da omuz hizasında yanlara aç.",
    start = "Başla", ready = "Hazırlan", hold = "Kıpırdama",
    failed = "Yanlara açılmış kollar algılanmadı. Tekrar dene.", skip = "Atla",
    heightTitle = "Boy", heightSub = "Boyun", heightRow = "Boy",
    height1 = "Ayakkabısız boyun: kol uzunluğu ve omuzlar buna göre hesaplanır.", confirm = "Onayla",
    bodyTitle = "Vücut", bodySub = "Tüm vücut mu görünsün, yalnızca eller mi?", bodyFull = "Tüm vücut", bodyHands = "Yalnızca eller",
    bindTitle = "Kontroller", bindSub = "Sol el için hangi tuş düzeni?", bindSame = "Sağ elle aynı",
    bindWalk = "Sol çubuk: yürüme", bindTurn = "Sol çubuk: dönme",
    bindNote = "Sol tetik ateş eder. Sol çubuk seçeneklerinde tuşlar aynalanır." },
  ["ua-ua"] = { title = "Налаштування", sub = "Перший запуск", handTitle = "Рука для зброї",
    handSub = "У якій руці тримати зброю?", right = "Права рука", left = "Ліва рука",
    later = "Це можна змінити пізніше в налаштуваннях VR.", calTitle = "Калібрування", calSub = "Т-поза",
    cal1 = "Станьте рівно й дивіться вперед.", cal2 = "Витягніть обидві руки в боки на рівні плечей.",
    start = "Почати", ready = "Приготуйтеся", hold = "Не рухайтеся",
    failed = "Руки, витягнуті в боки, не виявлено. Спробуйте ще раз.", skip = "Пропустити",
    heightTitle = "Зріст", heightSub = "Ваш зріст", heightRow = "Зріст",
    height1 = "Ваш зріст без взуття: за ним розраховуються довжина рук і плечі.", confirm = "Підтвердити",
    bodyTitle = "Тіло", bodySub = "Показувати все тіло чи лише руки?", bodyFull = "Аватар", bodyHands = "Лише руки",
    bindTitle = "Керування", bindSub = "Які прив’язки для лівої руки?", bindSame = "Як для правої руки",
    bindWalk = "Лівий стік: ходьба", bindTurn = "Лівий стік: поворот",
    bindNote = "Стріляє лівий курок. У варіантах з лівим стіком кнопки віддзеркалено." },
  ["jp-jp"] = { title = "セットアップ", sub = "初回起動", handTitle = "武器を持つ手",
    handSub = "武器をどちらの手で持ちますか？", right = "右手", left = "左手",
    later = "あとでVR設定から変更できます。", calTitle = "キャリブレーション", calSub = "Tポーズ",
    cal1 = "まっすぐ立って正面を見てください。", cal2 = "両腕を肩の高さで左右に伸ばしてください。",
    start = "開始", ready = "準備してください", hold = "そのまま動かないでください",
    failed = "左右に伸ばした腕を検出できませんでした。もう一度お試しください。", skip = "スキップ",
    heightTitle = "身長", heightSub = "あなたの身長", heightRow = "身長",
    height1 = "靴を履かない身長です。腕の長さと肩幅はここから計算されます。", confirm = "決定",
    bodyTitle = "体", bodySub = "全身を表示しますか、それとも手だけですか？", bodyFull = "全身", bodyHands = "手のみ",
    bindTitle = "操作", bindSub = "左手の操作設定は？", bindSame = "右利きと同じ",
    bindWalk = "左スティック：移動", bindTurn = "左スティック：旋回",
    bindNote = "左トリガーで射撃します。左スティックの設定ではボタンが左右反転します。" },
  ["kr-kr"] = { title = "설정", sub = "첫 실행", handTitle = "무기를 든 손",
    handSub = "어느 손으로 무기를 드시겠습니까?", right = "오른손", left = "왼손",
    later = "나중에 VR 설정에서 변경할 수 있습니다.", calTitle = "보정", calSub = "T 포즈",
    cal1 = "똑바로 서서 정면을 바라보세요.", cal2 = "양팔을 어깨 높이에서 옆으로 뻗으세요.",
    start = "시작", ready = "준비하세요", hold = "움직이지 마세요",
    failed = "옆으로 뻗은 팔이 감지되지 않았습니다. 다시 시도하세요.", skip = "건너뛰기",
    heightTitle = "키", heightSub = "내 키", heightRow = "키",
    height1 = "신발을 신지 않은 키입니다. 팔 길이와 어깨는 이 값으로 계산됩니다.", confirm = "확인",
    bodyTitle = "몸", bodySub = "몸 전체를 표시할까요, 손만 표시할까요?", bodyFull = "전신", bodyHands = "손만",
    bindTitle = "조작", bindSub = "왼손 조작 방식을 선택하세요", bindSame = "오른손잡이와 동일",
    bindWalk = "왼쪽 스틱: 이동", bindTurn = "왼쪽 스틱: 회전",
    bindNote = "왼쪽 트리거로 사격합니다. 왼쪽 스틱 옵션에서는 버튼이 좌우 반전됩니다." },
  ["zh-cn"] = { title = "设置", sub = "首次启动", handTitle = "持枪手", handSub = "用哪只手持武器？",
    right = "右手", left = "左手", later = "之后可以在 VR 设置中更改。", calTitle = "校准", calSub = "T 字姿势",
    cal1 = "站直并直视前方。", cal2 = "双臂在肩膀高度向两侧伸直。", start = "开始", ready = "准备",
    hold = "保持不动", failed = "未检测到向两侧伸展的手臂。请重试。", skip = "跳过",
    heightTitle = "身高", heightSub = "你的身高", heightRow = "身高",
    height1 = "不穿鞋时的身高：手臂长度和肩宽据此计算。", confirm = "确认",
    bodyTitle = "身体", bodySub = "显示整个身体还是只显示双手？", bodyFull = "完整身体", bodyHands = "仅双手",
    bindTitle = "操作", bindSub = "左手使用哪种按键设置？", bindSame = "与右手相同",
    bindWalk = "左摇杆：移动", bindTurn = "左摇杆：转向",
    bindNote = "左扳机射击。左摇杆选项中按键左右互换。" },
  ["zh-tw"] = { title = "設定", sub = "首次啟動", handTitle = "持槍手", handSub = "用哪隻手持武器？",
    right = "右手", left = "左手", later = "之後可以在 VR 設定中變更。", calTitle = "校準", calSub = "T 字姿勢",
    cal1 = "站直並直視前方。", cal2 = "雙臂在肩膀高度向兩側伸直。", start = "開始", ready = "準備",
    hold = "保持不動", failed = "未偵測到向兩側伸展的手臂。請再試一次。", skip = "略過",
    heightTitle = "身高", heightSub = "你的身高", heightRow = "身高",
    height1 = "不穿鞋時的身高：手臂長度與肩寬據此計算。", confirm = "確認",
    bodyTitle = "身體", bodySub = "顯示整個身體還是只顯示雙手？", bodyFull = "完整身體", bodyHands = "僅雙手",
    bindTitle = "操作", bindSub = "左手使用哪種按鍵設定？", bindSame = "與右手相同",
    bindWalk = "左搖桿：移動", bindTurn = "左搖桿：轉向",
    bindNote = "左扳機射擊。左搖桿選項中按鍵左右互換。" },
  ["th-th"] = { title = "การตั้งค่า", sub = "เปิดครั้งแรก", handTitle = "มือที่ถืออาวุธ",
    handSub = "คุณถืออาวุธด้วยมือข้างไหน?", right = "มือขวา", left = "มือซ้าย",
    later = "เปลี่ยนได้ภายหลังในการตั้งค่า VR", calTitle = "การปรับเทียบ", calSub = "ท่า T",
    cal1 = "ยืนตัวตรงและมองไปข้างหน้า", cal2 = "กางแขนทั้งสองข้างออกด้านข้างระดับไหล่",
    start = "เริ่ม", ready = "เตรียมตัว", hold = "อยู่นิ่ง ๆ",
    failed = "ไม่พบแขนที่กางออกด้านข้าง ลองอีกครั้ง", skip = "ข้าม",
    heightTitle = "ส่วนสูง", heightSub = "ส่วนสูงของคุณ", heightRow = "ส่วนสูง",
    height1 = "ส่วนสูงโดยไม่สวมรองเท้า ใช้คำนวณความยาวแขนและไหล่", confirm = "ยืนยัน",
    bodyTitle = "ร่างกาย", bodySub = "แสดงทั้งตัวหรือเฉพาะมือ?", bodyFull = "ทั้งตัว", bodyHands = "เฉพาะมือ",
    bindTitle = "การควบคุม", bindSub = "ใช้การตั้งปุ่มแบบไหนสำหรับมือซ้าย?", bindSame = "เหมือนถนัดขวา",
    bindWalk = "สติ๊กซ้าย: เดิน", bindTurn = "สติ๊กซ้าย: หมุน",
    bindNote = "ไกปืนซ้ายใช้ยิง ตัวเลือกสติ๊กซ้ายจะสลับปุ่มซ้ายขวา" },
  ["ar-ar"] = { title = "الإعداد", sub = "التشغيل الأول", handTitle = "يد السلاح", handSub = "بأي يد تحمل السلاح؟",
    right = "اليد اليمنى", left = "اليد اليسرى", later = "يمكنك تغيير ذلك لاحقًا من إعدادات الواقع الافتراضي.",
    calTitle = "المعايرة", calSub = "وضعية T", cal1 = "قف مستقيمًا وانظر إلى الأمام.",
    cal2 = "مُدّ ذراعيك إلى الجانبين بمستوى الكتفين.", start = "ابدأ", ready = "استعد", hold = "اثبت مكانك",
    failed = "لم يتم رصد الذراعين ممدودتين إلى الجانبين. حاول مرة أخرى.", skip = "تخطي",
    heightTitle = "الطول", heightSub = "طولك", heightRow = "الطول",
    height1 = "طولك من دون حذاء: يُحسب منه طول الذراعين والكتفان.", confirm = "تأكيد",
    bodyTitle = "الجسم", bodySub = "إظهار الجسم كاملًا أم اليدين فقط؟", bodyFull = "الجسم كاملًا", bodyHands = "اليدان فقط",
    bindTitle = "التحكم", bindSub = "أي تخطيط أزرار لليد اليسرى؟", bindSame = "مثل الأيمن",
    bindWalk = "العصا اليسرى: المشي", bindTurn = "العصا اليسرى: الدوران",
    bindNote = "الزناد الأيسر يطلق النار. في خيارَي العصا اليسرى تنعكس الأزرار." },
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

-- The body drawn whole (the avatar) or as its hands alone (xr_body_hands_only, the HandsOnly mod).
local function bodyPage(T, Holo)
  local choose = function(handsOnly)
    native(VRSetBodyHandsOnly, handsOnly)
    S.phase = "idle"
    Holo.push(calibrationPage(T, Holo))
  end
  return {
    title = T.bodyTitle, subtitle = T.bodySub, code = "SETUP//AVATAR",
    rows = function()
      local handsOnly = native(VRBodyHandsOnly) == true
      return {
        { key = "body", cards = {
            { key = "full", picture = { ART, "body_full" }, label = T.bodyFull, on = not handsOnly,
              go = function() choose(false) end },
            { key = "hands", picture = { ART, "body_hands" }, label = T.bodyHands, on = handsOnly,
              go = function() choose(true) end } } },
        { key = "later", text = true, label = T.later },
      }
    end,
  }
end

-- Left-handed, how far the controllers trade (VRSetLeftHandedBindings, xr_left_handed_bindings; src/Hooks/
-- Handedness.hpp): the right hand's bindings as they are, the left stick walking with the buttons mirrored, or the
-- left stick turning with everything mirrored (the first left-handed mode).
local function bindingsPage(T, Holo)
  local choose = function(mode)
    native(VRSetLeftHandedBindings, mode)
    Holo.push(bodyPage(T, Holo))
  end
  return {
    title = T.bindTitle, subtitle = T.bindSub, code = "SETUP//BINDINGS",
    rows = function()
      local mode = native(VRLeftHandedBindings) or 2
      return {
        { key = "bindings", cards = {
            { key = "same", picture = { ART, "bind_same" }, label = T.bindSame, on = mode == 0,
              go = function() choose(0) end },
            { key = "walk", picture = { ART, "bind_walk" }, label = T.bindWalk, on = mode == 1,
              go = function() choose(1) end },
            { key = "turn", picture = { ART, "bind_turn" }, label = T.bindTurn, on = mode == 2,
              go = function() choose(2) end } } },
        { key = "note", text = true, label = T.bindNote },
        { key = "later", text = true, label = T.later },
      }
    end,
  }
end

local function handPage(T, Holo)
  local choose = function(left)
    native(VRSetLeftHanded, left)
    Holo.push(left and bindingsPage(T, Holo) or bodyPage(T, Holo))
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
