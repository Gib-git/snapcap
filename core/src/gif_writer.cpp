#include <algorithm>
#include <cstring>
#include <format>
#include <unordered_map>

#include "snapcap/gif.h"

namespace snapcap {
namespace {

constexpr uint8_t kTransparent = 255;

uint64_t centiseconds(Duration d) {
  return static_cast<uint64_t>((std::chrono::duration_cast<std::chrono::milliseconds>(d).count() + 5) / 10);
}

void le16(std::vector<uint8_t>& out, uint32_t v) {
  out.push_back(static_cast<uint8_t>(v));
  out.push_back(static_cast<uint8_t>(v >> 8));
}

// Maps changed pixels to palette indices (unchanged ones to kTransparent). Flat UI regions
// with at most 255 colours get an exact palette; anything richer goes through NeuQuant,
// sampling every pixel for small regions.
std::pair<std::vector<uint8_t>, std::vector<uint8_t>> quantize(const std::vector<uint8_t>& px,
                                                               const std::vector<bool>& keep) {
  const size_t n = keep.size();
  std::vector<uint8_t> palette;
  palette.reserve(256 * 3);
  std::vector<uint8_t> indices(n, kTransparent);
  std::unordered_map<uint32_t, uint8_t> exact;
  bool fits = true;
  for (size_t i = 0; i < n && fits; ++i) {
    if (keep[i]) continue;
    const uint8_t* p = &px[i * 4];
    uint32_t key = uint32_t{p[0]} << 16 | uint32_t{p[1]} << 8 | p[2];
    auto [it, inserted] = exact.try_emplace(key, static_cast<uint8_t>(exact.size()));
    if (inserted) {
      if (exact.size() > 255) {
        fits = false;
        break;
      }
      palette.insert(palette.end(), p, p + 3);
    }
    indices[i] = it->second;
  }
  if (!fits) {
    std::vector<uint8_t> changed;
    changed.reserve(n * 4);
    for (size_t i = 0; i < n; ++i) {
      if (!keep[i]) changed.insert(changed.end(), &px[i * 4], &px[i * 4] + 4);
    }
    int sample = changed.size() / 4 < 50'000 ? 1 : 10;
    NeuQuant nq(changed, sample, 255);
    for (size_t i = 0; i < n; ++i) {
      const uint8_t* p = &px[i * 4];
      indices[i] = keep[i] ? kTransparent : nq.index_of(p[0], p[1], p[2]);
    }
    palette = nq.palette_rgb();
  }
  palette.resize(256 * 3, 0);
  return {std::move(palette), std::move(indices)};
}

}  // namespace

std::optional<std::array<size_t, 4>> changed_rect(std::span<const uint8_t> a, std::span<const uint8_t> b, size_t w,
                                                  size_t h) {
  const size_t row = w * 4;
  size_t top = h, bottom = 0, left = w, right = 0;
  for (size_t y = 0; y < h; ++y) {
    const uint8_t* ra = a.data() + y * row;
    const uint8_t* rb = b.data() + y * row;
    if (std::memcmp(ra, rb, row) == 0) continue;
    top = std::min(top, y);
    bottom = y;
    for (size_t x = 0; x < w; ++x) {
      if (std::memcmp(ra + x * 4, rb + x * 4, 4) != 0) {
        left = std::min(left, x);
        break;
      }
    }
    for (size_t x = w; x-- > 0;) {
      if (std::memcmp(ra + x * 4, rb + x * 4, 4) != 0) {
        right = std::max(right, x);
        break;
      }
    }
  }
  if (top == h) return std::nullopt;
  return std::array<size_t, 4>{left, top, right - left + 1, bottom - top + 1};
}

Result<std::unique_ptr<GifWriter>> GifWriter::create(const std::filesystem::path& path, uint32_t width,
                                                     uint32_t height) {
  if (width == 0 || height == 0 || width > 65535 || height > 65535) return fail("unsupported GIF size");
  std::FILE* f = std::fopen(path.string().c_str(), "wb");
  if (f == nullptr) return fail(std::format("cannot create {}", path.string()));
  std::unique_ptr<GifWriter> w(new GifWriter(f, static_cast<uint16_t>(width), static_cast<uint16_t>(height)));
  std::vector<uint8_t> header{'G', 'I', 'F', '8', '9', 'a'};
  le16(header, width);
  le16(header, height);
  header.insert(header.end(), {0x00, 0x00, 0x00});  // no global colour table
  // NETSCAPE2.0 application extension: loop forever.
  header.insert(header.end(), {0x21, 0xFF, 0x0B, 'N', 'E', 'T', 'S', 'C', 'A', 'P', 'E', '2', '.', '0', 0x03, 0x01,
                               0x00, 0x00, 0x00});
  if (auto r = w->put(header); !r) return std::unexpected(r.error());
  return w;
}

GifWriter::GifWriter(std::FILE* file, uint16_t width, uint16_t height) : file_(file), width_(width), height_(height) {}

GifWriter::~GifWriter() {
  if (file_ != nullptr) std::fclose(file_);
}

Result<void> GifWriter::put(std::span<const uint8_t> bytes) {
  if (std::fwrite(bytes.data(), 1, bytes.size(), file_) != bytes.size()) return fail("cannot write GIF (disk full?)");
  return {};
}

Result<void> GifWriter::push(const VideoFrame& frame) {
  if (finished_) return fail("GIF already finished");
  const size_t w = width_, h = height_;
  RgbaImage rgba = to_rgba(frame.data, frame.width, frame.height, frame.stride, frame.order);
  std::vector<uint8_t> scaled;
  if (frame.width == w && frame.height == h) {
    scaled = std::move(rgba.pixels);
  } else {
    downscale(rgba.pixels, frame.width, frame.height, rgba.stride(), w, h, scaled);
  }
  if (!pending_) {
    written_cs_ = centiseconds(frame.ts);
    pending_.emplace(frame.ts, std::move(scaled));
    return {};
  }
  // Identical content: let the previous frame last longer.
  if (pending_->second == scaled) return {};
  auto prev = std::move(pending_->second);
  pending_.emplace(frame.ts, std::move(scaled));
  return write_frame(std::move(prev), frame.ts);
}

Result<void> GifWriter::write_frame(std::vector<uint8_t> rgba, Duration end) {
  const size_t w = width_, h = height_;
  uint64_t delay = std::max<uint64_t>(2, centiseconds(end) > written_cs_ ? centiseconds(end) - written_cs_ : 0);
  written_cs_ += delay;

  std::array<size_t, 4> rect{0, 0, w, h};
  if (shown_) rect = changed_rect(*shown_, rgba, w, h).value_or(std::array<size_t, 4>{0, 0, 1, 1});
  const auto [x, y, rw, rh] = rect;

  // Gather the sub-rectangle, marking pixels that did not change.
  std::vector<uint8_t> sub(rw * rh * 4);
  std::vector<bool> keep(rw * rh);
  for (size_t row = 0; row < rh; ++row) {
    for (size_t col = 0; col < rw; ++col) {
      size_t o = ((y + row) * w + x + col) * 4;
      size_t s = row * rw + col;
      std::memcpy(&sub[s * 4], &rgba[o], 4);
      keep[s] = shown_ && std::memcmp(&(*shown_)[o], &rgba[o], 4) == 0;
    }
  }
  auto [palette, indices] = quantize(sub, keep);

  std::vector<uint8_t> out;
  out.reserve(palette.size() + indices.size() / 2 + 64);
  // Graphic control extension: disposal "keep", transparent index.
  out.insert(out.end(), {0x21, 0xF9, 0x04, (1 << 2) | 0x01});
  le16(out, static_cast<uint32_t>(std::min<uint64_t>(delay, 65535)));
  out.insert(out.end(), {kTransparent, 0x00});
  // Image descriptor with a 256-entry local colour table.
  out.push_back(0x2C);
  le16(out, static_cast<uint32_t>(x));
  le16(out, static_cast<uint32_t>(y));
  le16(out, static_cast<uint32_t>(rw));
  le16(out, static_cast<uint32_t>(rh));
  out.push_back(0x87);
  out.insert(out.end(), palette.begin(), palette.end());
  out.push_back(8);
  std::vector<uint8_t> lzw = lzw_encode(indices, 8);
  for (size_t off = 0; off < lzw.size(); off += 255) {
    size_t n = std::min<size_t>(255, lzw.size() - off);
    out.push_back(static_cast<uint8_t>(n));
    out.insert(out.end(), lzw.begin() + static_cast<std::ptrdiff_t>(off),
               lzw.begin() + static_cast<std::ptrdiff_t>(off + n));
  }
  out.push_back(0x00);
  if (auto r = put(out); !r) return r;
  shown_ = std::move(rgba);
  ++frames_;
  return {};
}

Result<uint64_t> GifWriter::finish(Duration end) {
  if (finished_) return fail("GIF already finished");
  finished_ = true;
  if (pending_) {
    auto [start, rgba] = std::move(*pending_);
    pending_.reset();
    if (auto r = write_frame(std::move(rgba), std::max(end, start + std::chrono::milliseconds(100))); !r) {
      return std::unexpected(r.error());
    }
  }
  if (frames_ == 0) return fail("no frames were captured");
  const uint8_t trailer = 0x3B;
  if (auto r = put({&trailer, 1}); !r) return std::unexpected(r.error());
  if (std::fflush(file_) != 0) return fail("cannot write GIF (disk full?)");
  std::FILE* f = std::exchange(file_, nullptr);
  if (std::fclose(f) != 0) return fail("cannot write GIF (disk full?)");
  return frames_;
}

}  // namespace snapcap
