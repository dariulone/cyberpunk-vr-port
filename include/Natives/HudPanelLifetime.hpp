#pragma once

namespace RED4ext { struct CGameApplication; }

namespace cvr::hud {
// Register before installing the HUD hooks. Native widget caches must be released
// on the game thread before engine teardown, never by DLL static destruction.
void NativeWidgetShutdownRegistered(bool registered);
bool ReleaseNativeWidgets(RED4ext::CGameApplication*);
}
