#include "Framegen/GpuCorrelation.hpp"
#include <cstdlib>
#include <cmath>
#include <iostream>
using cvr::framegen::GpuCorrelation;
void check(bool v){if(!v)std::abort();}
int main(){
    GpuCorrelation c{10000000000ULL,100000000ULL,100010000ULL,1000000000ULL,10000000ULL};
    auto r=c.Resolve(c.gpu+7000000,c.gpu+12000000);
    check(r && std::abs(r->queueMs-6)<.000001 && std::abs(r->totalMs-11)<.000001);
    check(!c.Resolve(c.gpu,c.gpu+1000000)); // negative calibrated delay
    check(!c.Resolve(c.gpu+7000000,c.gpu+6000000));
    check(!c.Resolve(c.gpu+7000000,c.gpu+2000000000));
    c.cpuHz=0;check(!c.Resolve(1,2));
    check(GpuCorrelation::Delta(3,10)==-7); // no unsigned wrap
    std::cout<<"PASS: queue-specific GPU/QPC conversion and invalid calibration rejection\n";
}
