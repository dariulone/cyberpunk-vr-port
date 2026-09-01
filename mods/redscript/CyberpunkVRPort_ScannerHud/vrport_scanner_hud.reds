// CyberpunkVRPort -- drag the scanner's HUD where you want it, because HUDitor cannot.
//
// HUDITOR HAS NO SCANNER. Its movable set is a fixed list of named HUD controllers -- minimap, quest
// tracker, health, stamina, dpad, hotkeys, phone, weapon roster, crouch indicator, boss bar, dialog,
// compass, fps counter, the car HUD family -- and not one of them is the scanner. The only thing it
// does with the scanner is listen for its own ScannerDetailsAppearedEvent so it can slide the MINIMAP
// and the TRACKER out of the scanner's way. There was no setting to find: the feature is absent
// there, and this is it.
//
// WHAT IS MOVABLE, enumerated from the game's own scanner folder:
//
//   0  scannerGameController                            the frame and the scan progress
//   1  scannerDetailsGameController                     the details panel: name, level, tabs, chunks
//   2  QuickhacksListGameController                     the quickhack panel as a whole
//   3  ScannerHintInkGameController                     the hint line
//   4  QuickhacksListGameController.m_memoryWidget      the cyberdeck's memory
//   5  QuickhacksListGameController.m_listWidget        the script list
//   6  scannerDetailsGameController.m_changeTabButtonHints   the scanner's key hints
//
// The last three are CHILDREN of pieces 1 and 2 -- the memory, the scripts and the key hints all
// sat inside one movable block, which is what made them impossible to separate. Their offsets
// compose with the parent's: move the panel and they follow, then nudge one of them on top.
//
// scannerBorderGameController is deliberately absent: it is an inkProjectedHUDGameController, drawn
// projected onto the scanned object out in the world, so moving it would only tear the outline off
// the thing it outlines. The ~15 BaseChunkGameController subclasses (name, level, vulnerabilities,
// abilities, resistances, description, requirements, quest clues, vehicle info, quickhack
// description, twintone) are absent for the opposite reason -- they are children of the details
// panel, so slot 1 already carries them.
//
// NO CODEWARE AND NO REPARENTING, and both of those are measured rather than chosen. HUDitor reaches
// the HUD through GameInstance.GetInkSystem() and walks widgets with GetParentWidget(); the compiler
// rejects BOTH as unknown to the game ("method 'GetInkSystem' not found on 'GameInstance'", "method
// 'GetParent' not found on 'inkWidget'") -- they are Codeware's, registered at runtime. Everything
// used below was put to the compiler one call per function and came back resolved. Without a parent
// there is no way to hang a container beside a HUD root, so the transform goes on the root itself,
// which also means this mod never restructures the game's widget tree: the inkWidgetRef paths every
// scanner controller resolves internally stay exactly as the game built them.
//
// THE FOUR ROOTS MEET THROUGH THE PLAYER. Each controller writes its own root into a field added to
// PlayerPuppet as it initialises, because redscript has no global and, with no ink system to
// enumerate, one controller cannot otherwise see another's widget. The editor lives on the details
// panel and reads the four handles back from there.
//
// THE CONTROLS, while the scanner is up:
//
//   RIGHT SHIFT, tapped     enter the editor / step to the next piece (the others dim to 15%)
//   RIGHT SHIFT, held 0.4s  leave the editor and SAVE
//   mouse                   drag the selected piece -- no button to hold
//   W A S D                 nudge it one pixel
//   mouse wheel             scale it about its own middle, 0.1 .. 5.0
//
// THE MOUSE AND THE WHEEL ARE THE PORT'S OWN ACTIONS, declared in r6/input/, and that is not
// tidiness. Borrowing the game's UI_MoveUp / UI_MoveDown for the wheel also borrowed IK_W and IK_S,
// which the game binds to them as secondary UI navigation -- so W and S SCALED the piece while they
// were meant to nudge it. Before that, 'mouse_wheel' turned out to exist only in the UIShared
// context, which gameplay never enters, so the wheel did nothing at all.
//
// THE DRAG IS RAW MOUSE DELTA, not a cursor, and it needs no mouse button: 'click' is an action only
// in the PhotoMode, device, menu and popup contexts, so in gameplay a gate on it can never open --
// which is precisely why the mouse appeared to do nothing while W A S D worked. CameraMouseX and
// CameraMouseY do arrive here (context CameraMovement) and are consumed, so the view cannot turn
// while a piece is being dragged. There is no cursor to show either: HUDitor draws its own inkImage
// and even that one is decorative, since its drag is delta-based too. What tells you where you are
// is the dimming and the piece's name on screen.
//
// THE WHEEL ARRIVES UNDER A DIFFERENT NAME. 'mouse_wheel' belongs to the UIShared context and never
// fires in gameplay, so scaling was dead for the same reason the mouse was; UI_MoveUp / UI_MoveDown
// are the same notch seen from UI_QuickHackPanel, and the quickhack panel is muted while the editor
// is up, so nothing else takes them.
//
// Only while scanning: the Right Shift listener is registered when a scanned object appears and
// dropped when it goes, so the key does nothing at all the rest of the time.
//
// WHERE THE NUMBERS LIVE: the plugin, in bin/x64/vrport.ini as xr_scanner_frame / _details /
// _hacks / _hint, three values each (x, y, scale). A redscript cannot write a file, so the editor
// writes through VRScannerSlotSet and asks for ONE ini save when it closes -- not per drag frame,
// which would be hundreds of writes for one gesture.
//
// NO `module` LINE HERE: a native declared inside a module is looked up by its module-qualified name
// while the plugin registers plain globals. That mistake cost a startup once already.

