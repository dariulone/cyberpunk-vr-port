// The holster place icons of the VR menu's inventory (mods/cet/CyberpunkVRPort_Menu modules/holster_icons.lua): a
// figure seen as in a mirror -- its right on the right, as a player thinks of his own body -- and, each on its own
// layer, the weapon where it hangs. A weapon on the back is drawn bright where it shows past the body (the handle over
// the shoulder, the ends past the waist) and faint where the body hides it, so the menu tints each layer on its own:
// white premultiplied by the shape's alpha, as the game's UI textures are. One cell of `size` px per part.
//
//   holster_icons.exe <sheet.tga> <parts.json> <width> <height> <size> <part>...
//     body | body_w | none | <place>:<family>[:w]     (_w: the woman's figure; a weapon on the back is cut by it)
//       place:  hip_r hip_l thigh_r thigh_l underarm_l underarm_r chest_l chest_r shoulder_r shoulder_l lower_back
//       family: gun (a pistol) | long (a rifle) | blade (a sword) | knife
//
// Shapes are exact signed distances in the cell's unit square (y down), 4 x 4 samples a pixel.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <functional>
#include <string>
#include <vector>

namespace {

constexpr int kPad = 2;

struct V2 { double x, y; };
V2 operator+(V2 a, V2 b) { return {a.x + b.x, a.y + b.y}; }
V2 operator-(V2 a, V2 b) { return {a.x - b.x, a.y - b.y}; }
V2 operator*(V2 a, double k) { return {a.x * k, a.y * k}; }
double dot(V2 a, V2 b) { return a.x * b.x + a.y * b.y; }
double len(V2 a) { return std::sqrt(dot(a, a)); }
V2 norm(V2 a) { double l = len(a); return l > 1e-12 ? a * (1 / l) : V2{0, 1}; }
V2 perp(V2 a) { return {-a.y, a.x}; }
V2 rot(V2 a, double deg) {
  double r = deg * 3.14159265358979323846 / 180, c = std::cos(r), s = std::sin(r);
  return {a.x * c - a.y * s, a.x * s + a.y * c};
}

// A segment of radius r1 at a, r2 at b (a tapered capsule; equal radii a plain one).
double capsule(V2 p, V2 a, V2 b, double r1, double r2) {
  V2 ab = b - a, ap = p - a;
  double t = std::clamp(dot(ap, ab) / std::max(dot(ab, ab), 1e-12), 0.0, 1.0);
  return len(ap - ab * t) - (r1 + (r2 - r1) * t);
}
double capsule(V2 p, V2 a, V2 b, double r) { return capsule(p, a, b, r, r); }
double circle(V2 p, V2 c, double r) { return len(p - c) - r; }
// A box along axis u from a, `length` long, `half` thick either side, corners rounded by rc.
double bar(V2 p, V2 a, V2 u, double length, double half, double rc = 0.004) {
  u = norm(u);
  V2 v = perp(u), d = p - a;
  double x = dot(d, u) - length / 2, y = dot(d, v);
  double qx = std::fabs(x) - (length / 2 - rc), qy = std::fabs(y) - (half - rc);
  return len({std::max(qx, 0.0), std::max(qy, 0.0)}) + std::min(std::max(qx, qy), 0.0) - rc;
}
// A convex polygon, its corners clockwise or not.
double polygon(V2 p, const std::vector<V2>& v) {
  double d = dot(p - v[0], p - v[0]);
  double s = 1;
  for (size_t i = 0, j = v.size() - 1; i < v.size(); j = i, ++i) {
    V2 e = v[j] - v[i], w = p - v[i];
    V2 b = w - e * std::clamp(dot(w, e) / dot(e, e), 0.0, 1.0);
    d = std::min(d, dot(b, b));
    bool c1 = p.y >= v[i].y, c2 = p.y < v[j].y, c3 = e.x * w.y > e.y * w.x;
    if ((c1 && c2 && c3) || (!c1 && !c2 && !c3)) s = -s;
  }
  return s * std::sqrt(d);
}
double smin(double a, double b, double k) {
  double h = std::clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
  return b + (a - b) * h - k * h * (1 - h);
}

// THE WOMAN'S FIGURE: hair to the jaw, narrower shoulders, the chest, a narrow waist and wider hips, slimmer arms
// and legs. The weapons hang at the same points as on the man's: her hips' edge and shoulders come out where his do.
double bodyWoman(V2 p) {
  const double cx = 0.5;
  double d = circle(p, {cx, 0.118}, 0.062);
  // hair: a bob -- the crown fuller than the head, falling straight past the jaw either side, wider than the face
  double hair = smin(circle(p, {cx, 0.108}, 0.072), bar(p, {cx, 0.11}, {0, 1}, 0.098, 0.079, 0.032), 0.025);
  d = std::min(d, hair);
  d = smin(d, capsule(p, {cx, 0.19}, {cx, 0.24}, 0.022), 0.015);
  double torso = polygon(p, {{cx - 0.138, 0.245}, {cx + 0.138, 0.245}, {cx + 0.13, 0.32}, {cx + 0.112, 0.37},
                             {cx + 0.09, 0.42}, {cx + 0.118, 0.47}, {cx + 0.148, 0.515}, {cx + 0.135, 0.57},
                             {cx - 0.135, 0.57}, {cx - 0.148, 0.515}, {cx - 0.118, 0.47}, {cx - 0.09, 0.42},
                             {cx - 0.112, 0.37}, {cx - 0.13, 0.32}}) - 0.02;
  d = smin(d, torso, 0.03);
  for (double s : {-1.0, 1.0}) {
    V2 shoulder{cx + s * 0.152, 0.275}, elbow{cx + s * 0.205, 0.435}, wrist{cx + s * 0.232, 0.58};
    d = smin(d, capsule(p, shoulder, elbow, 0.033, 0.028), 0.02);
    d = std::min(d, capsule(p, elbow, wrist, 0.028, 0.022));
    d = std::min(d, circle(p, wrist + V2{s * 0.004, 0.027}, 0.026));
    V2 hip{cx + s * 0.08, 0.56}, knee{cx + s * 0.082, 0.765}, ankle{cx + s * 0.084, 0.945};
    d = smin(d, capsule(p, hip, knee, 0.058, 0.042), 0.02);
    d = std::min(d, capsule(p, knee, ankle, 0.042, 0.03));
    d = std::min(d, capsule(p, ankle, ankle + V2{s * 0.026, 0.022}, 0.02));
  }
  return d;
}

// THE FIGURE, standing, arms a little out; the unit square's centre line is its middle.
double bodyMan(V2 p);
double body(V2 p, bool woman) { return woman ? bodyWoman(p) : bodyMan(p); }
double bodyMan(V2 p) {
  const double cx = 0.5;
  double d = circle(p, {cx, 0.115}, 0.068);
  d = smin(d, capsule(p, {cx, 0.17}, {cx, 0.235}, 0.03), 0.02);
  double torso = polygon(p, {{cx - 0.165, 0.245}, {cx + 0.165, 0.245}, {cx + 0.118, 0.50}, {cx + 0.14, 0.57},
                             {cx - 0.14, 0.57}, {cx - 0.118, 0.50}}) - 0.02;
  d = smin(d, torso, 0.03);
  for (double s : {-1.0, 1.0}) {
    V2 shoulder{cx + s * 0.175, 0.275}, elbow{cx + s * 0.225, 0.44}, wrist{cx + s * 0.245, 0.585};
    d = smin(d, capsule(p, shoulder, elbow, 0.04, 0.034), 0.02);
    d = std::min(d, capsule(p, elbow, wrist, 0.034, 0.027));
    d = std::min(d, circle(p, wrist + V2{s * 0.004, 0.03}, 0.03));
    V2 hip{cx + s * 0.075, 0.56}, knee{cx + s * 0.088, 0.765}, ankle{cx + s * 0.092, 0.945};
    d = smin(d, capsule(p, hip, knee, 0.062, 0.047), 0.02);
    d = std::min(d, capsule(p, knee, ankle, 0.047, 0.036));
    d = std::min(d, capsule(p, ankle, ankle + V2{s * 0.03, 0.022}, 0.024));
  }
  return d;
}

// THE WEAPONS, each from the point the hand holds it by (the grip; a blade's guard), `u` where the barrel or the
// blade points, `side` (+1/-1) the side its grip, trigger or edge turns to.
double pistol(V2 p, V2 grip, V2 u, double side) {
  u = norm(u);
  V2 v = perp(u) * side;
  double d = bar(p, grip - u * 0.025, u, 0.205, 0.026, 0.007);                // slide and barrel
  V2 down = norm(v * -1 + u * -0.28);                                           // the grip, raked back
  d = std::min(d, bar(p, grip + v * -0.005, down, 0.115, 0.027, 0.009));
  d = std::min(d, std::fabs(circle(p, grip + u * 0.055 + v * -0.042, 0.026)) - 0.007);   // trigger guard
  return d;
}
double rifle(V2 p, V2 grip, V2 u, double side) {
  u = norm(u);
  V2 v = perp(u) * side;
  double d = bar(p, grip - u * 0.15, u, 0.13, 0.03, 0.01);                     // stock
  d = std::min(d, bar(p, grip - u * 0.03, u, 0.22, 0.026, 0.006));             // receiver
  d = std::min(d, bar(p, grip + u * 0.18, u, 0.22, 0.011, 0.004));            // barrel
  d = std::min(d, bar(p, grip + u * 0.07, norm(v * -1 + u * 0.15), 0.085, 0.017, 0.005));  // magazine
  d = std::min(d, bar(p, grip - u * 0.02, norm(v * -1 + u * -0.3), 0.06, 0.016, 0.005));    // grip
  return d;
}
double sword(V2 p, V2 guard, V2 u, double side, double length = 0.44) {
  u = norm(u);
  V2 v = perp(u) * side;
  double d = capsule(p, guard - u * 0.11, guard - u * 0.012, 0.016);          // handle
  d = std::min(d, circle(p, guard - u * 0.118, 0.019));                         // pommel
  d = std::min(d, bar(p, guard - v * 0.034, v, 0.068, 0.009, 0.004));          // guard
  // the blade, a little curved toward the edge side, tapering to the point
  V2 mid = guard + u * (length * 0.55) + v * 0.012, tip = guard + u * length + v * 0.03;
  d = std::min(d, capsule(p, guard + u * 0.01, mid, 0.0135, 0.012));
  d = std::min(d, capsule(p, mid, tip, 0.012, 0.002));
  return d;
}
double knife(V2 p, V2 guard, V2 u, double side) {
  u = norm(u);
  V2 v = perp(u) * side;
  double d = capsule(p, guard - u * 0.065, guard - u * 0.008, 0.016);
  d = std::min(d, bar(p, guard - v * 0.026, v, 0.052, 0.008, 0.003));
  d = std::min(d, capsule(p, guard + u * 0.008, guard + u * 0.11 + v * 0.01, 0.018, 0.002));
  return d;
}
// A strap round a limb or over a shoulder.
double strap(V2 p, V2 a, V2 b) { return capsule(p, a, b, 0.008); }

struct Glyph {
  std::function<double(V2)> sdf;
  bool behind = false;   // on the back: faint where the body covers it
};

// Every place as a right-handed player has it, seen as in a mirror: the character's right is +x. The left ones are
// the right ones turned over.
bool glyph(const std::string& place, const std::string& family, Glyph& g) {
  const double cx = 0.5;
  bool mirror = false;
  std::string at = place;
  static const char* pairs[][2] = {{"hip_l", "hip_r"}, {"thigh_l", "thigh_r"}, {"underarm_r", "underarm_l"},
                                   {"chest_r", "chest_l"}, {"shoulder_l", "shoulder_r"}};
  // hip_l is a place of its own for a blade (the cross draw), the mirror of hip_r for a gun
  for (auto& pr : pairs) {
    if (place == pr[0] && !(place == "hip_l" && family == "blade") && !(place == "hip_r" && family == "blade")) {
      at = pr[1];
      mirror = true;
    }
  }
  if (place == "hip_r" && family == "blade") { at = "hip_l"; mirror = true; }
  std::function<double(V2)> f;
  bool behind = false;
  if (at == "hip_r" && (family == "gun" || family == "knife")) {
    if (family == "gun") f = [=](V2 p) { return pistol(p, {cx + 0.172, 0.49}, {0.06, 1}, 1); };
    else f = [=](V2 p) { return knife(p, {cx + 0.168, 0.52}, {0.05, 1}, 1); };
  } else if (at == "thigh_r") {
    f = [=](V2 p) {
      double d = family == "gun" ? pistol(p, {cx + 0.118, 0.615}, {0.02, 1}, 1)
                                 : knife(p, {cx + 0.112, 0.63}, {0.02, 1}, 1);
      d = std::min(d, strap(p, {cx + 0.03, 0.665}, {cx + 0.15, 0.672}));
      return std::min(d, strap(p, {cx + 0.035, 0.735}, {cx + 0.145, 0.74}));
    };
  } else if (at == "underarm_l") {
    // level, the muzzle out, the grip and its magazine down (the gun hangs top up)
    f = [=](V2 p) {
      double d = pistol(p, {cx - 0.095, 0.35}, {-1, 0.12}, 1);
      d = std::min(d, strap(p, {cx - 0.115, 0.33}, {cx - 0.09, 0.25}));
      return std::min(d, strap(p, {cx - 0.09, 0.25}, {cx + 0.07, 0.36}));
    };
  } else if (at == "chest_l") {
    f = [=](V2 p) {
      double d = knife(p, {cx - 0.075, 0.31}, {0.05, 1}, -1);
      return std::min(d, strap(p, {cx - 0.13, 0.30}, {cx - 0.02, 0.33}));
    };
  } else if (at == "hip_l" && family == "blade") {
    // the cross draw: the handle forward over the belly, the blade down and back past the left leg
    f = [=](V2 p) { return sword(p, {cx - 0.115, 0.49}, {-0.5, 1}, 1, 0.42); };
  } else if (at == "shoulder_r") {
    behind = true;
    if (family == "long") f = [=](V2 p) { return rifle(p, {cx + 0.14, 0.25}, {-0.62, 0.78}, 1); };
    else f = [=](V2 p) { return sword(p, {cx + 0.115, 0.215}, {-0.6, 0.8}, -1, family == "knife" ? 0.2 : 0.47); };
  } else if (at == "lower_back") {
    behind = true;
    // a gun's top up across the back: its magazine (and a pistol's grip) down
    if (family == "long") f = [=](V2 p) { return rifle(p, {cx + 0.12, 0.535}, {-1, 0}, 1); };
    else if (family == "blade") f = [=](V2 p) { return sword(p, {cx + 0.13, 0.535}, {-1, 0.02}, 1, 0.42); };
    else if (family == "knife") f = [=](V2 p) { return knife(p, {cx + 0.1, 0.535}, {-1, 0}, 1); };
    else f = [=](V2 p) { return pistol(p, {cx + 0.08, 0.535}, {-1, 0}, 1); };
  } else {
    return false;
  }
  if (mirror) g.sdf = [f](V2 p) { return f({1 - p.x, p.y}); };
  else g.sdf = f;
  g.behind = behind;
  return true;
}

struct Part {
  std::string name;
  int w = 0, h = 0, x = 0, y = 0;
  std::vector<float> alpha;
};

double cover(double sdfPx) { return std::clamp(0.5 - sdfPx, 0.0, 1.0); }

template <class F>
void render(Part& part, int size, F alphaAt) {
  part.w = part.h = size;
  part.alpha.assign(size_t(size) * size, 0.0f);
  for (int py = 0; py < size; ++py) {
    for (int px = 0; px < size; ++px) {
      double sum = 0.0;
      for (int sy = 0; sy < 4; ++sy) {
        for (int sx = 0; sx < 4; ++sx) {
          V2 p{(px + (sx + 0.5) / 4.0) / size, (py + (sy + 0.5) / 4.0) / size};
          sum += alphaAt(p);
        }
      }
      part.alpha[size_t(py) * size + px] = float(std::clamp(sum / 16.0, 0.0, 1.0));
    }
  }
}

bool make(const std::string& spec, int size, Part& part) {
  part.name = spec;
  const double px = 1.0 / size;
  if (spec == "body" || spec == "body_w") {
    // a soft fill and a crisp outline: tinted dim, it stays the ground the weapon is read on
    const bool woman = spec == "body_w";
    render(part, size, [&](V2 p) {
      double d = body(p, woman) / px;
      double fill = cover(d) * 0.42;
      double line = std::clamp(1.6 - std::fabs(d + 1.0), 0.0, 1.0);
      return std::max(fill, line);
    });
    return true;
  }
  if (spec == "none") {
    render(part, size, [&](V2 p) {
      double ring = std::fabs(circle(p, {0.5, 0.5}, 0.33)) - 0.026;
      double slash = capsule(p, {0.5 - 0.23, 0.5 + 0.23}, {0.5 + 0.23, 0.5 - 0.23}, 0.026);
      return cover(std::min(ring, slash) / px);
    });
    return true;
  }
  // <place>:<family>[:w] -- ":w" over the woman's figure (only a weapon on the back differs: it is cut by the body)
  size_t colon = spec.find(':');
  if (colon == std::string::npos) return false;
  std::string family = spec.substr(colon + 1);
  bool woman = false;
  size_t second = family.find(':');
  if (second != std::string::npos) {
    if (family.substr(second + 1) != "w") return false;
    woman = true;
    family = family.substr(0, second);
  }
  Glyph g;
  if (!glyph(spec.substr(0, colon), family, g)) return false;
  part.name = spec.substr(0, colon) + "_" + family + (woman ? "_w" : "");
  render(part, size, [&](V2 p) {
    double w = cover(g.sdf(p) / px);
    if (w <= 0) return 0.0;
    if (!g.behind) return w;
    // behind the body: faint inside it, a hair-thin gap at its outline so the two read apart
    double b = body(p, woman) / px;
    double inside = std::clamp(0.5 - b, 0.0, 1.0);
    double gap = std::clamp(std::fabs(b) - 0.6, 0.0, 1.0);
    return w * (1.0 - inside * 0.55) * (inside > 0.99 ? 1.0 : gap);
  });
  return true;
}

bool pack(std::vector<Part>& parts, int width, int height) {
  int x = kPad, y = kPad, shelf = 0;
  for (auto& p : parts) {
    if (x + p.w + kPad > width) { x = kPad; y += shelf + kPad; shelf = 0; }
    if (y + p.h + kPad > height) return false;
    p.x = x; p.y = y;
    x += p.w + kPad;
    shelf = std::max(shelf, p.h);
  }
  return true;
}

bool writeTga(const char* path, const std::vector<Part>& parts, int width, int height) {
  std::vector<uint8_t> px(size_t(width) * height * 4, 0);
  for (const auto& p : parts)
    for (int y = 0; y < p.h; ++y)
      for (int x = 0; x < p.w; ++x) {
        size_t at = (size_t(p.y + y) * width + (p.x + x)) * 4;
        uint8_t a = uint8_t(std::lround(p.alpha[size_t(y) * p.w + x] * 255.0));
        px[at] = px[at + 1] = px[at + 2] = px[at + 3] = a;
      }
  FILE* f = std::fopen(path, "wb");
  if (!f) return false;
  uint8_t header[18] = {};
  header[2] = 2;
  header[12] = uint8_t(width & 255); header[13] = uint8_t(width >> 8);
  header[14] = uint8_t(height & 255); header[15] = uint8_t(height >> 8);
  header[16] = 32; header[17] = 8 | 0x20;
  std::fwrite(header, 1, sizeof header, f);
  std::fwrite(px.data(), 1, px.size(), f);   // white: BGRA and RGBA are the same bytes
  return std::fclose(f) == 0;
}

bool writeJson(const char* path, const std::vector<Part>& parts, int width, int height) {
  FILE* f = std::fopen(path, "wb");
  if (!f) return false;
  std::fprintf(f, "{\"width\": %d, \"height\": %d, \"parts\": {\n", width, height);
  for (size_t i = 0; i < parts.size(); ++i) {
    const auto& p = parts[i];
    std::fprintf(f, "  \"%s\": {\"x\": %d, \"y\": %d, \"w\": %d, \"h\": %d}%s\n", p.name.c_str(), p.x, p.y, p.w, p.h,
                 i + 1 < parts.size() ? "," : "");
  }
  std::fprintf(f, "}}\n");
  return std::fclose(f) == 0;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 7) {
    std::fprintf(stderr, "usage: holster_icons <sheet.tga> <parts.json> <width> <height> <size> <part>...\n");
    return 2;
  }
  int width = std::atoi(argv[3]), height = std::atoi(argv[4]), size = std::atoi(argv[5]);
  std::vector<Part> parts;
  for (int i = 6; i < argc; ++i) {
    Part p;
    if (!make(argv[i], size, p)) {
      std::fprintf(stderr, "unknown part: %s\n", argv[i]);
      return 2;
    }
    parts.push_back(std::move(p));
  }
  if (!pack(parts, width, height)) {
    std::fprintf(stderr, "the parts do not fit in %d x %d\n", width, height);
    return 1;
  }
  if (!writeTga(argv[1], parts, width, height) || !writeJson(argv[2], parts, width, height)) {
    std::fprintf(stderr, "write failed\n");
    return 1;
  }
  std::printf("%zu parts\n", parts.size());
  return 0;
}
