cmake_minimum_required(VERSION 3.20)
project(MeleeTrajectories LANGUAGES CXX)
add_executable(melee_trajectory trajectory.cpp)
target_compile_features(melee_trajectory PRIVATE cxx_std_20)
