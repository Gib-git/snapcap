// Compile-time facts about the target operating system.
#pragma once

namespace snapcap {

enum class Os { MacOS, Windows, Linux };

#if defined(__APPLE__)
inline constexpr Os kOs = Os::MacOS;
#elif defined(_WIN32)
inline constexpr Os kOs = Os::Windows;
#else
inline constexpr Os kOs = Os::Linux;
#endif

// Desktop coordinates are points on macOS and physical pixels on Windows and Linux.
inline constexpr bool kDesktopInPoints = kOs == Os::MacOS;

}  // namespace snapcap
