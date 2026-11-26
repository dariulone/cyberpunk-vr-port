// Fit controller offsets to measured native tracking targets, then repeat a
// fixed reach, vertical slide, and withdrawal at the storefront post.
#include <algorithm>
#include <array>
#include <cmath>
#include <iomanip>
#include <iostream>
#include <stdexcept>
#include <string>

using Vec = std::array<double, 3>;
Vec sub(Vec a, Vec b) { return {a[0]-b[0], a[1]-b[1], a[2]-b[2]}; }
Vec cross(Vec a, Vec b) { return {a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]}; }
double dot(Vec a, Vec b) { return a[0]*b[0]+a[1]*b[1]+a[2]*b[2]; }

int generate(int cycles) {
    // stdin: post XYZ, station XYZ, neutral right controller offset, axis
    // displacement, world wrist at neutral and after each positive axis move,
    // and a reference wrist measured on the static post during fixture setup.
    auto vector = [] {
        Vec v{};
        for (auto& x:v) if (!(std::cin>>x) || !std::isfinite(x)) throw std::runtime_error("invalid calibration");
        return v;
    };
    const Vec post=vector(), station=vector(), neutral=vector();
    double step{};
    if (!(std::cin>>step) || !(step>=.05 && step<=.20)) throw std::runtime_error("invalid calibration step");
    const Vec calibrationNeutral=vector();
    std::array<Vec,3> basis;
    for (auto& axis:basis) {
        axis=sub(vector(),calibrationNeutral);
        for(auto& x:axis) x/=step;
        double length=std::sqrt(dot(axis,axis));
        if (!(length>.65 && length<1.45)) throw std::runtime_error("native wrist did not follow calibration axis");
    }
    std::array<Vec,3> reciprocal{cross(basis[1],basis[2]),cross(basis[2],basis[0]),cross(basis[0],basis[1])};
    double det=dot(basis[0],reciprocal[0]);
    if (!(std::abs(det)>.45)) throw std::runtime_error("degenerate native wrist calibration");
    const Vec wrist=vector(), patch=vector(), motorTarget=vector();
    if (std::hypot(patch[0]-post[0],patch[1]-post[1])>.40 || std::abs(patch[2]-post[2])>.40)
        throw std::runtime_error("contact patch outside post fixture");
    double push{}, lateral{}, vertical{}, pitch{};
    if(!(std::cin>>push>>lateral>>vertical>>pitch) || !(push>=0 && push<=.12 && std::abs(lateral)<=.12 &&
       std::abs(vertical)<=.10 && std::abs(pitch)<=60)) throw std::runtime_error("invalid press profile");
    double dx=post[0]-station[0], dy=post[1]-station[1], length=std::hypot(dx,dy);
    if (!(length>.4 && length<1.1)) throw std::runtime_error("station outside post reach fixture");
    dx/=length; dy/=length;
    const Vec refused=sub(motorTarget,patch);
    const double planar=std::hypot(refused[0],refused[1]);
    if(planar>.005 && (refused[0]*dx+refused[1]*dy)/planar>.65) {
        dx=refused[0]/planar;
        dy=refused[1]/planar;
    }
    const Vec aim{motorTarget[0]+push*dx-lateral*dy,motorTarget[1]+push*dy+lateral*dx,motorTarget[2]+vertical};
    std::cout<<std::setprecision(12);
    for(int i=0;i<cycles*40;++i) {
        int k=i%40;
        double reach=std::clamp((k-3)/8.,0.,1.)*std::clamp((32-k)/8.,0.,1.);
        reach=reach*reach*(3-2*reach);
        const bool loaded=k>=12 && k<24;
        Vec delta=sub(aim,wrist), right{};
        for(int axis=0;axis<3;++axis) right[axis]=neutral[axis]+reach*dot(reciprocal[axis],delta)/det;
        if (std::sqrt(dot(right,right))>1.30) throw std::runtime_error("calibrated offset beyond the fixture reach");
        // Hold all three devices still while sampling the load. The return
        // includes head motion and ends at the initial yaw.
        constexpr double pi=3.14159265358979323846;
        const double turn=k<=24?0.:.050*std::sin(pi*(k-24)/15.);
        const double half=pitch*(pi/360.)*reach;
        std::cout<<i/10.<<' '<<turn<<' '<<right[0]<<' '<<right[1]<<' '<<right[2]<<' '<<std::sin(half)<<' '<<std::cos(half)<<' '
                 <<(loaded?"press":(k<=3 || k>=32)?"clear":"travel")<<'\n';
    }
    return 0;
}
int main(int argc,char** argv) {
    try {
        if(argc!=3 || std::string(argv[2])!="settled-dwell")
            throw std::runtime_error("usage: post_trace <cycles> settled-dwell, calibration on stdin");
        int cycles=std::stoi(argv[1]);
        if(cycles<1 || cycles>8) throw std::runtime_error("cycles must be 1..8");
        return generate(cycles);
    } catch(const std::exception& e) { std::cerr<<e.what()<<'\n'; return 1; }
}
