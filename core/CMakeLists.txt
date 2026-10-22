# Portable core: no OS UI or capture APIs, fully unit-tested.
add_library(snapcap_core STATIC
  src/actions.cpp
  src/app_controller.cpp
  src/audio_mix.cpp
  src/clock.cpp
  src/geometry.cpp
  src/gif_decode.cpp
  src/gif_writer.cpp
  src/hotkeys.cpp
  src/icon.cpp
  src/image.cpp
  src/log.cpp
  src/lzw.cpp
  src/neuquant.cpp
  src/output.cpp
  src/paths.cpp
  src/png.cpp
  src/recent.cpp
  src/selection.cpp
  src/settings.cpp)

target_include_directories(snapcap_core PUBLIC include)
target_compile_definitions(snapcap_core PUBLIC SNAPCAP_VERSION="${PROJECT_VERSION}")
target_link_libraries(snapcap_core
  PUBLIC Threads::Threads
  PRIVATE nlohmann_json::nlohmann_json ZLIB::ZLIB)
snapcap_warnings(snapcap_core)