// Queued to the player and handled on the details controller, which is how the quickhack panel does
// its own delayed work (DelayedDescriptionIntro). It exists because the panel's intro animations and
// its rebuilt memory cells land AFTER the hook that first applies our transform, so the layout has to
// be put back once the game has finished moving things about.
public class VRPortScannerReapply extends Event {}

native func VRScannerSlotGet(idx: Int32, comp: Int32) -> Float;
native func VRScannerSlotSet(idx: Int32, x: Float, y: Float, scale: Float) -> Int32;
native func VRScannerSlotSave() -> Int32;

// The four roots, parked on the player because that is the one object every scanner controller can
// reach and the editor can reach too.
@addField(PlayerPuppet) public let vrpScanRoot0: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanRoot1: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanRoot2: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanRoot3: wref<inkWidget>;
// 4, 5 and 6 are SUB-WIDGETS of the two panels above -- the cyberdeck's memory bar, the script
// list, and the scanner's key hints. They shared one movable block until now, which is exactly the
// complaint. Reached through inkWidgetRef.Get from a method added to their own controller, so the
// private fields that hold them are in scope.
@addField(PlayerPuppet) public let vrpScanRoot4: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanRoot5: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanRoot6: wref<inkWidget>;
// A COMPANION for the two pieces that are really two widgets. m_memoryWidget is only the container
// the panel spawns memory cells into; the words 'Cyberdeck memory: 3/8' are m_avaliableMemory, a
// different widget entirely, so moving the cells left the text sitting where it was. The key hints
// have the same split, m_changeTabButtonHints and m_changeTabInlineHint. A companion takes the same
// transform and the same dimming as its piece, so the pair behaves as one thing.
@addField(PlayerPuppet) public let vrpScanMate4: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanMate5: wref<inkWidget>;
@addField(PlayerPuppet) public let vrpScanMate6: wref<inkWidget>;
// The editor is on. Parked on the player for the same reason the roots are: the quickhack panel
// has to see it, and it is a different object in a different file.
@addField(PlayerPuppet) public let vrpScanEditing: Bool;

// Named on screen when it is selected, because dimming the other three says which one is live only
// if you can see all four -- and the hint line is invisible unless the game has something to hint.
func VRPortScannerPieceName(idx: Int32) -> String {
  switch idx {
    case 0: return "SCANNER FRAME";
    case 1: return "DETAILS PANEL";
    case 2: return "QUICKHACK LIST";
    case 3: return "HINT LINE";
    case 4: return "CYBERDECK MEMORY";
    case 5: return "SCRIPT LIST";
    case 6: return "SCRIPT DESCRIPTION";
  };
  return "";
}

// The eight the editor takes for itself while it is up. Consumed rather than merely handled: the
// mouse is the camera in gameplay and W A S D are the legs, and a drag that also walks the player
// out of scanning range is not a drag.
func VRPortScannerOwnsAction(name: CName) -> Bool {
  return Equals(name, n"CameraMouseX") || Equals(name, n"CameraMouseY")
      || Equals(name, n"mouse_x") || Equals(name, n"mouse_y")
      || Equals(name, n"VRPortScannerMoveX") || Equals(name, n"VRPortScannerMoveY")
      || Equals(name, n"VRPortScannerScaleUp") || Equals(name, n"VRPortScannerScaleDown")
      || Equals(name, n"Forward") || Equals(name, n"Back")
      || Equals(name, n"Left") || Equals(name, n"Right");
}

func VRPortScanRootAt(pp: ref<PlayerPuppet>, idx: Int32) -> ref<inkWidget> {
  if !IsDefined(pp) {
    return null;
  };
  switch idx {
    case 0: return pp.vrpScanRoot0;
    case 1: return pp.vrpScanRoot1;
    case 2: return pp.vrpScanRoot2;
    case 3: return pp.vrpScanRoot3;
    case 4: return pp.vrpScanRoot4;
    case 5: return pp.vrpScanRoot5;
    case 6: return pp.vrpScanRoot6;
  };
  return null;
}

// A piece may still carry one companion widget, and nothing currently needs one: that facility was
// added to drag a stray label along, and a container has no strays. Kept because the next piece that
// turns out to be two widgets will want it.
func VRPortScanMateAt(pp: ref<PlayerPuppet>, idx: Int32) -> ref<inkWidget> {
  if !IsDefined(pp) {
    return null;
  };
  switch idx {
    case 4: return pp.vrpScanMate4;
    case 5: return pp.vrpScanMate5;
    case 6: return pp.vrpScanMate6;
  };
  return null;
}

// HALF SIZE IN A BRAINDANCE, AND NOWHERE ELSE.
//
// A braindance is played on a 3072x3072 square view with the whole HUD pulled toward the centre, and at
// authored size the scanner covers it. Asked for as braindance-only, so it cannot go in the asset: a
// scale written into scanner_hud or quickhacks would shrink the scanner in ordinary play too.
//
// IT BELONGS HERE rather than in a mod of its own, and the reason is TIMING. A separate hook on
// scannerGameController.OnInitialize was written first, compiled clean, and did nothing: that fires when
// the HUD is BUILT -- once, at load, long before any braindance -- so the factor was computed as 1 and
// never recomputed. This file already re-applies on OnScannerHudSpawned and on the reapply event, which
// is per scanner open, and that is the moment the state has to be read.
//
// A MULTIPLIER on the slot scale, not a replacement: whatever a slot carries keeps working, and outside
// a braindance the product is unchanged, so the "1.00 means DO NOT TOUCH IT" rule below still holds and
// the asset's own halving of the quickhack panel is not overwritten.
// ASK THE GAME, DO NOT INFER FROM A CONTROLLER'S LIFETIME.
//
// The first version kept a flag raised by BraindanceGameController.OnInitialize. That is wrong, and it
// showed: BraindanceGameController is a HUD game controller, so it is built with the rest of the HUD and
// initialises on every load, braindance or not -- the flag was true always, and the companion CET mod,
// armed by the same hook, spent its whole budget walking the ink tree after each save load. Reported as
// "какие-то фризы где-то секунд 5-10 после загрузки в сейв".
//
// BraindanceSystem answers the actual question, and the port's own ForceFPP mod already reads it exactly
// this way. It is a scriptable system lookup on a path that runs once per scanner open, not per frame.
func VRPortScannerBdFactor() -> Float {
  let sys: ref<BraindanceSystem> =
    GameInstance.GetScriptableSystemsContainer(GetGameInstance()).Get(n"BraindanceSystem") as BraindanceSystem;
  if IsDefined(sys) && sys.GetIsInBraindance() {
    return 0.5;
  };
  return 1.0;
}

