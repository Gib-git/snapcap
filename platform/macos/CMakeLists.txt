# macOS app: AppKit UI and ScreenCaptureKit capture, in Objective-C++ with ARC.

# "-" signs ad hoc. A stable identity (e.g. "Apple Development: …") keeps the Screen
# Recording permission across rebuilds, because macOS ties it to the signature.
set(SNAPCAP_SIGN_IDENTITY "-" CACHE STRING "codesign identity for SnapCap.app")

add_library(snapcap_mac STATIC
  app_delegate.mm
  capture.mm
  hotkeys.mm
  keymap.mm
  mac_host.mm
  mac_util.mm
  main_window.mm
  notice_panel.mm
  recorder.mm
  recording_ui.mm
  selector.mm
  status_item.mm
  widgets.mm
  ../posix/ipc.cpp)
target_include_directories(snapcap_mac PUBLIC ${CMAKE_CURRENT_SOURCE_DIR})
target_compile_options(snapcap_mac PUBLIC -fobjc-arc
  -Wall -Wextra -Wshadow -Wno-unused-parameter $<$<BOOL:${SNAPCAP_WERROR}>:-Werror>)
target_link_libraries(snapcap_mac PUBLIC snapcap_core
  "-framework AppKit"
  "-framework AVFoundation"
  "-framework Carbon"
  "-framework CoreGraphics"
  "-framework CoreImage"
  "-framework CoreMedia"
  "-framework CoreVideo"
  "-framework QuartzCore"
  "-framework QuickLookThumbnailing"
  "-framework ScreenCaptureKit"
  "-framework UniformTypeIdentifiers")

add_executable(snapcap main.mm)
target_link_libraries(snapcap PRIVATE snapcap_mac)

# ---- SnapCap.app: the executable plus Info.plist, icon and licences, signed.
set(SNAPCAP_APP ${CMAKE_BINARY_DIR}/SnapCap.app)
set(SNAPCAP_APP ${SNAPCAP_APP} PARENT_SCOPE)

file(READ ${PROJECT_SOURCE_DIR}/packaging/macos/Info.plist SNAPCAP_PLIST)
string(REPLACE "__VERSION__" "${PROJECT_VERSION}" SNAPCAP_PLIST "${SNAPCAP_PLIST}")
file(WRITE ${CMAKE_CURRENT_BINARY_DIR}/Info.plist "${SNAPCAP_PLIST}")

set(SNAPCAP_ICONSET ${CMAKE_CURRENT_BINARY_DIR}/SnapCap.iconset)
set(SNAPCAP_ICNS ${CMAKE_CURRENT_BINARY_DIR}/SnapCap.icns)
set(SNAPCAP_ICON_COMMANDS)
foreach(size 16 32 128 256 512)
  math(EXPR double "${size} * 2")
  list(APPEND SNAPCAP_ICON_COMMANDS
    COMMAND ${CMAKE_COMMAND} -E copy ${PROJECT_SOURCE_DIR}/assets/icon_${size}.png ${SNAPCAP_ICONSET}/icon_${size}x${size}.png
    COMMAND ${CMAKE_COMMAND} -E copy ${PROJECT_SOURCE_DIR}/assets/icon_${double}.png ${SNAPCAP_ICONSET}/icon_${size}x${size}@2x.png)
endforeach()
add_custom_command(OUTPUT ${SNAPCAP_ICNS}
  COMMAND ${CMAKE_COMMAND} -E make_directory ${SNAPCAP_ICONSET}
  ${SNAPCAP_ICON_COMMANDS}
  COMMAND iconutil -c icns ${SNAPCAP_ICONSET} -o ${SNAPCAP_ICNS}
  DEPENDS ${PROJECT_SOURCE_DIR}/assets/icon_512.png
  COMMENT "Building SnapCap.icns"
  VERBATIM)

add_custom_command(OUTPUT ${SNAPCAP_APP}/Contents/MacOS/snapcap
  COMMAND ${CMAKE_COMMAND} -E rm -rf ${SNAPCAP_APP}
  COMMAND ${CMAKE_COMMAND} -E make_directory ${SNAPCAP_APP}/Contents/MacOS ${SNAPCAP_APP}/Contents/Resources
  COMMAND ${CMAKE_COMMAND} -E copy $<TARGET_FILE:snapcap> ${SNAPCAP_APP}/Contents/MacOS/snapcap
  COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_CURRENT_BINARY_DIR}/Info.plist ${SNAPCAP_APP}/Contents/Info.plist
  COMMAND ${CMAKE_COMMAND} -E copy ${SNAPCAP_ICNS} ${SNAPCAP_APP}/Contents/Resources/SnapCap.icns
  COMMAND ${CMAKE_COMMAND} -E copy ${PROJECT_SOURCE_DIR}/LICENSE ${PROJECT_SOURCE_DIR}/LICENSE-EXCEPTION
          ${SNAPCAP_APP}/Contents/Resources/
  COMMAND plutil -lint -s ${SNAPCAP_APP}/Contents/Info.plist
  COMMAND codesign --force --sign ${SNAPCAP_SIGN_IDENTITY} ${SNAPCAP_APP}
  DEPENDS snapcap ${SNAPCAP_ICNS} ${CMAKE_CURRENT_BINARY_DIR}/Info.plist
  COMMENT "Assembling and signing SnapCap.app (${SNAPCAP_SIGN_IDENTITY})"
  VERBATIM)
add_custom_target(snapcap_app ALL DEPENDS ${SNAPCAP_APP}/Contents/MacOS/snapcap)
