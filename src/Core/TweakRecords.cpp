#include "Core/TweakRecords.hpp"
#include "Core/Paths.hpp"

#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Relocation.hpp>
#include <RED4ext/RTTISystem.hpp>
#include <RED4ext/RTTITypes.hpp>
#include <RED4ext/TweakDB.hpp>
#include <RED4ext/Scripting/Natives/gamedataTweakDBRecord.hpp>

#include <windows.h>

#include <yaml-cpp/yaml.h>

#include <algorithm>
#include <execution>
#include <numeric>
#include <cctype>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <string>
#include <unordered_set>
#include <vector>

extern void Log(const char* fmt, ...);

namespace cvr::tweak_records {
namespace {

namespace fs = std::filesystem;
using RED4ext::CName;
using RED4ext::TweakDB;
using RED4ext::TweakDBID;
using RED4ext::rtti::IType;

// Address-library hashes (red4ext's cyberpunk2077_addresses.json), the two TweakXL resolves for the same purpose.
constexpr uint32_t kTryLoadTweakDBHash = 464332086;   // loads tweakdb.bin; 2.5: (a1, TweakDB*, path, a4, bool)
constexpr uint32_t kTweakDBIDDeriveHash = 326438016;  // registers a TweakDBID's name: (base, id, name)

using TryLoadFn = bool (*)(void*, TweakDB*, RED4ext::CString&, void*, bool);
using DeriveFn = void (*)(const TweakDBID*, const TweakDBID*, const char*);
TryLoadFn g_tryLoad = nullptr;
DeriveFn g_derive = nullptr;

struct Record {
    std::string name;
    std::string file;
    YAML::Node body;
};

// A value of a TweakDB flat type, built outside the flat buffer and handed to it.
class Value {
public:
    explicit Value(IType* type) : type_(type) {
        const size_t alignment = (std::max)(type->GetAlignment(), 8u);
        memory_ = _aligned_malloc((std::max)(type->GetSize(), 1u), alignment);
        type->Construct(memory_);
    }
    ~Value() {
        type_->Destruct(memory_);
        _aligned_free(memory_);
    }
    Value(const Value&) = delete;
    Value& operator=(const Value&) = delete;
    void* get() const { return memory_; }

private:
    IType* type_;
    void* memory_;
};

std::string Lower(std::string text) {
    for (char& c : text) c = static_cast<char>(std::tolower(static_cast<unsigned char>(c)));
    return text;
}

// One element of a struct written as a YAML map: {x: 0, y: 0, z: 0}.
bool StructInto(const std::string& typeName, void* out, const YAML::Node& node) {
    struct Field { const char* name; int index; };
    auto floats = [&](std::initializer_list<Field> fields) {
        auto* f = static_cast<float*>(out);
        for (const auto& item : node) {
            const std::string key = Lower(item.first.as<std::string>());
            const auto field = std::find_if(fields.begin(), fields.end(), [&](const Field& x) { return key == x.name; });
            if (field == fields.end()) return false;
            f[field->index] = item.second.as<float>();
        }
        return true;
    };
    if (typeName == "Vector2") return floats({{"x", 0}, {"y", 1}});
    if (typeName == "Vector3") return floats({{"x", 0}, {"y", 1}, {"z", 2}});
    if (typeName == "Vector4") return floats({{"x", 0}, {"y", 1}, {"z", 2}, {"w", 3}});
    if (typeName == "Quaternion") return floats({{"i", 0}, {"j", 1}, {"k", 2}, {"r", 3}});
    if (typeName == "EulerAngles") return floats({{"roll", 0}, {"pitch", 1}, {"yaw", 2}});
    if (typeName == "Color") {
        static const char* kChannels[] = {"red", "green", "blue", "alpha"};
        auto* c = static_cast<uint8_t*>(out);
        for (const auto& item : node) {
            const std::string key = Lower(item.first.as<std::string>());
            const auto* channel = std::find_if(std::begin(kChannels), std::end(kChannels), [&](const char* x) { return key == x; });
            if (channel == std::end(kChannels)) return false;
            c[channel - std::begin(kChannels)] = static_cast<uint8_t>(item.second.as<int>());
        }
        return true;
    }
    return false;
}

// A single value of `type` from YAML. False when the YAML does not fit the type.
bool ScalarInto(IType* type, void* out, const YAML::Node& node) {
    const std::string typeName = type->GetName().ToString();
    if (node.IsMap()) return StructInto(typeName, out, node);
    if (!node.IsScalar() && !node.IsNull()) return false;
    const std::string text = node.IsNull() ? std::string() : node.as<std::string>();
    try {
        if (typeName == "TweakDBID") {
            *static_cast<TweakDBID*>(out) = text.empty() ? TweakDBID() : TweakDBID(text);
        } else if (typeName == "CName") {
            *static_cast<CName*>(out) = text.empty() || text == "None" ? CName() : RED4ext::CNamePool::Add(text.c_str());
        } else if (typeName == "String") {
            *static_cast<RED4ext::CString*>(out) = RED4ext::CString(text.c_str());
        } else if (typeName == "Float") {
            *static_cast<float*>(out) = std::stof(text);
        } else if (typeName == "Int32") {
            *static_cast<int32_t*>(out) = std::stoi(text);
        } else if (typeName == "Bool") {
            const std::string value = Lower(text);
            if (value != "true" && value != "false" && value != "1" && value != "0") return false;
            *static_cast<bool*>(out) = value == "true" || value == "1";
        } else if (typeName == "raRef:CResource") {
            *static_cast<RED4ext::ResourcePath*>(out) = text.empty() ? RED4ext::ResourcePath() : RED4ext::ResourcePath(text.c_str());
        } else if (typeName == "gamedataLocKeyWrapper") {
            // LocKey#<number>, or a secondary key that the localization hashes.
            uint64_t key = 0;
            if (text.rfind("LocKey#", 0) == 0) key = std::stoull(text.substr(7));
            else if (!text.empty()) key = RED4ext::FNV1a64(text.c_str());
            *static_cast<uint64_t*>(out) = key;
        } else {
            return type->FromString(out, RED4ext::CString(text.c_str()));
        }
    } catch (const std::exception&) {
        return false;
    }
    return true;
}

enum class Op { Plain, Append, AppendOnce, Prepend, PrependOnce, Remove };

Op OpOf(const YAML::Node& node) {
    const std::string& tag = node.Tag();
    if (tag == "!append") return Op::Append;
    if (tag == "!append-once") return Op::AppendOnce;
    if (tag == "!prepend") return Op::Prepend;
    if (tag == "!prepend-once") return Op::PrependOnce;
    if (tag == "!remove") return Op::Remove;
    return Op::Plain;
}

int32_t IndexOf(const RED4ext::CRTTIBaseArrayType* array, void* instance, IType* inner, const void* element) {
    const uint32_t length = array->GetLength(instance);
    for (uint32_t i = 0; i < length; ++i)
        if (inner->IsEqual(array->GetElement(instance, i), element)) return static_cast<int32_t>(i);
    return -1;
}

// An array value from a YAML list. Untagged lists replace the array; a list with operators edits `current`.
bool ArrayInto(IType* type, void* out, const YAML::Node& node, const void* current) {
    if (!node.IsSequence()) return false;
    auto* array = static_cast<RED4ext::CRTTIBaseArrayType*>(type);
    IType* inner = array->GetInnerType();
    const bool edit = std::any_of(node.begin(), node.end(), [](const YAML::Node& item) { return OpOf(item) != Op::Plain; });
    if (edit && current) type->Assign(out, current);
    for (const YAML::Node& item : node) {
        Value element(inner);
        if (!ScalarInto(inner, element.get(), item)) return false;
        const Op op = OpOf(item);
        const int32_t at = IndexOf(array, out, inner, element.get());
        if (op == Op::Remove) {
            if (at >= 0) array->RemoveAt(out, at);
            continue;
        }
        if ((op == Op::AppendOnce || op == Op::PrependOnce) && at >= 0) continue;
        uint32_t index = array->GetLength(out);
        if (op == Op::Prepend || op == Op::PrependOnce) {
            if (!array->InsertAt(out, 0)) return false;
            index = 0;
        } else if (!array->Resize(out, index + 1)) {
            return false;
        }
        inner->Assign(array->GetElement(out, index), element.get());
    }
    return true;
}

// The record's flat names: each getter of the class chain whose flat exists on `sample` -- lowercase first as most
// are, uppercase otherwise (OnAttach). The helpers around a property (Get<P>Count, <P>Contains, ...) have no flat.
std::vector<std::string> FlatNames(TweakDB* db, const RED4ext::CClass* type, TweakDBID sample) {
    std::vector<std::string> names;
    std::unordered_set<std::string> seen;
    for (const RED4ext::CClass* cls = type; cls; cls = cls->parent) {
        if (std::strcmp(cls->GetName().ToString(), "gamedataTweakDBRecord") == 0) break;
        for (const RED4ext::CClassFunction* func : cls->funcs) {
            std::string name = func->shortName.ToString();
            if (name.empty()) continue;
            for (const bool upper : {false, true}) {
                name[0] = static_cast<char>(upper ? std::toupper(static_cast<unsigned char>(name[0]))
                                                  : std::tolower(static_cast<unsigned char>(name[0])));
                if (db->GetFlatValue(TweakDBID(sample, "." + name))) {
                    if (seen.insert(name).second) names.push_back(name);
                    break;
                }
            }
        }
    }
    return names;
}

const RED4ext::CClass* RecordType(TweakDB* db, TweakDBID id) {
    const RED4ext::Handle<RED4ext::IScriptable> record = db->GetRecord(id);
    return record ? record->GetNativeType() : nullptr;
}

// Applies the records a level at a time: the ones whose $base is already a record. Every record a level creates gets
// its flats in ONE insert into the TweakDB's sorted flat array -- an insert moves the array's tail, ~2 million ids, so
// once per record cost ~3 ms each -- then its record object, then the properties of the whole level are written.
class Applier {
public:
    explicit Applier(TweakDB* db) : db_(db), rtti_(RED4ext::CRTTISystem::Get()) {}

