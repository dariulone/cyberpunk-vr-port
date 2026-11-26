# cvr_rust: rust/crates/cvr-ffi built by cargo as cvr_rust.lib and linked into C++ targets (stage 1 of
# docs/rust-migration-plan.md). Cargo always runs -- it decides itself what is out of date -- and the library is
# always built with the release profile: one Rust binary, whatever the C++ configuration.
#
#   include(${CVR_REPO_ROOT}/cmake/RustStaticlib.cmake)
#   cvr_link_rust(<target>)    # links cvr_rust.lib, defines CVR_RUST_INPUT=1, adds include/ for Rust/CvrRust.h
if(TARGET cvr_rust)
    return()
endif()

get_filename_component(CVR_REPO_ROOT "${CMAKE_CURRENT_LIST_DIR}/.." ABSOLUTE)
find_program(CVR_CARGO cargo HINTS "$ENV{USERPROFILE}/.cargo/bin" REQUIRED)
set(CVR_RUST_TARGET_DIR "${CMAKE_BINARY_DIR}/cargo")
set(CVR_RUST_LIB "${CVR_RUST_TARGET_DIR}/release/cvr_rust.lib")

# The redscript release whose compiler ships beside the plugin (CMakeLists.txt's CVR_REDSCRIPT_TAG), for the log.
if(DEFINED CVR_REDSCRIPT_TAG)
    string(REGEX REPLACE "^v" "" CVR_RUST_REDSCRIPT_VERSION ${CVR_REDSCRIPT_TAG})
else()
    set(CVR_RUST_REDSCRIPT_VERSION unknown)
endif()
# CVR_RUST_FEATURES (set before including this file): cvr-ffi features; the plugin DLL sets "plugin" (its entry points).
if(CVR_RUST_FEATURES)
    string(REPLACE ";" "," cvr_rust_features "${CVR_RUST_FEATURES}")
    set(CVR_RUST_FEATURE_ARGS --features ${cvr_rust_features})
else()
    set(CVR_RUST_FEATURE_ARGS)
endif()
# The DXC the world markers' shaders are compiled with (cmake/WorldMarkerShaders.cmake), for cvr-render's build.rs.
if(CVR_DXC_EXECUTABLE)
    set(CVR_RUST_DXC_ENV CVR_DXC=${CVR_DXC_EXECUTABLE})
else()
    set(CVR_RUST_DXC_ENV)
endif()
add_custom_target(cvr_rust_build ALL
    COMMAND ${CMAKE_COMMAND} -E env CVR_REDSCRIPT_VERSION=${CVR_RUST_REDSCRIPT_VERSION} ${CVR_RUST_DXC_ENV}
            ${CVR_CARGO} build --release --locked -p cvr-ffi ${CVR_RUST_FEATURE_ARGS}
            --manifest-path "${CVR_REPO_ROOT}/rust/Cargo.toml" --target-dir "${CVR_RUST_TARGET_DIR}"
    BYPRODUCTS ${CVR_RUST_LIB}
    WORKING_DIRECTORY "${CVR_REPO_ROOT}/rust"
    COMMENT "cargo: cvr_rust.lib"
    VERBATIM
)
add_library(cvr_rust STATIC IMPORTED GLOBAL)
set_target_properties(cvr_rust PROPERTIES
    IMPORTED_LOCATION "${CVR_RUST_LIB}"
    # rustc --print native-static-libs for the msvc target; msvcrt is the C++ side's /MD already.
    INTERFACE_LINK_LIBRARIES "kernel32;ntdll;userenv;ws2_32;dbghelp"
    INTERFACE_INCLUDE_DIRECTORIES "${CVR_REPO_ROOT}/include"
    INTERFACE_COMPILE_DEFINITIONS "CVR_RUST_INPUT=1"
)
add_dependencies(cvr_rust cvr_rust_build)

function(cvr_link_rust target)
    target_link_libraries(${target} PRIVATE cvr_rust)
    add_dependencies(${target} cvr_rust_build)
endfunction()
