// CyberpunkVRPort -- the device-screen native, and it lives here for one reason only.
//
// THIS FILE INTENTIONALLY HAS NO `module` LINE. A native declared inside a module is looked up by
// its MODULE-QUALIFIED name, so declaring this one in vrport_device_cam.reds (which is
// `module CyberpunkVRPort.DeviceCam`) made redscript refuse the whole file -- "the following scripts
// contain invalid native definitions and will prevent your game from starting" -- because the plugin
// registers a plain global `SetVRDeviceScreen`. The identical mistake is already documented in
// vrport_no_anims.reds, which hit it with SetVRSprintActive; the fix there was this same split, and
// this file is that fix copied rather than reinvented. A global func stays callable from inside the
// module, so nothing else has to move.
//
// WHAT IT IS FOR. It tells the plugin a device screen (computer, terminal) is up, so the XInput merge
// hands the RIGHT STICK'S Y back to the game. The game scrolls these screens with UI_MoveY_Axis, and
// r6\config\inputUserMappings.xml binds that to IK_Pad_RightAxisY and to nothing else -- while the
// port was eating that axis three ways: the crouch gesture, the dash gesture, and xr_disable_mouse_y
// zeroing it for headset-only pitch. On a computer the message list could not be scrolled at all.
//
// Deliberately NOT SetVRMenuOpen: that flag also stops the HMD driving the game camera, which is
// right for the world map and is the frozen view vrport_device_cam.reds exists to avoid.

native func SetVRDeviceScreen(open: Int32) -> Int32;
