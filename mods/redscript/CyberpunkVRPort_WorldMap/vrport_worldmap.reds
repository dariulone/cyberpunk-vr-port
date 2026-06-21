// CyberpunkVRPort — World map head-lock bridge.
//
// The full-screen world map SWIMS with head rotation because the VR pipeline
// drives the game camera with the HMD orientation, and the native menu-mode hook
// in dxgi.dll does NOT catch the world map. This mod reliably detects the world
// map opening/closing (its controller's lifecycle) and tells dxgi.dll via the
// shared-memory bridge (native SetVRMenuOpen, written to a dedicated shared slot)
// applying the HMD orientation while the map is open -> the map stays put, like
// RealVR's head-locked 2D virtual screen.
//
// Pure detection + a single native call. Cannot crash the game. The native is
// provided by our red4ext plugin (CyberpunkVRPort.dll, always loaded with dxgi).

native func SetVRMenuOpen(open: Int32) -> Int32;

@wrapMethod(WorldMapMenuGameController)
protected cb func OnInitialize() -> Bool {
  let result = wrappedMethod();
  SetVRMenuOpen(1);
  FTLog("[VRPortMap] world map OPEN -> head-lock on");
  return result;
}

@wrapMethod(WorldMapMenuGameController)
protected cb func OnUninitialize() -> Bool {
  let result = wrappedMethod();
  SetVRMenuOpen(0);
  FTLog("[VRPortMap] world map CLOSE -> head-lock off");
  return result;
}
