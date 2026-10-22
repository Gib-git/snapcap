#include "snapcap/png.h"

#include <algorithm>
#include <cstdlib>
#include <format>
#include <fstream>
#include <string_view>

#include <zlib.h>

#include "snapcap/parallel.h"

namespace snapcap {
namespace {

void put_u32(std::vector<uint8_t>& out, uint32_t v) {
  out.push_back(static_cast<uint8_t>(v >> 24));
  out.push_back(static_cast<uint8_t>(v >> 16));
  out.push_back(static_cast<uint8_t>(v >> 8));
  out.push_back(static_cast<uint8_t>(v));
}

void put_chunk(std::vector<uint8_t>& out, std::string_view type, const uint8_t* data, size_t len) {
  put_u32(out, static_cast<uint32_t>(len));
  size_t start = out.size();
  out.insert(out.end(), type.begin(), type.end());
  out.insert(out.end(), data, data + len);
  uLong crc = crc32(0L, Z_NULL, 0);
  crc = crc32_z(crc, out.data() + start, out.size() - start);
  put_u32(out, static_cast<uint32_t>(crc));
}

uint32_t cost(const uint8_t* filtered, size_t n) {
  uint32_t sum = 0;
  for (size_t i = 0; i < n; ++i) sum += static_cast<uint32_t>(std::abs(static_cast<int8_t>(filtered[i])));
  return sum;
}

// Writes the filter byte plus the filtered row: whichever of None/Sub/Up is cheapest.
void filter_row(const uint8_t* row, const uint8_t* prev, size_t bytes, uint8_t* out, std::vector<uint8_t>& sub,
                std::vector<uint8_t>& up) {
  sub.resize(bytes);
  up.resize(bytes);
  for (size_t i = 0; i < bytes; ++i) {
    sub[i] = static_cast<uint8_t>(row[i] - (i >= 4 ? row[i - 4] : 0));
    up[i] = static_cast<uint8_t>(row[i] - (prev ? prev[i] : 0));
  }
  uint32_t c_none = cost(row, bytes), c_sub = cost(sub.data(), bytes), c_up = cost(up.data(), bytes);
  if (c_sub <= c_none && c_sub <= c_up) {
    out[0] = 1;
    std::copy_n(sub.data(), bytes, out + 1);
  } else if (c_up <= c_none) {
    out[0] = 2;
    std::copy_n(up.data(), bytes, out + 1);
  } else {
    out[0] = 0;
    std::copy_n(row, bytes, out + 1);
  }
}

struct Piece {
  std::vector<uint8_t> deflated;
  uLong adler = 1;
  size_t raw_len = 0;
  bool ok = false;
};

// Raw-deflates rows [begin, end). Non-final pieces end with a sync flush so the
// pieces can be concatenated into one deflate stream.
void deflate_rows(const RgbaImage& img, size_t begin, size_t end, bool last, int level, Piece& piece) {
  const size_t bytes = img.stride();
  std::vector<uint8_t> raw((end - begin) * (bytes + 1));
  std::vector<uint8_t> sub, up;
  for (size_t y = begin; y < end; ++y) {
    const uint8_t* row = img.pixels.data() + y * bytes;
    const uint8_t* prev = y > 0 ? row - bytes : nullptr;
    filter_row(row, prev, bytes, raw.data() + (y - begin) * (bytes + 1), sub, up);
  }
  piece.raw_len = raw.size();
  piece.adler = adler32_z(1L, raw.data(), raw.size());

  z_stream zs{};
  if (deflateInit2(&zs, level, Z_DEFLATED, -15, 8, Z_DEFAULT_STRATEGY) != Z_OK) return;
  piece.deflated.resize(deflateBound(&zs, raw.size()) + 16);
  zs.next_in = raw.data();
  zs.avail_in = static_cast<uInt>(raw.size());
  zs.next_out = piece.deflated.data();
  zs.avail_out = static_cast<uInt>(piece.deflated.size());
  int rc = deflate(&zs, last ? Z_FINISH : Z_SYNC_FLUSH);
  piece.ok = last ? rc == Z_STREAM_END : (rc == Z_OK && zs.avail_in == 0);
  piece.deflated.resize(zs.total_out);
  deflateEnd(&zs);
}

}  // namespace

Result<std::vector<uint8_t>> encode_png(const RgbaImage& image, int level) {
  if (image.empty()) return fail("cannot encode an empty image");
  if (image.pixels.size() != size_t{image.width} * image.height * 4) return fail("image buffer has the wrong size");
  // Each piece's raw size must fit zlib's 32-bit counters.
  const size_t row_bytes = image.stride() + 1;
  size_t rows_per_piece = std::max<size_t>(64, (size_t{256} << 20) / row_bytes);
  size_t hw = std::clamp<size_t>(std::thread::hardware_concurrency(), 1, 8);
  rows_per_piece = std::min(rows_per_piece, std::max<size_t>(64, (image.height + hw - 1) / hw));
  size_t pieces_n = (image.height + rows_per_piece - 1) / rows_per_piece;

  std::vector<Piece> pieces(pieces_n);
  parallel_for(pieces_n, 1, [&](size_t first, size_t last) {
    for (size_t i = first; i < last; ++i) {
      size_t begin = i * rows_per_piece;
      size_t end = std::min<size_t>(image.height, begin + rows_per_piece);
      deflate_rows(image, begin, end, i + 1 == pieces_n, level, pieces[i]);
    }
  });

  std::vector<uint8_t> idat{0x78, 0x01};
  uLong adler = 1;
  for (const Piece& p : pieces) {
    if (!p.ok) return fail("png compression failed");
    idat.insert(idat.end(), p.deflated.begin(), p.deflated.end());
    adler = adler32_combine(adler, p.adler, static_cast<z_off_t>(p.raw_len));
  }
  put_u32(idat, static_cast<uint32_t>(adler));

  std::vector<uint8_t> out{0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'};
  out.reserve(idat.size() + 64);
  std::vector<uint8_t> ihdr;
  put_u32(ihdr, image.width);
  put_u32(ihdr, image.height);
  ihdr.insert(ihdr.end(), {8, 6, 0, 0, 0});  // 8-bit RGBA, deflate, adaptive filtering, no interlace
  put_chunk(out, "IHDR", ihdr.data(), ihdr.size());
  // Keep IDAT chunks well below the 2^31 limit.
  constexpr size_t kMaxChunk = size_t{1} << 30;
  for (size_t off = 0; off < idat.size(); off += kMaxChunk) {
    put_chunk(out, "IDAT", idat.data() + off, std::min(kMaxChunk, idat.size() - off));
  }
  put_chunk(out, "IEND", nullptr, 0);
  return out;
}

Result<void> write_png(const std::filesystem::path& path, const RgbaImage& image, int level) {
  auto bytes = encode_png(image, level);
  if (!bytes) return std::unexpected(bytes.error());
  std::ofstream out(path, std::ios::binary | std::ios::trunc);
  if (!out) return fail(std::format("cannot create {}", path.string()));
  out.write(reinterpret_cast<const char*>(bytes->data()), static_cast<std::streamsize>(bytes->size()));
  out.flush();
  if (!out) return fail(std::format("cannot write {}", path.string()));
  return {};
}

}  // namespace snapcap
