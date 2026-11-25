#pragma once
#include <imgui.h>

namespace overlay::widgets {
bool SliderFloat(const char* label,float* value,float minimum,float maximum,
                 const char* format="%.3f",ImGuiSliderFlags flags=0);
bool SliderInt(const char* label,int* value,int minimum,int maximum,
               const char* format="%d",ImGuiSliderFlags flags=0);
bool Combo(const char* label,int* selected,const char* const* items,int count,int height=-1);
bool Combo(const char* label,int* selected,const char* zeroSeparatedItems,int height=-1);
void Section(const char* label);
void Tabs(const char* id,int& selected,const char* const* labels,int count);
void DrawBindings(bool cyberfinger=false);
}
