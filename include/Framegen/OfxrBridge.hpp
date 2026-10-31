#pragma once
namespace cvr::framegen { struct Settings; struct RuntimeStatus; }
namespace cvr::ofxr {
// The NVIDIA menu choice owns a private V557 OpenXR chain, not the native generator.
bool NativeAllowed();
int RecordingStatus(); // -1 unavailable, 0 stopped, 1 writing
void Configure(const framegen::Settings&);
framegen::RuntimeStatus Status();
}