func VRPortScannerApply(idx: Int32, root: ref<inkWidget>) -> Void {
  let scale: Float;
  if !IsDefined(root) {
    return;
  };
  scale = VRScannerSlotGet(idx, 2);
  // A zero would erase the piece rather than shrink it, and zero is what an unregistered native
  // returns. The plugin clamps as well; this is the half that survives the plugin not being there.
  if scale <= 0.01 {
    scale = 1.00;
  };
  scale = scale * VRPortScannerBdFactor();
  // Pivot set explicitly, so the scale is about the piece's own middle rather than about whatever its
  // resource happened to declare. Otherwise shrinking also walks the piece across the screen and the
  // two knobs fight each other.
  root.SetRenderTransformPivot(new Vector2(0.5, 0.5));
  root.SetTranslation(new Vector2(VRScannerSlotGet(idx, 0), VRScannerSlotGet(idx, 1)));
  // A RENDER TRANSFORM, and softness when it shrinks is the price. Scaling in layout space instead --
  // size and font size from each widget's remembered original -- was tried and does not hold: the text
  // jumped larger for a frame and the engine's next layout pass put the authored values back. Keeping
  // it would mean rewriting those properties every frame against the engine's own layout. The durable
  // way to get crisp small text is to change the authored numbers in quickhacks.inkwidget itself, where
  // the engine rasterises from its own values and there is nothing of ours to overwrite.
  // A SLOT SCALE OF 1 MEANS 'DO NOT TOUCH IT', not 'set it to 1'. The panel is halved in the
  // asset now (the Root widget's own renderTransform), and this line was writing 1.0 straight
  // over that every time a piece was applied -- which is why quickhacks stayed full size while
  // the scanner, which no slot points at, halved correctly.
  if AbsF(scale - 1.00) > 0.001 {
    root.SetScale(new Vector2(scale, scale));
  };
}

// Called from each piece's OnInitialize AND from a later hook of the same controller: a controller
// can initialise before its widget is where it will finally live, and re-registering costs nothing.
// Applies a piece AND its companion, which is what every caller wants -- a piece that is two widgets
// is still one piece to the person dragging it.
func VRPortScannerApplyPair(pp: ref<PlayerPuppet>, idx: Int32) -> Void {
  VRPortScannerApply(idx, VRPortScanRootAt(pp, idx));
  VRPortScannerApply(idx, VRPortScanMateAt(pp, idx));
}

func VRPortScannerRegisterMate(idx: Int32, owner: ref<GameObject>, mate: ref<inkWidget>) -> Void {
  let pp: ref<PlayerPuppet> = owner as PlayerPuppet;
  if !IsDefined(pp) || !IsDefined(mate) {
    return;
  };
  switch idx {
    case 4: pp.vrpScanMate4 = mate; break;
    case 5: pp.vrpScanMate5 = mate; break;
    case 6: pp.vrpScanMate6 = mate; break;
  };
  VRPortScannerApply(idx, mate);
}

// THE QUICKHACK PANEL'S THREE CONTAINERS, by the names its .inkwidget gives them. Direct children of
// the controller's root widget, so GetWidget reaches them without a parent walk -- which matters,
// because the game exposes no way to walk upward from a widget.
//
//   top_panel    the memory: its title line, its separator, its background fluff and its cells
//   left_panel   the script list and the heading above it
//   right_panel  the description block beside the list
//
// input_container is hidden rather than registered: it holds nothing but input_hint, the key hints,
// and those name keyboard keys that mean nothing in a headset. Delete the line to keep them.
func VRPortScannerRegisterPanels(ctrl: ref<inkGameController>) -> Void {
  let root: ref<inkCompoundWidget> = ctrl.GetRootCompoundWidget();
  let owner: ref<GameObject> = ctrl.GetPlayerControlledObject();
  let hints: ref<inkWidget>;
  if !IsDefined(root) {
    return;
  };
  VRPortScannerRegister(4, owner, root.GetWidget(n"top_panel"));
  VRPortScannerRegister(5, owner, root.GetWidget(n"left_panel"));
  VRPortScannerRegister(6, owner, root.GetWidget(n"right_panel"));
  hints = root.GetWidget(n"input_container");
  if IsDefined(hints) {
    hints.SetVisible(false);
  };
}

func VRPortScannerRegister(idx: Int32, owner: ref<GameObject>, root: ref<inkWidget>) -> Void {
  let pp: ref<PlayerPuppet> = owner as PlayerPuppet;
  if !IsDefined(pp) || !IsDefined(root) {
    return;
  };
  switch idx {
    case 0: pp.vrpScanRoot0 = root; break;
    case 1: pp.vrpScanRoot1 = root; break;
    case 2: pp.vrpScanRoot2 = root; break;
    case 3: pp.vrpScanRoot3 = root; break;
    case 4: pp.vrpScanRoot4 = root; break;
    case 5: pp.vrpScanRoot5 = root; break;
    case 6: pp.vrpScanRoot6 = root; break;
  };
  VRPortScannerApply(idx, root);
}

// ---- the four pieces register themselves ------------------------------------------------------

