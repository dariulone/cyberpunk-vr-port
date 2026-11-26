#pragma once
#include <array>
#include <vector>

namespace cvr::blade::mesh {
using Point = std::array<double,3>;
using Triangle = std::array<Point,3>;
using Ray = std::array<Point,2>;
using SweepSamples = std::array<Ray,3>;
struct SurfaceHit {
    Point point{},normal{};
    double seedDistanceSquared{.16*.16};
    bool found{},exact{},onSegment{};
};
// With per-vertex region weights (in packed 0..255 units), accept only a
// finite sweep intersection that has positive representable skin influence.
void LocateStamp(const SweepSamples& rays,const Point& seed,const Point& contactNormal,const Triangle& face,SurfaceHit& hit,
                 double shellDepth=0.0,const Point* shadingNormal=nullptr,const Point* blendWeights=nullptr);
// Nearest point on the closed triangle. Ignore collapsed packed triangles.
bool ClosestSurfacePoint(const Triangle& face,const Point& seed,Point& point,Point& normal);
struct Cut {
    Point from{}, to{}, normal{};
    double halfWidth{}, depth{}, outside{};
};
struct Volume {
    Point center{};
    Triangle axes{};
    Point extent{};
    double outside{};
};
bool MakeVolume(const Cut& cut, Volume& volume);
SweepSamples SampleSweep(const std::array<Point,4>& endpoints);
bool ResolveSweep(const SweepSamples& samples,const std::array<double,3>& fractions,const Triangle& normals,Cut& cut);
double SegmentDistance(const Volume& first, const Volume& second);
bool ProjectSurface(const Point& point,const Point& normal,const Triangle& face,double& distance,Point& weights);
bool SegmentHit(const Point& from,const Point& to,const Triangle& face,double& fraction,Point& normal);
bool Subtract(const Triangle& positions, const Volume& volume, std::vector<Triangle>& barycentricFaces,
              std::vector<Triangle>* covered=nullptr,bool diamond=false);
// Both outputs are barycentric coordinates on the supplied render triangle.
bool MarkFace(const Triangle& face,const Volume& footprint,bool openSeam,
              std::vector<Triangle>& outside,std::vector<Triangle>& ink,bool diamond=false);
}
