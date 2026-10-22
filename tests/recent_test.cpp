#include <fstream>
#include <thread>

#include <doctest/doctest.h>

#include "snapcap/recent.h"
#include "test_util.h"

using namespace snapcap;
using namespace std::chrono_literals;

TEST_CASE("scan finds media newest first") {
  TempDir dir("recent");
  auto touch = [&](const char* name, std::filesystem::file_time_type t) {
    std::ofstream(dir.path() / name) << "x";
    std::filesystem::last_write_time(dir.path() / name, t);
  };
  auto now = std::filesystem::file_time_type::clock::now();
  touch("a.png", now - 10s);
  touch("b.MP4", now);
  touch("c.gif", now - 5s);
  touch("notes.txt", now + 5s);
  std::filesystem::create_directory(dir.path() / "folder.png");
  auto found = scan_recent(dir.path());
  REQUIRE(found.size() == 3);
  CHECK(found[0].filename() == "b.MP4");
  CHECK(found[1].filename() == "c.gif");
  CHECK(found[2].filename() == "a.png");
  CHECK(scan_recent(dir.path() / "missing").empty());
}

TEST_CASE("scan keeps at most kMaxRecent") {
  TempDir dir("recent-max");
  for (int i = 0; i < 20; ++i) std::ofstream(dir.path() / std::format("{}.png", i)) << "x";
  CHECK(scan_recent(dir.path()).size() == kMaxRecent);
}

TEST_CASE("media kinds") {
  CHECK(media_kind("x.PNG") == MediaKind::Png);
  CHECK(media_kind("x.gif") == MediaKind::Gif);
  CHECK(media_kind("x.mov") == std::nullopt);
  CHECK(label(MediaKind::Mp4) == "MP4");
}