@wrapMethod(scannerGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VRPortScannerRegister(0, this.GetPlayerControlledObject(), this.GetRootWidget());
  return result;
}

@wrapMethod(scannerGameController)
protected cb func OnScannerHudSpawned(widget: ref<inkWidget>, userData: ref<IScriptable>) -> Bool {
  let result: Bool = wrappedMethod(widget, userData);
  VRPortScannerRegister(0, this.GetPlayerControlledObject(), this.GetRootWidget());
  return result;
}

@wrapMethod(scannerDetailsGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VRPortScannerRegister(1, this.GetPlayerControlledObject(), this.GetRootWidget());
  // THE SCANNER'S KEY HINTS ARE HIDDEN, not moved: asked for outright, and they are the one piece
  // that has nothing to say in VR -- the keys they name are not the ones a headset uses. Both halves
  // go, the button row and the inline line. To bring them back, delete these two calls.
  inkWidgetRef.SetVisible(this.m_changeTabButtonHints, false);
  inkWidgetRef.SetVisible(this.m_changeTabInlineHint, false);
  return result;
}

@wrapMethod(QuickhacksListGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VRPortScannerRegister(2, this.GetPlayerControlledObject(), this.GetRootWidget());
  VRPortScannerRegisterPanels(this);
  return result;
}


// THE SELECTED SCRIPT'S NAME AND ITS OWN STATUS CHIPS, ABOVE THE RAM USAGE LINE.
//
// THE CHIPS ARE MOVED, NOT REDRAWN. Writing the same words into three plain texts of
// additional_information_container looked cheap next to the real thing, and it could not reproduce
// the chip backgrounds, the icons or the Locked/Default state colouring. Those chips exist only
// inside a row -- status, category, reveal and cooldown, in one horizontal panel inside the row's
// text_container, which the asset hides so the row keeps just its cost and icons. So the SELECTED
// row's panel is reparented into the block and the previous one is put back where it came from.
// Moving the game's own widget means the style, the highlight and the empty-slot gaps are whatever
// the row would have shown, because it IS what the row would have shown.
@addField(QuickhacksListGameController) public let vrpChips: wref<inkWidget>;
@addField(QuickhacksListGameController) public let vrpChipsHome: wref<inkCompoundWidget>;

func VRPortFindByName(w: ref<inkWidget>, want: CName, depth: Int32) -> ref<inkWidget> {
  let comp: ref<inkCompoundWidget>;
  let i: Int32;
  let found: ref<inkWidget>;
  if !IsDefined(w) || depth > 12 {
    return null;
  };
  if Equals(w.GetName(), want) {
    return w;
  };
  comp = w as inkCompoundWidget;
  if !IsDefined(comp) {
    return null;
  };
  i = 0;
  while i < comp.GetNumChildren() {
    found = VRPortFindByName(comp.GetWidgetByIndex(i), want, depth + 1);
    if IsDefined(found) {
      return found;
    };
    i += 1;
  };
  return null;
}

// The chip panel is the row's text_container child that is NOT the name canvas. Its authored name is
// a generated one (inkHorizontalPanelWidget36), so it is found by elimination rather than by that.
func VRPortRowChips(rowRoot: ref<inkWidget>) -> ref<inkWidget> {
  let host: ref<inkCompoundWidget> = VRPortFindByName(rowRoot, n"text_container", 0) as inkCompoundWidget;
  let i: Int32;
  let child: ref<inkWidget>;
  if !IsDefined(host) {
    return null;
  };
  i = 0;
  while i < host.GetNumChildren() {
    child = host.GetWidgetByIndex(i);
    if IsDefined(child) && NotEquals(child.GetName(), n"qh_name_canvas") {
      return child;
    };
    i += 1;
  };
  return null;
}

// A CHIP KEEPS ITS WORDS BUT LOSES ITS COLOUR WHEN IT MOVES. A chip's colour comes from its STATE
// resolved against a style, and a style resolves along the parent chain -- reparented out of the row,
// that chain is gone. Painting the whole panel one colour fixed the red but flattened the rest, so
// each chip gets its own authored colour back, measured out of the 'action' library item:
//
//   status_container    (0.369 0.965 1.000) cyan, and (1.176 0.381 0.348) red when not Ready --
//                       the same condition the row uses in SetActionState
//   category_container  (0.369 0.965 1.000) cyan
//   reveal_container    (1.119 0.844 0.257) yellow -- 'может быть обнаружен' was never cyan
//   cooldown_panel      (1.176 0.381 0.348) red, on its text AND its icon
//
// Only text widgets are touched, plus the cooldown icon by name: bg_Status is a near-black plate
// (0.055 0.055 0.090) and recolouring it would light up the chip's own background.
// THE FRAME TOO, not just the words. A chip is a near-black plate (bg_Status) with a frame drawn
// by fg_Status and its three ghosts, and those carry the authored cyan as well -- so 'может быть
// обнаружен' came out yellow inside a blue box, and its percentage variant yellow inside a red one.
// Frame and text take the same colour; bg_Status is left alone, or the chip's own background
// lights up.
func VRPortIsChipFrame(nm: CName) -> Bool {
  return Equals(nm, n"fg_Status") || Equals(nm, n"fg_Status_ghost")
      || Equals(nm, n"fg_Status_ghost2") || Equals(nm, n"fg_Status_ghost3");
}

func VRPortTintTexts(w: ref<inkWidget>, c: HDRColor, depth: Int32) -> Void {
  let comp: ref<inkCompoundWidget>;
  let i: Int32;
  if !IsDefined(w) || depth > 6 {
    return;
  };
  if IsDefined(w as inkText) || VRPortIsChipFrame(w.GetName()) {
    w.SetTintColor(c);
  };
  comp = w as inkCompoundWidget;
  if !IsDefined(comp) {
    return;
  };
  i = 0;
  while i < comp.GetNumChildren() {
    VRPortTintTexts(comp.GetWidgetByIndex(i), c, depth + 1);
    i += 1;
  };
}

