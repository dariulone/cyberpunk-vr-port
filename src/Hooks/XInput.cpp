#include "Utils/DebugGate.hpp"
#include "Overlay/VrOverlay.hpp"
// XInput -- one hook, one file. It registers itself at the bottom; Hooks/Hook.hpp says why the
// stage and order live here rather than in a boot function.
//
// Merges the VR controller into gamepad 0 by patching the game's XInput imports. Opt-out
// via xr_xinput_install, because it is the one hook that changes how ordinary input
// reaches the game.

#include "Anim/WheelGrab.hpp"
#include "Anim/Cyberware.hpp"
#include "Anim/HandGestures.hpp"
#include "Anim/HeadAimWeapon.hpp"
#include "Anim/VrikState.hpp"   // g_VRLocomotionState, for the sprint crouch gate
#include "Core/VrCoreShared.hpp"
#include "Camera/CameraState.hpp"   // DeviceCamActive -- the stick belongs to the camera, not the player
#include "Core/LiveControls.hpp"
#include "Core/Telemetry.hpp"
#include "Hooks/Hook.hpp"
#include "Hooks/Trampoline.hpp"
#include "Hooks/TurnInput.hpp"
#include "Hooks/VehicleButtons.hpp"
#include "Hooks/SwimmingInput.hpp"
#include "Hooks/LadderInput.hpp"
#include "Hooks/InputPacketSequence.hpp"
#include "Hooks/Psvr2Input.hpp"
#include "Hooks/AnalogStick.hpp"
#include "Lobby/Intro.hpp"
#include "Lobby/MenuLobby.hpp"
#include "Hooks/WeaponTrigger.hpp"
#include "Hooks/ScannerGesture.hpp"
#include "Hooks/Handedness.hpp"
#include "Hooks/KeypadInput.hpp"
#include "Hooks/CyberwareChord.hpp"
#include "Hooks/WeaponWheelHold.hpp"
#include "Natives/Reach.hpp"
#include "Natives/PhysHands.hpp"
#include "Natives/VrMenu.hpp"
#include "Quest/StoryAttention.hpp"
#include "Runtimes/OpenXRManager.hpp"
#include "Utils/AobScanner.hpp"
#include "Utils/MemorySafe.hpp"
#include "Utils/SharedSlots.hpp"

#include <windows.h>
#include <algorithm>
#include <cstdint>
#include <cstddef>
#include <Xinput.h>

// The right grip's published flag (shared[49]) and why it is what it is, logged when that changes (physhands trace=1):
// 0 released, 1 a ladder, 2 the cyberware, 3 held, 4 a VR UI capture, 5 a menu or braindance, 6 the VR menu's capture.
static int s_rightGripWhy = -1;
static void NoteRightGrip(int why, float raw) {
    if (why == s_rightGripWhy) return;
    if (cvr::physhands::Tracing())
        Log("[xinput] right grip published %s (%d, the controller's %.2f)\n", why == 3 ? "held" : "off", why, raw);
    s_rightGripWhy = why;
}
#include <mutex>

extern "C" int CyberpunkVR_WeaponClass;
extern HWND g_gameHwnd;
namespace {
    std::atomic<bool> s_weaponTriggerEnabled{false};
    std::atomic<uint64_t> s_weaponTriggerStamp{0};
    std::atomic<uint64_t> s_scannerVision{0};
}
void cvr::input::PublishScannerVision(int state) {
    scannerHandPhysics.Publish(state);
    s_scannerVision.store((GetTickCount64()<<1)|(state==1?1:0),std::memory_order_release);
}
int cvr::input::ScannerVision() {
    const auto sample=s_scannerVision.load(std::memory_order_acquire),stamp=sample>>1,now=GetTickCount64();
    return stamp && now>=stamp && now-stamp<=350 ? int(sample&1) : -1;
}
bool cvr::input::WeaponTriggerEnabled() {
    return s_weaponTriggerEnabled.load(std::memory_order_relaxed) &&
        GetTickCount64()-s_weaponTriggerStamp.load(std::memory_order_relaxed)<250;
}

// DASH on the right stick pushed fully UP. 1 = on. A knob rather than a hardcoded gesture like
// sprint and crouch, because this one claims half of an axis that a player who turns "Disable Mouse Y"
// off is using for camera pitch -- and they should be able to have it back.
// DISCRETE MOVEMENT SPEED. 1 = a push past the deadzone moves at ONE speed whatever the deflection,
// which is what the keyboard does (LeftY_Axis binds IK_W at val="1.0"); 0 = the pad's analogue
// magnitude, where the speed rides the thumb.
// GAMEPLAY REMAPS ARE OFF WHILE A MENU OR A BRAINDANCE OWNS THE SCREEN.
//
// In menus every binding is the default one and none of our override bindings apply, and the same
// for a braindance. Every remap in this file was written for gameplay -- the
// grip standing in for a shoulder, the ear gesture on LB, the dash on B, the magazine drop's mask, the
// walk tiers on the left stick -- and in a menu they are at best redundant and at worst they eat a
// button the menu itself needs. With this on, a menu or a braindance gets the raw controller: buttons,
// triggers and sticks merged as they come, and nothing else.
//
// The Quest Touch controllers have no physical LB/RB. In menus and in a braindance only, their grip
// buttons stand in for those two shoulders so the UI remains navigable with Virtual Desktop gamepad
// emulation off, and the braindance has its editor (LB) and its layers (RB). This is a hardware
// translation, not a gameplay remap; gameplay grips remain reserved for VR actions.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_InputDefaultInUi = 1;
// How long the magazine drop stays blocked after a UI overlay closes. Spamming the close button is what
// this is for: the popup goes away on the first press and the next one would land in gameplay, where B
// drops the magazine.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_PopupMagBlockMs = 1000;
// TEST INPUT: XInput button bits (0x4000 X, 0x2000 B, 0x0080 R3, ...) OR'ed into the VR controller state as if
// the controllers pressed them, so a harness can drive the whole input path -- shared slots, the merge, the
// game's own actions. The OpenXR simulator does not map the port's boolean actions; written by a test process
// only, and 0 in normal play. Bits 16/17 hold the left/right grip fully: the simulator's pose command has no
// grip, and the monowire deploys and swings only while one is held.
extern "C" __declspec(dllexport) volatile int32_t CyberpunkVR_TestPadButtons = 0;
// Bounded background test stick: signed X/Y int16 in the low DWORD, GetTickCount deadline in the high DWORD.
// No controller replacement in normal play; a lost harness expires within ten seconds.
extern "C" __declspec(dllexport) std::atomic<uint64_t> CyberpunkVR_TestPadLeft{};
extern "C" __declspec(dllexport) int32_t CyberpunkVR_MoveTiers = 1;
// HOW LONG THE STICK MUST STAY AT THE STOP BEFORE IT SPRINTS, milliseconds. It used to be instant, and
// with discrete speed that is wrong by construction: any push already runs at full speed, so the stick
// naturally sits AT the stop and an instant detent sprints on every step. 200 ms separates walking
// forward from leaning on it.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SprintHoldMs = 200;
// 0 (default, MEASURED) = a crouch blocks the detent. PSMLocomotionStates has a CrouchSprint state, so
// this was worth asking for -- and the answer, in game, was that the player is STOOD UP and sprints.
// That is the opposite of what a crouching player wants, so the block is the honest default. 1 asks
// anyway, for a build or a cyberware where the game answers differently.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SprintFromCrouch = 0;
// The left stick's clicks, counted by the OpenXR loop (Runtimes/OpenXRFrameLoop.cpp): the sprint-on-click mode.
extern "C" volatile long CyberpunkVR_LeftStickClicks;
// THE RE-ENTRY BLIP. Sprint is a HOLD -- measured: ten press/release pulses never entered the sprint
// state once, while a held button always has. But a game that has LEFT the sprint (a dash, a jump, ADS)
// will not re-enter on a button that is already down, so the hold is briefly RELEASED to make the next
// hold an edge. Held for SprintPulseMs at a minimum between blips, released for SprintGapMs.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SprintPulseMs = 240;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SprintGapMs   = 60;
// After this long without reaching the sprint, stop blipping and just HOLD. Holding is the behaviour
// that shipped and worked, so it is the right thing to fall back to when the game is refusing us
// (no stamina, a state that forbids it) -- a blip every 300 ms forever would only flicker it.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_SprintGiveUpMs = 1500;
// The WALK band, opt-in: stick magnitudes up to this walk instead of run. 0 = no band, every push runs.
// It is off by default on purpose -- a band is a slice of travel that has to be HELD, which is the thing
// discrete speed exists to remove -- but the game gives a pad no walk button at all (ToggleWalk_Button
// is IK_G, a key an XInput hook cannot press), so a band is the only way to reach a walk.
extern "C" __declspec(dllexport) float CyberpunkVR_MoveWalkMax = 0.0f;
// What the walk band outputs, and how much wider the band gets once you are in it (so a thumb resting on
// the boundary does not flip between walk and run).
extern "C" __declspec(dllexport) float CyberpunkVR_MoveWalkMag    = 0.35f;
extern "C" __declspec(dllexport) float CyberpunkVR_MoveWalkMargin = 0.06f;

// The left trigger held off the game while it works a pump's fore-end -- a GetTickCount64 deadline, see the merge.
extern "C" __declspec(dllexport) volatile uint64_t CyberpunkVR_LeftTriggerBlockUntil = 0;

// DUAL WIELDING: each hand's trigger fires its own gun (see the merge). The deadline is held up by the
// reload mod (SetVRDualWield) for as long as the game reports two guns in hand; the switch turns the
// whole remap off, and the press delay is how long a new squeeze keeps the fire action released so the
// script's hand routing reaches the right gun before the press does.
extern "C" __declspec(dllexport) volatile uint64_t CyberpunkVR_DualWieldUntil = 0;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DualWieldInput = 1;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DualWieldPressDelayMs = 40;
// How long Y has to be held before it is the game's Y again while two guns are in hand; shorter is a tap,
// the left gun's magazine drop (see the end of the merge).
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DualWieldYHoldMs = 300;

// SCANNER GESTURE -- physical left hand at the left ear + a fresh left-stick click.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerGesture = 1;
// The ear, in HMD-LOCAL XR axes (x right, y up, z BACKWARD -- the published hand position is in the
// headset's own frame, so this rides the head and works whichever way the player faces).
// This locates the grip centre, below and outside the point where fingers touch the headset.
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerEarX = -0.18f;
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerEarY = -0.08f;
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerEarZ =  0.08f;
// How near the ear point counts, and how much wider it has to get before it stops counting. The margin
// is what keeps a hand sitting on the boundary from chattering.
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerEarRadius = 0.22f;
extern "C" __declspec(dllexport) float CyberpunkVR_ScannerEarMargin = 0.04f;
// The two debounce windows, milliseconds. Asymmetric on purpose: arming is a decision, dropping is a
// dropout, so a bad tick has to be swallowed rather than believed.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerEnterMs = 80;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerExitMs  = 250;
// TOGGLE OR HOLD. The game's Vision_Hold_Button stays open while LB is asserted.
// By default each fresh left-ear L3 click flips the latch; 0 keeps it open only
// while that click is held in the ear zone.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerToggle = 1;
// 1 = a button bound to LB toggles the scanner as well (press opens, press closes); 0 = the game's raw hold.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerButtonToggle = 1;
// THE HACK LIST ON THE MOVEMENT STICK, up and down. Navigation keeps the existing
// handedness mapping; the activation gesture alone always belongs to physical LEFT L3.
// 0.90 to fire and 0.50 to re-arm keep a resting thumb from paging a list.
// 1 = while MOUNTED, A also emits X so a dialogue line can be confirmed. X is the vehicle exit
// there and is held out of the merge, which left nothing able to confirm at all. The handbrake on
// A keeps working -- the mirror is added, not substituted. 0 restores the previous behaviour.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VehicleDialogConfirmOnA = 1;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerStickNav      = 1;
extern "C" __declspec(dllexport) float   CyberpunkVR_ScannerStickNavFire  = 0.90f;
extern "C" __declspec(dllexport) float   CyberpunkVR_ScannerStickNavRearm = 0.50f;

// SCANNER, WORKED WITH ONE HAND. What each of these has to become is not a preference: the game
// binds the hack list to the D-PAD, applying to X, the scanner's tab to RB and the tag to the right
// stick click. The block that emits them, down in the merge, names the file every one of those was
// read out of. Each is separately switchable because each takes a button away from gameplay.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerFaceNav     = 1;   // left Y up, left X down / apply
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerTriggerTag  = 1;   // right trigger tags, and does not fire
// Raised by the weapon module while the weapon is carried in the LEFT hand (see TwoHandGrip.cpp).
extern "C" __declspec(dllexport) extern int CyberpunkVR_CarryLeft;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerStickTab    = 1;   // right stick click changes the tab
// THE SCANNER'S ZOOM: hold the LEFT trigger and the right stick zooms, in the game's own steps.
// The right stick is busy -- crouch, dash, pitch, snap turn -- so the trigger is the modifier that
// borrows it, and while it is held the axis is consumed before any of those four sees it.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerZoom        = 1;
// How far the stick must go for a step. Lower than the 0.90 the crouch and dash gestures use on
// purpose: those are deliberate shoves to the stop, this is a readout being nudged.
extern "C" __declspec(dllexport) float   CyberpunkVR_ScannerZoomStick   = 0.50f;
// How often a held stick repeats the step. ZoomIn_Button is a button, so holding it does not keep
// zooming -- the press has to be made again, and this is the rate.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerZoomRepeatMs = 200;
// How long X must be HELD to apply the selected hack instead of paging down one. The same split the
// vehicle exit uses and for the same reason: one button with two meanings, the cheap one on the tap.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerApplyHoldMs = 350;
// How long each synthesised press stays down. The game's listeners act on BUTTON_PRESSED, so this
// only has to outlast one XInput poll -- it is not a repeat rate.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_ScannerPulseMs     = 90;

