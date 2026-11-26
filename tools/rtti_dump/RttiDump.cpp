// RttiDump -- a RED4ext plugin that writes the running game's RTTI layouts to a JSON file and does nothing else.
//
// Patch-day tool (tools/patchday, docs/patchday-runbook.md): after a game update the RED4ext.SDK headers carry the
// old build's class layouts until they are regenerated. This dump is the new build's truth: every class (size,
// alignment, parent, flags, each property's type and offset), every enum and bitfield. sdk_layouts.py compares it
// with the SDK. It reads only (CRTTISystem's enumerators, CClass/CProperty fields), registers no type, no function,
// no RTTI callback -- the parts of the SDK a patch is most likely to have moved -- and runs once, when the game
// state machine enters Running (RED4ext's own game-state hook).
//
// Output: <plugin folder>\rtti_dump-<exe timestamp>.json
#include <windows.h>

#include <cstdio>
#include <string>

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/RTTISystem.hpp>
#include <RED4ext/RTTITypes.hpp>
#include <RED4ext/Scripting/CProperty.hpp>

namespace {

HMODULE g_module{};

std::string Escape(const char* s) {
    std::string out;
    for (; s && *s; ++s) {
        if (*s == '"' || *s == '\\') out += '\\';
        if (static_cast<unsigned char>(*s) < 0x20) continue;
        out += *s;
    }
    return out;
}

const char* Name(RED4ext::CName n) {
    const char* s = n.ToString();
    return s ? s : "";
}

uint32_t ExeTimestamp() {
    auto* base = reinterpret_cast<const uint8_t*>(GetModuleHandleW(nullptr));
    auto* dos = reinterpret_cast<const IMAGE_DOS_HEADER*>(base);
    auto* nt = reinterpret_cast<const IMAGE_NT_HEADERS64*>(base + dos->e_lfanew);
    return nt->FileHeader.TimeDateStamp;
}

void Dump() {
    wchar_t dir[MAX_PATH]{};
    GetModuleFileNameW(g_module, dir, MAX_PATH);
    if (wchar_t* slash = wcsrchr(dir, L'\\')) *slash = 0;
    wchar_t path[MAX_PATH]{};
    swprintf_s(path, L"%s\\rtti_dump-%08X.json", dir, ExeTimestamp());
    FILE* f = nullptr;
    if (_wfopen_s(&f, path, L"wb") || !f) return;

    auto* rtti = RED4ext::CRTTISystem::Get();
    RED4ext::DynArray<RED4ext::CClass*> classes;
    rtti->GetClasses(nullptr, classes, nullptr, true);
    std::fputs("{\"classes\":[\n", f);
    bool first = true;
    for (RED4ext::CClass* cls : classes) {
        if (!cls) continue;
        std::fprintf(f, "%s{\"name\":\"%s\",\"size\":%u,\"align\":%u,\"holder\":%u,\"flags\":%u,\"parent\":\"%s\",\"props\":[",
                     first ? "" : ",\n", Escape(Name(cls->name)).c_str(), cls->size, cls->alignment, cls->holderSize,
                     *reinterpret_cast<const uint32_t*>(&cls->flags),
                     cls->parent ? Escape(Name(cls->parent->name)).c_str() : "");
        first = false;
        bool firstProp = true;
        for (RED4ext::CProperty* p : cls->props) {
            if (!p) continue;
            std::fprintf(f, "%s{\"name\":\"%s\",\"type\":\"%s\",\"offset\":%u,\"flags\":%llu}", firstProp ? "" : ",",
                         Escape(Name(p->name)).c_str(), p->type ? Escape(Name(p->type->GetName())).c_str() : "",
                         p->valueOffset, static_cast<unsigned long long>(*reinterpret_cast<const uint64_t*>(&p->flags)));
            firstProp = false;
        }
        std::fprintf(f, "],\"funcs\":%u,\"staticFuncs\":%u}", cls->funcs.size(), cls->staticFuncs.size());
    }
    std::fputs("\n],\"enums\":[\n", f);
    RED4ext::DynArray<RED4ext::CEnum*> enums;
    rtti->GetEnums(enums, false);
    first = true;
    for (RED4ext::CEnum* e : enums) {
        if (!e) continue;
        std::fprintf(f, "%s{\"name\":\"%s\",\"size\":%u,\"members\":[", first ? "" : ",\n", Escape(Name(e->name)).c_str(),
                     e->actualSize);
        first = false;
        for (uint32_t i = 0; i < e->hashList.size() && i < e->valueList.size(); ++i)
            std::fprintf(f, "%s[\"%s\",%lld]", i ? "," : "", Escape(Name(e->hashList[i])).c_str(),
                         static_cast<long long>(e->valueList[i]));
        std::fputs("]}", f);
    }
    std::fputs("\n]}\n", f);
    std::fclose(f);
}

bool OnRunning(RED4ext::CGameApplication*) {
    static bool done = false;
    if (!done) {
        done = true;
        Dump();
    }
    return true;
}

bool Nop(RED4ext::CGameApplication*) { return true; }

} // namespace

BOOL APIENTRY DllMain(HMODULE module, DWORD reason, LPVOID) {
    if (reason == DLL_PROCESS_ATTACH) g_module = module;
    return TRUE;
}

RED4EXT_C_EXPORT bool RED4EXT_CALL Main(RED4ext::v1::PluginHandle handle, RED4ext::v1::EMainReason reason,
                                        const RED4ext::v1::Sdk* sdk) {
    if (reason == RED4ext::v1::EMainReason::Load) {
        static RED4ext::v1::GameState state{&OnRunning, &OnRunning, &Nop};
        sdk->gameStates->Add(handle, RED4ext::EGameStateType::Running, &state);
    }
    return true;
}

RED4EXT_C_EXPORT void RED4EXT_CALL Query(RED4ext::v1::PluginInfo* info) {
    info->name = L"RttiDump";
    info->author = L"CyberpunkVRPort";
    info->version = RED4EXT_V1_SEMVER(1, 0, 0);
    info->runtime = RED4EXT_V1_RUNTIME_VERSION_INDEPENDENT;
    info->sdk = RED4EXT_V1_SDK_VERSION_1_0_0_COMPAT_0_5_0;
}

RED4EXT_C_EXPORT uint32_t RED4EXT_CALL Supports() { return RED4EXT_API_VERSION_1_COMPAT_0; }
