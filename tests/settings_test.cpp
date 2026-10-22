#include <fstream>

#include <doctest/doctest.h>

#include "snapcap/settings.h"
#include "test_util.h"

using namespace snapcap;

TEST_CASE("settings round-trip through JSON") {
  Settings s;
  s.fps = 60;
  s.save_dir = "/tmp/caps";
  s.mic_device = "USB Mic";
  s.video_format = VideoFormat::Gif;
  s.theme = ThemePref::Dark;
  s.shortcuts[Action::ShotRegion] = "Ctrl+Shift+S";
  CHECK(Settings::from_json(s.to_json()) == s);
}

TEST_CASE("missing fields use defaults") {
  Settings s = Settings::from_json(R"({"fps": 15})");
  CHECK(s.fps == 15);
  CHECK(s.video_format == VideoFormat::Mp4);
  CHECK(s.shortcut(Action::ShotRegion) == "Alt+Shift+2");
}

TEST_CASE("malformed or out-of-range values are tolerated") {
  CHECK(Settings::from_json("not json") == Settings{});
  Settings s = Settings::from_json(R"({"fps": 1000, "gif_fps": 1, "quality": "Ultra", "record_mic": "yes"})");
  CHECK(s.fps == 60);
  CHECK(s.gif_fps == 5);
  CHECK(s.quality == Quality::Medium);
  CHECK_FALSE(s.record_mic);
}

TEST_CASE("reads the file the Rust version wrote") {
  const char* rust = R"({
    "save_dir": null, "video_format": "Gif", "fps": 30, "quality": "High", "resolution": "P1080",
    "gif_fps": 12, "gif_max_width": 800, "record_mic": true, "mic_device": "MacBook Pro Microphone",
    "record_system_audio": true, "countdown_secs": 0, "show_cursor": false, "copy_to_clipboard": false,
    "open_after_capture": true, "theme": "Light",
    "shortcuts": {"ShotScreen": "Alt+Shift+1", "ShotRegion": "Cmd+Shift+7", "ShowWindow": ""},
    "tray_hint_shown": true})";
  Settings s = Settings::from_json(rust);
  CHECK(s.video_format == VideoFormat::Gif);
  CHECK(s.quality == Quality::High);
  CHECK(s.resolution == ResolutionCap::P1080);
  CHECK(s.gif_fps == 12);
  CHECK(s.mic_device == "MacBook Pro Microphone");
  CHECK(s.countdown_secs == 0);
  CHECK_FALSE(s.show_cursor);
  CHECK(s.theme == ThemePref::Light);
  CHECK(s.shortcut(Action::ShotRegion) == "Cmd+Shift+7");
  CHECK(s.shortcut(Action::RecordScreen) == "Alt+Shift+3");  // missing: default
  CHECK(s.tray_hint_shown);
}

TEST_CASE("save writes atomically and load reads it back") {
  TempDir dir("settings");
  Settings s;
  s.countdown_secs = 5;
  auto path = dir.path() / "SnapCap" / "settings.json";
  REQUIRE(s.save_to(path));
  CHECK_FALSE(std::filesystem::exists(path.string() + ".tmp"));
  std::ifstream in(path);
  std::string text((std::istreambuf_iterator<char>(in)), std::istreambuf_iterator<char>());
  CHECK(Settings::from_json(text) == s);
}

TEST_CASE("video bitrate scales with size and is clamped") {
  CHECK(video_bitrate(1920, 1080, 30, Quality::Medium) == 5'598'720);
  CHECK(video_bitrate(1920, 1080, 60, Quality::Medium) == 5'598'720);  // fps counts up to 30
  CHECK(video_bitrate(100, 100, 30, Quality::Low) == 500'000);
  CHECK(video_bitrate(7680, 4320, 30, Quality::High) == 40'000'000);
}