    void ApplyLevel(const std::vector<const Record*>& level) {
        std::vector<Planned> planned;
        RED4ext::SortedUniqueArray<TweakDBID> flats;
        std::vector<const Record*> ready;
        for (const Record* record : level) {
            const TweakDBID id(record->name);
            if (RecordType(db_, id)) {
                ready.push_back(record);
                continue;
            }
            Planned plan{record, id};
            if (!Plan(*record, plan, flats)) continue;
            planned.push_back(std::move(plan));
        }
        {
            std::lock_guard<RED4ext::SharedSpinLock> lock(db_->mutex00);
            db_->flats.InsertOrAssign(flats);
        }
        for (const Planned& plan : planned) {
            if (!db_->CreateRecord(plan.id, const_cast<RED4ext::CClass*>(plan.type)) || !RecordType(db_, plan.id)) {
                Fail(*plan.record, "was not created");
                continue;
            }
            RegisterNames(plan);
            ready.push_back(plan.record);
            ++created_;
        }
        // Every property of the level, then one insert for the flats that are new, then each record reads its flats.
        FlatWrites writes(db_);
        for (const Record* record : ready) Write(*record, writes);
        writes.Commit();
        for (const Record* record : ready) {
            std::lock_guard<RED4ext::SharedSpinLock> lock(db_->mutex01);
            if (!db_->UpdateRecord(TweakDBID(record->name))) Fail(*record, "could not be refreshed");
        }
    }

