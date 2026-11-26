// The RED4ext.SDK class layouts the port is compiled against, checked against the running game.
//
// The SDK's headers carry the field offsets of the game build they were generated from, and the port
// reads engine objects through them (entEntity::components, entAnimatedComponent::controlBinding,
// inkImageWidget::textureAtlas ...). After a game patch the SDK lags until someone regenerates it,
// and a class whose fields moved is then read wrong by every line that touches it -- with no error
// anywhere. The game's RTTI holds the real layout. So, once, right after the types are registered:
// every class the port includes (SdkLayouts.gen.inc, from tools/patchday/sdk_layouts.py gen) by its
// size and by each field RTTI knows. On every build, before the build gate -- the first launch of a
// new game build then already names what moved.
//
// Report only. A field RTTI does not list (hand-reversed ones like entEntity::transformComponent) is
// counted, not checked; the code guards and the patch-day tools cover what reads those. The fix for
// a mismatch is the regenerated SDK (sdk_layouts.py diff / build), not anything done here.

#include <windows.h>

#include <cstddef>
#include <cstdint>
#include <iterator>

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/RTTITypes.hpp>

#if !CVR_RUST_VRIK
extern void Log(const char* fmt, ...);
#endif

namespace {

struct SdkField {
    const char* name;
    uint32_t offset;
};

struct SdkType {
    const char* rtti;
    uint32_t size;
    const SdkField* fields;
    uint32_t count;
};

} // namespace

// At file scope: it starts with the SDK headers, then defines kSdkTypes[] (static).
#include "SdkLayouts.gen.inc"

#if CVR_RUST_VRIK
// The Rust build's check is rust/crates/cvr-ffi/src/sdk_layouts.rs; the generated table stays here and is handed over.
static_assert(sizeof(SdkField) == 16 && sizeof(SdkType) == 32, "rust/crates/cvr-ffi/src/sdk_layouts.rs SdkField/SdkType");
extern "C" void cvr_sdk_layouts_check(const void* types, size_t count);
void CyberpunkVR_CheckSdkLayouts() { cvr_sdk_layouts_check(kSdkTypes, std::size(kSdkTypes)); }
#else
namespace {

struct Tally {
    uint32_t types = 0, unknown = 0, sizes = 0, fields = 0, offsets = 0, absent = 0, printed = 0;
};

const RED4ext::CProperty* FindProperty(RED4ext::CClass* cls, RED4ext::CName name) {
    for (; cls; cls = cls->parent)
        for (const RED4ext::CProperty* p : cls->props)
            if (p && p->name == name) return p;
    return nullptr;
}

void Mismatch(Tally& t, const char* fmt, const char* a, const char* b, uint32_t sdk, uint32_t game) {
    if (t.printed++ < 40) Log(fmt, a, b, sdk, game);
}

void Compare(Tally& t) {
    auto* rtti = RED4ext::CRTTISystem::Get();
    for (const auto& type : kSdkTypes) {
        ++t.types;
        RED4ext::CClass* cls = rtti->GetClass(RED4ext::CName(type.rtti));
        if (!cls) {
            ++t.unknown;
            if (t.printed++ < 40) Log("[sdk-layout] %s: not a class in this game's RTTI\n", type.rtti);
            continue;
        }
        if (cls->size != type.size)
            ++t.sizes, Mismatch(t, "[sdk-layout] MISMATCH %s%s size: SDK 0x%X, game 0x%X\n", type.rtti, "",
                                type.size, cls->size);
        for (uint32_t i = 0; i < type.count; ++i) {
            const SdkField& f = type.fields[i];
            const RED4ext::CProperty* p = FindProperty(cls, RED4ext::CName(f.name));
            if (!p) {
                ++t.absent;
                continue;
            }
            ++t.fields;
            if (p->valueOffset != f.offset)
                ++t.offsets, Mismatch(t, "[sdk-layout] MISMATCH %s.%s: SDK +0x%X, game +0x%X\n", type.rtti, f.name,
                                      f.offset, p->valueOffset);
        }
    }
}

// Reads only, but through the SDK's own view of CClass: if THAT layout changed, a fault is the
// answer, and it must not take the game with it.
bool CompareGuarded(Tally& t) {
    __try {
        Compare(t);
        return true;
    } __except (EXCEPTION_EXECUTE_HANDLER) {
        return false;
    }
}

} // namespace

void CyberpunkVR_CheckSdkLayouts() {
    Tally t;
    if (!CompareGuarded(t)) {
        Log("[sdk-layout] the check faulted after %u classes (the SDK's CClass/CProperty view no longer fits "
            "this game) -- not checked\n", t.types);
        return;
    }
    Log("[sdk-layout] %u classes (%u not in RTTI), %u fields checked: %u size and %u offset mismatches "
        "(%u SDK fields RTTI does not list)\n",
        t.types, t.unknown, t.fields, t.sizes, t.offsets, t.absent);
}
#endif