func VRPortColourChips(chips: ref<inkWidget>, ready: Bool) -> Void {
  let panel: ref<inkCompoundWidget> = chips as inkCompoundWidget;
  let cyan: HDRColor = new HDRColor(0.369, 0.965, 1.000, 1.0);
  let red: HDRColor = new HDRColor(1.176, 0.381, 0.348, 1.0);
  let yellow: HDRColor = new HDRColor(1.119, 0.844, 0.257, 1.0);
  let i: Int32;
  let chip: ref<inkWidget>;
  let icon: ref<inkWidget>;
  let nm: CName;
  if !IsDefined(panel) {
    return;
  };
  i = 0;
  while i < panel.GetNumChildren() {
    chip = panel.GetWidgetByIndex(i);
    if IsDefined(chip) {
      nm = chip.GetName();
      if Equals(nm, n"status_container") {
        if ready {
          VRPortTintTexts(chip, cyan, 0);
        } else {
          VRPortTintTexts(chip, red, 0);
        };
      } else {
        if Equals(nm, n"reveal_container") {
          VRPortTintTexts(chip, yellow, 0);
        } else {
          if Equals(nm, n"cooldown_panel") {
            VRPortTintTexts(chip, red, 0);
            icon = VRPortFindByName(chip, n"icon", 0);
            if IsDefined(icon) {
              icon.SetTintColor(red);
            };
          } else {
            VRPortTintTexts(chip, cyan, 0);
          };
        };
      };
    };
    i += 1;
  };
}
@wrapMethod(QuickhacksListGameController)
protected cb func OnItemSelected(index: Int32, itemController: ref<ListItemController>) -> Bool {
  let result: Bool = wrappedMethod(index, itemController);
  let root: ref<inkWidget> = this.GetRootWidget();
  let block: ref<inkCompoundWidget>;
  let label: ref<inkText>;
  let chips: ref<inkWidget>;
  let home: ref<inkCompoundWidget>;
  if !IsDefined(root) || !IsDefined(this.m_selectedData) || !IsDefined(itemController) {
    return result;
  };
  block = VRPortFindByName(root, n"description_text_container", 0) as inkCompoundWidget;
  if !IsDefined(block) {
    return result;
  };
  block.SetVisible(true);
  label = VRPortFindByName(block, n"name_tooltip", 0) as inkText;
  if IsDefined(label) {
    label.SetVisible(true);
    label.SetText(GetLocalizedText(this.m_selectedData.m_title));
  };
  // the previous row gets its chips back before this row hands over its own
  if IsDefined(this.vrpChips) && IsDefined(this.vrpChipsHome) {
    this.vrpChips.Reparent(this.vrpChipsHome);
    this.vrpChips = null;
    this.vrpChipsHome = null;
  };
  chips = VRPortRowChips(itemController.GetRootWidget());
  home = VRPortFindByName(itemController.GetRootWidget(), n"text_container", 0) as inkCompoundWidget;
  if IsDefined(chips) && IsDefined(home) {
    this.vrpChips = chips;
    this.vrpChipsHome = home;
    chips.Reparent(block);
    chips.SetVisible(true);
    // GROUPED UNDER THE NAME, FROM THE CENTRE. In the row this panel sat to the right of the
    // name canvas and carried that row's margin, so dropped into the block as it was it hung off
    // to one side. The block is a vertical panel, so a child's horizontal placement is its HAlign.
    chips.SetHAlign(inkEHorizontalAlign.Center);
    chips.SetVAlign(inkEVerticalAlign.Top);
    chips.SetMargin(new inkMargin(0.0, 6.0, 0.0, 0.0));
    VRPortColourChips(chips, Equals(this.m_selectedData.m_actionState, EActionInactivityReson.Ready));
  };
  return result;
}
@wrapMethod(QuickhacksListGameController)
protected cb func OnTargetDisplayNameChanged(value: Variant) -> Bool {
  let result: Bool = wrappedMethod(value);
  VRPortScannerRegister(2, this.GetPlayerControlledObject(), this.GetRootWidget());
  VRPortScannerRegisterPanels(this);
  return result;
}

@wrapMethod(ScannerHintInkGameController)
protected cb func OnInitialize() -> Bool {
  let result: Bool = wrappedMethod();
  VRPortScannerRegister(3, this.GetPlayerControlledObject(), this.GetRootWidget());
  return result;
}

@wrapMethod(ScannerHintInkGameController)
protected cb func OnVisionModeChanged(value: Int32) -> Bool {
  let result: Bool = wrappedMethod(value);
  VRPortScannerRegister(3, this.GetPlayerControlledObject(), this.GetRootWidget());
  return result;
}

// THE QUICKHACK PANEL IS MUTED WHILE THE EDITOR IS UP, and consuming our own action could never
// have done it: the panel listens for UI_MoveUp / UI_MoveDown, the game binds IK_MouseWheelUp and
// IK_MouseWheelDown to those, and 'mouse_wheel' is a SEPARATE action off the same physical wheel.
// Consuming one says nothing about the other, which is exactly why the wheel kept picking hacks
// while it was meant to be scaling a widget. Muting the whole handler is also what the game itself
// does when it wants this panel to stop listening -- see m_isUILocked, the first thing its OnAction
// tests.
@wrapMethod(QuickhacksListGameController)
protected cb func OnAction(action: ListenerAction, consumer: ListenerActionConsumer) -> Bool {
  let pp: ref<PlayerPuppet> = this.GetPlayerControlledObject() as PlayerPuppet;
  if IsDefined(pp) && pp.vrpScanEditing {
    return false;
  };
  return wrappedMethod(action, consumer);
}

