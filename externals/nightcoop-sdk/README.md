# NightCoop SDK (vendored header)

`include/nightcoop_sdk.h` is the public C interface of NightCoop, the co-op mod for Cyberpunk 2077, unchanged
from `NightCoop-SDK.zip` (https://nightcoop.net/sdk.html, SDK version 1, archive SHA-256
`52993842cf342c8ab3976bd607017e4dc8d7b7ad8da2d5a6519d8c72d216c7af`). MIT, see `LICENSE`.

The port uses it in `src/Natives/NightCoopBridge.cpp`: the VR head, hands, fingers, aim and crouch go to NightCoop
as a pose source (`poseSet`), and NightCoop lays them on the player's body on the partners' machines. Nothing is
linked: NightCoop.dll is looked up at run time, and without it the port runs as before.
