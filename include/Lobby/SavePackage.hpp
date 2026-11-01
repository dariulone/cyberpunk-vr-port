// A RED package inside a save node: how the save keeps the game's scripted systems (the node
// ScriptableSystemsContainer -- the equipment system's data among them). Read without the game, so that the lobby
// can find what a playthrough's V wears (src/Lobby/SaveLooks.cpp) and tools/saves/savdump.cpp can show it.
//
// Layout (2.5): the node's own u32 index and u32 size, then the package -- u16 version (4), u16 section count (6),
// u32 component count, four u32 offsets (name descriptors, name data, chunk descriptors, chunk data; each from the
// base below), u32 count of root ids and that many u64. The base is where the root ids end. A name descriptor is a
// u32: low 24 bits its offset, high 8 bits its length with the terminating zero. A chunk descriptor: u32 type name,
// u32 offset. A chunk, and every struct value in it: u16 property count, then per property u16 name, u16 type
// (names), u32 offset from the chunk's or struct's start; a property at its default value is not written.
// Values: Bool / Int8 / Uint8 one byte, Int16 / Uint16 two, Int32 / Uint32 / Float four, Int64 / Uint64 / Double /
// TweakDBID / CRUID eight, CName and an enum's value a u16 name, handle:T / whandle:T a u32 chunk index,
// array:T a u32 count and the values back to back, String a u16 length and its bytes.
#pragma once
#include <cstdint>
#include <cstring>
#include <functional>
#include <string>
#include <vector>

namespace cvr::save_file::package {

struct Prop {
    std::string name, type;
    size_t at{};  // where its value is, in the package's bytes
};

class Package {
public:
    // Whether a type is an enum (its value a u16 name): the game knows (RTTI); without it, a guess by the name.
    std::function<bool(const std::string&)> isEnum = [](const std::string& t) {
        return t.rfind("gamedata", 0) == 0 || (t.size() > 1 && t[0] == 'E' && t[1] >= 'A' && t[1] <= 'Z');
    };

    // The node's bytes; false when they do not read as a package.
    bool Open(const uint8_t* data, size_t size) {
        if (size < 36) return false;
        d_ = data + 8;
        n_ = size - 8;
        if (U16(0) != 4 || U16(2) != 6) return false;
        const uint32_t nameDesc = U32(8), nameData = U32(12), chunkDesc = U32(16), chunkData = U32(20);
        const uint32_t roots = U32(24);
        base_ = 28 + 8ull * roots;
        if (base_ + chunkData > n_ || nameDesc > nameData || nameData > chunkDesc || chunkDesc > chunkData) return false;
        names_.clear();
        for (uint32_t k = nameDesc; k + 4 <= nameData; k += 4) {
            const uint32_t v = U32(base_ + k), off = v & 0xFFFFFF, len = v >> 24;
            if (len == 0 || base_ + off + len > n_) return false;
            names_.emplace_back(reinterpret_cast<const char*>(d_ + base_ + off), len - 1);
        }
        chunks_.clear();
        for (uint32_t k = chunkDesc; k + 8 <= chunkData; k += 8) {
            const uint32_t type = U32(base_ + k), off = U32(base_ + k + 4);
            if (type >= names_.size() || base_ + off > n_) return false;
            chunks_.push_back({type, base_ + off});
        }
        return true;
    }

    size_t Chunks() const { return chunks_.size(); }
    const std::string& ChunkType(size_t k) const { return names_[chunks_[k].type]; }
    size_t ChunkAt(size_t k) const { return chunks_[k].at; }
    const std::string& Name(size_t k) const {
        static const std::string none;
        return k < names_.size() ? names_[k] : none;
    }

    // A chunk's or a struct's properties.
    std::vector<Prop> Props(size_t at) const {
        std::vector<Prop> out;
        if (at + 2 > n_) return out;
        const uint16_t count = U16(at);
        if (at + 2 + 8ull * count > n_) return out;
        for (uint16_t k = 0; k < count; ++k) {
            const size_t e = at + 2 + 8ull * k;
            const uint16_t name = U16(e), type = U16(e + 2);
            const uint32_t off = U32(e + 4);
            if (name >= names_.size() || type >= names_.size() || at + off > n_) return {};
            out.push_back({names_[name], names_[type], at + off});
        }
        return out;
    }

    // A property by name (also as a script writes it, m_ in front).
    const Prop* Find(const std::vector<Prop>& props, const char* name) const {
        const std::string m = std::string("m_") + name;
        for (const auto& p : props)
            if (p.name == name || p.name == m) return &p;
        return nullptr;
    }

    // Where a value of this type ends; 0 when it does not read.
    size_t End(const std::string& t, size_t at, int depth = 0) const {
        if (depth > 32 || at > n_) return 0;
        if (t == "Bool" || t == "Int8" || t == "Uint8") return Fits(at, 1);
        if (t == "Int16" || t == "Uint16" || t == "CName") return Fits(at, 2);
        if (t == "Int32" || t == "Uint32" || t == "Float") return Fits(at, 4);
        if (t == "Int64" || t == "Uint64" || t == "Double" || t == "TweakDBID" || t == "CRUID") return Fits(at, 8);
        if (t == "String") return at + 2 <= n_ ? Fits(at, 2 + U16(at)) : 0;
        if (t.rfind("handle:", 0) == 0 || t.rfind("whandle:", 0) == 0) return Fits(at, 4);
        if (t.rfind("array:", 0) == 0) {
            if (at + 4 > n_) return 0;
            const uint32_t count = U32(at);
            const std::string element = t.substr(6);
            size_t p = at + 4;
            for (uint32_t k = 0; k < count && p; ++k) p = End(element, p, depth + 1);
            return p;
        }
        if (isEnum(t)) return Fits(at, 2);
        // A struct: its property table, then its values.
        const auto props = Props(at);
        if (at + 2 > n_) return 0;
        size_t end = at + 2 + 8 * props.size();
        for (const auto& p : props) {
            const size_t e = End(p.type, p.at, depth + 1);
            if (!e) return 0;
            if (e > end) end = e;
        }
        return end;
    }

    // An array's elements: where each starts.
    std::vector<size_t> Elements(const std::string& t, size_t at) const {
        std::vector<size_t> out;
        if (t.rfind("array:", 0) != 0 || at + 4 > n_) return out;
        const uint32_t count = U32(at);
        const std::string element = t.substr(6);
        size_t p = at + 4;
        for (uint32_t k = 0; k < count && p; ++k) {
            out.push_back(p);
            p = End(element, p);
        }
        if (!p) out.clear();
        return out;
    }

    uint8_t U8(size_t at) const { return at < n_ ? d_[at] : 0; }
    uint16_t U16(size_t at) const { uint16_t v = 0; if (at + 2 <= n_) std::memcpy(&v, d_ + at, 2); return v; }
    uint32_t U32(size_t at) const { uint32_t v = 0; if (at + 4 <= n_) std::memcpy(&v, d_ + at, 4); return v; }
    uint64_t U64(size_t at) const { uint64_t v = 0; if (at + 8 <= n_) std::memcpy(&v, d_ + at, 8); return v; }

private:
    struct Chunk {
        uint32_t type;
        size_t at;
    };
    size_t Fits(size_t at, size_t len) const { return at + len <= n_ ? at + len : 0; }

    const uint8_t* d_{};
    size_t n_{}, base_{};
    std::vector<std::string> names_;
    std::vector<Chunk> chunks_;
};

}  // namespace cvr::save_file::package
