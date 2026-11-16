#pragma once
#include "Stereo/ScopeDlssState.hpp"
#include <sl_core_types.h>
#include <sl_consts.h>
#ifndef SL_FEATURE_FUN_IMPORT_STATIC
// This port imports feature functions itself; SDK inline helpers are unused.
#define CVR_SCOPE_LOCAL_SL_IMPORT
#define SL_FEATURE_FUN_IMPORT_STATIC(feature,name) static PFun_##name* s_##name = nullptr
#endif
#include <sl_dlss_d.h>
#ifdef CVR_SCOPE_LOCAL_SL_IMPORT
#undef SL_FEATURE_FUN_IMPORT_STATIC
#undef CVR_SCOPE_LOCAL_SL_IMPORT
#endif

namespace cvr::scope_dlss {
// RR's mode defines its expected input size. MAIN's Ultra Performance mode
// cannot describe a scope whose actual tagged input and output are BOTH341x341:
// it asks NGX to consume114x114 instead. Preserve native dimensions/matrices and
// use the SDK's 1:1 mode only for this exact, independently identified viewport.
inline bool MatchRrMode(uint32_t viewport,sl::DLSSDOptions& options,
                        uint32_t inputW,uint32_t inputH,uint32_t outputW,uint32_t outputH) {
    if(viewport!=kViewport || options.structType!=sl::DLSSDOptions::s_structType ||
       options.structVersion!=sl::kStructVersion3 || options.mode==sl::DLSSMode::eOff ||
       inputW<16 || inputH<16 || inputW>8192 || inputH>8192 ||
       inputW!=outputW || inputH!=outputH ||
       options.outputWidth!=outputW || options.outputHeight!=outputH) return false;
    options.mode=sl::DLSSMode::eDLAA;
    return true;
}
}
