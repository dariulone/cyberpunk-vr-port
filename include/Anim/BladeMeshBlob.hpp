#pragma once
#include "Anim/BladeMeshCut.hpp"
#include <bitset>
#include <cstdint>
#include <utility>

namespace RED4ext { struct CMesh; struct Matrix; }
namespace cvr::blade::mesh {
enum class Mark { None, Slash, Puncture, Trail };
struct Edit {
    Volume volume;
    int bone{-1};
    bool swept{};
    std::array<Point,4> rays{};
    Mark mark{};
    uint64_t visibleChunks{UINT64_MAX};
    double shellDepth{}; // Inward distance to the capsule centerline, if available.
    std::bitset<256> skinRegion; // Indices in this source mesh's byte skin palette.
};
struct Counts {
    uint32_t split{}, inserted{}, indicesBefore{}, indicesAfter{}, inkTriangles{};
    std::vector<std::pair<unsigned,unsigned>> inkChunks;
    Point contact{},normal{};
    int contactEdit{-1};
    unsigned contactChunk{},contactInkChunk{},contactLod{256};
    double entryDistanceSquared{};
    bool projectedFromShell{};
    bool noOverlap{}; // A supported blob was scanned without changing any face.
};
bool CutPrivateBlob(RED4ext::CMesh& mesh,const std::vector<Edit>& edits,Counts& counts);
}