    void Fail(const Record& record, const char* what) {
        Log("[tweaks] %s %s (%s)\n", record.name.c_str(), what, record.file.c_str());
        ++errors_;
    }

    int created() const { return created_; }
    int flats() const { return flats_; }
    int errors() const { return errors_; }

private:
    struct Planned {
        const Record* record;
        TweakDBID id;
        const RED4ext::CClass* type = nullptr;
        std::vector<std::string> names;
    };

    // The class $type names: the full RTTI name, or the short one TweakXL also takes (AttachmentSlot).
    const RED4ext::CClass* TypeNamed(const std::string& name) {
        if (const auto* cls = rtti_->GetClass(CName(name.c_str()))) return cls;
        return rtti_->GetClass(CName(("gamedata" + name + "_Record").c_str()));
    }

    // A new record of the type of its $base (every flat the base holds, shared with it) or of its $type (default
    // flats, of the types any record of the type holds them in).
    bool Plan(const Record& record, Planned& plan, RED4ext::SortedUniqueArray<TweakDBID>& flats) {
        const YAML::Node base = record.body["$base"], typeName = record.body["$type"];
        TweakDBID source;
        bool copy = true;
        if (base) {
            source = TweakDBID(base.as<std::string>());
            plan.type = RecordType(db_, source);
            if (!plan.type) {
                Fail(record, "has a $base that is no record");
                return false;
            }
        } else if (typeName) {
            plan.type = TypeNamed(typeName.as<std::string>());
            if (plan.type) {
                std::shared_lock<RED4ext::SharedSpinLock> lock(db_->mutex01);
                const auto* sameType = db_->recordsByType.Get(const_cast<RED4ext::CClass*>(plan.type));
                if (sameType && sameType->size() > 0)
                    source = reinterpret_cast<RED4ext::gamedataTweakDBRecord*>((*sameType)[0].GetPtr())->recordID;
            }
            if (!source.IsValid()) {
                Fail(record, "has a $type no record has");
                return false;
            }
            copy = false;
        } else {
            Fail(record, "is no record and has neither $base nor $type");
            return false;
        }
        plan.names = FlatNames(db_, plan.type, source);
        for (const std::string& property : plan.names) {
            TweakDB::FlatValue* from = db_->GetFlatValue(TweakDBID(source, "." + property));
            if (!from) continue;
            TweakDBID flat(plan.id, "." + property);
            int32_t offset = from->ToTDBOffset();
            if (!copy) {
                IType* flatType = from->GetValue().type;
                Value empty(flatType);
                offset = db_->CreateFlatValue(RED4ext::CStackType(flatType, empty.get()));
                if (offset < 0) {
                    Fail(record, "could not get its flats");
                    return false;
                }
            }
            flat.SetTDBOffset(offset);
            flats.Insert(flat);
        }
        return true;
    }

