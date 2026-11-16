// HAIR THAT THE HANDS MOVE. A person's hair (and a coat's tails, a pair of earrings) swings by the animation's dangle
// simulations: an animDangleConstraint_SimulationDyng (particles on the hair's bones, linked and coned, against rounded
// collision shapes on the head and shoulders) or a pendulum per bone, in the anim graph of an animated component of
// the person's (Judy's hair: hh_051_wa__judy_dangle.animgraph, 26 particles, 19 of them free). The hands are no bone of
// that rig, so they are given to it as what the simulation already takes from outside: its external force, world space,
// which every particle feels (gravity's company). Natives/PhysHands-side code decides when a hand goes through the
// hair and how hard; this file only finds the simulations and sets the force.
//
// The simulations are the graph's, which is a resource: every person who wears that hair shares it. A force given for
// one is felt by all who wear the same at that moment, so it is given only while a hand is in someone's hair, and
// taken back the moment it is not.
#include "Natives/NativeFunctions.hpp"
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/AnimatedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimGraph.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimNode_Base.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/AnimNode_Dangle.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/DangleConstraint_Simulation.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/DangleConstraint_SimulationDyng.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/DangleConstraint_SimulationPendulum.hpp>
#include <RED4ext/Scripting/Natives/Generated/anim/Rig.hpp>
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <string>
#include <unordered_map>
#include <vector>
#include <windows.h>

namespace {
template <typename T>
T Param(RED4ext::CStackFrame* frame) {
    T value{};
    RED4ext::GetParameter(frame, &value);
    return value;
}

// Each dangle simulation in the entity's animated components' graphs: fn(component, simulation, its class's name).
// The graph each was found in, its resource's path hash, to `graphOf` when given (the last one's).
uint64_t g_lastGraph = 0;
template <typename Fn>
int EachDangle(RED4ext::ent::Entity* entity, Fn&& fn) {
    if (!entity) return 0;
    auto* rtti = RED4ext::CRTTISystem::Get();
    static RED4ext::CClass* animated = rtti ? rtti->GetClass("entAnimatedComponent") : nullptr;
    static RED4ext::CClass* dangleNode = rtti ? rtti->GetClass("animAnimNode_Dangle") : nullptr;
    if (!animated || !dangleNode) return 0;
    int n = 0;
    for (const auto& handle : entity->components) {
        auto* component = handle.GetPtr();
        if (!component || !component->GetType()->IsA(animated)) continue;
        auto* ac = reinterpret_cast<RED4ext::ent::AnimatedComponent*>(component);
        auto& token = ac->graph.token;
        if (!token || !token->IsLoaded()) continue;
        auto* graph = token->resource.GetPtr();
        if (!graph) continue;
        for (const auto& nodeHandle : graph->nodesToInit) {
            auto* node = nodeHandle.GetPtr();
            if (!node || !node->GetType()->IsA(dangleNode)) continue;
            auto* sim = reinterpret_cast<RED4ext::anim::AnimNode_Dangle*>(node)->dangleConstraint.GetPtr();
            if (!sim) continue;
            g_lastGraph = token->path.hash;
            fn(ac, sim, sim->GetType()->GetName());
            ++n;
        }
    }
    return n;
}
}  // namespace

static const RED4ext::CName kDyng("animDangleConstraint_SimulationDyng");
static const RED4ext::CName kPendulum("animDangleConstraint_SimulationPendulum");

// Outside any __try's frame: nothing here needs unwinding.
static void Describe(RED4ext::ent::Entity* entity, std::string* text) {
    EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        char line[320];
        if (kind == kDyng) {
            auto* d = reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim);
            std::snprintf(line, sizeof(line), "%s: dyng %u particles, %u shapes, force %.2f %.2f %.2f, gravity %.2f (graph %016llx, sim %p); ",
                          ac->name.ToString(), d->particlesContainer.particles.size(), sim->collisionRoundedShapes.size(),
                          d->particlesContainer.externalForceWS.X, d->particlesContainer.externalForceWS.Y,
                          d->particlesContainer.externalForceWS.Z, d->particlesContainer.gravityWS,
                          static_cast<unsigned long long>(g_lastGraph), static_cast<void*>(sim));
        } else if (kind == kPendulum) {
            auto* p = reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationPendulum*>(sim);
            std::snprintf(line, sizeof(line), "%s: pendulum, %u shapes, force %.2f %.2f %.2f; ", ac->name.ToString(),
                          sim->collisionRoundedShapes.size(), p->externalForceWS.X, p->externalForceWS.Y, p->externalForceWS.Z);
        } else {
            std::snprintf(line, sizeof(line), "%s: %s; ", ac->name.ToString(), kind.ToString());
        }
        *text += line;
    });
}

