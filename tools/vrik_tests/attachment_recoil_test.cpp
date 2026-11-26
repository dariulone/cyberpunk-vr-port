#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <limits>
#include <thread>
#include <cstdint>

// Exercise the production spring and final pose sample with no game or XR session.
namespace cvr::anim { bool WeaponInLeftHand() { return false; } }
namespace cvr::haptics { void Shot(int, bool, int, float, bool) {} }
extern "C" {
    int CyberpunkVR_RuntimeDiagnostics = 0, CyberpunkVR_TwoHandActive = 0;
    float CyberpunkVR_TwoHandRecoil = 1, CyberpunkVR_TwoHandLever = 0, CyberpunkVR_TwoHandWeight = 0;
    volatile uint64_t CyberpunkVR_DualWieldUntil = 0;
    extern int CyberpunkVR_WeaponClass;
    extern float CyberpunkVR_WeaponKickDeg;
    void RecoilTick();
    void RecoilOnShot();
    void RecoilOnShotLeft();
    void RecoilSample(int, int, float*, float*);
}
void RecoilSetAttachmentScale(int side, float scale);
uint32_t RecoilLastShotPeak(int side, float& degrees);

struct Sample { float travel, rise; };
static Sample sample(int side, int weaponHand) {
    Sample pose{};
    RecoilSample(side, weaponHand, &pose.travel, &pose.rise);
    return pose;
}
static void checkScale(Sample base, Sample result, float scale) {
    if (!(base.travel > 0 && base.rise > 0 &&
          std::fabs(result.travel / base.travel - scale) < 1e-5f &&
          std::fabs(result.rise / base.rise - scale) < 1e-5f)) {
        std::fprintf(stderr, "Expected %g of (%g, %g), got (%g, %g)\n",
                     scale, base.travel, base.rise, result.travel, result.rise);
        std::exit(1);
    }
}

// RecoilSample does not advance the spring. Both reads use the same solved
// displacement, gun, grip, caps and player scale. No inter-shot timing enters the ratio.
static void checkRifleOutput(const char* grip) {
    RecoilSetAttachmentScale(1, 1.0f);
    const Sample bare = sample(1, 1);
    RecoilSetAttachmentScale(1, .8f);
    const Sample brake = sample(1, 1);
    checkScale(bare, brake, .8f);
    RecoilSetAttachmentScale(1, 1.0f);
    checkScale(bare, sample(1, 1), 1.0f);
    std::printf("%s bare_m=%.9f brake_m=%.9f travel_ratio=%.9f "
                "bare_rad=%.9f brake_rad=%.9f rise_ratio=%.9f\n",
                grip, bare.travel, brake.travel, brake.travel / bare.travel,
                bare.rise, brake.rise, brake.rise / bare.rise);
}
int main() {
    CyberpunkVR_DualWieldUntil = UINT64_MAX;
    RecoilTick();
    RecoilOnShot();
    RecoilOnShotLeft();
    std::this_thread::sleep_for(std::chrono::milliseconds(12));
    RecoilTick();
    const auto primary = sample(1, 1), partner = sample(0, 0);
    float degrees;
    if (RecoilLastShotPeak(1, degrees) != 1 || std::fabs(degrees - primary.rise * 57.29578f) > 1e-4f)
        return 1;
    RecoilSetAttachmentScale(1, .8f);
    RecoilSetAttachmentScale(0, .65f);
    checkScale(primary, sample(1, 1), .8f);
    checkScale(partner, sample(0, 0), .65f);
    RecoilSetAttachmentScale(0, 0);
    checkScale(partner, sample(0, 0), 0);
    checkScale(primary, sample(1, 1), .8f);

    CyberpunkVR_DualWieldUntil = 0;
    CyberpunkVR_TwoHandActive = 1;
    checkScale(partner, sample(0, 0), .8f);
    RecoilSetAttachmentScale(1, 1);
    checkScale(primary, sample(1, 1), 1);
    checkScale(partner, sample(0, 0), 1);
    RecoilSetAttachmentScale(1, std::numeric_limits<float>::quiet_NaN());
    checkScale(primary, sample(1, 1), 1);
    CyberpunkVR_WeaponClass = 2;
    CyberpunkVR_WeaponKickDeg = 1.0f;
    CyberpunkVR_TwoHandActive = 0;
    checkRifleOutput("rifle_single");
    CyberpunkVR_TwoHandActive = 1;
    CyberpunkVR_TwoHandLever = .9f;
    CyberpunkVR_TwoHandRecoil = .4f;
    CyberpunkVR_TwoHandWeight = 1.0f;
    checkRifleOutput("rifle_supported");
    const auto partnerSeq = RecoilLastShotPeak(0, degrees);
    const float partnerPeak = degrees;
    std::this_thread::sleep_for(std::chrono::milliseconds(50));
    RecoilSetAttachmentScale(1, 0);
    RecoilOnShot();
    RecoilTick();
    sample(1, 1);
    if (RecoilLastShotPeak(1, degrees) != 2 || degrees != 0) return 1;
    if (RecoilLastShotPeak(0, degrees) != partnerSeq || degrees != partnerPeak || partnerPeak <= 0) return 1;
}
