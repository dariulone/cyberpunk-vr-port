# Patch step for the FidelityFX SDK built with Ninja (cmake/NativeFramegen.cmake): cmake -DSDK=<sdk dir> -P <this>.
#
# compile_shaders_with_depfile (include/FidelityFX/gpu/CMakeCompileShaders.txt) appends its headers to the parameter
# that holds the NAME of the caller's list, so the list it returns starts with that name itself
# ("FRAMEINTERPOLATION_PERMUTATION_OUTPUTS"). The Visual Studio generators let a dependency that is no file go by;
# Ninja stops on it. Each entry that is a real header is a path, so the bare names are dropped. Idempotent.

set(file "${SDK}/src/backends/dx12/CMakeLists.txt")
file(READ "${file}" text)
set(marker "# cvr: drop bare list names")
string(FIND "${text}" "${marker}" found)
if(found EQUAL -1)
    set(anchor "add_custom_target(ffx_shader_permutations_dx12 DEPENDS \${FFX_SC_PERMUTATION_OUTPUTS})")
    string(FIND "${text}" "${anchor}" at)
    if(at EQUAL -1)
        message(FATAL_ERROR "FidelityFX patch: '${anchor}' not found in ${file}")
    endif()
    string(REPLACE "${anchor}"
        "${marker}\nlist(FILTER FFX_SC_PERMUTATION_OUTPUTS INCLUDE REGEX \"/\")\n${anchor}" text "${text}")
    file(WRITE "${file}" "${text}")
endif()

# Built on Linux: the shader compiler is a Windows .exe, checked out without the execute bit. CVR_FFX_SC_LAUNCHER
# (Wine, passed at configure) runs it; empty, the command is the .exe as before.
set(file "${SDK}/CMakeLists.txt")
file(READ "${file}" text)
set(anchor "set(FFX_SC_EXECUTABLE \${CMAKE_CURRENT_SOURCE_DIR}/tools/binary_store/FidelityFX_SC.exe)")
string(FIND "${text}" "${anchor}" at)
if(NOT at EQUAL -1)
    string(REPLACE "${anchor}"
        "set(FFX_SC_EXECUTABLE \${CVR_FFX_SC_LAUNCHER} \${CMAKE_CURRENT_SOURCE_DIR}/tools/binary_store/FidelityFX_SC.exe)"
        text "${text}")
    file(WRITE "${file}" "${text}")
elseif(NOT text MATCHES "CVR_FFX_SC_LAUNCHER")
    message(FATAL_ERROR "FidelityFX patch: '${anchor}' not found in ${file}")
endif()

# Under Wine the compiler resolves no #include against a Linux include directory ("-I /home/..."), only against the
# same directory on Wine's Z: drive, which maps /.
set(file "${SDK}/include/FidelityFX/gpu/CMakeCompileShaders.txt")
file(READ "${file}" text)
set(anchor "list(APPEND COMPILE_INCLUDE_ARGS \"-I\${INCLUDE_SPACE}\${INC}\")")
# The original line stays in the patched text (the else branch), so whether it is there says nothing: the launcher
# test is what marks the file as patched.
if(NOT text MATCHES "if\\(CVR_FFX_SC_LAUNCHER\\)")
    string(FIND "${text}" "${anchor}" at)
    if(at EQUAL -1)
        message(FATAL_ERROR "FidelityFX patch: '${anchor}' not found in ${file}")
    endif()
    string(REPLACE "${anchor}"
        "if(CVR_FFX_SC_LAUNCHER)\n\t\t\tlist(APPEND COMPILE_INCLUDE_ARGS \"-I\${INCLUDE_SPACE}Z:\${INC}\")\n\t\telse()\n\t\t\t${anchor}\n\t\tendif()"
        text "${text}")
    file(WRITE "${file}" "${text}")
endif()

# A Unix shell (Ninja runs each command through /bin/sh) brace-expands the permutation ranges: -DX={0,1} reaches the
# compiler as -DX=0 -DX=1, every permutation key comes out empty and the backend's blob accessors fail to compile.
# cmd leaves them be; CMake quotes no braces, even VERBATIM, so they are escaped.
file(READ "${file}" text)
set(anchor "set(SC_ARGS \${BASE_ARGS} \${API_BASE_ARGS} \${PERMUTATION_ARGS})")
set(escape "if(CMAKE_HOST_UNIX)\n\t\t\tstring(REPLACE \"{\" \"\\\\{\" SC_ARGS \"\${SC_ARGS}\")\n\t\tendif()")
string(FIND "${text}" "${escape}" done)
if(done EQUAL -1)
    string(FIND "${text}" "${anchor}" at)
    if(at EQUAL -1)
        message(FATAL_ERROR "FidelityFX patch: '${anchor}' not found in ${file}")
    endif()
    string(REPLACE "${anchor}" "${anchor}\n\t\t${escape}" text "${text}")
    file(WRITE "${file}" "${text}")
endif()
