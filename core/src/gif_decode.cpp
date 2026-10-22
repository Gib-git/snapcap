#include <algorithm>
#include <cstring>

#include "snapcap/gif.h"

namespace snapcap {
namespace {

class Reader {
 public:
  explicit Reader(std::span<const uint8_t> b) : b_(b) {}
  bool has(size_t n) const { return pos_ + n <= b_.size(); }
  uint8_t u8() { return has(1) ? b_[pos_++] : 0; }
  uint16_t u16() {
    uint16_t lo = u8();
    return static_cast<uint16_t>(lo | (u8() << 8));
  }
  std::span<const uint8_t> take(size_t n) {
    if (!has(n)) {
      pos_ = b_.size();
      return {};
    }
    auto s = b_.subspan(pos_, n);
    pos_ += n;
    return s;
  }
  // Concatenated data sub-blocks up to the zero terminator.
  std::vector<uint8_t> blocks() {
    std::vector<uint8_t> out;
    while (has(1)) {
      uint8_t n = u8();
      if (n == 0) break;
      auto s = take(n);
      out.insert(out.end(), s.begin(), s.end());
    }
    return out;
  }
  bool done() const { return pos_ >= b_.size(); }

 private:
  std::span<const uint8_t> b_;
  size_t pos_ = 0;
};

}  // namespace

Result<DecodedGif> decode_gif(std::span<const uint8_t> bytes, size_t max_frames) {
  Reader r(bytes);
  auto sig = r.take(6);
  if (sig.size() != 6 || std::memcmp(sig.data(), "GIF8", 4) != 0) return fail("not a GIF file");
  DecodedGif gif;
  gif.width = r.u16();
  gif.height = r.u16();
  uint8_t flags = r.u8();
  r.u8();  // background colour index
  r.u8();  // pixel aspect ratio
  if (gif.width == 0 || gif.height == 0) return fail("GIF has no size");
  std::vector<uint8_t> global;
  if (flags & 0x80) {
    auto t = r.take(size_t{3} << ((flags & 7) + 1));
    global.assign(t.begin(), t.end());
  }
  RgbaImage canvas(gif.width, gif.height);
  int transparent = -1;
  uint16_t delay = 0;
  int disposal = 0;

  while (!r.done() && gif.frames.size() < max_frames) {
    uint8_t block = r.u8();
    if (block == 0x3B) break;
    if (block == 0x21) {
      uint8_t label = r.u8();
      std::vector<uint8_t> data = r.blocks();
      if (label == 0xF9 && data.size() >= 4) {
        disposal = (data[0] >> 2) & 7;
        delay = static_cast<uint16_t>(data[1] | (data[2] << 8));
        transparent = (data[0] & 1) ? data[3] : -1;
      }
      continue;
    }
    if (block != 0x2C) return fail("corrupt GIF");
    uint16_t left = r.u16(), top = r.u16(), fw = r.u16(), fh = r.u16();
    uint8_t iflags = r.u8();
    std::vector<uint8_t> palette = global;
    if (iflags & 0x80) {
      auto t = r.take(size_t{3} << ((iflags & 7) + 1));
      palette.assign(t.begin(), t.end());
    }
    int min_code = r.u8();
    std::vector<uint8_t> data = r.blocks();
    size_t count = size_t{fw} * fh;
    auto indices = lzw_decode(data, min_code, count);
    if (!indices) return fail("corrupt GIF image data");
    indices->resize(count, 0);

    RgbaImage before = disposal == 3 ? canvas : RgbaImage{};
    const bool interlaced = (iflags & 0x40) != 0;
    std::vector<uint32_t> rows(fh);
    if (interlaced) {
      size_t i = 0;
      for (auto [start, step] : {std::pair{0u, 8u}, {4u, 8u}, {2u, 4u}, {1u, 2u}}) {
        for (uint32_t y = start; y < fh; y += step) rows[i++] = y;
      }
    } else {
      for (uint32_t y = 0; y < fh; ++y) rows[y] = y;
    }
    for (uint32_t i = 0; i < fh; ++i) {
      uint32_t y = top + rows[i];
      if (y >= gif.height) continue;
      for (uint32_t x = 0; x < fw; ++x) {
        uint32_t cx = left + x;
        if (cx >= gif.width) continue;
        uint8_t idx = (*indices)[size_t{i} * fw + x];
        if (idx == transparent || size_t{idx} * 3 + 2 >= palette.size()) continue;
        uint8_t* p = canvas.pixel(cx, y);
        p[0] = palette[size_t{idx} * 3];
        p[1] = palette[size_t{idx} * 3 + 1];
        p[2] = palette[size_t{idx} * 3 + 2];
        p[3] = 255;
      }
    }
    gif.frames.push_back(canvas);
    gif.delays_cs.push_back(delay);
    gif.rects.push_back({left, top, fw, fh});

    if (disposal == 2) {  // restore to background (transparent)
      for (uint32_t y = top; y < std::min<uint32_t>(gif.height, top + fh); ++y) {
        for (uint32_t x = left; x < std::min<uint32_t>(gif.width, left + fw); ++x) std::memset(canvas.pixel(x, y), 0, 4);
      }
    } else if (disposal == 3) {
      canvas = std::move(before);
    }
    transparent = -1;
    delay = 0;
    disposal = 0;
  }
  if (gif.frames.empty()) return fail("GIF has no frames");
  return gif;
}

}  // namespace snapcap
