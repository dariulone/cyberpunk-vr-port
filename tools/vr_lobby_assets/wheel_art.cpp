// The weapon wheel's cell art (mods/cet/CyberpunkVRPort_Menu modules/wheel.lua): annular wedges and discs drawn
// from exact signed distances, white premultiplied by the shape's alpha, packed into one sheet.
//
//   wheel_art.exe <sheet.tga> <parts.json> <width> <height> <shape>...
//     wedge:<name>:<inner>:<outer>:<half span deg>:<edge inset>:<corner radius>
//     disc:<name>:<radius>
//
// Each shape gives three parts (a disc two): <name>_fill (a radial gradient, brighter outwards, with a sheen
// along the edge), <name>_edge (a 2.5 px stroke just inside the boundary and its soft glow both ways) and, for
// a wedge, <name>_band (an accent strip along the outer arc). A wedge is drawn pointing up from the wheel's
// centre; parts.json gives every part's rect in the sheet and where the wheel's centre (a disc's own centre)
// lies in the part's pixels, so the page can turn it about that point.

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

namespace {

constexpr double kPi = 3.14159265358979323846;
constexpr int kMargin = 16;   // room for the glow round each part
constexpr int kPad = 4;       // between parts in the sheet

struct V2 { double x, y; };

double len(V2 a) { return std::sqrt(a.x * a.x + a.y * a.y); }
double dot(V2 a, V2 b) { return a.x * b.x + a.y * b.y; }

double segment(V2 p, V2 a, V2 b) {
  V2 ab{b.x - a.x, b.y - a.y}, ap{p.x - a.x, p.y - a.y};
  double t = std::clamp(dot(ap, ab) / std::max(dot(ab, ab), 1e-12), 0.0, 1.0);
  return len({ap.x - ab.x * t, ap.y - ab.y * t});
}

// An annular sector about the up axis (y up): r1..r2, |angle| <= half, its straight edges moved in by `inset`
// (so neighbouring wedges keep a gap of constant width) and its corners rounded by rc. Exact: the rounded shape
// is the core (everything moved in by rc more) grown by rc, and the core's distance is the distance to its two
// arcs and its straight edge.
struct Sector {
  double r1, r2, half, inset, rc;
  double rho1, rho2, off;   // the core's radii, its edge line's offset
  V2 u, n, c1, c2;          // edge direction and outward normal; the core's corners on the right
  double a1, a2;            // the corners' angles from the axis

  Sector(double r1_, double r2_, double halfDeg, double inset_, double rc_)
      : r1(r1_), r2(r2_), half(halfDeg * kPi / 180.0), inset(inset_), rc(rc_) {
    rho1 = r1 + rc;
    rho2 = r2 - rc;
    off = inset + rc;
    u = {std::sin(half), std::cos(half)};
    n = {std::cos(half), -std::sin(half)};
    auto corner = [&](double rho) {
      double t = std::sqrt(std::max(rho * rho - off * off, 0.0));
      return V2{-off * n.x + t * u.x, -off * n.y + t * u.y};
    };
    c1 = corner(rho1);
    c2 = corner(rho2);
    a1 = std::atan2(c1.x, c1.y);
    a2 = std::atan2(c2.x, c2.y);
  }