    // The engine's names for a new record and its flats, so they read back as names (TDBID.ToStringDEBUG).
    void RegisterNames(const Planned& plan) {
        if (!g_derive) return;
        const TweakDBID none;
        g_derive(&none, &plan.id, plan.record->name.c_str());
        for (const std::string& property : plan.names) {
            const TweakDBID flat(plan.id, "." + property);
            g_derive(&plan.id, &flat, ("." + property).c_str());
        }
    }

    // Each property, as a flat of the type the record holds it in.
    void Write(const Record& record, FlatWrites& writes) {
        const TweakDBID id(record.name);
        for (const auto& property : record.body) {
            const std::string name = property.first.as<std::string>();
            if (name.empty() || name[0] == '$') continue;
            const TweakDBID flat(id, "." + name);
            TweakDB::FlatValue* current = db_->GetFlatValue(flat);
            if (!current) {
                Log("[tweaks] %s.%s: the record has no such property (%s)\n", record.name.c_str(), name.c_str(),
                    record.file.c_str());
                ++errors_;
                continue;
            }
            const RED4ext::CStackType held = current->GetValue();
            Value value(held.type);
            const bool ok = held.type->GetType() == RED4ext::ERTTIType::Array
                                ? ArrayInto(held.type, value.get(), property.second, held.value)
                                : ScalarInto(held.type, value.get(), property.second);
            if (!ok || !writes.Set(flat, held.type, value.get())) {
                Log("[tweaks] %s.%s: not a %s value (%s)\n", record.name.c_str(), name.c_str(),
                    held.type->GetName().ToString(), record.file.c_str());
                ++errors_;
                continue;
            }
            ++flats_;
        }
    }

