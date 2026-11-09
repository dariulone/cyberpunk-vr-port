#pragma once

// ofxr_bridge.ini read straight from disk (V518).
//
// The layer used GetPrivateProfileStringW/IntW, which run inside the game's
// process, where a mod may have hooked them. Skyrim's PrivateProfileRedirector
// (an SKSE plugin in common mod lists) caches every ini at its first read and
// answers later reads from that copy, so the tray's live changes never reached
// the layer: the write time changed, the values read back did not. These read
// the file itself on every call, with the profile API's rules: section and key
// names are case-insensitive, the first [section] of that name is the one
// read, the value is trimmed and loses one pair of enclosing quotes, an
// integer is its leading decimal (or 0x hex) digits, negated in two's
// complement after a minus, and an empty value gives the default.
// UTF-16LE and UTF-8 with a byte-order mark are decoded; anything else is the
// ANSI code page, as the profile API takes it.

#include <windows.h>

#include <cstdint>
#include <cwchar>
#include <cwctype>
#include <filesystem>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

namespace xrfg::ini_file {

[[nodiscard]] inline std::optional<std::wstring> read_text(
    const std::filesystem::path& path) noexcept {
    try {
        HANDLE file = CreateFileW(path.c_str(), GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, nullptr, OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL, nullptr);
        if (file == INVALID_HANDLE_VALUE) return std::nullopt;
        LARGE_INTEGER size{};
        std::vector<char> bytes;
        bool ok = GetFileSizeEx(file, &size) != FALSE && size.QuadPart >= 0 &&
            size.QuadPart <= 1024 * 1024;
        if (ok) {
            bytes.resize(static_cast<std::size_t>(size.QuadPart));
            DWORD read = 0;
            ok = bytes.empty() ||
                (ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()), &read, nullptr) !=
                     FALSE &&
                 read == bytes.size());
        }
        CloseHandle(file);
        if (!ok) return std::nullopt;
        std::wstring text;
        if (bytes.size() >= 2 && static_cast<unsigned char>(bytes[0]) == 0xFF &&
            static_cast<unsigned char>(bytes[1]) == 0xFE) {
            text.assign(reinterpret_cast<const wchar_t*>(bytes.data() + 2),
                (bytes.size() - 2) / sizeof(wchar_t));
            return text;
        }
        UINT code_page = CP_ACP;
        std::size_t start = 0;
        if (bytes.size() >= 3 && static_cast<unsigned char>(bytes[0]) == 0xEF &&
            static_cast<unsigned char>(bytes[1]) == 0xBB &&
            static_cast<unsigned char>(bytes[2]) == 0xBF) {
            code_page = CP_UTF8;
            start = 3;
        }
        const int length = static_cast<int>(bytes.size() - start);
        if (length == 0) return text;
        const int count = MultiByteToWideChar(code_page, 0, bytes.data() + start, length, nullptr, 0);
        if (count <= 0) return std::nullopt;
        text.resize(static_cast<std::size_t>(count));
        MultiByteToWideChar(code_page, 0, bytes.data() + start, length, text.data(), count);
        return text;
    } catch (...) {
        return std::nullopt;
    }
}

[[nodiscard]] inline std::wstring_view trim(std::wstring_view value) noexcept {
    while (!value.empty() && std::iswspace(value.front())) value.remove_prefix(1);
    while (!value.empty() && std::iswspace(value.back())) value.remove_suffix(1);
    return value;
}

[[nodiscard]] inline bool same_name(std::wstring_view a, std::wstring_view b) noexcept {
    return a.size() == b.size() &&
        CompareStringOrdinal(a.data(), static_cast<int>(a.size()), b.data(),
            static_cast<int>(b.size()), TRUE) == CSTR_EQUAL;
}

// The key's value, or nothing when the file, the section or the key is
// missing.
[[nodiscard]] inline std::optional<std::wstring> find_value(
    std::wstring_view text, std::wstring_view section, std::wstring_view key) {
    bool in_section = false;
    bool section_seen = false;
    std::size_t position = 0;
    while (position <= text.size()) {
        std::size_t end = text.find_first_of(L"\r\n", position);
        if (end == std::wstring_view::npos) end = text.size();
        const std::wstring_view line = trim(text.substr(position, end - position));
        position = end + 1;
        if (line.empty() || line.front() == L';') continue;
        if (line.front() == L'[') {
            const std::size_t close = line.find(L']');
            if (close == std::wstring_view::npos) continue;
            // Only the first section of that name is read.
            if (in_section) return std::nullopt;
            in_section = !section_seen && same_name(trim(line.substr(1, close - 1)), section);
            section_seen = section_seen || in_section;
            continue;
        }
        if (!in_section) continue;
        const std::size_t equals = line.find(L'=');
        if (equals == std::wstring_view::npos) continue;
        if (!same_name(trim(line.substr(0, equals)), key)) continue;
        std::wstring_view value = trim(line.substr(equals + 1));
        if (value.size() >= 2 && (value.front() == L'"' || value.front() == L'\'') &&
            value.back() == value.front()) {
            value = value.substr(1, value.size() - 2);
        }
        return std::wstring(value);
    }
    return std::nullopt;
}

[[nodiscard]] inline std::optional<std::wstring> read_value(
    const std::filesystem::path& path, std::wstring_view section, std::wstring_view key) noexcept {
    try {
        const auto text = read_text(path);
        if (!text) return std::nullopt;
        return find_value(*text, section, key);
    } catch (...) {
        return std::nullopt;
    }
}

[[nodiscard]] inline std::wstring read_string(const std::filesystem::path& path,
    std::wstring_view section, std::wstring_view key, std::wstring_view fallback) noexcept {
    try {
        if (auto value = read_value(path, section, key)) return std::move(*value);
        return std::wstring(fallback);
    } catch (...) {
        return {};
    }
}

[[nodiscard]] inline UINT parse_int(std::wstring_view value) noexcept {
    bool negative = false;
    if (!value.empty() && (value.front() == L'-' || value.front() == L'+')) {
        negative = value.front() == L'-';
        value.remove_prefix(1);
    }
    std::uint64_t result = 0;
    if (value.size() > 2 && value[0] == L'0' && (value[1] == L'x' || value[1] == L'X')) {
        for (wchar_t c : value.substr(2)) {
            const int digit = c >= L'0' && c <= L'9' ? c - L'0'
                : c >= L'a' && c <= L'f'             ? c - L'a' + 10
                : c >= L'A' && c <= L'F'             ? c - L'A' + 10
                                                      : -1;
            if (digit < 0) break;
            result = (result * 16 + static_cast<std::uint64_t>(digit)) & 0xFFFFFFFFULL;
        }
    } else {
        for (wchar_t c : value) {
            if (c < L'0' || c > L'9') break;
            result = (result * 10 + static_cast<std::uint64_t>(c - L'0')) & 0xFFFFFFFFULL;
        }
    }
    return static_cast<UINT>(negative ? (0 - result) & 0xFFFFFFFFULL : result);
}

[[nodiscard]] inline UINT read_int(const std::filesystem::path& path, std::wstring_view section,
    std::wstring_view key, UINT fallback) noexcept {
    const auto value = read_value(path, section, key);
    return value && !value->empty() ? parse_int(*value) : fallback;
}

}  // namespace xrfg::ini_file
