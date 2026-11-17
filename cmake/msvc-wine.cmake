# Cross-compiling from Linux with the real MSVC toolchain under Wine (github.com/mstorsjo/msvc-wine).
#
#   cmake -S . -B build-wine -G Ninja -DCMAKE_BUILD_TYPE=Release \
#         -DCMAKE_TOOLCHAIN_FILE=$PWD/cmake/msvc-wine.cmake
#   cmake --build build-wine --target cyberpunkvrport_stereo
#
# The msvc-wine install (what install.sh was given) is ~/msvc unless MSVC_WINE_ROOT says otherwise. Wine sees the
# file system case-insensitively, so the SDK's mixed-case headers and the sources' #pragma comment(lib) names resolve
# as they do on Windows.

set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSTEM_PROCESSOR AMD64)
# What the project's Linux-only paths test (cargo's target, dxc.exe under Wine, the FidelityFX toolchain): not
# CMAKE_CROSSCOMPILING, which any toolchain naming CMAKE_SYSTEM_NAME sets -- a Windows one (vcpkg, a preset) too.
set(CVR_MSVC_WINE ON)

if(DEFINED ENV{MSVC_WINE_ROOT})
    set(CVR_MSVC_WINE_ROOT "$ENV{MSVC_WINE_ROOT}" CACHE PATH "msvc-wine install root")
else()
    set(CVR_MSVC_WINE_ROOT "$ENV{HOME}/msvc" CACHE PATH "msvc-wine install root")
endif()
set(cvr_msvc_bin "${CVR_MSVC_WINE_ROOT}/bin/x64")
if(NOT EXISTS "${cvr_msvc_bin}/cl")
    message(FATAL_ERROR "msvc-wine not found at ${CVR_MSVC_WINE_ROOT} (no bin/x64/cl): set MSVC_WINE_ROOT")
endif()

set(CMAKE_C_COMPILER "${cvr_msvc_bin}/cl")
set(CMAKE_CXX_COMPILER "${cvr_msvc_bin}/cl")
set(CMAKE_ASM_MASM_COMPILER "${cvr_msvc_bin}/ml64")
set(CMAKE_RC_COMPILER "${cvr_msvc_bin}/rc")
set(CMAKE_MT "${cvr_msvc_bin}/mt")
set(CMAKE_LINKER "${cvr_msvc_bin}/link")
set(CMAKE_AR "${cvr_msvc_bin}/lib")

# Libraries, headers and packages are the target's, never the Linux host's: OpenXR otherwise finds /usr's jsoncpp and
# links a Linux archive into a Windows loader. Programs (cargo, git, wine, dxc) still come from the host.
set(CMAKE_FIND_ROOT_PATH "${CVR_MSVC_WINE_ROOT}")
set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
set(ENV{PKG_CONFIG_PATH} "")
set(ENV{PKG_CONFIG_LIBDIR} "${CVR_MSVC_WINE_ROOT}/pkgconfig-none")