// ---- the editor, hosted on the details controller ---------------------------------------------
// It is the one piece always present while scanning and it already carries the scanner's lifecycle
// hook, so the mode and the selection live here.

@addField(scannerDetailsGameController) public let vrpListening: Bool;
@addField(scannerDetailsGameController) public let vrpEditing: Bool;
@addField(scannerDetailsGameController) public let vrpActive: Int32;
// How many of each mouse axis has arrived since the editor opened. Reported on every Right Shift,
// because 'the mouse Y does nothing' has more than one possible cause and this separates them in one
// press: no events at all is an input-layer answer, events with no movement is an arithmetic one.
@addField(scannerDetailsGameController) public let vrpMouseXHits: Int32;
@addField(scannerDetailsGameController) public let vrpMouseYHits: Int32;
@addField(scannerDetailsGameController) public let vrpUiMouseXHits: Int32;
@addField(scannerDetailsGameController) public let vrpUiMouseYHits: Int32;
@addField(scannerDetailsGameController) public let vrpOwnXHits: Int32;
@addField(scannerDetailsGameController) public let vrpOwnYHits: Int32;

// One piece bright, the rest at 15% -- HUDitor's own way of saying which one the mouse is holding,
// and the only feedback this editor needs. It doubles as the proof that the key arrived at all.
@addMethod(scannerDetailsGameController)
public func VRPortHighlight(editing: Bool) -> Void {
  let pp: ref<PlayerPuppet> = this.GetPlayerControlledObject() as PlayerPuppet;
  let root: ref<inkWidget>;
  let mate: ref<inkWidget>;
  let idx: Int32 = 0;
  while idx < 7 {
    root = VRPortScanRootAt(pp, idx);
    mate = VRPortScanMateAt(pp, idx);
    if !editing || Equals(idx, this.vrpActive) {
      if IsDefined(root) {
        root.SetOpacity(1.00);
      };
      if IsDefined(mate) {
        mate.SetOpacity(1.00);
      };
    } else {
      if IsDefined(root) {
        root.SetOpacity(0.15);
      };
      if IsDefined(mate) {
        mate.SetOpacity(0.15);
      };
    };
    idx += 1;
  };
}

@addMethod(scannerDetailsGameController)
public func VRPortEditorInput(on: Bool) -> Void {
  let player: ref<GameObject> = this.GetPlayerControlledObject();
  if !IsDefined(player) {
    return;
  };
  if on {
    player.RegisterInputListener(this, n"CameraMouseX");
    player.RegisterInputListener(this, n"CameraMouseY");
    // The same two axes under the names the UI contexts give them. Focus mode stacks its own
    // contexts over gameplay, so which pair actually reaches a listener here is not a given.
    player.RegisterInputListener(this, n"mouse_x");
    player.RegisterInputListener(this, n"mouse_y");
    // OUR OWN, from r6/input/CyberpunkVRPort_ScannerHud.xml: two mouse axes and the two wheel
    // directions, under names nothing else in the game listens for. Borrowing UI_MoveUp / UI_MoveDown
    // for the wheel also borrowed IK_W and IK_S, which the game binds to them as secondary UI
    // navigation -- so W and S scaled the piece while they were supposed to nudge it.
    player.RegisterInputListener(this, n"VRPortScannerMoveX");
    player.RegisterInputListener(this, n"VRPortScannerMoveY");
    player.RegisterInputListener(this, n"VRPortScannerScaleUp");
    player.RegisterInputListener(this, n"VRPortScannerScaleDown");
    player.RegisterInputListener(this, n"Forward");
    player.RegisterInputListener(this, n"Back");
    player.RegisterInputListener(this, n"Left");
    player.RegisterInputListener(this, n"Right");
  } else {
    player.UnregisterInputListener(this, n"CameraMouseX");
    player.UnregisterInputListener(this, n"CameraMouseY");
    player.UnregisterInputListener(this, n"mouse_x");
    player.UnregisterInputListener(this, n"mouse_y");
    player.UnregisterInputListener(this, n"VRPortScannerMoveX");
    player.UnregisterInputListener(this, n"VRPortScannerMoveY");
    player.UnregisterInputListener(this, n"VRPortScannerScaleUp");
    player.UnregisterInputListener(this, n"VRPortScannerScaleDown");
    player.UnregisterInputListener(this, n"Forward");
    player.UnregisterInputListener(this, n"Back");
    player.UnregisterInputListener(this, n"Left");
    player.UnregisterInputListener(this, n"Right");
  };
}

// The on-screen line, through the blackboard the game uses for its own notifications -- the same
// call HUDitor makes to tell you its editor is blocked.
// TWICE, and late. The first shot catches a normal appearance; the second covers an intro that ran
// long or a panel that rebuilt itself again. Re-applying a transform that is already correct costs a
// pair of float writes, so there is no reason to be clever about the timing.
@addMethod(scannerDetailsGameController)
public func VRPortScheduleReapply() -> Void {
  let player: ref<GameObject> = this.GetPlayerControlledObject();
  if !IsDefined(player) {
    return;
  };
  GameInstance.GetDelaySystem(player.GetGame()).DelayEvent(player, new VRPortScannerReapply(), 0.35, false);
  GameInstance.GetDelaySystem(player.GetGame()).DelayEvent(player, new VRPortScannerReapply(), 1.20, false);
}

@addMethod(scannerDetailsGameController)
protected cb func OnVRPortScannerReapply(evt: ref<VRPortScannerReapply>) -> Bool {
  let pp: ref<PlayerPuppet> = this.GetPlayerControlledObject() as PlayerPuppet;
  let idx: Int32 = 0;
  while idx < 7 {
    VRPortScannerApplyPair(pp, idx);
    idx += 1;
  };
}

