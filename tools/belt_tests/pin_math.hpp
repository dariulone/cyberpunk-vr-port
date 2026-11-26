#pragma once
#include <cmath>
#include <nlohmann/json.hpp>
using Json = nlohmann::json;
struct V {
    double x = 0, y = 0, z = 0;
};
inline V operator+(V a, V b) {
    return {a.x + b.x, a.y + b.y, a.z + b.z};
}
inline V operator-(V a, V b) {
    return {a.x - b.x, a.y - b.y, a.z - b.z};
}
inline V operator*(V a, double s) {
    return {a.x * s, a.y * s, a.z * s};
}
inline double dot(V a, V b) {
    return a.x * b.x + a.y * b.y + a.z * b.z;
}
inline V cross(V a, V b) {
    return {a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x};
}
inline V unit(V v) {
    return v * (1 / std::sqrt(dot(v, v)));
}
struct Q {
    double x = 0, y = 0, z = 0, w = 1;
};
inline Q operator*(Q a, Q b) {
    return {a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y, a.w * b.y + a.y * b.w + a.z * b.x - a.x * b.z,
            a.w * b.z + a.z * b.w + a.x * b.y - a.y * b.x, a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z};
}
inline Q inv(Q q) {
    return {-q.x, -q.y, -q.z, q.w};
}
inline Q blend(Q a, Q b, double u) {
    double sign = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w < 0 ? -1 : 1;
    Q q{a.x + (b.x * sign - a.x) * u, a.y + (b.y * sign - a.y) * u, a.z + (b.z * sign - a.z) * u,
        a.w + (b.w * sign - a.w) * u};
    double n = std::sqrt(q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w);
    return {q.x / n, q.y / n, q.z / n, q.w / n};
}
inline V rot(Q q, V v) {
    auto t = cross({q.x, q.y, q.z}, v) * 2;
    return v + t * q.w + cross({q.x, q.y, q.z}, t);
}
inline Q axis(int a, double d) {
    double h = d * 3.141592653589793 / 360, s = std::sin(h);
    return {a == 0 ? s : 0, a == 1 ? s : 0, a == 2 ? s : 0, std::cos(h)};
}
inline Q euler(double x, double y, double z) {
    return axis(0, x) * axis(1, y) * axis(2, z);
}
inline V vec(const Json &a) {
    return {a[0], a[1], a[2]};
}
inline Q quat(const Json &a) {
    return {a[0], a[1], a[2], a[3]};
}
struct T {
    V p;
    Q q;
};
inline T relative(T a, T b) {
    return {rot(inv(a.q), b.p - a.p), inv(a.q) * b.q};
}
inline T compose(T a, T b) {
    return {a.p + rot(a.q, b.p), a.q * b.q};
}