static bool SafeDescribe(RED4ext::ent::Entity* entity, std::string* text) {
    __try {
        Describe(entity, text);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// THE HAIR ALONE, where the entity's components say which is hair: a name with "hh_" or "hair" in it (the game's hair
// meshes and their dangles are hh_...). An earring's and a coat's tails swung with the hair at a hand in it. With none so
// named, every dangle, as before; a force taken back (zero) from every one.
static bool HairName(RED4ext::CName name) {
    const char* s = name.ToString();
    if (!s) return false;
    char low[160];
    size_t i = 0;
    for (; s[i] && i < sizeof(low) - 1; ++i) low[i] = char(std::tolower(static_cast<unsigned char>(s[i])));
    low[i] = 0;
    return std::strstr(low, "hh_") || std::strstr(low, "hair");
}

static int Force(RED4ext::ent::Entity* entity, const float* force) {
    bool anyHair = false;
    const bool zero = force[0] == 0.f && force[1] == 0.f && force[2] == 0.f;
    if (!zero)
        EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation*, RED4ext::CName) {
            if (HairName(ac->name)) anyHair = true;
        });
    return EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        if (anyHair && !HairName(ac->name)) return;
        RED4ext::Vector3* f = nullptr;
        if (kind == kDyng) f = &reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer.externalForceWS;
        else if (kind == kPendulum) f = &reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationPendulum*>(sim)->externalForceWS;
        if (f) f->X = force[0], f->Y = force[1], f->Z = force[2];
    });
}

// THE BREASTS: a component named so ("breast", "boob").
static bool BreastName(RED4ext::CName name) {
    const char* s = name.ToString();
    if (!s) return false;
    char low[160];
    size_t i = 0;
    for (; s[i] && i < sizeof(low) - 1; ++i) low[i] = char(std::tolower(static_cast<unsigned char>(s[i])));
    low[i] = 0;
    return std::strstr(low, "breast") || std::strstr(low, "boob");
}

// One kind of dangle (0 the hair, 1 the breasts, 2 any), all of that kind (index < 0) or the index-th of them.
static int PartForce(RED4ext::ent::Entity* entity, const float* force, int part, int index) {
    int seen = 0;
    int set = 0;
    EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        const bool match = part == 0 ? HairName(ac->name) : part == 1 ? BreastName(ac->name) : true;
        if (!match) return;
        const int at = seen++;
        if (index >= 0 && at != index) return;
        RED4ext::Vector3* f = nullptr;
        if (kind == kDyng) f = &reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer.externalForceWS;
        else if (kind == kPendulum) f = &reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationPendulum*>(sim)->externalForceWS;
        if (!f) return;
        f->X = force[0], f->Y = force[1], f->Z = force[2];
        ++set;
    });
    return set;
}

