// PNG encoding. Rows are filtered and deflated in parallel chunks that are joined into
// one valid zlib stream, so a 5K screenshot encodes in well under 100 ms.
#pragma once

#include <cstdint>
#include <filesystem>
#include <vector>

#include "snapcap/error.h"
#include "snapcap/image.h"

namespace snapcap {

// `level` is the zlib compression level (1 = fastest).
Result<std::vector<uint8_t>> encode_png(const RgbaImage& image, int level = 1);
Result<void> write_png(const std::filesystem::path& path, const RgbaImage& image, int level = 1);

}  // namespace snapcap
