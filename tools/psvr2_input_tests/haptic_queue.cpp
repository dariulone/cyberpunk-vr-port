#include "Runtimes/HapticPulseQueue.hpp"
#include <cstdlib>
#include <iostream>
#include <limits>
void check(bool ok) { if(!ok) std::exit(1); }
int main() {
    cvr::psvr2::HapticPulseQueue q;
    cvr::psvr2::HapticPulseQueue::Event e;
    q.Push(0,.85f,75,10); q.Push(1,1,125,10);
    check(q.Pop(10,e) && e.hand==0 && e.duration==75);
    check(!q.Pop(11,e));
    q.Push(0,.9f,90,15); // repeated left cannot starve waiting right
    check(q.Pop(18,e) && e.hand==1 && e.duration==125);
    check(q.Pop(26,e) && e.hand==0 && e.amplitude==.9f);
    q.Push(1,1,90,30); check(!q.Pop(131,e)); // stale impacts expire
    q.Push(-1,1,90,140); q.Push(2,1,90,140);
    q.Push(0,std::numeric_limits<float>::quiet_NaN(),90,140);
    check(!q.Pop(140,e));
    q.Push(0,.3f,45,150); q.Push(0,.8f,90,151);
    check(q.Pop(151,e) && e.amplitude==.8f && e.duration==90);
    std::cout << "Fair dual-hand pulse queue checks passed\n";
}
