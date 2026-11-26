#include "Anim/WeaponRig.hpp"
#include "Anim/RigPoseWrites.hpp"
#include "Core/Paths.hpp"
#include <array>
#include <cmath>
#include <cstring>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <thread>

uintptr_t g_gameModuleBase{};
size_t g_gameModuleSize{};
// NativeState also defines diagnostic paths, unused by this headless pose test.
namespace cvr::paths {
std::string FileA(Folder, const char*) { throw std::runtime_error("unexpected diagnostic file access"); }
}
using namespace cvr::anim;
void Check(bool value, const char* what) { if (!value) throw std::runtime_error(what); }
bool Near(float a, float b) { return std::abs(a-b) < 1e-5f; }

struct Fixture {
    alignas(16) std::array<uint8_t, 256> rig{}, remapping{}, descriptor{};
    void* context[9]{};
    void* output[7]{};
    uint64_t names[8]{};
    uint32_t mapping[2]{3, 1};
    uint16_t counts[4]{1, 1, 0, 0};
    float track{};
    float bones[24]{};
    template<class T> static void Put(uint8_t* into, size_t offset, T value) {
        std::memcpy(into+offset, &value, sizeof(value));
    }
    Fixture() {
        ClearRigWrites();
        names[3] = 42;
        context[8] = rig.data(); output[6] = remapping.data();
        Put(rig.data(), 0x50, names); Put(rig.data(), 0x58, uint32_t(8));
        Put(remapping.data(), 0x40, mapping); Put(remapping.data(), 0x50, counts);
        Put(descriptor.data(), 8, uint32_t(2));
        g_RigSigN = 1; g_RigSigWhich[0] = 1; g_RigSigBones[0] = 8;
        for (int i=0;i<VRRIG_SIG_NAMES;++i) g_RigSigIdx[0][i] = -1;
        g_RigSigIdx[0][0] = 3; g_RigSigHash[0][0] = 42;
        Native(.1f);
    }
    float* Part() { return bones+12; }
    void Native(float position) {
        std::fill(std::begin(bones), std::end(bones), 0.0f);
        bones[0] = 987; // unmapped guard bone
        for (int i=0;i<3;++i) { Part()[i] = position; Part()[8+i] = 1; }
        Part()[7] = 1;
    }
    void Apply(unsigned lod=0) {
        WeaponRigIdentifyAndWrite(context, output, lod, descriptor.data(),
            reinterpret_cast<uint8_t*>(bones), reinterpret_cast<uintptr_t>(&track));
        Check(bones[0] == 987, "write escaped remapped bone");
    }
};

static bool FaultedPose() {
    __try {
        WeaponRigIdentifyAndWrite(nullptr, nullptr, 0, nullptr, nullptr, 0);
        return false;
    } __except(EXCEPTION_EXECUTE_HANDLER) { return true; }
}

int main() try {
    Fixture f;
    int slot=WriteRigScale(1,3,.001f);
    f.Apply(); Check(Near(f.Part()[8],.001f), "hidden scale not applied");
    f.Native(.3f); f.Apply();
    Check(Near(f.Part()[0],.3f) && !g_RigWriteHaveBase[slot], "scale pinned animated translation");
    WriteRigScale(1,3,0); f.Native(.4f); f.Apply();
    Check(Near(f.Part()[0],.4f) && f.Part()[8]==1, "scale release retained an override");

    ClearRigWrites(); WriteRigRotation(1,3,60,0,0,1); f.Native(.1f); f.Apply();
    Check(Near(f.Part()[6],.5f), "rotation did not apply");
    f.Native(.2f); f.Apply();
    Check(Near(f.Part()[0],.2f) && Near(f.Part()[6],.5f), "rotation pinned translation or accumulated");

    ClearRigWrites(); WriteRigOffset(1,3,.05f,0,0); WriteRigScale(1,3,.001f);
    f.Native(.1f); f.Apply(); Check(Near(f.Part()[0],.15f), "explicit offset missing");
    f.Apply(); Check(Near(f.Part()[0],.15f), "relative offset accumulated across passes");
    WriteRigScale(1,3,0); f.Native(.1f); f.Apply();
    Check(Near(f.Part()[0],.15f) && f.Part()[8]==1, "scale release cleared another channel");
    ReleaseRigWrite(1,3); f.Native(.35f); f.Apply();
    Check(Near(f.Part()[0],.35f), "released bone still overridden");

    ClearRigWrites(); WriteRigPose(1,3,.2f,.3f,.4f,0,0,0,1,true);
    f.Native(.1f); f.Apply(2); Check(Near(f.Part()[2],.4f), "absolute pose did not survive omitted LOD bone");
    WriteRigOffset(1,3,.05f,0,0); f.Native(.1f); f.Apply();
    Check(Near(f.Part()[0],.15f), "offset reused absolute-coordinate mode");
    f.Native(.6f); f.Apply(2); Check(Near(f.Part()[0],.6f), "relative pose wrote an omitted LOD bone");

    ClearRigWrites();
    const float nan=std::numeric_limits<float>::quiet_NaN();
    Check(WriteRigOffset(1,3,nan,0,0)==-1 && WriteRigRotation(1,3,nan,0,0,1)==-1 &&
          WriteRigScale(1,3,nan)==-1 && WriteRigPose(1,3,0,0,0,0,0,0,nan,true)==-1 &&
          WriteRigOffset(VRRIG_N,3,0,0,0)==-1 && g_RigWriteN==0, "invalid request published a slot");

    // Pose jobs must see a whole request, never x from one update and y from another.
    WriteRigPose(1,3,.1f,.1f,.1f,0,0,0,1,true);
    std::thread script([] {
        for (int i=0;i<20000;++i) {
            const float p=i%2 ? .1f : .2f;
            WriteRigPose(1,3,p,p,p,0,0,0,1,true);
        }
    });
    bool coherent=true;
    for (int i=0;i<20000;++i) {
        f.Native(0); f.Apply();
        coherent &= f.Part()[0]==f.Part()[1] && f.Part()[1]==f.Part()[2] &&
                    (f.Part()[0]==.1f || f.Part()[0]==.2f);
    }
    script.join(); Check(coherent,"animation saw a partially published pose request");
    Check(FaultedPose(),"invalid engine pointer did not exercise SEH");
    ClearRigWrites(); // also proves the fault's finally block released the lock
    WriteRigScale(1,3,.001f); f.Native(.25f); f.Apply();
    Check(Near(f.Part()[0],.25f),"reused slot inherited old position channel");
    std::cout << "PASS: production rig channel isolation, remapping, atomic requests and SEH unlock\n";
    return 0;
} catch(const std::exception& error) {
    std::cerr << error.what() << '\n'; return 1;
}
