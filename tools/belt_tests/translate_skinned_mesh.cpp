// Translate an already placed/skinned belt mesh in the player's bind frame.
// JSON and binaries are orchestrated externally; all matrix/box arithmetic is C++.
#include <nlohmann/json.hpp>
#include <array>
#include <cmath>
#include <fstream>
#include <iostream>
#include <stdexcept>
#include <string>
using Json = nlohmann::ordered_json;
static constexpr const char* Axes[]{"X", "Y", "Z"};
static Json Read(const char* path) { std::ifstream f(path); if (!f) throw std::runtime_error(path); Json j; f >> j; return j; }
static Json& Data(Json& j) { return j.contains("Data") ? j["Data"] : j; }
int main(int argc, char** argv) try {
    if (argc != 7) throw std::runtime_error("reference.json mesh.json output.json x y z");
    auto reference = Read(argv[1]), mesh = Read(argv[2]);
    auto& ref = reference["Data"]["RootChunk"];
    auto& root = mesh["Data"]["RootChunk"];
    std::array<double, 3> delta{};
    for (int i=0; i!=3; ++i) { delta[i]=std::stod(argv[4+i]); if (!std::isfinite(delta[i])) throw std::runtime_error("invalid translation"); }
    const auto original = root;
    for (size_t bone=0; bone<root["boneNames"].size(); ++bone) {
        const auto name=root["boneNames"][bone]["$value"];
        size_t r=0;
        while (r<ref["boneNames"].size() && ref["boneNames"][r]["$value"]!=name) ++r;
        if (r==ref["boneNames"].size()) throw std::runtime_error("reference bone missing: "+name.get<std::string>());
        auto& bind=Data(ref["boneRigMatrices"][r]);
        auto& placed=Data(root["boneRigMatrices"][bone]);
        // B' = B * inverse(B_ref) * T(delta) * B_ref. For a pure translation,
        // only W changes, by delta expressed in the reference bone's frame.
        for (int j=0; j!=3; ++j) {
            double shift=0;
            for (int i=0; i!=3; ++i) shift+=delta[i]*bind[Axes[i]][Axes[j]].get<double>();
            placed["W"][Axes[j]]=placed["W"][Axes[j]].get<double>()+shift;
        }
    }
    if (root["boneNames"].empty()) throw std::runtime_error("mesh is not skinned");
    for (const char* side : {"Min", "Max"}) for (int i=0; i!=3; ++i)
        root["boundingBox"][side][Axes[i]]=root["boundingBox"][side][Axes[i]].get<double>()+delta[i];
    // The operation must leave vertex data, normals, skin weights and material
    // definitions byte-for-byte equal at the JSON value level.
    auto untouched=root;
    untouched["boneRigMatrices"]=original["boneRigMatrices"];
    untouched["boundingBox"]=original["boundingBox"];
    if (untouched!=original) throw std::runtime_error("unexpected mesh modification");
    std::ofstream out(argv[3]); if (!out) throw std::runtime_error(argv[3]); out << mesh.dump(2) << '\n'; out.close();
    std::cout << argv[3] << " center { ";
    for(int i=0; i!=3; ++i) std::cout << (root["boundingBox"]["Min"][Axes[i]].get<double>()+root["boundingBox"]["Max"][Axes[i]].get<double>())*.5 << (i==2?" }\n":", ");
    return 0;
} catch(const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
