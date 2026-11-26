#include "Natives/VehicleHands.hpp"
#include "Natives/NativeFunctions.hpp"
#include "Natives/PhysHands.hpp"
#include "Natives/HeldBody.hpp"
#include <RED4ext/ResourceReference.hpp>
#include <RED4ext/Scripting/Utils.hpp>
#include <RED4ext/Scripting/Natives/entEntity.hpp>
#include <RED4ext/Scripting/Natives/entIPlacedComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/HardTransformBinding.hpp>
#include <RED4ext/Scripting/Natives/Generated/ent/MeshComponent.hpp>
#include <RED4ext/Scripting/Natives/Generated/CMesh.hpp>
#include <windows.h>
#include <algorithm>
#include <atomic>
#include <cmath>
#include <string_view>

bool ClassIsA(RED4ext::CClass* type, RED4ext::CName name);

namespace cvr::vehiclehands {
namespace { std::atomic<uint64_t> until[2]{}; }
bool Busy(int side) { return side >= 0 && side < 2 && GetTickCount64() < until[side].load(); }
void Claim(int side, bool on) {
    if (side >= 0 && side < 2) until[side].store(on ? GetTickCount64() + 250 : 0);
}
}

namespace {
// The same indices as EVehicleDoor. Follow rig bindings, including components
// bound to another component (mirrors), rather than model-specific coordinates.
int SlotPart(const char* name) {
    const std::string_view s = name ? name : "";
    const char* prefixes[] = {"door_front_left", "door_front_right", "door_back_left", "door_back_right", "trunk", "hood"};
    const char* windows[] = {"window_front_left", "window_front_right", "window_back_left", "window_back_right"};
    const auto match = [s](std::string_view prefix) {
        return s == prefix || (s.starts_with(prefix) && s.size() > prefix.size() && s[prefix.size()] == '_');
    };
    for (int i = 0; i < 6; ++i) if (match(prefixes[i])) return i;
    for (int i = 0; i < 4; ++i) if (match(windows[i])) return i;
    return -1;
}
int Part(RED4ext::ent::Entity* entity, RED4ext::ent::IPlacedComponent* mesh) {
    auto* current = mesh;
    for (int depth = 0; depth < 8 && current; ++depth) {
        auto* binding = current->parentTransform.instance;
        if (!binding || !ClassIsA(binding->GetType(), "entHardTransformBinding")) return -1;
        const auto* hard = static_cast<RED4ext::ent::HardTransformBinding*>(binding);
        if (const int part = SlotPart(hard->slotName.ToString()); part >= 0) return part;
        current = nullptr;
        for (const auto& c : entity->components) {
            if (c && c->name == binding->bindName && ClassIsA(c->GetType(), "entIPlacedComponent")) {
                current = static_cast<RED4ext::ent::IPlacedComponent*>(c.instance); break;
            }
        }
    }
    return -1;
}

bool Bounds(RED4ext::ResourcePath path, RED4ext::Box& out) {
    if (path.IsEmpty()) return false;
    // Only numeric bounds survive this call. No retained mesh, resource token,
    // GPU allocation, component, or vehicle handle when the player walks away.
    struct Entry { RED4ext::ResourcePath path{}; RED4ext::Box box{}; bool valid = false; };
    static thread_local Entry cache[128]{};
    static thread_local unsigned next = 0;
    for (const auto& entry : cache) if (entry.valid && entry.path == path) { out = entry.box; return true; }
    auto resource = RED4ext::ResourceLoader::Get()->LoadAsync<RED4ext::CMesh>(path);
    if (!resource || !resource->IsLoaded() || !resource->Get()) return false; // never wait on streaming
    out = resource->Get()->boundingBox;
    const float* lo = &out.Min.X; const float* hi = &out.Max.X;
    for (int k = 0; k < 3; ++k) if (!std::isfinite(lo[k]) || !std::isfinite(hi[k]) || hi[k] < lo[k]) return false;
    cache[next++ % 128] = {path, out, true};
    return true;
}

float Distance(const RED4ext::ent::MeshComponent& mesh, const RED4ext::Box& box, const RED4ext::Vector4& point) {
    const auto p = mesh.worldTransform.Position.AsVector3();
    const auto& q = mesh.worldTransform.Orientation;
    const float norm = q.i*q.i + q.j*q.j + q.k*q.k + q.r*q.r;
    if (!std::isfinite(norm) || std::abs(norm - 1.f) > .02f) return INFINITY;
    // Inverse rotation, then distance to the scaled local bounds. Negative
    // visualScale is valid. This chooses a panel; it never constrains a hand.
    const float x = point.X-p.X, y = point.Y-p.Y, z = point.Z-p.Z;
    const float tx = 2*(-q.j*z+q.k*y), ty = 2*(-q.k*x+q.i*z), tz = 2*(-q.i*y+q.j*x);
    const float local[3] = {x+q.r*tx-q.j*tz+q.k*ty, y+q.r*ty-q.k*tx+q.i*tz, z+q.r*tz-q.i*ty+q.j*tx};
    const float* lo = &box.Min.X; const float* hi = &box.Max.X; const float* scale = &mesh.visualScale.X;
    float d2 = 0;
    for (int k = 0; k < 3; ++k) {
        if (!std::isfinite(local[k]) || !std::isfinite(scale[k]) || std::abs(scale[k]) < .0001f) return INFINITY;
        const float a = lo[k]*scale[k], b = hi[k]*scale[k];
        const float d = local[k] - std::clamp(local[k], std::min(a,b), std::max(a,b));
        d2 += d*d;
    }
    return std::sqrt(d2);
}
}

