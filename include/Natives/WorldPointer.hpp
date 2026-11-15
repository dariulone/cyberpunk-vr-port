#pragma once
#include <RED4ext/RED4ext.hpp>
#include <RED4ext/Scripting/Natives/inkWidget.hpp>
namespace cvr::worldpointer {
// Native mesh/UV intersection on the currently zoomed computer only.
bool ScreenRay(const RED4ext::Handle<RED4ext::IScriptable>& component,
    RED4ext::ink::Widget* window,int hand,RED4ext::Vector2& local,RED4ext::Vector2& cursor);
}
