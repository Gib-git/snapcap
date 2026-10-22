#include <fstream>

#include <doctest/doctest.h>

#include "snapcap/output.h"
#include "test_util.h"

using namespace snapcap;

TEST_CASE("file names are filesystem safe") {
  std::tm t{};
  t.tm_year = 2026 - 1900;
  t.tm_mon = 9;
  t.tm_mday = 7;
  t.tm_hour = 18;
  t.tm_min = 56;
  t.tm_sec = 12;
  CHECK(capture_file_name(CaptureKind::Screenshot, "png", t) == "Screenshot 2026-10-07 at 18.56.12.png");
  CHECK(capture_file_name(CaptureKind::Recording, "mp4", t) == "Recording 2026-10-07 at 18.56.12.mp4");
}

TEST_CASE("creates the folder and avoids collisions") {
  TempDir tmp("output");
  auto dir = tmp.path() / "nested" / "caps";
  auto a = new_capture_path(dir, CaptureKind::Screenshot, "png");
  REQUIRE(a.has_value());
  CHECK(std::filesystem::is_directory(dir));
  std::ofstream(*a) << "x";
  auto b = unique_path(*a);
  CHECK(b != *a);
  CHECK(b.string().ends_with("(2).png"));
  std::ofstream(b) << "x";
  CHECK(unique_path(*a).string().ends_with("(3).png"));
}

TEST_CASE("default folder is Desktop/screenshots") {
  auto d = default_save_dir();
  CHECK(d.filename() == "screenshots");
  Settings s;
  CHECK(save_dir(s) == d);
  s.save_dir = "/somewhere/else";
  CHECK(save_dir(s) == std::filesystem::path("/somewhere/else"));
}
