// Typed OpenXR entry points routed through the private V557 instance.
// Generated from OpenXR 1.0.34 signatures; global loader calls remain downstream.

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrAcquireSwapchainImage(
    XrSwapchain                                 swapchain,
    const XrSwapchainImageAcquireInfo*          acquireInfo,
    uint32_t*                                   index) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrAcquireSwapchainImage> cache;
    auto fn = cvr::ofxr::Resolve("xrAcquireSwapchainImage",cache);
    return fn ? fn(swapchain, acquireInfo, index) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrApplyHapticFeedback(
    XrSession                                   session,
    const XrHapticActionInfo*                   hapticActionInfo,
    const XrHapticBaseHeader*                   hapticFeedback) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrApplyHapticFeedback> cache;
    auto fn = cvr::ofxr::Resolve("xrApplyHapticFeedback",cache);
    return fn ? fn(session, hapticActionInfo, hapticFeedback) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrAttachSessionActionSets(
    XrSession                                   session,
    const XrSessionActionSetsAttachInfo*        attachInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrAttachSessionActionSets> cache;
    auto fn = cvr::ofxr::Resolve("xrAttachSessionActionSets",cache);
    return fn ? fn(session, attachInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrBeginFrame(
    XrSession                                   session,
    const XrFrameBeginInfo*                     frameBeginInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrBeginFrame> cache;
    auto fn = cvr::ofxr::Resolve("xrBeginFrame",cache);
    return fn ? fn(session, frameBeginInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrBeginSession(
    XrSession                                   session,
    const XrSessionBeginInfo*                   beginInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrBeginSession> cache;
    auto fn = cvr::ofxr::Resolve("xrBeginSession",cache);
    return fn ? fn(session, beginInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateAction(
    XrActionSet                                 actionSet,
    const XrActionCreateInfo*                   createInfo,
    XrAction*                                   action) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateAction> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateAction",cache);
    return fn ? fn(actionSet, createInfo, action) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSet(
    XrInstance                                  instance,
    const XrActionSetCreateInfo*                createInfo,
    XrActionSet*                                actionSet) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateActionSet> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateActionSet",cache);
    return fn ? fn(instance, createInfo, actionSet) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateActionSpace(
    XrSession                                   session,
    const XrActionSpaceCreateInfo*              createInfo,
    XrSpace*                                    space) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateActionSpace> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateActionSpace",cache);
    return fn ? fn(session, createInfo, space) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateReferenceSpace(
    XrSession                                   session,
    const XrReferenceSpaceCreateInfo*           createInfo,
    XrSpace*                                    space) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateReferenceSpace> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateReferenceSpace",cache);
    return fn ? fn(session, createInfo, space) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateSession(
    XrInstance                                  instance,
    const XrSessionCreateInfo*                  createInfo,
    XrSession*                                  session) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateSession> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateSession",cache);
    return fn ? fn(instance, createInfo, session) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrCreateSwapchain(
    XrSession                                   session,
    const XrSwapchainCreateInfo*                createInfo,
    XrSwapchain*                                swapchain) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrCreateSwapchain> cache;
    auto fn = cvr::ofxr::Resolve("xrCreateSwapchain",cache);
    return fn ? fn(session, createInfo, swapchain) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrDestroyActionSet(
    XrActionSet                                 actionSet) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrDestroyActionSet> cache;
    auto fn = cvr::ofxr::Resolve("xrDestroyActionSet",cache);
    return fn ? fn(actionSet) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrDestroySession(
    XrSession                                   session) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrDestroySession> cache;
    auto fn = cvr::ofxr::Resolve("xrDestroySession",cache);
    return fn ? fn(session) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrDestroySpace(
    XrSpace                                     space) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrDestroySpace> cache;
    auto fn = cvr::ofxr::Resolve("xrDestroySpace",cache);
    return fn ? fn(space) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrDestroySwapchain(
    XrSwapchain                                 swapchain) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrDestroySwapchain> cache;
    auto fn = cvr::ofxr::Resolve("xrDestroySwapchain",cache);
    return fn ? fn(swapchain) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEndFrame(
    XrSession                                   session,
    const XrFrameEndInfo*                       frameEndInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEndFrame> cache;
    auto fn = cvr::ofxr::Resolve("xrEndFrame",cache);
    return fn ? fn(session, frameEndInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEndSession(
    XrSession                                   session) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEndSession> cache;
    auto fn = cvr::ofxr::Resolve("xrEndSession",cache);
    return fn ? fn(session) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateBoundSourcesForAction(
    XrSession                                   session,
    const XrBoundSourcesForActionEnumerateInfo* enumerateInfo,
    uint32_t                                    sourceCapacityInput,
    uint32_t*                                   sourceCountOutput,
    XrPath*                                     sources) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEnumerateBoundSourcesForAction> cache;
    auto fn = cvr::ofxr::Resolve("xrEnumerateBoundSourcesForAction",cache);
    return fn ? fn(session, enumerateInfo, sourceCapacityInput, sourceCountOutput, sources) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateInstanceExtensionProperties(
    const char*                                 layerName,
    uint32_t                                    propertyCapacityInput,
    uint32_t*                                   propertyCountOutput,
    XrExtensionProperties*                      properties) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEnumerateInstanceExtensionProperties> cache;
    auto fn = cvr::ofxr::Resolve("xrEnumerateInstanceExtensionProperties",cache);
    return fn ? fn(layerName, propertyCapacityInput, propertyCountOutput, properties) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateSwapchainFormats(
    XrSession                                   session,
    uint32_t                                    formatCapacityInput,
    uint32_t*                                   formatCountOutput,
    int64_t*                                    formats) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEnumerateSwapchainFormats> cache;
    auto fn = cvr::ofxr::Resolve("xrEnumerateSwapchainFormats",cache);
    return fn ? fn(session, formatCapacityInput, formatCountOutput, formats) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateSwapchainImages(
    XrSwapchain                                 swapchain,
    uint32_t                                    imageCapacityInput,
    uint32_t*                                   imageCountOutput,
    XrSwapchainImageBaseHeader*                 images) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEnumerateSwapchainImages> cache;
    auto fn = cvr::ofxr::Resolve("xrEnumerateSwapchainImages",cache);
    return fn ? fn(swapchain, imageCapacityInput, imageCountOutput, images) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrEnumerateViewConfigurationViews(
    XrInstance                                  instance,
    XrSystemId                                  systemId,
    XrViewConfigurationType                     viewConfigurationType,
    uint32_t                                    viewCapacityInput,
    uint32_t*                                   viewCountOutput,
    XrViewConfigurationView*                    views) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrEnumerateViewConfigurationViews> cache;
    auto fn = cvr::ofxr::Resolve("xrEnumerateViewConfigurationViews",cache);
    return fn ? fn(instance, systemId, viewConfigurationType, viewCapacityInput, viewCountOutput, views) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStateBoolean(
    XrSession                                   session,
    const XrActionStateGetInfo*                 getInfo,
    XrActionStateBoolean*                       state) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetActionStateBoolean> cache;
    auto fn = cvr::ofxr::Resolve("xrGetActionStateBoolean",cache);
    return fn ? fn(session, getInfo, state) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStateFloat(
    XrSession                                   session,
    const XrActionStateGetInfo*                 getInfo,
    XrActionStateFloat*                         state) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetActionStateFloat> cache;
    auto fn = cvr::ofxr::Resolve("xrGetActionStateFloat",cache);
    return fn ? fn(session, getInfo, state) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStatePose(
    XrSession                                   session,
    const XrActionStateGetInfo*                 getInfo,
    XrActionStatePose*                          state) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetActionStatePose> cache;
    auto fn = cvr::ofxr::Resolve("xrGetActionStatePose",cache);
    return fn ? fn(session, getInfo, state) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetActionStateVector2f(
    XrSession                                   session,
    const XrActionStateGetInfo*                 getInfo,
    XrActionStateVector2f*                      state) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetActionStateVector2f> cache;
    auto fn = cvr::ofxr::Resolve("xrGetActionStateVector2f",cache);
    return fn ? fn(session, getInfo, state) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetInstanceProperties(
    XrInstance                                  instance,
    XrInstanceProperties*                       instanceProperties) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetInstanceProperties> cache;
    auto fn = cvr::ofxr::Resolve("xrGetInstanceProperties",cache);
    return fn ? fn(instance, instanceProperties) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetSystem(
    XrInstance                                  instance,
    const XrSystemGetInfo*                      getInfo,
    XrSystemId*                                 systemId) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetSystem> cache;
    auto fn = cvr::ofxr::Resolve("xrGetSystem",cache);
    return fn ? fn(instance, getInfo, systemId) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrGetSystemProperties(
    XrInstance                                  instance,
    XrSystemId                                  systemId,
    XrSystemProperties*                         properties) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrGetSystemProperties> cache;
    auto fn = cvr::ofxr::Resolve("xrGetSystemProperties",cache);
    return fn ? fn(instance, systemId, properties) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrLocateSpace(
    XrSpace                                     space,
    XrSpace                                     baseSpace,
    XrTime                                      time,
    XrSpaceLocation*                            location) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrLocateSpace> cache;
    auto fn = cvr::ofxr::Resolve("xrLocateSpace",cache);
    return fn ? fn(space, baseSpace, time, location) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrLocateViews(
    XrSession                                   session,
    const XrViewLocateInfo*                     viewLocateInfo,
    XrViewState*                                viewState,
    uint32_t                                    viewCapacityInput,
    uint32_t*                                   viewCountOutput,
    XrView*                                     views) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrLocateViews> cache;
    auto fn = cvr::ofxr::Resolve("xrLocateViews",cache);
    return fn ? fn(session, viewLocateInfo, viewState, viewCapacityInput, viewCountOutput, views) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrPathToString(
    XrInstance                                  instance,
    XrPath                                      path,
    uint32_t                                    bufferCapacityInput,
    uint32_t*                                   bufferCountOutput,
    char*                                       buffer) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrPathToString> cache;
    auto fn = cvr::ofxr::Resolve("xrPathToString",cache);
    return fn ? fn(instance, path, bufferCapacityInput, bufferCountOutput, buffer) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrPollEvent(
    XrInstance                                  instance,
    XrEventDataBuffer*                          eventData) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrPollEvent> cache;
    auto fn = cvr::ofxr::Resolve("xrPollEvent",cache);
    return fn ? fn(instance, eventData) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrReleaseSwapchainImage(
    XrSwapchain                                 swapchain,
    const XrSwapchainImageReleaseInfo*          releaseInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrReleaseSwapchainImage> cache;
    auto fn = cvr::ofxr::Resolve("xrReleaseSwapchainImage",cache);
    return fn ? fn(swapchain, releaseInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrRequestExitSession(
    XrSession                                   session) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrRequestExitSession> cache;
    auto fn = cvr::ofxr::Resolve("xrRequestExitSession",cache);
    return fn ? fn(session) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrStringToPath(
    XrInstance                                  instance,
    const char*                                 pathString,
    XrPath*                                     path) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrStringToPath> cache;
    auto fn = cvr::ofxr::Resolve("xrStringToPath",cache);
    return fn ? fn(instance, pathString, path) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrSuggestInteractionProfileBindings(
    XrInstance                                  instance,
    const XrInteractionProfileSuggestedBinding* suggestedBindings) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrSuggestInteractionProfileBindings> cache;
    auto fn = cvr::ofxr::Resolve("xrSuggestInteractionProfileBindings",cache);
    return fn ? fn(instance, suggestedBindings) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrSyncActions(
    XrSession                                   session,
    const XrActionsSyncInfo*                    syncInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrSyncActions> cache;
    auto fn = cvr::ofxr::Resolve("xrSyncActions",cache);
    return fn ? fn(session, syncInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrWaitFrame(
    XrSession                                   session,
    const XrFrameWaitInfo*                      frameWaitInfo,
    XrFrameState*                               frameState) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrWaitFrame> cache;
    auto fn = cvr::ofxr::Resolve("xrWaitFrame",cache);
    return fn ? fn(session, frameWaitInfo, frameState) : XR_ERROR_FUNCTION_UNSUPPORTED;
}

extern "C" XRAPI_ATTR XrResult XRAPI_CALL xrWaitSwapchainImage(
    XrSwapchain                                 swapchain,
    const XrSwapchainImageWaitInfo*             waitInfo) {
    thread_local cvr::ofxr::DispatchCache<PFN_xrWaitSwapchainImage> cache;
    auto fn = cvr::ofxr::Resolve("xrWaitSwapchainImage",cache);
    return fn ? fn(swapchain, waitInfo) : XR_ERROR_FUNCTION_UNSUPPORTED;
}