// HOW LONG X MUST BE HELD TO LEAVE A CAR, milliseconds. Not zero, and that is the whole point:
// ExitVehicle_Button has no hold in the game's own mappings, so the vehicle state machine acts on
// the first frame it sees -- and at speed that means jumping out. Level-mirroring X ejected the
// player the moment the button was brushed while driving, which reads as a physics bug rather than
// as a button. 400 ms is the timeout the game uses for its own hold bindings.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_VehicleExitHoldMs = 400;
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DashStickUp = 1;
// Raw horizontal travel, not the deadzoned camera axis. Requiring >0.90 after
// deadzone meant >0.918 raw travel and missed slightly diagonal flicks. Keep
// a deliberate push with a wide hysteresis band, without requiring the stop.
extern "C" __declspec(dllexport) float CyberpunkVR_SnapTurnStickFire  = cvr::input::kSnapTurnStickFire;
extern "C" __declspec(dllexport) float CyberpunkVR_SnapTurnStickRearm = cvr::input::kSnapTurnStickRearm;
// How long the synthetic dodge press is held, milliseconds. A dodge is a button PRESS, so the pulse
// only has to be long enough for the game to see an edge; held level it would dodge over and over.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_DashPulseMs = 100;
// How long the synthetic R3 of the crouch gesture is held, milliseconds: short enough to stay a tap.
extern "C" __declspec(dllexport) int32_t CyberpunkVR_CrouchPulseMs = 100;

// The left trigger is claimed by the lighter gesture while it is armed; the gate says so.
extern "C" __declspec(dllexport) int CyberpunkVR_LtLighterGate;
// How many times the wheel-hub horn gesture fired while it had no button to press. Non-zero here
// with xr_wheel_horn on means the gesture still works and only the binding is missing.
extern "C" __declspec(dllexport) uint64_t CyberpunkVR_DebugWheelHornGestures = 0;

namespace { volatile bool g_sprintInputActive = false; }  // private to this hook

// XInput merge: hook XInputGetState in the loaded XInput1_*.dll and OR the VR
// controller snapshot from OpenXRManager into the gamepad state the game reads
// every frame. CP2077 already has full native gamepad bindings so movement,
// jump, dodge, fire/aim, weapon switch, reload, etc. all "just work" once the
// VR state lands in XINPUT_GAMEPAD.
// ===========================================================================

using XInputGetState_t = DWORD (WINAPI*)(DWORD, XINPUT_STATE*);

XInputGetState_t g_realXInputGetState = nullptr;
static bool     g_xinputHooked = false;
static int      g_xinputSnapArmedDir = 0;       // currently latched stick direction so we don't fire while held
static DWORD    g_xinputSnapPulseStartMs = 0;   // when the current pulse began
static int      g_xinputSnapPulseDir = 0;       // direction of the active pulse (0 = idle)

extern "C" int GetSnapTurnPulseMs();

static SHORT FloatToSHORT(float v) {
    if (v > 1.0f) v = 1.0f;
    if (v < -1.0f) v = -1.0f;
    return (v >= 0.0f) ? static_cast<SHORT>(v * 32767.0f) : static_cast<SHORT>(v * 32768.0f);
}
static BYTE FloatToBYTE(float v) {
    if (v < 0.0f) v = 0.0f;
    if (v > 1.0f) v = 1.0f;
    return static_cast<BYTE>(v * 255.0f);
}
// ONE KEYSTROKE, FOR THE SCANNER'S ZOOM. The game's own gamepad zoom IS the D pad: ZoomIn_Button
// is IK_Pad_DigitUp and ZoomOut_Button is IK_Pad_DigitDown. Emitting those worked and then paged
// the script list instead, because while the quickhack panel is focused UI_QuickHackPanel binds
// the same D pad to UI_MoveUp / UI_MoveDown -- the game's own collision, and every pad input that
// context leaves free falls through to something worse (D pad left/right are QH_MoveLeft/Right,
// R3 is Tag_Button and crouch, the shoulders carry a dozen actions between them).
//
// So the ACTION stays the game's and the KEY becomes ours: r6\input\CyberpunkVRPort_ScannerHud.xml
// declares ZoomIn / ZoomOut on IK_Insert and IK_Delete, which nothing in inputUserMappings.xml
// binds. This presses them. Down and up in one call: the game consumes raw input per frame, and a
// press that never releases would repeat by itself and defeat the step timer above.
static void SendZoomKey(bool zoomIn) {
    INPUT in[2] = {};
    // NOT VK_INSERT, and that is not a preference: this port's own ImGui overlay toggles on
    // VK_F10 OR VK_INSERT (Overlay/ImGuiOverlay.cpp), so zooming in was opening the overlay every
    // step. VK_END is the next key the game binds to nothing.
    const WORD vk = zoomIn ? VK_END : VK_DELETE;
    in[0].type = INPUT_KEYBOARD;
    in[0].ki.wVk = vk;
    in[0].ki.dwFlags = KEYEVENTF_EXTENDEDKEY;
    in[1] = in[0];
    in[1].ki.dwFlags = KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP;
    SendInput(2, in, sizeof(INPUT));
}

// THE HACK LIST, AS A KEY, and for the same reason the zoom above is one: the pad button the game binds
// the list to is the button it also binds the ZOOM to.
//
//     ZoomIn_Button   IK_Pad_DigitUp    IK_MouseWheelUp
//     UI_MoveUp       IK_Pad_DigitUp    IK_Q  IK_W  IK_MouseWheelUp  IK_Up
//
// (read out of r6\cache\inputUserMappings.xml, the merged file the game actually loads). So a synthetic
// D-pad press for the list also zoomed, which is what was reported. IK_Up / IK_Down are on UI_MoveUp /
// UI_MoveDown and on neither zoom mapping, so the arrows page the list and do nothing else. Nothing to
// declare: the game ships those keys on the action already.
//
// Extended-key flag for the same reason as the zoom's: the arrows ARE extended keys, and down+up in one
// call because the game consumes raw input per frame and a press that never releases repeats by itself.
static void SendListKey(bool up) {
    INPUT in[2] = {};
    in[0].type = INPUT_KEYBOARD;
    in[0].ki.wVk = up ? VK_UP : VK_DOWN;
    in[0].ki.dwFlags = KEYEVENTF_EXTENDEDKEY;
    in[1] = in[0];
    in[1].ki.dwFlags = KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP;
    SendInput(2, in, sizeof(INPUT));
}

static float ApplyStickDeadzone(float v, float dz) {
    float a = v < 0.0f ? -v : v;
    if (a < dz) return 0.0f;
    float s = v < 0.0f ? -1.0f : 1.0f;
    return s * (a - dz) / (1.0f - dz);
}

static void PublishMergedPacket(XINPUT_STATE* state) {
    static std::mutex mutex;
    static cvr::input::InputPacketSequence<XINPUT_GAMEPAD> packets;
    std::lock_guard lock(mutex);
    // Axes matter too: a swimming glide must publish its final zero, even
    // when no buttons/triggers changed and the real pad packet stayed fixed.
    state->dwPacketNumber=packets.Publish(state->Gamepad);
}

static uint16_t SenseSystemEvent(bool pressed, bool available) {
    static std::mutex mutex;
    static cvr::input::SenseSystemButton button;
    std::lock_guard lock(mutex);
    const auto event = button.Update(pressed, GetTickCount64(), available);
    static uint16_t logged = 0;
    if (event && !(logged & event)) {
        logged |= event;
        Log("XInput[PSVR2]: SystemButton mapped to %s.\n", event == 0x10 ? "Start/Pause" : "Back/Select");
    }
    return event;
}

