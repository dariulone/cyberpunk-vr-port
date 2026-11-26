// Timed virtual controller inputs in the simulator's body frame. No game hit
// events enter this stream. The driver supplies the equipped model's measured
// blade and edge directions in tracking coordinates.
#include <algorithm>
#include <array>
#include <cmath>
#include <iomanip>
#include <iostream>
#include <numbers>
#include <stdexcept>
#include <string>

using Vec = std::array<double, 3>;
using Quat = std::array<double,4>;
Quat turn(Vec axis,double degrees) {
    const double half=degrees*std::numbers::pi/360;
    return {axis[0]*std::sin(half),axis[1]*std::sin(half),axis[2]*std::sin(half),std::cos(half)};
}
Quat compose(Quat p,Quat q) {
    return {p[3]*q[0]+p[0]*q[3]+p[1]*q[2]-p[2]*q[1],
            p[3]*q[1]-p[0]*q[2]+p[1]*q[3]+p[2]*q[0],
            p[3]*q[2]+p[0]*q[1]-p[1]*q[0]+p[2]*q[3],
            p[3]*q[3]-p[0]*q[0]-p[1]*q[1]-p[2]*q[2]};
}
double dot(Vec a, Vec b) { return a[0]*b[0]+a[1]*b[1]+a[2]*b[2]; }
Vec unit(Vec v) {
    double n=std::sqrt(dot(v,v));
    if (!std::isfinite(n) || n<0.01) throw std::invalid_argument("invalid calibration axis");
    for (auto& x:v) x/=n;
    return v;
}

int main(int argc, char** argv) {
    const bool neutral=argc>=2 && std::string(argv[1])=="neutral";
    const int required=neutral?2:8;
    if (argc!=required && argc!=required+2) {
        std::cerr<<"usage: melee_trajectory neutral|<motion> [axisXYZ edgeXYZ] [yaw roll]\n"; return 2;
    }
    try {
        const double yaw=argc>required?std::stod(argv[required]):0.;
        const double roll=argc>required?std::stod(argv[required+1]):0.;
        if (!std::isfinite(yaw) || !std::isfinite(roll)) throw std::invalid_argument("invalid angle");
        const Quat initial=compose(turn({0,0,-1},90+roll),compose(turn({1,0,0},-65),turn({0,0,-1},-10)));
        const Quat baseRotation=compose(turn({0,1,0},yaw),initial);
        std::cout<<std::setprecision(10);
        if (neutral) {
            for(double x:baseRotation) std::cout<<x<<' ';
            return 0;
        }
        std::string kind=argv[1];
        Vec axis=unit({std::stod(argv[2]),std::stod(argv[3]),std::stod(argv[4])});
        Vec edge={std::stod(argv[5]),std::stod(argv[6]),std::stod(argv[7])};
        const double parallel=dot(axis,edge);
        for (unsigned i=0;i<3;++i) edge[i]-=axis[i]*parallel;
        edge=unit(edge);
        const bool thrust=kind=="thrust", reverse=kind=="reverse", arc=kind=="arc", withdraw=kind=="withdraw";
        const bool quickReturn=kind=="quick-return";
        const bool edgePress=kind=="edge-press",flatPress=kind=="flat-press";
        if (!thrust && !reverse && !arc && !withdraw && !quickReturn && !edgePress && !flatPress && kind!="swing") throw std::invalid_argument("unknown motion");
        const Vec base={0.15,-0.50,-0.24};
        const Vec flat={axis[1]*edge[2]-axis[2]*edge[1],axis[2]*edge[0]-axis[0]*edge[2],axis[0]*edge[1]-axis[1]*edge[0]};
        const Vec direction=(thrust || withdraw || quickReturn)?axis:(flatPress?flat:edge);
        const bool press=edgePress || flatPress;
        const double duration=quickReturn?.56:(press?.30:(thrust?.22:.16));
        const int steps=quickReturn?48:24;
        for (int i=0;i<=steps;++i) {
            const double t=double(i)/steps;
            const double s=t*t*(3-2*t);
            double offset;
            if (quickReturn) {
                // Start from the held thrust. One continuous controller stream,
                // with maximum axial speed on both sides of the clear turning point.
                const double leg=1.-std::abs(2.*t-1.);
                offset=.42-.84*leg*leg;
            } else {
                offset=withdraw?-.18-.24*s:(press?.145*s:(thrust?-.18+.60*s:(reverse?-1.:1.)*(s-.5)*(arc?.40:.95)));
            }
            std::cout<<duration*t;
            for(unsigned j=0;j<3;++j) std::cout<<' '<<base[j]+direction[j]*offset+(press?.42*axis[j]:0.);
            const Quat orientation=compose(turn({0,1,0},arc?92.*(s-.5):0.),baseRotation);
            for(double q:orientation) std::cout<<' '<<q;
            std::cout<<'\n';
        }
    } catch(const std::exception& e) { std::cerr<<e.what()<<'\n'; return 2; }
    return 0;
}
