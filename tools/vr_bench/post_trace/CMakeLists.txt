cmake_minimum_required(VERSION 3.20)
project(post_trace LANGUAGES CXX)
add_executable(post_trace post_trace.cpp)
target_compile_features(post_trace PRIVATE cxx_std_17)