DWORD WINAPI HookedXInputGetState(DWORD dwUserIndex, XINPUT_STATE* pState) {
    DWORD r = ERROR_DEVICE_NOT_CONNECTED;
    if (g_realXInputGetState) r = g_realXInputGetState(dwUserIndex, pState);

    if (!pState) return r;
    if (dwUserIndex != 0) return r;
    if (g_liveControls.xrXInputHook == 0) { SenseSystemEvent(false,false);cvr::quest::SuspendManualClueInput();cvr::swimming::ResetInput();cvr::ladder::ResetInput();return r; }

    VRControllerState vr{};
    if (!OpenXRManager::Get().GetControllerState(&vr)) { SenseSystemEvent(false,false);cvr::quest::SuspendManualClueInput();cvr::swimming::ResetInput();cvr::ladder::ResetInput();return r; }
    vr.buttons |= static_cast<uint16_t>(CyberpunkVR_TestPadButtons & 0xFFFF);
    if (CyberpunkVR_TestPadButtons & 0x10000) vr.leftGrip = 1.f;
    if (CyberpunkVR_TestPadButtons & 0x20000) vr.rightGrip = 1.f;
    if (const auto test = CyberpunkVR_TestPadLeft.load(std::memory_order_relaxed)) {
        const int32_t remaining = static_cast<int32_t>(uint32_t(test >> 32) - GetTickCount());
        if (remaining > 0 && remaining <= 10000) {
            vr.leftThumbX = static_cast<int16_t>(test & 0xFFFF) / 32767.f;
            vr.leftThumbY = static_cast<int16_t>((test >> 16) & 0xFFFF) / 32767.f;
        }
    }
    // PSVR2: the Sense controller's Create button, tap = Start, hold = Back (Hooks/Psvr2Input.hpp)
    vr.buttons |= SenseSystemEvent(vr.psvr2SystemPressed, vr.psvr2SystemAvailable && !cvr::vrui::CapturesInput());
    // LEFT-HANDED, THE CONTROLLERS TRADE ROLES HERE, once (Hooks/Handedness.hpp): from this line `vr` is what the
    // controllers mean to the game -- "right" the weapon hand, "left" the other one -- and `physical` is the two
    // controllers as they are, for what a hand does as a hand (its fingers, a ladder rung, its arm's cyberware, the
    // menu tab its grip pages toward). The scanner always uses the physical left hand.
    VRControllerState physical = vr;
    static uint64_t scannerPressesSeen=0;
    static bool scannerLatch=false;
    const uint64_t freshScannerPresses=physical.scannerPresses>=scannerPressesSeen ?
        physical.scannerPresses-scannerPressesSeen : 0;
    scannerPressesSeen=physical.scannerPresses;
    const bool leftHanded = g_liveControls.xrLeftHanded != 0;
    if (leftHanded) vr = cvr::input::Mirrored(vr, g_liveControls.xrLeftHandedBindings);
    // The hand's grip on what it reaches for presses the interaction button (Natives/Reach.hpp).
    if (cvr::reach::UseHeld()) vr.buttons |= 0x4000;
    cvr::input::DispatchCyberwareChord();

    if (r != ERROR_SUCCESS) {
        memset(pState, 0, sizeof(*pState));
        r = ERROR_SUCCESS;
    }
    if (vr.psvr2DpadShift) {
        // A physical/Steam XInput contribution must not reintroduce look after
        // the XR action snapshot suppressed it. Also cancel an in-flight snap.
        pState->Gamepad.sThumbRX = pState->Gamepad.sThumbRY = 0;
        vr.rightThumbX = vr.rightThumbY = 0;
        g_xinputSnapArmedDir = 0;
        g_xinputSnapPulseDir = 0;
        InterlockedExchange(&g_pendingSnapYawDeltaBits, 0);
    }
    // Exact, already-scanned manual clue only. Capture before scanner X remaps
    // and swallow the whole press so the newly revealed Take needs another X.
    if(cvr::quest::ConsumeManualClueButton(((vr.buttons|pState->Gamepad.wButtons)&0x4000)!=0,true)) {
        vr.buttons &= static_cast<uint16_t>(~0x4000);
        pState->Gamepad.wButtons &= static_cast<uint16_t>(~0x4000);
    }
    if(cvr::vrui::CapturesInput()){
        scannerLatch=false;
        pState->Gamepad={};g_lookStickX.store(0,std::memory_order_relaxed);
        cvr::swimming::ResetInput();cvr::ladder::ResetInput();
        for(int slot:{30,49,vrshared::kRightSecondaryBtn,vrshared::kLeftSecondaryBtn,vrshared::kRightStickClick,vrshared::kLeftGripPressed,vrshared::kRightTriggerAnalog,vrshared::kLeftTriggerAnalog})
            OpenXRManager::Get().SetSharedSlot(slot,0);
        NoteRightGrip(4,physical.rightGrip);
        PublishMergedPacket(pState);return r;
    }
    // THE VR MENU holds the triggers, grips and face buttons while it is open (Natives/VrMenu.cpp reads them
    // raw); the sticks, Start and the stick clicks stay the game's, so the player can still walk and turn.
    // The grips and buttons are taken out here, before anything below can act on them. The triggers stay
    // in `vr` because the hands' finger curl is drawn from their analog slots -- pointing and clicking
    // should look like it -- and are taken off the game's packet at the end instead.
    // Hand animation owns the physical controls, independently of the menu's gameplay capture.
    // Keep this snapshot before masking grips/buttons so a dropdown never forces an open hand.
    const VRControllerState handInput=physical;
    const bool menuCapture=cvr::vrmenu::CapturesButtons();
    if(menuCapture){
        vr.buttons&=static_cast<uint16_t>(0x00FF);vr.leftGrip=vr.rightGrip=0;
        physical.buttons&=static_cast<uint16_t>(0x00FF);physical.leftGrip=physical.rightGrip=0;
    }
    auto menuMask=[&]{
        if(!menuCapture)return;
        pState->Gamepad.bLeftTrigger=pState->Gamepad.bRightTrigger=0;
        pState->Gamepad.wButtons&=static_cast<uint16_t>(0x00FF);
    };
    const auto swim=cvr::swimming::UpdateInput(vr,vr.leftThumbY<-.2f || pState->Gamepad.sThumbLY<-6553);
    const bool inWater=cvr::swimming::Active();
    cvr::ladder::UpdateInput(physical);
    const bool onLadder=cvr::ladder::Active() && g_liveControls.xrLadderGripClimb!=0;

    // Buttons: OR (so a physical pad can still augment, and vice versa) -- except the two the port has
    // taken for itself, which are masked out here instead of reaching the pad:
    //
    //   RIGHT STICK CLICK (0x0080) is the physical reload's SLIDE RELEASE. The game had it on crouch,
    //   and crouch is the same stick pushed fully down (further below), so nothing is lost and a slide
    //   release no longer squats the player.
    //
    //   B (0x2000) is the physical reload's MAGAZINE DROP, which is what a thumb does on a real pistol.
    //   The game had it on DODGE, and a dodge on every magazine drop is not a compromise: it threw the
    //   player sideways in the middle of a reload. Dodge moves to a gesture of its own (see the dash
    //   block below) and is emitted from there, so the raw press means one thing again.
    // AND IT IS GAMEPLAY-ONLY, which the first version of this was not. Both of those buttons mean
    // something else outside gameplay, and taking them everywhere broke more than it fixed:
    //
    //   B is Exit_Button and ExitWheel_Button in menus, and ExitVehicle_Button in a car -- all three
    //   from the same mappings file. Masked unconditionally, there was no way to back out of a menu or
    //   to get out of a vehicle at all. A magazine drop only exists while a gameplay screen is up, so
    //   that is the only place the port needs the button.
    //
    //   IN A VEHICLE X IS THE EXIT, not the horn. Vehicle_Horn is X and ExitVehicle_Button is B, so
    //   the mask above had removed the only pad exit while leaving the horn -- exactly backwards for
    //   a seated player. X is taken here and re-emitted as B further down; B itself is left alone in a
    //   vehicle, both because it is the vanilla exit and because there is no physical reload in a car
    //   for the mask to protect.
    // B ONLY WHILE A WEAPON IS IN HAND, and that is a correction. The mask exists to protect the
    // physical reload's magazine drop -- and a magazine drop cannot happen with nothing to drop it
    // from. Taken on every gameplay screen instead, it also swallowed B where B is Exit_Button, so
    // the PHONE could not be closed: it is an overlay, not a menu, so g_menuModeValue stays 0 and
    // the mask applied. Holstered, B reaches the game again and closes it.
    //
    // (With a weapon actually drawn the phone still cannot be closed by B. Fixing that needs a
    // phone-open signal, and the game's own is the blackboard bool UI_ComDevice.ContactsActive --
    // which is what PhoneSystem.IsPhoneOpened reads. Not wired yet; this covers the common case.)
    // ...AND NOT WHILE AN OVERLAY OWNS B. The phone, the radio port and the vehicle list are not menus
    // -- menu mode stays 0 for all three -- so this mask applied and there was no way to close any of
    // them with a weapon in hand. The script publishes which of them is up (VRUiPopup); B then reaches
    // the game as Exit_Button, and the magazine drop is held back instead (see the publish below), so
    // the press that closes a popup cannot also eject a magazine.
    const bool uiPopupOpen = g_uiPopupOpen.load(std::memory_order_relaxed) != 0;
    const uint16_t kPortOwnedButtons =
        static_cast<uint16_t>(0x0080 | ((g_hasWeaponEquipped && !uiPopupOpen) ? 0x2000 : 0x0000));
    // X AND B BOTH, and B is the correction: letting the raw B through in a car looked more
    // forgiving and was the opposite. B is ExitVehicle_Button, so a stray press ejects the
    // player from a moving car -- which reads as being thrown across the street, not as a
    // button. Exit is a deliberate X press now, translated below; nothing else can eject.
    const bool keypadInput = cvr::input::KeypadInputActive();
    const bool gameplayScreen = (g_menuModeValue == 0) && !keypadInput;
    // The fingers follow grip, trigger and thumb in play, and in the lobby too: the main menu is a menu to the port,
    // but its world is walked with the hands in view, and the gesture only draws the fingers -- nothing reaches the
    // game from it.
    cvr::gestures::PublishInput(handInput.leftGrip,handInput.rightGrip,handInput.leftTrigger,handInput.rightTrigger,
        (handInput.buttons&0x0080)!=0,
        (gameplayScreen && !uiPopupOpen && g_bdActive.load(std::memory_order_relaxed)==0) || cvr::lobby::WorldShown());
    const bool mounted = g_isInVehicle;
    const int vehicleState=g_vehicleState.load(std::memory_order_relaxed);
    const auto vehicleButtons=cvr::input::VehicleButtonPolicy(mounted,vehicleState);
    uint16_t ownedNow = 0;
    if (gameplayScreen) ownedNow = vehicleButtons.owned ? vehicleButtons.owned : kPortOwnedButtons;
    pState->Gamepad.wButtons |=
        (vr.buttons & static_cast<uint16_t>(~ownedNow));

    // MENU-ONLY: the grips stand in for the shoulders (LB/RB page a menu's tabs). Never in gameplay -- there the
    // right grip is the hand-to-holster equip and the D-pad modifier, the left one takes the magazine, and RB/LB
    // are gameplay actions (LB the scanner) that would misfire on every reach. Menu state = the native menu-mode
    // hook OR the redscript world-map bridge flag (shared[81]): a world map does not always raise the native flag.
    // Worked out here, ahead of the vanilla pad below, because that path returns -- with it on (the default) a menu
    // never reached a translation placed after it.
    //
    // ONLY A SQUEEZE THAT STARTS INSIDE THE MENU. A grip already held when the menu opens is the hand's resting
    // pose -- the VR menu's map button is pressed with a gripped pointing hand -- and turned into RB/LB it paged the
    // hub off the map on its first frame (right grip: Character, left: Inventory), so the map button seemed to open
    // some other menu at random. Each grip arms once it is seen open in a menu.
    //
    // NOT IN THE LOBBY. The main menu is a menu to the game, but the lobby's world is walked like a level and its
    // panels and things read the controllers themselves: a left grip turned into LB reached the player there as
    // Vision_Hold_Button and opened the scanner on every squeeze.
    //
    // AND IN A BRAINDANCE, where the pad's shoulders are its own two buttons -- LB the editor (BdCameraToggle), RB
    // the layer (SwitchLayer) -- and a controller without shoulders had neither. Nothing else there wants a grip:
    // no weapon, no holster, and the VR menu's swipe is off in a braindance (Natives/VrMenu.cpp). The braindance
    // itself (InBraindance), not g_bdActive, which any scene that owns the camera raises.
    const bool inBraindance = InBraindance();
    bool menuOpenForGripShoulders = (g_menuModeValue != 0 || inBraindance) && !cvr::lobby::WorldShown();
    if (!menuOpenForGripShoulders && !cvr::lobby::WorldShown()) {
        if (float* sh = GetShotShared()) {
            if (reinterpret_cast<volatile uint32_t*>(sh)[81] != 0u) menuOpenForGripShoulders = true;
        }
    }
    uint16_t gripShoulders = 0;
    {
        static bool s_gripShoulderArmed[2] = {false, false};
        const float grips[2] = {physical.leftGrip, physical.rightGrip};
        for (int h = 0; h < 2; ++h) {
            if (!menuOpenForGripShoulders || menuCapture) s_gripShoulderArmed[h] = false;
            else if (grips[h] < 0.5f) s_gripShoulderArmed[h] = true;
        }
        if (s_gripShoulderArmed[1] && physical.rightGrip >= 0.7f) gripShoulders |= XINPUT_GAMEPAD_RIGHT_SHOULDER;
        if (s_gripShoulderArmed[0] && physical.leftGrip >= 0.7f) gripShoulders |= XINPUT_GAMEPAD_LEFT_SHOULDER;
    }

    // THE VANILLA PAD, WHILE A MENU OR A BRAINDANCE IS UP. See CyberpunkVR_InputDefaultInUi.
    //
    // Placed here because everything below this line is a port remap of one kind or another, so a
    // return is the whole implementation: the mask above is undone (it only ever removed bits), the raw
    // triggers and sticks are merged the way the blocks below would have merged the shaped ones, and
    // the shared slots the CET mods read are put down -- a gesture must not fire from a menu press, and
    // a slot left standing at 1 would keep firing after the menu closes.
    if (keypadInput || (CyberpunkVR_InputDefaultInUi != 0 &&
        (g_menuModeValue != 0 || inBraindance || g_bdActive.load(std::memory_order_relaxed) != 0))) {
        scannerLatch=false;
        pState->Gamepad.wButtons |= vr.buttons;
        // the grips' shoulders: a menu's and a braindance's, never another scene's or the keypad's
        if (!keypadInput && (inBraindance || g_bdActive.load(std::memory_order_relaxed) == 0))
            pState->Gamepad.wButtons |= gripShoulders;
        const BYTE ltUi = FloatToBYTE(vr.leftTrigger);
        const BYTE rtUi = FloatToBYTE(vr.rightTrigger);
        if (ltUi > pState->Gamepad.bLeftTrigger)  pState->Gamepad.bLeftTrigger  = ltUi;
        if (rtUi > pState->Gamepad.bRightTrigger) pState->Gamepad.bRightTrigger = rtUi;
        float leftX = vr.leftThumbX, leftY = vr.leftThumbY;
        if (keypadInput) {
            const float padX = pState->Gamepad.sThumbLX / 32767.f, padY = pState->Gamepad.sThumbLY / 32767.f;
            if (fabsf(padX) > fabsf(leftX)) leftX = padX;
            if (fabsf(padY) > fabsf(leftY)) leftY = padY;
            cvr::input::PublishKeypadStick(leftX, leftY, g_liveControls.xrLeftStickDeadzone);
            // The native world-hit hook moves in keypad pixels. Sending these
            // axes to the game's screen-space cursor too would apply them twice.
            leftX = leftY = 0;
            pState->Gamepad.sThumbLX = pState->Gamepad.sThumbLY = 0;
        }
        if (fabsf(leftX) > fabsf(pState->Gamepad.sThumbLX / 32767.0f))
            pState->Gamepad.sThumbLX = FloatToSHORT(leftX);
        if (fabsf(leftY) > fabsf(pState->Gamepad.sThumbLY / 32767.0f))
            pState->Gamepad.sThumbLY = FloatToSHORT(leftY);
        if (fabsf(vr.rightThumbX) > fabsf(pState->Gamepad.sThumbRX / 32767.0f))
            pState->Gamepad.sThumbRX = FloatToSHORT(vr.rightThumbX);
        // PUBLISHED FOR THE SCENE YAW. The camera composition needs the look axis on frames where
        // the game does nothing with it -- see g_sceneStickYawRad in PatchCamera.cpp. Taken after
        // the merge so it is the axis the game is being handed, controller or pad alike.
        g_lookStickX.store(pState->Gamepad.sThumbRX / 32767.0f, std::memory_order_relaxed);
        if (fabsf(vr.rightThumbY) > fabsf(pState->Gamepad.sThumbRY / 32767.0f))
            pState->Gamepad.sThumbRY = FloatToSHORT(vr.rightThumbY);
        // THE LOBBY, the main menu's world walked like a level (Lobby/MenuLobby.hpp WorldShown). The triggers are its
        // hands' (its menus and things read them from the controllers): reaching the game, the aim trigger only zoomed
        // the camera, the game's unarmed zoom. The right stick pushed fully down crouches, as in play (R3, the port's
        // crouch; the weapon hand's stick, mirrored with the controllers). And nothing moves the player until the lobby
        // is there to walk in: its curtain open and its intro over, when its menu comes up.
        if (cvr::lobby::WorldShown()) {
            pState->Gamepad.bLeftTrigger = 0;
            pState->Gamepad.bRightTrigger = 0;
            if (cvr::lobby::Curtain() > 0.0f || cvr::lobby::intro::Latest() != nullptr) {
                pState->Gamepad.sThumbLX = pState->Gamepad.sThumbLY = 0;
                pState->Gamepad.sThumbRX = pState->Gamepad.sThumbRY = 0;
                g_lookStickX.store(0.0f, std::memory_order_relaxed);
            } else if (cvr::input::AtFullTravel(vr.rightThumbY,
                           cvr::input::UseAnalogMovement(g_liveControls.xrMovementSpeedMode, false),
                           cvr::input::StickFullInput(g_liveControls.xrMaxInputThreshold), true)) {
                pState->Gamepad.sThumbRY = 0;
                pState->Gamepad.wButtons |= XINPUT_GAMEPAD_RIGHT_THUMB;
            }
        }
        // The discrete ones go down; the analog trigger slots keep their real value, because the hands'
        // finger curl is drawn from them and a menu is no reason for a fist.
        OpenXRManager::Get().SetSharedSlot(30, 0.0f);
        OpenXRManager::Get().SetSharedSlot(49, 0.0f);
        NoteRightGrip(5, physical.rightGrip);
        OpenXRManager::Get().SetSharedSlot(vrshared::kRightSecondaryBtn, 0.0f);
        OpenXRManager::Get().SetSharedSlot(vrshared::kLeftSecondaryBtn, 0.0f);
        OpenXRManager::Get().SetSharedSlot(vrshared::kRightStickClick, 0.0f);
        OpenXRManager::Get().SetSharedSlot(vrshared::kLeftGripPressed, 0.0f);
        OpenXRManager::Get().SetSharedSlot(vrshared::kRightTriggerAnalog, physical.rightTrigger);
        OpenXRManager::Get().SetSharedSlot(vrshared::kLeftTriggerAnalog, physical.leftTrigger);
        menuMask();
        PublishMergedPacket(pState);
        return r;
    }

    // ...and with the vanilla pad off (CyberpunkVR_InputDefaultInUi = 0), or on a world map that raised no menu flag.
    pState->Gamepad.wButtons |= gripShoulders;

    // Triggers: take the max so a physical squeeze isn't lost.
    // (VR melee block does NOT inject LT here: the gesture guard is STAT-driven — the CET weapon
    // mod sets IsBlocking/IsDeflecting directly, damageManager.script mitigates on those stats,
    // and the PSM Block state with its AimWalk/sprint debuffs is never entered. A physical left
    // trigger still reaches the game's own 'MeleeBlock' action through this merge as in flat.)
    BYTE lt = FloatToBYTE(vr.leftTrigger);
    BYTE rt = FloatToBYTE(vr.rightTrigger);
    // The VR left trigger reaches the gamepad LT (aim / zoom, melee block, VEHICLE BRAKE) unless
    // the smoking lighter has a claim on it, which is only true on foot with empty hands.
    //
    // The weapon test alone was too wide and cost the brake: no weapon is equipped while driving,
    // so LT did nothing at all in a vehicle -- LT did not work, and rightly so.
    // The lighter is not in anyone's hand behind a steering wheel, so the vehicle flag we already
    // maintain for VRIK settles it. CyberpunkVR_LtLighterGate=0 (an export, no ini key) drops the gate.
    // NOT WHILE A DEVICE CAMERA IS TAKEN OVER: V holds no weapon of her own in a camera or the q303
    // SniperNest, so the gate read "empty hands" and swallowed the nest's zoom (LT), 2026-11-12.
    const bool deviceTakeover = DeviceCamActive();
    const bool ltClaimedByLighter =
        CyberpunkVR_LtLighterGate != 0 && !g_hasWeaponEquipped && !g_isInVehicle && !deviceTakeover;
    if (!ltClaimedByLighter && lt > pState->Gamepad.bLeftTrigger)  pState->Gamepad.bLeftTrigger  = lt;
    // ...and not while that trigger is working a pump's fore-end: grip + trigger on the fore-end is the rack, and
    // the same squeeze reaching the game would aim down the sights halfway through it. The reload mod holds the
    // deadline up for as long as the hand is on the fore-end (SetVRLeftTriggerBlock); the raw value above is
    // still published, which is what it reads.
    if (GetTickCount64() < CyberpunkVR_LeftTriggerBlockUntil) pState->Gamepad.bLeftTrigger = 0;

    // ---- SCANNER: physical left hand to the left ear + fresh L3 -------------------------------
    // OpenXR owns the raw click before its D-pad/deferred-sprint conversion and publishes a
    // monotonic press count, so a short press between XInput polls is not lost.
    const int cyberwareKind=cvr::cyberware::SelectedKind();
    const bool physicalCyberware=((cyberwareKind>=1 && cyberwareKind<=3) || cyberwareKind==5) && g_menuModeValue==0 && !cvr::anim::IsHeadAimWeaponActive();
    const bool wireOwnsGrip=physicalCyberware && cyberwareKind==3 && !onLadder;
    const bool firearmTrigger=g_VRBind==4 && gameplayScreen && !uiPopupOpen && !g_isInVehicle &&
        g_hasWeaponEquipped && CyberpunkVR_WeaponClass>=1 && CyberpunkVR_WeaponClass<=4 && cyberwareKind!=4 &&
        !cvr::anim::IsHeadAimWeaponActive();
    static cvr::input::WeaponTrigger weaponTrigger;
    const bool fireHeld=weaponTrigger.Update(firearmTrigger,vr.rightTrigger);
    if(firearmTrigger)rt=fireHeld?255:0;
    s_weaponTriggerEnabled.store(firearmTrigger,std::memory_order_relaxed);
    s_weaponTriggerStamp.store(GetTickCount64(),std::memory_order_relaxed);
    cvr::cyberware::PublishGrips(wireOwnsGrip && physical.leftGrip>.5f,wireOwnsGrip && physical.rightGrip>.5f);
    bool scannerHold = false;
    const int scannerVision=cvr::input::ScannerVision();
    const bool scannerActive=scannerVision==1 && g_menuModeValue==0;
    {
        static cvr::input::ScannerRequest request;
        const bool enabled=CyberpunkVR_ScannerGesture!=0 && gameplayScreen && !menuCapture &&
            !cvr::lobby::WorldShown();
        if(!enabled) {
            scannerLatch=false;
        } else if(CyberpunkVR_ScannerToggle!=0) {
            if(freshScannerPresses&1)scannerLatch=!scannerLatch;
        } else {
            scannerLatch=false;
        }
        request.Update(scannerLatch,scannerVision,GetTickCount64());
        scannerHold=enabled && (CyberpunkVR_ScannerToggle!=0 ? scannerLatch : physical.scannerHeld);
    }

    // ---- SCANNER ON THE LB BUTTON, AS A TOGGLE ---------------------------------------------------
    //
    // The game's LB is Vision_Hold_Button, a HOLD. The ear gesture above has always been a toggle; a
    // button bound to LB (here the Frame bumper action, which a SteamVR binding can put on an Index
    // trackpad) went straight through as the raw hold. 2026-11-12, the user: the scanner should toggle.
    // One press opens, the next closes. The raw LB is taken out of the merge and asserted from the latch.
    // On foot the same request check as the gesture's releases a press the game did not take; in a
    // device takeover (the q303 SniperNest) PlayerStateMachine.Vision is not trusted to report the
    // device's scanner, so only a second press or leaving the takeover ends it.
    // Not in a vehicle and not in menus/braindance (those keep the vanilla LB; the latter never reach here).
    {
        static bool s_lbWas = false, s_btnLatch = false, s_wasTakeover = false;
        static cvr::input::ScannerRequest s_btnRequest;
        const bool eligible = CyberpunkVR_ScannerButtonToggle != 0 && gameplayScreen && !uiPopupOpen &&
            !g_isInVehicle && g_menuModeValue == 0 && !cvr::lobby::WorldShown();
        const bool lb = (vr.buttons & 0x0100) != 0;
        if (!eligible || deviceTakeover != s_wasTakeover) s_btnLatch = false;
        else if (lb && !s_lbWas) s_btnLatch = !s_btnLatch;
        s_lbWas = lb;
        s_wasTakeover = deviceTakeover;
        if (!deviceTakeover) s_btnRequest.Update(s_btnLatch, scannerVision, GetTickCount64());
        else { bool none = false; s_btnRequest.Update(none, 0, 0); }
        if (eligible) {
            pState->Gamepad.wButtons &= static_cast<uint16_t>(~0x0100);
            scannerHold = scannerHold || s_btnLatch;
        }
    }

    // ---- THE SCANNER, WORKED WITH THE HAND THAT RAISED IT ---------------------------------------
    //
    // WHAT THE GAME LISTENS FOR, read out of its own files rather than guessed:
    //
    //   quickhacks.swift registers UI_MoveUp, UI_MoveDown and UI_ApplyAndClose. inputContexts.xml
    //   resolves the first two to the mappings of the same name and the third to
    //   ApplyAndCloseQHackWidget; inputUserMappings.xml binds those to IK_Pad_DigitUp,
    //   IK_Pad_DigitDown and IK_Pad_X_SQUARE. hudManager.swift changes the scanner's TAB on
    //   DescriptionChange, which resolves to ToggleQHackDescription = IK_Pad_RightShoulder. The tag
    //   is Tag_Button = IK_Pad_RightThumb.
    //
    // So: the hack list is a D-pad, applying is X, the tab is RB, the tag is the right stick click --
    // and not one of those is reachable one-handed in VR. The D-pad only existed behind the
    // left-stick-click chord, which asks for the second hand and a thumb reach in the middle of a
    // gesture the other hand is already holding, and RB is never emitted in gameplay at all.
    //
    //   left stick up     -> IK_Up        page up the hack list   (NOT the D-pad: that is also the zoom)
    //   left stick down   -> IK_Down      page down
    //   X                 -> X            apply the selected hack
    //   right trigger     -> R3            tag the target
    //   right stick click -> RB            change the scanner tab
    //
    // THE TAP/HOLD SPLIT ON X IS DECIDED AT THE RELEASE, and it has to be. X is both the page-down
    // and the apply, so acting on the press would step the selection and then apply -- applying the
    // hack BELOW the one that was looked at, every single time. Held past the threshold the apply
    // fires and the release is swallowed; released before it, only the page-down fires.
    //
    // X AND Y ARE TAKEN OUT OF THE MERGE while the gesture is held, or the game sees their gameplay
    // meanings underneath -- X is Takedown_Button, PickUpBodyFromTakedown_Button and Reload_Button,
    // Y is SwitchItem_Button and WeaponWheel_Button. The right stick click is already the port's own
    // (the slide release) and never reaches the game, so it needs no mask.
    //
    // EDGE-TRIGGERED WITH A RE-ARM, like the dash: the listeners act on BUTTON_PRESSED, so a held bit
    // is one press and anything else would be inventing a repeat rate. A pulse already in flight is
    // deliberately NOT cut short when the gesture ends -- applying a hack closes the panel and the
    // hand relaxes in the same breath, and a swallowed apply is worse than a 90 ms D-pad tail.
    bool scanDpadUp = false, scanDpadDown = false, scanApply = false, scanTag = false, scanTab = false;
    {
        static bool     s_xWasDown     = false;
        static bool     s_r3WasDown    = false;
        static bool     s_rtWasDown    = false;
        static uint64_t s_upUntilMs = 0, s_downUntilMs = 0, s_applyUntilMs = 0,
                        s_tagUntilMs = 0, s_tabUntilMs = 0;

        const uint64_t now = GetTickCount64();
        const uint64_t pulseMs = (CyberpunkVR_ScannerPulseMs > 0)
                                     ? static_cast<uint64_t>(CyberpunkVR_ScannerPulseMs) : 90;
        // CyberpunkVR_ScannerApplyHoldMs is no longer read: the tap/hold split it timed existed only
        // while X was both the page-down and the apply. The knob stays exported so an existing
        // vrport.ini keeps parsing.

        if (scannerActive && CyberpunkVR_ScannerFaceNav != 0) {
            const bool xDown = (vr.buttons & 0x4000) != 0;
            const bool yDown = (vr.buttons & 0x8000) != 0;

            // APPLY IS A PLAIN PRESS OF X. The tap/hold split existed only because X had to be both
            // the page-down and the apply, and acting on the press would then have stepped the selection
            // and applied the hack BELOW the one being looked at, every single time. With the paging on
            // the stick there is nothing left to disambiguate.
            if (xDown && !s_xWasDown) s_applyUntilMs = now + pulseMs;
            s_xWasDown = xDown;

            // Y NO LONGER PAGES THE LIST. It stays masked below all the same: its gameplay meanings are
            // SwitchItem_Button and WeaponWheel_Button, and opening the weapon wheel over an open
            // scanner is not an improvement on doing nothing.
            (void)yDown;

            // The tag, on a travel the finger cannot chatter through: 0.65 to arm, 0.35 to release, so
            // one squeeze is one tag and a finger resting on the trigger is not a stream of them.
            if (CyberpunkVR_ScannerTriggerTag != 0) {
                const bool rtDown = s_rtWasDown ? (vr.rightTrigger > 0.35f)
                                                : (vr.rightTrigger > 0.65f);
                if (rtDown && !s_rtWasDown) s_tagUntilMs = now + pulseMs;
                s_rtWasDown = rtDown;
            } else {
                s_rtWasDown = false;
            }

            if (CyberpunkVR_ScannerStickTab != 0) {
                const bool r3Down = (vr.buttons & 0x0080) != 0;
                if (r3Down && !s_r3WasDown) s_tabUntilMs = now + pulseMs;
                s_r3WasDown = r3Down;
            } else {
                s_r3WasDown = false;
            }

            // Both face buttons belong to the scanner for as long as it is up.
            pState->Gamepad.wButtons &= static_cast<uint16_t>(~(0x4000 | 0x8000));
        } else {
            s_xWasDown  = false;
            s_r3WasDown = false;
            s_rtWasDown = false;
        }

        scanDpadUp   = (now < s_upUntilMs);
        scanDpadDown = (now < s_downUntilMs);
        scanApply    = (now < s_applyUntilMs);
        scanTag      = (now < s_tagUntilMs);
        scanTab      = (now < s_tabUntilMs);
    }

    // Publish whether the VR right trigger is held (shared[30]) so the CET melee mod can use it as the
    // power/strong modifier (hold = strong attack).
    OpenXRManager::Get().SetSharedSlot(30, (vr.rightTrigger > 0.5f) ? 1.0f : 0.0f);
    // ...and the same trigger as an ANALOG value, which a flag cannot stand in for: an action worked progressively
    // needs the whole travel, not the half-way mark. The physical reload rides a revolver's hammer on it.
    // THE GRIPS AND THE ANALOG TRIGGERS ARE THE HANDS' OWN, published per PHYSICAL controller whatever the
    // left-handed trade did to the buttons: every reader is a hand doing something -- a magazine, a lighter, a
    // ball, the wheel, a holster -- and one that wants the weapon hand's asks for it by handedness.
    OpenXRManager::Get().SetSharedSlot(vrshared::kRightTriggerAnalog, physical.rightTrigger);
    // Right grip is published as a BINARY pressed/not-pressed flag in shared[49] (a previously free
    // slot — [50]/[51]/[52] are owned by the camera-trace producer and we were stomping on them).
    // The CET hand-to-holster mod reads this + the IN-GAME wrist-to-hip distances the plugin
    // publishes from the live FK pose to decide whether reaching for a visual holster + a grip
    // press should equip / unequip the corresponding weapon.
    OpenXRManager::Get().SetSharedSlot(49, !onLadder && !wireOwnsGrip && physical.rightGrip > 0.5f ? 1.0f : 0.0f);
    NoteRightGrip(menuCapture && handInput.rightGrip > 0.5f ? 6 : physical.rightGrip <= 0.5f ? 0 : onLadder ? 1 : wireOwnsGrip ? 2 : 3,
                  handInput.rightGrip);
    // LEFT hand, for the smoking mod: the lighter is ignited by the left trigger and the cigarette
    // is taken to and from the mouth with either grip, so it needs all three. Only the right grip
    // was ever published; the left pair had no channel at all, and the CET bridge was reading [67]
    // and [68] on the strength of a map comment that called them free. They are not: [67] carries
    // the hand-sample millisecond stamp and [68] a QPC timestamp, both far above any threshold, so
    // the lighter read as permanently at full trigger and the left grip as permanently held.
    OpenXRManager::Get().SetSharedSlot(vrshared::kLeftTriggerAnalog, physical.leftTrigger);
    OpenXRManager::Get().SetSharedSlot(vrshared::kLeftGripPressed,
                                       !onLadder && !wireOwnsGrip && physical.leftGrip > 0.5f ? 1.0f : 0.0f);
    // Face buttons B / Y as flags, for gestures that want a button (the physical reload drops the magazine on
    // B). Y still reaches the game through the merge above; B no longer does -- it is the port's now, so a
    // magazine drop cannot also dodge.
    // THE MAGAZINE DROP, AND THE SECOND AFTER A POPUP CLOSES.
    //
    // This slot is what the reload module reads for the drop, so blocking it here is what makes the
    // close press harmless: while an overlay is up B belongs to the overlay, and for a moment after it
    // goes the next press of a spammed close cannot eject a magazine either,
    // with the window as a live key rather than a constant.
    {
        const unsigned long long closedMs = g_uiPopupClosedMs.load(std::memory_order_relaxed);
        const int32_t blockMs = (CyberpunkVR_PopupMagBlockMs > 0) ? CyberpunkVR_PopupMagBlockMs : 0;
        const bool justClosed = closedMs != 0ull &&
                                (GetTickCount64() - closedMs) < static_cast<unsigned long long>(blockMs);
        const bool magBlocked = uiPopupOpen || justClosed;
        OpenXRManager::Get().SetSharedSlot(vrshared::kRightSecondaryBtn,
                                           (!magBlocked && (vr.buttons & 0x2000)) ? 1.0f : 0.0f);
    }
    OpenXRManager::Get().SetSharedSlot(vrshared::kLeftSecondaryBtn,  (vr.buttons & 0x8000) ? 1.0f : 0.0f);
    // The right stick click, which the merge above deliberately does NOT pass on: the physical reload uses it as
    // the slide release. Published as a flag; the Lua side takes the rising edge.
    // The scanner used to hold this click and the publish was suppressed while it did. It fires on
    // the left grip now, so the click means one thing again and needs no guard.
    OpenXRManager::Get().SetSharedSlot(vrshared::kRightStickClick,
                                       (vr.buttons & 0x0080) ? 1.0f : 0.0f);
    // One switch for the whole port. The launcher's DEBUG checkbox already gates the plugin's own
    // chatter; republishing it here lets the CET bridges obey it too, live, without each of them
    // growing a setting of its own that nobody remembers to turn off.
    OpenXRManager::Get().SetSharedSlot(vrshared::kDebugLog, g_verboseLog ? 1.0f : 0.0f);
    // Melee RT IMPULSE (shared[29] = a frame countdown the CET mod raises on a detected VR swing): tap RT
    // so the game enters its NATIVE melee-attack state (full native damage/combo/numbers/markers), then
    // count it down. Otherwise merge the physical trigger into RT normally (guns shooting / held attack).
    float meleeImpulse = OpenXRManager::Get().GetSharedSlot(29);
    if (meleeImpulse > 0.5f) {
        pState->Gamepad.bRightTrigger = 255;
        OpenXRManager::Get().SetSharedSlot(29, meleeImpulse - 1.0f);
    } else {
        if (rt > pState->Gamepad.bRightTrigger) pState->Gamepad.bRightTrigger = rt;
    }
    // THE PORT'S OWN SAY OVER THE TRIGGER, last, so it beats both the physical pad and the merge above.
    //   1 = SWALLOW it. A revolver with its cylinder swung out has nothing under the hammer and must not fire, and
    //       this is the only honest way to say so: the alternative tried first was emptying the magazine behind the
    //       game's back, and putting it back cost the player real ammunition out of the reserve every time.
    //   2 = PRESS it fully. A hammer already cocked lets go under a touch -- single action -- so the port decides
    //       where in the travel the shot falls instead of the game's own threshold.
    const float trgMode = OpenXRManager::Get().GetSharedSlot(vrshared::kTriggerOverride);
    if (trgMode > 1.5f)      pState->Gamepad.bRightTrigger = (!firearmTrigger || fireHeld)?255:0;
    else if (trgMode > 0.5f) pState->Gamepad.bRightTrigger = 0;

    // AND NOTHING FIRES WHILE THE WEAPON IS IN THE LEFT HAND. Part of the feature: the right
    // hand has let go of the gun, so a trigger under that finger is pulling on nothing. Placed after
    // the override above because "cannot fire at all" outranks a cocked hammer's early break.
    if (CyberpunkVR_CarryLeft != 0) pState->Gamepad.bRightTrigger = 0;
    // Raw triggers were published above for strong VR swings. Do not also
    // launch a vanilla melee animation/attack from the same held trigger.
    // The selected left-arm launcher uses LT to enter its native firing state.
    // Publish raw RT above for free-hand gestures; it must not fire the launcher.
    // NONE OF THE THREE WHILE A DEVICE CAMERA IS TAKEN OVER: V's own hands hold nothing there, and the
    // q303 SniperNest fires its rifle on RT and zooms on LT (2026-11-12: RT never reached the game).
    if(physicalCyberware && !deviceTakeover)pState->Gamepad.bRightTrigger=0;
    if(cyberwareKind==4 && gameplayScreen && !g_isInVehicle && !deviceTakeover) {
        pState->Gamepad.bRightTrigger=FloatToBYTE(vr.leftTrigger);
        pState->Gamepad.bLeftTrigger=0;
    } else if(g_VRBind==4 && gameplayScreen && !g_isInVehicle && !g_hasWeaponEquipped && !deviceTakeover) {
        // Empty-hand RT is a finger gesture, never the game's draw-weapon action.
        pState->Gamepad.bRightTrigger=0;
    }

    // THE SCANNER'S TRIGGER IS A TAG AND NOT A SHOT. While the gesture is held the trigger marks the
    // target (Tag_Button, emitted further down), and a tag that also fires a round is a tag nobody can
    // use where tagging matters: it spends real ammunition and announces the player. Zeroed AFTER the
    // override above so the scanner wins over a cocked hammer, which costs nothing -- one of them wants
    // the left hand at the ear and the other wants it at the weapon, so they cannot both be true.
    if (scannerActive && CyberpunkVR_ScannerTriggerTag != 0) pState->Gamepad.bRightTrigger = 0;

    // DUAL WIELDING: the left trigger fires the left gun. The game has ONE fire action for both guns and
    // picks the gun through its ShootState machine (alternating on release, both while aiming); the
    // CyberpunkVRPort_DualWield script routes that machine from the two physical triggers instead, and only
    // while kDualWieldActive says this merge is on. Here both triggers become the one fire action, so the
    // left trigger no longer aims.
    // EACH GUN BREAKS WHERE THE OTHER DOES: the two-stage trigger a lone firearm has (Hooks/WeaponTrigger.hpp), per
    // physical hand. The hands are published with the switch, and the script routes the shot by them.
    // A NEW SQUEEZE HOLDS THE ACTION RELEASED FOR A MOMENT. The routing has to reach the other gun before the
    // press edge does: a semi-automatic fires on the edge, and the gun that sees it while the other hand still
    // owns the shot never fires. With the other trigger already held there would be no edge at all.
    const bool dualWield = CyberpunkVR_DualWieldInput != 0 && GetTickCount64() < CyberpunkVR_DualWieldUntil &&
        gameplayScreen && !uiPopupOpen && !g_isInVehicle && g_hasWeaponEquipped && !menuCapture && !scannerActive;
    {
        static bool s_dualLeft = false, s_dualRight = false;
        static uint64_t s_dualHoldUntil = 0;
        static cvr::input::WeaponTrigger s_dualBreak[2];
        const bool left = s_dualBreak[0].Update(dualWield, physical.leftTrigger);
        const bool right = s_dualBreak[1].Update(dualWield, physical.rightTrigger);
        if ((left && !s_dualLeft) || (right && !s_dualRight))
            s_dualHoldUntil = GetTickCount64() + static_cast<uint64_t>((std::max)(0, CyberpunkVR_DualWieldPressDelayMs));
        s_dualLeft = left;
        s_dualRight = right;
        if (dualWield) {
            pState->Gamepad.bLeftTrigger = 0;
            const BYTE fire = (left || right) ? BYTE{255} : pState->Gamepad.bRightTrigger;
            pState->Gamepad.bRightTrigger = GetTickCount64() < s_dualHoldUntil ? 0 : fire;
        }
        OpenXRManager::Get().SetSharedSlot(vrshared::kDualWieldActive,
            dualWield ? static_cast<float>(1 | (left ? vrshared::kDualWieldLeftFires : 0) | (right ? vrshared::kDualWieldRightFires : 0)) : 0.0f);
    }

    // Left stick = locomotion (always merged when magnitude exceeds the
    // physical pad's so the game uses our values).
    const bool analogMovement=cvr::input::UseAnalogMovement(g_liveControls.xrMovementSpeedMode,g_isInVehicle);
    const float fullInput=cvr::input::StickFullInput(g_liveControls.xrMaxInputThreshold);
    const float scannerLy=ApplyStickDeadzone(vr.leftThumbY,0.12f);
    float lx=analogMovement?cvr::input::AnalogAxis(vr.leftThumbX,g_liveControls.xrLeftStickDeadzone,fullInput)
                           :ApplyStickDeadzone(vr.leftThumbX,0.12f);
    float ly=analogMovement?cvr::input::AnalogAxis(vr.leftThumbY,g_liveControls.xrLeftStickDeadzone,fullInput):scannerLy;

    // HOW FAR THE STICK IS ACTUALLY PUSHED, kept before the quantiser below rewrites it. The gesture
    // that means "to the stop" -- the sprint detent further down -- has to read the player's own
    // deflection: after quantising, every forward push is exactly 1.0, so a detent tested on the
    // output would sprint on every single step.
    // THE HACK LIST ON THE LEFT STICK, UP AND DOWN, TO THE STOP.
    //
    // The left hand is the one that raised the scanner, so its stick is under the thumb that is already
    // there. Reaching across to the face buttons in the middle of a held gesture is the very thing the
    // one-handed scanner exists to avoid, which is why X and Y no longer page the list at all.
    //
    // HERE, AND NOT LOWER DOWN. The sprint gesture below reads the RAW deflection (lyDetent, taken on the
    // next line) and asserts L3 at full forward, so a push meant to page the list would have sprinted
    // instead. Taking the axis after that point is too late; taking it here is seen by the sprint, by the
    // detent quantiser and by the movement the game receives.
    //
    // AND THE AXIS IS ONLY TAKEN AT THE STOP. Below the threshold the stick still walks -- the same trade
    // the sprint and the crouch make in this file, "a partial push is left as the game's normal jog" --
    // so a scanning player can still move.
    //
    // EDGE-TRIGGERED WITH A RE-ARM, like every other gesture here: the stick must come back below the
    // re-arm before it can fire again, so a held stick is one page rather than a repeat rate we invented.
    //
    // AND IT SENDS A KEY, NOT A D-PAD PRESS. IK_Pad_DigitUp/Down are on UI_MoveUp/UI_MoveDown AND on
    // ZoomIn_Button/ZoomOut_Button, so a synthetic D-pad press paged the list and zoomed at the same
    // time. See SendListKey for the mappings this was read out of.
    if (scannerActive && CyberpunkVR_ScannerFaceNav != 0 && CyberpunkVR_ScannerStickNav != 0 &&
        !g_isInVehicle && (g_menuModeValue == 0)) {
        static int s_navArmedDir = 0;

        float navFire = CyberpunkVR_ScannerStickNavFire;
        if (!(navFire > 0.05f) || navFire > 1.0f) navFire = 0.90f;
        float navRearm = CyberpunkVR_ScannerStickNavRearm;
        if (!(navRearm >= 0.0f) || navRearm >= navFire) navRearm = navFire * 0.55f;

        int navDir = 0;
        if (scannerLy > navFire) navDir = +1;
        else if (scannerLy < -navFire) navDir = -1;

        if (fabsf(scannerLy) < navRearm) s_navArmedDir = 0;
        if (navDir != 0 && navDir != s_navArmedDir) {
            s_navArmedDir = navDir;
            SendListKey(navDir > 0);
        }

        if (navDir != 0) ly = 0.0f;   // at the stop the axis is the list's, not the legs'
    }

    const float lyDetent = ly==0 ? 0 : (analogMovement?vr.leftThumbY:ly);

    // ONE SPEED PER PUSH. The pad's analogue magnitude is the odd one out in this game: the keyboard
    // binds the same axis at val="1.0", so W runs, sprint is its own key and walk is its own toggle. A
    // thumbstick on a headset is the worst place for an analogue speed -- it rides every tremor, and a
    // push meant as a run comes out as a walk. So past the deadzone the magnitude is constant and only
    // the DIRECTION comes from the stick.
    //
    // Scaled as a 2D vector, not per axis: normalising the components separately would send (1,1) on a
    // diagonal, which is sqrt(2) of the speed sideways-forward -- the classic diagonal-is-faster bug.
    //
    // NOT IN A VEHICLE. There the left stick is not locomotion at all: X is steering, Y is lean, and Y
    // is what the shoot-while-driving throttle trim integrates -- a quantised +/-1 would take the
    // throttle from idle to floored in one frame.
    {
        static bool s_walking = false;
        const float mag = sqrtf(lx * lx + ly * ly);
        if (!analogMovement && CyberpunkVR_MoveTiers != 0 && !g_isInVehicle && mag > 1e-4f) {
            float outMag = 1.0f;
            float walkMax = CyberpunkVR_MoveWalkMax;
            if (!(walkMax > 0.0f) || walkMax > 0.95f) walkMax = 0.0f;   // 0 = no band
            if (walkMax > 0.0f) {
                float margin = CyberpunkVR_MoveWalkMargin;
                if (!(margin >= 0.0f) || margin > 0.30f) margin = 0.06f;
                // Wider on the way out than on the way in, so the boundary cannot chatter.
                s_walking = (mag <= (s_walking ? walkMax + margin : walkMax));
                if (s_walking) {
                    outMag = CyberpunkVR_MoveWalkMag;
                    if (!(outMag > 0.05f) || outMag > 1.0f) outMag = 0.35f;
                }
            }
            const float k = outMag / mag;
            lx *= k;
            ly *= k;
        } else if (mag <= 1e-4f || analogMovement) {
            s_walking = false;   // released: the next push decides again from scratch
        }
    }

    if (fabsf(lx) > fabsf(pState->Gamepad.sThumbLX / 32767.0f)) pState->Gamepad.sThumbLX = FloatToSHORT(lx);
    if (fabsf(ly) > fabsf(pState->Gamepad.sThumbLY / 32767.0f)) pState->Gamepad.sThumbLY = FloatToSHORT(ly);
    // Swimming gestures enter the player's native action reader. XInput
    // remains the physical controller source and does not duplicate them.

    // WHEEL STEERING. Controller angle changes drive left stick X immediately;
    // the arm's visual grab blend does not attenuate steering. Releasing the last
    // hand centers the synthetic input while the arm blends back. ASSIGNMENT, not max():
    // the wheel drives the car, and
    // a thumb still resting on the stick must not fight it. Y is left alone (throttle and brake are the
    // triggers).
    //
    // ONE DEPARTURE from the upstream version, and it is about the 110 ms AFTER you leave a car: his
    // condition is the grab blends alone, and those fade out over kReleaseSec rather than dropping, so
    // for a tenth of a second on foot the left stick's X was still being ASSIGNED the (now zero)
    // steering -- i.e. no strafing right after stepping out of a vehicle. Gated on driving as well, the
    // fade still does its job where it was meant to (releasing the grip while still at the wheel) and
    // the stick comes back the instant the seat is empty.
    {
        const float bR = cvr::anim::g_wheelBlendRight.load(std::memory_order_relaxed);
        const float bL = cvr::anim::g_wheelBlendLeft.load(std::memory_order_relaxed);
        if (g_isDriving.load(std::memory_order_relaxed) && (bR > 0.01f || bL > 0.01f)) {
            float steer = cvr::anim::WheelSteerInput();
            if (steer >  1.0f) steer =  1.0f;
            if (steer < -1.0f) steer = -1.0f;
            pState->Gamepad.sThumbLX = FloatToSHORT(cvr::anim::WheelGamepadSteer(steer));
        }
    }

    // HORN. A hand laid on the middle of the wheel = pad X, which is what the game's own Vehicle_Horn
    // mapping listens to (r6\config\inputUserMappings.xml: IK_Pad_X_SQUARE / IK_Z). Level-triggered,
    // so the horn sounds for as long as the hand stays on the hub.
    //
    // Gated on g_isDriving here as well as in the pose hook: the mask is only rewritten inside a fresh
    // VRIK solve, and a mask left raised by a solve that stopped must not press X on foot, where the
    // same button is a gameplay action (reload).
    // THE HORN HAS NO BUTTON ANY MORE, and that is deliberate rather than a regression. Vehicle_Horn
    // is X, and X is now how the player gets out of the car -- so a hand resting on the wheel hub
    // would eject them. The gesture and its mask stay wired and measured; what is missing is a button
    // that is free while seated, and there is not one. xr_wheel_horn therefore defaults off, and this
    // block is where a replacement goes if one is found.
    if (g_isDriving.load(std::memory_order_relaxed) && g_liveControls.xrWheelHorn != 0) {
        if (cvr::anim::g_wheelHornMask.load(std::memory_order_relaxed) != 0) {
            CVR_DIAGNOSTIC(++CyberpunkVR_DebugWheelHornGestures);   // counted, not pressed
        }
    }

    // DRIVING WITH A GUN OUT. Vanilla puts the throttle and the trigger finger on the same hand: the
    // throttle is RT (Acceleration_Axis) and firing your own weapon from the driver seat is RB
    // (VehicleDriverCombatRangedAttack_Button) -- both from the game's own inputUserMappings.xml. That
    // split works with a thumb and a finger on one pad; it does not work when the right hand is holding
    // the gun and the left is on the wheel.
    //
    // So while a weapon is equipped in the driver seat:
    //   * the right trigger becomes FIRE (pressed through as RB),
    //   * the throttle is LATCHED at whatever it was the instant the weapon came out, so the car keeps
    //     rolling with nothing holding RT,
    //   * the left stick's Y TRIMS that latched throttle (forward = more, back = less).
    // Holstering ends all three and the trigger is the throttle again, no state left behind.
    //
    // The trim axis is CONSUMED (zeroed, and the sprint gesture on it suppressed): in a vehicle stick Y
    // is lean/rock and its CLICK is Vehicle_Autodrive, which our full-forward gesture asserts as L3 --
    // a throttle trim that handed the car to the autopilot at the top of its travel would be worse than
    // no trim at all. Stick X is untouched, so stick steering still works for anyone not holding the
    // wheel.
    bool vehGunMode = false;
    {
        static bool  s_vehGunPrev = false;
        static float s_vehThrottle = 0.0f;
        static LARGE_INTEGER s_vehGunQpc = {};

        vehGunMode = (g_isDriving.load(std::memory_order_relaxed) && g_hasWeaponEquipped
                      && g_liveControls.xrVehicleGunTrigger != 0);
        if (vehGunMode) {
            LARGE_INTEGER now, freq;
            QueryPerformanceCounter(&now);
            QueryPerformanceFrequency(&freq);
            float dt = 0.0f;
            if (s_vehGunPrev && s_vehGunQpc.QuadPart != 0 && freq.QuadPart > 0) {
                dt = static_cast<float>(static_cast<double>(now.QuadPart - s_vehGunQpc.QuadPart)
                                        / static_cast<double>(freq.QuadPart));
            }
            if (dt < 0.0f) dt = 0.0f;
            if (dt > 0.10f) dt = 0.10f;   // a hitch must not slam the throttle
            s_vehGunQpc = now;

            // Freeze on the way in: whatever the trigger (ours or a physical pad's) was asking for on
            // the last frame before the weapon appeared is the speed the car holds.
            if (!s_vehGunPrev) s_vehThrottle = pState->Gamepad.bRightTrigger / 255.0f;

            const float trimRate = g_liveControls.xrVehicleThrottleTrim > 0.0f
                                 ? g_liveControls.xrVehicleThrottleTrim : 0.5f;
            s_vehThrottle += ly * trimRate * dt;
            if (s_vehThrottle < 0.0f) s_vehThrottle = 0.0f;
            if (s_vehThrottle > 1.0f) s_vehThrottle = 1.0f;

            // Assignment, not max(): the trigger is the gun's now, and the value it reports must not
            // leak back into the throttle it no longer drives.
            pState->Gamepad.bRightTrigger = static_cast<BYTE>(s_vehThrottle * 255.0f + 0.5f);
            pState->Gamepad.sThumbLY = 0;                                   // consumed by the trim
            // RB = ranged attack. THE PORT'S TRIGGER OVERRIDE STILL DECIDES, because in this mode the
            // shot leaves through RB and not through the trigger byte the override was written for: a
            // revolver with its cylinder swung out would otherwise fire quite happily from the driver
            // seat, which is the one thing that channel exists to prevent. 1 = swallow, 2 = force.
            const bool fireHeld = (trgMode > 1.5f) ? true
                                : (trgMode > 0.5f) ? false
                                : (vr.rightTrigger > 0.5f);
            if (fireHeld) pState->Gamepad.wButtons |= 0x0200;
        }
        s_vehGunPrev = vehGunMode;
    }

    // Left stick pushed near FULL forward => SPRINT. A partial push is left as the
    // game's normal jog; only "to the stop" sprints. CP2077 sprint is the left-stick
    // click (L3), so we just assert L3 while the stick is forward past the threshold
    // -- no more clicking the stick. Level-triggered (held while past the threshold)
    // mirrors physically holding L3: correct for hold-to-sprint, and toggle-sprint
    // auto-cancels on slow-down so it stays in sync as well.
    // THE RAW DEFLECTION, not the quantised output -- see lyDetent above.
    //
    // NOT IN A VEHICLE, which is a bug found while reading the game's mappings: Vehicle_Autodrive is
    // IK_Pad_LeftThumb, the very L3 this gesture asserts, so pushing the left stick to the stop in a
    // car was handing it to the autopilot. The vehicle test subsumes the gun-mode one that used to be
    // here, that being a vehicle state as well.
    //
    // NOT OUT OF A CROUCH either. A sneaking player holds the stick forward like anyone else, and
    // standing them up to sprint is the opposite of what they asked for. The state comes from the
    // game's own locomotion state machine (gamePSMLocomotionStates, published by the VRIK CET mod);
    // negative means nobody told us, and then there is no gate rather than a guess.
    //
    // AND IT HAS TO BE HELD. See CyberpunkVR_SprintHoldMs.
    bool wantSprint = false;
    {
        static double s_detentMs = 0.0;
        static double s_phaseMs = 0.0;    // where we are in the press/release square wave
        static double s_askMs = 0.0;      // how long we have been asking for a state we cannot reach
        static bool   s_gaveUp = false;
        static LARGE_INTEGER s_sprintQpc = {};
        LARGE_INTEGER nowS, freqS;
        QueryPerformanceCounter(&nowS);
        QueryPerformanceFrequency(&freqS);
        double dtMs = 0.0;
        if (s_sprintQpc.QuadPart != 0 && freqS.QuadPart > 0) {
            dtMs = static_cast<double>(nowS.QuadPart - s_sprintQpc.QuadPart) * 1000.0
                 / static_cast<double>(freqS.QuadPart);
        }
        s_sprintQpc = nowS;
        if (dtMs < 0.0) dtMs = 0.0;
        if (dtMs > 100.0) dtMs = 100.0;   // a hitch must not satisfy the whole window by itself

        const int loco = g_VRLocomotionState;
        // Crouch, and the two states that ARE a crouch (the game's own sprint-from-crouch and
        // dodge-from-crouch). Slide is deliberately not here: it is entered FROM a sprint and is over
        // in a moment, so blocking there would only interrupt the player's own momentum.
        const bool crouched = (loco == 1 || loco == 12 || loco == 13);
        // CROUCH SPRINT IS A REAL STATE (PSMLocomotionStates.CrouchSprint = 12), so by default the
        // detent asks for a sprint from a crouch too and the game decides what that means. The knob
        // puts the old outright block back.
        const bool crouchBlocks = ((CyberpunkVR_SprintFromCrouch == 0) && crouched) ||
            OpenXRManager::Get().PhysicalCrouchRequested();

        const bool fullForward=cvr::input::AtFullTravel(lyDetent,analogMovement,fullInput);
        const bool detent = fullForward && !g_isInVehicle && !crouchBlocks;
        if (detent) s_detentMs += dtMs; else s_detentMs = 0.0;
        const double holdMs = (CyberpunkVR_SprintHoldMs >= 0) ? CyberpunkVR_SprintHoldMs : 200;
        bool want = detent && (s_detentMs >= holdMs);

        // SPRINT ON A CLICK (xr_sprint_mode=1, beside the stick held at the stop): a click of the
        // left stick starts the sprint and it lasts until the stick is let go -- the game's own toggle on a pad (its L3
        // is ToggleSprint: a second press does not end a sprint, only the stick coming back does), only kept: after a
        // dash or a jump it comes back by itself, through the same hold below. A click made standing still waits
        // CLICK_WAIT_MS for the push; once pushed, the stick at rest for CLICK_REST_MS ends it. The clicks are COUNTED
        // by the OpenXR loop (CyberpunkVR_LeftStickClicks): its one-frame L3 press can fall between two polls of this
        // hook. That press still reaches the game as it always did. In a vehicle the click is the autopilot's.
        {
            constexpr double CLICK_WAIT_MS = 600.0;
            constexpr double CLICK_REST_MS = 100.0;
            static long s_clicksSeen = -1;
            static bool s_clickSprint = false;
            static bool s_pushedSinceClick = false;
            static double s_restMs = 0.0;
            const long clicks = CyberpunkVR_LeftStickClicks;
            const bool clicked = s_clicksSeen >= 0 && clicks != s_clicksSeen;
            s_clicksSeen = clicks;
            if (g_liveControls.xrSprintMode == 1) {
                // pushed as far as the game sprints from: any push in fixed speed (it reaches the game at full
                // magnitude), the stop in analog movement (the game wants 0.9 of travel)
                const bool pushed = analogMovement ? fullForward : (lx * lx + ly * ly) > 1e-4f;
                if (clicked && !s_clickSprint) {
                    s_clickSprint = true;
                    s_pushedSinceClick = false;
                    s_restMs = 0.0;
                }
                if (pushed) {
                    s_pushedSinceClick = true;
                    s_restMs = 0.0;
                } else {
                    s_restMs += dtMs;
                }
                if (g_isInVehicle || crouchBlocks || s_restMs > (s_pushedSinceClick ? CLICK_REST_MS : CLICK_WAIT_MS))
                    s_clickSprint = false;
                want = s_clickSprint && pushed;
            } else {
                s_clickSprint = false;
            }
        }

        // ---- L3 IS A TOGGLE, SO DRIVE IT WITH EDGES -------------------------------------------
        //
        // ToggleSprint_Button (the game's own mapping) reacts to a PRESS, not to a held button. Held
        // down, it fires once -- and when the game leaves the sprint state by itself (a dash, a jump,
        // a mantle, stamina) there is no second edge, so the sprint never comes back until the player
        // releases the stick and pushes again. That was the reported dash bug.
        //
        // So: while what we want and what the game is disagree, pulse. When they agree, stop pressing.
        // THE SPRINT STATE MACHINE, not the blackboard. Measured: PlayerStateMachine.Locomotion stays
        // at Default through a player sprint, so a loop waiting for Sprint(2) never saw its target and
        // kept toggling its own sprint back off -- which is why a held detent produced a stutter
        // standing and nothing but a stand-up from a crouch. SprintEvents is the sprint.
        // HELD, NOT PULSED. Measured twice: ten press/release edges never entered the sprint state,
        // while the level-held button this port shipped with always did. The game wants the button
        // down. What it will NOT do is re-enter a sprint it has just left while the button is already
        // down -- so the only thing the feedback is used for is a short RELEASE, which makes the next
        // hold an edge. That is the dash case, and nothing else needs to change.
        const int sprintFlag = g_VRSprintActive;
        const bool haveFeedback = (sprintFlag >= 0);
        const bool sprinting = haveFeedback && (sprintFlag != 0);

        if (swim.water) {
            wantSprint=cvr::swimming::SprintInput(want);
            s_phaseMs=s_askMs=0.0;s_gaveUp=false;
        } else if (!want) {
            wantSprint = false;
            s_phaseMs = 0.0;
            s_askMs = 0.0;
            s_gaveUp = false;
        } else if (!haveFeedback || sprinting || s_gaveUp) {
            // Hold. This is the whole mechanism: sprinting -> keep holding; no feedback at all -> hold,
            // because that is what worked before any of this; given up -> hold, for the same reason.
            wantSprint = true;
            s_phaseMs = 0.0;
        } else {
            // Wanted, and the game is NOT in the sprint state: hold, but let go briefly so the next
            // hold is a fresh edge. Mostly-held by design -- the release is the short half.
            s_askMs += dtMs;
            const double giveUp = (CyberpunkVR_SprintGiveUpMs > 0) ? CyberpunkVR_SprintGiveUpMs : 1500;
            if (s_askMs > giveUp) {
                s_gaveUp = true;      // stop blipping; the hold above takes over
                wantSprint = true;
            } else {
                double hold = (CyberpunkVR_SprintPulseMs > 0) ? CyberpunkVR_SprintPulseMs : 240;
                double gap  = (CyberpunkVR_SprintGapMs   > 0) ? CyberpunkVR_SprintGapMs   : 60;
                s_phaseMs += dtMs;
                const double period = hold + gap;
                while (s_phaseMs >= period) s_phaseMs -= period;
                wantSprint = (s_phaseMs < hold);
            }
        }

    }
    // Published for the snap-event machinery: DURING SPRINT the game RATE-LIMITS heading
    // changes (sprint turns arc over several frames instead of jumping), so the instant
    // packet pre-rotation must be suppressed there (OnFootDeltaHeadCallback).
    // THE STATE, NOT THE BUTTON. This gates the snap-turn packet pre-rotation because the game
    // rate-limits heading changes DURING SPRINT -- so it wants to know whether the player IS sprinting,
    // and it used to be handed our input instead. Now that the loop above pulses the button, the input
    // is not even a good proxy: it is low for most of a sprint. Published from the game's own state
    // when there is one, and from the intent when there is not (which is what it was before).
    {
        // SprintEvents, not the locomotion blackboard: measured, that blackboard never reports the
        // player's sprint at all, so this suppression has been reading a constant false.
        const int sfPub = g_VRSprintActive;
        g_sprintInputActive = (sfPub >= 0) ? (sfPub != 0) : wantSprint;
    }

    // Right stick = camera turn / pitch.
    const float legacyRx=ApplyStickDeadzone(vr.rightThumbX,.18f);
    const float legacyRy=ApplyStickDeadzone(vr.rightThumbY,.18f);
    float rx=analogMovement?cvr::input::AnalogAxis(vr.rightThumbX,g_liveControls.xrRightStickDeadzone,fullInput):legacyRx;
    float ry=analogMovement?cvr::input::AnalogAxis(vr.rightThumbY,g_liveControls.xrRightStickDeadzone,fullInput):legacyRy;
    float snapStickX=vr.rightThumbX;

    // Right stick pushed near FULL down => CROUCH. Same bind as the right-stick click
    // (R3, ToggleCrouch in the Alternative scheme the preset selects); each push taps R3 once
    // and the downward Y is consumed while held so it doesn't also drive camera pitch. Detected here, before the snap
    // turn block may zero ry, so it works regardless of the turn mode.
    // CROUCH IS THIS, AND ONLY THIS, since the click became the slide release: the stick past 0.90 down.
    // ON FOOT ONLY, for the same reason the dash below is: crouching means nothing in a car, and
    // R3 there is VehicleInverseCameraToggle_Button -- so the right stick pushed down was
    // flipping the driving camera. Found while fixing the exit button; same family of bug.
    // A DEVICE SCREEN IS UP: HAND THE RIGHT STICK'S Y BACK TO THE GAME.
    //
    // The game scrolls a device screen -- a computer's message list, a terminal -- with
    // UI_MoveY_Axis, and its own r6\config\inputUserMappings.xml binds that to IK_Pad_RightAxisY
    // and to nothing else. Three things here were eating exactly that: the crouch gesture and the
    // dash gesture each consume their half of the axis, and xr_disable_mouse_y zeroes it outright
    // for anyone who wants pitch from the headset only -- which is the shipped setting. So on a
    // computer the list could not be scrolled at all, and pushing the stick to read it dodged or
    // crouched instead.
    //
    // The flag is [164], published by the CyberpunkVRPort_DeviceCam redscript at the same two
    // points the game pushes and pops UIGameContext.DeviceZoom. It is NOT the world-map flag [81]:
    // that one also stops the HMD driving the game camera, and a device screen must keep the head
    // free to look around it.
    bool deviceScreen = false;
    if (float* shDev = GetShotShared()) {
        if (reinterpret_cast<volatile uint32_t*>(shDev)[vrshared::kDeviceScreenOpen] != 0u) {
            deviceScreen = true;
        }
    }

    // THE SCANNER'S ZOOM TAKES THE RIGHT STICK, and it takes it here -- before the crouch, the dash,
    // the pitch suppression and the snap turn, all of which read this same axis below. Consuming rx/ry
    // is what keeps the gesture from squatting the player or dodging while a zoom is being nudged.
    {
        static bool     s_ltZoomWas   = false;
        static uint64_t s_nextStepMs  = 0;
        const uint64_t now = GetTickCount64();
        // Hysteresis on the trigger for the same reason the tag has it: one squeeze is one gesture,
        // and a finger resting at the break point is not a stream of them.
        const bool ltDown = s_ltZoomWas ? (vr.leftTrigger > 0.35f) : (vr.leftTrigger > 0.60f);
        s_ltZoomWas = ltDown;
        const bool armed = scannerActive && (CyberpunkVR_ScannerZoom != 0) && ltDown
                           && !g_isInVehicle && (g_menuModeValue == 0);
        if (armed) {
            float th = CyberpunkVR_ScannerZoomStick;
            if (!(th > 0.05f) || th > 1.0f) th = 0.50f;
            const int32_t rep = (CyberpunkVR_ScannerZoomRepeatMs > 0)
                                    ? CyberpunkVR_ScannerZoomRepeatMs : 200;
            if (legacyRy > th || legacyRy < -th) {
                if (now >= s_nextStepMs) {
                    SendZoomKey(legacyRy > 0.0f);
                    s_nextStepMs = now + static_cast<uint64_t>(rep);
                }
            } else {
                s_nextStepMs = 0;   // back to centre re-arms the next step immediately
            }
            rx = 0.0f;
            ry = 0.0f;
            snapStickX = 0.0f; // Scanner zoom owns raw snap travel too.
        } else {
            s_nextStepMs = 0;
        }
    }

    const bool fullDown=cvr::input::AtFullTravel(analogMovement?vr.rightThumbY:ry,analogMovement,fullInput,true);
    const bool fullUp=cvr::input::AtFullTravel(analogMovement?vr.rightThumbY:ry,analogMovement,fullInput);
    const bool crouchAllowed = !inWater && !g_isInVehicle && !deviceScreen && !scannerActive
                               && !DeviceCamActive();   // in a camera the stick aims the camera
    const bool wantCrouch = fullDown && crouchAllowed;
    if (wantCrouch) ry = 0.0f;
    // R3 is a tap per push, not a level: ToggleCrouch only reacts to a tap, so a held stick stood still.
    static cvr::input::StickPulse s_crouchPulse;
    const bool crouchPulse = s_crouchPulse.Update(fullDown, analogMovement ? -vr.rightThumbY : -legacyRy,
        GetTickCount64(), static_cast<uint32_t>(CyberpunkVR_CrouchPulseMs > 0 ? CyberpunkVR_CrouchPulseMs : 100),
        crouchAllowed);

    // Right stick pushed near FULL UP => DASH (the game's Dodge_Button, pad B). The mirror image of
    // the crouch gesture above, in every respect: same 0.90 threshold, detected here BEFORE the pitch
    // suppression so it works whichever way "Disable Mouse Y" is set, and its half of the axis is
    // consumed so a dash never also pitches the camera.
    //
    // DASH LIVES HERE AND NOT ON A. A is Jump_Button and carries three things already -- jump, the
    // double jump, and Charge Jump on the hold -- so a tap/hold split there has to spend one of them:
    // either the jump moves to the release (late, and the charge can never charge) or the jump always
    // fires first and the dash is mid-air only. This half of this axis was doing nothing.
    //
    // EDGE-TRIGGERED, with the same re-arm rule the snap turn uses: the stick must come back before
    // another dash can fire, so holding it up dodges exactly once. The direction is the LEFT stick's,
    // as it is on a pad -- B plus a held direction, nothing to invent.
    bool dashPulse = false;
    {
        static int      s_dashArmedDir = 0;   // 1 = fired on this push, waiting for the stick to return
        static uint64_t s_dashUntilMs  = 0;
        const bool allowDash = (CyberpunkVR_DashStickUp != 0)
                               && !inWater
                               && !scannerActive        // the scanner owns this stick
                               && !deviceScreen         // the stick is scrolling a screen
                               && !DeviceCamActive()    // in a camera it is aiming the camera
                               && !g_isInVehicle            // in a car the right stick is free look
                               && (g_menuModeValue == 0);    // in menus it navigates
        if (allowDash) {
            const uint64_t now = GetTickCount64();
            if (fullUp) {
                if (s_dashArmedDir == 0) {
                    s_dashArmedDir = 1;
                    const int ms = (CyberpunkVR_DashPulseMs > 0) ? CyberpunkVR_DashPulseMs : 100;
                    s_dashUntilMs = now + static_cast<uint64_t>(ms);
                }
            } else if (legacyRy < 0.50f) {
                s_dashArmedDir = 0;
            }
            dashPulse = (now < s_dashUntilMs);
        } else {
            s_dashArmedDir = 0;
            s_dashUntilMs = 0;
        }
    }
    if (fullUp && !deviceScreen) ry = 0.0f;   // consumed, exactly as the crouch half is

    // Suppress pitch from the stick if HMD-only pitch is selected.
    // ...but never on a device screen: there this axis is not camera pitch at all, it is the
    // game's own list navigation, and zeroing it there is what made a computer unreadable.
    if (g_liveControls.xrDisableMouseY != 0 && !deviceScreen) ry = 0.0f;

    // NOT INSIDE A SURVEILLANCE CAMERA. The block below consumes stick X and turns the PLAYER's
    // on-foot heading instead, so in a camera it turned a body nobody can see while the view -- composed
    // from the lens -- did not move, and the stick could not pan the camera because its X never reached
    // the game. Off, the game's own continuous camera aim works exactly as it does on a flat screen.
    // ...AND NOT IN A SCENE THAT TURNS NOTHING ITSELF. Same shape as the camera above and for the
    // same reason: this block CONSUMES stick X, and in such a scene the port's own yaw is the only
    // thing that can turn the view -- so a snap turn here spends the axis on a body heading the
    // scene ignores and leaves the view frozen. Seen exactly that way: with snap turn on
    // the camera does not turn.
    // Vehicles and attached ladders use the game's continuous camera turn.
    // Clear queued snaps and require stick release before snapping on exit.
    const bool snapEnabled=g_liveControls.xrSnapTurn != 0 && !DeviceCamActive() &&
        !SceneStickYawArmed() && !cvr::input::NativeOwnsTurn(
            g_isInVehicle,g_vehicleState.load(std::memory_order_relaxed),cvr::ladder::Active());
    const auto turn=cvr::input::RouteControllerTurn(snapStickX,rx,snapEnabled,g_xinputSnapArmedDir,
        g_liveControls.xrSnapTurnAngleDeg,CyberpunkVR_SnapTurnStickFire,CyberpunkVR_SnapTurnStickRearm);
    if (turn.clearPending) InterlockedExchange(&g_pendingSnapYawDeltaBits,0);
    else if (turn.snapDegrees!=0) {
        LONG bits;
        memcpy(&bits,&turn.snapDegrees,sizeof(bits));
        InterlockedExchange(&g_pendingSnapYawDeltaBits,bits);
    }
    rx=turn.axis;

    if (fabsf(rx) > fabsf(pState->Gamepad.sThumbRX / 32767.0f)) pState->Gamepad.sThumbRX = FloatToSHORT(rx);
    // PUBLISHED ON THE PATH THE GAME ACTUALLY PLAYS ON. The first attempt at this sat beside the
    // merge in the MENU branch, which never runs during play -- the accumulator read a flat 0.0
    // with its gate wide open, which is how that was caught rather than guessed at.
    g_lookStickX.store(pState->Gamepad.sThumbRX / 32767.0f, std::memory_order_relaxed);
    if (fabsf(ry) > fabsf(pState->Gamepad.sThumbRY / 32767.0f)) pState->Gamepad.sThumbRY = FloatToSHORT(ry);

    // Stick-gesture buttons: full-forward left stick => sprint (L3), full-down right
    // stick => crouch (R3). OR'd in on top of any physical / VR button press.
    uint16_t synthButtons = 0;
    if (wantSprint) synthButtons |= XINPUT_GAMEPAD_LEFT_THUMB;
    if (crouchPulse) synthButtons |= XINPUT_GAMEPAD_RIGHT_THUMB;
    // The dash, as the B the game's Dodge_Button listens for. Emitted HERE rather than merged with the
    // controller's own buttons, because the physical B is the port's now (the magazine drop) and is
    // masked out of that merge -- so this pulse is the only thing in the process that can dodge.
    if (dashPulse) synthButtons |= 0x2000;   // XINPUT_GAMEPAD_B
    // The scanner, as the LB the game's Vision_Hold_Button listens for. Level-triggered by
    // construction: it is a HOLD binding, so it stays down exactly as long as the gesture does.
    if (scannerHold) synthButtons |= 0x0100;   // XINPUT_GAMEPAD_LEFT_SHOULDER
    // ...and the scanner's own five, as the buttons the game's listeners are actually bound to. The
    // block that computes these names the file each binding was read out of.
    if (scanDpadUp)   synthButtons |= 0x0001;   // DPAD_UP       -> UI_MoveUp
    if (scanDpadDown) synthButtons |= 0x0002;   // DPAD_DOWN     -> UI_MoveDown
    if (scanApply)    synthButtons |= 0x4000;   // X             -> UI_ApplyAndClose
    if (scanTag)      synthButtons |= 0x0080;   // RIGHT_THUMB   -> Tag_Button
    if (scanTab)      synthButtons |= 0x0200;   // RIGHT_SHOULDER-> DescriptionChange
    // A CONFIRMS A DIALOGUE LINE WHILE MOUNTED, because X cannot.
    //
    // Read out of the game's merged r6\cache\inputUserMappings.xml rather than assumed:
    //
    //     <mapping name="DialogConfirm">  IK_Pad_X_SQUARE
    //     <mapping name="Choice1">        IK_Pad_X_SQUARE
    //
    // so confirming a line is X -- and the block below takes X out of the merge entirely while mounted,
    // because in a car X is the exit. The consequence was not a compromise, it was a dead end: seated,
    // there was no button left that could confirm a line at all.
    //
    // So A is mirrored to X while mounted. A is NOT free there -- it is Vehicle_Handbrake -- and that is
    // deliberately left working: the mirror is ADDED, so the handbrake still fires and the dialogue gets
    // its confirm. Nothing else listens for X in a car, since the game never sees the physical one.
    //
    // ON THE EDGE, not level: DialogConfirm acts on a press, and a held A would repeat it. The handbrake
    // needs the level, so only the mirrored X is edge-triggered.
    //
    // THE ONE CAVEAT, stated rather than discovered later: a scene that puts its options on the face
    // buttons (Choice1..4, where A is natively Choice3) will see A fire Choice3 AND the mirrored Choice1.
    // The ordinary scroll-and-confirm list is unaffected, and it is what a car conversation is.
    if (CyberpunkVR_VehicleDialogConfirmOnA != 0) {
        static bool s_aWas = false;
        const bool aDown = vehicleButtons.confirmOnA && gameplayScreen && (vr.buttons & 0x1000) != 0;
        if (aDown && !s_aWas) synthButtons |= 0x4000;   // XINPUT_GAMEPAD_X -> DialogConfirm
        s_aWas = aDown;
    }

    // X GETS YOU OUT OF THE CAR, HELD. The game has no pad binding for the exit except B
    // (ExitVehicle_Button = IK_F + IK_Pad_B_CIRCLE), so the press is translated rather than rebound:
    // X is held out of the merge above while mounted, and mirrored to B here.
    //
    // A HOLD, not a level mirror, and that is a correction rather than a refinement. ExitVehicle_Button
    // carries no <hold> and no acceptedEvents in the game's mappings, so the vehicle state machine acts
    // on the first frame it sees the action -- and at speed acting on it means throwing the player out
    // of the car. Mirrored level, brushing X while driving ejected them into a ragdoll, which reads as
    // a collision or physics bug and not as a button at all. Held, a stray tap costs nothing.
    {
        static cvr::input::VehicleExitHold exitHold;
        const uint64_t need=CyberpunkVR_VehicleExitHoldMs>0 ? uint64_t(CyberpunkVR_VehicleExitHoldMs) : 400;
        if(exitHold.Update(vehicleButtons.exitOnX && gameplayScreen,(vr.buttons&0x4000)!=0,GetTickCount64(),need))
            synthButtons |= 0x2000;   // XINPUT_GAMEPAD_B = ExitVehicle_Button
    }
    pState->Gamepad.wButtons |= synthButtons;
    if(cvr::RuntimeDiagnosticsEnabled()) {
        static uint64_t lastScanOutput=0;static bool lastHold=false;
        const auto tick=GetTickCount64();
        if(scannerHold!=lastHold || (scannerHold && tick-lastScanOutput>=250)) {
            lastScanOutput=tick;lastHold=scannerHold;
            Log("[scanner-output] requested=%d vision=%d buttons=%04X grip=%.3f foreground=%d\n",
                scannerHold,scannerVision,unsigned(pState->Gamepad.wButtons),vr.leftGrip,GetForegroundWindow()==g_gameHwnd);
        }
    }

    // Y WHILE TWO GUNS ARE IN HAND IS TWO THINGS, split by how long it is held. A TAP drops the left gun's
    // magazine: counted on the release into kDualWieldYTap, which the reload mod watches, and never seen by
    // the game -- there it would switch weapons mid-fight. A HOLD is the game's own Y again from the moment it
    // has lasted CyberpunkVR_DualWieldYHoldMs, so there is still a way out of two guns: let go then and the
    // game sees a tap (SwitchItem_Button, the next weapon), keep holding and it opens the weapon wheel
    // (WeaponWheel_Button). A hold drops nothing.
    {
        static uint64_t s_yDownAt = 0;
        static bool s_yPass = false;
        static float s_yTaps = 0.0f;
        const bool yDown = (vr.buttons & 0x8000) != 0;
        const uint64_t now = GetTickCount64();
        if (dualWield && yDown) {
            if (s_yDownAt == 0) s_yDownAt = now;
            const int32_t holdMs = (std::max)(0, CyberpunkVR_DualWieldYHoldMs);
            if (now - s_yDownAt >= static_cast<uint64_t>(holdMs)) s_yPass = true;
        } else {
            if (dualWield && s_yDownAt != 0 && !s_yPass) {
                s_yTaps += 1.0f;
                OpenXRManager::Get().SetSharedSlot(vrshared::kDualWieldYTap, s_yTaps);
            }
            s_yDownAt = 0;
            s_yPass = false;
        }
        if (dualWield && !s_yPass) pState->Gamepad.wButtons &= static_cast<uint16_t>(~0x8000);
    }

    // THE PORT'S WEAPON WHEEL TAKES THE HOLD OF Y (Hooks/WeaponWheelHold.hpp): a press is withheld until it is
    // decided -- let go early and the game gets a tap (SwitchItem, the next weapon), held and the wheel script
    // (VRWeaponWheel) shows its wheel while the game sees nothing. Composed after the two-gun split above, so
    // there it is that split's passed-on hold that is decided here. Only on a gameplay screen with no popup, menu
    // capture, scanner or braindance: everywhere else Y means something else.
    {
        static cvr::input::WheelHold s_wheelHold;
        const uint64_t now = GetTickCount64();
        const bool armed = cvr::vrmenu::WheelArmed(now) && gameplayScreen && !uiPopupOpen && !menuCapture &&
                           !scannerActive && g_bdActive.load(std::memory_order_relaxed) == 0;
        const auto wheel = s_wheelHold.Step((pState->Gamepad.wButtons & 0x8000) != 0, armed, now);
        pState->Gamepad.wButtons = static_cast<uint16_t>((pState->Gamepad.wButtons & ~0x8000) |
                                                         (wheel.gameY ? 0x8000 : 0));
        cvr::vrmenu::PublishWheelHold(wheel.pending, wheel.held);
    }

    menuMask();
    PublishMergedPacket(pState);
    return r;
}

