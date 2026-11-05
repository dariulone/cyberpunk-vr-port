# Lock-on markers for all smart weapons (2026-11-05)

VR weapon aim hides the crosshair. For smart weapons the crosshair widget also holds the lock-on markers, and until now only the
smart sniper rifle (Ashura) kept it. Other smart guns aim without seeing what they lock on to.

`CyberpunkVRPort_Crosshair/init.lua` now keeps it for every weapon of evolution Smart, and no longer asks the item type.

Used in game on Index controllers with SteamVR since 2026-10. No particular list of guns was gone through, and whether something other than the markers shows with them on some gun is not checked.
