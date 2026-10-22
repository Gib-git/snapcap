#include <sstream>
#include <vector>

#include <doctest/doctest.h>

#include "snapcap/actions.h"

using namespace snapcap;

namespace {
std::vector<std::string> args(const std::string& s) {
  std::istringstream in(s);
  std::vector<std::string> out;
  for (std::string w; in >> w;) out.push_back(w);
  return out;
}
}  // namespace

TEST_CASE("parses CLI forms") {
  CHECK(action_from_args(args("--shot region")) == Action::ShotRegion);
  CHECK(action_from_args(args("--record stop")) == Action::StopRecording);
  CHECK(action_from_args(args("--show")) == Action::ShowWindow);
  CHECK(action_from_args(args("shot-all")) == Action::ShotAllScreens);
  CHECK(action_from_args(args("--background --shot screen")) == Action::ShotScreen);
  CHECK(action_from_args(args("--shot nope")) == std::nullopt);
  CHECK(action_from_args(args("--shot")) == std::nullopt);
  CHECK(action_from_args(args("")) == std::nullopt);
}

TEST_CASE("CLI and settings names round-trip") {
  for (Action a : kAllActions) {
    CHECK(from_cli_name(cli_name(a)) == a);
    CHECK(from_settings_key(settings_key(a)) == a);
    CHECK_FALSE(label(a).empty());
  }
}