static int PatchXInputIat(HMODULE mod, void* newFunc, void** outOrig) {
    auto base = reinterpret_cast<uint8_t*>(mod);
    auto dos = reinterpret_cast<IMAGE_DOS_HEADER*>(base);
    if (!base || dos->e_magic != IMAGE_DOS_SIGNATURE) return 0;
    auto nt = reinterpret_cast<IMAGE_NT_HEADERS*>(base + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE) return 0;
    const IMAGE_DATA_DIRECTORY& dir =
        nt->OptionalHeader.DataDirectory[IMAGE_DIRECTORY_ENTRY_IMPORT];
    if (dir.VirtualAddress == 0 || dir.Size == 0) return 0;

    int patched = 0;
    for (auto imp = reinterpret_cast<IMAGE_IMPORT_DESCRIPTOR*>(base + dir.VirtualAddress);
         imp->Name; ++imp) {
        const char* dll = reinterpret_cast<const char*>(base + imp->Name);
        if (_strnicmp(dll, "xinput", 6) != 0) continue;            // xinput1_4 / 1_3 / 9_1_0
        if (imp->OriginalFirstThunk == 0 || imp->FirstThunk == 0) continue;
        auto nameThunk = reinterpret_cast<IMAGE_THUNK_DATA*>(base + imp->OriginalFirstThunk);
        auto iatThunk  = reinterpret_cast<IMAGE_THUNK_DATA*>(base + imp->FirstThunk);
        for (; nameThunk->u1.AddressOfData; ++nameThunk, ++iatThunk) {
            if (nameThunk->u1.Ordinal & IMAGE_ORDINAL_FLAG) continue;   // imported by ordinal: no name
            auto ibn = reinterpret_cast<IMAGE_IMPORT_BY_NAME*>(base + nameThunk->u1.AddressOfData);
            if (strcmp(reinterpret_cast<const char*>(ibn->Name), "XInputGetState") != 0) continue;
            void** slot = reinterpret_cast<void**>(&iatThunk->u1.Function);
            if (*slot == newFunc) continue;                            // already ours (re-scan)
            DWORD oldP = 0;
            if (VirtualProtect(slot, sizeof(void*), PAGE_READWRITE, &oldP)) {
                if (outOrig && !*outOrig) *outOrig = *slot;            // chain whatever was there
                *slot = newFunc;
                VirtualProtect(slot, sizeof(void*), oldP, &oldP);
                ++patched;
            }
        }
    }
    return patched;
}

