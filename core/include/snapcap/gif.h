// Animated GIF support: a writer tuned for screen recordings and a small decoder
// (used for thumbnails and tests).
//
// The writer:
// * stores only the rectangle that changed in each frame, with unchanged pixels
//   transparent, which keeps files small when only part of the screen moves;
// * gives every frame its own palette: exact for flat UI content with at most 255
//   colours, NeuQuant otherwise (255 colours + transparency);
// * merges identical consecutive frames, and carries delay rounding forward so the
//   total duration matches the recording.
#pragma once

#include <array>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <memory>
#include <optional>
#include <span>
#include <vector>

#include "snapcap/clock.h"
#include "snapcap/error.h"
#include "snapcap/image.h"

namespace snapcap {

// A captured video frame on its way to an encoder.
struct VideoFrame {
  std::span<const uint8_t> data;
  uint32_t width = 0;
  uint32_t height = 0;
  size_t stride = 0;  // bytes per row; may exceed width * 4
  PixelOrder order = PixelOrder::Bgra;
  Duration ts{0};  // recording-relative presentation time (pauses already removed)
};

// NeuQuant neural-net colour quantizer (Anthony Dekker, 1994) over RGB.
class NeuQuant {
 public:
  // `rgba`: packed RGBA pixels (alpha ignored). `sample_factor` 1 = look at every pixel.
  NeuQuant(std::span<const uint8_t> rgba, int sample_factor, size_t colors);
  uint8_t index_of(uint8_t r, uint8_t g, uint8_t b) const;
  // `colors()` RGB triples.
  std::vector<uint8_t> palette_rgb() const;
  size_t colors() const { return net_size_; }

 private:
  void learn(std::span<const uint8_t> rgba, int sample_factor);
  size_t contest(double r, double g, double b);
  void alter_neighbours(double alpha, int radius, size_t i, double r, double g, double b);
  void build_index();

  struct Neuron {
    double r, g, b;
  };
  struct Entry {
    int r, g, b;
    uint8_t original;
  };
  size_t net_size_;
  std::vector<Neuron> network_;
  std::vector<double> bias_;
  std::vector<double> freq_;
  std::vector<Entry> sorted_;  // colour map sorted by green
  std::vector<size_t> green_index_;
};

// LZW compression as used by GIF (variable code size up to 12 bits).
std::vector<uint8_t> lzw_encode(std::span<const uint8_t> indices, int min_code_size);
// Returns nothing if the data is corrupt.
std::optional<std::vector<uint8_t>> lzw_decode(std::span<const uint8_t> data, int min_code_size, size_t max_out);

class GifWriter {
 public:
  static Result<std::unique_ptr<GifWriter>> create(const std::filesystem::path& path, uint32_t width,
                                                   uint32_t height);
  ~GifWriter();
  GifWriter(const GifWriter&) = delete;
  GifWriter& operator=(const GifWriter&) = delete;

  // Scales the frame to the output size if needed.
  Result<void> push(const VideoFrame& frame);
  // Writes the last frame (lasting until `end`) and the trailer. Returns frames written.
  Result<uint64_t> finish(Duration end);

 private:
  GifWriter(std::FILE* file, uint16_t width, uint16_t height);
  Result<void> write_frame(std::vector<uint8_t> rgba, Duration end);
  Result<void> put(std::span<const uint8_t> bytes);

  std::FILE* file_;
  uint16_t width_;
  uint16_t height_;
  std::optional<std::vector<uint8_t>> shown_;
  std::optional<std::pair<Duration, std::vector<uint8_t>>> pending_;
  uint64_t written_cs_ = 0;
  uint64_t frames_ = 0;
  bool finished_ = false;
};

// Bounding box (x, y, w, h) of pixels that differ between two RGBA images of the same size.
std::optional<std::array<size_t, 4>> changed_rect(std::span<const uint8_t> a, std::span<const uint8_t> b, size_t w,
                                                  size_t h);

struct DecodedGif {
  uint32_t width = 0;
  uint32_t height = 0;
  // The full canvas after each frame.
  std::vector<RgbaImage> frames;
  std::vector<uint16_t> delays_cs;
  std::vector<std::array<uint16_t, 4>> rects;  // left, top, width, height
};

// Decodes up to `max_frames` frames.
Result<DecodedGif> decode_gif(std::span<const uint8_t> bytes, size_t max_frames = SIZE_MAX);

}  // namespace snapcap
