#pragma once

// THE PORT'S OWN SOUNDS, played on the system's default output with XAudio2, beside the game's Wwise: the VR menu's
// clicks, the lobby's intro and its ambience (mods/cet: VRMenuPlayWav, VRSoundChannel). A sound is a file the mod
// ships -- a PCM WAV (16-bit) or an Ogg Vorbis (externals/stb/stb_vorbis.c decodes it once, on first use) -- named
// relative to the plugin's Lua mods folder (PLUGIN\lua) unless the path is absolute. Volumes are linear, 0..1: the
// scripts work them out from the game's own sliders. No voice callbacks: nothing of this DLL runs on XAudio2's thread,
// so unloading the plugin at exit cannot leave a call into it; voices are let go from Tick and the next play instead.
namespace cvr::sound {

// Once, at `volume`. False when the file or the audio device is not there.
bool PlayOnce(const char* file, float volume);

// What a channel holds.
//   lowpass     the cutoff of a low-pass filter over it, Hz -- the sound muffled, as through a wall; 0 = none
//   loopFrom    the sound plays from its start and then loops from here (s), so a loop does not keep starting
//               over; < 0 = played once
//   loopTo      where the loop goes back (s): before a fade the file ends on, say; <= loopFrom = the file's end
//   crossfade   the loop's seam (s): the end flows into the loop's start, blended (equal power) over this much of
//               what comes before it in the file
struct Sound {
    const char* file = "";
    float volume = 1.0f;
    float lowpass = 0.0f;
    float loopFrom = -1.0f;
    float loopTo = 0.0f;
    float crossfade = 0.0f;
};

// A CHANNEL holds one sound under a name until it is told otherwise; each call says what it should be now, reached
// over `fade` seconds:
//  - the same file (and loop) as it holds: its volume and its filter go to the new ones;
//  - another file: the one it held fades out while the new one fades in;
//  - no file (""): what it holds fades out and the channel ends.
// A sound played once ends the channel when it has played out. False when the file or the device is not there.
bool Channel(const char* name, const Sound& sound, float fade);

// Once per game Present: the channels' fades go on, voices that have finished are let go.
void Tick();

}  // namespace cvr::sound