bool InstallXInputHook() {
    // Make sure an XInput DLL is resolvable so a not-yet-bound import is live and
    // the GetProcAddress fallback below works. Not fatal if absent -- the IAT
    // match is by name, independent of which XInput variant the game imports.
    HMODULE xi = GetModuleHandleA("XInput1_4.dll");
    if (!xi) xi = LoadLibraryA("XInput1_4.dll");
    if (!xi) xi = LoadLibraryA("XInput1_3.dll");
    if (!xi) xi = LoadLibraryA("xinput9_1_0.dll");

    void* orig = nullptr;
    int patched = 0;
    void* hook = reinterpret_cast<void*>(&HookedXInputGetState);

    // Main executable first (CP2077 imports XInputGetState here), then every other
    // loaded module that imports it, so no caller is missed. The exe is also in the
    // EnumProcessModules list; the "already ours" guard makes the re-scan a no-op.
    if (HMODULE exe = GetModuleHandleW(nullptr))
        patched += PatchXInputIat(exe, hook, &orig);

    HMODULE mods[512];
    DWORD needed = 0;
    if (EnumProcessModules(GetCurrentProcess(), mods, sizeof(mods), &needed)) {
        const DWORD count = needed / sizeof(HMODULE);
        const DWORD n = count < 512 ? count : 512;
        for (DWORD i = 0; i < n; ++i)
            patched += PatchXInputIat(mods[i], hook, &orig);
    }

    if (patched > 0 && orig) {
        g_realXInputGetState = reinterpret_cast<XInputGetState_t>(orig);
        g_xinputHooked = true;
        Log("XInput: IAT hook installed (%d slot(s) patched, real=%p)\n", patched, orig);
        return true;
    }

    // No import slot found (game resolves XInput dynamically or by ordinal). Keep a
    // real pointer so the shim could still chain if ever invoked, and fail soft --
    // controller input is simply unavailable, the game is NOT patched, no crash.
    if (xi && !g_realXInputGetState)
        g_realXInputGetState = reinterpret_cast<XInputGetState_t>(GetProcAddress(xi, "XInputGetState"));
    Log("XInput: no XInputGetState import slot found (patched=%d) -- controller input unavailable\n", patched);
    return false;
}
// THE GUARD THIS HOOK ALWAYS HAD: the boot wrapped the call in
// `if (g_liveControls.xrXInputInstall != 0)`. It travels with the hook, so the setting cannot be
// lost by a boot function forgetting to ask.
namespace { bool XInputWanted() { return g_liveControls.xrXInputInstall != 0; } }

CVR_HOOK_IF("XInput", ::cvr::hooks::Stage::Boot, 100, InstallXInputHook, XInputWanted);