    TweakDB* db_;
    RED4ext::CRTTISystem* rtti_;
    int created_ = 0, flats_ = 0, errors_ = 0;
};

fs::path TweaksDir() { return fs::path(paths::Dir(paths::Folder::Config)) / L"tweaks"; }

// The files are parsed in parallel -- yaml-cpp is most of the time spent here -- and their records kept in file order.
std::vector<Record> ReadRecords() {
    std::vector<fs::path> files;
    std::error_code error;
    for (auto it = fs::recursive_directory_iterator(TweaksDir(), error); !error && it != fs::recursive_directory_iterator();
         it.increment(error)) {
        const auto extension = Lower(it->path().extension().string());
        if (it->is_regular_file() && (extension == ".yaml" || extension == ".yml")) files.push_back(it->path());
    }
    std::sort(files.begin(), files.end());
    std::vector<std::vector<Record>> perFile(files.size());
    std::vector<std::string> problems(files.size());
    std::vector<size_t> order(files.size());
    std::iota(order.begin(), order.end(), size_t{0});
    std::for_each(std::execution::par, order.begin(), order.end(), [&](size_t i) {
        std::error_code relError;
        const std::string shown = fs::relative(files[i], TweaksDir(), relError).string();
        try {
            std::ifstream stream(files[i], std::ios::binary);
            const YAML::Node document = YAML::Load(stream);
            if (!document.IsMap()) return;
            for (const auto& entry : document) perFile[i].push_back({entry.first.as<std::string>(), shown, entry.second});
        } catch (const std::exception& e) {
            problems[i] = shown + ": " + e.what();
        }
    });
    std::vector<Record> records;
    for (size_t i = 0; i < files.size(); ++i) {
        if (!problems[i].empty()) Log("[tweaks] %s\n", problems[i].c_str());
        for (Record& record : perFile[i]) records.push_back(std::move(record));
    }
    return records;
}

std::vector<NativeTweak>& NativeTweaks() {
    static std::vector<NativeTweak> tweaks;
    return tweaks;
}

void ApplyAll(TweakDB* db) {
    LARGE_INTEGER started{}, read{}, done{}, frequency{};
    QueryPerformanceFrequency(&frequency);
    QueryPerformanceCounter(&started);
    const std::vector<Record> records = ReadRecords();
    QueryPerformanceCounter(&read);
    std::unordered_set<std::string> pending;
    for (const Record& record : records) pending.insert(record.name);
    Applier applier(db);
    // A level: every record still pending whose $base is not itself pending. Its base is then a record already --
    // the game's, or one an earlier level made with its properties written -- so a copy takes the base's final flats.
    int levels = 0;
    while (!pending.empty()) {
        std::vector<const Record*> level;
        for (const Record& record : records) {
            if (!pending.count(record.name)) continue;
            const YAML::Node base = record.body["$base"];
            if (base && base.as<std::string>() != record.name && pending.count(base.as<std::string>())) continue;
            level.push_back(&record);
        }
        if (level.empty()) {
            for (const Record& record : records)
                if (pending.count(record.name)) applier.Fail(record, "is in a $base cycle");
            break;
        }
        for (const Record* record : level) pending.erase(record->name);
        applier.ApplyLevel(level);
        ++levels;
    }
    for (NativeTweak tweak : NativeTweaks()) tweak(db);
    QueryPerformanceCounter(&done);
    const double toMs = 1000.0 / double(frequency.QuadPart);
    Log("[tweaks] %zu records from config\\tweaks in %d level(s): %d created, %d flats written, %d errors, "
        "%zu native tweak(s) (read %.1f ms, applied %.1f ms)\n",
        records.size(), levels, applier.created(), applier.flats(), applier.errors(), NativeTweaks().size(),
        double(read.QuadPart - started.QuadPart) * toMs, double(done.QuadPart - read.QuadPart) * toMs);
}

bool Detour_TryLoadTweakDB(void* a1, TweakDB* db, RED4ext::CString& path, void* a4, bool a5) {
    const bool loaded = g_tryLoad(a1, db, path, a4, a5);
    if (loaded && db) ApplyAll(db);
    return loaded;
}

// An earlier release installed the same files in r6\tweaks, where TweakXL would apply them a second time: the copies
// that are the port's are taken out (same bytes: deleted; different: renamed .legacy, which TweakXL does not read).
void RetireLegacyCopies() {
    // A file a release no longer carries stays behind when the next one is unpacked over it, and every file in the
    // folder is applied. VRPort.LobbyBody.yaml (0.2.0) marked the man's entity `initial`: what that does to saves is
    // in Lobby/PlayerBody.cpp.
    {
        std::error_code removed;
        const fs::path retired = TweaksDir() / L"vrport" / L"VRPort.LobbyBody.yaml";
        if (fs::remove(retired, removed)) Log("[tweaks] removed %ls: no release carries it any more\n", retired.c_str());
    }
    std::vector<wchar_t> exe(32768);
    const DWORD length = GetModuleFileNameW(nullptr, exe.data(), static_cast<DWORD>(exe.size()));
    if (!length || length >= exe.size()) return;
    const fs::path legacy = fs::path(std::wstring(exe.data(), length)).parent_path().parent_path().parent_path() / L"r6" / L"tweaks";
    std::error_code error;
    for (auto it = fs::recursive_directory_iterator(TweaksDir(), error); !error && it != fs::recursive_directory_iterator();
         it.increment(error)) {
        if (!it->is_regular_file()) continue;
        const fs::path old = legacy / fs::relative(it->path(), TweaksDir(), error);
        if (error || !fs::exists(old, error)) continue;
        auto bytes = [](const fs::path& p) {
            std::ifstream s(p, std::ios::binary);
            return std::string(std::istreambuf_iterator<char>(s), {});
        };
        if (bytes(old) == bytes(it->path())) {
            fs::remove(old, error);
            Log("[tweaks] removed the earlier release's copy %ls\n", old.c_str());
        } else {
            fs::path aside = old;
            aside += L".legacy";
            fs::rename(old, aside, error);
            Log("[tweaks] set the earlier release's copy aside: %ls\n", aside.c_str());
        }
        fs::path dir = old.parent_path();
        while (dir != legacy && fs::is_empty(dir, error) && !error) {
            fs::remove(dir, error);
            dir = dir.parent_path();
        }
    }
}

}  // namespace

void AddNativeTweak(NativeTweak tweak) { NativeTweaks().push_back(tweak); }

RED4ext::CStackType FlatWrites::Get(TweakDBID flat) const {
    TweakDB::FlatValue* value = db_->GetFlatValue(flat);
    return value ? value->GetValue() : RED4ext::CStackType();
}

bool FlatWrites::Set(TweakDBID flat, IType* type, void* value) {
    if (TweakDB::FlatValue* current = db_->GetFlatValue(flat)) {
        const RED4ext::CStackType held = current->GetValue();
        if (held.type == type && type->IsEqual(held.value, value)) return true;
    }
    // The SDK's CreateFlatValue MOVES from what it is given (std::forward of a non-reference type). Handed a flat's own
    // value -- shared by every flat that holds the same one -- it emptied them all: copying a state's alias into the
    // launcher's state cleared the alias of five others. It gets a copy of its own.
    Value copy(type);
    type->Assign(copy.get(), value);
    const int32_t offset = db_->CreateFlatValue(RED4ext::CStackType(type, copy.get()));
    if (offset < 0) return false;
    flat.SetTDBOffset(offset);
    pending_.push_back(flat);
    return true;
}

void FlatWrites::Commit() {
    if (pending_.empty()) return;
    RED4ext::SortedUniqueArray<TweakDBID> flats;
    for (const TweakDBID& flat : pending_) flats.InsertOrAssign(flat);
    pending_.clear();
    std::lock_guard<RED4ext::SharedSpinLock> lock(db_->mutex00);
    db_->flats.InsertOrAssign(flats);
}

void Install(RED4ext::v1::PluginHandle handle, const RED4ext::v1::Sdk* sdk) {
    RetireLegacyCopies();
    void* tryLoad = reinterpret_cast<void*>(RED4ext::UniversalRelocBase::Resolve(kTryLoadTweakDBHash));
    g_derive = reinterpret_cast<DeriveFn>(RED4ext::UniversalRelocBase::Resolve(kTweakDBIDDeriveHash));
    const bool hooked = tryLoad && sdk->hooking->Attach(handle, tryLoad, reinterpret_cast<void*>(&Detour_TryLoadTweakDB),
                                                        reinterpret_cast<void**>(&g_tryLoad));
    Log("[tweaks] TweakDB load %s%s\n", hooked ? "hooked" : "NOT hooked: the port's records are missing",
        g_derive ? "" : " (record names not registered)");
}

}  // namespace cvr::tweak_records
