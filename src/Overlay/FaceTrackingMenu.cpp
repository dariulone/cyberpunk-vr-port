#include "Anim/FaceTracking.hpp"
#include "Overlay/UiRec.hpp"
namespace cvr::face {
void DrawSettings() {
    if(!UI::CollapsingHeader("FACE TRACKING"))return;
    auto settings=GetSettings();bool changed=false;
    changed|=UI::Checkbox("Quest Pro face tracking in mirrors (experimental)",&settings.enabled);
    UI::TextWrapped("Steam Link ALT OSC. Applies to V in interactive mirrors; story scenes retain their animation.");
    UI::BeginDisabled(!settings.enabled);
    changed|=UI::Checkbox("Smile and mouth",&settings.mouth);
    changed|=UI::Checkbox("Blink",&settings.blink);
    changed|=UI::Checkbox("Eye direction",&settings.eyes);
    changed|=UI::Checkbox("Eyebrows",&settings.brows);
    UI::EndDisabled();
    UI::Text("OSC port: %d (localhost)",settings.port);
    const auto status=GetStatus();
    if(status.saveError)UI::TextWrapped("Settings apply this session, but saving face_tracking.ini failed.");
    if(settings.enabled) {
        if(!status.hooks)UI::TextWrapped("Facial adapter unavailable for this game build.");
        else if(status.socketError==10048 || status.socketError==10013)
            UI::TextWrapped("OSC port is busy or unavailable. Close the face diagnostic or another listener, or change port in face_tracking.ini.");
        else if(status.socketError)UI::Text("OSC socket error: %d",status.socketError);
        else if(!status.fresh)UI::TextWrapped("Waiting for face data. Enable Steam Link face sharing and ALT OSC output.");
        else UI::Text("Live face channels: %u",status.fresh);
        if(status.bound)UI::Text("V's mirror rig: %u mapped controls",status.mapped);
        else UI::TextWrapped("Open an interactive mirror to connect V's face.");
    }
    if(changed)SetSettings(settings);
}
}
