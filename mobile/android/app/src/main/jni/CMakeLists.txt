cmake_minimum_required(VERSION 3.22.1)
project(appmodules)
# Keep RN's generated Fabric descriptors and TurboModule registration intact.
include(${REACT_ANDROID_DIR}/cmake-utils/ReactNative-application.cmake)
include(${CMAKE_CURRENT_LIST_DIR}/../cpp/CMakeLists.txt)
