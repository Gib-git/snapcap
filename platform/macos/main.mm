// SnapCap — screenshots and screen recording.
#import <AppKit/AppKit.h>

#include <os/log.h>
#include <unistd.h>

#include <algorithm>
#include <cstdio>
#include <string>
#include <vector>

#include "../posix/ipc.h"
#import "app_delegate.h"
#include "snapcap/actions.h"
#include "snapcap/icon.h"
#include "snapcap/log.h"
#include "snapcap/paths.h"

using namespace snapcap;

namespace {

constexpr const char* kUsage = R"(SnapCap — screenshots and screen recording

Usage: snapcap [OPTION]

  --shot screen|all|region     Take a screenshot
  --record screen|region|stop  Start or stop a recording
  --pause                      Pause or resume the current recording
  --show                       Open the SnapCap window
  --background                 Start without opening the window
  --export-icons <dir>         Write the app icons (used by packaging scripts)
  --help, --version

If SnapCap is already running, the action is sent to that instance.)";

void install_log_sink() {
  static os_log_t log = os_log_create("dev.snapcap.SnapCap", "app");
  const bool tty = isatty(STDERR_FILENO) != 0;
  log::set_sink([tty](log::Level level, std::string_view message) {
    os_log_type_t type = level == log::Level::Error  ? OS_LOG_TYPE_ERROR
                         : level == log::Level::Warn ? OS_LOG_TYPE_DEFAULT
                                                     : OS_LOG_TYPE_INFO;
    std::string text(message);
    os_log_with_type(log, type, "%{public}s", text.c_str());
    if (tty) std::fprintf(stderr, "snapcap: %s\n", text.c_str());
  });
  if (auto home = paths::home_dir()) log::open_file(*home / "Library" / "Logs" / "SnapCap" / "snapcap.log");
}

// Drops arguments macOS or Xcode add on their own (-psn_0_123, -NSDocumentRevisionsDebugMode YES).
std::vector<std::string> user_args(int argc, char** argv) {
  std::vector<std::string> out;
  for (int i = 1; i < argc; ++i) {
    std::string a = argv[i];
    if (a.starts_with("-psn_")) continue;
    if (a.starts_with("-NS") || a.starts_with("-Apple")) {
      ++i;
      continue;
    }
    out.push_back(std::move(a));
  }
  return out;
}

}  // namespace

int main(int argc, char** argv) {
  @autoreleasepool {
    std::vector<std::string> args = user_args(argc, argv);
    if (!args.empty() && (args[0] == "--help" || args[0] == "-h")) {
      std::puts(kUsage);
      return 0;
    }
    if (!args.empty() && (args[0] == "--version" || args[0] == "-V")) {
      std::printf("snapcap %s\n", SNAPCAP_VERSION);
      return 0;
    }
    if (!args.empty() && args[0] == "--export-icons") {
      auto r = export_icons(args.size() > 1 ? args[1] : "assets");
      if (!r) {
        std::fprintf(stderr, "could not export icons: %s\n", r.error().message.c_str());
        return 1;
      }
      return 0;
    }

    auto action = action_from_args(args);
    bool background = std::ranges::find(args, "--background") != args.end();
    if (!args.empty() && !action && !background) {
      std::string joined;
      for (const auto& a : args) joined += (joined.empty() ? "" : " ") + a;
      std::fprintf(stderr, "unrecognised arguments: %s\n\n%s\n", joined.c_str(), kUsage);
      return 2;
    }
    install_log_sink();

    // Single instance: hand the request to the running copy.
    if (ipc::send_to_running(action.value_or(Action::ShowWindow))) return 0;

    NSApplication* app = NSApplication.sharedApplication;
    SNCAppDelegate* delegate = [[SNCAppDelegate alloc] initWithStartupAction:action background:background];
    app.delegate = delegate;
    [app run];
  }
  return 0;
}