@addMethod(scannerDetailsGameController)
public func VRPortEditorSay(text: String) -> Void {
  let message: SimpleScreenMessage;
  let board: wref<IBlackboard> = this.GetBlackboardSystem().Get(GetAllBlackboardDefs().UI_Notifications);
  if !IsDefined(board) {
    return;
  };
  message.isShown = true;
  message.message = text;
  message.duration = 2.00;
  board.SetVariant(GetAllBlackboardDefs().UI_Notifications.OnscreenMessage, ToVariant(message), true);
}

// PAUSE IS WHAT MAKES A MOUSE USABLE HERE. Unpaused, CameraMouseX/Y is the camera: the view turned
// and the widget did not, which reads as 'the mouse does nothing'. Paused, the scanner also freezes
// on screen -- which is the only way to work on it with both hands, since holding it open is a VR
// gesture that needs the left hand at the ear. HUDitor pauses for the same reason.
@addMethod(scannerDetailsGameController)
public func VRPortEditorStart() -> Void {
  let handler: wref<inkISystemRequestsHandler> = this.GetSystemRequestsHandler();
  let pp: ref<PlayerPuppet> = this.GetPlayerControlledObject() as PlayerPuppet;
  this.vrpEditing = true;
  this.vrpActive = 0;
  if IsDefined(pp) {
    pp.vrpScanEditing = true;
  };
  this.VRPortEditorInput(true);
  if IsDefined(handler) {
    handler.PauseGame();
  };
}

@addMethod(scannerDetailsGameController)
public func VRPortEditorStop(save: Bool) -> Void {
  let handler: wref<inkISystemRequestsHandler> = this.GetSystemRequestsHandler();
  let pp: ref<PlayerPuppet> = this.GetPlayerControlledObject() as PlayerPuppet;
  if !this.vrpEditing {
    return;
  };
  this.vrpEditing = false;
  if IsDefined(pp) {
    pp.vrpScanEditing = false;
  };
  this.VRPortEditorInput(false);
  this.VRPortHighlight(false);
  // Unpaused LAST, so nothing the game does on resuming lands while the editor still owns input.
  if IsDefined(handler) {
    handler.UnpauseGame();
  };
  if save {
    VRScannerSlotSave();
    this.VRPortEditorSay("Scanner layout saved");
  };
  // The game resumes here, and resuming replays whatever the panel had queued. Put the layout back
  // after that settles, or a layout that looked right in the editor is wrong the moment you leave it.
  this.VRPortScheduleReapply();
}

@addMethod(scannerDetailsGameController)
protected cb func OnAction(action: ListenerAction, consumer: ListenerActionConsumer) -> Bool {
  let name: CName = ListenerAction.GetName(action);
  let pp: ref<PlayerPuppet>;
  let idx: Int32;
  let x: Float;
  let y: Float;
  let scale: Float;
  let changed: Bool = false;

  if Equals(name, n"VRPortScannerEditor") {
    // HELD leaves and saves, TAPPED enters or steps to the next piece -- the same two events
    // HUDitor's F11 uses, and the pair our input file declares: BUTTON_HOLD_COMPLETE and
    // BUTTON_RELEASED. Without both declared there only the press would arrive, and one key could
    // not carry two meanings.
    if Equals(ListenerAction.GetType(action), gameinputActionType.BUTTON_HOLD_COMPLETE) {
      this.VRPortEditorStop(true);
      return true;
    };
    if Equals(ListenerAction.GetType(action), gameinputActionType.BUTTON_RELEASED) {
      if this.vrpEditing {
        if this.vrpActive >= 6 {
          this.vrpActive = 0;
        } else {
          this.vrpActive += 1;
        };
      } else {
        this.vrpMouseXHits = 0;
        this.vrpMouseYHits = 0;
        this.vrpUiMouseXHits = 0;
        this.vrpUiMouseYHits = 0;
        this.vrpOwnXHits = 0;
        this.vrpOwnYHits = 0;
        this.VRPortEditorStart();
      };
      this.VRPortHighlight(true);
      this.VRPortEditorSay(VRPortScannerPieceName(this.vrpActive)
                           + "   own " + ToString(this.vrpOwnXHits) + "/" + ToString(this.vrpOwnYHits)
                           + "   cam " + ToString(this.vrpMouseXHits) + "/" + ToString(this.vrpMouseYHits)
                           + "   ui " + ToString(this.vrpUiMouseXHits) + "/" + ToString(this.vrpUiMouseYHits));
      return true;
    };
    return true;
  };

  if !this.vrpEditing {
    return false;
  };

  // Taken off the game for as long as the editor is up, before anything is done with it.
  if !VRPortScannerOwnsAction(name) {
    return false;
  };
  ListenerActionConsumer.Consume(consumer);

  // NO BUTTON TO HOLD. 'click' is an action only in the PhotoMode, device, menu and popup contexts --
  // there is no such action in gameplay, so a gate on it could never open and every mouse delta was
  // discarded behind it. Which is exactly what 'the mouse does not work' was. While the editor is up
  // the mouse simply moves the selected piece: the game is paused, the axes are consumed, and nothing
  // else is asking for them.

  // Read the live values back from the plugin on every event instead of caching them here, so there
  // is exactly one copy of the layout and the widget cannot drift away from the ini.
  idx = this.vrpActive;
  x = VRScannerSlotGet(idx, 0);
  y = VRScannerSlotGet(idx, 1);
  scale = VRScannerSlotGet(idx, 2);
  if scale <= 0.01 {
    scale = 1.00;
  };

  // OURS FIRST. Everything below it is a fallback that only acts if this one has never fired, so a
  // motion cannot be applied twice under two names -- which would read as one axis being twice as
  // sensitive as the other and nothing being broken.
  if Equals(name, n"VRPortScannerMoveX") {
    this.vrpOwnXHits += 1;
    x += ListenerAction.GetValue(action) * 0.60;
    changed = true;
  };
  if Equals(name, n"VRPortScannerMoveY") {
    this.vrpOwnYHits += 1;
    y -= ListenerAction.GetValue(action) * 0.60;
    changed = true;
  };

  if Equals(name, n"CameraMouseX") {
    this.vrpMouseXHits += 1;
    if Equals(this.vrpOwnXHits, 0) {
      x += ListenerAction.GetValue(action) * 0.60;
      changed = true;
    };
  };
  if Equals(name, n"CameraMouseY") {
    this.vrpMouseYHits += 1;
    if Equals(this.vrpOwnYHits, 0) {
      y -= ListenerAction.GetValue(action) * 0.60;
      changed = true;
    };
  };

  // The fallbacks, applied ONLY where the camera-named twin has never fired. If both names arrive
  // this would otherwise move the piece twice per motion, which reads as double sensitivity on one
  // axis and nothing at all being wrong -- the worst kind of bug to find later.
  if Equals(name, n"mouse_x") {
    this.vrpUiMouseXHits += 1;
    if Equals(this.vrpMouseXHits, 0) && Equals(this.vrpOwnXHits, 0) {
      x += ListenerAction.GetValue(action) * 0.60;
      changed = true;
    };
  };
  if Equals(name, n"mouse_y") {
    this.vrpUiMouseYHits += 1;
    if Equals(this.vrpMouseYHits, 0) && Equals(this.vrpOwnYHits, 0) {
      y -= ListenerAction.GetValue(action) * 0.60;
      changed = true;
    };
  };

  if Equals(name, n"Forward") && ListenerAction.IsButtonJustPressed(action) {
    y -= 1.00;
    changed = true;
  };
  if Equals(name, n"Back") && ListenerAction.IsButtonJustPressed(action) {
    y += 1.00;
    changed = true;
  };
  if Equals(name, n"Left") && ListenerAction.IsButtonJustPressed(action) {
    x -= 1.00;
    changed = true;
  };
  if Equals(name, n"Right") && ListenerAction.IsButtonJustPressed(action) {
    x += 1.00;
    changed = true;
  };

  // ONE NOTCH, ONE TENTH. Two buttons rather than an axis, because that is what a wheel notch is in
  // this input system -- the direction is the action's identity, not a value. Both are ours alone, so
  // nothing here can collide with a movement key the way the borrowed UI_MoveUp / UI_MoveDown did.
  if Equals(name, n"VRPortScannerScaleUp") && ListenerAction.IsButtonJustPressed(action) {
    scale += 0.10;
    changed = true;
  };
  if Equals(name, n"VRPortScannerScaleDown") && ListenerAction.IsButtonJustPressed(action) {
    scale -= 0.10;
    changed = true;
  };

  // Clamped once, here, because three different inputs now write it. The plugin clamps too, to the
  // same range, so a number that arrives from the ini cannot mean something different from one
  // dragged in.
  if scale < 0.10 {
    scale = 0.10;
  };
  if scale > 5.00 {
    scale = 5.00;
  };

  if changed {
    VRScannerSlotSet(idx, x, y, scale);
    pp = this.GetPlayerControlledObject() as PlayerPuppet;
    VRPortScannerApplyPair(pp, idx);
    return true;
  };
  return false;
}