void VRVehiclePartNear(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::Vector4* out, int64_t) {
    RED4ext::Handle<RED4ext::ent::Entity> entity;
    RED4ext::Vector4 point{}; float radius = 0;
    RED4ext::GetParameter(frame, &entity); RED4ext::GetParameter(frame, &point); RED4ext::GetParameter(frame, &radius);
    frame->code++;
    if (!out) return;
    *out = {};
    if (!entity || !ClassIsA(entity->GetType(), "vehicleCarBaseObject") || !std::isfinite(radius) || radius <= 0) return;
    float best = std::min(radius, .15f);
    for (const auto& c : entity->components) {
        if (!c || !c->isEnabled || !ClassIsA(c->GetType(), "entPhysicalMeshComponent")) continue;
        auto* mesh = static_cast<RED4ext::ent::MeshComponent*>(c.instance);
        if (!mesh->chunkMask) continue;
        const int part = Part(entity.instance, mesh);
        if (part < 0) continue;
        RED4ext::Box box{};
        if (!Bounds(mesh->mesh.path, box)) continue;
        const float distance = Distance(*mesh, box, point);
        if (distance <= best) { best = distance; *out = {float(part), distance, 0, 1}; }
    }
}

void VRVehicleHandWorld(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, RED4ext::Vector4* out, int64_t) {
    int32_t side = -1; RED4ext::GetParameter(frame, &side); frame->code++;
    if (!out) return;
    *out = {};
    if (side < 0 || side > 1 || (cvr::heldbody::HoldingMask() & (1 << side)) || cvr::physhands::HoldsBody(side)) return;
    float pull[3]{};
    if (cvr::physhands::GrabbedPerson(side, pull)) return;
    float p[3]{};
    if (cvr::physhands::DrawnWorld(side, p)) *out = {p[0], p[1], p[2], 1};
}
void VRVehicleHandClaim(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, void*, int64_t) {
    int32_t side = -1; bool on = false;
    RED4ext::GetParameter(frame, &side); RED4ext::GetParameter(frame, &on); frame->code++;
    cvr::vehiclehands::Claim(side, on);
}
void VRVehicleHandBusy(RED4ext::IScriptable*, RED4ext::CStackFrame* frame, bool* out, int64_t) {
    int32_t side = -1; RED4ext::GetParameter(frame, &side); frame->code++;
    if (out) *out = cvr::vehiclehands::Busy(side);
}
