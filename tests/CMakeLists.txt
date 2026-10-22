add_executable(snapcap_core_tests
  main.cpp
  actions_test.cpp
  app_controller_test.cpp
  audio_mix_test.cpp
  clock_test.cpp
  geometry_test.cpp
  gif_test.cpp
  hotkeys_test.cpp
  icon_test.cpp
  image_test.cpp
  output_test.cpp
  png_test.cpp
  recent_test.cpp
  selection_test.cpp
  settings_test.cpp)
target_link_libraries(snapcap_core_tests PRIVATE snapcap_core doctest ZLIB::ZLIB)
snapcap_warnings(snapcap_core_tests)
add_test(NAME core COMMAND snapcap_core_tests)

if(APPLE)
  add_executable(snapcap_mac_tests macos/mac_tests.mm)
  target_link_libraries(snapcap_mac_tests PRIVATE snapcap_mac doctest "-framework ImageIO")
  add_test(NAME macos COMMAND snapcap_mac_tests)
endif()