// THE EDITOR IS ONLY REACHABLE WHILE SCANNING, which is both what was asked for and the only state
// where it means anything: outside a scan all four pieces are invisible, so there would be nothing
// to aim at. This is also where the saved layout is re-applied, because the vanilla method walks
// GetRootWidget().SetVisible() through four branches -- a transform applied once at initialize is
// not guaranteed to still be the one in force when the panel next comes up.
// The panel opening or changing tab over a scan that is ALREADY running never touches
// OnScannedObjectChanged, so those two moments need their own re-apply. Both replay layout.
@wrapMethod(scannerDetailsGameController)
protected cb func OnQuickHackPanelOpened(value: Bool) -> Bool {
  let result: Bool = wrappedMethod(value);
  this.VRPortScheduleReapply();
  return result;
}

@wrapMethod(scannerDetailsGameController)
protected cb func OnScannerTabChangeEvent(evt: ref<ScannerTabChangeEvent>) -> Bool {
  let result: Bool = wrappedMethod(evt);
  this.VRPortScheduleReapply();
  return result;
}

@wrapMethod(scannerDetailsGameController)
protected cb func OnScannedObjectChanged(value: EntityID) -> Bool {
  let result: Bool = wrappedMethod(value);
  let scanning: Bool = EntityID.IsDefined(value);
  let player: ref<GameObject> = this.GetPlayerControlledObject();
  let pp: ref<PlayerPuppet> = player as PlayerPuppet;
  let idx: Int32 = 0;
  VRPortScannerRegister(1, player, this.GetRootWidget());
  if scanning {
    if !this.vrpListening && IsDefined(player) {
      player.RegisterInputListener(this, n"VRPortScannerEditor");
      this.vrpListening = true;
    };
    while idx < 7 {
      VRPortScannerApplyPair(pp, idx);
      idx += 1;
    };
    // ...and again once the panel has finished appearing, which is the only apply that sticks.
    this.VRPortScheduleReapply();
  } else {
    // NOT while editing. The scanner is held open by a VR gesture, and the hand that holds it is the
    // one that would otherwise be on the keyboard -- so letting go is the normal thing to do once the
    // editor is up and the game is paused. Tearing the editor down here would make it unusable by
    // the very motion that frees your hands. The hold on RIGHT SHIFT is the only way out.
    if !this.vrpEditing {
      if this.vrpListening && IsDefined(player) {
        player.UnregisterInputListener(this, n"VRPortScannerEditor");
        this.vrpListening = false;
      };
    };
  };
  return result;
}
