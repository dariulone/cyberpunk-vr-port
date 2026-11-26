#pragma once
#include <cstdint>

namespace RED4ext { struct CMesh; }
namespace RED4ext::ent { struct IComponent; }
namespace RED4ext { template<class T> class Handle; }
namespace cvr::blade {
uint64_t MeshDepotSource(uint64_t requested);
void BeforePrivateMeshUpload(RED4ext::CMesh* mesh);
void EnableMeshCopies(bool enabled);
void RegisterMeshInstanceProbe();
bool ReloadCutMesh(const RED4ext::Handle<RED4ext::ent::IComponent>& component);
}
