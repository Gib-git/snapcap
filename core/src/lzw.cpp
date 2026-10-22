// GIF-flavoured LZW: LSB-first bit packing, clear/end codes, codes grow to 12 bits.
// The encoder widens its codes one code after its table reaches 2^size, matching the
// decoder, which builds its table one step behind (the convention giflib follows).
#include <array>

#include "snapcap/gif.h"

namespace snapcap {
namespace {

constexpr int kMaxCode = 4095;

class BitWriter {
 public:
  void put(uint32_t code, int bits) {
    acc_ |= uint64_t{code} << n_;
    n_ += bits;
    while (n_ >= 8) {
      out.push_back(static_cast<uint8_t>(acc_));
      acc_ >>= 8;
      n_ -= 8;
    }
  }
  void flush() {
    if (n_ > 0) out.push_back(static_cast<uint8_t>(acc_));
    acc_ = 0;
    n_ = 0;
  }
  std::vector<uint8_t> out;

 private:
  uint64_t acc_ = 0;
  int n_ = 0;
};

// Open-addressing table from (prefix code, next index) to code.
class CodeTable {
 public:
  static constexpr size_t kSize = 5003;  // prime, ~80% load at 4096 codes
  void clear() { keys_.fill(-1); }
  int find(int32_t key) const {
    for (size_t h = slot(key);; h = (h + 1) % kSize) {
      if (keys_[h] == key) return codes_[h];
      if (keys_[h] < 0) return -1;
    }
  }
  void insert(int32_t key, int code) {
    size_t h = slot(key);
    while (keys_[h] >= 0) h = (h + 1) % kSize;
    keys_[h] = key;
    codes_[h] = static_cast<int16_t>(code);
  }

 private:
  static size_t slot(int32_t key) { return (static_cast<size_t>(key) * 2654435761u) % kSize; }
  std::array<int32_t, kSize> keys_{};
  std::array<int16_t, kSize> codes_{};
};

}  // namespace

std::vector<uint8_t> lzw_encode(std::span<const uint8_t> indices, int min_code_size) {
  const int clear_code = 1 << min_code_size;
  const int end_code = clear_code + 1;
  int size = min_code_size + 1;
  int next = clear_code + 2;
  auto table = std::make_unique<CodeTable>();
  table->clear();
  BitWriter bw;

  auto emit = [&](int code) {
    bw.put(static_cast<uint32_t>(code), size);
    if (next >= (1 << size) && size < 12 && code != clear_code) ++size;
  };

  bw.put(static_cast<uint32_t>(clear_code), size);
  if (indices.empty()) {
    bw.put(static_cast<uint32_t>(end_code), size);
    bw.flush();
    return std::move(bw.out);
  }
  int prefix = indices[0];
  for (size_t i = 1; i < indices.size(); ++i) {
    int k = indices[i];
    int32_t key = (prefix << 8) | k;
    if (int code = table->find(key); code >= 0) {
      prefix = code;
      continue;
    }
    emit(prefix);
    prefix = k;
    if (next >= kMaxCode) {
      bw.put(static_cast<uint32_t>(clear_code), size);
      table->clear();
      next = clear_code + 2;
      size = min_code_size + 1;
    } else {
      table->insert(key, next++);
    }
  }
  emit(prefix);
  bw.put(static_cast<uint32_t>(end_code), size);
  bw.flush();
  return std::move(bw.out);
}

std::optional<std::vector<uint8_t>> lzw_decode(std::span<const uint8_t> data, int min_code_size, size_t max_out) {
  if (min_code_size < 2 || min_code_size > 11) return std::nullopt;
  const int clear_code = 1 << min_code_size;
  const int end_code = clear_code + 1;
  std::array<uint16_t, 4096> prefix{};
  std::array<uint8_t, 4096> suffix{};
  std::array<uint8_t, 4096> first{};
  for (int i = 0; i < clear_code; ++i) {
    suffix[static_cast<size_t>(i)] = static_cast<uint8_t>(i);
    first[static_cast<size_t>(i)] = static_cast<uint8_t>(i);
  }
  int size = min_code_size + 1;
  int next = clear_code + 2;
  int prev = -1;
  std::vector<uint8_t> out;
  std::vector<uint8_t> stack;
  uint64_t acc = 0;
  int bits = 0;
  size_t pos = 0;

  auto expand = [&](int code) {
    stack.clear();
    while (code >= clear_code) {
      stack.push_back(suffix[static_cast<size_t>(code)]);
      code = prefix[static_cast<size_t>(code)];
    }
    stack.push_back(static_cast<uint8_t>(code));
    out.insert(out.end(), stack.rbegin(), stack.rend());
  };

  while (out.size() < max_out) {
    while (bits < size && pos < data.size()) {
      acc |= uint64_t{data[pos++]} << bits;
      bits += 8;
    }
    if (bits < size) break;  // ran out of data without an end code; keep what we have
    int code = static_cast<int>(acc & ((1u << size) - 1));
    acc >>= size;
    bits -= size;

    if (code == clear_code) {
      size = min_code_size + 1;
      next = clear_code + 2;
      prev = -1;
      continue;
    }
    if (code == end_code) break;
    if (prev < 0) {
      if (code >= clear_code) return std::nullopt;
      out.push_back(static_cast<uint8_t>(code));
      prev = code;
      continue;
    }
    uint8_t head;
    if (code < next) {
      head = first[static_cast<size_t>(code)];
      expand(code);
    } else if (code == next) {
      head = first[static_cast<size_t>(prev)];
      expand(prev);
      out.push_back(head);
    } else {
      return std::nullopt;
    }
    if (next < 4096) {
      prefix[static_cast<size_t>(next)] = static_cast<uint16_t>(prev);
      suffix[static_cast<size_t>(next)] = head;
      first[static_cast<size_t>(next)] = first[static_cast<size_t>(prev)];
      ++next;
      if (next == (1 << size) && size < 12) ++size;
    }
    prev = code;
  }
  if (out.size() > max_out) out.resize(max_out);
  return out;
}

}  // namespace snapcap
