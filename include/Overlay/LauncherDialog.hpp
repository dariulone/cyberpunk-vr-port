#pragma once

#ifdef __cplusplus
extern "C" {
#endif

void ShowLauncherDialog();
int IsLauncherOpen();
void WaitForLauncherStartup(unsigned int milliseconds);
// A resolution that is not in the headset's ladder becomes the ladder entry nearest in cost; 1 when it
// changed. For the saved pick of a session that skips the dialog (src/Overlay/LauncherDialog.cpp).
int SnapLauncherResolution(int hmdType, int* width, int* height);
// A resolution larger than maxWidth x maxHeight becomes the largest entry of the headset's ladder within
// them -- or, for a headset without a ladder or one with no entry that small, the same aspect scaled into
// them (multiples of 8); 1 when it changed.
int FitLauncherResolution(int hmdType, int maxWidth, int maxHeight, int* width, int* height);

#ifdef __cplusplus
}
#endif
