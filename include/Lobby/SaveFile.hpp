// A game save (sav.dat) read without the game: its compressed chunks unpacked and its node table, so that the lobby
// can find what a save holds (src/Lobby/SaveLooks.cpp: the character's looks) and tools/saves/savdump.cpp can dump it.
//
// Layout (2.5, save version 277): "VASC", save and game version, then an "FZLC" chunk table -- u32 count, then
// (u32 file offset, u32 stored size, u32 unpacked size) per chunk (the table is sized for 512 of them); each chunk is "4ZLX" + u32 unpacked size + an LZ4 block. Unpacked back to back they are the nodes'
// data, which starts where the first chunk starts in the file (node offsets count from the file's start). The file
// ends with u32 the node table's offset and "ENOD"; the table: "EDON", a VLQ count, then per node a string (VLQ length: bit 7
// set = 8-bit text), i32 next sibling, i32 first child, u32 offset, u32 size.
#pragma once
#include <cstdint>
#include <cstring>
#include <fstream>
#include <string>
#include <vector>

namespace cvr::save_file {

struct Node {
    std::string name;
    int32_t next{-1}, child{-1};
    uint32_t offset{}, size{};
};

struct Save {
    std::vector<uint8_t> data;      // the unpacked node data, indexed by file offset (the part before it zero)
    std::vector<Node> nodes;
    std::string error;
    const Node* Find(const char* name) const {
        for (const auto& n : nodes)
            if (n.name == name) return &n;
        return nullptr;
    }
};

namespace detail {
inline uint32_t U32(const uint8_t* p) { uint32_t v; std::memcpy(&v, p, 4); return v; }

// The engine's variable-length number: the first byte's bit 7 a flag (a string's 8-bit text), bit 6 = more, its
// low 6 bits; then 7 bits a byte, bit 7 = more.
inline size_t Vlq(const std::vector<uint8_t>& d, size_t& at, bool& flag) {
    if (at >= d.size()) return 0;
    uint8_t b = d[at++];
    flag = (b & 0x80) != 0;
    size_t v = b & 0x3F;
    int shift = 6;
    bool more = (b & 0x40) != 0;
    while (more && at < d.size() && shift < 32) {
        b = d[at++];
        v |= static_cast<size_t>(b & 0x7F) << shift;
        shift += 7;
        more = (b & 0x80) != 0;
    }
    return v;
}

// One LZ4 block (no frame): literals and matches until the input runs out.
inline bool Lz4(const uint8_t* src, size_t n, uint8_t* dst, size_t cap) {
    size_t i = 0, o = 0;
    while (i < n) {
        const uint8_t token = src[i++];
        size_t lit = token >> 4;
        if (lit == 15) {
            uint8_t b;
            do { if (i >= n) return false; b = src[i++]; lit += b; } while (b == 255);
        }
        if (i + lit > n || o + lit > cap) return false;
        std::memcpy(dst + o, src + i, lit);
        i += lit; o += lit;
        if (i >= n) break;                       // the last sequence has no match
        if (i + 2 > n) return false;
        const size_t back = src[i] | (src[i + 1] << 8);
        i += 2;
        size_t len = token & 15;
        if (len == 15) {
            uint8_t b;
            do { if (i >= n) return false; b = src[i++]; len += b; } while (b == 255);
        }
        len += 4;
        if (back == 0 || back > o || o + len > cap) return false;
        for (size_t k = 0; k < len; ++k, ++o) dst[o] = dst[o - back];
    }
    return o == cap;
}
}  // namespace detail

inline Save Read(const std::wstring& path) {
    using detail::U32;
    Save s;
    std::ifstream f(path, std::ios::binary);
    if (!f) { s.error = "cannot open"; return s; }
    std::vector<uint8_t> file((std::istreambuf_iterator<char>(f)), {});
    if (file.size() < 64 || std::memcmp(file.data(), "VASC", 4) != 0) { s.error = "not a save"; return s; }
    if (std::memcmp(file.data() + file.size() - 4, "ENOD", 4) != 0) { s.error = "no node table"; return s; }
    size_t table = 0;
    for (size_t k = 4; k + 4 < 64; ++k)
        if (std::memcmp(file.data() + k, "FZLC", 4) == 0) { table = k + 4; break; }
    if (!table) { s.error = "no chunk table"; return s; }
    const uint32_t count = U32(&file[table]);
    if (count == 0 || count > 512 || table + 4 + 12ull * count > file.size()) { s.error = "bad chunk table"; return s; }
    const uint32_t first = U32(&file[table + 4]);
    size_t total = first;
    for (uint32_t c = 0; c < count; ++c) total += U32(&file[table + 4 + 12 * c + 8]);
    s.data.assign(total, 0);
    size_t at = first;
    for (uint32_t c = 0; c < count; ++c) {
        const uint8_t* e = &file[table + 4 + 12 * c];
        const uint32_t off = U32(e), stored = U32(e + 4), size = U32(e + 8);
        if (off + stored > file.size() || stored < 8 || std::memcmp(&file[off], "4ZLX", 4) != 0 ||
            U32(&file[off + 4]) != size || !detail::Lz4(&file[off + 8], stored - 8, &s.data[at], size)) {
            s.error = "chunk " + std::to_string(c) + " does not unpack";
            return s;
        }
        at += size;
    }
    // The node table.
    size_t p = U32(&file[file.size() - 8]);
    if (p + 8 > file.size()) { s.error = "bad node table offset"; return s; }
    p += 4;                                       // its own tag, "EDON"
    bool narrow = false;
    const size_t nodes = detail::Vlq(file, p, narrow);
    for (size_t k = 0; k < nodes && p < file.size() - 8; ++k) {
        Node n;
        const size_t chars = detail::Vlq(file, p, narrow);
        if (narrow) {
            n.name.assign(reinterpret_cast<const char*>(&file[p]), chars);
            p += chars;
        } else {
            for (size_t c = 0; c < chars; ++c) n.name.push_back(static_cast<char>(file[p + 2 * c]));
            p += 2 * chars;
        }
        if (p + 16 > file.size()) break;
        std::memcpy(&n.next, &file[p], 4);
        std::memcpy(&n.child, &file[p + 4], 4);
        n.offset = U32(&file[p + 8]);
        n.size = U32(&file[p + 12]);
        p += 16;
        s.nodes.push_back(std::move(n));
    }
    if (s.nodes.size() != nodes) s.error = "node table cut short";
    return s;
}

}  // namespace cvr::save_file
