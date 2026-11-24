#pragma once
#include "Runtimes/HudFollow.hpp"
#include <openxr/openxr.h>

namespace cvr::loading {
// The artwork is mono. Give the compositor one image, its physical size and
// its anchor so it can place it for either eye even between game presents.
inline XrCompositionLayerQuad Panel(const XrSwapchainSubImage& image, float aspect,
                                    XrSpace local, XrSpace view, const XrPosef* head,
                                    const XrCompositionLayerQuad* previous) {
    XrCompositionLayerQuad quad{XR_TYPE_COMPOSITION_LAYER_QUAD};
    quad.eyeVisibility = XR_EYE_VISIBILITY_BOTH;
    quad.subImage = image;
    quad.size = {1.8f * aspect, 1.8f};
    if (previous && previous->space == local) {
        quad.space = local;
        quad.pose = previous->pose;
    } else if (head) {
        const auto& q = head->orientation;
        const float yaw = cvr::hud::HeadYaw(q.x, q.y, q.z, q.w, 0.0f);
        quad.space = local;
        quad.pose.orientation = {0, std::sin(yaw * 0.5f), 0, std::cos(yaw * 0.5f)};
        quad.pose.position = {head->position.x - 3.0f * std::sin(yaw), head->position.y,
                              head->position.z - 3.0f * std::cos(yaw)};
    } else {
        // Tracking can be absent on the first frame. Anchor on the first valid locate.
        quad.space = view;
        quad.pose = {{0, 0, 0, 1}, {0, 0, -3}};
    }
    return quad;
}
}