static int SafePartForce(RED4ext::ent::Entity* entity, const float* force, int part, int index) {
    __try {
        return PartForce(entity, force, part, index);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

static int SafeForce(RED4ext::ent::Entity* entity, const float* force) {
    __try {
        return Force(entity, force);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

// WHERE A PERSON'S HAIR IS. Each hair dyng particle's place in its rig's reference pose, in the frame of that rig's Head
// bone (the head's X up, Y to the back, Z to the right), and the particle it hangs from. A hand is tried against these
// chains rather than a line down the middle of the head: a shaved side has no hair to move, and a hand there moved all
// of it the other way.
namespace {
struct Quat {
    float i, j, k, r;
};
Quat QMul(const Quat& a, const Quat& b) {
    return {a.r * b.i + a.i * b.r + a.j * b.k - a.k * b.j, a.r * b.j - a.i * b.k + a.j * b.r + a.k * b.i,
            a.r * b.k + a.i * b.j - a.j * b.i + a.k * b.r, a.r * b.r - a.i * b.i - a.j * b.j - a.k * b.k};
}
void QRot(const Quat& q, const float* v, float* out) {
    const float tx = 2.f * (q.j * v[2] - q.k * v[1]);
    const float ty = 2.f * (q.k * v[0] - q.i * v[2]);
    const float tz = 2.f * (q.i * v[1] - q.j * v[0]);
    out[0] = v[0] + q.r * tx + (q.j * tz - q.k * ty);
    out[1] = v[1] + q.r * ty + (q.k * tx - q.i * tz);
    out[2] = v[2] + q.r * tz + (q.i * ty - q.j * tx);
}
Quat QInv(const Quat& q) {
    return {-q.i, -q.j, -q.k, q.r};
}

// A hair rig's reference pose in its model space (each bone from its parent; the parents come first) and its Head bone.
struct RigPose {
    std::vector<float> pos;
    std::vector<Quat> rot;
    int head = -1;
};
bool BuildPose(RED4ext::anim::Rig* rig, RigPose& out) {
    static const RED4ext::CName headName("Head");
    const int n = rig ? int(rig->boneNames.size()) : 0;
    if (n <= 0 || n > 1024 || !rig->parentIndices || !rig->referencePoseLS) return false;
    out.pos.assign(size_t(n) * 3, 0.f);
    out.rot.assign(n, Quat{0.f, 0.f, 0.f, 1.f});
    out.head = -1;
    for (int b = 0; b < n; ++b) {
        const auto& t = rig->referencePoseLS[b];
        const float lt[3] = {t.Translation.X, t.Translation.Y, t.Translation.Z};
        const Quat lr{t.Rotation.i, t.Rotation.j, t.Rotation.k, t.Rotation.r};
        const int p = rig->parentIndices[b];
        if (p >= 0 && p < b) {
            float w[3];
            QRot(out.rot[p], lt, w);
            for (int k = 0; k < 3; ++k) out.pos[b * 3 + k] = out.pos[p * 3 + k] + w[k];
            out.rot[b] = QMul(out.rot[p], lr);
        } else {
            for (int k = 0; k < 3; ++k) out.pos[b * 3 + k] = lt[k];
            out.rot[b] = lr;
        }
        if (out.head < 0 && rig->boneNames[b] == headName) out.head = b;
    }
    return out.head >= 0;
}

RED4ext::anim::Rig* RigOf(RED4ext::ent::AnimatedComponent* ac) {
    auto& token = ac->rig.token;
    if (!token || !token->IsLoaded()) return nullptr;
    return token->resource.GetPtr();
}
}  // namespace

static void Points(RED4ext::ent::Entity* entity, std::string* text) {
    int count = 0;
    EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        if (kind != kDyng || !HairName(ac->name)) return;
        auto* rig = RigOf(ac);
        RigPose pose;
        if (!BuildPose(rig, pose)) return;
        const int n = int(rig->boneNames.size());
        const auto& pos = pose.pos;
        const auto& rot = pose.rot;
        const int head = pose.head;
        auto& parts = reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer.particles;
        const int m = int(parts.size());
        std::vector<int> boneOf(m, -1), atBone(n, -1), place(m, -1);
        int c = 0;
        for (int i = 0; i < m; ++i) {
            for (int b = 0; b < n; ++b)
                if (rig->boneNames[b] == parts[i].bone.name) {
                    boneOf[i] = b;
                    atBone[b] = i;
                    break;
                }
            if (boneOf[i] >= 0) place[i] = count + c++;
        }
        const Quat inv{-rot[head].i, -rot[head].j, -rot[head].k, rot[head].r};
        for (int i = 0; i < m; ++i) {
            const int b = boneOf[i];
            if (b < 0) continue;
            const float v[3] = {pos[b * 3] - pos[head * 3], pos[b * 3 + 1] - pos[head * 3 + 1], pos[b * 3 + 2] - pos[head * 3 + 2]};
            float l[3];
            QRot(inv, v, l);
            // the nearest particle up its bones (none for one on the scalp)
            int up = rig->parentIndices[b];
            for (int steps = 0; up >= 0 && up < n && atBone[up] < 0 && steps < n; ++steps) up = rig->parentIndices[up];
            const int parent = (up >= 0 && up < n && atBone[up] >= 0) ? place[atBone[up]] : -1;
            char line[96];
            std::snprintf(line, sizeof(line), "%.4f %.4f %.4f %d;", l[0], l[1], l[2], parent);
            *text += line;
        }
        count += c;
    });
}

static bool SafePoints(RED4ext::ent::Entity* entity, std::string* text) {
    __try {
        Points(entity, text);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

namespace {
struct RawArray {
    void* entries;
    uint32_t capacity;
    uint32_t size;
};
}  // namespace

// Raw bytes of the first hair dyng (a test tool's look): its shapes list's header and first shape, and the particle's.
static void Raw(RED4ext::ent::Entity* entity, int index, std::string* text) {
    bool done = false;
    EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        if (done || kind != kDyng || !HairName(ac->name)) return;
        done = true;
        auto* raw = reinterpret_cast<RawArray*>(&sim->collisionRoundedShapes);
        char line[160];
        std::snprintf(line, sizeof(line), "shapes %p cap %u size %u; ", raw->entries, raw->capacity, raw->size);
        *text += line;
        auto hex = [&](const void* p, size_t bytes) {
            const auto* b = static_cast<const uint8_t*>(p);
            for (size_t i = 0; i < bytes; ++i) {
                char h[4];
                std::snprintf(h, sizeof(h), "%02x", b[i]);
                *text += h;
                if ((i & 3) == 3) *text += ' ';
            }
            *text += "; ";
        };
        if (raw->size > 0 && raw->capacity >= raw->size) {
            *text += "shape0 ";
            hex(raw->entries, sizeof(RED4ext::anim::CollisionRoundedShape));
        }
        auto& parts = reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer.particles;
        if (index >= 0 && uint32_t(index) < parts.size()) {
            std::snprintf(line, sizeof(line), "particle %d of %u (%s) ", index, parts.size(), parts[index].bone.name.ToString());
            *text += line;
            hex(&parts[index], sizeof(RED4ext::anim::DyngParticle));
        }
        *text += "container ";
        hex(&reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer, 0x38);
    });
}

static bool SafeRaw(RED4ext::ent::Entity* entity, int index, std::string* text) {
    __try {
        Raw(entity, index, text);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

// VRHairRaw(entity, particle) -> String: raw bytes of the entity's first hair dyng (a test tool's look).
void VRHairRaw(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::CString* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    const int32_t index = Param<int32_t>(frame);
    frame->code++;
    std::string text;
    if (!SafeRaw(entity.GetPtr(), index, &text)) text += "(failed)";
    if (out) *out = RED4ext::CString(text.c_str());
}

// EACH LOCK ITS OWN. The hair's force is one for the whole dyng, but the simulation divides it by each particle's mass
// (gravity it does not: a heavy particle hangs and swings as before). So while a hand is in the hair, each particle gets
// its own mass over its weight (1 where a hand is, down its lock from there), and the rest are made heavy enough that the
// force leaves them be: the force moves the locks the fingers are in, and the others stay. The hair's own masses are kept
// here and given back when the weights are taken out (an empty list). Shared, as the force: all who wear that hair have
// the same locks light while a hand is in one's.
namespace {
struct OwnMasses {
    uint64_t graph = 0;
    std::vector<float> mass;
};
std::unordered_map<void*, OwnMasses> g_ownMasses;  // simulation -> its particles' own masses, while weighted
constexpr float kLeastWeight = 1e-4f;
}  // namespace

static int Strands(RED4ext::ent::Entity* entity, const std::vector<float>& weights) {
    int set = 0;
    int at = 0;  // the place in VRHairPoints' list of this dyng's first particle
    EachDangle(entity, [&](RED4ext::ent::AnimatedComponent* ac, RED4ext::anim::DangleConstraint_Simulation* sim, RED4ext::CName kind) {
        if (kind != kDyng || !HairName(ac->name)) return;
        auto* rig = RigOf(ac);
        RigPose pose;
        if (!BuildPose(rig, pose)) return;
        auto& parts = reinterpret_cast<RED4ext::anim::DangleConstraint_SimulationDyng*>(sim)->particlesContainer.particles;
        const uint32_t m = parts.size();
        const int n = int(rig->boneNames.size());
        auto own = g_ownMasses.find(sim);
        // kept for another hair that came to the same address: dropped
        if (own != g_ownMasses.end() && (own->second.graph != g_lastGraph || own->second.mass.size() != m)) {
            g_ownMasses.erase(own);
            own = g_ownMasses.end();
        }
        if (weights.empty()) {
            if (own != g_ownMasses.end()) {
                for (uint32_t i = 0; i < m; ++i) parts[i].mass = own->second.mass[i];
                g_ownMasses.erase(own);
                set += int(m);
            }
            return;
        }
        if (own == g_ownMasses.end()) {
            OwnMasses o;
            o.graph = g_lastGraph;
            o.mass.resize(m);
            for (uint32_t i = 0; i < m; ++i) o.mass[i] = parts[i].mass;
            own = g_ownMasses.emplace(sim, std::move(o)).first;
        }
        for (uint32_t i = 0; i < m; ++i) {
            bool listed = false;
            for (int b = 0; b < n && !listed; ++b) listed = rig->boneNames[b] == parts[i].bone.name;
            if (!listed) continue;
            const float w = at < int(weights.size()) ? weights[at] : 0.f;
            ++at;
            parts[i].mass = own->second.mass[i] / (std::max)(kLeastWeight, (std::min)(1.f, w));
            ++set;
        }
    });
    return set;
}

static int SafeStrands(RED4ext::ent::Entity* entity, const std::vector<float>* weights) {
    __try {
        return Strands(entity, *weights);
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return -1;
    }
}

// VRHairStrands(entity, weights) -> Int32: each hair particle (VRHairPoints' order) as touched as its weight, 0 to 1
// ("w0 w1 ..."; a missing one 0), the force on the hair moving each by its weight; empty gives the hair its own masses
// back. The count set, -1 when reading the hair failed.
void VRHairStrands(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    auto data = Param<RED4ext::CString>(frame);
    frame->code++;
    std::vector<float> weights;
    const char* s = data.c_str();
    while (s && *s) {
        char* end = nullptr;
        const float v = std::strtof(s, &end);
        if (!end || end == s) break;
        weights.push_back(v);
        s = end;
    }
    const int n = SafeStrands(entity.GetPtr(), &weights);
    if (out) *out = n;
}

// VRHairPoints(entity) -> String: where the entity's hair is on its head, one "x y z parent;" per hair dyng particle
// (metres, the Head bone's frame; parent the particle it hangs from, its place in the list, -1 for one on the scalp).
// Empty when no hair dyng has a rig with a Head bone, or reading it failed.
void VRHairPoints(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::CString* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    frame->code++;
    std::string text;
    if (!SafePoints(entity.GetPtr(), &text)) text.clear();
    if (out) *out = RED4ext::CString(text.c_str());
}

// VRHairDangles(entity) -> String: the dangle simulations an entity's animated components run, one line each (a test
// tool's look at them).
void VRHairDangles(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::CString* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    frame->code++;
    std::string text;
    if (!SafeDescribe(entity.GetPtr(), &text)) text += "(failed)";
    if (out) *out = RED4ext::CString(text.c_str());
}

// VRDangleForce(entity, force, part, index) -> Int32: the external force (world space, on each particle; zero gives it
// back) on the entity's dangles of one kind -- part 0 the hair, 1 the breasts, 2 any -- all of them (index -1) or the
// index-th of that kind; the count set, -1 when reading them failed. Shared by all who wear the same graph, as the hair's.
void VRDangleForce(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    auto force = Param<RED4ext::Vector4>(frame);
    const int32_t part = Param<int32_t>(frame);
    const int32_t index = Param<int32_t>(frame);
    frame->code++;
    const float f[3] = {force.X, force.Y, force.Z};
    const int n = SafePartForce(entity.GetPtr(), f, part, index);
    if (out) *out = n;
}

// VRHairForce(entity, force) -> Int32: every dyng and pendulum of the entity's dangles given that external force (world
// space, on each particle; zero gives the hair back to itself); the count of them, -1 when reading them failed.
void VRHairForce(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, int32_t* out, int64_t) {
    auto entity = Param<RED4ext::Handle<RED4ext::ent::Entity>>(frame);
    auto force = Param<RED4ext::Vector4>(frame);
    frame->code++;
    const float f[3] = {force.X, force.Y, force.Z};
    const int n = SafeForce(entity.GetPtr(), f);
    if (out) *out = n;
}
