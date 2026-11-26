// The DLL's grip-from-fingers switch (src/Runtimes/FingerGrip.cpp), off: the controllers' own grips.
namespace cvr::fingergrip {
bool GripFromFingers() { return false; }
}
