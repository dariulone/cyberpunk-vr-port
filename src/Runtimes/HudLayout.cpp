#include "Runtimes/HudLayout.hpp"

#if CVR_RUST_VRIK
#if !CVR_RUST_VRIK || CVR_RUST_TEST_FACADES   // the plugin does not use it; the Rust test variants do
// THE LAYOUT IS RUST'S (rust/crates/cvr-settings/src/hud_layout.rs): the elements' edits and status, and the
// hud_element_<key> lines read with sscanf and written with fprintf. These are the C++ names.
extern "C" {
void cvr_hud_layout_set_settings(const cvr::hud::ElementSettings* value);
bool cvr_hud_layout_parse(const char* line, cvr::hud::ElementSettings* settings);
}
namespace cvr::hud {
static_assert(sizeof(ElementSettings) == 20 && sizeof(ElementStatus) == 20 && ElementCount == 40,
              "mirrored in rust/crates/cvr-settings/src/hud_layout.rs");
void SetLayoutSettings(const LayoutSettings& value) { cvr_hud_layout_set_settings(value.data()); }
bool ParseLayoutSetting(const char* line, LayoutSettings& value) { return cvr_hud_layout_parse(line, value.data()); }
}  // namespace cvr::hud
#endif
#else
#include <mutex>
#include <cstring>

namespace cvr::hud {
namespace { std::mutex mutex; LayoutSettings settings; LayoutStatus status; }
LayoutSettings GetLayoutSettings() { std::lock_guard lock(mutex);return settings; }
void SetLayoutSettings(const LayoutSettings& value) {
    std::lock_guard lock(mutex);
    for(size_t i=0;i<ElementCount;++i) settings[i]=Sanitize(value[i]);
}
LayoutStatus GetLayoutStatus() { std::lock_guard lock(mutex);return status; }
void SetLayoutStatus(const LayoutStatus& value) { std::lock_guard lock(mutex);status=value; }
bool ParseLayoutSetting(const char* line,LayoutSettings& value) {
    char key[80]{};
    for(size_t i=0;i<ElementCount;++i) {
        std::snprintf(key,sizeof(key),"hud_element_%s",Elements[i].key);
        const size_t n=std::strlen(key);
        if(std::strncmp(line,key,n)) continue;
        const char* p=line+n;while(*p==' ' || *p=='\t')++p;if(*p!='=')continue;
        ElementSettings s;
        if(std::sscanf(p+1,"%f , %f , %f , %f , %d",&s.x,&s.y,&s.scale,&s.opacity,&s.visible)==5)
            value[i]=Sanitize(s);
        return true;
    }
    return false;
}
void SaveLayoutSettings(FILE* file,const LayoutSettings& value) {
    for(size_t i=0;i<ElementCount;++i) {
        const auto s=Sanitize(value[i]);
        std::fprintf(file,"hud_element_%s=%.3f,%.3f,%.4f,%.4f,%d\n",Elements[i].key,s.x,s.y,s.scale,s.opacity,s.visible);
    }
}
}
#endif
