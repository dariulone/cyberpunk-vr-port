#pragma once
#include <algorithm>
#include <cmath>
#include <cstddef>

namespace cvr::stereo {
inline constexpr size_t CascadeMatrixCount=4;
inline constexpr size_t CascadeMatrixFloatCount=CascadeMatrixCount*16;

// CSConstants contains four affine world-to-shadow matrices. The native
// quality setting selects a contiguous active prefix; unused matrices are zero.
#if CVR_RUST_VRIK
// The rules: rust/crates/cvr-render/src/cascade_sampling.rs through render_parity_abi.rs (tools/render_parity_tests'
// cascade cases run against them in their Rust variant).
extern "C" {
size_t cvr_casc_active(const float*);
bool cvr_casc_same_layout(const float*,const float*,float*);
}
inline size_t ActiveCascadeMatrices(const float* matrices){return cvr_casc_active(matrices);}
// Matches extents for every active cascade. Translation and orientation are deliberately free: sharing them is the
// purpose of the stereo correction.
inline bool SameCascadeSamplingLayout(const float* a,const float* b,float* worst=nullptr){return cvr_casc_same_layout(a,b,worst);}
#else
inline size_t ActiveCascadeMatrices(const float* matrices) {
    if(!matrices)return 0;
    size_t count=0;bool ended=false;
    for(size_t i=0;i<CascadeMatrixCount;++i) {
        const auto* m=matrices+16*i;bool zero=true;
        for(size_t j=0;j<16;++j) {
            if(!std::isfinite(m[j]))return 0;
            if(m[j]!=0)zero=false;
        }
        if(zero){ended=true;continue;}
        if(ended || m[3]!=0 || m[7]!=0 || m[11]!=0 || m[15]!=1)return 0;
        for(size_t column=0;column<3;++column) {
            const double length2=double(m[column])*m[column]+double(m[4+column])*m[4+column]+
                                 double(m[8+column])*m[8+column];
            if(length2<=0 || !std::isfinite(length2))return 0;
        }
        ++count;
    }
    return count;
}

inline bool SameCascadeSamplingLayout(const float* a,const float* b,float* worst=nullptr) {
    const auto count=ActiveCascadeMatrices(a);
    if(!count || count!=ActiveCascadeMatrices(b)) {
        if(worst)*worst=1;return false;
    }
    double difference=0;
    for(size_t i=0;i<count;++i)for(size_t column=0;column<3;++column) {
        const auto* ma=a+16*i;const auto* mb=b+16*i;double la=0,lb=0;
        for(size_t row=0;row<3;++row){la+=double(ma[4*row+column])*ma[4*row+column];lb+=double(mb[4*row+column])*mb[4*row+column];}
        la=std::sqrt(la);lb=std::sqrt(lb);
        difference=std::max(difference,std::abs(la-lb)/std::max(la,lb));
    }
    if(worst)*worst=float(difference);
    // Match extents for every active cascade. Translation and orientation are
    // deliberately free: sharing them is the purpose of the stereo correction.
    return difference<=.02;
}
#endif
}
