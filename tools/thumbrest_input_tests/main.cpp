#include "Hooks/ThumbrestDpad.hpp"
#include <cstdlib>
#include <iostream>
void check(bool value) { if (!value) std::exit(1); }
int main() {
    using namespace cvr::input;
    for (int direction=0; direction<4; ++direction) {
        float x = direction==2 ? -1.f : direction==3 ? 1.f : 0.f;
        float y = direction==0 ? 1.f : direction==1 ? -1.f : 0.f;
        uint16_t buttons=0x1000;
        check(ThumbrestDpad(true,x,y,buttons));
        check(buttons==(0x1000|(1<<direction)) && x==0 && y==0);
    }
    float x=.5f,y=-.5f;uint16_t buttons=0;
    check(!ThumbrestDpad(true,x,y,buttons) && x==0 && y==0 && buttons==0);
    x=1;y=1;check(!ThumbrestDpad(false,x,y,buttons) && x==1 && y==1 && buttons==0);
    check(ThumbrestDpad(true,x,y,buttons) && buttons==9);
    check(ThumbrestProfileAllowed("Oculus Quest 3"));
    check(!ThumbrestProfileAllowed("Sony PlayStation_VR2"));
    check(!ThumbrestProfileAllowed("PSVR2"));
    std::cout << "Thumb-rest directions, threshold, look suppression and Sense exclusion passed\n";
}
