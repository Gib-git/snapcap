#define DOCTEST_CONFIG_IMPLEMENT
#include <doctest/doctest.h>

#include <cstdlib>

#include "snapcap/log.h"
#include "test_util.h"

int main(int argc, char** argv) {
  // Keep tests away from the user's real settings file.
  TempDir config("config");
  setenv("SNAPCAP_CONFIG_DIR", config.path().c_str(), 1);
  // The app logs a lot on purpose; keep test output readable unless asked for.
  if (std::getenv("SNAPCAP_TEST_LOG") == nullptr) snapcap::log::set_sink([](auto, auto) {});
  doctest::Context context(argc, argv);
  return context.run();
}
