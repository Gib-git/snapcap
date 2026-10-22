cmake_minimum_required(VERSION 3.24)

# Must be set before project() to take effect.
# 13.3: the first release whose C++ library supports std::format with floating point.
set(CMAKE_OSX_DEPLOYMENT_TARGET "13.3" CACHE STRING "Minimum macOS version")

project(SnapCap
  VERSION 0.2.0
  DESCRIPTION "Cross-platform screenshot and screen recorder"
  LANGUAGES C CXX)

if(APPLE)
  enable_language(OBJCXX)
endif()

set(CMAKE_CXX_STANDARD 23)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
set(CMAKE_OBJCXX_STANDARD 23)
set(CMAKE_OBJCXX_STANDARD_REQUIRED ON)
set(CMAKE_OBJCXX_EXTENSIONS OFF)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

option(SNAPCAP_WERROR "Treat warnings as errors" ON)
option(SNAPCAP_TESTS "Build the test suite" ON)
set(SNAPCAP_SANITIZE "" CACHE STRING "Comma-separated sanitizers, e.g. address,undefined or thread")

include(cmake/Warnings.cmake)
include(cmake/Sanitizers.cmake)
include(cmake/Dependencies.cmake)

add_subdirectory(core)

if(SNAPCAP_TESTS)
  enable_testing()
  add_subdirectory(tests)
endif()
