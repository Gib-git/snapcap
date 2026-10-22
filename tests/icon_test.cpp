#include <doctest/doctest.h>

#include "snapcap/icon.h"
#include "test_util.h"

using namespace snapcap;

TEST_CASE("icons have the expected shape") {
  RgbaImage icon = app_icon(64);
  CHECK(icon.pixels.size() == 64 * 64 * 4);
  CHECK(icon.pixel(0, 0)[3] == 0);  // transparent corner
  const uint8_t* mid = icon.pixel(32, 32);
  CHECK(mid[3] == 255);  // opaque coral-ish centre
  CHECK(mid[0] > 200);
  CHECK(mid[2] < 150);

  RgbaImage tray = tray_icon(22, false, true);
  CHECK(tray.pixel(11, 11)[3] > 200);
  CHECK(tray.pixel(11, 11)[0] == 0);
  CHECK(tray_icon(22, true).pixel(11, 11)[0] == 255);

  std::array<uint32_t, 2> sizes{16, 32};
  auto ico = ico_bytes(sizes);
  CHECK(std::vector<uint8_t>(ico.begin(), ico.begin() + 6) == std::vector<uint8_t>{0, 0, 1, 0, 2, 0});
}

TEST_CASE("export writes every icon file") {
  TempDir dir("icons");
  REQUIRE(export_icons(dir.path()));
  for (const char* f : {"icon_16.png", "icon_1024.png", "icon.png", "icon.ico"}) {
    CHECK_MESSAGE(std::filesystem::file_size(dir.path() / f) > 0, f);
  }
}
