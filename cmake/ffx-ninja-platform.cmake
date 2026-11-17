# The FidelityFX SDK built with Ninja (cmake/NativeFramegen.cmake), run after each project() as CMAKE_PROJECT_INCLUDE:
# its libs/pix reads CMAKE_GENERATOR_PLATFORM for the bin/x64 folder, which Ninja refuses to be given up front.
set(CMAKE_GENERATOR_PLATFORM x64)