  double sdf(V2 p) const {
    p.x = std::fabs(p.x);
    double r = len(p);
    double a = std::atan2(p.x, p.y);
    double dOuter = a <= a2 ? std::fabs(r - rho2) : len({p.x - c2.x, p.y - c2.y});
    double dInner = a <= a1 ? std::fabs(r - rho1) : len({p.x - c1.x, p.y - c1.y});
    double dEdge = segment(p, c1, c2);
    double d = std::min({dOuter, dInner, dEdge});
    bool inside = r >= rho1 && r <= rho2 && dot(p, n) <= -off;
    return (inside ? -d : d) - rc;
  }
};

struct Part {
  std::string name;
  int w = 0, h = 0, x = 0, y = 0;
  double ox = 0, oy = 0;      // the centre, in the part's pixels (y down)
  std::vector<float> alpha;
};

double cover(double sdf) { return std::clamp(0.5 - sdf, 0.0, 1.0); }

// 4 x 4 samples per pixel: the strokes are a couple of pixels wide.
template <class F>
void render(Part& part, F alphaAt) {
  part.alpha.assign(size_t(part.w) * part.h, 0.0f);
  for (int py = 0; py < part.h; ++py) {
    for (int px = 0; px < part.w; ++px) {
      double sum = 0.0;
      for (int sy = 0; sy < 4; ++sy) {
        for (int sx = 0; sx < 4; ++sx) {
          double x = px + (sx + 0.5) / 4.0 - part.ox;
          double y = part.oy - (py + (sy + 0.5) / 4.0);
          sum += alphaAt(V2{x, y});
        }
      }
      part.alpha[size_t(py) * part.w + px] = float(std::clamp(sum / 16.0, 0.0, 1.0));
    }
  }
}

double strokeAlpha(double sdf) {
  constexpr double centre = -1.75, halfWidth = 1.25;
  double d = std::fabs(sdf - centre);
  double stroke = std::clamp(halfWidth + 0.5 - d, 0.0, 1.0);
  double glow = 0.42 * std::exp(-std::max(0.0, d - halfWidth) / 3.5);
  return std::max(stroke, glow);
}

void wedge(std::vector<Part>& parts, const std::string& name, double r1, double r2, double halfDeg, double inset,
           double rc) {
  Sector body(r1, r2, halfDeg, inset, rc);
  Sector band(r2 - 17.0, r2 - 8.0, halfDeg, inset + 14.0, 3.5);
  double half = halfDeg * kPi / 180.0;
  int w = int(std::ceil(2.0 * ((r2 + kMargin) * std::sin(half))));
  double top = r2 + kMargin, bottom = r1 * std::cos(half) - kMargin;
  int h = int(std::ceil(top - bottom));
  auto make = [&](const char* suffix) {
    Part p;
    p.name = name + suffix;
    p.w = w;
    p.h = h;
    p.ox = w / 2.0;
    p.oy = top;
    return p;
  };
  Part fill = make("_fill"), edge = make("_edge"), strip = make("_band");
  render(fill, [&](V2 p) {
    double s = body.sdf(p);
    if (s > 0.5) return 0.0;
    double t = std::clamp((len(p) - r1) / (r2 - r1), 0.0, 1.0);
    double sheen = 0.30 * std::exp(-std::max(0.0, -s) / 7.0);
    return cover(s) * std::min(1.0, 0.28 + 0.72 * std::pow(t, 1.6) + sheen);
  });
  render(edge, [&](V2 p) { return strokeAlpha(body.sdf(p)); });
  render(strip, [&](V2 p) {
    double s = band.sdf(p);
    return std::max(cover(s), 0.35 * std::exp(-std::max(0.0, s) / 3.0));
  });
  parts.push_back(std::move(fill));
  parts.push_back(std::move(edge));
  parts.push_back(std::move(strip));
}

void disc(std::vector<Part>& parts, const std::string& name, double radius) {
  int size = int(std::ceil(2.0 * (radius + kMargin)));
  auto make = [&](const char* suffix) {
    Part p;
    p.name = name + suffix;
    p.w = p.h = size;
    p.ox = p.oy = size / 2.0;
    return p;
  };
  Part fill = make("_fill"), edge = make("_edge");
  render(fill, [&](V2 p) {
    double s = len(p) - radius;
    if (s > 0.5) return 0.0;
    double t = std::clamp(len(p) / radius, 0.0, 1.0);
    double sheen = 0.30 * std::exp(-std::max(0.0, -s) / 6.0);
    return cover(s) * std::min(1.0, 0.22 + 0.78 * t * t + sheen);
  });
  render(edge, [&](V2 p) { return strokeAlpha(len(p) - radius); });
  parts.push_back(std::move(fill));
  parts.push_back(std::move(edge));
}

std::vector<std::string> split(const std::string& s, char sep) {
  std::vector<std::string> out;
  size_t from = 0;
  for (;;) {
    size_t at = s.find(sep, from);
    out.push_back(s.substr(from, at - from));
    if (at == std::string::npos) return out;
    from = at + 1;
  }
}

// Shelves, tallest first.
bool pack(std::vector<Part>& parts, int width, int height) {
  std::vector<Part*> order;
  for (auto& p : parts) order.push_back(&p);
  std::stable_sort(order.begin(), order.end(), [](Part* a, Part* b) { return a->h > b->h; });
  int x = kPad, y = kPad, shelf = 0;
  for (Part* p : order) {
    if (x + p->w + kPad > width) {
      x = kPad;
      y += shelf + kPad;
      shelf = 0;
    }
    if (y + p->h + kPad > height) return false;
    p->x = x;
    p->y = y;
    x += p->w + kPad;
    shelf = std::max(shelf, p->h);
  }
  return true;
}

// White, premultiplied as the game's UI textures are (ink blends them so: colour = alpha, black where nothing
// is). A white texel under zero alpha is not nothing to ink: it lights whatever the part's rect lies over.
bool writeTga(const char* path, const std::vector<Part>& parts, int width, int height) {
  std::vector<uint8_t> px(size_t(width) * height * 4, 0);
  for (const auto& p : parts) {
    for (int y = 0; y < p.h; ++y) {
      for (int x = 0; x < p.w; ++x) {
        size_t at = (size_t(p.y + y) * width + (p.x + x)) * 4;
        uint8_t a = uint8_t(std::lround(p.alpha[size_t(y) * p.w + x] * 255.0));
        px[at] = px[at + 1] = px[at + 2] = px[at + 3] = a;
      }
    }
  }
  FILE* f = std::fopen(path, "wb");
  if (!f) return false;
  uint8_t header[18] = {};
  header[2] = 2;   // uncompressed true colour
  header[12] = uint8_t(width & 255);
  header[13] = uint8_t(width >> 8);
  header[14] = uint8_t(height & 255);
  header[15] = uint8_t(height >> 8);
  header[16] = 32;
  header[17] = 8 | 0x20;   // 8 alpha bits, top-left origin
  std::fwrite(header, 1, sizeof header, f);
  std::vector<uint8_t> bgra(px.size());
  for (size_t i = 0; i < px.size(); i += 4) {
    bgra[i] = px[i + 2];
    bgra[i + 1] = px[i + 1];
    bgra[i + 2] = px[i];
    bgra[i + 3] = px[i + 3];
  }
  std::fwrite(bgra.data(), 1, bgra.size(), f);
  return std::fclose(f) == 0;
}

bool writeJson(const char* path, const std::vector<Part>& parts, int width, int height) {
  FILE* f = std::fopen(path, "wb");
  if (!f) return false;
  std::fprintf(f, "{\"width\": %d, \"height\": %d, \"parts\": {\n", width, height);
  for (size_t i = 0; i < parts.size(); ++i) {
    const auto& p = parts[i];
    std::fprintf(f, "  \"%s\": {\"x\": %d, \"y\": %d, \"w\": %d, \"h\": %d, \"ox\": %.3f, \"oy\": %.3f}%s\n",
                 p.name.c_str(), p.x, p.y, p.w, p.h, p.ox, p.oy, i + 1 < parts.size() ? "," : "");
  }
  std::fprintf(f, "}}\n");
  return std::fclose(f) == 0;
}

}  // namespace

int main(int argc, char** argv) {
  if (argc < 6) {
    std::fprintf(stderr, "usage: wheel_art <sheet.tga> <parts.json> <width> <height> <shape>...\n");
    return 2;
  }
  int width = std::atoi(argv[3]), height = std::atoi(argv[4]);
  std::vector<Part> parts;
  for (int i = 5; i < argc; ++i) {
    auto f = split(argv[i], ':');
    if (f[0] == "wedge" && f.size() == 7) {
      wedge(parts, f[1], std::atof(f[2].c_str()), std::atof(f[3].c_str()), std::atof(f[4].c_str()),
            std::atof(f[5].c_str()), std::atof(f[6].c_str()));
    } else if (f[0] == "disc" && f.size() == 3) {
      disc(parts, f[1], std::atof(f[2].c_str()));
    } else {
      std::fprintf(stderr, "bad shape: %s\n", argv[i]);
      return 2;
    }
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
