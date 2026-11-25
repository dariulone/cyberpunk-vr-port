#pragma once
#include "Overlay/ModUiApi.h"
#include "Overlay/VrPanel.hpp"
namespace cvr::modui {
void Tracking(const cvr::vrui::Tracking&,ID3D12Device*,bool focused,bool available);
void Suspend();
bool CapturesInput();
bool Visible();
cvr::vrui::View GetView();
std::shared_ptr<cvr::vrui::Canvas> Latest();
const CvrModUiApi* Api();
}
